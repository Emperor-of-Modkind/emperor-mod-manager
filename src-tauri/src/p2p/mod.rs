//! Serverless peer-to-peer social system powered by `iroh`.
//!
//! Uses direct QUIC connections first (via socket addresses encoded in join
//! codes), with UDP holepunching and community relay fallback when NATs
//! require it. Collections are exchanged as live-updating Emperor Share Codes
//! (`#emperor1...`) to keep wire payloads tiny.

use std::{
    collections::{BTreeSet, HashMap, HashSet, VecDeque},
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use iroh::{
    endpoint::{presets, Incoming},
    Endpoint, EndpointAddr, EndpointId, SecretKey, TransportAddr,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::{OnceCell, Semaphore};

use crate::{
    config::{load_config, Paths},
    mods,
    share::{self, ShareManifest},
};

pub const P2P_ALPN: &[u8] = b"emperor-p2p/1";
pub const JOIN_CODE_PREFIX: &str = "emperor-friend1:";
const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
const STREAM_IO_TIMEOUT: Duration = Duration::from_secs(10);
const MAX_CONCURRENT_INCOMING: usize = 32;
const MAX_CONCURRENT_PER_PEER: usize = 3;
const MAX_STREAMS_PER_CONN: usize = 8;
const MAX_PENDING_REQUESTS: usize = 50;
const MAX_JOIN_CODE_LEN: usize = 16 * 1024;
const MAX_JOIN_TICKET_BYTES: usize = 64 * 1024;
const MAX_DISPLAY_NAME_CHARS: usize = 64;
const MAX_REMOTE_COLLECTIONS: usize = 128;
const MAX_REMOTE_FRIENDS: usize = 256;
const MAX_FRIEND_CACHED_SHARE_BYTES: usize = 4 * 1024 * 1024;
const MAX_COLLECTION_ID_CHARS: usize = 128;
const MAX_COLLECTION_NAME_CHARS: usize = 128;
const MAX_SHARE_HASH_CHARS: usize = 128;
const MAX_PEER_RELAY_ADDRS: usize = 2;
const MAX_PEER_IP_ADDRS: usize = 8;
const MAX_FEED_EVENTS: usize = 100;
const MAX_FEED_DIFF_NAMES: usize = 5;
const LOCAL_ACTIVITY_COALESCE_WINDOW_SECS: i64 = 120;
const FEED_DEDUPE_WINDOW_SECS: i64 = 300;
const MAX_CLOCK_SKEW_FUTURE_SECS: i64 = 300;
const HELLO_PER_PEER_COOLDOWN: Duration = Duration::from_secs(5);
const HELLO_GLOBAL_WINDOW: Duration = Duration::from_secs(60);
const HELLO_GLOBAL_MAX_PER_WINDOW: usize = 20;
const PROFILE_RATE_BURST: f64 = 8.0;
const PROFILE_RATE_REFILL_PER_SEC: f64 = 0.5;
const RELAY_PROFILE_DOMAIN_PREFIX: &[u8] = b"emperor-relay-profile-v1\0";
const RELAY_INVITE_DOMAIN_PREFIX: &[u8] = b"emperor-relay-invite-v1\0";
const MAX_QUEUED_INVITES: usize = 50;
const MAX_RELAYED_INVITES_STORED: usize = 64;
const MAX_RELAYED_INVITES_PER_SENDER: usize = 8;
const MAX_RELAYED_INVITES_PER_TARGET: usize = 16;
const RELAYED_INVITE_TTL_SECS: i64 = 14 * 24 * 60 * 60;
const RELAYED_INVITE_MAX_CLOCK_SKEW_SECS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum FriendCollectionKind {
    #[default]
    ActiveLoadout,
    SavedCollection,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FriendCollectionMatchKind {
    /// Friend is syncing one of your collections and their active loadout uses it.
    SyncedToYours,
    /// Friend's active loadout matches the mod set of one of your saved collections.
    MatchesSaved,
    /// Friend's active loadout matches the mod set of one of your installed collections.
    MatchesInstalled,
    /// Friend's active loadout matches the mod set of your own active loadout.
    MatchesActiveLoadout,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendMatchedLocalCollection {
    pub id: String,
    pub name: String,
    pub kind: FriendCollectionMatchKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_id: Option<String>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub has_local_changes: bool,
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FriendShareRef {
    pub id: String,
    pub kind: FriendCollectionKind,
    pub name: String,
    /// Compact `#emperor1...` share code. May be empty in delta responses when
    /// `share_hash` matches the caller's cached hash.
    pub share_code: String,
    pub share_hash: String,
    pub updated_at: String,
    /// True when the sender shared this collection only via `SpecificFriends`
    /// (not `AllFriends`), so local caches preserve it across `AllFriends`-only
    /// mutual-friend relays.
    #[serde(default, skip_serializing_if = "is_false")]
    pub specific_friends_only: bool,
    /// Set by the sender on `ActiveLoadout` when their active loadout corresponds
    /// to an installed collection synced from the requesting `viewer_peer_id`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synced_source_collection_id: Option<String>,
    /// Viewer-side resolution populated by `get_state()` when a friend's active
    /// loadout matches or is synced to one of the viewer's local collections.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub matched_local_collection: Option<FriendMatchedLocalCollection>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedFriendRef {
    pub peer_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<EndpointAddr>,
    /// `updated_at` of this friend's `SignedRelayProfile` cached on the sender,
    /// allowing mutual friends to detect newer offline caches.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_updated_at: Option<String>,
    /// Whether the sharing friend currently observes this peer as online.
    /// `None` when omitted by older peers or in offline relay snapshots.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub online: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct QueuedFriendInvite {
    pub peer_id: String,
    pub display_name: String,
    pub addr: EndpointAddr,
    pub queued_at: String,
    /// Mutual friend peer IDs that have accepted a signed relay copy of this
    /// invite to deliver when the target comes online.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relayed_via_peer_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FriendInvitePayload {
    pub from_peer_id: String,
    pub to_peer_id: String,
    pub from_display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_profile_picture: Option<String>,
    /// Relay-only address for the sender (same privacy policy as `shared_friend_addr`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_addr: Option<EndpointAddr>,
    pub created_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedFriendInvite {
    pub payload: FriendInvitePayload,
    /// Hex-encoded Ed25519 signature by `from_peer_id` over
    /// `RELAY_INVITE_DOMAIN_PREFIX || canonical JSON(payload)`.
    pub signature: String,
}

fn default_allow_mutual_connections() -> bool {
    true
}

fn default_allow_cache_relay() -> bool {
    true
}

fn default_share_active_loadouts_in_feed() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RelayProfilePayload {
    pub peer_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<String>,
    pub updated_at: String,
    pub collections: Vec<FriendShareRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friends: Option<Vec<SharedFriendRef>>,
    #[serde(default = "default_allow_mutual_connections")]
    pub allow_mutual_connections: bool,
    /// Author-recorded `AllFriends`-only activity events, signed together with
    /// the relay payload so offline mutual-friend relays deliver real history.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_events: Option<Vec<LocalActivityEvent>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedRelayProfile {
    pub payload: RelayProfilePayload,
    /// Hex-encoded Ed25519 signature over canonical JSON of `payload`.
    pub signature: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerProfileSummary {
    pub peer_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<String>,
    pub updated_at: String,
    pub collections: Vec<FriendShareRef>,
    /// Connected friends shared by this peer, or `None` if their friends list
    /// is private to the viewer (or the peer runs an older version).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friends: Option<Vec<SharedFriendRef>>,
    /// Whether this peer allows their friends to include them in shared
    /// friends lists (visible to friends-of-friends / mutual connections).
    #[serde(default = "default_allow_mutual_connections")]
    pub allow_mutual_connections: bool,
    /// Signed `AllFriends`-only snapshot that mutual friends may relay when
    /// this peer is offline.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signed_relay_profile: Option<SignedRelayProfile>,
    /// Author-recorded activity events visible to the viewer. `None` indicates
    /// a legacy peer that does not record its own activity (fall back to
    /// receiver-side snapshot diffing).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub activity_events: Option<Vec<LocalActivityEvent>>,
    /// Signed friend invites from mutual friends held while the viewer was
    /// offline. Populated only in live `GetProfile` responses for the intended
    /// `viewer_peer_id`, never in `SignedRelayProfile`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relayed_invites: Vec<SignedFriendInvite>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FriendStatus {
    Connected,
    PendingOutgoing,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendRecord {
    pub peer_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<String>,
    pub addr: EndpointAddr,
    pub status: FriendStatus,
    #[serde(default)]
    pub online: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_at: Option<String>,
    /// RFC3339 timestamp when this friend was added locally. Used as a cutoff
    /// so we only sync activity events from when they were added onwards.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_profile: Option<PeerProfileSummary>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub syncing_my_collections: Vec<String>,
    /// Display name of the mutual friend from whom the current cached profile
    /// was relayed while this friend was offline (cleared on direct sync).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_via_peer_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingFriendRequest {
    pub peer_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<String>,
    pub addr: EndpointAddr,
    pub received_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum CollectionVisibility {
    AllFriends,
    SpecificFriends { peer_ids: Vec<String> },
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FriendFeedEventKind {
    CollectionAdded,
    CollectionUpdated,
    CollectionRenamed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct FriendFeedDiff {
    #[serde(default)]
    pub mods_added: Vec<String>,
    #[serde(default)]
    pub mods_added_count: usize,
    #[serde(default)]
    pub mods_removed: Vec<String>,
    #[serde(default)]
    pub mods_removed_count: usize,
    #[serde(default)]
    pub mods_updated: Vec<String>,
    #[serde(default)]
    pub mods_updated_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalActivityEvent {
    pub id: String,
    pub kind: FriendFeedEventKind,
    pub collection_id: String,
    pub collection_kind: FriendCollectionKind,
    pub collection_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_count: Option<usize>,
    pub share_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<FriendFeedDiff>,
    pub occurred_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalCollectionSnapshot {
    pub id: String,
    pub kind: FriendCollectionKind,
    pub name: String,
    pub share_hash: String,
    pub share_code: String,
    #[serde(default)]
    pub shared: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coalesce_base_code: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coalesce_event_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendFeedEvent {
    pub id: String,
    pub peer_id: String,
    pub friend_display_name: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_self: bool,
    pub kind: FriendFeedEventKind,
    pub collection_id: String,
    pub collection_kind: FriendCollectionKind,
    pub collection_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_count: Option<usize>,
    pub share_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub diff: Option<FriendFeedDiff>,
    pub occurred_at: String,
}

fn default_friends_list_visibility() -> CollectionVisibility {
    CollectionVisibility::AllFriends
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendsStore {
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub display_name_set: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<String>,
    #[serde(default)]
    pub friends: Vec<FriendRecord>,
    #[serde(default)]
    pub pending_requests: Vec<PendingFriendRequest>,
    #[serde(default)]
    pub hidden_collection_ids: HashSet<String>,
    #[serde(default)]
    pub collection_visibility: HashMap<String, CollectionVisibility>,
    #[serde(default = "default_friends_list_visibility")]
    pub friends_list_visibility: CollectionVisibility,
    #[serde(default = "default_allow_mutual_connections")]
    pub allow_mutual_connections: bool,
    #[serde(default = "default_allow_cache_relay")]
    pub allow_cache_relay: bool,
    #[serde(default = "default_share_active_loadouts_in_feed")]
    pub share_active_loadouts_in_feed: bool,
    #[serde(default)]
    pub appear_offline: bool,
    #[serde(default)]
    pub feed_events: Vec<FriendFeedEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_last_read_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_cleared_at: Option<String>,
    #[serde(default)]
    pub local_activity: Vec<LocalActivityEvent>,
    #[serde(default)]
    pub local_collection_snapshots: HashMap<String, LocalCollectionSnapshot>,
    #[serde(default)]
    pub local_activity_initialized: bool,
    /// Outgoing friend-of-friend invites queued until the target peer is online.
    #[serde(default)]
    pub queued_invites: Vec<QueuedFriendInvite>,
    /// Signed friend invites deposited by connected friends for other connected
    /// friends who were offline, held until the target comes online.
    #[serde(default)]
    pub relayed_invites: Vec<SignedFriendInvite>,
}

impl Default for FriendsStore {
    fn default() -> Self {
        Self {
            display_name: String::new(),
            display_name_set: false,
            profile_picture: None,
            friends: Vec::new(),
            pending_requests: Vec::new(),
            hidden_collection_ids: HashSet::new(),
            collection_visibility: HashMap::new(),
            friends_list_visibility: default_friends_list_visibility(),
            allow_mutual_connections: default_allow_mutual_connections(),
            allow_cache_relay: default_allow_cache_relay(),
            share_active_loadouts_in_feed: default_share_active_loadouts_in_feed(),
            appear_offline: false,
            feed_events: Vec::new(),
            feed_last_read_at: None,
            feed_cleared_at: None,
            local_activity: Vec::new(),
            local_collection_snapshots: HashMap::new(),
            local_activity_initialized: false,
            queued_invites: Vec::new(),
            relayed_invites: Vec::new(),
        }
    }
}

fn default_display_name() -> String {
    "Emperor User".to_string()
}

fn is_disallowed_ui_char(c: char) -> bool {
    c.is_control()
        || matches!(
            c,
            '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}'
        )
}

pub fn sanitize_short_text(raw: &str, max_chars: usize) -> String {
    raw.chars()
        .filter(|&c| !is_disallowed_ui_char(c))
        .collect::<String>()
        .trim()
        .chars()
        .take(max_chars)
        .collect()
}

fn sanitize_display_name(raw: &str, fallback: &str) -> String {
    let cleaned = sanitize_short_text(raw, MAX_DISPLAY_NAME_CHARS);
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

fn effective_display_name(store: &FriendsStore) -> String {
    let trimmed = sanitize_short_text(&store.display_name, MAX_DISPLAY_NAME_CHARS);
    if trimmed.is_empty() {
        default_display_name()
    } else {
        trimmed
    }
}

const MAX_PROFILE_PICTURE_BYTES: usize = 262_144;

fn validate_profile_picture(data_url: &str) -> Result<()> {
    if data_url.len() > MAX_PROFILE_PICTURE_BYTES {
        bail!("Profile picture exceeds 256 KB limit");
    }
    #[derive(Clone, Copy)]
    enum ImageKind {
        Png,
        Jpeg,
        Webp,
        Gif,
    }
    let (kind, b64_payload) = if let Some(p) = data_url.strip_prefix("data:image/png;base64,") {
        (ImageKind::Png, p)
    } else if let Some(p) = data_url.strip_prefix("data:image/jpeg;base64,") {
        (ImageKind::Jpeg, p)
    } else if let Some(p) = data_url.strip_prefix("data:image/webp;base64,") {
        (ImageKind::Webp, p)
    } else if let Some(p) = data_url.strip_prefix("data:image/gif;base64,") {
        (ImageKind::Gif, p)
    } else {
        bail!("Profile picture must be a PNG, JPEG, WebP, or GIF image");
    };
    if b64_payload.is_empty() {
        bail!("Profile picture payload is empty");
    }
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(b64_payload.as_bytes())
        .context("Profile picture contains invalid base64 data")?;
    let valid_magic = match kind {
        ImageKind::Png => decoded.starts_with(b"\x89PNG\r\n\x1a\n"),
        ImageKind::Jpeg => decoded.starts_with(b"\xFF\xD8\xFF"),
        ImageKind::Gif => decoded.starts_with(b"GIF87a") || decoded.starts_with(b"GIF89a"),
        ImageKind::Webp => {
            decoded.len() >= 12 && &decoded[..4] == b"RIFF" && &decoded[8..12] == b"WEBP"
        }
    };
    if !valid_magic {
        bail!("Profile picture bytes do not match declared image format");
    }
    Ok(())
}

/// Validates that a peer-supplied `EndpointAddr` matches the cryptographically
/// verified `expected_id` and filters out unsafe/excessive transport addresses.
fn sanitize_peer_addr(expected_id: EndpointId, addr: &EndpointAddr) -> Result<EndpointAddr> {
    if addr.id != expected_id {
        bail!("Peer address EndpointId does not match authenticated peer");
    }
    let mut filtered = BTreeSet::new();
    let mut relay_count = 0usize;
    let mut ip_count = 0usize;
    for ta in &addr.addrs {
        match ta {
            TransportAddr::Relay(url) => {
                let scheme = url.scheme();
                let allowed_scheme = scheme == "https" || (cfg!(test) && scheme == "http");
                if allowed_scheme && relay_count < MAX_PEER_RELAY_ADDRS {
                    filtered.insert(TransportAddr::Relay(url.clone()));
                    relay_count += 1;
                }
            }
            TransportAddr::Ip(sock) => {
                let ip = sock.ip();
                let allowed_ip = !ip.is_unspecified()
                    && !ip.is_multicast()
                    && (!ip.is_loopback() || cfg!(test));
                if allowed_ip && ip_count < MAX_PEER_IP_ADDRS {
                    filtered.insert(TransportAddr::Ip(*sock));
                    ip_count += 1;
                }
            }
            _ => {}
        }
    }
    Ok(EndpointAddr {
        id: expected_id,
        addrs: filtered,
    })
}

/// Builds the `EndpointAddr` embedded in shareable join codes.
/// When at least one relay URL is present, raw IP addresses are stripped so
/// pasting join codes in public chat does not leak LAN/home IPs; `iroh` still
/// connects immediately via the relay and upgrades to direct UDP holepunching.
/// When no relay is available (e.g. relay-less LAN or tests), direct IPs are kept.
fn join_ticket_addr(addr: &EndpointAddr) -> EndpointAddr {
    let relays: BTreeSet<TransportAddr> = addr
        .addrs
        .iter()
        .filter(|ta| matches!(ta, TransportAddr::Relay(_)))
        .take(MAX_PEER_RELAY_ADDRS)
        .cloned()
        .collect();
    if !relays.is_empty() {
        EndpointAddr {
            id: addr.id,
            addrs: relays,
        }
    } else {
        addr.clone()
    }
}

/// Returns a relay-only `EndpointAddr` for sharing a friend's address with third-party
/// friends. In production, omits raw IP addresses when no relay is available so home/LAN
/// IPs are never forwarded to third parties; in tests (`N0DisableRelay`), keeps the
/// loopback address so local multi-endpoint discovery tests can connect.
fn shared_friend_addr(addr: &EndpointAddr) -> Option<EndpointAddr> {
    let relays: BTreeSet<TransportAddr> = addr
        .addrs
        .iter()
        .filter(|ta| matches!(ta, TransportAddr::Relay(_)))
        .take(MAX_PEER_RELAY_ADDRS)
        .cloned()
        .collect();
    if !relays.is_empty() {
        Some(EndpointAddr {
            id: addr.id,
            addrs: relays,
        })
    } else if cfg!(test) {
        Some(addr.clone())
    } else {
        None
    }
}

#[derive(Default)]
struct PeerProfileRateLimiter {
    buckets: HashMap<String, (f64, Instant)>,
}

impl PeerProfileRateLimiter {
    fn allow(&mut self, peer_id: &str) -> bool {
        let now = Instant::now();
        if self.buckets.len() > 512 {
            self.buckets
                .retain(|_, (_, ts)| now.duration_since(*ts) < Duration::from_secs(300));
        }
        let entry = self
            .buckets
            .entry(peer_id.to_string())
            .or_insert((PROFILE_RATE_BURST, now));
        let elapsed = now.duration_since(entry.1).as_secs_f64();
        entry.0 = (entry.0 + elapsed * PROFILE_RATE_REFILL_PER_SEC).min(PROFILE_RATE_BURST);
        entry.1 = now;
        if entry.0 >= 1.0 {
            entry.0 -= 1.0;
            true
        } else {
            false
        }
    }
}

#[derive(Default)]
struct HelloRateLimiter {
    per_peer: HashMap<String, Instant>,
    global: VecDeque<Instant>,
}

impl HelloRateLimiter {
    fn allow(&mut self, peer_id: &str) -> bool {
        let now = Instant::now();
        while let Some(&front) = self.global.front() {
            if now.duration_since(front) >= HELLO_GLOBAL_WINDOW {
                self.global.pop_front();
            } else {
                break;
            }
        }
        if self.global.len() >= HELLO_GLOBAL_MAX_PER_WINDOW {
            return false;
        }
        if let Some(&last) = self.per_peer.get(peer_id) {
            if now.duration_since(last) < HELLO_PER_PEER_COOLDOWN {
                return false;
            }
        }
        if self.per_peer.len() > 512 {
            self.per_peer
                .retain(|_, ts| now.duration_since(*ts) < HELLO_GLOBAL_WINDOW);
        }
        self.per_peer.insert(peer_id.to_string(), now);
        self.global.push_back(now);
        true
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalShareableCollection {
    pub id: String,
    pub kind: FriendCollectionKind,
    pub name: String,
    pub game_title: String,
    pub mod_count: usize,
    pub hidden: bool,
    pub visibility: CollectionVisibility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendsState {
    pub endpoint_ready: bool,
    pub appear_offline: bool,
    pub peer_id: Option<String>,
    pub join_code: Option<String>,
    pub display_name: String,
    pub display_name_set: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<String>,
    pub friends_list_visibility: CollectionVisibility,
    #[serde(default = "default_allow_mutual_connections")]
    pub allow_mutual_connections: bool,
    #[serde(default = "default_allow_cache_relay")]
    pub allow_cache_relay: bool,
    #[serde(default = "default_share_active_loadouts_in_feed")]
    pub share_active_loadouts_in_feed: bool,
    pub friends: Vec<FriendRecord>,
    pub pending_requests: Vec<PendingFriendRequest>,
    #[serde(default)]
    pub queued_invites: Vec<QueuedFriendInvite>,
    pub local_collections: Vec<LocalShareableCollection>,
    #[serde(default)]
    pub feed_events: Vec<FriendFeedEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_last_read_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendJoinTicket {
    pub addr: EndpointAddr,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum P2pRequest {
    Hello {
        display_name: String,
        addr: EndpointAddr,
    },
    GetProfile {
        known_hashes: HashMap<String, String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        synced_collection_ids: Option<Vec<String>>,
        /// RFC3339 cutoff timestamp (`max(added_at, feed_cleared_at)`) so the
        /// sender only returns activity events from when this friend was added onwards.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        activity_since: Option<String>,
    },
    GetCachedFriendProfile {
        target_peer_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        known_updated_at: Option<String>,
    },
    /// Deposit a signed friend invite with a mutual friend to hold until
    /// `invite.payload.to_peer_id` comes online.
    DepositFriendInvite {
        invite: SignedFriendInvite,
    },
    /// Cancel a previously deposited relayed invite for `target_peer_id`.
    CancelRelayedInvite {
        target_peer_id: String,
    },
    /// Acknowledge receipt of relayed invites delivered via `PeerProfileSummary.relayed_invites`
    /// so the mutual friend can purge them from its relay queue.
    AckRelayedInvites {
        from_peer_ids: Vec<String>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum P2pResponse {
    HelloAck {
        accepted: bool,
        display_name: String,
        addr: EndpointAddr,
    },
    Profile(PeerProfileSummary),
    CachedFriendProfile(SignedRelayProfile),
    CachedFriendNotNewer,
    RelayedInviteStored,
    RelayedInviteCancelled,
    RelayedInvitesAcked,
    Denied(String),
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn is_timestamp_newer(candidate: &str, existing: Option<&str>) -> bool {
    let Ok(cand_dt) = chrono::DateTime::parse_from_rfc3339(candidate) else {
        return false;
    };
    let Some(existing_str) = existing else {
        return true;
    };
    match chrono::DateTime::parse_from_rfc3339(existing_str) {
        Ok(exist_dt) => cand_dt > exist_dt,
        Err(_) => true,
    }
}

fn is_timestamp_at_or_after(candidate: &str, cutoff: &str) -> bool {
    let Ok(cand_dt) = chrono::DateTime::parse_from_rfc3339(candidate) else {
        return false;
    };
    let Ok(cutoff_dt) = chrono::DateTime::parse_from_rfc3339(cutoff) else {
        return true;
    };
    cand_dt >= cutoff_dt
}

fn max_rfc3339(a: Option<&str>, b: Option<&str>) -> Option<String> {
    let da = a.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| (d, s)));
    let db = b.and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok().map(|d| (d, s)));
    match (da, db) {
        (Some((ta, sa)), Some((tb, sb))) => {
            if ta >= tb {
                Some(sa.to_string())
            } else {
                Some(sb.to_string())
            }
        }
        (Some((_, sa)), None) => Some(sa.to_string()),
        (None, Some((_, sb))) => Some(sb.to_string()),
        (None, None) => None,
    }
}

fn sign_relay_profile(key: &SecretKey, payload: RelayProfilePayload) -> Result<SignedRelayProfile> {
    let json_bytes = serde_json::to_vec(&payload).context("serialize relay profile payload")?;
    let mut msg = Vec::with_capacity(RELAY_PROFILE_DOMAIN_PREFIX.len() + json_bytes.len());
    msg.extend_from_slice(RELAY_PROFILE_DOMAIN_PREFIX);
    msg.extend_from_slice(&json_bytes);
    let sig = key.sign(&msg);
    Ok(SignedRelayProfile {
        payload,
        signature: hex::encode(sig.to_bytes()),
    })
}

fn validate_relay_profile_payload(payload: &RelayProfilePayload) -> Result<()> {
    chrono::DateTime::parse_from_rfc3339(&payload.updated_at)
        .context("invalid relay profile updated_at timestamp")?;
    if let Some(ref pic) = payload.profile_picture {
        validate_profile_picture(pic)?;
    }
    if payload.collections.len() > MAX_REMOTE_COLLECTIONS {
        bail!("relay profile exceeds max collections");
    }
    let mut total_share_bytes = 0usize;
    for col in &payload.collections {
        if col.share_code.len() > share::MAX_SHARE_CODE_BYTES {
            bail!("relay profile collection share_code exceeds max size");
        }
        total_share_bytes = total_share_bytes.saturating_add(col.share_code.len());
        if total_share_bytes > MAX_FRIEND_CACHED_SHARE_BYTES {
            bail!("relay profile total share_code bytes exceed budget");
        }
    }
    if let Some(ref friends) = payload.friends {
        if friends.len() > MAX_REMOTE_FRIENDS {
            bail!("relay profile exceeds max friends");
        }
    }
    if let Some(ref events) = payload.activity_events {
        if events.len() > MAX_FEED_EVENTS {
            bail!("relay profile exceeds max activity events");
        }
        for ev in events {
            chrono::DateTime::parse_from_rfc3339(&ev.occurred_at)
                .context("invalid relay profile activity event occurred_at timestamp")?;
        }
    }
    Ok(())
}

fn verify_signed_relay_profile(expected_peer_id: &str, signed: &SignedRelayProfile) -> Result<()> {
    if signed.payload.peer_id != expected_peer_id {
        bail!("Relay profile peer_id does not match target peer");
    }
    validate_relay_profile_payload(&signed.payload)?;
    let pk: EndpointId = expected_peer_id
        .trim()
        .parse()
        .context("invalid target peer_id")?;
    let sig_bytes = hex::decode(signed.signature.trim()).context("invalid relay signature hex")?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| anyhow!("invalid relay signature length"))?;
    let sig = iroh::Signature::from_bytes(&sig_arr);
    let json_bytes =
        serde_json::to_vec(&signed.payload).context("serialize relay profile payload")?;
    let mut domain_msg = Vec::with_capacity(RELAY_PROFILE_DOMAIN_PREFIX.len() + json_bytes.len());
    domain_msg.extend_from_slice(RELAY_PROFILE_DOMAIN_PREFIX);
    domain_msg.extend_from_slice(&json_bytes);
    if pk.verify(&domain_msg, &sig).is_ok() {
        return Ok(());
    }
    // Dual-verify legacy unprefixed JSON signatures from older peers for smooth upgrades.
    pk.verify(&json_bytes, &sig)
        .map_err(|e| anyhow!("invalid relay profile signature: {e}"))?;
    Ok(())
}

fn sign_friend_invite(key: &SecretKey, payload: FriendInvitePayload) -> Result<SignedFriendInvite> {
    let json_bytes = serde_json::to_vec(&payload).context("serialize friend invite payload")?;
    let mut msg = Vec::with_capacity(RELAY_INVITE_DOMAIN_PREFIX.len() + json_bytes.len());
    msg.extend_from_slice(RELAY_INVITE_DOMAIN_PREFIX);
    msg.extend_from_slice(&json_bytes);
    let sig = key.sign(&msg);
    Ok(SignedFriendInvite {
        payload,
        signature: hex::encode(sig.to_bytes()),
    })
}

fn is_relayed_invite_expired(created_at: &str) -> bool {
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(created_at) else {
        return true;
    };
    let now = chrono::Utc::now();
    let age_secs = now
        .signed_duration_since(dt.with_timezone(&chrono::Utc))
        .num_seconds();
    age_secs > RELAYED_INVITE_TTL_SECS
}

fn prune_expired_relayed_invites(invites: &mut Vec<SignedFriendInvite>) -> bool {
    let before = invites.len();
    invites.retain(|inv| !is_relayed_invite_expired(&inv.payload.created_at));
    invites.len() != before
}

fn verify_signed_friend_invite(signed: &SignedFriendInvite) -> Result<()> {
    let from_id: EndpointId = signed
        .payload
        .from_peer_id
        .trim()
        .parse()
        .context("invalid invite from_peer_id")?;
    let to_id: EndpointId = signed
        .payload
        .to_peer_id
        .trim()
        .parse()
        .context("invalid invite to_peer_id")?;
    if from_id == to_id {
        bail!("invite from_peer_id and to_peer_id must differ");
    }
    if signed.payload.from_peer_id != from_id.to_string()
        || signed.payload.to_peer_id != to_id.to_string()
    {
        bail!("invite peer IDs must be canonical");
    }
    let dt = chrono::DateTime::parse_from_rfc3339(&signed.payload.created_at)
        .context("invalid invite created_at timestamp")?;
    let now = chrono::Utc::now();
    let age_secs = now
        .signed_duration_since(dt.with_timezone(&chrono::Utc))
        .num_seconds();
    if age_secs > RELAYED_INVITE_TTL_SECS {
        bail!("relayed friend invite expired");
    }
    if age_secs < -RELAYED_INVITE_MAX_CLOCK_SKEW_SECS {
        bail!("relayed friend invite created_at is too far in the future");
    }
    if let Some(ref pic) = signed.payload.from_profile_picture {
        validate_profile_picture(pic)?;
    }
    if let Some(ref addr) = signed.payload.from_addr {
        sanitize_peer_addr(from_id, addr)?;
    }
    let sig_bytes = hex::decode(signed.signature.trim()).context("invalid invite signature hex")?;
    let sig_arr: [u8; 64] = sig_bytes
        .try_into()
        .map_err(|_| anyhow!("invalid invite signature length"))?;
    let sig = iroh::Signature::from_bytes(&sig_arr);
    let json_bytes =
        serde_json::to_vec(&signed.payload).context("serialize friend invite payload")?;
    let mut domain_msg = Vec::with_capacity(RELAY_INVITE_DOMAIN_PREFIX.len() + json_bytes.len());
    domain_msg.extend_from_slice(RELAY_INVITE_DOMAIN_PREFIX);
    domain_msg.extend_from_slice(&json_bytes);
    from_id
        .verify(&domain_msg, &sig)
        .map_err(|e| anyhow!("invalid friend invite signature: {e}"))?;
    Ok(())
}

fn deposit_relayed_invite_into_store(
    store: &mut FriendsStore,
    sender_peer_id: &str,
    invite: SignedFriendInvite,
) -> Result<(), String> {
    if invite.payload.from_peer_id != sender_peer_id {
        return Err("Invite sender does not match authenticated peer".into());
    }
    let sender_is_friend = store
        .friends
        .iter()
        .any(|f| f.peer_id == sender_peer_id && f.status == FriendStatus::Connected);
    if !sender_is_friend {
        return Err("Not connected as friends".into());
    }
    if !is_friends_list_visible_to(store, Some(sender_peer_id)) {
        return Err("Friends list is not shared".into());
    }
    let target_peer_id = invite.payload.to_peer_id.clone();
    let Some(target) = store
        .friends
        .iter()
        .find(|f| f.peer_id == target_peer_id && f.status == FriendStatus::Connected)
    else {
        return Err("Target peer is not a connected friend".into());
    };
    let allows_mutual = target
        .cached_profile
        .as_ref()
        .is_some_and(|p| p.allow_mutual_connections);
    if !allows_mutual {
        return Err("Target peer opted out of mutual connections".into());
    }
    if let Err(e) = verify_signed_friend_invite(&invite) {
        return Err(format!("Invalid signed invite: {e}"));
    }

    prune_expired_relayed_invites(&mut store.relayed_invites);

    if let Some(existing) = store.relayed_invites.iter_mut().find(|r| {
        r.payload.from_peer_id == invite.payload.from_peer_id
            && r.payload.to_peer_id == invite.payload.to_peer_id
    }) {
        if is_timestamp_newer(&invite.payload.created_at, Some(&existing.payload.created_at)) {
            *existing = invite;
        }
        return Ok(());
    }

    let sender_count = store
        .relayed_invites
        .iter()
        .filter(|r| r.payload.from_peer_id == invite.payload.from_peer_id)
        .count();
    if sender_count >= MAX_RELAYED_INVITES_PER_SENDER {
        return Err("Too many relayed invites queued for this sender".into());
    }
    let target_count = store
        .relayed_invites
        .iter()
        .filter(|r| r.payload.to_peer_id == invite.payload.to_peer_id)
        .count();
    if target_count >= MAX_RELAYED_INVITES_PER_TARGET {
        return Err("Target peer relay invite queue is full".into());
    }
    if store.relayed_invites.len() >= MAX_RELAYED_INVITES_STORED {
        return Err("Relay invite queue is full".into());
    }

    store.relayed_invites.push(invite);
    Ok(())
}

fn share_mod_identity(m: &share::ShareModEntry) -> String {
    match m.s {
        share::ShareModSource::Nexus => format!(
            "nexus:{}:{}",
            m.domain.as_deref().unwrap_or(""),
            m.mod_id.unwrap_or(0)
        ),
        share::ShareModSource::Thunderstore => format!(
            "ts:{}:{}:{}",
            m.community.as_deref().unwrap_or(""),
            m.namespace.as_deref().unwrap_or(""),
            m.name.as_deref().unwrap_or("")
        ),
        share::ShareModSource::Modio => format!(
            "modio:{}:{}",
            m.game_id.unwrap_or(0),
            m.mod_id.unwrap_or(0)
        ),
    }
}

fn share_mod_label(m: &share::ShareModEntry) -> String {
    if let Some(dn) = m.display_name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return sanitize_short_text(dn, MAX_COLLECTION_NAME_CHARS);
    }
    if let Some(name) = m.name.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        return sanitize_short_text(name, MAX_COLLECTION_NAME_CHARS);
    }
    if let Some(id) = m.mod_id {
        return format!("Mod #{id}");
    }
    "Unknown mod".to_string()
}

fn share_mod_changed(a: &share::ShareModEntry, b: &share::ShareModEntry) -> bool {
    a.file_id != b.file_id
        || a.version != b.version
        || a.enabled != b.enabled
        || a.fomod != b.fomod
        || a.options != b.options
}

fn summarize_share_manifest(code: &str) -> (Option<String>, Option<usize>) {
    if code.is_empty() {
        return (None, None);
    }
    let Ok(manifest) = share::decode_share_code(code) else {
        return (None, None);
    };
    let title = manifest
        .game
        .title
        .as_deref()
        .map(|t| sanitize_short_text(t, MAX_COLLECTION_NAME_CHARS))
        .filter(|t| !t.is_empty());
    (title, Some(manifest.mods.len()))
}

fn diff_share_manifests(
    prev_code: &str,
    next_code: &str,
) -> Option<(FriendFeedDiff, Option<String>, usize)> {
    if prev_code.is_empty() || next_code.is_empty() {
        return None;
    }
    let prev = share::decode_share_code(prev_code).ok()?;
    let next = share::decode_share_code(next_code).ok()?;

    let mut prev_map: HashMap<String, &share::ShareModEntry> = HashMap::new();
    for m in &prev.mods {
        prev_map.insert(share_mod_identity(m), m);
    }
    let mut next_map: HashMap<String, &share::ShareModEntry> = HashMap::new();
    for m in &next.mods {
        next_map.insert(share_mod_identity(m), m);
    }

    let mut diff = FriendFeedDiff::default();
    let mut seen_added = HashSet::new();
    let mut seen_updated = HashSet::new();
    for m in &next.mods {
        let key = share_mod_identity(m);
        match prev_map.get(&key) {
            None => {
                if seen_added.insert(key) {
                    diff.mods_added_count += 1;
                    if diff.mods_added.len() < MAX_FEED_DIFF_NAMES {
                        diff.mods_added.push(share_mod_label(m));
                    }
                }
            }
            Some(old_m) => {
                if share_mod_changed(old_m, m) && seen_updated.insert(key) {
                    diff.mods_updated_count += 1;
                    if diff.mods_updated.len() < MAX_FEED_DIFF_NAMES {
                        diff.mods_updated.push(share_mod_label(m));
                    }
                }
            }
        }
    }

    let mut seen_removed = HashSet::new();
    for m in &prev.mods {
        let key = share_mod_identity(m);
        if !next_map.contains_key(&key) && seen_removed.insert(key) {
            diff.mods_removed_count += 1;
            if diff.mods_removed.len() < MAX_FEED_DIFF_NAMES {
                diff.mods_removed.push(share_mod_label(m));
            }
        }
    }

    let game_title = next
        .game
        .title
        .as_deref()
        .map(|t| sanitize_short_text(t, MAX_COLLECTION_NAME_CHARS))
        .filter(|t| !t.is_empty());

    Some((diff, game_title, next.mods.len()))
}

fn collect_profile_feed_events(
    peer_id: &str,
    display_name: &str,
    prev: &PeerProfileSummary,
    next: &PeerProfileSummary,
    now: &str,
) -> Vec<FriendFeedEvent> {
    let prev_by_id: HashMap<&str, &FriendShareRef> = prev
        .collections
        .iter()
        .map(|c| (c.id.as_str(), c))
        .collect();

    let mut events = Vec::new();
    for (idx, col) in next.collections.iter().enumerate() {
        match prev_by_id.get(col.id.as_str()) {
            None => {
                let (game_title, mod_count) = summarize_share_manifest(&col.share_code);
                events.push(FriendFeedEvent {
                    id: format!("{peer_id}:{}:{}:{now}:{idx}", col.id, col.share_hash),
                    peer_id: peer_id.to_string(),
                    friend_display_name: display_name.to_string(),
                    is_self: false,
                    kind: FriendFeedEventKind::CollectionAdded,
                    collection_id: col.id.clone(),
                    collection_kind: col.kind,
                    collection_name: col.name.clone(),
                    previous_name: None,
                    game_title,
                    mod_count,
                    share_hash: col.share_hash.clone(),
                    diff: None,
                    occurred_at: now.to_string(),
                });
            }
            Some(old) => {
                if old.share_hash != col.share_hash {
                    let (diff, game_title, mod_count) =
                        match diff_share_manifests(&old.share_code, &col.share_code) {
                            Some((d, gt, mc)) => (Some(d), gt, Some(mc)),
                            None => {
                                let (gt, mc) = summarize_share_manifest(&col.share_code);
                                (None, gt, mc)
                            }
                        };
                    let previous_name = if old.name != col.name {
                        Some(old.name.clone())
                    } else {
                        None
                    };
                    events.push(FriendFeedEvent {
                        id: format!("{peer_id}:{}:{}:{now}:{idx}", col.id, col.share_hash),
                        peer_id: peer_id.to_string(),
                        friend_display_name: display_name.to_string(),
                        is_self: false,
                        kind: FriendFeedEventKind::CollectionUpdated,
                        collection_id: col.id.clone(),
                        collection_kind: col.kind,
                        collection_name: col.name.clone(),
                        previous_name,
                        game_title,
                        mod_count,
                        share_hash: col.share_hash.clone(),
                        diff,
                        occurred_at: now.to_string(),
                    });
                } else if old.name != col.name {
                    let (game_title, mod_count) = summarize_share_manifest(&col.share_code);
                    events.push(FriendFeedEvent {
                        id: format!("{peer_id}:{}:rename:{now}:{idx}", col.id),
                        peer_id: peer_id.to_string(),
                        friend_display_name: display_name.to_string(),
                        is_self: false,
                        kind: FriendFeedEventKind::CollectionRenamed,
                        collection_id: col.id.clone(),
                        collection_kind: col.kind,
                        collection_name: col.name.clone(),
                        previous_name: Some(old.name.clone()),
                        game_title,
                        mod_count,
                        share_hash: col.share_hash.clone(),
                        diff: None,
                        occurred_at: now.to_string(),
                    });
                }
            }
        }
    }
    events
}

fn diff_is_empty(diff: &FriendFeedDiff) -> bool {
    diff.mods_added_count == 0 && diff.mods_removed_count == 0 && diff.mods_updated_count == 0
}

fn sanitize_local_activity_event(
    ev: &LocalActivityEvent,
    now_dt: chrono::DateTime<chrono::FixedOffset>,
) -> Option<LocalActivityEvent> {
    let ev_dt = chrono::DateTime::parse_from_rfc3339(&ev.occurred_at).ok()?;
    let max_allowed = now_dt + chrono::Duration::seconds(MAX_CLOCK_SKEW_FUTURE_SECS);
    let occurred_at = if ev_dt > max_allowed {
        now_dt.to_rfc3339()
    } else {
        ev.occurred_at.clone()
    };

    let id = sanitize_short_text(&ev.id, MAX_COLLECTION_ID_CHARS * 2);
    let collection_id = sanitize_short_text(&ev.collection_id, MAX_COLLECTION_ID_CHARS);
    if id.is_empty() || collection_id.is_empty() {
        return None;
    }
    let collection_name = sanitize_short_text(&ev.collection_name, MAX_COLLECTION_NAME_CHARS);
    let previous_name = ev
        .previous_name
        .as_deref()
        .map(|s| sanitize_short_text(s, MAX_COLLECTION_NAME_CHARS))
        .filter(|s| !s.is_empty());
    let game_title = ev
        .game_title
        .as_deref()
        .map(|s| sanitize_short_text(s, MAX_COLLECTION_NAME_CHARS))
        .filter(|s| !s.is_empty());
    let share_hash = sanitize_short_text(&ev.share_hash, MAX_SHARE_HASH_CHARS);
    let diff = ev.diff.as_ref().map(|d| FriendFeedDiff {
        mods_added: d
            .mods_added
            .iter()
            .take(MAX_FEED_DIFF_NAMES)
            .map(|s| sanitize_short_text(s, MAX_COLLECTION_NAME_CHARS))
            .filter(|s| !s.is_empty())
            .collect(),
        mods_added_count: d.mods_added_count,
        mods_removed: d
            .mods_removed
            .iter()
            .take(MAX_FEED_DIFF_NAMES)
            .map(|s| sanitize_short_text(s, MAX_COLLECTION_NAME_CHARS))
            .filter(|s| !s.is_empty())
            .collect(),
        mods_removed_count: d.mods_removed_count,
        mods_updated: d
            .mods_updated
            .iter()
            .take(MAX_FEED_DIFF_NAMES)
            .map(|s| sanitize_short_text(s, MAX_COLLECTION_NAME_CHARS))
            .filter(|s| !s.is_empty())
            .collect(),
        mods_updated_count: d.mods_updated_count,
    });

    Some(LocalActivityEvent {
        id,
        kind: ev.kind,
        collection_id,
        collection_kind: ev.collection_kind,
        collection_name,
        previous_name,
        game_title,
        mod_count: ev.mod_count,
        share_hash,
        diff,
        occurred_at,
    })
}

fn collect_current_local_collections(
    paths: &Paths,
) -> Vec<(String, FriendCollectionKind, String, String, String)> {
    let mut out = Vec::new();
    if let Ok(cfg) = load_config(paths) {
        for game in &cfg.managed_games {
            if let Ok(order) = mods::load_loadorder(paths, &game.id) {
                let name = format!("{} Loadout", game.title);
                let (mut manifest, _, _) = share::build_manifest_from_loadorder_with_selections(
                    &order,
                    game,
                    Some(name.clone()),
                    true,
                );
                if manifest.mods.is_empty() {
                    continue;
                }
                manifest.game_versions = game.game_version.as_ref().map(|v| vec![v.clone()]);
                if let Ok(code) = share::encode_share_manifest(&manifest) {
                    let hash = share::share_hash(&code);
                    out.push((
                        format!("active:{}", game.id),
                        FriendCollectionKind::ActiveLoadout,
                        name,
                        hash,
                        code,
                    ));
                }
            }
        }
    }
    if let Ok(saved) = share::load_saved_collections(paths) {
        for entry in saved.collections {
            let hash = share::share_hash(&entry.code);
            out.push((
                format!("saved:{}", entry.id),
                FriendCollectionKind::SavedCollection,
                entry.name,
                hash,
                entry.code,
            ));
        }
    }
    out
}

fn record_local_activity_in_store(
    store: &mut FriendsStore,
    current: Vec<(String, FriendCollectionKind, String, String, String)>,
    now: &str,
) -> bool {
    if !store.local_activity_initialized {
        store.local_collection_snapshots.clear();
        for (id, kind, name, share_hash, share_code) in current {
            let shared = !matches!(
                resolve_collection_visibility(store, &id),
                CollectionVisibility::Private
            );
            store.local_collection_snapshots.insert(
                id.clone(),
                LocalCollectionSnapshot {
                    id,
                    kind,
                    name,
                    share_hash,
                    share_code,
                    shared,
                    coalesce_base_code: None,
                    coalesce_event_id: None,
                },
            );
        }
        store.local_activity_initialized = true;
        return true;
    }

    let now_dt = chrono::DateTime::parse_from_rfc3339(now).ok();
    let shared_saved_manifests: Vec<ShareManifest> = current
        .iter()
        .filter(|(id, kind, _, _, _)| {
            *kind == FriendCollectionKind::SavedCollection
                && !matches!(
                    resolve_collection_visibility(store, id),
                    CollectionVisibility::Private
                )
        })
        .filter_map(|(_, _, _, _, share_code)| share::decode_share_code(share_code).ok())
        .collect();
    let mut current_ids = HashSet::with_capacity(current.len());
    let mut mutated = false;

    for (idx, (id, kind, name, share_hash, share_code)) in current.into_iter().enumerate() {
        current_ids.insert(id.clone());
        let is_shared = !matches!(
            resolve_collection_visibility(store, &id),
            CollectionVisibility::Private
        );
        let suppress_active_event = kind == FriendCollectionKind::ActiveLoadout
            && (!store.share_active_loadouts_in_feed
                || (!shared_saved_manifests.is_empty()
                    && share::decode_share_code(&share_code).ok().is_some_and(|am| {
                        shared_saved_manifests
                            .iter()
                            .any(|sm| share::manifest_mods_match(&am.mods, &sm.mods))
                    })));
        let prev = store.local_collection_snapshots.get(&id).cloned();

        if !is_shared || suppress_active_event {
            let needs_update = prev.as_ref().is_none_or(|p| {
                p.shared != is_shared || p.name != name || p.share_hash != share_hash
            });
            if needs_update {
                store.local_collection_snapshots.insert(
                    id.clone(),
                    LocalCollectionSnapshot {
                        id,
                        kind,
                        name,
                        share_hash,
                        share_code,
                        shared: is_shared,
                        coalesce_base_code: None,
                        coalesce_event_id: None,
                    },
                );
                mutated = true;
            }
            continue;
        }

        match prev {
            None => {
                let (game_title, mod_count) = summarize_share_manifest(&share_code);
                store.local_activity.push(LocalActivityEvent {
                    id: format!("{id}:{share_hash}:{now}:{idx}"),
                    kind: FriendFeedEventKind::CollectionAdded,
                    collection_id: id.clone(),
                    collection_kind: kind,
                    collection_name: name.clone(),
                    previous_name: None,
                    game_title,
                    mod_count,
                    share_hash: share_hash.clone(),
                    diff: None,
                    occurred_at: now.to_string(),
                });
                store.local_collection_snapshots.insert(
                    id.clone(),
                    LocalCollectionSnapshot {
                        id,
                        kind,
                        name,
                        share_hash,
                        share_code,
                        shared: true,
                        coalesce_base_code: None,
                        coalesce_event_id: None,
                    },
                );
                mutated = true;
            }
            Some(old) if !old.shared => {
                // Collection transitioned from Private -> Shared: emit a single CollectionAdded.
                let (game_title, mod_count) = summarize_share_manifest(&share_code);
                store.local_activity.push(LocalActivityEvent {
                    id: format!("{id}:{share_hash}:{now}:{idx}"),
                    kind: FriendFeedEventKind::CollectionAdded,
                    collection_id: id.clone(),
                    collection_kind: kind,
                    collection_name: name.clone(),
                    previous_name: None,
                    game_title,
                    mod_count,
                    share_hash: share_hash.clone(),
                    diff: None,
                    occurred_at: now.to_string(),
                });
                store.local_collection_snapshots.insert(
                    id.clone(),
                    LocalCollectionSnapshot {
                        id,
                        kind,
                        name,
                        share_hash,
                        share_code,
                        shared: true,
                        coalesce_base_code: None,
                        coalesce_event_id: None,
                    },
                );
                mutated = true;
            }
            Some(old) => {
                if old.share_hash != share_hash {
                    // Check if we can coalesce into the most recent CollectionUpdated event
                    // for this collection within LOCAL_ACTIVITY_COALESCE_WINDOW_SECS.
                    let coalesce_pos = old.coalesce_event_id.as_deref().and_then(|cid| {
                        let ndt = now_dt?;
                        store.local_activity.iter().position(|ev| {
                            if ev.id != cid
                                || ev.collection_id != id
                                || ev.kind != FriendFeedEventKind::CollectionUpdated
                            {
                                return false;
                            }
                            let Ok(edt) = chrono::DateTime::parse_from_rfc3339(&ev.occurred_at)
                            else {
                                return false;
                            };
                            let delta = (ndt - edt).num_seconds();
                            (0..=LOCAL_ACTIVITY_COALESCE_WINDOW_SECS).contains(&delta)
                        })
                    });

                    if let Some(pos) = coalesce_pos {
                        let base_code = old
                            .coalesce_base_code
                            .as_deref()
                            .unwrap_or(&old.share_code);
                        let (diff, game_title, mod_count) =
                            match diff_share_manifests(base_code, &share_code) {
                                Some((d, gt, mc)) => (Some(d), gt, Some(mc)),
                                None => {
                                    let (gt, mc) = summarize_share_manifest(&share_code);
                                    (None, gt, mc)
                                }
                            };
                        let net_empty = diff.as_ref().is_some_and(diff_is_empty);
                        let renamed_in_ev = store.local_activity[pos].previous_name.is_some()
                            || store.local_activity[pos].collection_name != name;
                        if net_empty && !renamed_in_ev {
                            // Rapid edits cancelled out completely back to baseline.
                            store.local_activity.remove(pos);
                            store.local_collection_snapshots.insert(
                                id.clone(),
                                LocalCollectionSnapshot {
                                    id,
                                    kind,
                                    name,
                                    share_hash,
                                    share_code,
                                    shared: true,
                                    coalesce_base_code: None,
                                    coalesce_event_id: None,
                                },
                            );
                        } else {
                            let ev = &mut store.local_activity[pos];
                            if ev.previous_name.is_none() && ev.collection_name != name {
                                ev.previous_name = Some(ev.collection_name.clone());
                            }
                            ev.collection_name = name.clone();
                            ev.game_title = game_title;
                            ev.mod_count = mod_count;
                            ev.share_hash = share_hash.clone();
                            ev.diff = diff;
                            ev.occurred_at = now.to_string();
                            let kept_base = old
                                .coalesce_base_code
                                .clone()
                                .unwrap_or_else(|| old.share_code.clone());
                            let kept_ev_id = ev.id.clone();
                            store.local_collection_snapshots.insert(
                                id.clone(),
                                LocalCollectionSnapshot {
                                    id,
                                    kind,
                                    name,
                                    share_hash,
                                    share_code,
                                    shared: true,
                                    coalesce_base_code: Some(kept_base),
                                    coalesce_event_id: Some(kept_ev_id),
                                },
                            );
                        }
                    } else {
                        let (diff, game_title, mod_count) =
                            match diff_share_manifests(&old.share_code, &share_code) {
                                Some((d, gt, mc)) => (Some(d), gt, Some(mc)),
                                None => {
                                    let (gt, mc) = summarize_share_manifest(&share_code);
                                    (None, gt, mc)
                                }
                            };
                        let only_renamed =
                            diff.as_ref().is_some_and(diff_is_empty) && old.name != name;
                        if only_renamed {
                            store.local_activity.push(LocalActivityEvent {
                                id: format!("{id}:rename:{now}:{idx}"),
                                kind: FriendFeedEventKind::CollectionRenamed,
                                collection_id: id.clone(),
                                collection_kind: kind,
                                collection_name: name.clone(),
                                previous_name: Some(old.name.clone()),
                                game_title,
                                mod_count,
                                share_hash: share_hash.clone(),
                                diff: None,
                                occurred_at: now.to_string(),
                            });
                            store.local_collection_snapshots.insert(
                                id.clone(),
                                LocalCollectionSnapshot {
                                    id,
                                    kind,
                                    name,
                                    share_hash,
                                    share_code,
                                    shared: true,
                                    coalesce_base_code: None,
                                    coalesce_event_id: None,
                                },
                            );
                        } else {
                            let ev_id = format!("{id}:{share_hash}:{now}:{idx}");
                            let previous_name = if old.name != name {
                                Some(old.name.clone())
                            } else {
                                None
                            };
                            store.local_activity.push(LocalActivityEvent {
                                id: ev_id.clone(),
                                kind: FriendFeedEventKind::CollectionUpdated,
                                collection_id: id.clone(),
                                collection_kind: kind,
                                collection_name: name.clone(),
                                previous_name,
                                game_title,
                                mod_count,
                                share_hash: share_hash.clone(),
                                diff,
                                occurred_at: now.to_string(),
                            });
                            store.local_collection_snapshots.insert(
                                id.clone(),
                                LocalCollectionSnapshot {
                                    id,
                                    kind,
                                    name,
                                    share_hash,
                                    share_code,
                                    shared: true,
                                    coalesce_base_code: Some(old.share_code),
                                    coalesce_event_id: Some(ev_id),
                                },
                            );
                        }
                    }
                    mutated = true;
                } else if old.name != name {
                    let (game_title, mod_count) = summarize_share_manifest(&share_code);
                    store.local_activity.push(LocalActivityEvent {
                        id: format!("{id}:rename:{now}:{idx}"),
                        kind: FriendFeedEventKind::CollectionRenamed,
                        collection_id: id.clone(),
                        collection_kind: kind,
                        collection_name: name.clone(),
                        previous_name: Some(old.name.clone()),
                        game_title,
                        mod_count,
                        share_hash: share_hash.clone(),
                        diff: None,
                        occurred_at: now.to_string(),
                    });
                    store.local_collection_snapshots.insert(
                        id.clone(),
                        LocalCollectionSnapshot {
                            id,
                            kind,
                            name,
                            share_hash,
                            share_code,
                            shared: true,
                            coalesce_base_code: None,
                            coalesce_event_id: None,
                        },
                    );
                    mutated = true;
                }
            }
        }
    }

    let before_snaps = store.local_collection_snapshots.len();
    store
        .local_collection_snapshots
        .retain(|k, _| current_ids.contains(k));
    if store.local_collection_snapshots.len() != before_snaps {
        mutated = true;
    }

    if mutated {
        store.local_activity.sort_by(|a, b| {
            let da = chrono::DateTime::parse_from_rfc3339(&a.occurred_at).ok();
            let db = chrono::DateTime::parse_from_rfc3339(&b.occurred_at).ok();
            db.cmp(&da)
                .then_with(|| b.occurred_at.cmp(&a.occurred_at))
                .then_with(|| b.id.cmp(&a.id))
        });
        store.local_activity.truncate(MAX_FEED_EVENTS);
    }

    mutated
}

fn merge_remote_activity_events(
    peer_id: &str,
    display_name: &str,
    incoming: &[LocalActivityEvent],
    visible_col_ids: &HashSet<String>,
    cutoff: Option<&str>,
    feed_events: &mut Vec<FriendFeedEvent>,
    now: &str,
) -> bool {
    let now_dt = chrono::DateTime::parse_from_rfc3339(now)
        .unwrap_or_else(|_| chrono::Utc::now().fixed_offset());
    let mut changed = false;

    for raw in incoming.iter().take(MAX_FEED_EVENTS) {
        let Some(clean) = sanitize_local_activity_event(raw, now_dt) else {
            continue;
        };
        if !visible_col_ids.contains(&clean.collection_id) {
            continue;
        }
        if let Some(c) = cutoff {
            if !is_timestamp_at_or_after(&clean.occurred_at, c) {
                continue;
            }
        }
        let global_id = format!("{peer_id}:{}", clean.id);
        if let Some(existing) = feed_events.iter_mut().find(|ev| ev.id == global_id) {
            if existing.share_hash != clean.share_hash
                || existing.occurred_at != clean.occurred_at
                || existing.diff != clean.diff
                || existing.collection_name != clean.collection_name
                || existing.previous_name != clean.previous_name
                || existing.mod_count != clean.mod_count
                || existing.game_title != clean.game_title
            {
                existing.friend_display_name = display_name.to_string();
                existing.kind = clean.kind;
                existing.collection_kind = clean.collection_kind;
                existing.collection_name = clean.collection_name;
                existing.previous_name = clean.previous_name;
                existing.game_title = clean.game_title;
                existing.mod_count = clean.mod_count;
                existing.share_hash = clean.share_hash;
                existing.diff = clean.diff;
                existing.occurred_at = clean.occurred_at;
                changed = true;
            }
        } else {
            feed_events.push(FriendFeedEvent {
                id: global_id,
                peer_id: peer_id.to_string(),
                friend_display_name: display_name.to_string(),
                is_self: false,
                kind: clean.kind,
                collection_id: clean.collection_id,
                collection_kind: clean.collection_kind,
                collection_name: clean.collection_name,
                previous_name: clean.previous_name,
                game_title: clean.game_title,
                mod_count: clean.mod_count,
                share_hash: clean.share_hash,
                diff: clean.diff,
                occurred_at: clean.occurred_at,
            });
            changed = true;
        }
    }

    if changed {
        feed_events.sort_by(|a, b| {
            let da = chrono::DateTime::parse_from_rfc3339(&a.occurred_at).ok();
            let db = chrono::DateTime::parse_from_rfc3339(&b.occurred_at).ok();
            db.cmp(&da)
                .then_with(|| b.occurred_at.cmp(&a.occurred_at))
                .then_with(|| b.id.cmp(&a.id))
        });
        feed_events.truncate(MAX_FEED_EVENTS);
    }

    changed
}

fn timestamps_within_secs(a: &str, b: &str, window_secs: i64) -> bool {
    let (Ok(da), Ok(db)) = (
        chrono::DateTime::parse_from_rfc3339(a),
        chrono::DateTime::parse_from_rfc3339(b),
    ) else {
        return a == b;
    };
    (da - db).num_seconds().abs() <= window_secs
}

fn dedupe_active_loadout_local_events(events: &mut Vec<LocalActivityEvent>) {
    let saved: Vec<LocalActivityEvent> = events
        .iter()
        .filter(|ev| ev.collection_kind == FriendCollectionKind::SavedCollection)
        .cloned()
        .collect();
    if saved.is_empty() {
        return;
    }
    events.retain(|ev| {
        if ev.collection_kind != FriendCollectionKind::ActiveLoadout {
            return true;
        }
        !saved.iter().any(|s| {
            s.kind == ev.kind
                && s.game_title == ev.game_title
                && s.mod_count == ev.mod_count
                && s.diff == ev.diff
                && timestamps_within_secs(&s.occurred_at, &ev.occurred_at, FEED_DEDUPE_WINDOW_SECS)
        })
    });
}

fn dedupe_active_loadout_feed_events(events: &mut Vec<FriendFeedEvent>) {
    let saved: Vec<FriendFeedEvent> = events
        .iter()
        .filter(|ev| ev.collection_kind == FriendCollectionKind::SavedCollection)
        .cloned()
        .collect();
    if saved.is_empty() {
        return;
    }
    events.retain(|ev| {
        if ev.collection_kind != FriendCollectionKind::ActiveLoadout {
            return true;
        }
        !saved.iter().any(|s| {
            s.is_self == ev.is_self
                && s.peer_id == ev.peer_id
                && s.kind == ev.kind
                && s.game_title == ev.game_title
                && s.mod_count == ev.mod_count
                && s.diff == ev.diff
                && timestamps_within_secs(&s.occurred_at, &ev.occurred_at, FEED_DEDUPE_WINDOW_SECS)
        })
    });
}

fn build_combined_feed_events(
    store: &FriendsStore,
    local_collections: &[LocalShareableCollection],
    self_peer_id: Option<&str>,
) -> Vec<FriendFeedEvent> {
    let mut combined = store.feed_events.clone();
    if !store.local_activity.is_empty() {
        let shared_local_col_ids: HashSet<&str> = local_collections
            .iter()
            .filter(|c| !matches!(c.visibility, CollectionVisibility::Private))
            .map(|c| c.id.as_str())
            .collect();

        let self_pid = self_peer_id.unwrap_or("self").to_string();
        let self_name = effective_display_name(store);
        let cleared_at = store.feed_cleared_at.as_deref();
        let mut added_self = false;

        for ev in &store.local_activity {
            if !store.share_active_loadouts_in_feed
                && ev.collection_kind == FriendCollectionKind::ActiveLoadout
            {
                continue;
            }
            if !shared_local_col_ids.contains(ev.collection_id.as_str()) {
                continue;
            }
            if !is_timestamp_newer(&ev.occurred_at, cleared_at) {
                continue;
            }
            combined.push(FriendFeedEvent {
                id: format!("self:{}", ev.id),
                peer_id: self_pid.clone(),
                friend_display_name: self_name.clone(),
                is_self: true,
                kind: ev.kind,
                collection_id: ev.collection_id.clone(),
                collection_kind: ev.collection_kind,
                collection_name: ev.collection_name.clone(),
                previous_name: ev.previous_name.clone(),
                game_title: ev.game_title.clone(),
                mod_count: ev.mod_count,
                share_hash: ev.share_hash.clone(),
                diff: ev.diff.clone(),
                occurred_at: ev.occurred_at.clone(),
            });
            added_self = true;
        }

        if added_self {
            combined.sort_by(|a, b| {
                let da = chrono::DateTime::parse_from_rfc3339(&a.occurred_at).ok();
                let db = chrono::DateTime::parse_from_rfc3339(&b.occurred_at).ok();
                db.cmp(&da)
                    .then_with(|| b.occurred_at.cmp(&a.occurred_at))
                    .then_with(|| b.id.cmp(&a.id))
            });
            combined.truncate(MAX_FEED_EVENTS);
        }
    }

    dedupe_active_loadout_feed_events(&mut combined);
    combined
}

pub fn encode_join_code(ticket: &FriendJoinTicket) -> Result<String> {
    let json = serde_json::to_vec(ticket).context("serialize join ticket")?;
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&json).context("deflate join ticket")?;
    let compressed = encoder.finish().context("finish deflate")?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed);
    Ok(format!("{JOIN_CODE_PREFIX}{encoded}"))
}

pub fn decode_join_code(raw: &str) -> Result<FriendJoinTicket> {
    let trimmed = raw.trim();
    let normalized = if let Some(code) = crate::deep_link::parse_emperor_friend_url(trimmed) {
        code
    } else if let Ok(url) = url::Url::parse(trimmed) {
        if matches!(url.scheme(), "https" | "http") {
            let route = url
                .query_pairs()
                .find(|(k, _)| k == "to")
                .map(|(_, v)| v.into_owned())
                .or_else(|| url.fragment().map(|f| f.to_string()));
            route
                .as_deref()
                .and_then(|r| r.trim().strip_prefix("friend/"))
                .unwrap_or(trimmed)
                .to_string()
        } else {
            trimmed.to_string()
        }
    } else {
        trimmed.to_string()
    };
    let payload = normalized
        .trim()
        .strip_prefix(JOIN_CODE_PREFIX)
        .ok_or_else(|| anyhow!("Join code must start with {JOIN_CODE_PREFIX}"))?;
    if payload.len() > MAX_JOIN_CODE_LEN {
        bail!("Join code exceeds maximum allowed length");
    }
    let compressed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim())
        .context("invalid base64 in join code")?;
    let decoder = DeflateDecoder::new(&compressed[..]);
    let mut json = Vec::new();
    decoder
        .take((MAX_JOIN_TICKET_BYTES + 1) as u64)
        .read_to_end(&mut json)
        .context("failed to decompress join code")?;
    if json.len() > MAX_JOIN_TICKET_BYTES {
        bail!("Decompressed join code exceeds maximum allowed size");
    }
    let mut ticket: FriendJoinTicket =
        serde_json::from_slice(&json).context("invalid join ticket payload")?;
    ticket.name = sanitize_display_name(&ticket.name, "Friend");
    ticket.addr = sanitize_peer_addr(ticket.addr.id, &ticket.addr)?;
    Ok(ticket)
}

fn store_path(paths: &Paths) -> PathBuf {
    paths.data_dir.join("friends_store.json")
}

fn identity_key_path(paths: &Paths) -> PathBuf {
    paths.config_dir.join("p2p_identity.key")
}

pub fn load_friends_store(paths: &Paths) -> FriendsStore {
    let path = store_path(paths);
    let Ok(raw) = fs::read_to_string(&path) else {
        return FriendsStore::default();
    };
    let mut store: FriendsStore = match serde_json::from_str(&raw) {
        Ok(s) => s,
        Err(e) => {
            log::warn!("Failed to parse friends_store.json ({e}); backing up corrupt file");
            let backup = path.with_extension("json.corrupt");
            let _ = fs::copy(&path, &backup);
            return FriendsStore::default();
        }
    };
    // Migrate existing stores written before `display_name_set` was tracked.
    if !store.display_name_set && !store.display_name.trim().is_empty() {
        let has_flag = serde_json::from_str::<serde_json::Value>(&raw)
            .ok()
            .and_then(|v| v.get("display_name_set").cloned())
            .is_some();
        if !has_flag {
            store.display_name_set = true;
        }
    }
    // On cold start, all friends start marked offline until dialed.
    // Also backfill `added_at` for stores written before it existed so existing
    // friends don't flood the feed with pre-upgrade activity history.
    let migration_now = now_rfc3339();
    for f in &mut store.friends {
        f.online = false;
        if f.added_at.is_none() {
            f.added_at = f
                .last_seen_at
                .clone()
                .or_else(|| f.cached_profile.as_ref().map(|p| p.updated_at.clone()))
                .or_else(|| Some(migration_now.clone()));
        }
    }
    store
}

pub fn save_friends_store(paths: &Paths, store: &FriendsStore) -> Result<()> {
    let path = store_path(paths);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(store).context("serialize friends store")?;
    let tmp_path = path.with_extension("json.tmp");
    {
        let mut opts = fs::OpenOptions::new();
        opts.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            opts.mode(0o600);
        }
        let mut file = opts.open(&tmp_path).context("open temp friends store")?;
        file.write_all(json.as_bytes())
            .context("write temp friends store")?;
        let _ = file.sync_all();
    }
    fs::rename(&tmp_path, &path).context("rename temp friends store")
}

fn load_or_create_secret_key(paths: &Paths) -> Result<SecretKey> {
    let path = identity_key_path(paths);
    if let Ok(bytes) = fs::read(&path) {
        if bytes.len() == 32 {
            let arr: [u8; 32] = bytes.try_into().expect("checked length 32");
            return Ok(SecretKey::from_bytes(&arr));
        }
    }
    let key = SecretKey::generate();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&path).context("open p2p identity key")?;
    file.write_all(&key.to_bytes())
        .context("write p2p identity key")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }
    Ok(key)
}

pub fn resolve_collection_visibility(
    store: &FriendsStore,
    col_id: &str,
) -> CollectionVisibility {
    if let Some(vis) = store.collection_visibility.get(col_id) {
        return vis.clone();
    }
    if store.hidden_collection_ids.contains(col_id) {
        return CollectionVisibility::Private;
    }
    if col_id.starts_with("saved:") {
        return CollectionVisibility::Private;
    }
    CollectionVisibility::AllFriends
}

pub fn is_collection_visible_to(
    store: &FriendsStore,
    col_id: &str,
    viewer_peer_id: Option<&str>,
) -> bool {
    match resolve_collection_visibility(store, col_id) {
        CollectionVisibility::AllFriends => true,
        CollectionVisibility::SpecificFriends { peer_ids } => {
            viewer_peer_id.is_some_and(|pid| peer_ids.iter().any(|p| p == pid))
        }
        CollectionVisibility::Private => false,
    }
}

pub fn is_friends_list_visible_to(store: &FriendsStore, viewer_peer_id: Option<&str>) -> bool {
    match &store.friends_list_visibility {
        CollectionVisibility::AllFriends => true,
        CollectionVisibility::SpecificFriends { peer_ids } => {
            viewer_peer_id.is_some_and(|pid| peer_ids.iter().any(|p| p == pid))
        }
        CollectionVisibility::Private => false,
    }
}

fn resolve_cached_friend_profile_for_peer(
    store: &FriendsStore,
    requester_peer_id: &str,
    target_peer_id: &str,
    known_updated_at: Option<&str>,
) -> P2pResponse {
    let requester_is_friend = store
        .friends
        .iter()
        .any(|f| f.peer_id == requester_peer_id && f.status == FriendStatus::Connected);
    if !requester_is_friend {
        return P2pResponse::Denied("Not connected as friends".into());
    }
    if !store.allow_cache_relay {
        return P2pResponse::Denied("Cache relay is disabled by this peer".into());
    }
    if requester_peer_id == target_peer_id {
        return P2pResponse::Denied("Cannot relay your own profile".into());
    }
    if !is_friends_list_visible_to(store, Some(requester_peer_id)) {
        return P2pResponse::Denied("Friends list is not shared".into());
    }
    let Some(target) = store
        .friends
        .iter()
        .find(|f| f.peer_id == target_peer_id && f.status == FriendStatus::Connected)
    else {
        return P2pResponse::Denied("Target peer is not a connected friend".into());
    };
    let Some(ref profile) = target.cached_profile else {
        return P2pResponse::Denied("No cached profile for target peer".into());
    };
    if !profile.allow_mutual_connections {
        return P2pResponse::Denied("Target peer opted out of mutual connections".into());
    }
    let Some(ref relay) = profile.signed_relay_profile else {
        return P2pResponse::Denied("Target peer has no relayable profile".into());
    };
    if verify_signed_relay_profile(target_peer_id, relay).is_err() {
        return P2pResponse::Denied("Cached relay profile signature invalid".into());
    }
    // Check the AllFriends-only relay payload's friends list so a SpecificFriends
    // list shared only with us cannot be used to infer a hidden friendship.
    let mutual_confirmed = relay
        .payload
        .friends
        .as_ref()
        .is_some_and(|list| list.iter().any(|sf| sf.peer_id == requester_peer_id));
    if !mutual_confirmed {
        return P2pResponse::Denied("Mutual friendship not confirmed".into());
    }
    if !is_timestamp_newer(&relay.payload.updated_at, known_updated_at) {
        return P2pResponse::CachedFriendNotNewer;
    }
    P2pResponse::CachedFriendProfile(relay.clone())
}

/// Collects local collection IDs (`active:<game_id>` or `saved:<id>`) that are
/// currently linked via `friend_sync` to the given remote `peer_id`.
pub fn collect_synced_collection_ids(paths: &Paths, peer_id: &str) -> Vec<String> {
    let mut ids = HashSet::new();
    if let Ok(cfg) = load_config(paths) {
        for game in &cfg.managed_games {
            if let Ok(cols) = mods::load_collections(paths, &game.id) {
                for col in cols.collections {
                    if let Some(sync) = col.friend_sync {
                        if sync.friend_peer_id == peer_id && !sync.friend_collection_id.trim().is_empty() {
                            ids.insert(sync.friend_collection_id);
                        }
                    }
                }
            }
        }
    }
    let mut out: Vec<String> = ids.into_iter().collect();
    out.sort();
    out.truncate(MAX_REMOTE_COLLECTIONS);
    out
}

/// Sanitizes and filters incoming `synced_collection_ids` reported by a friend
/// so only non-empty, visible collection IDs are stored.
fn sanitize_synced_collection_ids(
    store: &FriendsStore,
    remote_id: &str,
    ids: Vec<String>,
) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for raw in ids.into_iter().take(MAX_REMOTE_COLLECTIONS) {
        let clean = sanitize_short_text(&raw, MAX_COLLECTION_ID_CHARS);
        if clean.is_empty() || !is_collection_visible_to(store, &clean, Some(remote_id)) {
            continue;
        }
        if seen.insert(clean.clone()) {
            out.push(clean);
        }
    }
    out.sort();
    out
}

pub fn list_local_shareable_collections(
    paths: &Paths,
    store: &FriendsStore,
) -> Vec<LocalShareableCollection> {
    let mut out = Vec::new();
    if let Ok(cfg) = load_config(paths) {
        for game in &cfg.managed_games {
            if let Ok(order) = mods::load_loadorder(paths, &game.id) {
                let (manifest, _, _) = share::build_manifest_from_loadorder_with_selections(
                    &order,
                    game,
                    Some(format!("{} Loadout", game.title)),
                    true,
                );
                if !manifest.mods.is_empty() {
                    let id = format!("active:{}", game.id);
                    let visibility = resolve_collection_visibility(store, &id);
                    out.push(LocalShareableCollection {
                        hidden: matches!(visibility, CollectionVisibility::Private),
                        visibility,
                        id,
                        kind: FriendCollectionKind::ActiveLoadout,
                        name: format!("{} Loadout", game.title),
                        game_title: game.title.clone(),
                        mod_count: manifest.mods.len(),
                    });
                }
            }
        }
    }
    if let Ok(saved) = share::load_saved_collections(paths) {
        for entry in saved.collections {
            let id = format!("saved:{}", entry.id);
            let visibility = resolve_collection_visibility(store, &id);
            out.push(LocalShareableCollection {
                hidden: matches!(visibility, CollectionVisibility::Private),
                visibility,
                id,
                kind: FriendCollectionKind::SavedCollection,
                name: entry.name,
                game_title: entry
                    .game
                    .title
                    .unwrap_or_else(|| "Saved Collection".to_string()),
                mod_count: entry.mod_count,
            });
        }
    }
    out
}

fn resolve_active_synced_source_collection_id(
    paths: &Paths,
    game_id: &str,
    active_manifest: &share::ShareManifest,
    order: &mods::LoadOrder,
    viewer_peer_id: Option<&str>,
) -> Option<String> {
    let viewer = viewer_peer_id?;
    let installed = mods::list_installed_collections(paths, game_id).ok()?;
    let enabled_ids: HashSet<&str> = order
        .mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.id.as_str())
        .collect();
    for col in installed {
        let Some(sync) = col.friend_sync.as_ref() else {
            continue;
        };
        if sync.friend_peer_id != viewer || sync.friend_collection_id.trim().is_empty() {
            continue;
        }
        let matches_manifest = col
            .profile_code
            .as_deref()
            .and_then(|c| share::decode_share_code(c).ok())
            .is_some_and(|m| share::manifest_mods_match(&active_manifest.mods, &m.mods));
        let matches_mod_ids = !col.mod_ids.is_empty()
            && col.mod_ids.len() == enabled_ids.len()
            && col.mod_ids.iter().all(|id| enabled_ids.contains(id.as_str()));
        if matches_manifest || matches_mod_ids {
            return Some(sync.friend_collection_id.clone());
        }
    }
    None
}

#[cfg(test)]
pub fn build_local_profile(
    paths: &Paths,
    peer_id: &str,
    store: &FriendsStore,
    known_hashes: Option<&HashMap<String, String>>,
    viewer_peer_id: Option<&str>,
    signing_key: Option<&SecretKey>,
) -> PeerProfileSummary {
    build_local_profile_with_since(
        paths,
        peer_id,
        store,
        known_hashes,
        viewer_peer_id,
        signing_key,
        None,
    )
}

pub fn build_local_profile_with_since(
    paths: &Paths,
    peer_id: &str,
    store: &FriendsStore,
    known_hashes: Option<&HashMap<String, String>>,
    viewer_peer_id: Option<&str>,
    signing_key: Option<&SecretKey>,
    activity_since: Option<&str>,
) -> PeerProfileSummary {
    let mut collections = Vec::new();
    let mut relay_collections = Vec::new();
    let now = now_rfc3339();

    if let Ok(cfg) = load_config(paths) {
        for game in &cfg.managed_games {
            let col_id = format!("active:{}", game.id);
            let vis = resolve_collection_visibility(store, &col_id);
            let visible_to_viewer = is_collection_visible_to(store, &col_id, viewer_peer_id);
            let is_all_friends = matches!(vis, CollectionVisibility::AllFriends);
            if !visible_to_viewer && !(signing_key.is_some() && is_all_friends) {
                continue;
            }
            if let Ok(order) = mods::load_loadorder(paths, &game.id) {
                let name = format!("{} Loadout", game.title);
                let (mut manifest, _, _) = share::build_manifest_from_loadorder_with_selections(
                    &order,
                    game,
                    Some(name.clone()),
                    true,
                );
                if manifest.mods.is_empty() {
                    continue;
                }
                manifest.game_versions = game.game_version.as_ref().map(|v| vec![v.clone()]);
                if let Ok(code) = share::encode_share_manifest(&manifest) {
                    let hash = share::share_hash(&code);
                    if signing_key.is_some() && is_all_friends {
                        relay_collections.push(FriendShareRef {
                            id: col_id.clone(),
                            kind: FriendCollectionKind::ActiveLoadout,
                            name: name.clone(),
                            share_code: code.clone(),
                            share_hash: hash.clone(),
                            updated_at: now.clone(),
                            specific_friends_only: false,
                            synced_source_collection_id: None,
                            matched_local_collection: None,
                        });
                    }
                    if visible_to_viewer {
                        let unchanged = known_hashes
                            .and_then(|m| m.get(&col_id))
                            .is_some_and(|h| h == &hash);
                        let synced_source_collection_id =
                            resolve_active_synced_source_collection_id(
                                paths,
                                &game.id,
                                &manifest,
                                &order,
                                viewer_peer_id,
                            );
                        collections.push(FriendShareRef {
                            id: col_id,
                            kind: FriendCollectionKind::ActiveLoadout,
                            name,
                            share_code: if unchanged { String::new() } else { code },
                            share_hash: hash,
                            updated_at: now.clone(),
                            specific_friends_only: matches!(vis, CollectionVisibility::SpecificFriends { .. }),
                            synced_source_collection_id,
                            matched_local_collection: None,
                        });
                    }
                }
            }
        }
    }

    if let Ok(saved) = share::load_saved_collections(paths) {
        for entry in saved.collections {
            let col_id = format!("saved:{}", entry.id);
            let vis = resolve_collection_visibility(store, &col_id);
            let visible_to_viewer = is_collection_visible_to(store, &col_id, viewer_peer_id);
            let is_all_friends = matches!(vis, CollectionVisibility::AllFriends);
            if !visible_to_viewer && !(signing_key.is_some() && is_all_friends) {
                continue;
            }
            let hash = share::share_hash(&entry.code);
            if signing_key.is_some() && is_all_friends {
                relay_collections.push(FriendShareRef {
                    id: col_id.clone(),
                    kind: FriendCollectionKind::SavedCollection,
                    name: entry.name.clone(),
                    share_code: entry.code.clone(),
                    share_hash: hash.clone(),
                    updated_at: entry.updated_at.clone(),
                    specific_friends_only: false,
                    synced_source_collection_id: None,
                    matched_local_collection: None,
                });
            }
            if visible_to_viewer {
                let unchanged = known_hashes
                    .and_then(|m| m.get(&col_id))
                    .is_some_and(|h| h == &hash);
                collections.push(FriendShareRef {
                    id: col_id,
                    kind: FriendCollectionKind::SavedCollection,
                    name: entry.name,
                    share_code: if unchanged { String::new() } else { entry.code },
                    share_hash: hash,
                    updated_at: entry.updated_at,
                    specific_friends_only: matches!(vis, CollectionVisibility::SpecificFriends { .. }),
                    synced_source_collection_id: None,
                    matched_local_collection: None,
                });
            }
        }
    }

    let build_shared_friends = |exclude_peer: Option<&str>, include_live_meta: bool| -> Vec<SharedFriendRef> {
        store
            .friends
            .iter()
            .filter(|f| {
                f.status == FriendStatus::Connected
                    && exclude_peer.is_none_or(|p| f.peer_id != p)
                    && f.cached_profile
                        .as_ref()
                        .is_some_and(|p| p.allow_mutual_connections)
            })
            .take(MAX_REMOTE_FRIENDS)
            .map(|f| SharedFriendRef {
                peer_id: f.peer_id.clone(),
                display_name: sanitize_display_name(&f.display_name, "Friend"),
                addr: shared_friend_addr(&f.addr),
                profile_updated_at: if include_live_meta && store.allow_cache_relay {
                    f.cached_profile
                        .as_ref()
                        .and_then(|p| p.signed_relay_profile.as_ref())
                        .map(|r| r.payload.updated_at.clone())
                } else {
                    None
                },
                online: if include_live_meta {
                    Some(f.online)
                } else {
                    None
                },
            })
            .collect()
    };

    let friends = if is_friends_list_visible_to(store, viewer_peer_id) {
        Some(build_shared_friends(viewer_peer_id, true))
    } else {
        None
    };

    let display_name = effective_display_name(store);
    let visible_col_ids: HashSet<&str> = collections.iter().map(|c| c.id.as_str()).collect();
    let mut direct_activity_events: Vec<LocalActivityEvent> = store
        .local_activity
        .iter()
        .filter(|ev| {
            (store.share_active_loadouts_in_feed
                || ev.collection_kind != FriendCollectionKind::ActiveLoadout)
                && visible_col_ids.contains(ev.collection_id.as_str())
                && is_collection_visible_to(store, &ev.collection_id, viewer_peer_id)
                && activity_since
                    .is_none_or(|since| is_timestamp_at_or_after(&ev.occurred_at, since))
        })
        .take(MAX_FEED_EVENTS)
        .cloned()
        .collect();
    dedupe_active_loadout_local_events(&mut direct_activity_events);

    let signed_relay_profile = signing_key.and_then(|key| {
        let relay_friends = if is_friends_list_visible_to(store, None) {
            Some(build_shared_friends(None, false))
        } else {
            None
        };
        let relay_col_ids: HashSet<&str> = relay_collections.iter().map(|c| c.id.as_str()).collect();
        let mut relay_activity_events: Vec<LocalActivityEvent> = store
            .local_activity
            .iter()
            .filter(|ev| {
                (store.share_active_loadouts_in_feed
                    || ev.collection_kind != FriendCollectionKind::ActiveLoadout)
                    && relay_col_ids.contains(ev.collection_id.as_str())
                    && matches!(
                        resolve_collection_visibility(store, &ev.collection_id),
                        CollectionVisibility::AllFriends
                    )
            })
            .take(MAX_FEED_EVENTS)
            .cloned()
            .collect();
        dedupe_active_loadout_local_events(&mut relay_activity_events);
        let payload = RelayProfilePayload {
            peer_id: peer_id.to_string(),
            display_name: display_name.clone(),
            profile_picture: store.profile_picture.clone(),
            updated_at: now.clone(),
            collections: relay_collections,
            friends: relay_friends,
            allow_mutual_connections: store.allow_mutual_connections,
            activity_events: Some(relay_activity_events),
        };
        sign_relay_profile(key, payload)
            .map_err(|e| log::warn!("Failed to sign relay profile: {e}"))
            .ok()
    });

    let relayed_invites: Vec<SignedFriendInvite> = match viewer_peer_id {
        Some(viewer) => store
            .relayed_invites
            .iter()
            .filter(|inv| {
                inv.payload.to_peer_id == viewer
                    && !is_relayed_invite_expired(&inv.payload.created_at)
            })
            .take(MAX_RELAYED_INVITES_PER_TARGET)
            .cloned()
            .collect(),
        None => Vec::new(),
    };

    PeerProfileSummary {
        peer_id: peer_id.to_string(),
        display_name,
        profile_picture: store.profile_picture.clone(),
        updated_at: now,
        collections,
        friends,
        allow_mutual_connections: store.allow_mutual_connections,
        signed_relay_profile,
        activity_events: Some(direct_activity_events),
        relayed_invites,
    }
}

async fn write_frame<T: Serialize>(send: &mut iroh::endpoint::SendStream, msg: &T) -> Result<()> {
    let data = serde_json::to_vec(msg).context("serialize p2p frame")?;
    if data.len() > MAX_FRAME_BYTES {
        bail!("p2p frame too large");
    }
    let len = (data.len() as u32).to_be_bytes();
    tokio::time::timeout(STREAM_IO_TIMEOUT, async {
        send.write_all(&len).await?;
        send.write_all(&data).await?;
        send.finish()?;
        Ok::<_, anyhow::Error>(())
    })
    .await
    .map_err(|_| anyhow!("p2p frame write timed out"))??;
    Ok(())
}

async fn read_frame<T: for<'de> Deserialize<'de>>(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<T> {
    tokio::time::timeout(STREAM_IO_TIMEOUT, async {
        let mut len_buf = [0u8; 4];
        recv.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_FRAME_BYTES {
            bail!("incoming p2p frame exceeds max size");
        }
        // Allocate incrementally so a 4-byte length prefix cannot trigger an upfront 4 MB allocation.
        let mut buf = Vec::with_capacity(len.min(64 * 1024));
        let mut chunk = [0u8; 16 * 1024];
        while buf.len() < len {
            let to_read = (len - buf.len()).min(chunk.len());
            match recv.read(&mut chunk[..to_read]).await? {
                Some(0) | None => bail!("unexpected EOF while reading p2p frame"),
                Some(n) => buf.extend_from_slice(&chunk[..n]),
            }
        }
        serde_json::from_slice(&buf).context("decode p2p frame")
    })
    .await
    .map_err(|_| anyhow!("p2p frame read timed out"))?
}

struct CheckingGuard<'a>(&'a P2pService);

impl<'a> CheckingGuard<'a> {
    fn new(svc: &'a P2pService) -> Self {
        svc.emit_checking(true);
        Self(svc)
    }
}

impl Drop for CheckingGuard<'_> {
    fn drop(&mut self) {
        self.0.emit_checking(false);
    }
}

struct PeerConnGuard<'a> {
    service: &'a P2pService,
    peer_id: String,
}

impl Drop for PeerConnGuard<'_> {
    fn drop(&mut self) {
        if let Ok(mut counts) = self.service.active_peer_conns.lock() {
            if let Some(count) = counts.get_mut(&self.peer_id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    counts.remove(&self.peer_id);
                }
            }
        }
    }
}

pub struct P2pService {
    paths: Paths,
    store: Arc<Mutex<FriendsStore>>,
    endpoint: OnceCell<Endpoint>,
    secret_key: OnceCell<SecretKey>,
    app: Mutex<Option<AppHandle>>,
    incoming_sem: Arc<Semaphore>,
    hello_limiter: Mutex<HelloRateLimiter>,
    profile_limiter: Mutex<PeerProfileRateLimiter>,
    active_peer_conns: Mutex<HashMap<String, usize>>,
}

struct LocalShareManifestEntry {
    id: String,
    name: String,
    game_id: Option<String>,
    manifest: share::ShareManifest,
}

fn find_local_share_by_id<'a>(
    saved: &'a [LocalShareManifestEntry],
    active: &'a [LocalShareManifestEntry],
    id: &str,
) -> Option<&'a LocalShareManifestEntry> {
    saved
        .iter()
        .chain(active.iter())
        .find(|entry| entry.id == id)
}

fn resolve_friend_active_loadouts(paths: &Paths, friends: &mut [FriendRecord]) {
    let has_active_loadouts = friends.iter().any(|f| {
        f.cached_profile.as_ref().is_some_and(|p| {
            p.collections
                .iter()
                .any(|c| c.kind == FriendCollectionKind::ActiveLoadout)
        })
    });
    if !has_active_loadouts {
        return;
    }

    let mut saved_entries = Vec::new();
    if let Ok(saved) = share::load_saved_collections(paths) {
        for entry in saved.collections {
            if let Ok(manifest) = share::decode_share_code(&entry.code) {
                saved_entries.push(LocalShareManifestEntry {
                    id: format!("saved:{}", entry.id),
                    name: entry.name,
                    game_id: entry.source_game_id,
                    manifest,
                });
            }
        }
    }

    let mut active_entries = Vec::new();
    let mut installed_entries = Vec::new();
    if let Ok(cfg) = load_config(paths) {
        for game in &cfg.managed_games {
            if let Ok(order) = mods::load_loadorder(paths, &game.id) {
                let name = format!("{} Loadout", game.title);
                let (manifest, _, _) = share::build_manifest_from_loadorder_with_selections(
                    &order,
                    game,
                    Some(name.clone()),
                    true,
                );
                if !manifest.mods.is_empty() {
                    active_entries.push(LocalShareManifestEntry {
                        id: format!("active:{}", game.id),
                        name,
                        game_id: Some(game.id.clone()),
                        manifest,
                    });
                }
            }
            if let Ok(installed) = mods::list_installed_collections(paths, &game.id) {
                for col in installed {
                    if let Some(code) = col.profile_code.as_deref() {
                        if let Ok(manifest) = share::decode_share_code(code) {
                            installed_entries.push(LocalShareManifestEntry {
                                id: format!("installed:{}:{}", game.id, col.id),
                                name: col.name,
                                game_id: Some(game.id.clone()),
                                manifest,
                            });
                        }
                    }
                }
            }
        }
    }

    for friend in friends.iter_mut() {
        let syncing_ids: HashSet<String> = friend.syncing_my_collections.iter().cloned().collect();
        let Some(profile) = friend.cached_profile.as_mut() else {
            continue;
        };
        for col in profile.collections.iter_mut() {
            if col.kind != FriendCollectionKind::ActiveLoadout {
                col.matched_local_collection = None;
                continue;
            }
            let friend_manifest = (!col.share_code.is_empty())
                .then(|| share::decode_share_code(&col.share_code).ok())
                .flatten();

            if let Some(src_id) = col.synced_source_collection_id.as_deref() {
                if let Some(local) = find_local_share_by_id(&saved_entries, &active_entries, src_id)
                {
                    let has_local_changes = friend_manifest.as_ref().is_some_and(|fm| {
                        !share::manifest_mods_match(&fm.mods, &local.manifest.mods)
                    });
                    col.matched_local_collection = Some(FriendMatchedLocalCollection {
                        id: local.id.clone(),
                        name: local.name.clone(),
                        kind: FriendCollectionMatchKind::SyncedToYours,
                        game_id: local.game_id.clone(),
                        has_local_changes,
                    });
                    continue;
                }
            }

            let Some(fm) = friend_manifest.as_ref() else {
                col.matched_local_collection = None;
                continue;
            };

            if let Some(local) = saved_entries
                .iter()
                .chain(active_entries.iter())
                .find(|entry| {
                    syncing_ids.contains(&entry.id)
                        && share::manifest_mods_match(&fm.mods, &entry.manifest.mods)
                })
            {
                col.matched_local_collection = Some(FriendMatchedLocalCollection {
                    id: local.id.clone(),
                    name: local.name.clone(),
                    kind: FriendCollectionMatchKind::SyncedToYours,
                    game_id: local.game_id.clone(),
                    has_local_changes: false,
                });
                continue;
            }

            if let Some(saved) = saved_entries
                .iter()
                .find(|entry| share::manifest_mods_match(&fm.mods, &entry.manifest.mods))
            {
                col.matched_local_collection = Some(FriendMatchedLocalCollection {
                    id: saved.id.clone(),
                    name: saved.name.clone(),
                    kind: FriendCollectionMatchKind::MatchesSaved,
                    game_id: saved.game_id.clone(),
                    has_local_changes: false,
                });
                continue;
            }

            if let Some(installed) = installed_entries
                .iter()
                .find(|entry| share::manifest_mods_match(&fm.mods, &entry.manifest.mods))
            {
                col.matched_local_collection = Some(FriendMatchedLocalCollection {
                    id: installed.id.clone(),
                    name: installed.name.clone(),
                    kind: FriendCollectionMatchKind::MatchesInstalled,
                    game_id: installed.game_id.clone(),
                    has_local_changes: false,
                });
                continue;
            }

            if let Some(active) = active_entries
                .iter()
                .find(|entry| share::manifest_mods_match(&fm.mods, &entry.manifest.mods))
            {
                col.matched_local_collection = Some(FriendMatchedLocalCollection {
                    id: active.id.clone(),
                    name: active.name.clone(),
                    kind: FriendCollectionMatchKind::MatchesActiveLoadout,
                    game_id: active.game_id.clone(),
                    has_local_changes: false,
                });
                continue;
            }

            col.matched_local_collection = None;
        }
    }
}

impl P2pService {
    pub fn new(paths: Paths) -> Arc<Self> {
        let store = load_friends_store(&paths);
        Arc::new(Self {
            paths,
            store: Arc::new(Mutex::new(store)),
            endpoint: OnceCell::new(),
            secret_key: OnceCell::new(),
            app: Mutex::new(None),
            incoming_sem: Arc::new(Semaphore::new(MAX_CONCURRENT_INCOMING)),
            hello_limiter: Mutex::new(HelloRateLimiter::default()),
            profile_limiter: Mutex::new(PeerProfileRateLimiter::default()),
            active_peer_conns: Mutex::new(HashMap::new()),
        })
    }

    pub fn start(self: &Arc<Self>, app: AppHandle) {
        if let Ok(mut guard) = self.app.lock() {
            *guard = Some(app);
        }
        let svc = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            if let Err(e) = svc.init_and_serve().await {
                log::warn!("P2P endpoint startup failed: {e:#}");
            }
        });
    }

    /// Scans local active loadouts and saved collections, recording any new
    /// author-side activity events with the current timestamp.
    pub fn sync_local_activity(&self) -> Result<bool> {
        self.sync_local_activity_at(&now_rfc3339())
    }

    fn sync_local_activity_at(&self, now: &str) -> Result<bool> {
        let current = collect_current_local_collections(&self.paths);
        let mutated = {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            let mutated = record_local_activity_in_store(&mut store, current, now);
            if mutated {
                let _ = save_friends_store(&self.paths, &store);
            }
            mutated
        };
        if mutated {
            self.emit_changed();
        }
        Ok(mutated)
    }

    /// Spawns a non-blocking background scan to record local activity right
    /// after a collection or loadout mutation.
    pub fn trigger_local_activity_sync(self: &Arc<Self>) {
        let svc = Arc::clone(self);
        tauri::async_runtime::spawn_blocking(move || {
            let _ = svc.sync_local_activity();
        });
    }

    async fn init_and_serve(self: Arc<Self>) -> Result<()> {
        let svc_init = Arc::clone(&self);
        let _ = tokio::task::spawn_blocking(move || {
            let _ = svc_init.sync_local_activity();
        })
        .await;
        let key = load_or_create_secret_key(&self.paths)?;
        let _ = self.secret_key.set(key.clone());
        let ep = Endpoint::builder(presets::N0)
            .secret_key(key)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .context("bind iroh endpoint")?;
        let _ = self.endpoint.set(ep.clone());
        let _ = tokio::time::timeout(Duration::from_secs(3), ep.online()).await;
        self.emit_changed();

        // Kick off initial friend presence & collection sync, then keepalive every 25s.
        let sync_svc = Arc::clone(&self);
        tauri::async_runtime::spawn(async move {
            let _ = sync_svc.refresh_all_friends().await;
            let mut interval = tokio::time::interval(Duration::from_secs(25));
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                interval.tick().await;
                let _ = sync_svc.heartbeat_friends().await;
            }
        });

        while let Some(incoming) = ep.accept().await {
            let Ok(permit) = self.incoming_sem.clone().try_acquire_owned() else {
                log::debug!("P2P incoming connection dropped: concurrency limit reached");
                drop(incoming);
                continue;
            };
            let svc = Arc::clone(&self);
            tauri::async_runtime::spawn(async move {
                let _permit = permit;
                if let Err(e) = svc.handle_incoming(incoming).await {
                    log::debug!("P2P incoming connection error: {e:#}");
                }
            });
        }
        Ok(())
    }

    fn emit_changed(&self) {
        if let Ok(guard) = self.app.lock() {
            if let Some(app) = guard.as_ref() {
                let _ = app.emit("friends-changed", ());
            }
        }
    }

    fn emit_checking(&self, checking: bool) {
        if let Ok(guard) = self.app.lock() {
            if let Some(app) = guard.as_ref() {
                let _ = app.emit("friends-checking", checking);
            }
        }
    }

    fn is_appear_offline(&self) -> bool {
        self.store
            .lock()
            .map(|store| store.appear_offline)
            .unwrap_or(false)
    }

    async fn handle_incoming(self: &Arc<Self>, incoming: Incoming) -> Result<()> {
        if self.is_appear_offline() {
            drop(incoming);
            return Ok(());
        }
        let conn = tokio::time::timeout(DIAL_TIMEOUT, incoming)
            .await
            .map_err(|_| anyhow!("incoming handshake timed out"))??;
        let remote_endpoint_id = conn.remote_id();
        let remote_id = remote_endpoint_id.to_string();

        let acquired_peer_slot = {
            let mut counts = self.active_peer_conns.lock().map_err(|e| anyhow!("{e}"))?;
            let count = counts.entry(remote_id.clone()).or_insert(0);
            if *count >= MAX_CONCURRENT_PER_PEER {
                false
            } else {
                *count += 1;
                true
            }
        };
        if !acquired_peer_slot {
            log::debug!("Rejected P2P connection from {remote_id}: per-peer limit reached");
            return Ok(());
        }
        let _peer_conn_guard = PeerConnGuard {
            service: self,
            peer_id: remote_id.clone(),
        };

        let mut stream_count = 0usize;
        while stream_count < MAX_STREAMS_PER_CONN {
            let Ok(Ok((mut send, mut recv))) =
                tokio::time::timeout(STREAM_IO_TIMEOUT, conn.accept_bi()).await
            else {
                break;
            };
            stream_count += 1;

            let req: P2pRequest = match read_frame(&mut recv).await {
                Ok(r) => r,
                Err(_) => break,
            };

            let Some(ep) = self.endpoint.get() else {
                let _ = write_frame(&mut send, &P2pResponse::Denied("Endpoint not ready".into())).await;
                break;
            };
            let local_addr = ep.addr();

            match req {
                P2pRequest::Hello { display_name, addr } => {
                    let sanitized_addr = match sanitize_peer_addr(remote_endpoint_id, &addr) {
                        Ok(a) => a,
                        Err(e) => {
                            log::debug!("Rejected P2P Hello with invalid addr from {remote_id}: {e}");
                            let _ = write_frame(
                                &mut send,
                                &P2pResponse::Denied("Invalid peer address".into()),
                            )
                            .await;
                            break;
                        }
                    };
                    let sanitized_name = sanitize_display_name(&display_name, "Friend");

                    let is_known_friend = self
                        .store
                        .lock()
                        .map(|store| {
                            store.friends.iter().any(|f| f.peer_id == remote_id)
                                || store.queued_invites.iter().any(|q| q.peer_id == remote_id)
                        })
                        .unwrap_or(false);
                    if !is_known_friend {
                        let allowed = self
                            .hello_limiter
                            .lock()
                            .map(|mut lim| lim.allow(&remote_id))
                            .unwrap_or(true);
                        if !allowed {
                            let _ = write_frame(
                                &mut send,
                                &P2pResponse::Denied("Too many requests".into()),
                            )
                            .await;
                            break;
                        }
                    }

                    let (accepted, needs_profile_pull, local_name, should_save) = {
                        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                        let local_name = effective_display_name(&store);
                        if let Some(friend) = store.friends.iter_mut().find(|f| f.peer_id == remote_id)
                        {
                            let was_pending = friend.status == FriendStatus::PendingOutgoing;
                            let missing_profile = friend.cached_profile.is_none();
                            friend.status = FriendStatus::Connected;
                            friend.display_name = sanitized_name;
                            friend.addr = sanitized_addr;
                            friend.online = true;
                            friend.last_seen_at = Some(now_rfc3339());
                            store.queued_invites.retain(|q| q.peer_id != remote_id);
                            (true, was_pending || missing_profile, local_name, true)
                        } else if let Some(q_pos) = store
                            .queued_invites
                            .iter()
                            .position(|q| q.peer_id == remote_id)
                        {
                            store.queued_invites.remove(q_pos);
                            store.pending_requests.retain(|r| r.peer_id != remote_id);
                            let now = now_rfc3339();
                            store.friends.push(FriendRecord {
                                peer_id: remote_id.clone(),
                                display_name: sanitized_name,
                                profile_picture: None,
                                addr: sanitized_addr,
                                status: FriendStatus::Connected,
                                online: true,
                                last_seen_at: Some(now.clone()),
                                added_at: Some(now),
                                cached_profile: None,
                                syncing_my_collections: Vec::new(),
                                cached_via_peer_name: None,
                            });
                            (true, true, local_name, true)
                        } else {
                            let mutated = if let Some(existing) = store
                                .pending_requests
                                .iter_mut()
                                .find(|r| r.peer_id == remote_id)
                            {
                                existing.display_name = sanitized_name;
                                existing.addr = sanitized_addr;
                                true
                            } else if store.pending_requests.len() < MAX_PENDING_REQUESTS {
                                store.pending_requests.push(PendingFriendRequest {
                                    peer_id: remote_id.clone(),
                                    display_name: sanitized_name,
                                    profile_picture: None,
                                    addr: sanitized_addr,
                                    received_at: now_rfc3339(),
                                });
                                true
                            } else {
                                log::debug!(
                                    "Ignored incoming friend request from {remote_id}: pending queue full ({MAX_PENDING_REQUESTS})"
                                );
                                false
                            };
                            (false, false, local_name, mutated)
                        }
                    };
                    if should_save {
                        if let Ok(store) = self.store.lock() {
                            let _ = save_friends_store(&self.paths, &store);
                        }
                        self.emit_changed();
                    }
                    write_frame(
                        &mut send,
                        &P2pResponse::HelloAck {
                            accepted,
                            display_name: local_name,
                            addr: local_addr,
                        },
                    )
                    .await?;

                    if accepted && needs_profile_pull {
                        let svc = Arc::clone(self);
                        let pid = remote_id.clone();
                        tauri::async_runtime::spawn(async move {
                            let _ = svc.sync_peer_profile_only(&pid).await;
                        });
                    }
                }
                P2pRequest::GetProfile {
                    known_hashes,
                    synced_collection_ids,
                    activity_since,
                } => {
                    let allowed = self
                        .profile_limiter
                        .lock()
                        .map(|mut lim| lim.allow(&remote_id))
                        .unwrap_or(true);
                    if !allowed {
                        write_frame(
                            &mut send,
                            &P2pResponse::Denied("Too many requests".into()),
                        )
                        .await?;
                        break;
                    }
                    let sanitized_since = activity_since
                        .as_deref()
                        .filter(|ts| chrono::DateTime::parse_from_rfc3339(ts).is_ok())
                        .map(str::to_string);
                    let (is_friend, status_flipped, sync_list_changed) = {
                        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                        let is_friend = store
                            .friends
                            .iter()
                            .any(|f| f.peer_id == remote_id && f.status == FriendStatus::Connected);
                        if !is_friend {
                            (false, false, false)
                        } else {
                            let sanitized_sync_ids = synced_collection_ids.map(|ids| {
                                sanitize_synced_collection_ids(&store, &remote_id, ids)
                            });
                            let mut flipped = false;
                            let mut sync_changed = false;
                            if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == remote_id) {
                                if !f.online {
                                    flipped = true;
                                }
                                f.online = true;
                                f.last_seen_at = Some(now_rfc3339());
                                if let Some(next_ids) = sanitized_sync_ids {
                                    if f.syncing_my_collections != next_ids {
                                        f.syncing_my_collections = next_ids;
                                        sync_changed = true;
                                    }
                                }
                            }
                            if sync_changed {
                                let _ = save_friends_store(&self.paths, &store);
                            }
                            (true, flipped, sync_changed)
                        }
                    };
                    if status_flipped || sync_list_changed {
                        self.emit_changed();
                    }
                    if is_friend {
                        let svc = Arc::clone(self);
                        let local_id = ep.id().to_string();
                        let viewer = remote_id.clone();
                        let signing_key = self.secret_key.get().cloned();
                        let profile = tokio::task::spawn_blocking(move || -> Result<PeerProfileSummary> {
                            let _ = svc.sync_local_activity();
                            let snapshot_store = svc.store.lock().map_err(|e| anyhow!("{e}"))?.clone();
                            Ok(build_local_profile_with_since(
                                &svc.paths,
                                &local_id,
                                &snapshot_store,
                                Some(&known_hashes),
                                Some(&viewer),
                                signing_key.as_ref(),
                                sanitized_since.as_deref(),
                            ))
                        })
                        .await
                        .context("build local profile task failed")??;
                        write_frame(&mut send, &P2pResponse::Profile(profile)).await?;
                    } else {
                        write_frame(
                            &mut send,
                            &P2pResponse::Denied("Not connected as friends".into()),
                        )
                        .await?;
                    }
                }
                P2pRequest::GetCachedFriendProfile {
                    target_peer_id,
                    known_updated_at,
                } => {
                    let allowed = self
                        .profile_limiter
                        .lock()
                        .map(|mut lim| lim.allow(&remote_id))
                        .unwrap_or(true);
                    if !allowed {
                        write_frame(
                            &mut send,
                            &P2pResponse::Denied("Too many requests".into()),
                        )
                        .await?;
                        break;
                    }
                    let resp = {
                        let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                        resolve_cached_friend_profile_for_peer(
                            &store,
                            &remote_id,
                            &target_peer_id,
                            known_updated_at.as_deref(),
                        )
                    };
                    write_frame(&mut send, &resp).await?;
                }
                P2pRequest::DepositFriendInvite { invite } => {
                    let allowed = self
                        .profile_limiter
                        .lock()
                        .map(|mut lim| lim.allow(&remote_id))
                        .unwrap_or(true);
                    if !allowed {
                        write_frame(
                            &mut send,
                            &P2pResponse::Denied("Too many requests".into()),
                        )
                        .await?;
                        break;
                    }
                    let resp = {
                        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                        match deposit_relayed_invite_into_store(&mut store, &remote_id, invite) {
                            Ok(()) => {
                                let _ = save_friends_store(&self.paths, &store);
                                P2pResponse::RelayedInviteStored
                            }
                            Err(msg) => P2pResponse::Denied(msg),
                        }
                    };
                    write_frame(&mut send, &resp).await?;
                }
                P2pRequest::CancelRelayedInvite { target_peer_id } => {
                    let allowed = self
                        .profile_limiter
                        .lock()
                        .map(|mut lim| lim.allow(&remote_id))
                        .unwrap_or(true);
                    if !allowed {
                        write_frame(
                            &mut send,
                            &P2pResponse::Denied("Too many requests".into()),
                        )
                        .await?;
                        break;
                    }
                    {
                        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                        let before = store.relayed_invites.len();
                        store.relayed_invites.retain(|inv| {
                            !(inv.payload.from_peer_id == remote_id
                                && inv.payload.to_peer_id == target_peer_id)
                        });
                        if store.relayed_invites.len() != before {
                            let _ = save_friends_store(&self.paths, &store);
                        }
                    }
                    write_frame(&mut send, &P2pResponse::RelayedInviteCancelled).await?;
                }
                P2pRequest::AckRelayedInvites { from_peer_ids } => {
                    let allowed = self
                        .profile_limiter
                        .lock()
                        .map(|mut lim| lim.allow(&remote_id))
                        .unwrap_or(true);
                    if !allowed {
                        write_frame(
                            &mut send,
                            &P2pResponse::Denied("Too many requests".into()),
                        )
                        .await?;
                        break;
                    }
                    let acked: HashSet<String> = from_peer_ids
                        .into_iter()
                        .take(MAX_RELAYED_INVITES_PER_TARGET)
                        .collect();
                    if !acked.is_empty() {
                        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                        let before = store.relayed_invites.len();
                        store.relayed_invites.retain(|inv| {
                            !(inv.payload.to_peer_id == remote_id
                                && acked.contains(&inv.payload.from_peer_id))
                        });
                        if store.relayed_invites.len() != before {
                            let _ = save_friends_store(&self.paths, &store);
                        }
                    }
                    write_frame(&mut send, &P2pResponse::RelayedInvitesAcked).await?;
                }
            }
        }
        Ok(())
    }

    pub fn get_state(&self) -> Result<FriendsState> {
        let (
            endpoint_ready,
            peer_id,
            join_code,
            appear_offline,
            display_name,
            display_name_set,
            profile_picture,
            friends_list_visibility,
            allow_mutual_connections,
            allow_cache_relay,
            share_active_loadouts_in_feed,
            mut friends,
            pending_requests,
            queued_invites,
            local_collections,
            feed_events,
            feed_last_read_at,
        ) = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            let (endpoint_ready, peer_id, join_code) = match self.endpoint.get() {
                Some(ep) => {
                    let addr = join_ticket_addr(&ep.addr());
                    let pid = ep.id().to_string();
                    let ticket = FriendJoinTicket {
                        addr,
                        name: effective_display_name(&store),
                    };
                    let code = encode_join_code(&ticket).ok();
                    (true, Some(pid), code)
                }
                None => (false, None, None),
            };
            let local_collections = list_local_shareable_collections(&self.paths, &store);
            let feed_events =
                build_combined_feed_events(&store, &local_collections, peer_id.as_deref());
            (
                endpoint_ready,
                peer_id,
                join_code,
                store.appear_offline,
                store.display_name.clone(),
                store.display_name_set,
                store.profile_picture.clone(),
                store.friends_list_visibility.clone(),
                store.allow_mutual_connections,
                store.allow_cache_relay,
                store.share_active_loadouts_in_feed,
                store.friends.clone(),
                store.pending_requests.clone(),
                store.queued_invites.clone(),
                local_collections,
                feed_events,
                store.feed_last_read_at.clone(),
            )
        };
        resolve_friend_active_loadouts(&self.paths, &mut friends);
        Ok(FriendsState {
            endpoint_ready,
            appear_offline,
            peer_id,
            join_code,
            display_name,
            display_name_set,
            profile_picture,
            friends_list_visibility,
            allow_mutual_connections,
            allow_cache_relay,
            share_active_loadouts_in_feed,
            friends,
            pending_requests,
            queued_invites,
            local_collections,
            feed_events,
            feed_last_read_at,
        })
    }

    pub fn set_display_name(&self, name: String) -> Result<FriendsState> {
        let trimmed = sanitize_short_text(&name, MAX_DISPLAY_NAME_CHARS);
        if trimmed.is_empty() {
            bail!("Display name cannot be empty");
        }
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.display_name = trimmed;
            store.display_name_set = true;
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub fn set_appear_offline(self: &Arc<Self>, appear_offline: bool) -> Result<FriendsState> {
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.appear_offline = appear_offline;
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        if !appear_offline && self.endpoint.get().is_some() {
            let svc = Arc::clone(self);
            tauri::async_runtime::spawn(async move {
                let _ = svc.refresh_all_friends().await;
            });
        }
        self.get_state()
    }

    pub fn set_profile_picture(&self, data_url: Option<String>) -> Result<FriendsState> {
        let normalized = match data_url {
            Some(raw) => {
                let trimmed = raw.trim().to_string();
                if trimmed.is_empty() {
                    None
                } else {
                    validate_profile_picture(&trimmed)?;
                    Some(trimmed)
                }
            }
            None => None,
        };
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.profile_picture = normalized;
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub fn set_collection_sharing(
        &self,
        collection_id: String,
        mut visibility: CollectionVisibility,
    ) -> Result<FriendsState> {
        if let CollectionVisibility::SpecificFriends { ref mut peer_ids } = visibility {
            let mut seen = HashSet::new();
            peer_ids.retain(|id| seen.insert(id.clone()));
        }
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            if matches!(visibility, CollectionVisibility::Private) {
                store.hidden_collection_ids.insert(collection_id.clone());
            } else {
                store.hidden_collection_ids.remove(&collection_id);
            }
            store.collection_visibility.insert(collection_id, visibility);
            save_friends_store(&self.paths, &store)?;
        }
        let _ = self.sync_local_activity();
        self.emit_changed();
        self.get_state()
    }

    pub fn set_collection_visibility(&self, collection_id: String, hidden: bool) -> Result<FriendsState> {
        let visibility = if hidden {
            CollectionVisibility::Private
        } else {
            CollectionVisibility::AllFriends
        };
        self.set_collection_sharing(collection_id, visibility)
    }

    pub fn set_friends_list_visibility(
        &self,
        mut visibility: CollectionVisibility,
    ) -> Result<FriendsState> {
        if let CollectionVisibility::SpecificFriends { ref mut peer_ids } = visibility {
            let mut seen = HashSet::new();
            peer_ids.retain(|id| seen.insert(id.clone()));
        }
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.friends_list_visibility = visibility;
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub fn set_allow_mutual_connections(&self, allow: bool) -> Result<FriendsState> {
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.allow_mutual_connections = allow;
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub fn set_allow_cache_relay(&self, allow: bool) -> Result<FriendsState> {
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.allow_cache_relay = allow;
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub fn set_share_active_loadouts_in_feed(&self, share: bool) -> Result<FriendsState> {
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.share_active_loadouts_in_feed = share;
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    fn inspect_mutual_target_info(
        store: &FriendsStore,
        target_id: EndpointId,
    ) -> (
        Option<String>,
        Option<EndpointAddr>,
        Option<bool>,
        Vec<(String, EndpointAddr)>,
    ) {
        let canonical_id = target_id.to_string();
        let mut discovered_name: Option<String> = None;
        let mut discovered_addr: Option<EndpointAddr> = None;
        let mut addr_from_online = false;
        let mut saw_online = false;
        let mut saw_offline = false;
        let mut online_mutuals: Vec<(String, EndpointAddr)> = Vec::new();

        for friend in &store.friends {
            if friend.status != FriendStatus::Connected {
                continue;
            }
            let Some(ref profile) = friend.cached_profile else {
                continue;
            };
            let Some(ref shared_friends) = profile.friends else {
                continue;
            };
            let Some(entry) = shared_friends.iter().find(|sf| sf.peer_id == canonical_id) else {
                continue;
            };
            if discovered_name.is_none() && !entry.display_name.is_empty() {
                discovered_name = Some(entry.display_name.clone());
            }
            if friend.online {
                online_mutuals.push((friend.peer_id.clone(), friend.addr.clone()));
                match entry.online {
                    Some(true) => saw_online = true,
                    Some(false) => saw_offline = true,
                    None => {}
                }
            }
            if let Some(ref a) = entry.addr {
                if let Ok(sanitized) = sanitize_peer_addr(target_id, a) {
                    if !sanitized.addrs.is_empty()
                        && (discovered_addr.is_none() || (friend.online && !addr_from_online))
                    {
                        discovered_addr = Some(sanitized);
                        addr_from_online = friend.online;
                    }
                }
            }
        }

        let online_hint = if saw_online {
            Some(true)
        } else if saw_offline {
            Some(false)
        } else {
            None
        };
        (discovered_name, discovered_addr, online_hint, online_mutuals)
    }

    pub async fn add_friend_by_peer(
        &self,
        peer_id: String,
        display_name: Option<String>,
    ) -> Result<FriendsState> {
        let trimmed = peer_id.trim();
        let parsed_id: EndpointId = trimmed
            .parse()
            .map_err(|_| anyhow!("Invalid peer ID format"))?;
        let canonical_id = parsed_id.to_string();
        if let Some(ep) = self.endpoint.get() {
            if ep.id() == parsed_id {
                bail!("You cannot add yourself as a friend");
            }
        }

        let has_incoming_request = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store
                .pending_requests
                .iter()
                .any(|r| r.peer_id == canonical_id)
        };
        if has_incoming_request {
            return self.respond_friend_request(canonical_id, true).await;
        }

        enum EnqueueOutcome {
            AlreadyConnected,
            AlreadyPendingOutgoing,
            Queued { online_hint: Option<bool> },
        }

        let outcome = {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            if let Some(existing) = store.friends.iter().find(|f| f.peer_id == canonical_id) {
                if existing.status == FriendStatus::Connected {
                    EnqueueOutcome::AlreadyConnected
                } else {
                    EnqueueOutcome::AlreadyPendingOutgoing
                }
            } else {
                let (discovered_name, discovered_addr, online_hint, _) =
                    Self::inspect_mutual_target_info(&store, parsed_id);
                let final_name = display_name
                    .as_deref()
                    .map(|n| sanitize_display_name(n, ""))
                    .filter(|n| !n.is_empty())
                    .or(discovered_name)
                    .unwrap_or_else(|| "Friend".to_string());
                let final_addr = discovered_addr.unwrap_or_else(|| EndpointAddr::new(parsed_id));

                if let Some(existing_q) = store
                    .queued_invites
                    .iter_mut()
                    .find(|q| q.peer_id == canonical_id)
                {
                    if existing_q.display_name == "Friend" && final_name != "Friend" {
                        existing_q.display_name = final_name;
                    }
                    if !final_addr.addrs.is_empty() {
                        existing_q.addr = final_addr;
                    }
                } else {
                    if store.queued_invites.len() >= MAX_QUEUED_INVITES {
                        bail!("Outgoing friend request queue is full ({MAX_QUEUED_INVITES})");
                    }
                    store.queued_invites.push(QueuedFriendInvite {
                        peer_id: canonical_id.clone(),
                        display_name: final_name,
                        addr: final_addr,
                        queued_at: now_rfc3339(),
                        relayed_via_peer_ids: Vec::new(),
                    });
                }
                save_friends_store(&self.paths, &store)?;
                EnqueueOutcome::Queued { online_hint }
            }
        };

        self.emit_changed();
        match outcome {
            EnqueueOutcome::AlreadyConnected => {}
            EnqueueOutcome::AlreadyPendingOutgoing => {
                let _ = self.dial_and_sync_peer(&canonical_id).await;
            }
            EnqueueOutcome::Queued { online_hint } => {
                let mut delivered = false;
                if online_hint != Some(false) {
                    delivered = self
                        .deliver_queued_invite(&canonical_id)
                        .await
                        .unwrap_or(false);
                }
                if !delivered {
                    let _ = self.deposit_queued_invite_to_mutuals(&canonical_id).await;
                }
            }
        }
        self.get_state()
    }

    pub async fn cancel_queued_friend_invite(&self, peer_id: String) -> Result<FriendsState> {
        let canonical_id = peer_id
            .trim()
            .parse::<EndpointId>()
            .map(|id| id.to_string())
            .unwrap_or_else(|_| peer_id.trim().to_string());

        let relay_targets: Vec<EndpointAddr> = {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            let removed = store
                .queued_invites
                .iter()
                .position(|q| q.peer_id == canonical_id)
                .map(|i| store.queued_invites.remove(i));
            let Some(removed) = removed else {
                return self.get_state();
            };
            let targets = removed
                .relayed_via_peer_ids
                .iter()
                .filter_map(|mid| {
                    store
                        .friends
                        .iter()
                        .find(|f| {
                            f.peer_id == *mid && f.status == FriendStatus::Connected && f.online
                        })
                        .map(|f| f.addr.clone())
                })
                .collect();
            save_friends_store(&self.paths, &store)?;
            targets
        };
        self.emit_changed();

        if let Some(ep) = self.endpoint.get() {
            futures_util::future::join_all(relay_targets.into_iter().map(|mutual_addr| {
                let target_id = canonical_id.clone();
                async move {
                    let _ = tokio::time::timeout(DIAL_TIMEOUT, async {
                        let conn = Self::connect_peer(ep, &mutual_addr).await?;
                        let (mut send, mut recv) = conn.open_bi().await?;
                        write_frame(
                            &mut send,
                            &P2pRequest::CancelRelayedInvite {
                                target_peer_id: target_id,
                            },
                        )
                        .await?;
                        let _resp: P2pResponse = read_frame(&mut recv).await?;
                        Ok::<_, anyhow::Error>(())
                    })
                    .await;
                }
            }))
            .await;
        }
        self.get_state()
    }

    pub async fn add_friend_by_code(&self, code: String) -> Result<FriendsState> {
        let ticket = decode_join_code(&code)?;
        let peer_id = ticket.addr.id.to_string();
        if let Some(ep) = self.endpoint.get() {
            if ep.id() == ticket.addr.id {
                bail!("You cannot add your own join code");
            }
        }
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.pending_requests.retain(|r| r.peer_id != peer_id);
            store.queued_invites.retain(|q| q.peer_id != peer_id);
            if let Some(existing) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
                existing.addr = ticket.addr.clone();
                existing.display_name = ticket.name.clone();
                if existing.added_at.is_none() {
                    existing.added_at = Some(now_rfc3339());
                }
            } else {
                store.friends.push(FriendRecord {
                    peer_id: peer_id.clone(),
                    display_name: ticket.name.clone(),
                    profile_picture: None,
                    addr: ticket.addr.clone(),
                    status: FriendStatus::PendingOutgoing,
                    online: false,
                    last_seen_at: None,
                    added_at: Some(now_rfc3339()),
                    cached_profile: None,
                    syncing_my_collections: Vec::new(),
                    cached_via_peer_name: None,
                });
            }
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        let _ = self.dial_and_sync_peer(&peer_id).await;
        self.get_state()
    }

    pub async fn respond_friend_request(
        &self,
        peer_id: String,
        accept: bool,
    ) -> Result<FriendsState> {
        let req_opt = {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            let pos = store
                .pending_requests
                .iter()
                .position(|r| r.peer_id == peer_id);
            let req = pos.map(|i| store.pending_requests.remove(i));
            if accept {
                store.queued_invites.retain(|q| q.peer_id != peer_id);
                if let Some(ref r) = req {
                    if let Some(existing) = store.friends.iter_mut().find(|f| f.peer_id == peer_id)
                    {
                        existing.status = FriendStatus::Connected;
                        existing.addr = r.addr.clone();
                        existing.display_name = r.display_name.clone();
                        if existing.added_at.is_none() {
                            existing.added_at = Some(now_rfc3339());
                        }
                    } else {
                        let now = now_rfc3339();
                        store.friends.push(FriendRecord {
                            peer_id: r.peer_id.clone(),
                            display_name: r.display_name.clone(),
                            profile_picture: r.profile_picture.clone(),
                            addr: r.addr.clone(),
                            status: FriendStatus::Connected,
                            online: false,
                            last_seen_at: Some(now.clone()),
                            added_at: Some(now),
                            cached_profile: None,
                            syncing_my_collections: Vec::new(),
                            cached_via_peer_name: None,
                        });
                    }
                }
            }
            save_friends_store(&self.paths, &store)?;
            req
        };
        self.emit_changed();
        if accept && req_opt.is_some() {
            let _ = self.dial_and_sync_peer(&peer_id).await;
        }
        self.get_state()
    }

    pub fn remove_friend(&self, peer_id: String) -> Result<FriendsState> {
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.friends.retain(|f| f.peer_id != peer_id);
            store.pending_requests.retain(|r| r.peer_id != peer_id);
            store.queued_invites.retain(|q| q.peer_id != peer_id);
            store.relayed_invites.retain(|inv| {
                inv.payload.from_peer_id != peer_id && inv.payload.to_peer_id != peer_id
            });
            store.feed_events.retain(|ev| ev.peer_id != peer_id);
            for vis in store.collection_visibility.values_mut() {
                if let CollectionVisibility::SpecificFriends { peer_ids } = vis {
                    peer_ids.retain(|id| id != &peer_id);
                }
            }
            if let CollectionVisibility::SpecificFriends { ref mut peer_ids } =
                store.friends_list_visibility
            {
                peer_ids.retain(|id| id != &peer_id);
            }
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub fn mark_feed_read(&self) -> Result<FriendsState> {
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.feed_last_read_at = Some(now_rfc3339());
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub fn clear_feed(&self) -> Result<FriendsState> {
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            let now = now_rfc3339();
            store.feed_events.clear();
            store.feed_last_read_at = Some(now.clone());
            store.feed_cleared_at = Some(now);
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub async fn refresh_all_friends(&self) -> Result<FriendsState> {
        if self.is_appear_offline() {
            return self.get_state();
        }
        let (peer_ids, has_queued): (Vec<String>, bool) = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            (
                store.friends.iter().map(|f| f.peer_id.clone()).collect(),
                !store.queued_invites.is_empty(),
            )
        };
        if !peer_ids.is_empty() || has_queued {
            let _guard = CheckingGuard::new(self);
            if !peer_ids.is_empty() {
                futures_util::future::join_all(
                    peer_ids.iter().map(|pid| self.dial_and_sync_peer(pid)),
                )
                .await;
                self.pull_newer_caches_from_mutual_friends().await;
            }
            self.process_queued_invites().await;
        }
        self.get_state()
    }

    async fn heartbeat_friends(self: &Arc<Self>) -> Result<()> {
        let svc = Arc::clone(self);
        let _ = tokio::task::spawn_blocking(move || {
            let _ = svc.sync_local_activity();
        })
        .await;
        if self.is_appear_offline() {
            return Ok(());
        }
        let (peers, has_queued): (Vec<(String, FriendStatus)>, bool) = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            (
                store
                    .friends
                    .iter()
                    .map(|f| (f.peer_id.clone(), f.status))
                    .collect(),
                !store.queued_invites.is_empty(),
            )
        };
        if !peers.is_empty() || has_queued {
            let _guard = CheckingGuard::new(self);
            if !peers.is_empty() {
                futures_util::future::join_all(peers.iter().map(|(pid, status)| async move {
                    match status {
                        FriendStatus::Connected => {
                            let _ = self.sync_peer_profile_only(pid).await;
                        }
                        FriendStatus::PendingOutgoing => {
                            let _ = self.dial_and_sync_peer(pid).await;
                        }
                    }
                }))
                .await;
                self.pull_newer_caches_from_mutual_friends().await;
            }
            self.process_queued_invites().await;
        }
        Ok(())
    }

    async fn connect_peer(ep: &Endpoint, addr: &EndpointAddr) -> Result<iroh::endpoint::Connection> {
        let direct_timeout = Duration::from_secs(6);
        if !addr.addrs.is_empty() {
            if let Ok(Ok(conn)) =
                tokio::time::timeout(direct_timeout, ep.connect(addr.clone(), P2P_ALPN)).await
            {
                return Ok(conn);
            }
        }
        // Fallback to n0/pkarr discovery with just EndpointId if cached socket addrs are empty or stale.
        let conn = tokio::time::timeout(
            direct_timeout,
            ep.connect(EndpointAddr::new(addr.id), P2P_ALPN),
        )
        .await
        .map_err(|_| anyhow!("connect timed out"))??;
        Ok(conn)
    }

    async fn deliver_queued_invite(&self, target_peer_id: &str) -> Result<bool> {
        let Some(ep) = self.endpoint.get() else {
            return Ok(false);
        };
        let (addr, local_name) = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            let Some(queued) = store
                .queued_invites
                .iter()
                .find(|q| q.peer_id == target_peer_id)
            else {
                return Ok(false);
            };
            (queued.addr.clone(), effective_display_name(&store))
        };
        let local_addr = ep.addr();

        let session_timeout = DIAL_TIMEOUT + STREAM_IO_TIMEOUT;
        let session_res = tokio::time::timeout(session_timeout, async {
            let conn = Self::connect_peer(ep, &addr).await?;
            let (mut send1, mut recv1) = conn.open_bi().await?;
            write_frame(
                &mut send1,
                &P2pRequest::Hello {
                    display_name: local_name,
                    addr: local_addr,
                },
            )
            .await?;
            let hello_resp: P2pResponse = read_frame(&mut recv1).await?;
            let accepted = match &hello_resp {
                P2pResponse::HelloAck { accepted, .. } => *accepted,
                _ => false,
            };
            let profile_resp = if accepted {
                let (mut send2, mut recv2) = conn.open_bi().await?;
                write_frame(
                    &mut send2,
                    &P2pRequest::GetProfile {
                        known_hashes: HashMap::new(),
                        synced_collection_ids: Some(Vec::new()),
                        activity_since: Some(now_rfc3339()),
                    },
                )
                .await?;
                Some(read_frame::<P2pResponse>(&mut recv2).await?)
            } else {
                None
            };
            Ok::<_, anyhow::Error>((hello_resp, profile_resp))
        })
        .await;

        let Ok(Ok((
            P2pResponse::HelloAck {
                accepted,
                display_name,
                addr: remote_addr,
            },
            profile_resp,
        ))) = session_res
        else {
            return Ok(false);
        };

        let sanitized_name = sanitize_display_name(&display_name, "Friend");
        let sanitized_addr = sanitize_peer_addr(addr.id, &remote_addr).unwrap_or(addr);
        let now = now_rfc3339();
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store
                .queued_invites
                .retain(|q| q.peer_id != target_peer_id);
            if let Some(existing) = store
                .friends
                .iter_mut()
                .find(|f| f.peer_id == target_peer_id)
            {
                existing.display_name = sanitized_name;
                existing.addr = sanitized_addr;
                existing.online = true;
                existing.last_seen_at = Some(now.clone());
                if accepted {
                    existing.status = FriendStatus::Connected;
                }
                if existing.added_at.is_none() {
                    existing.added_at = Some(now);
                }
            } else {
                store.friends.push(FriendRecord {
                    peer_id: target_peer_id.to_string(),
                    display_name: sanitized_name,
                    profile_picture: None,
                    addr: sanitized_addr,
                    status: if accepted {
                        FriendStatus::Connected
                    } else {
                        FriendStatus::PendingOutgoing
                    },
                    online: true,
                    last_seen_at: Some(now.clone()),
                    added_at: Some(now),
                    cached_profile: None,
                    syncing_my_collections: Vec::new(),
                    cached_via_peer_name: None,
                });
            }
            let _ = save_friends_store(&self.paths, &store);
        }
        if let Some(P2pResponse::Profile(mut profile)) = profile_resp {
            let relayed = std::mem::take(&mut profile.relayed_invites);
            let _ = self.apply_fetched_profile(target_peer_id, profile);
            if !relayed.is_empty() {
                let _ = self.apply_received_relayed_invites(relayed);
            }
        }
        self.emit_changed();
        Ok(true)
    }

    async fn deposit_queued_invite_to_mutuals(&self, target_peer_id: &str) -> Result<bool> {
        let Some(ep) = self.endpoint.get() else {
            return Ok(false);
        };
        let Some(signing_key) = self.secret_key.get().cloned() else {
            return Ok(false);
        };
        let Ok(target_id) = target_peer_id.parse::<EndpointId>() else {
            return Ok(false);
        };

        let (payload, candidates): (FriendInvitePayload, Vec<(String, EndpointAddr)>) = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            let Some(queued) = store
                .queued_invites
                .iter()
                .find(|q| q.peer_id == target_peer_id)
            else {
                return Ok(false);
            };
            let (_, _, _, online_mutuals) = Self::inspect_mutual_target_info(&store, target_id);
            let pending_mutuals: Vec<(String, EndpointAddr)> = online_mutuals
                .into_iter()
                .filter(|(mid, _)| !queued.relayed_via_peer_ids.contains(mid))
                .collect();
            if pending_mutuals.is_empty() {
                return Ok(false);
            }
            let payload = FriendInvitePayload {
                from_peer_id: ep.id().to_string(),
                to_peer_id: target_peer_id.to_string(),
                from_display_name: effective_display_name(&store),
                from_profile_picture: store.profile_picture.clone(),
                from_addr: shared_friend_addr(&ep.addr()),
                created_at: now_rfc3339(),
            };
            (payload, pending_mutuals)
        };

        let signed = sign_friend_invite(&signing_key, payload)?;
        let stored_mutuals: Vec<String> =
            futures_util::future::join_all(candidates.into_iter().map(|(mid, maddr)| {
                let inv = signed.clone();
                async move {
                    let res = tokio::time::timeout(DIAL_TIMEOUT, async {
                        let conn = Self::connect_peer(ep, &maddr).await?;
                        let (mut send, mut recv) = conn.open_bi().await?;
                        write_frame(&mut send, &P2pRequest::DepositFriendInvite { invite: inv })
                            .await?;
                        let resp: P2pResponse = read_frame(&mut recv).await?;
                        Ok::<_, anyhow::Error>(resp)
                    })
                    .await;
                    match res {
                        Ok(Ok(P2pResponse::RelayedInviteStored)) => Some(mid),
                        _ => None,
                    }
                }
            }))
            .await
            .into_iter()
            .flatten()
            .collect();

        if stored_mutuals.is_empty() {
            return Ok(false);
        }

        let mut changed = false;
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            if let Some(q) = store
                .queued_invites
                .iter_mut()
                .find(|q| q.peer_id == target_peer_id)
            {
                for mid in stored_mutuals {
                    if !q.relayed_via_peer_ids.contains(&mid) {
                        q.relayed_via_peer_ids.push(mid);
                        changed = true;
                    }
                }
            }
            if changed {
                let _ = save_friends_store(&self.paths, &store);
            }
        }
        if changed {
            self.emit_changed();
        }
        Ok(changed)
    }

    async fn process_queued_invites(&self) {
        if self.is_appear_offline() {
            return;
        }
        let tasks: Vec<(String, Option<bool>)> = {
            let Ok(mut store) = self.store.lock() else {
                return;
            };
            if store.queued_invites.is_empty() {
                return;
            }
            let mut mutated = false;
            let mut out = Vec::with_capacity(store.queued_invites.len());
            let snapshots: Vec<(String, Option<String>, Option<EndpointAddr>, Option<bool>)> = store
                .queued_invites
                .iter()
                .filter_map(|q| {
                    let parsed = q.peer_id.parse::<EndpointId>().ok()?;
                    let (name, addr, online_hint, _) =
                        Self::inspect_mutual_target_info(&store, parsed);
                    Some((q.peer_id.clone(), name, addr, online_hint))
                })
                .collect();

            for (pid, name, addr, online_hint) in snapshots {
                if let Some(q) = store.queued_invites.iter_mut().find(|q| q.peer_id == pid) {
                    if let Some(fresh_name) = name {
                        if q.display_name == "Friend" && fresh_name != "Friend" {
                            q.display_name = fresh_name;
                            mutated = true;
                        }
                    }
                    if let Some(fresh_addr) = addr {
                        if q.addr != fresh_addr {
                            q.addr = fresh_addr;
                            mutated = true;
                        }
                    }
                }
                out.push((pid, online_hint));
            }
            if mutated {
                let _ = save_friends_store(&self.paths, &store);
            }
            out
        };

        for (pid, online_hint) in tasks {
            let mut delivered = false;
            if online_hint != Some(false) {
                delivered = self.deliver_queued_invite(&pid).await.unwrap_or(false);
            }
            if !delivered {
                let _ = self.deposit_queued_invite_to_mutuals(&pid).await;
            }
        }
    }

    fn apply_received_relayed_invites(&self, invites: Vec<SignedFriendInvite>) -> Vec<String> {
        let Some(ep) = self.endpoint.get() else {
            return Vec::new();
        };
        let local_id = ep.id().to_string();
        let mut ack_from_ids: Vec<String> = Vec::new();
        let mut mutated = false;

        if let Ok(mut store) = self.store.lock() {
            for invite in invites.into_iter().take(MAX_RELAYED_INVITES_PER_TARGET) {
                if invite.payload.to_peer_id != local_id {
                    continue;
                }
                if let Err(e) = verify_signed_friend_invite(&invite) {
                    log::warn!(
                        "Rejected invalid relayed friend invite from {}: {e}",
                        invite.payload.from_peer_id
                    );
                    continue;
                }
                let from_id_str = invite.payload.from_peer_id.clone();
                let Ok(from_ep_id) = from_id_str.parse::<EndpointId>() else {
                    continue;
                };
                let sanitized_name =
                    sanitize_display_name(&invite.payload.from_display_name, "Friend");
                let sanitized_addr = invite
                    .payload
                    .from_addr
                    .as_ref()
                    .and_then(|a| sanitize_peer_addr(from_ep_id, a).ok())
                    .unwrap_or_else(|| EndpointAddr::new(from_ep_id));
                let sanitized_pic = invite
                    .payload
                    .from_profile_picture
                    .as_deref()
                    .filter(|pic| validate_profile_picture(pic).is_ok())
                    .map(str::to_string);

                if let Some(existing) = store.friends.iter_mut().find(|f| f.peer_id == from_id_str)
                {
                    if existing.status == FriendStatus::PendingOutgoing {
                        existing.status = FriendStatus::Connected;
                        existing.display_name = sanitized_name;
                        if !sanitized_addr.addrs.is_empty() {
                            existing.addr = sanitized_addr;
                        }
                        if sanitized_pic.is_some() {
                            existing.profile_picture = sanitized_pic;
                        }
                        mutated = true;
                    }
                    ack_from_ids.push(from_id_str);
                    continue;
                }

                if let Some(q_pos) = store
                    .queued_invites
                    .iter()
                    .position(|q| q.peer_id == from_id_str)
                {
                    store.queued_invites.remove(q_pos);
                    let now = now_rfc3339();
                    store.friends.push(FriendRecord {
                        peer_id: from_id_str.clone(),
                        display_name: sanitized_name,
                        profile_picture: sanitized_pic,
                        addr: sanitized_addr,
                        status: FriendStatus::Connected,
                        online: false,
                        last_seen_at: Some(now.clone()),
                        added_at: Some(now),
                        cached_profile: None,
                        syncing_my_collections: Vec::new(),
                        cached_via_peer_name: None,
                    });
                    mutated = true;
                    ack_from_ids.push(from_id_str);
                    continue;
                }

                if let Some(existing_req) = store
                    .pending_requests
                    .iter_mut()
                    .find(|r| r.peer_id == from_id_str)
                {
                    existing_req.display_name = sanitized_name;
                    if !sanitized_addr.addrs.is_empty() {
                        existing_req.addr = sanitized_addr;
                    }
                    if sanitized_pic.is_some() {
                        existing_req.profile_picture = sanitized_pic;
                    }
                    mutated = true;
                    ack_from_ids.push(from_id_str);
                } else if store.pending_requests.len() < MAX_PENDING_REQUESTS {
                    store.pending_requests.push(PendingFriendRequest {
                        peer_id: from_id_str.clone(),
                        display_name: sanitized_name,
                        profile_picture: sanitized_pic,
                        addr: sanitized_addr,
                        received_at: invite.payload.created_at,
                    });
                    mutated = true;
                    ack_from_ids.push(from_id_str);
                }
            }
            if mutated {
                let _ = save_friends_store(&self.paths, &store);
            }
        }

        if mutated {
            self.emit_changed();
        }
        ack_from_ids
    }

    fn peer_context(
        &self,
        peer_id: &str,
    ) -> Result<
        Option<(
            EndpointAddr,
            String,
            HashMap<String, String>,
            Vec<String>,
            Option<String>,
        )>,
    > {
        let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
        let Some(friend) = store.friends.iter().find(|f| f.peer_id == peer_id) else {
            return Ok(None);
        };
        let mut hashes = HashMap::new();
        if let Some(profile) = &friend.cached_profile {
            for c in &profile.collections {
                hashes.insert(c.id.clone(), c.share_hash.clone());
            }
        }
        let addr = friend.addr.clone();
        let local_name = effective_display_name(&store);
        let activity_since = max_rfc3339(
            friend.added_at.as_deref(),
            store.feed_cleared_at.as_deref(),
        );
        drop(store);
        let synced_ids = collect_synced_collection_ids(&self.paths, peer_id);
        Ok(Some((addr, local_name, hashes, synced_ids, activity_since)))
    }

    fn enforce_collection_share_budget(peer_id: &str, collections: &mut [FriendShareRef]) {
        let mut total_share_bytes = 0usize;
        for col in collections.iter_mut() {
            if col.share_code.len() > share::MAX_SHARE_CODE_BYTES {
                log::warn!(
                    "Dropped oversized share_code from {peer_id} for collection {}",
                    col.id
                );
                col.share_code.clear();
                continue;
            }
            if !col.share_code.is_empty() {
                if total_share_bytes.saturating_add(col.share_code.len()) > MAX_FRIEND_CACHED_SHARE_BYTES
                {
                    log::warn!(
                        "Dropped share_code from {peer_id} for collection {} (total cache budget exceeded)",
                        col.id
                    );
                    col.share_code.clear();
                } else {
                    total_share_bytes += col.share_code.len();
                }
            }
        }
    }

    fn sanitize_profile_fields(
        peer_id: &str,
        local_peer_id: Option<&str>,
        display_name: &mut String,
        profile_picture: &mut Option<String>,
        collections: &mut Vec<FriendShareRef>,
        friends: &mut Option<Vec<SharedFriendRef>>,
    ) {
        *display_name = sanitize_display_name(display_name, "Friend");
        if let Some(ref pic) = profile_picture {
            if let Err(e) = validate_profile_picture(pic) {
                log::warn!("Dropped invalid remote profile picture from {peer_id}: {e}");
                *profile_picture = None;
            }
        }
        collections.truncate(MAX_REMOTE_COLLECTIONS);
        for col in collections.iter_mut() {
            col.id = sanitize_short_text(&col.id, MAX_COLLECTION_ID_CHARS);
            col.name = sanitize_short_text(&col.name, MAX_COLLECTION_NAME_CHARS);
            col.share_hash = sanitize_short_text(&col.share_hash, MAX_SHARE_HASH_CHARS);
            col.synced_source_collection_id = col
                .synced_source_collection_id
                .as_deref()
                .map(|raw| sanitize_short_text(raw, MAX_COLLECTION_ID_CHARS))
                .filter(|clean| !clean.is_empty());
            col.matched_local_collection = None;
        }
        collections.retain(|c| !c.id.is_empty());
        Self::enforce_collection_share_budget(peer_id, collections);

        if let Some(ref mut shared_friends) = friends {
            let mut seen_peers = HashSet::new();
            let mut cleaned = Vec::with_capacity(shared_friends.len().min(MAX_REMOTE_FRIENDS));
            for entry in shared_friends.drain(..).take(MAX_REMOTE_FRIENDS) {
                let Ok(parsed_id) = entry.peer_id.trim().parse::<EndpointId>() else {
                    continue;
                };
                let canonical = parsed_id.to_string();
                if canonical == peer_id || local_peer_id == Some(canonical.as_str()) {
                    continue;
                }
                if !seen_peers.insert(canonical.clone()) {
                    continue;
                }
                let sanitized_name = sanitize_display_name(&entry.display_name, "Friend");
                let sanitized_addr = entry
                    .addr
                    .as_ref()
                    .and_then(|a| sanitize_peer_addr(parsed_id, a).ok());
                let sanitized_updated_at = entry
                    .profile_updated_at
                    .as_deref()
                    .filter(|ts| chrono::DateTime::parse_from_rfc3339(ts).is_ok())
                    .map(str::to_string);
                cleaned.push(SharedFriendRef {
                    peer_id: canonical,
                    display_name: sanitized_name,
                    addr: sanitized_addr,
                    profile_updated_at: sanitized_updated_at,
                    online: entry.online,
                });
            }
            *shared_friends = cleaned;
        }
    }

    fn apply_fetched_profile(&self, peer_id: &str, mut profile: PeerProfileSummary) -> Result<bool> {
        if profile.peer_id != peer_id {
            bail!("Remote profile peer_id does not match authenticated peer");
        }
        let local_peer_id = self.endpoint.get().map(|ep| ep.id().to_string());
        Self::sanitize_profile_fields(
            peer_id,
            local_peer_id.as_deref(),
            &mut profile.display_name,
            &mut profile.profile_picture,
            &mut profile.collections,
            &mut profile.friends,
        );
        if let Some(ref relay) = profile.signed_relay_profile {
            if let Err(e) = verify_signed_relay_profile(peer_id, relay) {
                log::warn!("Dropped invalid signed_relay_profile from {peer_id}: {e}");
                profile.signed_relay_profile = None;
            }
        }

        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
        let mut changed = false;
        let now = now_rfc3339();
        let feed_cleared_at = store.feed_cleared_at.clone();
        let mut new_feed_events: Vec<FriendFeedEvent> = Vec::new();
        let mut visible_col_ids: Option<HashSet<String>> = None;
        let mut friend_cutoff: Option<String> = None;
        let incoming_activity = profile.activity_events.clone();
        let incoming_display_name = profile.display_name.clone();
        if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
            friend_cutoff = max_rfc3339(f.added_at.as_deref(), feed_cleared_at.as_deref());
            // Rehydrate unchanged share_code strings from cache.
            if let Some(prev) = &f.cached_profile {
                let prev_codes: HashMap<&str, &str> = prev
                    .collections
                    .iter()
                    .map(|c| (c.id.as_str(), c.share_code.as_str()))
                    .collect();
                for col in &mut profile.collections {
                    if col.share_code.is_empty() {
                        if let Some(cached_code) = prev_codes.get(col.id.as_str()) {
                            col.share_code = (*cached_code).to_string();
                        }
                    }
                }
                Self::enforce_collection_share_budget(peer_id, &mut profile.collections);
                // Legacy peer fallback: only synthesize receiver-side diffs when the
                // remote peer does not send author-recorded `activity_events`.
                if incoming_activity.is_none() {
                    new_feed_events = collect_profile_feed_events(
                        peer_id,
                        &profile.display_name,
                        prev,
                        &profile,
                        &now,
                    );
                }
            }
            visible_col_ids = Some(profile.collections.iter().map(|c| c.id.clone()).collect());
            let collections_changed = match &f.cached_profile {
                None => true,
                Some(prev) => {
                    prev.collections.len() != profile.collections.len()
                        || prev.collections.iter().zip(profile.collections.iter()).any(|(a, b)| {
                            a.id != b.id
                                || a.name != b.name
                                || a.share_hash != b.share_hash
                                || a.share_code != b.share_code
                        })
                }
            };
            let friends_changed = match &f.cached_profile {
                None => profile.friends.is_some() || !profile.allow_mutual_connections,
                Some(prev) => {
                    prev.friends != profile.friends
                        || prev.allow_mutual_connections != profile.allow_mutual_connections
                }
            };
            changed = !f.online
                || f.cached_via_peer_name.is_some()
                || f.display_name != profile.display_name
                || f.profile_picture != profile.profile_picture
                || collections_changed
                || friends_changed;
            f.display_name = profile.display_name.clone();
            f.profile_picture = profile.profile_picture.clone();
            f.cached_profile = Some(profile);
            f.online = true;
            f.last_seen_at = Some(now.clone());
            f.cached_via_peer_name = None;
        }
        if let Some(ref visible_ids) = visible_col_ids {
            // Prune feed items for collections no longer visible from this peer (e.g. made Private or unshared).
            let before_len = store.feed_events.len();
            store
                .feed_events
                .retain(|ev| ev.peer_id != peer_id || visible_ids.contains(&ev.collection_id));
            if store.feed_events.len() != before_len {
                changed = true;
            }
            if let Some(ref events) = incoming_activity {
                if merge_remote_activity_events(
                    peer_id,
                    &incoming_display_name,
                    events,
                    visible_ids,
                    friend_cutoff.as_deref(),
                    &mut store.feed_events,
                    &now,
                ) {
                    changed = true;
                }
            }
        }
        if !new_feed_events.is_empty() {
            new_feed_events.append(&mut store.feed_events);
            new_feed_events.truncate(MAX_FEED_EVENTS);
            store.feed_events = new_feed_events;
            changed = true;
        }
        if changed {
            let _ = save_friends_store(&self.paths, &store);
        }
        Ok(changed)
    }

    fn apply_relayed_profile(
        &self,
        target_peer_id: &str,
        via_peer_name: &str,
        signed: SignedRelayProfile,
    ) -> Result<bool> {
        verify_signed_relay_profile(target_peer_id, &signed)?;
        let mut payload = signed.payload.clone();
        let local_peer_id = self.endpoint.get().map(|ep| ep.id().to_string());
        Self::sanitize_profile_fields(
            target_peer_id,
            local_peer_id.as_deref(),
            &mut payload.display_name,
            &mut payload.profile_picture,
            &mut payload.collections,
            &mut payload.friends,
        );

        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
        let mut changed = false;
        let now = now_rfc3339();
        let feed_cleared_at = store.feed_cleared_at.clone();
        let mut new_feed_events: Vec<FriendFeedEvent> = Vec::new();
        let mut visible_col_ids: Option<HashSet<String>> = None;
        let mut friend_cutoff: Option<String> = None;
        let incoming_activity = payload.activity_events.clone();
        let incoming_display_name = payload.display_name.clone();

        if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == target_peer_id) {
            if f.online || f.status != FriendStatus::Connected {
                return Ok(false);
            }
            let existing_updated_at = f.cached_profile.as_ref().map(|p| p.updated_at.as_str());
            if !is_timestamp_newer(&payload.updated_at, existing_updated_at) {
                return Ok(false);
            }
            friend_cutoff = max_rfc3339(f.added_at.as_deref(), feed_cleared_at.as_deref());

            // Preserve any locally cached SpecificFriends collections that would
            // not appear in an AllFriends-only relay payload.
            if let Some(prev) = &f.cached_profile {
                let relay_ids: HashSet<String> =
                    payload.collections.iter().map(|c| c.id.clone()).collect();
                for prev_col in &prev.collections {
                    if prev_col.specific_friends_only
                        && !relay_ids.contains(&prev_col.id)
                        && payload.collections.len() < MAX_REMOTE_COLLECTIONS
                    {
                        payload.collections.push(prev_col.clone());
                    }
                }
                Self::enforce_collection_share_budget(target_peer_id, &mut payload.collections);
            }

            let merged_profile = PeerProfileSummary {
                peer_id: target_peer_id.to_string(),
                display_name: payload.display_name.clone(),
                profile_picture: payload.profile_picture.clone(),
                updated_at: payload.updated_at.clone(),
                collections: payload.collections,
                friends: payload.friends,
                allow_mutual_connections: payload.allow_mutual_connections,
                signed_relay_profile: Some(signed),
                activity_events: payload.activity_events,
                relayed_invites: Vec::new(),
            };

            if incoming_activity.is_none() {
                if let Some(prev) = &f.cached_profile {
                    new_feed_events = collect_profile_feed_events(
                        target_peer_id,
                        &merged_profile.display_name,
                        prev,
                        &merged_profile,
                        &merged_profile.updated_at,
                    );
                }
            }
            visible_col_ids = Some(
                merged_profile
                    .collections
                    .iter()
                    .map(|c| c.id.clone())
                    .collect(),
            );
            f.display_name = merged_profile.display_name.clone();
            f.profile_picture = merged_profile.profile_picture.clone();
            if is_timestamp_newer(&merged_profile.updated_at, f.last_seen_at.as_deref()) {
                f.last_seen_at = Some(merged_profile.updated_at.clone());
            }
            f.cached_profile = Some(merged_profile);
            f.cached_via_peer_name = Some(sanitize_display_name(via_peer_name, "Friend"));
            // Keep f.online = false since the friend was not reached directly.
            changed = true;
        }

        if let Some(ref visible_ids) = visible_col_ids {
            let before_len = store.feed_events.len();
            store.feed_events.retain(|ev| {
                ev.peer_id != target_peer_id || visible_ids.contains(&ev.collection_id)
            });
            if store.feed_events.len() != before_len {
                changed = true;
            }
            if let Some(ref events) = incoming_activity {
                if merge_remote_activity_events(
                    target_peer_id,
                    &incoming_display_name,
                    events,
                    visible_ids,
                    friend_cutoff.as_deref(),
                    &mut store.feed_events,
                    &now,
                ) {
                    changed = true;
                }
            }
        }
        if !new_feed_events.is_empty() {
            new_feed_events.append(&mut store.feed_events);
            new_feed_events.truncate(MAX_FEED_EVENTS);
            store.feed_events = new_feed_events;
            changed = true;
        }
        if changed {
            let _ = save_friends_store(&self.paths, &store);
        }
        Ok(changed)
    }

    async fn pull_newer_caches_from_mutual_friends(&self) {
        let Some(ep) = self.endpoint.get() else {
            return;
        };

        // Plan: for each offline Connected friend C, collect online Connected
        // mutual friends B whose advertised `profile_updated_at` for C is
        // strictly newer than our cached `C.updated_at`, sorted newest-first.
        let plan: Vec<(String, Option<String>, Vec<(EndpointAddr, String)>)> = {
            let Ok(store) = self.store.lock() else {
                return;
            };
            let online_friends: Vec<&FriendRecord> = store
                .friends
                .iter()
                .filter(|f| f.status == FriendStatus::Connected && f.online)
                .collect();

            store
                .friends
                .iter()
                .filter(|f| f.status == FriendStatus::Connected && !f.online)
                .filter_map(|offline| {
                    let local_updated = offline
                        .cached_profile
                        .as_ref()
                        .map(|p| p.updated_at.clone());
                    let mut candidates: Vec<(String, EndpointAddr, String)> = online_friends
                        .iter()
                        .filter_map(|mutual| {
                            let sf = mutual
                                .cached_profile
                                .as_ref()?
                                .friends
                                .as_ref()?
                                .iter()
                                .find(|sf| sf.peer_id == offline.peer_id)?;
                            let remote_ts = sf.profile_updated_at.as_deref()?;
                            if is_timestamp_newer(remote_ts, local_updated.as_deref()) {
                                Some((
                                    remote_ts.to_string(),
                                    mutual.addr.clone(),
                                    mutual.display_name.clone(),
                                ))
                            } else {
                                None
                            }
                        })
                        .collect();
                    if candidates.is_empty() {
                        return None;
                    }
                    candidates.sort_by(|a, b| {
                        let da = chrono::DateTime::parse_from_rfc3339(&a.0).ok();
                        let db = chrono::DateTime::parse_from_rfc3339(&b.0).ok();
                        db.cmp(&da)
                    });
                    Some((
                        offline.peer_id.clone(),
                        local_updated,
                        candidates
                            .into_iter()
                            .map(|(_, addr, name)| (addr, name))
                            .collect(),
                    ))
                })
                .collect()
        };

        if plan.is_empty() {
            return;
        }

        let mut any_changed = false;
        let results = futures_util::future::join_all(plan.into_iter().map(
            |(target_peer_id, known_updated_at, candidates)| async move {
                for (mutual_addr, mutual_name) in candidates {
                    let res = tokio::time::timeout(DIAL_TIMEOUT, async {
                        let conn = Self::connect_peer(ep, &mutual_addr).await?;
                        let (mut send, mut recv) = conn.open_bi().await?;
                        write_frame(
                            &mut send,
                            &P2pRequest::GetCachedFriendProfile {
                                target_peer_id: target_peer_id.clone(),
                                known_updated_at: known_updated_at.clone(),
                            },
                        )
                        .await?;
                        let resp: P2pResponse = read_frame(&mut recv).await?;
                        Ok::<_, anyhow::Error>(resp)
                    })
                    .await;

                    if let Ok(Ok(P2pResponse::CachedFriendProfile(signed))) = res {
                        match self.apply_relayed_profile(&target_peer_id, &mutual_name, signed) {
                            Ok(true) => return true,
                            Ok(false) => return false,
                            Err(e) => {
                                log::warn!(
                                    "Rejected relayed profile for {target_peer_id} from {mutual_name}: {e}"
                                );
                            }
                        }
                    }
                }
                false
            },
        ))
        .await;

        if results.into_iter().any(|c| c) {
            any_changed = true;
        }
        if any_changed {
            self.emit_changed();
        }
    }

    pub fn notify_peer_sync_state(self: &Arc<Self>, peer_id: String) {
        let svc = Arc::clone(self);
        tauri::async_runtime::spawn(async move {
            let _ = svc.sync_peer_profile_only(&peer_id).await;
        });
    }

    async fn sync_peer_profile_only(&self, peer_id: &str) -> Result<()> {
        let Some(ep) = self.endpoint.get() else {
            return Ok(());
        };
        let Some((addr, _, known_hashes, synced_ids, activity_since)) =
            self.peer_context(peer_id)?
        else {
            return Ok(());
        };

        let session_timeout = DIAL_TIMEOUT + STREAM_IO_TIMEOUT;
        let profile_res = tokio::time::timeout(session_timeout, async {
            let conn = Self::connect_peer(ep, &addr).await?;
            let (mut send, mut recv) = conn.open_bi().await?;
            write_frame(
                &mut send,
                &P2pRequest::GetProfile {
                    known_hashes,
                    synced_collection_ids: Some(synced_ids),
                    activity_since,
                },
            )
            .await?;
            let mut resp: P2pResponse = read_frame(&mut recv).await?;
            if let P2pResponse::Profile(ref mut profile) = resp {
                let relayed = std::mem::take(&mut profile.relayed_invites);
                if !relayed.is_empty() {
                    let ack_ids = self.apply_received_relayed_invites(relayed);
                    if !ack_ids.is_empty() {
                        if let Ok((mut send_ack, mut recv_ack)) = conn.open_bi().await {
                            let _ = write_frame(
                                &mut send_ack,
                                &P2pRequest::AckRelayedInvites {
                                    from_peer_ids: ack_ids,
                                },
                            )
                            .await;
                            let _ = read_frame::<P2pResponse>(&mut recv_ack).await;
                        }
                    }
                }
            }
            Ok::<_, anyhow::Error>(resp)
        })
        .await;

        let changed = match profile_res {
            Ok(Ok(P2pResponse::Profile(profile))) => self.apply_fetched_profile(peer_id, profile)?,
            Ok(Ok(P2pResponse::Denied(_))) => {
                // Remote peer hasn't accepted or removed us; keep online state accurate.
                false
            }
            _ => {
                let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                let mut went_offline = false;
                if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
                    if f.online {
                        f.online = false;
                        went_offline = true;
                    }
                }
                if went_offline {
                    let _ = save_friends_store(&self.paths, &store);
                }
                went_offline
            }
        };
        if changed {
            self.emit_changed();
        }
        Ok(())
    }

    async fn dial_and_sync_peer(&self, peer_id: &str) -> Result<()> {
        let Some(ep) = self.endpoint.get() else {
            return Ok(());
        };
        let Some((addr, local_name, known_hashes, synced_ids, activity_since)) =
            self.peer_context(peer_id)?
        else {
            return Ok(());
        };
        let local_addr = ep.addr();

        // Single connection: stream 1 = Hello, stream 2 = GetProfile (if accepted), stream 3 = AckRelayedInvites (if any).
        let session_timeout = DIAL_TIMEOUT + STREAM_IO_TIMEOUT;
        let session_res = tokio::time::timeout(session_timeout, async {
            let conn = Self::connect_peer(ep, &addr).await?;
            let (mut send1, mut recv1) = conn.open_bi().await?;
            write_frame(
                &mut send1,
                &P2pRequest::Hello {
                    display_name: local_name,
                    addr: local_addr,
                },
            )
            .await?;
            let hello_resp: P2pResponse = read_frame(&mut recv1).await?;

            let accepted = match &hello_resp {
                P2pResponse::HelloAck { accepted, .. } => *accepted,
                _ => false,
            };

            let profile_resp = if accepted {
                let (mut send2, mut recv2) = conn.open_bi().await?;
                write_frame(
                    &mut send2,
                    &P2pRequest::GetProfile {
                        known_hashes,
                        synced_collection_ids: Some(synced_ids),
                        activity_since,
                    },
                )
                .await?;
                let mut resp = read_frame::<P2pResponse>(&mut recv2).await?;
                if let P2pResponse::Profile(ref mut profile) = resp {
                    let relayed = std::mem::take(&mut profile.relayed_invites);
                    if !relayed.is_empty() {
                        let ack_ids = self.apply_received_relayed_invites(relayed);
                        if !ack_ids.is_empty() {
                            if let Ok((mut send3, mut recv3)) = conn.open_bi().await {
                                let _ = write_frame(
                                    &mut send3,
                                    &P2pRequest::AckRelayedInvites {
                                        from_peer_ids: ack_ids,
                                    },
                                )
                                .await;
                                let _ = read_frame::<P2pResponse>(&mut recv3).await;
                            }
                        }
                    }
                }
                Some(resp)
            } else {
                None
            };

            Ok::<_, anyhow::Error>((hello_resp, profile_resp))
        })
        .await;

        let changed = match session_res {
            Ok(Ok((
                P2pResponse::HelloAck {
                    accepted,
                    display_name,
                    addr: remote_addr,
                },
                profile_resp,
            ))) => {
                let sanitized_name = sanitize_display_name(&display_name, "Friend");
                let sanitized_addr = match sanitize_peer_addr(addr.id, &remote_addr) {
                    Ok(a) => Some(a),
                    Err(e) => {
                        log::warn!("Ignored invalid remote_addr in HelloAck from {peer_id}: {e}");
                        None
                    }
                };
                let mut hello_changed = false;
                {
                    let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                    if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
                        if !f.online || f.display_name != sanitized_name {
                            hello_changed = true;
                        }
                        f.online = true;
                        f.last_seen_at = Some(now_rfc3339());
                        f.display_name = sanitized_name;
                        if let Some(valid_addr) = sanitized_addr {
                            f.addr = valid_addr;
                        }
                        if accepted && f.status != FriendStatus::Connected {
                            f.status = FriendStatus::Connected;
                            hello_changed = true;
                        }
                    }
                    if hello_changed {
                        let _ = save_friends_store(&self.paths, &store);
                    }
                }
                let profile_changed = if let Some(P2pResponse::Profile(profile)) = profile_resp {
                    self.apply_fetched_profile(peer_id, profile)?
                } else {
                    false
                };
                hello_changed || profile_changed
            }
            _ => {
                let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                let mut went_offline = false;
                if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
                    if f.online {
                        f.online = false;
                        went_offline = true;
                    }
                }
                if went_offline {
                    let _ = save_friends_store(&self.paths, &store);
                }
                went_offline
            }
        };

        if changed {
            self.emit_changed();
        }
        Ok(())
    }

    pub fn get_friend_share_code(&self, peer_id: &str, collection_id: &str) -> Result<String> {
        let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
        let friend = store
            .friends
            .iter()
            .find(|f| f.peer_id == peer_id)
            .ok_or_else(|| anyhow!("Friend not found"))?;
        let profile = friend
            .cached_profile
            .as_ref()
            .ok_or_else(|| anyhow!("Friend profile not cached yet"))?;
        let col = profile
            .collections
            .iter()
            .find(|c| c.id == collection_id)
            .ok_or_else(|| anyhow!("Friend collection not found"))?;
        if col.share_code.is_empty() {
            bail!("Share code body not cached yet; refresh friend while online");
        }
        Ok(col.share_code.clone())
    }
}

pub fn decode_friend_share(code: &str) -> Result<ShareManifest> {
    share::decode_share_code(code)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    use std::net::{IpAddr, Ipv4Addr, SocketAddr};
    use tempfile::tempdir;

    #[test]
    fn join_code_round_trip() {
        let key = SecretKey::generate();
        let mut addrs = BTreeSet::new();
        addrs.insert(iroh::TransportAddr::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
            41234,
        )));
        let ticket = FriendJoinTicket {
            addr: EndpointAddr {
                id: key.public(),
                addrs,
            },
            name: "Alice".into(),
        };
        let code = encode_join_code(&ticket).unwrap();
        assert!(code.starts_with(JOIN_CODE_PREFIX));
        let decoded = decode_join_code(&code).unwrap();
        assert_eq!(decoded.name, "Alice");
        assert_eq!(decoded.addr.id, key.public());
        assert_eq!(decoded.addr.addrs.len(), 1);

        let from_deep_link = decode_join_code(&format!("emperor://friend/{code}")).unwrap();
        assert_eq!(from_deep_link.addr.id, key.public());
        let from_share_url = decode_join_code(&format!(
            "https://emperor-of-modkind.github.io/open/?to=friend/{code}"
        ))
        .unwrap();
        assert_eq!(from_share_url.addr.id, key.public());
    }

    #[test]
    fn join_code_rejects_bad_prefix() {
        assert!(decode_join_code("not-a-code").is_err());
    }

    #[test]
    fn friends_store_offline_cache_persists() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let key = SecretKey::generate();
        let pid = key.public().to_string();
        let store = FriendsStore {
            display_name: "Bob".into(),
            display_name_set: true,
            profile_picture: None,
            friends: vec![FriendRecord {
                peer_id: pid.clone(),
                display_name: "Alice".into(),
                profile_picture: None,
                addr: EndpointAddr::new(key.public()),
                status: FriendStatus::Connected,
                online: true,
                last_seen_at: Some("2026-11-20T00:00:00Z".into()),
                added_at: None,
                cached_profile: Some(PeerProfileSummary {
                    peer_id: pid.clone(),
                    display_name: "Alice".into(),
                    profile_picture: None,
                    updated_at: "2026-11-20T00:00:00Z".into(),
                    collections: vec![FriendShareRef {
                        id: "active:skyrim".into(),
                        kind: FriendCollectionKind::ActiveLoadout,
                        name: "Skyrim Loadout".into(),
                        share_code: "#emperor1test".into(),
                        share_hash: "abc123".into(),
                        updated_at: "2026-11-20T00:00:00Z".into(),
                        specific_friends_only: false,
                        synced_source_collection_id: None,
                        matched_local_collection: None,
                    }],
                    friends: None,
            allow_mutual_connections: true,
                    signed_relay_profile: None,
                    activity_events: None,
                    relayed_invites: vec![],
                }),
                syncing_my_collections: Vec::new(),
                cached_via_peer_name: None,
            }],
            pending_requests: vec![],
            hidden_collection_ids: HashSet::new(),
            collection_visibility: HashMap::new(),
            friends_list_visibility: CollectionVisibility::AllFriends,
            allow_mutual_connections: true,
            allow_cache_relay: true,
            share_active_loadouts_in_feed: true,
            appear_offline: false,
            feed_events: vec![],
            feed_last_read_at: None,
            feed_cleared_at: None,
            local_activity: vec![],
            local_collection_snapshots: HashMap::new(),
            local_activity_initialized: false,
            queued_invites: vec![],
            relayed_invites: vec![],
        };
        save_friends_store(&paths, &store).unwrap();
        let loaded = load_friends_store(&paths);
        assert_eq!(loaded.display_name, "Bob");
        assert_eq!(loaded.friends.len(), 1);
        // On cold load, online resets to false while cached_profile remains intact.
        assert!(!loaded.friends[0].online);
        assert_eq!(
            loaded.friends[0]
                .cached_profile
                .as_ref()
                .unwrap()
                .collections[0]
                .share_code,
            "#emperor1test"
        );
    }

    #[tokio::test]
    async fn two_endpoints_mutual_accept_syncs_both_profiles() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let paths_a = Paths {
            config_dir: dir_a.path().join("cfg"),
            data_dir: dir_a.path().join("data"),
            cache_dir: dir_a.path().join("cache"),
        };
        let paths_b = Paths {
            config_dir: dir_b.path().join("cfg"),
            data_dir: dir_b.path().join("data"),
            cache_dir: dir_b.path().join("cache"),
        };
        fs::create_dir_all(&paths_a.data_dir).unwrap();
        fs::create_dir_all(&paths_b.data_dir).unwrap();

        // Give Peer A a saved collection so it has 1 collection to share.
        let sample_manifest = crate::share::ShareManifest {
            v: 1,
            name: Some("Alice Pack".into()),
            game: crate::share::ShareGameHint {
                id: Some("skyrim".into()),
                plugin_id: Some("skyrim".into()),
                nexus_domain: Some("skyrimspecialedition".into()),
                thunderstore_community: None,
                modio_game_id: None,
                title: Some("Skyrim SE".into()),
                project_name: None,
            },
            game_versions: None,
            mods: vec![crate::share::ShareModEntry {
                s: crate::share::ShareModSource::Nexus,
                domain: Some("skyrimspecialedition".into()),
                mod_id: Some(10),
                file_id: Some(20),
                community: None,
                namespace: None,
                name: None,
                game_id: None,
                display_name: Some("SkyUI".into()),
                version: Some("5.2".into()),
                fomod: None,
                options: None,
                enabled: None,
            }],
        };
        let code_a = crate::share::encode_share_manifest(&sample_manifest).unwrap();
        let entry_a = crate::share::entry_from_code(&code_a, Some("Alice Pack".into()), None).unwrap();
        let saved_col_id = format!("saved:{}", entry_a.id);
        crate::share::add_saved_collection(&paths_a, entry_a).unwrap();

        let svc_a = P2pService::new(paths_a);
        let svc_b = P2pService::new(paths_b);
        svc_a.set_display_name("Alice".into()).unwrap();
        svc_a
            .set_collection_sharing(saved_col_id, CollectionVisibility::AllFriends)
            .unwrap();
        svc_b.set_display_name("Bob".into()).unwrap();

        let ep_a = Endpoint::builder(presets::N0DisableRelay)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let ep_b = Endpoint::builder(presets::N0DisableRelay)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();

        let _ = svc_a.endpoint.set(ep_a.clone());
        let _ = svc_b.endpoint.set(ep_b.clone());

        let accept_a = {
            let svc = Arc::clone(&svc_a);
            let ep = ep_a.clone();
            tokio::spawn(async move {
                while let Some(inc) = ep.accept().await {
                    let s = Arc::clone(&svc);
                    tokio::spawn(async move {
                        let _ = s.handle_incoming(inc).await;
                    });
                }
            })
        };
        let accept_b = {
            let svc = Arc::clone(&svc_b);
            let ep = ep_b.clone();
            tokio::spawn(async move {
                while let Some(inc) = ep.accept().await {
                    let s = Arc::clone(&svc);
                    tokio::spawn(async move {
                        let _ = s.handle_incoming(inc).await;
                    });
                }
            })
        };

        let join_a = svc_a.get_state().unwrap().join_code.unwrap();
        let state_b1 = svc_b.add_friend_by_code(join_a).await.unwrap();
        assert_eq!(state_b1.friends.len(), 1);
        assert_eq!(state_b1.friends[0].status, FriendStatus::PendingOutgoing);

        // Alice should have a pending request from Bob.
        let state_a1 = svc_a.get_state().unwrap();
        assert_eq!(state_a1.pending_requests.len(), 1);
        let bob_id = state_a1.pending_requests[0].peer_id.clone();

        // Alice accepts Bob.
        let state_a2 = svc_a.respond_friend_request(bob_id, true).await.unwrap();
        assert_eq!(state_a2.friends.len(), 1);
        assert_eq!(state_a2.friends[0].status, FriendStatus::Connected);
        assert!(state_a2.friends[0].online);

        // Bob should now be Connected, online, AND have Alice's 1 collection cached (via reverse sync).
        tokio::time::sleep(Duration::from_millis(150)).await;
        let state_b2 = svc_b.get_state().unwrap();
        assert_eq!(state_b2.friends[0].status, FriendStatus::Connected);
        assert!(state_b2.friends[0].online);
        let alice_cols = &state_b2.friends[0]
            .cached_profile
            .as_ref()
            .expect("Bob should have Alice's cached profile")
            .collections;
        assert_eq!(alice_cols.len(), 1);
        assert_eq!(alice_cols[0].name, "Alice Pack");

        accept_a.abort();
        accept_b.abort();
    }

    #[test]
    fn collection_visibility_defaults_and_per_peer_filtering() {
        let mut store = FriendsStore::default();
        // Saved collections default to Private; active loadouts default to AllFriends.
        assert_eq!(
            resolve_collection_visibility(&store, "saved:col1"),
            CollectionVisibility::Private
        );
        assert!(!is_collection_visible_to(&store, "saved:col1", Some("peer_bob")));
        assert_eq!(
            resolve_collection_visibility(&store, "active:skyrim"),
            CollectionVisibility::AllFriends
        );
        assert!(is_collection_visible_to(&store, "active:skyrim", Some("peer_bob")));

        // Legacy hidden_collection_ids falls back to Private.
        store.hidden_collection_ids.insert("active:skyrim".into());
        assert_eq!(
            resolve_collection_visibility(&store, "active:skyrim"),
            CollectionVisibility::Private
        );

        // SpecificFriends only allows listed peers.
        store.collection_visibility.insert(
            "saved:col1".into(),
            CollectionVisibility::SpecificFriends {
                peer_ids: vec!["peer_bob".into()],
            },
        );
        assert!(is_collection_visible_to(&store, "saved:col1", Some("peer_bob")));
        assert!(!is_collection_visible_to(&store, "saved:col1", Some("peer_carol")));
        assert!(!is_collection_visible_to(&store, "saved:col1", None));
    }

    #[test]
    fn set_collection_sharing_and_remove_friend_cleanup() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let svc = P2pService::new(paths.clone());
        let state = svc
            .set_collection_sharing(
                "saved:c1".into(),
                CollectionVisibility::SpecificFriends {
                    peer_ids: vec!["peer_bob".into(), "peer_bob".into(), "peer_carol".into()],
                },
            )
            .unwrap();
        assert_eq!(state.local_collections.len(), 0);

        {
            let store = svc.store.lock().unwrap();
            assert_eq!(
                resolve_collection_visibility(&store, "saved:c1"),
                CollectionVisibility::SpecificFriends {
                    peer_ids: vec!["peer_bob".into(), "peer_carol".into()],
                }
            );
            assert!(!store.hidden_collection_ids.contains("saved:c1"));
        }

        svc.remove_friend("peer_bob".into()).unwrap();
        {
            let store = svc.store.lock().unwrap();
            assert_eq!(
                resolve_collection_visibility(&store, "saved:c1"),
                CollectionVisibility::SpecificFriends {
                    peer_ids: vec!["peer_carol".into()],
                }
            );
        }

        svc.set_collection_sharing("saved:c1".into(), CollectionVisibility::Private)
            .unwrap();
        {
            let store = svc.store.lock().unwrap();
            assert!(store.hidden_collection_ids.contains("saved:c1"));
        }
    }

    #[test]
    fn apply_fetched_profile_detects_changes_and_rehydrates_codes() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let key = SecretKey::generate();
        let peer_id = key.public().to_string();
        let addr = EndpointAddr::new(key.public());
        let svc = P2pService::new(paths);
        {
            let mut store = svc.store.lock().unwrap();
            store.friends.push(FriendRecord {
                peer_id: peer_id.clone(),
                display_name: "Bob".into(),
                profile_picture: None,
                addr,
                status: FriendStatus::Connected,
                online: false,
                last_seen_at: None,
                added_at: None,
                cached_profile: None,
                syncing_my_collections: Vec::new(),
                cached_via_peer_name: None,
            });
        }

        let initial = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Bob".into(),
            profile_picture: None,
            updated_at: "t1".into(),
            collections: vec![FriendShareRef {
                id: "saved:c1".into(),
                kind: FriendCollectionKind::SavedCollection,
                name: "Loadout".into(),
                share_code: "#emperor1abc".into(),
                share_hash: "h1".into(),
                updated_at: "t1".into(),
                specific_friends_only: false,
                synced_source_collection_id: None,
                matched_local_collection: None,
            }],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        assert!(svc.apply_fetched_profile(&peer_id, initial).unwrap());

        // Delta response with empty share_code and matching hash should rehydrate and report no change.
        let delta_same = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Bob".into(),
            profile_picture: None,
            updated_at: "t2".into(),
            collections: vec![FriendShareRef {
                id: "saved:c1".into(),
                kind: FriendCollectionKind::SavedCollection,
                name: "Loadout".into(),
                share_code: String::new(),
                share_hash: "h1".into(),
                updated_at: "t2".into(),
                specific_friends_only: false,
                synced_source_collection_id: None,
                matched_local_collection: None,
            }],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        assert!(!svc.apply_fetched_profile(&peer_id, delta_same).unwrap());
        assert_eq!(
            svc.get_friend_share_code(&peer_id, "saved:c1").unwrap(),
            "#emperor1abc"
        );

        // Updated share_hash + new share_code reports change.
        let updated = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Bob".into(),
            profile_picture: None,
            updated_at: "t3".into(),
            collections: vec![FriendShareRef {
                id: "saved:c1".into(),
                kind: FriendCollectionKind::SavedCollection,
                name: "Loadout".into(),
                share_code: "#emperor1xyz".into(),
                share_hash: "h2".into(),
                updated_at: "t3".into(),
                specific_friends_only: false,
                synced_source_collection_id: None,
                matched_local_collection: None,
            }],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        assert!(svc.apply_fetched_profile(&peer_id, updated).unwrap());
        assert_eq!(
            svc.get_friend_share_code(&peer_id, "saved:c1").unwrap(),
            "#emperor1xyz"
        );
    }

    #[test]
    fn display_name_set_defaults_false_and_migrates_legacy_stores() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        // Fresh store has display_name_set = false.
        let fresh = load_friends_store(&paths);
        assert!(!fresh.display_name_set);
        assert!(fresh.display_name.is_empty());

        // Legacy store with display_name and no display_name_set field migrates to true.
        fs::write(
            store_path(&paths),
            r#"{"display_name":"LegacyUser","friends":[]}"#,
        )
        .unwrap();
        let migrated = load_friends_store(&paths);
        assert!(migrated.display_name_set);
        assert_eq!(migrated.display_name, "LegacyUser");

        // set_display_name sets display_name_set = true.
        let svc = P2pService::new(paths.clone());
        let state = svc.set_display_name("NewName".into()).unwrap();
        assert!(state.display_name_set);
        assert_eq!(state.display_name, "NewName");
    }

    #[test]
    fn profile_picture_validation_and_persistence() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let svc = P2pService::new(paths);
        let valid = "data:image/png;base64,iVBORw0KGgo=";
        let state = svc.set_profile_picture(Some(valid.into())).unwrap();
        assert_eq!(state.profile_picture.as_deref(), Some(valid));

        // Rejects non-image prefix.
        assert!(svc
            .set_profile_picture(Some("data:text/plain;base64,SGVsbG8=".into()))
            .is_err());

        // Rejects oversized payload.
        let huge = format!("data:image/png;base64,{}", "A".repeat(MAX_PROFILE_PICTURE_BYTES));
        assert!(svc.set_profile_picture(Some(huge)).is_err());

        // Clears on None.
        let cleared = svc.set_profile_picture(None).unwrap();
        assert!(cleared.profile_picture.is_none());
    }

    #[test]
    fn appear_offline_toggles_and_persists() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let svc = P2pService::new(paths.clone());
        assert!(!svc.get_state().unwrap().appear_offline);

        let offline = svc.set_appear_offline(true).unwrap();
        assert!(offline.appear_offline);

        let reloaded = P2pService::new(paths);
        assert!(reloaded.get_state().unwrap().appear_offline);

        let online = reloaded.set_appear_offline(false).unwrap();
        assert!(!online.appear_offline);
    }

    #[test]
    fn join_code_rejects_decompression_bomb() {
        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::best());
        let chunk = vec![0u8; 16 * 1024];
        for _ in 0..8 {
            encoder.write_all(&chunk).unwrap();
        }
        let compressed = encoder.finish().unwrap();
        let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed);
        let code = format!("{JOIN_CODE_PREFIX}{encoded}");
        let err = decode_join_code(&code).unwrap_err();
        assert!(err.to_string().contains("exceeds maximum allowed size"));
    }

    #[test]
    fn sanitize_peer_addr_enforces_identity_and_filters_unsafe_addrs() {
        let key_a = SecretKey::generate();
        let key_b = SecretKey::generate();

        let mut addrs = BTreeSet::new();
        addrs.insert(TransportAddr::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)),
            1234,
        )));
        addrs.insert(TransportAddr::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(224, 0, 0, 1)),
            1234,
        )));
        addrs.insert(TransportAddr::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            41234,
        )));

        // Mismatched EndpointId must be rejected.
        let wrong_id = EndpointAddr {
            id: key_b.public(),
            addrs: addrs.clone(),
        };
        assert!(sanitize_peer_addr(key_a.public(), &wrong_id).is_err());

        // Valid EndpointId filters out unspecified and multicast IPs.
        let right_id = EndpointAddr {
            id: key_a.public(),
            addrs,
        };
        let sanitized = sanitize_peer_addr(key_a.public(), &right_id).unwrap();
        assert_eq!(sanitized.id, key_a.public());
        assert_eq!(sanitized.addrs.len(), 1);
    }

    #[test]
    fn join_ticket_addr_strips_ips_when_relay_present_and_keeps_fallback() {
        let key = SecretKey::generate();
        let relay: iroh::RelayUrl = "https://use1-1.relay.n0.iroh-canary.iroh.link.".parse().unwrap();

        let mut with_relay = BTreeSet::new();
        with_relay.insert(TransportAddr::Relay(relay.clone()));
        with_relay.insert(TransportAddr::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)),
            41234,
        )));
        let stripped = join_ticket_addr(&EndpointAddr {
            id: key.public(),
            addrs: with_relay,
        });
        assert_eq!(stripped.addrs.len(), 1);
        assert!(stripped.addrs.contains(&TransportAddr::Relay(relay)));

        let mut ip_only = BTreeSet::new();
        let sock = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 50)), 41234);
        ip_only.insert(TransportAddr::Ip(sock));
        let fallback = join_ticket_addr(&EndpointAddr {
            id: key.public(),
            addrs: ip_only,
        });
        assert_eq!(fallback.addrs.len(), 1);
        assert!(fallback.addrs.contains(&TransportAddr::Ip(sock)));
    }

    #[test]
    fn apply_fetched_profile_rejects_mismatched_peer_and_sanitizes_fields() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let key = SecretKey::generate();
        let other = SecretKey::generate();
        let peer_id = key.public().to_string();
        let svc = P2pService::new(paths);
        {
            let mut store = svc.store.lock().unwrap();
            store.friends.push(FriendRecord {
                peer_id: peer_id.clone(),
                display_name: "Bob".into(),
                profile_picture: None,
                addr: EndpointAddr::new(key.public()),
                status: FriendStatus::Connected,
                online: false,
                last_seen_at: None,
                added_at: None,
                cached_profile: None,
                syncing_my_collections: Vec::new(),
                cached_via_peer_name: None,
            });
        }

        // Mismatched peer_id must fail.
        let wrong_peer = PeerProfileSummary {
            peer_id: other.public().to_string(),
            display_name: "Mallory".into(),
            profile_picture: None,
            updated_at: "t1".into(),
            collections: vec![],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        assert!(svc.apply_fetched_profile(&peer_id, wrong_peer).is_err());

        // Malicious SVG/asset avatar & bidi control chars are stripped/sanitized.
        let malicious = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Bob\u{202E}\x07Evil".into(),
            profile_picture: Some("data:image/svg+xml;base64,PHN2Zz4=".into()),
            updated_at: "t2".into(),
            collections: vec![],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        assert!(svc.apply_fetched_profile(&peer_id, malicious).unwrap());
        let state = svc.get_state().unwrap();
        assert_eq!(state.friends[0].display_name, "BobEvil");
        assert!(state.friends[0].profile_picture.is_none());

        // asset:// URL avatar is also stripped.
        let asset_avatar = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Bob".into(),
            profile_picture: Some("asset://localhost/home/user/.ssh/id_rsa".into()),
            updated_at: "t3".into(),
            collections: vec![],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        let _ = svc.apply_fetched_profile(&peer_id, asset_avatar).unwrap();
        assert!(svc.get_state().unwrap().friends[0].profile_picture.is_none());
    }

    #[test]
    fn default_display_name_does_not_leak_os_user() {
        assert_eq!(default_display_name(), "Emperor User");
    }

    #[cfg(unix)]
    #[test]
    fn secret_key_and_store_created_with_0600_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let _ = load_or_create_secret_key(&paths).unwrap();
        let key_mode = fs::metadata(identity_key_path(&paths))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(key_mode, 0o600);

        save_friends_store(&paths, &FriendsStore::default()).unwrap();
        let store_mode = fs::metadata(store_path(&paths))
            .unwrap()
            .permissions()
            .mode()
            & 0o777;
        assert_eq!(store_mode, 0o600);
    }

    fn make_share_code_with_mods(game_title: &str, mods: Vec<(u64, u64, &str, &str)>) -> String {
        let manifest = crate::share::ShareManifest {
            v: 1,
            name: Some(format!("{game_title} Pack")),
            game: crate::share::ShareGameHint {
                id: Some("skyrim".into()),
                plugin_id: Some("skyrim".into()),
                nexus_domain: Some("skyrimspecialedition".into()),
                thunderstore_community: None,
                modio_game_id: None,
                title: Some(game_title.into()),
                project_name: None,
            },
            game_versions: None,
            mods: mods
                .into_iter()
                .map(|(mod_id, file_id, display_name, version)| crate::share::ShareModEntry {
                    s: crate::share::ShareModSource::Nexus,
                    domain: Some("skyrimspecialedition".into()),
                    mod_id: Some(mod_id),
                    file_id: Some(file_id),
                    community: None,
                    namespace: None,
                    name: None,
                    game_id: None,
                    display_name: Some(display_name.into()),
                    version: Some(version.into()),
                    fomod: None,
                    options: None,
                    enabled: Some(true),
                })
                .collect(),
        };
        crate::share::encode_share_manifest(&manifest).unwrap()
    }

    #[test]
    fn feed_tracks_add_update_rename_and_prunes_on_visibility_revocation() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let key = SecretKey::generate();
        let peer_id = key.public().to_string();
        let svc = P2pService::new(paths.clone());
        {
            let mut store = svc.store.lock().unwrap();
            store.friends.push(FriendRecord {
                peer_id: peer_id.clone(),
                display_name: "Alice".into(),
                profile_picture: None,
                addr: EndpointAddr::new(key.public()),
                status: FriendStatus::Connected,
                online: false,
                last_seen_at: None,
                added_at: None,
                cached_profile: None,
                syncing_my_collections: Vec::new(),
                cached_via_peer_name: None,
            });
        }

        let code_v1 = make_share_code_with_mods(
            "Skyrim SE",
            vec![(10, 100, "SkyUI", "5.1"), (20, 200, "OldMod", "1.0")],
        );
        let hash_v1 = crate::share::share_hash(&code_v1);

        // Initial baseline sync does not emit feed events.
        let baseline = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            updated_at: "2026-11-21T00:00:00Z".into(),
            collections: vec![FriendShareRef {
                id: "active:skyrim".into(),
                kind: FriendCollectionKind::ActiveLoadout,
                name: "Skyrim Loadout".into(),
                share_code: code_v1.clone(),
                share_hash: hash_v1.clone(),
                updated_at: "2026-11-21T00:00:00Z".into(),
                specific_friends_only: false,
                synced_source_collection_id: None,
                matched_local_collection: None,
            }],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        svc.apply_fetched_profile(&peer_id, baseline).unwrap();
        assert!(svc.get_state().unwrap().feed_events.is_empty());

        // Unchanged delta sync (with bumped updated_at) produces zero feed events.
        let delta_same = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            updated_at: "2026-11-21T00:01:00Z".into(),
            collections: vec![FriendShareRef {
                id: "active:skyrim".into(),
                kind: FriendCollectionKind::ActiveLoadout,
                name: "Skyrim Loadout".into(),
                share_code: String::new(),
                share_hash: hash_v1.clone(),
                updated_at: "2026-11-21T00:01:00Z".into(),
                specific_friends_only: false,
                synced_source_collection_id: None,
                matched_local_collection: None,
            }],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        svc.apply_fetched_profile(&peer_id, delta_same).unwrap();
        assert!(svc.get_state().unwrap().feed_events.is_empty());

        // Update existing collection (update SkyUI, remove OldMod, add USSEP) + add a new saved collection.
        let code_v2 = make_share_code_with_mods(
            "Skyrim SE",
            vec![(10, 101, "SkyUI", "5.2"), (30, 300, "USSEP", "4.3")],
        );
        let hash_v2 = crate::share::share_hash(&code_v2);
        let code_saved = make_share_code_with_mods("Skyrim SE", vec![(40, 400, "SMIM", "2.0")]);
        let hash_saved = crate::share::share_hash(&code_saved);

        let updated = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            updated_at: "2026-11-21T00:02:00Z".into(),
            collections: vec![
                FriendShareRef {
                    id: "active:skyrim".into(),
                    kind: FriendCollectionKind::ActiveLoadout,
                    name: "Skyrim Loadout".into(),
                    share_code: code_v2.clone(),
                    share_hash: hash_v2.clone(),
                    updated_at: "2026-11-21T00:02:00Z".into(),
                    specific_friends_only: false,
                    synced_source_collection_id: None,
                    matched_local_collection: None,
                },
                FriendShareRef {
                    id: "saved:cozy".into(),
                    kind: FriendCollectionKind::SavedCollection,
                    name: "Cozy Pack".into(),
                    share_code: code_saved.clone(),
                    share_hash: hash_saved.clone(),
                    updated_at: "2026-11-21T00:02:00Z".into(),
                    specific_friends_only: false,
                    synced_source_collection_id: None,
                    matched_local_collection: None,
                },
            ],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        svc.apply_fetched_profile(&peer_id, updated).unwrap();
        let state = svc.get_state().unwrap();
        assert_eq!(state.feed_events.len(), 2);

        let upd_ev = state
            .feed_events
            .iter()
            .find(|e| e.collection_id == "active:skyrim")
            .unwrap();
        assert_eq!(upd_ev.kind, FriendFeedEventKind::CollectionUpdated);
        assert_eq!(upd_ev.game_title.as_deref(), Some("Skyrim SE"));
        assert_eq!(upd_ev.mod_count, Some(2));
        let diff = upd_ev.diff.as_ref().unwrap();
        assert_eq!(diff.mods_added, vec!["USSEP"]);
        assert_eq!(diff.mods_added_count, 1);
        assert_eq!(diff.mods_removed, vec!["OldMod"]);
        assert_eq!(diff.mods_removed_count, 1);
        assert_eq!(diff.mods_updated, vec!["SkyUI"]);
        assert_eq!(diff.mods_updated_count, 1);

        let add_ev = state
            .feed_events
            .iter()
            .find(|e| e.collection_id == "saved:cozy")
            .unwrap();
        assert_eq!(add_ev.kind, FriendFeedEventKind::CollectionAdded);
        assert_eq!(add_ev.mod_count, Some(1));

        // Rename saved:cozy without changing hash.
        let renamed = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            updated_at: "2026-11-21T00:03:00Z".into(),
            collections: vec![
                FriendShareRef {
                    id: "active:skyrim".into(),
                    kind: FriendCollectionKind::ActiveLoadout,
                    name: "Skyrim Loadout".into(),
                    share_code: String::new(),
                    share_hash: hash_v2.clone(),
                    updated_at: "2026-11-21T00:03:00Z".into(),
                    specific_friends_only: false,
                    synced_source_collection_id: None,
                    matched_local_collection: None,
                },
                FriendShareRef {
                    id: "saved:cozy".into(),
                    kind: FriendCollectionKind::SavedCollection,
                    name: "Ultra Cozy Pack".into(),
                    share_code: String::new(),
                    share_hash: hash_saved.clone(),
                    updated_at: "2026-11-21T00:03:00Z".into(),
                    specific_friends_only: false,
                    synced_source_collection_id: None,
                    matched_local_collection: None,
                },
            ],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        svc.apply_fetched_profile(&peer_id, renamed).unwrap();
        let state = svc.get_state().unwrap();
        assert_eq!(state.feed_events.len(), 3);
        assert_eq!(state.feed_events[0].kind, FriendFeedEventKind::CollectionRenamed);
        assert_eq!(state.feed_events[0].previous_name.as_deref(), Some("Cozy Pack"));
        assert_eq!(state.feed_events[0].collection_name, "Ultra Cozy Pack");

        // Visibility revocation: Alice hides saved:cozy -> all saved:cozy feed events are pruned.
        let revoked = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            updated_at: "2026-11-21T00:04:00Z".into(),
            collections: vec![FriendShareRef {
                id: "active:skyrim".into(),
                kind: FriendCollectionKind::ActiveLoadout,
                name: "Skyrim Loadout".into(),
                share_code: String::new(),
                share_hash: hash_v2.clone(),
                updated_at: "2026-11-21T00:04:00Z".into(),
                specific_friends_only: false,
                synced_source_collection_id: None,
                matched_local_collection: None,
            }],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        svc.apply_fetched_profile(&peer_id, revoked).unwrap();
        let state = svc.get_state().unwrap();
        assert_eq!(state.feed_events.len(), 1);
        assert_eq!(state.feed_events[0].collection_id, "active:skyrim");

        // Mark read & clear feed persist.
        let marked = svc.mark_feed_read().unwrap();
        assert!(marked.feed_last_read_at.is_some());
        let reloaded = P2pService::new(paths.clone());
        assert_eq!(reloaded.get_state().unwrap().feed_last_read_at, marked.feed_last_read_at);

        let cleared = reloaded.clear_feed().unwrap();
        assert!(cleared.feed_events.is_empty());

        // Removing a friend also removes their feed events.
        let readded = PeerProfileSummary {
            peer_id: peer_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            updated_at: "2026-11-21T00:05:00Z".into(),
            collections: vec![
                FriendShareRef {
                    id: "active:skyrim".into(),
                    kind: FriendCollectionKind::ActiveLoadout,
                    name: "Skyrim Loadout".into(),
                    share_code: String::new(),
                    share_hash: hash_v2,
                    updated_at: "2026-11-21T00:05:00Z".into(),
                    specific_friends_only: false,
                    synced_source_collection_id: None,
                    matched_local_collection: None,
                },
                FriendShareRef {
                    id: "saved:cozy".into(),
                    kind: FriendCollectionKind::SavedCollection,
                    name: "Back Again".into(),
                    share_code: code_saved,
                    share_hash: hash_saved,
                    updated_at: "2026-11-21T00:05:00Z".into(),
                    specific_friends_only: false,
                    synced_source_collection_id: None,
                    matched_local_collection: None,
                },
            ],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };
        reloaded.apply_fetched_profile(&peer_id, readded).unwrap();
        assert_eq!(reloaded.get_state().unwrap().feed_events.len(), 1);
        let after_remove = reloaded.remove_friend(peer_id).unwrap();
        assert!(after_remove.feed_events.is_empty());
    }

    #[test]
    fn friends_list_visibility_defaults_and_per_peer_filtering() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let svc = P2pService::new(paths.clone());
        let initial = svc.get_state().unwrap();
        assert_eq!(initial.friends_list_visibility, CollectionVisibility::AllFriends);

        {
            let store = svc.store.lock().unwrap();
            assert!(is_friends_list_visible_to(&store, Some("peer_bob")));
            assert!(is_friends_list_visible_to(&store, Some("peer_carol")));
        }

        // SpecificFriends only allows listed peers and deduplicates.
        let specific = svc
            .set_friends_list_visibility(CollectionVisibility::SpecificFriends {
                peer_ids: vec!["peer_bob".into(), "peer_bob".into(), "peer_carol".into()],
            })
            .unwrap();
        assert_eq!(
            specific.friends_list_visibility,
            CollectionVisibility::SpecificFriends {
                peer_ids: vec!["peer_bob".into(), "peer_carol".into()],
            }
        );
        {
            let store = svc.store.lock().unwrap();
            assert!(is_friends_list_visible_to(&store, Some("peer_bob")));
            assert!(!is_friends_list_visible_to(&store, Some("peer_dave")));
            assert!(!is_friends_list_visible_to(&store, None));
        }

        // Removing a friend prunes them from friends_list_visibility.
        svc.remove_friend("peer_bob".into()).unwrap();
        assert_eq!(
            svc.get_state().unwrap().friends_list_visibility,
            CollectionVisibility::SpecificFriends {
                peer_ids: vec!["peer_carol".into()],
            }
        );

        // Private hides from everyone and persists across reload.
        svc.set_friends_list_visibility(CollectionVisibility::Private)
            .unwrap();
        let reloaded = P2pService::new(paths);
        assert_eq!(
            reloaded.get_state().unwrap().friends_list_visibility,
            CollectionVisibility::Private
        );
        {
            let store = reloaded.store.lock().unwrap();
            assert!(!is_friends_list_visible_to(&store, Some("peer_carol")));
        }
    }

    #[test]
    fn build_local_profile_shares_connected_friends_with_relay_sanitized_addrs() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let self_key = SecretKey::generate();
        let bob_key = SecretKey::generate();
        let carol_key = SecretKey::generate();
        let pending_key = SecretKey::generate();
        let relay: iroh::RelayUrl = "https://use1-1.relay.n0.iroh-canary.iroh.link.".parse().unwrap();

        let mut carol_addrs = BTreeSet::new();
        carol_addrs.insert(TransportAddr::Relay(relay.clone()));
        carol_addrs.insert(TransportAddr::Ip(SocketAddr::new(
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 99)),
            41234,
        )));

        let mut store = FriendsStore::default();
        store.friends.push(FriendRecord {
            peer_id: bob_key.public().to_string(),
            display_name: "Bob".into(),
            profile_picture: None,
            addr: EndpointAddr::new(bob_key.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: None,
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        store.friends.push(FriendRecord {
            peer_id: carol_key.public().to_string(),
            display_name: "Carol".into(),
            profile_picture: None,
            addr: EndpointAddr {
                id: carol_key.public(),
                addrs: carol_addrs,
            },
            status: FriendStatus::Connected,
            online: false,
            last_seen_at: None,
            added_at: None,
            cached_profile: Some(PeerProfileSummary {
                peer_id: carol_key.public().to_string(),
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: "2026-11-21T00:00:00Z".into(),
                collections: vec![],
                friends: None,
                allow_mutual_connections: true,
                signed_relay_profile: None,
                activity_events: None,
                relayed_invites: vec![],
            }),
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        store.friends.push(FriendRecord {
            peer_id: pending_key.public().to_string(),
            display_name: "PendingPete".into(),
            profile_picture: None,
            addr: EndpointAddr::new(pending_key.public()),
            status: FriendStatus::PendingOutgoing,
            online: false,
            last_seen_at: None,
            added_at: None,
            cached_profile: None,
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });

        let bob_id = bob_key.public().to_string();
        let profile = build_local_profile(
            &paths,
            &self_key.public().to_string(),
            &store,
            None,
            Some(&bob_id),
            None,
        );
        let shared = profile.friends.expect("AllFriends should share friends list");
        // Excludes viewer (Bob) and pending outgoing (PendingPete), includes Carol.
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].peer_id, carol_key.public().to_string());
        assert_eq!(shared[0].display_name, "Carol");
        // Raw LAN IP is stripped when relay is present.
        let shared_addr = shared[0].addr.as_ref().unwrap();
        assert_eq!(shared_addr.addrs.len(), 1);
        assert!(shared_addr.addrs.contains(&TransportAddr::Relay(relay)));

        // Private hides friends list completely.
        store.friends_list_visibility = CollectionVisibility::Private;
        let private_profile = build_local_profile(
            &paths,
            &self_key.public().to_string(),
            &store,
            None,
            Some(&bob_id),
            None,
        );
        assert!(private_profile.friends.is_none());
    }

    #[test]
    fn apply_fetched_profile_sanitizes_shared_friends() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let bob_key = SecretKey::generate();
        let carol_key = SecretKey::generate();
        let bob_id = bob_key.public().to_string();
        let carol_id = carol_key.public().to_string();

        let svc = P2pService::new(paths);
        {
            let mut store = svc.store.lock().unwrap();
            store.friends.push(FriendRecord {
                peer_id: bob_id.clone(),
                display_name: "Bob".into(),
                profile_picture: None,
                addr: EndpointAddr::new(bob_key.public()),
                status: FriendStatus::Connected,
                online: false,
                last_seen_at: None,
                added_at: None,
                cached_profile: None,
                syncing_my_collections: Vec::new(),
                cached_via_peer_name: None,
            });
        }

        let profile = PeerProfileSummary {
            peer_id: bob_id.clone(),
            display_name: "Bob".into(),
            profile_picture: None,
            updated_at: "t1".into(),
            collections: vec![],
            friends: Some(vec![
                // Invalid peer_id -> dropped
                SharedFriendRef {
                    peer_id: "not-a-valid-endpoint-id".into(),
                    display_name: "Bad".into(),
                    addr: None,
                    profile_updated_at: None,
                    online: None,
                },
                // Self-reference (Bob listing Bob) -> dropped
                SharedFriendRef {
                    peer_id: bob_id.clone(),
                    display_name: "Bob".into(),
                    addr: None,
                    profile_updated_at: None,
                    online: None,
                },
                // Valid friend with bidi chars & mismatched addr -> name sanitized, bad addr dropped
                SharedFriendRef {
                    peer_id: carol_id.clone(),
                    display_name: "Carol\u{202E}\x07".into(),
                    addr: Some(EndpointAddr::new(bob_key.public())),
                    profile_updated_at: None,
                    online: None,
                },
                // Duplicate Carol -> dropped
                SharedFriendRef {
                    peer_id: carol_id.clone(),
                    display_name: "Carol Dup".into(),
                    addr: None,
                    profile_updated_at: None,
                    online: None,
                },
            ]),
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: None,
            relayed_invites: vec![],
        };

        assert!(svc.apply_fetched_profile(&bob_id, profile).unwrap());
        let state = svc.get_state().unwrap();
        let cached_friends = state.friends[0]
            .cached_profile
            .as_ref()
            .unwrap()
            .friends
            .as_ref()
            .unwrap();
        assert_eq!(cached_friends.len(), 1);
        assert_eq!(cached_friends[0].peer_id, carol_id);
        assert_eq!(cached_friends[0].display_name, "Carol");
        assert!(cached_friends[0].addr.is_none());
    }

    #[tokio::test]
    async fn three_endpoints_discover_and_add_friend_by_peer() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let dir_c = tempdir().unwrap();
        let mk_paths = |d: &tempfile::TempDir| {
            let p = Paths {
                config_dir: d.path().join("cfg"),
                data_dir: d.path().join("data"),
                cache_dir: d.path().join("cache"),
            };
            fs::create_dir_all(&p.data_dir).unwrap();
            p
        };

        let svc_a = P2pService::new(mk_paths(&dir_a));
        let svc_b = P2pService::new(mk_paths(&dir_b));
        let svc_c = P2pService::new(mk_paths(&dir_c));
        svc_a.set_display_name("Alice".into()).unwrap();
        svc_b.set_display_name("Bob".into()).unwrap();
        svc_c.set_display_name("Carol".into()).unwrap();

        let ep_a = Endpoint::builder(presets::N0DisableRelay)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let ep_b = Endpoint::builder(presets::N0DisableRelay)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let ep_c = Endpoint::builder(presets::N0DisableRelay)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();

        let _ = svc_a.endpoint.set(ep_a.clone());
        let _ = svc_b.endpoint.set(ep_b.clone());
        let _ = svc_c.endpoint.set(ep_c.clone());

        let spawn_accept = |svc: Arc<P2pService>, ep: Endpoint| {
            tokio::spawn(async move {
                while let Some(inc) = ep.accept().await {
                    let s = Arc::clone(&svc);
                    tokio::spawn(async move {
                        let _ = s.handle_incoming(inc).await;
                    });
                }
            })
        };
        let accept_a = spawn_accept(Arc::clone(&svc_a), ep_a.clone());
        let accept_b = spawn_accept(Arc::clone(&svc_b), ep_b.clone());
        let accept_c = spawn_accept(Arc::clone(&svc_c), ep_c.clone());

        // 1. Connect Bob <-> Carol
        let join_c = svc_c.get_state().unwrap().join_code.unwrap();
        svc_b.add_friend_by_code(join_c).await.unwrap();
        let bob_id = ep_b.id().to_string();
        svc_c.respond_friend_request(bob_id, true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;

        // 2. Connect Alice <-> Bob
        let join_b = svc_b.get_state().unwrap().join_code.unwrap();
        svc_a.add_friend_by_code(join_b).await.unwrap();
        let alice_id = ep_a.id().to_string();
        svc_b.respond_friend_request(alice_id.clone(), true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Alice should have Bob's cached friends list containing Carol.
        let state_a = svc_a.get_state().unwrap();
        let bob_friends = state_a.friends[0]
            .cached_profile
            .as_ref()
            .unwrap()
            .friends
            .as_ref()
            .expect("Bob's friends list should be visible to Alice");
        let carol_id = ep_c.id().to_string();
        assert_eq!(bob_friends.len(), 1);
        assert_eq!(bob_friends[0].peer_id, carol_id);
        assert_eq!(bob_friends[0].display_name, "Carol");

        // 3. Alice adds Carol directly by peer_id (using cached addr from Bob's friends list, no join code).
        let state_a2 = svc_a
            .add_friend_by_peer(carol_id.clone(), Some("Carol".into()))
            .await
            .unwrap();
        assert_eq!(state_a2.friends.len(), 2);
        let carol_rec = state_a2.friends.iter().find(|f| f.peer_id == carol_id).unwrap();
        assert_eq!(carol_rec.status, FriendStatus::PendingOutgoing);

        // Carol should have a pending request from Alice and accept it.
        let state_c = svc_c.get_state().unwrap();
        assert_eq!(state_c.pending_requests.len(), 1);
        assert_eq!(state_c.pending_requests[0].peer_id, alice_id);

        let state_c2 = svc_c.respond_friend_request(alice_id, true).await.unwrap();
        assert!(state_c2
            .friends
            .iter()
            .any(|f| f.peer_id == ep_a.id().to_string() && f.status == FriendStatus::Connected));

        accept_a.abort();
        accept_b.abort();
        accept_c.abort();
    }

    #[test]
    fn allow_mutual_connections_opt_out_excludes_peer_from_shared_friends_list() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let svc = P2pService::new(paths.clone());
        // Defaults to true and friends list defaults to AllFriends.
        let initial = svc.get_state().unwrap();
        assert!(initial.allow_mutual_connections);
        assert_eq!(
            initial.friends_list_visibility,
            CollectionVisibility::AllFriends
        );

        // Persists false across reload.
        let disabled = svc.set_allow_mutual_connections(false).unwrap();
        assert!(!disabled.allow_mutual_connections);
        let reloaded = P2pService::new(paths.clone());
        assert!(!reloaded.get_state().unwrap().allow_mutual_connections);

        // When Carol sets allow_mutual_connections = false in her cached profile,
        // Bob omits Carol from Bob's shared friends list built for Alice.
        let self_key = SecretKey::generate();
        let alice_key = SecretKey::generate();
        let carol_key = SecretKey::generate();
        let dave_key = SecretKey::generate();
        let alice_id = alice_key.public().to_string();
        let carol_id = carol_key.public().to_string();
        let dave_id = dave_key.public().to_string();

        let mut store = FriendsStore::default();
        store.friends.push(FriendRecord {
            peer_id: carol_id.clone(),
            display_name: "Carol".into(),
            profile_picture: None,
            addr: EndpointAddr::new(carol_key.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: Some(PeerProfileSummary {
                peer_id: carol_id,
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: "t1".into(),
                collections: vec![],
                friends: None,
                allow_mutual_connections: false,
                signed_relay_profile: None,
                activity_events: None,
                relayed_invites: vec![],
            }),
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        store.friends.push(FriendRecord {
            peer_id: dave_id.clone(),
            display_name: "Dave".into(),
            profile_picture: None,
            addr: EndpointAddr::new(dave_key.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: Some(PeerProfileSummary {
                peer_id: dave_id.clone(),
                display_name: "Dave".into(),
                profile_picture: None,
                updated_at: "t1".into(),
                collections: vec![],
                friends: None,
                allow_mutual_connections: true,
                signed_relay_profile: None,
                activity_events: None,
                relayed_invites: vec![],
            }),
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });

        let profile_for_alice = build_local_profile(
            &paths,
            &self_key.public().to_string(),
            &store,
            None,
            Some(&alice_id),
            None,
        );
        let shared = profile_for_alice.friends.unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].peer_id, dave_id);
        assert_eq!(shared[0].display_name, "Dave");
    }

    #[test]
    fn get_profile_request_deserializes_with_and_without_synced_ids() {
        let legacy_json = r#"{"type":"get_profile","known_hashes":{"saved:s1":"h1"}}"#;
        let legacy: P2pRequest = serde_json::from_str(legacy_json).unwrap();
        match legacy {
            P2pRequest::GetProfile {
                known_hashes,
                synced_collection_ids,
                ..
            } => {
                assert_eq!(known_hashes.get("saved:s1").map(String::as_str), Some("h1"));
                assert_eq!(synced_collection_ids, None);
            }
            _ => panic!("expected GetProfile"),
        }

        let modern_json = r#"{"type":"get_profile","known_hashes":{},"synced_collection_ids":["saved:s1","active:skyrim"]}"#;
        let modern: P2pRequest = serde_json::from_str(modern_json).unwrap();
        match modern {
            P2pRequest::GetProfile {
                synced_collection_ids,
                ..
            } => {
                assert_eq!(
                    synced_collection_ids,
                    Some(vec!["saved:s1".to_string(), "active:skyrim".to_string()])
                );
            }
            _ => panic!("expected GetProfile"),
        }
    }

    #[test]
    fn sanitize_synced_collection_ids_respects_visibility_and_dedups() {
        let mut store = FriendsStore::default();
        store
            .collection_visibility
            .insert("saved:public".into(), CollectionVisibility::AllFriends);
        store
            .collection_visibility
            .insert("saved:private".into(), CollectionVisibility::Private);
        store.collection_visibility.insert(
            "saved:alice_only".into(),
            CollectionVisibility::SpecificFriends {
                peer_ids: vec!["alice".into()],
            },
        );

        let sanitized = sanitize_synced_collection_ids(
            &store,
            "alice",
            vec![
                "saved:public".into(),
                "saved:private".into(),
                "saved:alice_only".into(),
                "saved:public".into(),
                "".into(),
            ],
        );
        assert_eq!(
            sanitized,
            vec!["saved:alice_only".to_string(), "saved:public".to_string()]
        );

        let bob_sanitized = sanitize_synced_collection_ids(
            &store,
            "bob",
            vec!["saved:alice_only".into(), "saved:public".into()],
        );
        assert_eq!(bob_sanitized, vec!["saved:public".to_string()]);
    }

    #[test]
    fn signed_relay_profile_excludes_specific_friends_collections() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let saved_store = crate::share::SavedCollectionsStore {
            collections: vec![
                crate::share::SavedCollectionEntry {
                    id: "pub1".into(),
                    name: "Public Pack".into(),
                    code: "#emperor1pub".into(),
                    created_at: "2026-11-21T00:00:00Z".into(),
                    updated_at: "2026-11-21T00:00:00Z".into(),
                    source_game_id: None,
                    game: crate::share::ShareGameHint::default(),
                    game_versions: None,
                    mod_count: 0,
                    nexus_count: 0,
                    thunderstore_count: 0,
                    modio_count: 0,
                    share_id: None,
                },
                crate::share::SavedCollectionEntry {
                    id: "spec1".into(),
                    name: "Bob Only Pack".into(),
                    code: "#emperor1spec".into(),
                    created_at: "2026-11-21T00:00:00Z".into(),
                    updated_at: "2026-11-21T00:00:00Z".into(),
                    source_game_id: None,
                    game: crate::share::ShareGameHint::default(),
                    game_versions: None,
                    mod_count: 0,
                    nexus_count: 0,
                    thunderstore_count: 0,
                    modio_count: 0,
                    share_id: None,
                },
                crate::share::SavedCollectionEntry {
                    id: "priv1".into(),
                    name: "Private Pack".into(),
                    code: "#emperor1priv".into(),
                    created_at: "2026-11-21T00:00:00Z".into(),
                    updated_at: "2026-11-21T00:00:00Z".into(),
                    source_game_id: None,
                    game: crate::share::ShareGameHint::default(),
                    game_versions: None,
                    mod_count: 0,
                    nexus_count: 0,
                    thunderstore_count: 0,
                    modio_count: 0,
                    share_id: None,
                },
            ],
        };
        crate::share::save_saved_collections(&paths, &saved_store).unwrap();

        let carol_key = SecretKey::generate();
        let bob_key = SecretKey::generate();
        let carol_id = carol_key.public().to_string();
        let bob_id = bob_key.public().to_string();

        let mut store = FriendsStore::default();
        store
            .collection_visibility
            .insert("saved:pub1".into(), CollectionVisibility::AllFriends);
        store.collection_visibility.insert(
            "saved:spec1".into(),
            CollectionVisibility::SpecificFriends {
                peer_ids: vec![bob_id.clone()],
            },
        );
        store
            .collection_visibility
            .insert("saved:priv1".into(), CollectionVisibility::Private);

        let profile_for_bob = build_local_profile(
            &paths,
            &carol_id,
            &store,
            None,
            Some(&bob_id),
            Some(&carol_key),
        );

        // Bob sees both pub1 and spec1 directly, with specific_friends_only marked.
        assert_eq!(profile_for_bob.collections.len(), 2);
        let spec_ref = profile_for_bob
            .collections
            .iter()
            .find(|c| c.id == "saved:spec1")
            .unwrap();
        assert!(spec_ref.specific_friends_only);

        // Signed relay payload only contains AllFriends collection (pub1).
        let relay = profile_for_bob.signed_relay_profile.as_ref().unwrap();
        verify_signed_relay_profile(&carol_id, relay).unwrap();
        assert_eq!(relay.payload.collections.len(), 1);
        assert_eq!(relay.payload.collections[0].id, "saved:pub1");
        assert!(!relay.payload.collections[0].specific_friends_only);
    }

    #[test]
    fn verify_signed_relay_profile_rejects_tampered_payload_or_wrong_key() {
        let carol_key = SecretKey::generate();
        let mallory_key = SecretKey::generate();
        let carol_id = carol_key.public().to_string();

        let payload = RelayProfilePayload {
            peer_id: carol_id.clone(),
            display_name: "Carol".into(),
            profile_picture: None,
            updated_at: "2026-11-21T01:00:00Z".into(),
            collections: vec![],
            friends: None,
            allow_mutual_connections: true,
            activity_events: None,
        };
        let signed = sign_relay_profile(&carol_key, payload.clone()).unwrap();
        verify_signed_relay_profile(&carol_id, &signed).unwrap();

        // Tampering with display_name invalidates signature.
        let mut tampered = signed.clone();
        tampered.payload.display_name = "Mallory".into();
        assert!(verify_signed_relay_profile(&carol_id, &tampered).is_err());

        // Signing with a different key is rejected.
        let forged = sign_relay_profile(&mallory_key, payload).unwrap();
        assert!(verify_signed_relay_profile(&carol_id, &forged).is_err());
    }

    #[test]
    fn apply_relayed_profile_updates_cache_without_marking_online() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let carol_key = SecretKey::generate();
        let carol_id = carol_key.public().to_string();
        let svc = P2pService::new(paths);

        let code_v1 = make_share_code_with_mods("Skyrim SE", vec![(10, 100, "SkyUI", "5.1")]);
        let hash_v1 = crate::share::share_hash(&code_v1);
        let code_spec = make_share_code_with_mods("Skyrim SE", vec![(20, 200, "SecretMod", "1.0")]);
        let hash_spec = crate::share::share_hash(&code_spec);

        {
            let mut store = svc.store.lock().unwrap();
            store.friends.push(FriendRecord {
                peer_id: carol_id.clone(),
                display_name: "Carol".into(),
                profile_picture: None,
                addr: EndpointAddr::new(carol_key.public()),
                status: FriendStatus::Connected,
                online: false,
                last_seen_at: Some("2026-11-21T00:00:00Z".into()),
                added_at: None,
                cached_profile: Some(PeerProfileSummary {
                    peer_id: carol_id.clone(),
                    display_name: "Carol".into(),
                    profile_picture: None,
                    updated_at: "2026-11-21T00:00:00Z".into(),
                    collections: vec![
                        FriendShareRef {
                            id: "saved:pub".into(),
                            kind: FriendCollectionKind::SavedCollection,
                            name: "Public Pack".into(),
                            share_code: code_v1,
                            share_hash: hash_v1,
                            updated_at: "2026-11-21T00:00:00Z".into(),
                            specific_friends_only: false,
                            synced_source_collection_id: None,
                            matched_local_collection: None,
                        },
                        FriendShareRef {
                            id: "saved:spec".into(),
                            kind: FriendCollectionKind::SavedCollection,
                            name: "Carol Secret".into(),
                            share_code: code_spec,
                            share_hash: hash_spec,
                            updated_at: "2026-11-21T00:00:00Z".into(),
                            specific_friends_only: true,
                            synced_source_collection_id: None,
                            matched_local_collection: None,
                        },
                    ],
                    friends: None,
                    allow_mutual_connections: true,
                    signed_relay_profile: None,
                    activity_events: None,
                    relayed_invites: vec![],
                }),
                syncing_my_collections: Vec::new(),
                cached_via_peer_name: None,
            });
        }

        // Stale relay is ignored.
        let stale = sign_relay_profile(
            &carol_key,
            RelayProfilePayload {
                peer_id: carol_id.clone(),
                display_name: "Carol Old".into(),
                profile_picture: None,
                updated_at: "2026-11-20T23:00:00Z".into(),
                collections: vec![],
                friends: None,
                allow_mutual_connections: true,
                activity_events: None,
            },
        )
        .unwrap();
        assert!(!svc.apply_relayed_profile(&carol_id, "Bob", stale).unwrap());

        // Newer relay updates cache, emits feed event, preserves SpecificFriends collection, and keeps online = false.
        let code_v2 = make_share_code_with_mods(
            "Skyrim SE",
            vec![(10, 101, "SkyUI", "5.2"), (30, 300, "USSEP", "4.3")],
        );
        let hash_v2 = crate::share::share_hash(&code_v2);
        let newer = sign_relay_profile(
            &carol_key,
            RelayProfilePayload {
                peer_id: carol_id.clone(),
                display_name: "Carol New".into(),
                profile_picture: None,
                updated_at: "2026-11-21T02:00:00Z".into(),
                collections: vec![FriendShareRef {
                    id: "saved:pub".into(),
                    kind: FriendCollectionKind::SavedCollection,
                    name: "Public Pack".into(),
                    share_code: code_v2,
                    share_hash: hash_v2,
                    updated_at: "2026-11-21T02:00:00Z".into(),
                    specific_friends_only: false,
                    synced_source_collection_id: None,
                    matched_local_collection: None,
                }],
                friends: None,
                allow_mutual_connections: true,
                activity_events: None,
            },
        )
        .unwrap();
        assert!(svc.apply_relayed_profile(&carol_id, "Bob", newer).unwrap());

        let state = svc.get_state().unwrap();
        let carol = &state.friends[0];
        assert!(!carol.online);
        assert_eq!(carol.display_name, "Carol New");
        assert_eq!(carol.cached_via_peer_name.as_deref(), Some("Bob"));
        assert_eq!(carol.last_seen_at.as_deref(), Some("2026-11-21T02:00:00Z"));
        let cols = &carol.cached_profile.as_ref().unwrap().collections;
        assert_eq!(cols.len(), 2);
        assert!(cols.iter().any(|c| c.id == "saved:spec" && c.specific_friends_only));
        assert_eq!(state.feed_events.len(), 1);
        assert_eq!(state.feed_events[0].kind, FriendFeedEventKind::CollectionUpdated);
    }

    #[test]
    fn get_cached_friend_profile_enforces_mutual_friendship_and_opt_out() {
        let alice_key = SecretKey::generate();
        let carol_key = SecretKey::generate();
        let stranger_key = SecretKey::generate();
        let alice_id = alice_key.public().to_string();
        let carol_id = carol_key.public().to_string();
        let stranger_id = stranger_key.public().to_string();

        let carol_relay = sign_relay_profile(
            &carol_key,
            RelayProfilePayload {
                peer_id: carol_id.clone(),
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: "2026-11-21T03:00:00Z".into(),
                collections: vec![],
                friends: Some(vec![SharedFriendRef {
                    peer_id: alice_id.clone(),
                    display_name: "Alice".into(),
                    addr: None,
                    profile_updated_at: None,
                    online: None,
                }]),
                allow_mutual_connections: true,
                activity_events: None,
            },
        )
        .unwrap();

        let mut store = FriendsStore::default();
        store.friends.push(FriendRecord {
            peer_id: alice_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            addr: EndpointAddr::new(alice_key.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: None,
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        store.friends.push(FriendRecord {
            peer_id: stranger_id.clone(),
            display_name: "Stranger".into(),
            profile_picture: None,
            addr: EndpointAddr::new(stranger_key.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: None,
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        store.friends.push(FriendRecord {
            peer_id: carol_id.clone(),
            display_name: "Carol".into(),
            profile_picture: None,
            addr: EndpointAddr::new(carol_key.public()),
            status: FriendStatus::Connected,
            online: false,
            last_seen_at: None,
            added_at: None,
            cached_profile: Some(PeerProfileSummary {
                peer_id: carol_id.clone(),
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: "2026-11-21T03:00:00Z".into(),
                collections: vec![],
                friends: Some(vec![SharedFriendRef {
                    peer_id: alice_id.clone(),
                    display_name: "Alice".into(),
                    addr: None,
                    profile_updated_at: None,
                    online: None,
                }]),
                allow_mutual_connections: true,
                signed_relay_profile: Some(carol_relay),
                activity_events: None,
                relayed_invites: vec![],
            }),
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });

        // Mutual friend Alice with older cache gets CachedFriendProfile.
        assert!(matches!(
            resolve_cached_friend_profile_for_peer(
                &store,
                &alice_id,
                &carol_id,
                Some("2026-11-21T01:00:00Z"),
            ),
            P2pResponse::CachedFriendProfile(_)
        ));

        // Alice with same/newer cache gets CachedFriendNotNewer.
        assert!(matches!(
            resolve_cached_friend_profile_for_peer(
                &store,
                &alice_id,
                &carol_id,
                Some("2026-11-21T03:00:00Z"),
            ),
            P2pResponse::CachedFriendNotNewer
        ));

        // Connected friend of Bob who is NOT in Carol's friends list is denied.
        assert!(matches!(
            resolve_cached_friend_profile_for_peer(&store, &stranger_id, &carol_id, None),
            P2pResponse::Denied(_)
        ));

        // Opt-out by Carol denies Alice.
        store.friends[2].cached_profile.as_mut().unwrap().allow_mutual_connections = false;
        assert!(matches!(
            resolve_cached_friend_profile_for_peer(&store, &alice_id, &carol_id, None),
            P2pResponse::Denied(_)
        ));
    }

    #[test]
    fn build_local_profile_sets_synced_source_collection_id_only_for_matching_viewer() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.config_dir).unwrap();
        fs::create_dir_all(&paths.data_dir).unwrap();

        let game = crate::config::ManagedGame {
            id: "skyrim".into(),
            title: "Skyrim".into(),
            nexus_domain: "skyrimspecialedition".into(),
            install_path: dir.path().join("game").to_string_lossy().into_owned(),
            launcher: "steam".into(),
            plugin_id: "skyrimse".into(),
            cover_path: None,
            cover_url: None,
            project_name: None,
            thunderstore_community: None,
            modio_game_id: None,
            tool_overrides: None,
            game_version: None,
            ignored_requirements: Vec::new(),
            launch_executable: None,
            launch_command_source: Default::default(),
            custom_env_vars: Vec::new(),
            custom_wrapper: None,
            custom_launch_args: None,
            wine_overrides_dismissed: false,
        };
        let mut cfg = crate::config::AppConfig::default();
        cfg.managed_games.push(game.clone());
        crate::config::save_config(&paths, &cfg).unwrap();

        let mod_a = crate::mods::StagedMod {
            id: "m1".into(),
            name: "SkyUI".into(),
            enabled: true,
            source: crate::mods::ModSource::Nexus,
            domain: "skyrimspecialedition".into(),
            nexus_mod_id: 12604,
            nexus_file_id: 35407,
            ..Default::default()
        };
        let order = crate::mods::LoadOrder {
            mods: vec![mod_a],
        };
        crate::mods::save_loadorder(&paths, "skyrim", &order).unwrap();

        let (synced_manifest, _, _) = crate::share::build_manifest_from_loadorder_with_selections(
            &order,
            &game,
            Some("Alice Pack".into()),
            true,
        );
        let synced_code = crate::share::encode_share_manifest(&synced_manifest).unwrap();
        let synced_hash = crate::share::share_hash(&synced_code);

        let alice_col = crate::mods::InstalledCollection {
            id: "friend-alice-saved-pack1".into(),
            source: crate::mods::CollectionSource::Emperor,
            kind: crate::mods::CollectionKind::Share,
            name: "Alice Pack".into(),
            profile_code: Some(synced_code),
            mod_ids: vec!["m1".into()],
            friend_sync: Some(crate::mods::FriendSyncMetadata {
                friend_peer_id: "alice".into(),
                friend_collection_id: "saved:pack1".into(),
                last_synced_hash: synced_hash,
                last_synced_at: "2026-11-21T00:00:00Z".into(),
                auto_sync: true,
            }),
            ..Default::default()
        };
        crate::mods::sync_installed_collection(&paths, "skyrim", alice_col, &[]).unwrap();

        let store = FriendsStore::default();
        let signing_key = SecretKey::generate();

        // Alice (the sync source) receives synced_source_collection_id = Some("saved:pack1"),
        // while the signed relay profile keeps it None.
        let for_alice = build_local_profile(
            &paths,
            "bob",
            &store,
            None,
            Some("alice"),
            Some(&signing_key),
        );
        assert_eq!(
            for_alice.collections[0].synced_source_collection_id.as_deref(),
            Some("saved:pack1")
        );
        assert_eq!(
            for_alice
                .signed_relay_profile
                .as_ref()
                .unwrap()
                .payload
                .collections[0]
                .synced_source_collection_id,
            None
        );

        // Carol (unrelated peer) does NOT receive Alice's synced_source_collection_id.
        let for_carol = build_local_profile(&paths, "bob", &store, None, Some("carol"), None);
        assert_eq!(for_carol.collections[0].synced_source_collection_id, None);
    }

    #[test]
    fn resolve_friend_active_loadouts_resolves_synced_saved_installed_and_active_matches() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.config_dir).unwrap();
        fs::create_dir_all(&paths.data_dir).unwrap();

        let game = crate::config::ManagedGame {
            id: "skyrim".into(),
            title: "Skyrim".into(),
            nexus_domain: "skyrimspecialedition".into(),
            install_path: dir.path().join("game").to_string_lossy().into_owned(),
            launcher: "steam".into(),
            plugin_id: "skyrimse".into(),
            cover_path: None,
            cover_url: None,
            project_name: None,
            thunderstore_community: None,
            modio_game_id: None,
            tool_overrides: None,
            game_version: None,
            ignored_requirements: Vec::new(),
            launch_executable: None,
            launch_command_source: Default::default(),
            custom_env_vars: Vec::new(),
            custom_wrapper: None,
            custom_launch_args: None,
            wine_overrides_dismissed: false,
        };
        let mut cfg = crate::config::AppConfig::default();
        cfg.managed_games.push(game.clone());
        crate::config::save_config(&paths, &cfg).unwrap();

        let make_mod = |id: &str, mod_id: u64| crate::mods::StagedMod {
            id: id.into(),
            name: id.into(),
            enabled: true,
            source: crate::mods::ModSource::Nexus,
            domain: "skyrimspecialedition".into(),
            nexus_mod_id: mod_id,
            nexus_file_id: mod_id * 10,
            ..Default::default()
        };

        let mod1 = make_mod("m1", 101);
        let mod2 = make_mod("m2", 202);
        let mod3 = make_mod("m3", 303);

        // Local saved collection "Co-op Pack" with mod1.
        let (saved_manifest, _, _) = crate::share::build_manifest_from_loadorder_with_selections(
            &crate::mods::LoadOrder {
                mods: vec![mod1.clone()],
            },
            &game,
            Some("Co-op Pack".into()),
            true,
        );
        let saved_code = crate::share::encode_share_manifest(&saved_manifest).unwrap();
        let saved_store = crate::share::SavedCollectionsStore {
            collections: vec![crate::share::SavedCollectionEntry {
                id: "coop".into(),
                name: "Co-op Pack".into(),
                code: saved_code,
                created_at: "2026-11-21T00:00:00Z".into(),
                updated_at: "2026-11-21T00:00:00Z".into(),
                source_game_id: Some("skyrim".into()),
                game: saved_manifest.game.clone(),
                game_versions: None,
                mod_count: 1,
                nexus_count: 1,
                thunderstore_count: 0,
                modio_count: 0,
                share_id: None,
            }],
        };
        fs::write(
            paths.data_dir.join("saved_collections.json"),
            serde_json::to_string(&saved_store).unwrap(),
        )
        .unwrap();

        // Local installed collection "Installed Pack" with mod2.
        let (inst_manifest, _, _) = crate::share::build_manifest_from_loadorder_with_selections(
            &crate::mods::LoadOrder {
                mods: vec![mod2.clone()],
            },
            &game,
            Some("Installed Pack".into()),
            true,
        );
        let inst_code = crate::share::encode_share_manifest(&inst_manifest).unwrap();
        crate::mods::sync_installed_collection(
            &paths,
            "skyrim",
            crate::mods::InstalledCollection {
                id: "inst1".into(),
                name: "Installed Pack".into(),
                profile_code: Some(inst_code),
                mod_ids: vec!["m2".into()],
                ..Default::default()
            },
            &[],
        )
        .unwrap();

        // Local active loadout with mod3.
        crate::mods::save_loadorder(
            &paths,
            "skyrim",
            &crate::mods::LoadOrder {
                mods: vec![mod3.clone()],
            },
        )
        .unwrap();

        // Friend active loadout codes (notice generic name "Skyrim Loadout" differs from "Co-op Pack" / "Installed Pack").
        let encode_active = |m: crate::mods::StagedMod| {
            let (manifest, _, _) = crate::share::build_manifest_from_loadorder_with_selections(
                &crate::mods::LoadOrder { mods: vec![m] },
                &game,
                Some("Skyrim Loadout".into()),
                true,
            );
            let code = crate::share::encode_share_manifest(&manifest).unwrap();
            let hash = crate::share::share_hash(&code);
            (code, hash)
        };
        let (f_mod1_code, f_mod1_hash) = encode_active(mod1);
        let (f_mod2_code, f_mod2_hash) = encode_active(mod2);
        let (f_mod3_code, f_mod3_hash) = encode_active(mod3);

        let make_friend = |peer_id: &str,
                           code: String,
                           hash: String,
                           syncing: Vec<String>,
                           synced_src: Option<String>| FriendRecord {
            peer_id: peer_id.into(),
            display_name: peer_id.into(),
            profile_picture: None,
            addr: EndpointAddr::from(SecretKey::generate().public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: Some(PeerProfileSummary {
                peer_id: peer_id.into(),
                display_name: peer_id.into(),
                profile_picture: None,
                updated_at: "2026-11-21T00:00:00Z".into(),
                collections: vec![FriendShareRef {
                    id: "active:skyrim".into(),
                    kind: FriendCollectionKind::ActiveLoadout,
                    name: "Skyrim Loadout".into(),
                    share_code: code,
                    share_hash: hash,
                    updated_at: "2026-11-21T00:00:00Z".into(),
                    specific_friends_only: false,
                    synced_source_collection_id: synced_src,
                    matched_local_collection: None,
                }],
                friends: None,
                allow_mutual_connections: true,
                signed_relay_profile: None,
                activity_events: None,
                relayed_invites: vec![],
            }),
            syncing_my_collections: syncing,
            cached_via_peer_name: None,
        };

        let mut friends = vec![
            // 1. SyncedToYours via synced_source_collection_id even with outdated mods (mod2 vs saved:coop's mod1)
            make_friend(
                "p_synced_stale",
                f_mod2_code.clone(),
                f_mod2_hash.clone(),
                vec!["saved:coop".into()],
                Some("saved:coop".into()),
            ),
            // 2. SyncedToYours via syncing_my_collections + manifest_mods_match (no synced_source_collection_id)
            make_friend(
                "p_synced_legacy",
                f_mod1_code.clone(),
                f_mod1_hash.clone(),
                vec!["saved:coop".into()],
                None,
            ),
            // 3. MatchesSaved (not syncing)
            make_friend("p_saved", f_mod1_code, f_mod1_hash, vec![], None),
            // 4. MatchesInstalled
            make_friend("p_installed", f_mod2_code, f_mod2_hash, vec![], None),
            // 5. MatchesActiveLoadout
            make_friend("p_active", f_mod3_code, f_mod3_hash, vec![], None),
        ];

        resolve_friend_active_loadouts(&paths, &mut friends);

        let m0 = friends[0].cached_profile.as_ref().unwrap().collections[0]
            .matched_local_collection
            .as_ref()
            .unwrap();
        assert_eq!(m0.kind, FriendCollectionMatchKind::SyncedToYours);
        assert_eq!(m0.id, "saved:coop");
        assert!(m0.has_local_changes);

        let m1 = friends[1].cached_profile.as_ref().unwrap().collections[0]
            .matched_local_collection
            .as_ref()
            .unwrap();
        assert_eq!(m1.kind, FriendCollectionMatchKind::SyncedToYours);
        assert_eq!(m1.id, "saved:coop");
        assert!(!m1.has_local_changes);

        let m2 = friends[2].cached_profile.as_ref().unwrap().collections[0]
            .matched_local_collection
            .as_ref()
            .unwrap();
        assert_eq!(m2.kind, FriendCollectionMatchKind::MatchesSaved);
        assert_eq!(m2.name, "Co-op Pack");

        let m3 = friends[3].cached_profile.as_ref().unwrap().collections[0]
            .matched_local_collection
            .as_ref()
            .unwrap();
        assert_eq!(m3.kind, FriendCollectionMatchKind::MatchesInstalled);
        assert_eq!(m3.name, "Installed Pack");

        let m4 = friends[4].cached_profile.as_ref().unwrap().collections[0]
            .matched_local_collection
            .as_ref()
            .unwrap();
        assert_eq!(m4.kind, FriendCollectionMatchKind::MatchesActiveLoadout);
        assert_eq!(m4.id, "active:skyrim");
    }

    #[test]
    fn allow_cache_relay_opt_out_persists_and_blocks_relay() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let svc = P2pService::new(paths.clone());
        assert!(svc.get_state().unwrap().allow_cache_relay);

        let disabled = svc.set_allow_cache_relay(false).unwrap();
        assert!(!disabled.allow_cache_relay);
        let reloaded = P2pService::new(paths.clone());
        assert!(!reloaded.get_state().unwrap().allow_cache_relay);

        let self_key = SecretKey::generate();
        let alice_key = SecretKey::generate();
        let carol_key = SecretKey::generate();
        let alice_id = alice_key.public().to_string();
        let carol_id = carol_key.public().to_string();

        let carol_relay = sign_relay_profile(
            &carol_key,
            RelayProfilePayload {
                peer_id: carol_id.clone(),
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: "2026-11-21T03:00:00Z".into(),
                collections: vec![],
                friends: Some(vec![SharedFriendRef {
                    peer_id: alice_id.clone(),
                    display_name: "Alice".into(),
                    addr: None,
                    profile_updated_at: None,
                    online: None,
                }]),
                allow_mutual_connections: true,
                activity_events: None,
            },
        )
        .unwrap();

        let mut store = FriendsStore::default();
        store.allow_cache_relay = false;
        store.friends.push(FriendRecord {
            peer_id: alice_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            addr: EndpointAddr::new(alice_key.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: None,
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        store.friends.push(FriendRecord {
            peer_id: carol_id.clone(),
            display_name: "Carol".into(),
            profile_picture: None,
            addr: EndpointAddr::new(carol_key.public()),
            status: FriendStatus::Connected,
            online: false,
            last_seen_at: None,
            added_at: None,
            cached_profile: Some(PeerProfileSummary {
                peer_id: carol_id.clone(),
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: "2026-11-21T03:00:00Z".into(),
                collections: vec![],
                friends: Some(vec![SharedFriendRef {
                    peer_id: alice_id.clone(),
                    display_name: "Alice".into(),
                    addr: None,
                    profile_updated_at: None,
                    online: None,
                }]),
                allow_mutual_connections: true,
                signed_relay_profile: Some(carol_relay),
                activity_events: None,
                relayed_invites: vec![],
            }),
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });

        let profile_for_alice = build_local_profile(
            &paths,
            &self_key.public().to_string(),
            &store,
            None,
            Some(&alice_id),
            None,
        );
        let shared = profile_for_alice.friends.unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].profile_updated_at, None);

        assert!(matches!(
            resolve_cached_friend_profile_for_peer(&store, &alice_id, &carol_id, None),
            P2pResponse::Denied(_)
        ));
    }

    #[test]
    fn build_local_profile_excludes_friends_without_cached_profile() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let self_key = SecretKey::generate();
        let bob_key = SecretKey::generate();
        let carol_key = SecretKey::generate();
        let bob_id = bob_key.public().to_string();

        let mut store = FriendsStore::default();
        store.friends.push(FriendRecord {
            peer_id: bob_id.clone(),
            display_name: "Bob".into(),
            profile_picture: None,
            addr: EndpointAddr::new(bob_key.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: None,
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        store.friends.push(FriendRecord {
            peer_id: carol_key.public().to_string(),
            display_name: "Carol".into(),
            profile_picture: None,
            addr: EndpointAddr::new(carol_key.public()),
            status: FriendStatus::Connected,
            online: false,
            last_seen_at: None,
            added_at: None,
            cached_profile: None,
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });

        let profile = build_local_profile(
            &paths,
            &self_key.public().to_string(),
            &store,
            None,
            Some(&bob_id),
            None,
        );
        assert!(profile.friends.unwrap().is_empty());
    }

    #[test]
    fn validate_profile_picture_checks_magic_bytes() {
        // Fake PNG (plain text bytes masquerading as PNG) is rejected.
        assert!(validate_profile_picture("data:image/png;base64,SGVsbG8=").is_err());
        // Valid JPEG magic bytes accepted.
        assert!(validate_profile_picture("data:image/jpeg;base64,/9j/4AAQ").is_ok());
        // Valid GIF89a magic bytes accepted.
        assert!(validate_profile_picture("data:image/gif;base64,R0lGODlh").is_ok());
    }

    #[test]
    fn verify_signed_relay_profile_validates_bounds_and_legacy_dual_verify() {
        let key = SecretKey::generate();
        let peer_id = key.public().to_string();
        let payload = RelayProfilePayload {
            peer_id: peer_id.clone(),
            display_name: "Carol".into(),
            profile_picture: None,
            updated_at: "2026-11-21T03:00:00Z".into(),
            collections: vec![],
            friends: None,
            allow_mutual_connections: true,
            activity_events: None,
        };

        // Domain-separated signature verifies.
        let signed = sign_relay_profile(&key, payload.clone()).unwrap();
        assert!(verify_signed_relay_profile(&peer_id, &signed).is_ok());

        // Legacy unprefixed signature also verifies (dual-verify).
        let legacy_bytes = serde_json::to_vec(&payload).unwrap();
        let legacy_sig = key.sign(&legacy_bytes);
        let legacy_signed = SignedRelayProfile {
            payload: payload.clone(),
            signature: hex::encode(legacy_sig.to_bytes()),
        };
        assert!(verify_signed_relay_profile(&peer_id, &legacy_signed).is_ok());

        // Oversized/invalid profile picture in relay payload is rejected even if signed.
        let mut bad_payload = payload;
        bad_payload.profile_picture = Some("data:image/png;base64,SGVsbG8=".into());
        let bad_signed = sign_relay_profile(&key, bad_payload).unwrap();
        assert!(verify_signed_relay_profile(&peer_id, &bad_signed).is_err());
    }

    #[test]
    fn resolve_cached_friend_profile_requires_relay_friends_list() {
        let alice_key = SecretKey::generate();
        let carol_key = SecretKey::generate();
        let alice_id = alice_key.public().to_string();
        let carol_id = carol_key.public().to_string();

        // Carol's relay payload has friends: None (e.g. SpecificFriends only),
        // even though Carol shared her friends list directly with us.
        let carol_relay = sign_relay_profile(
            &carol_key,
            RelayProfilePayload {
                peer_id: carol_id.clone(),
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: "2026-11-21T03:00:00Z".into(),
                collections: vec![],
                friends: None,
                allow_mutual_connections: true,
                activity_events: None,
            },
        )
        .unwrap();

        let mut store = FriendsStore::default();
        store.friends.push(FriendRecord {
            peer_id: alice_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            addr: EndpointAddr::new(alice_key.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: None,
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        store.friends.push(FriendRecord {
            peer_id: carol_id.clone(),
            display_name: "Carol".into(),
            profile_picture: None,
            addr: EndpointAddr::new(carol_key.public()),
            status: FriendStatus::Connected,
            online: false,
            last_seen_at: None,
            added_at: None,
            cached_profile: Some(PeerProfileSummary {
                peer_id: carol_id.clone(),
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: "2026-11-21T03:00:00Z".into(),
                collections: vec![],
                friends: Some(vec![SharedFriendRef {
                    peer_id: alice_id.clone(),
                    display_name: "Alice".into(),
                    addr: None,
                    profile_updated_at: None,
                    online: None,
                }]),
                allow_mutual_connections: true,
                signed_relay_profile: Some(carol_relay),
                activity_events: None,
                relayed_invites: vec![],
            }),
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });

        assert!(matches!(
            resolve_cached_friend_profile_for_peer(&store, &alice_id, &carol_id, None),
            P2pResponse::Denied(_)
        ));
    }

    #[test]
    fn sanitize_profile_fields_enforces_share_code_budget() {
        let peer_id = SecretKey::generate().public().to_string();
        let mut name = "Peer".to_string();
        let mut pic = None;
        let mut friends = None;
        let big_code = "A".repeat(1_500_000);
        let mut collections = (0..4)
            .map(|i| FriendShareRef {
                id: format!("saved:{i}"),
                kind: FriendCollectionKind::SavedCollection,
                name: format!("Col {i}"),
                share_code: big_code.clone(),
                share_hash: format!("h{i}"),
                updated_at: "2026-11-21T00:00:00Z".into(),
                ..Default::default()
            })
            .collect();

        P2pService::sanitize_profile_fields(
            &peer_id,
            None,
            &mut name,
            &mut pic,
            &mut collections,
            &mut friends,
        );

        let total: usize = collections.iter().map(|c| c.share_code.len()).sum();
        assert!(total <= MAX_FRIEND_CACHED_SHARE_BYTES);
        assert!(!collections[0].share_code.is_empty());
        assert!(!collections[1].share_code.is_empty());
        // 3rd and 4th exceed 4 MB budget so their share_code bodies are cleared.
        assert!(collections[2].share_code.is_empty());
        assert!(collections[3].share_code.is_empty());
    }

    #[test]
    fn profile_rate_limiter_throttles_bursts() {
        let mut limiter = PeerProfileRateLimiter::default();
        for _ in 0..(PROFILE_RATE_BURST as usize) {
            assert!(limiter.allow("peer-a"));
        }
        assert!(!limiter.allow("peer-a"));
        assert!(limiter.allow("peer-b"));
    }

    #[test]
    fn local_activity_records_actions_coalesces_rapid_edits_and_skips_private() {
        let mut store = FriendsStore::default();
        let code_v1 = make_share_code_with_mods(
            "Skyrim SE",
            vec![(1, 10, "SkyUI", "5.1")],
        );
        let code_v2 = make_share_code_with_mods(
            "Skyrim SE",
            vec![(1, 10, "SkyUI", "5.1"), (2, 20, "USSEP", "4.3")],
        );
        let code_v3 = make_share_code_with_mods(
            "Skyrim SE",
            vec![
                (1, 10, "SkyUI", "5.1"),
                (2, 20, "USSEP", "4.3"),
                (3, 30, "SMIM", "2.0"),
            ],
        );
        let saved_v1 = make_share_code_with_mods(
            "Skyrim SE",
            vec![(10, 100, "Campfire", "1.0")],
        );
        let saved_v2 = make_share_code_with_mods(
            "Skyrim SE",
            vec![(10, 100, "Campfire", "1.0"), (11, 110, "Frostfall", "3.4")],
        );
        let mut renamed_manifest = share::decode_share_code(&saved_v2).unwrap();
        renamed_manifest.name = Some("Ultra Cozy Pack".into());
        let saved_renamed = share::encode_share_manifest(&renamed_manifest).unwrap();
        assert_ne!(share::share_hash(&saved_v2), share::share_hash(&saved_renamed));

        // 1. Baseline initialization records zero events.
        let seeded = record_local_activity_in_store(
            &mut store,
            vec![
                (
                    "active:skyrim".into(),
                    FriendCollectionKind::ActiveLoadout,
                    "Skyrim Loadout".into(),
                    share::share_hash(&code_v1),
                    code_v1.clone(),
                ),
                (
                    "saved:cozy".into(),
                    FriendCollectionKind::SavedCollection,
                    "Cozy Pack".into(),
                    share::share_hash(&saved_v1),
                    saved_v1.clone(),
                ),
            ],
            "2026-11-22T10:00:00Z",
        );
        assert!(seeded);
        assert!(store.local_activity_initialized);
        assert!(store.local_activity.is_empty());

        // 2. Edit active loadout (AllFriends by default) -> records CollectionUpdated.
        record_local_activity_in_store(
            &mut store,
            vec![
                (
                    "active:skyrim".into(),
                    FriendCollectionKind::ActiveLoadout,
                    "Skyrim Loadout".into(),
                    share::share_hash(&code_v2),
                    code_v2.clone(),
                ),
                // Private saved collection edit should NOT record any event.
                (
                    "saved:cozy".into(),
                    FriendCollectionKind::SavedCollection,
                    "Cozy Pack".into(),
                    share::share_hash(&saved_v2),
                    saved_v2.clone(),
                ),
            ],
            "2026-11-22T10:05:00Z",
        );
        assert_eq!(store.local_activity.len(), 1);
        assert_eq!(store.local_activity[0].kind, FriendFeedEventKind::CollectionUpdated);
        assert_eq!(store.local_activity[0].collection_id, "active:skyrim");
        assert_eq!(
            store.local_activity[0].diff.as_ref().unwrap().mods_added_count,
            1
        );

        // 3. Rapid second edit within 120s coalesces into the same event with net diff (+2).
        record_local_activity_in_store(
            &mut store,
            vec![
                (
                    "active:skyrim".into(),
                    FriendCollectionKind::ActiveLoadout,
                    "Skyrim Loadout".into(),
                    share::share_hash(&code_v3),
                    code_v3.clone(),
                ),
                (
                    "saved:cozy".into(),
                    FriendCollectionKind::SavedCollection,
                    "Cozy Pack".into(),
                    share::share_hash(&saved_v2),
                    saved_v2.clone(),
                ),
            ],
            "2026-11-22T10:05:45Z",
        );
        assert_eq!(store.local_activity.len(), 1);
        assert_eq!(store.local_activity[0].occurred_at, "2026-11-22T10:05:45Z");
        assert_eq!(
            store.local_activity[0].diff.as_ref().unwrap().mods_added_count,
            2
        );

        // 4. Edit after >120s creates a separate event.
        record_local_activity_in_store(
            &mut store,
            vec![
                (
                    "active:skyrim".into(),
                    FriendCollectionKind::ActiveLoadout,
                    "Skyrim Loadout".into(),
                    share::share_hash(&code_v2),
                    code_v2.clone(),
                ),
                (
                    "saved:cozy".into(),
                    FriendCollectionKind::SavedCollection,
                    "Cozy Pack".into(),
                    share::share_hash(&saved_v2),
                    saved_v2.clone(),
                ),
            ],
            "2026-11-22T10:15:00Z",
        );
        assert_eq!(store.local_activity.len(), 2);
        assert_eq!(store.local_activity[0].occurred_at, "2026-11-22T10:15:00Z");
        assert_eq!(
            store.local_activity[0].diff.as_ref().unwrap().mods_removed_count,
            1
        );

        // 5. Transition saved:cozy from Private -> AllFriends emits a single CollectionAdded.
        store
            .collection_visibility
            .insert("saved:cozy".into(), CollectionVisibility::AllFriends);
        record_local_activity_in_store(
            &mut store,
            vec![
                (
                    "active:skyrim".into(),
                    FriendCollectionKind::ActiveLoadout,
                    "Skyrim Loadout".into(),
                    share::share_hash(&code_v2),
                    code_v2.clone(),
                ),
                (
                    "saved:cozy".into(),
                    FriendCollectionKind::SavedCollection,
                    "Cozy Pack".into(),
                    share::share_hash(&saved_v2),
                    saved_v2.clone(),
                ),
            ],
            "2026-11-22T10:20:00Z",
        );
        assert_eq!(store.local_activity.len(), 3);
        assert_eq!(store.local_activity[0].kind, FriendFeedEventKind::CollectionAdded);
        assert_eq!(store.local_activity[0].collection_id, "saved:cozy");

        // 6. Renaming saved:cozy (even though manifest.name changes share_hash) emits CollectionRenamed.
        record_local_activity_in_store(
            &mut store,
            vec![
                (
                    "active:skyrim".into(),
                    FriendCollectionKind::ActiveLoadout,
                    "Skyrim Loadout".into(),
                    share::share_hash(&code_v2),
                    code_v2,
                ),
                (
                    "saved:cozy".into(),
                    FriendCollectionKind::SavedCollection,
                    "Ultra Cozy Pack".into(),
                    share::share_hash(&saved_renamed),
                    saved_renamed,
                ),
            ],
            "2026-11-22T10:25:00Z",
        );
        assert_eq!(store.local_activity.len(), 4);
        assert_eq!(store.local_activity[0].kind, FriendFeedEventKind::CollectionRenamed);
        assert_eq!(store.local_activity[0].previous_name.as_deref(), Some("Cozy Pack"));
        assert_eq!(store.local_activity[0].collection_name, "Ultra Cozy Pack");
    }

    #[test]
    fn friend_sync_respects_added_at_cutoff_clear_feed_and_deduplicates() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.data_dir).unwrap();

        let alice_key = SecretKey::generate();
        let alice_id = alice_key.public().to_string();
        let svc = P2pService::new(paths);
        {
            let mut store = svc.store.lock().unwrap();
            store.friends.push(FriendRecord {
                peer_id: alice_id.clone(),
                display_name: "Alice".into(),
                profile_picture: None,
                addr: EndpointAddr::new(alice_key.public()),
                status: FriendStatus::Connected,
                online: false,
                last_seen_at: None,
                added_at: Some("2026-11-22T12:00:00Z".into()),
                cached_profile: None,
                syncing_my_collections: Vec::new(),
                cached_via_peer_name: None,
            });
        }

        let code = make_share_code_with_mods(
            "Skyrim SE",
            vec![(1, 10, "SkyUI", "5.1")],
        );
        let hash = share::share_hash(&code);

        let ev_before_added = LocalActivityEvent {
            id: "ev-before".into(),
            kind: FriendFeedEventKind::CollectionUpdated,
            collection_id: "active:skyrim".into(),
            collection_kind: FriendCollectionKind::ActiveLoadout,
            collection_name: "Skyrim Loadout".into(),
            previous_name: None,
            game_title: Some("Skyrim SE".into()),
            mod_count: Some(1),
            share_hash: hash.clone(),
            diff: None,
            occurred_at: "2026-11-22T11:30:00Z".into(),
        };
        let ev_after_added = LocalActivityEvent {
            id: "ev-after".into(),
            kind: FriendFeedEventKind::CollectionUpdated,
            collection_id: "active:skyrim".into(),
            collection_kind: FriendCollectionKind::ActiveLoadout,
            collection_name: "Skyrim Loadout".into(),
            previous_name: None,
            game_title: Some("Skyrim SE".into()),
            mod_count: Some(1),
            share_hash: hash.clone(),
            diff: Some(FriendFeedDiff {
                mods_added: vec!["USSEP".into()],
                mods_added_count: 1,
                ..Default::default()
            }),
            occurred_at: "2026-11-22T12:10:00Z".into(),
        };

        // Even if the remote peer sends both events (ignoring activity_since),
        // receiver enforces added_at = 12:00:00Z and drops ev_before_added.
        let profile = PeerProfileSummary {
            peer_id: alice_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            updated_at: "2026-11-22T12:15:00Z".into(),
            collections: vec![FriendShareRef {
                id: "active:skyrim".into(),
                kind: FriendCollectionKind::ActiveLoadout,
                name: "Skyrim Loadout".into(),
                share_code: code.clone(),
                share_hash: hash.clone(),
                updated_at: "2026-11-22T12:10:00Z".into(),
                ..Default::default()
            }],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: Some(vec![ev_after_added.clone(), ev_before_added.clone()]),
            relayed_invites: vec![],
        };
        svc.apply_fetched_profile(&alice_id, profile.clone()).unwrap();
        let state = svc.get_state().unwrap();
        assert_eq!(state.feed_events.len(), 1);
        assert_eq!(state.feed_events[0].id, format!("{alice_id}:ev-after"));
        assert_eq!(state.feed_events[0].occurred_at, "2026-11-22T12:10:00Z");

        // Re-syncing does not duplicate ev-after.
        svc.apply_fetched_profile(&alice_id, profile).unwrap();
        assert_eq!(svc.get_state().unwrap().feed_events.len(), 1);

        // Coalesced update to ev-after updates it in place rather than duplicating.
        let mut ev_after_coalesced = ev_after_added.clone();
        ev_after_coalesced.occurred_at = "2026-11-22T12:11:00Z".into();
        ev_after_coalesced.diff = Some(FriendFeedDiff {
            mods_added: vec!["USSEP".into(), "SMIM".into()],
            mods_added_count: 2,
            ..Default::default()
        });
        let profile_coalesced = PeerProfileSummary {
            peer_id: alice_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            updated_at: "2026-11-22T12:16:00Z".into(),
            collections: vec![FriendShareRef {
                id: "active:skyrim".into(),
                kind: FriendCollectionKind::ActiveLoadout,
                name: "Skyrim Loadout".into(),
                share_code: code.clone(),
                share_hash: hash.clone(),
                updated_at: "2026-11-22T12:11:00Z".into(),
                ..Default::default()
            }],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: Some(vec![ev_after_coalesced.clone()]),
            relayed_invites: vec![],
        };
        svc.apply_fetched_profile(&alice_id, profile_coalesced).unwrap();
        let state_coalesced = svc.get_state().unwrap();
        assert_eq!(state_coalesced.feed_events.len(), 1);
        assert_eq!(
            state_coalesced.feed_events[0].diff.as_ref().unwrap().mods_added_count,
            2
        );

        // Clearing feed prevents ev_after_coalesced from reappearing on next sync.
        svc.clear_feed().unwrap();
        assert!(svc.get_state().unwrap().feed_events.is_empty());
        let profile_resync = PeerProfileSummary {
            peer_id: alice_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            updated_at: now_rfc3339(),
            collections: vec![FriendShareRef {
                id: "active:skyrim".into(),
                kind: FriendCollectionKind::ActiveLoadout,
                name: "Skyrim Loadout".into(),
                share_code: code,
                share_hash: hash,
                updated_at: now_rfc3339(),
                ..Default::default()
            }],
            friends: None,
            allow_mutual_connections: true,
            signed_relay_profile: None,
            activity_events: Some(vec![ev_after_coalesced]),
            relayed_invites: vec![],
        };
        svc.apply_fetched_profile(&alice_id, profile_resync).unwrap();
        assert!(svc.get_state().unwrap().feed_events.is_empty());
    }

    #[test]
    fn multiple_offline_actions_preserved_via_relay_and_exclude_specific_friends() {
        let dir = tempdir().unwrap();
        let alice_paths = Paths {
            config_dir: dir.path().join("alice_cfg"),
            data_dir: dir.path().join("alice_data"),
            cache_dir: dir.path().join("alice_cache"),
        };
        let bob_paths = Paths {
            config_dir: dir.path().join("bob_cfg"),
            data_dir: dir.path().join("bob_data"),
            cache_dir: dir.path().join("bob_cache"),
        };
        fs::create_dir_all(&alice_paths.data_dir).unwrap();
        fs::create_dir_all(&bob_paths.data_dir).unwrap();

        let alice_key = SecretKey::generate();
        let alice_id = alice_key.public().to_string();
        let bob_id = SecretKey::generate().public().to_string();
        let carol_id = SecretKey::generate().public().to_string();

        let public_code = make_share_code_with_mods(
            "Skyrim SE",
            vec![(1, 10, "SkyUI", "5.1")],
        );
        let secret_code = make_share_code_with_mods(
            "Skyrim SE",
            vec![(2, 20, "SecretMod", "1.0")],
        );
        crate::share::save_saved_collections(
            &alice_paths,
            &crate::share::SavedCollectionsStore {
                collections: vec![
                    crate::share::SavedCollectionEntry {
                        id: "pub".into(),
                        name: "Public Pack".into(),
                        code: public_code.clone(),
                        game: crate::share::ShareGameHint::default(),
                        game_versions: None,
                        mod_count: 1,
                        nexus_count: 1,
                        thunderstore_count: 0,
                        modio_count: 0,
                        source_game_id: None,
                        share_id: None,
                        created_at: "2026-11-22T10:00:00Z".into(),
                        updated_at: "2026-11-22T10:00:00Z".into(),
                    },
                    crate::share::SavedCollectionEntry {
                        id: "sec".into(),
                        name: "Carol Only".into(),
                        code: secret_code.clone(),
                        game: crate::share::ShareGameHint::default(),
                        game_versions: None,
                        mod_count: 1,
                        nexus_count: 1,
                        thunderstore_count: 0,
                        modio_count: 0,
                        source_game_id: None,
                        share_id: None,
                        created_at: "2026-11-22T10:00:00Z".into(),
                        updated_at: "2026-11-22T10:00:00Z".into(),
                    },
                ],
            },
        )
        .unwrap();

        let mut alice_store = FriendsStore::default();
        alice_store.display_name = "Alice".into();
        alice_store
            .collection_visibility
            .insert("saved:pub".into(), CollectionVisibility::AllFriends);
        alice_store.collection_visibility.insert(
            "saved:sec".into(),
            CollectionVisibility::SpecificFriends {
                peer_ids: vec![carol_id.clone()],
            },
        );
        alice_store.local_activity = vec![
            LocalActivityEvent {
                id: "ev-2".into(),
                kind: FriendFeedEventKind::CollectionUpdated,
                collection_id: "saved:pub".into(),
                collection_kind: FriendCollectionKind::SavedCollection,
                collection_name: "Public Pack".into(),
                previous_name: None,
                game_title: Some("Skyrim SE".into()),
                mod_count: Some(1),
                share_hash: share::share_hash(&public_code),
                diff: None,
                occurred_at: "2026-11-22T14:00:00Z".into(),
            },
            LocalActivityEvent {
                id: "ev-secret".into(),
                kind: FriendFeedEventKind::CollectionAdded,
                collection_id: "saved:sec".into(),
                collection_kind: FriendCollectionKind::SavedCollection,
                collection_name: "Carol Only".into(),
                previous_name: None,
                game_title: Some("Skyrim SE".into()),
                mod_count: Some(1),
                share_hash: share::share_hash(&secret_code),
                diff: None,
                occurred_at: "2026-11-22T13:30:00Z".into(),
            },
            LocalActivityEvent {
                id: "ev-1".into(),
                kind: FriendFeedEventKind::CollectionAdded,
                collection_id: "saved:pub".into(),
                collection_kind: FriendCollectionKind::SavedCollection,
                collection_name: "Public Pack".into(),
                previous_name: None,
                game_title: Some("Skyrim SE".into()),
                mod_count: Some(1),
                share_hash: share::share_hash(&public_code),
                diff: None,
                occurred_at: "2026-11-22T13:00:00Z".into(),
            },
        ];

        // Sender-side `activity_since` filters out ev-1 (13:00) for a viewer who added Alice at 13:45.
        let direct_for_bob = build_local_profile_with_since(
            &alice_paths,
            &alice_id,
            &alice_store,
            None,
            Some(&bob_id),
            Some(&alice_key),
            Some("2026-11-22T13:45:00Z"),
        );
        let direct_events = direct_for_bob.activity_events.as_ref().unwrap();
        assert_eq!(direct_events.len(), 1);
        assert_eq!(direct_events[0].id, "ev-2");

        // Signed relay profile includes both AllFriends events (ev-2 and ev-1), never ev-secret.
        let signed_relay = direct_for_bob.signed_relay_profile.clone().unwrap();
        let relay_events = signed_relay.payload.activity_events.as_ref().unwrap();
        assert_eq!(relay_events.len(), 2);
        assert!(relay_events.iter().all(|e| e.collection_id == "saved:pub"));

        // Bob receives the signed relay while Alice is offline and added Alice at 12:00:00Z ->
        // both distinct offline actions arrive with their original timestamps!
        let bob_svc = P2pService::new(bob_paths);
        {
            let mut store = bob_svc.store.lock().unwrap();
            store.friends.push(FriendRecord {
                peer_id: alice_id.clone(),
                display_name: "Alice".into(),
                profile_picture: None,
                addr: EndpointAddr::new(alice_key.public()),
                status: FriendStatus::Connected,
                online: false,
                last_seen_at: None,
                added_at: Some("2026-11-22T12:00:00Z".into()),
                cached_profile: None,
                syncing_my_collections: Vec::new(),
                cached_via_peer_name: None,
            });
        }
        bob_svc
            .apply_relayed_profile(&alice_id, "Carol", signed_relay)
            .unwrap();
        let bob_state = bob_svc.get_state().unwrap();
        assert_eq!(bob_state.feed_events.len(), 2);
        assert_eq!(bob_state.feed_events[0].id, format!("{alice_id}:ev-2"));
        assert_eq!(bob_state.feed_events[0].occurred_at, "2026-11-22T14:00:00Z");
        assert_eq!(bob_state.feed_events[1].id, format!("{alice_id}:ev-1"));
        assert_eq!(bob_state.feed_events[1].occurred_at, "2026-11-22T13:00:00Z");
    }

    #[tokio::test]
    async fn queued_invite_waits_while_target_offline_and_supports_cancel() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let dir_c = tempdir().unwrap();
        let mk_paths = |d: &tempfile::TempDir| {
            let p = Paths {
                config_dir: d.path().join("cfg"),
                data_dir: d.path().join("data"),
                cache_dir: d.path().join("cache"),
            };
            fs::create_dir_all(&p.data_dir).unwrap();
            p
        };

        let key_a = SecretKey::generate();
        let key_b = SecretKey::generate();
        let key_c = SecretKey::generate();
        let svc_a = P2pService::new(mk_paths(&dir_a));
        let svc_b = P2pService::new(mk_paths(&dir_b));
        let svc_c = P2pService::new(mk_paths(&dir_c));
        svc_a.set_display_name("Alice".into()).unwrap();
        svc_b.set_display_name("Bob".into()).unwrap();
        svc_c.set_display_name("Carol".into()).unwrap();
        let _ = svc_a.secret_key.set(key_a.clone());
        let _ = svc_b.secret_key.set(key_b.clone());
        let _ = svc_c.secret_key.set(key_c.clone());

        let ep_a = Endpoint::builder(presets::N0DisableRelay)
            .secret_key(key_a)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let ep_b = Endpoint::builder(presets::N0DisableRelay)
            .secret_key(key_b)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let ep_c = Endpoint::builder(presets::N0DisableRelay)
            .secret_key(key_c)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let _ = svc_a.endpoint.set(ep_a.clone());
        let _ = svc_b.endpoint.set(ep_b.clone());
        let _ = svc_c.endpoint.set(ep_c.clone());

        let spawn_accept = |svc: Arc<P2pService>, ep: Endpoint| {
            tokio::spawn(async move {
                while let Some(inc) = ep.accept().await {
                    let s = Arc::clone(&svc);
                    tokio::spawn(async move {
                        let _ = s.handle_incoming(inc).await;
                    });
                }
            })
        };
        let accept_a = spawn_accept(Arc::clone(&svc_a), ep_a.clone());
        let accept_b = spawn_accept(Arc::clone(&svc_b), ep_b.clone());
        let accept_c = spawn_accept(Arc::clone(&svc_c), ep_c.clone());

        let alice_id = ep_a.id().to_string();
        let bob_id = ep_b.id().to_string();
        let carol_id = ep_c.id().to_string();

        // Connect Bob <-> Carol and Alice <-> Bob.
        let join_c = svc_c.get_state().unwrap().join_code.unwrap();
        svc_b.add_friend_by_code(join_c).await.unwrap();
        svc_c.respond_friend_request(bob_id.clone(), true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let join_b = svc_b.get_state().unwrap().join_code.unwrap();
        svc_a.add_friend_by_code(join_b).await.unwrap();
        svc_b.respond_friend_request(alice_id.clone(), true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;

        // Carol goes offline; Bob marks Carol offline; Alice syncs Bob's updated friends list.
        accept_c.abort();
        ep_c.close().await;
        {
            let mut sb = svc_b.store.lock().unwrap();
            if let Some(c) = sb.friends.iter_mut().find(|f| f.peer_id == carol_id) {
                c.online = false;
            }
        }
        svc_a.refresh_all_friends().await.unwrap();

        let state_a_before = svc_a.get_state().unwrap();
        let shared_carol = state_a_before.friends[0]
            .cached_profile
            .as_ref()
            .unwrap()
            .friends
            .as_ref()
            .unwrap()
            .iter()
            .find(|sf| sf.peer_id == carol_id)
            .unwrap();
        assert_eq!(shared_carol.online, Some(false));

        // Alice adds Carol while Carol is offline -> queued (not in friends), relayed via Bob.
        let state_a_queued = svc_a
            .add_friend_by_peer(carol_id.clone(), Some("Carol".into()))
            .await
            .unwrap();
        assert_eq!(state_a_queued.friends.len(), 1);
        assert_eq!(state_a_queued.queued_invites.len(), 1);
        assert_eq!(state_a_queued.queued_invites[0].peer_id, carol_id);
        assert_eq!(
            state_a_queued.queued_invites[0].relayed_via_peer_ids,
            vec![bob_id.clone()]
        );
        {
            let sb = svc_b.store.lock().unwrap();
            assert_eq!(sb.relayed_invites.len(), 1);
            assert_eq!(sb.relayed_invites[0].payload.from_peer_id, alice_id);
            assert_eq!(sb.relayed_invites[0].payload.to_peer_id, carol_id);
        }

        // Cancelling removes from Alice's queue AND purges Bob's relayed copy.
        let state_a_cancelled = svc_a
            .cancel_queued_friend_invite(carol_id.clone())
            .await
            .unwrap();
        assert!(state_a_cancelled.queued_invites.is_empty());
        {
            let sb = svc_b.store.lock().unwrap();
            assert!(sb.relayed_invites.is_empty());
        }

        accept_a.abort();
        accept_b.abort();
    }

    #[tokio::test]
    async fn signed_relayed_invite_delivers_while_sender_offline_and_converges() {
        let dir_a = tempdir().unwrap();
        let dir_b = tempdir().unwrap();
        let dir_c = tempdir().unwrap();
        let mk_paths = |d: &tempfile::TempDir| {
            let p = Paths {
                config_dir: d.path().join("cfg"),
                data_dir: d.path().join("data"),
                cache_dir: d.path().join("cache"),
            };
            fs::create_dir_all(&p.data_dir).unwrap();
            p
        };
        let paths_a = mk_paths(&dir_a);
        let paths_b = mk_paths(&dir_b);
        let paths_c = mk_paths(&dir_c);

        let key_a = SecretKey::generate();
        let key_b = SecretKey::generate();
        let key_c = SecretKey::generate();

        let svc_a = P2pService::new(paths_a.clone());
        let svc_b = P2pService::new(paths_b.clone());
        let svc_c = P2pService::new(paths_c.clone());
        svc_a.set_display_name("Alice".into()).unwrap();
        svc_b.set_display_name("Bob".into()).unwrap();
        svc_c.set_display_name("Carol".into()).unwrap();
        let _ = svc_a.secret_key.set(key_a.clone());
        let _ = svc_b.secret_key.set(key_b.clone());
        let _ = svc_c.secret_key.set(key_c.clone());

        let ep_a = Endpoint::builder(presets::N0DisableRelay)
            .secret_key(key_a.clone())
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let ep_b = Endpoint::builder(presets::N0DisableRelay)
            .secret_key(key_b.clone())
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let ep_c = Endpoint::builder(presets::N0DisableRelay)
            .secret_key(key_c.clone())
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let _ = svc_a.endpoint.set(ep_a.clone());
        let _ = svc_b.endpoint.set(ep_b.clone());
        let _ = svc_c.endpoint.set(ep_c.clone());

        let spawn_accept = |svc: Arc<P2pService>, ep: Endpoint| {
            tokio::spawn(async move {
                while let Some(inc) = ep.accept().await {
                    let s = Arc::clone(&svc);
                    tokio::spawn(async move {
                        let _ = s.handle_incoming(inc).await;
                    });
                }
            })
        };
        let accept_a = spawn_accept(Arc::clone(&svc_a), ep_a.clone());
        let accept_b = spawn_accept(Arc::clone(&svc_b), ep_b.clone());
        let accept_c = spawn_accept(Arc::clone(&svc_c), ep_c.clone());

        let alice_id = ep_a.id().to_string();
        let bob_id = ep_b.id().to_string();
        let carol_id = ep_c.id().to_string();

        // 1. Connect Bob <-> Carol and Alice <-> Bob.
        let join_c = svc_c.get_state().unwrap().join_code.unwrap();
        svc_b.add_friend_by_code(join_c).await.unwrap();
        svc_c.respond_friend_request(bob_id.clone(), true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(100)).await;

        let join_b = svc_b.get_state().unwrap().join_code.unwrap();
        svc_a.add_friend_by_code(join_b).await.unwrap();
        svc_b.respond_friend_request(alice_id.clone(), true).await.unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;

        // 2. Carol goes offline; Bob marks Carol offline; Alice syncs & queues invite for Carol.
        accept_c.abort();
        ep_c.close().await;
        {
            let mut sb = svc_b.store.lock().unwrap();
            if let Some(c) = sb.friends.iter_mut().find(|f| f.peer_id == carol_id) {
                c.online = false;
            }
        }
        svc_a.refresh_all_friends().await.unwrap();
        let state_a_q = svc_a
            .add_friend_by_peer(carol_id.clone(), Some("Carol".into()))
            .await
            .unwrap();
        assert_eq!(state_a_q.queued_invites.len(), 1);
        assert_eq!(svc_b.store.lock().unwrap().relayed_invites.len(), 1);

        // 3. Alice goes offline BEFORE Carol comes back online!
        accept_a.abort();
        ep_a.close().await;

        // 4. Carol comes back online (reloaded from persisted store) and syncs with Bob.
        let svc_c2 = P2pService::new(paths_c.clone());
        let _ = svc_c2.secret_key.set(key_c.clone());
        let ep_c2 = Endpoint::builder(presets::N0DisableRelay)
            .secret_key(key_c)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let _ = svc_c2.endpoint.set(ep_c2.clone());
        let accept_c2 = spawn_accept(Arc::clone(&svc_c2), ep_c2.clone());

        let state_c2 = svc_c2.refresh_all_friends().await.unwrap();
        assert_eq!(state_c2.pending_requests.len(), 1);
        assert_eq!(state_c2.pending_requests[0].peer_id, alice_id);
        assert_eq!(state_c2.pending_requests[0].display_name, "Alice");
        // Bob should have purged the relayed invite after Carol's AckRelayedInvites.
        assert!(svc_b.store.lock().unwrap().relayed_invites.is_empty());

        // 5. Carol accepts Alice's relayed invite while Alice is still offline.
        let state_c3 = svc_c2
            .respond_friend_request(alice_id.clone(), true)
            .await
            .unwrap();
        assert!(state_c3
            .friends
            .iter()
            .any(|f| f.peer_id == alice_id && f.status == FriendStatus::Connected));

        // 6. Alice comes back online (reloaded from persisted store with queued_invites intact).
        let svc_a2 = P2pService::new(paths_a.clone());
        let _ = svc_a2.secret_key.set(key_a.clone());
        let ep_a2 = Endpoint::builder(presets::N0DisableRelay)
            .secret_key(key_a)
            .alpns(vec![P2P_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let _ = svc_a2.endpoint.set(ep_a2.clone());
        let accept_a2 = spawn_accept(Arc::clone(&svc_a2), ep_a2.clone());
        assert_eq!(svc_a2.get_state().unwrap().queued_invites.len(), 1);

        // Update Alice's fresh loopback addr on Carol's side (in prod n0/relay handles this) and sync.
        {
            let mut sc = svc_c2.store.lock().unwrap();
            if let Some(a) = sc.friends.iter_mut().find(|f| f.peer_id == alice_id) {
                a.addr = ep_a2.addr();
            }
        }
        svc_c2.refresh_all_friends().await.unwrap();
        tokio::time::sleep(Duration::from_millis(120)).await;

        // Alice should have promoted Carol out of queued_invites into Connected friends!
        let state_a_final = svc_a2.get_state().unwrap();
        assert!(state_a_final.queued_invites.is_empty());
        assert!(state_a_final
            .friends
            .iter()
            .any(|f| f.peer_id == carol_id && f.status == FriendStatus::Connected));

        accept_a2.abort();
        accept_b.abort();
        accept_c2.abort();
    }

    #[test]
    fn signed_friend_invite_signature_and_policy_validation() {
        let key_a = SecretKey::generate();
        let key_c = SecretKey::generate();
        let alice_id = key_a.public().to_string();
        let carol_id = key_c.public().to_string();

        let valid_payload = FriendInvitePayload {
            from_peer_id: alice_id.clone(),
            to_peer_id: carol_id.clone(),
            from_display_name: "Alice".into(),
            from_profile_picture: None,
            from_addr: None,
            created_at: now_rfc3339(),
        };
        let signed = sign_friend_invite(&key_a, valid_payload.clone()).unwrap();
        assert!(verify_signed_friend_invite(&signed).is_ok());

        // Tampered target fails signature verification.
        let mut tampered_target = signed.clone();
        tampered_target.payload.to_peer_id = SecretKey::generate().public().to_string();
        assert!(verify_signed_friend_invite(&tampered_target).is_err());

        // Self-invite rejected.
        let mut self_payload = valid_payload.clone();
        self_payload.to_peer_id = alice_id.clone();
        let self_signed = sign_friend_invite(&key_a, self_payload).unwrap();
        assert!(verify_signed_friend_invite(&self_signed).is_err());

        // Expired invite rejected.
        let mut old_payload = valid_payload.clone();
        old_payload.created_at = "2020-01-01T00:00:00Z".into();
        let old_signed = sign_friend_invite(&key_a, old_payload).unwrap();
        assert!(verify_signed_friend_invite(&old_signed).is_err());

        // Relay policy on Bob's store: rejects if target opted out of mutual connections.
        let mut bob_store = FriendsStore::default();
        bob_store.friends.push(FriendRecord {
            peer_id: alice_id.clone(),
            display_name: "Alice".into(),
            profile_picture: None,
            addr: EndpointAddr::new(key_a.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            added_at: None,
            cached_profile: None,
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        bob_store.friends.push(FriendRecord {
            peer_id: carol_id.clone(),
            display_name: "Carol".into(),
            profile_picture: None,
            addr: EndpointAddr::new(key_c.public()),
            status: FriendStatus::Connected,
            online: false,
            last_seen_at: None,
            added_at: None,
            cached_profile: Some(PeerProfileSummary {
                peer_id: carol_id.clone(),
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: now_rfc3339(),
                collections: Vec::new(),
                friends: None,
                allow_mutual_connections: false,
                signed_relay_profile: None,
                activity_events: None,
                relayed_invites: Vec::new(),
            }),
            syncing_my_collections: Vec::new(),
            cached_via_peer_name: None,
        });
        assert!(deposit_relayed_invite_into_store(&mut bob_store, &alice_id, signed.clone()).is_err());

        // Once Carol allows mutual connections, deposit succeeds and deduplicates.
        bob_store.friends[1]
            .cached_profile
            .as_mut()
            .unwrap()
            .allow_mutual_connections = true;
        assert!(deposit_relayed_invite_into_store(&mut bob_store, &alice_id, signed.clone()).is_ok());
        assert_eq!(bob_store.relayed_invites.len(), 1);
        assert!(deposit_relayed_invite_into_store(&mut bob_store, &alice_id, signed).is_ok());
        assert_eq!(bob_store.relayed_invites.len(), 1);
    }

    #[test]
    fn get_state_includes_local_activity_in_feed_and_respects_privacy_and_clear() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.config_dir).unwrap();
        fs::create_dir_all(&paths.data_dir).unwrap();
        fs::create_dir_all(&paths.cache_dir).unwrap();

        let pub_code = share::encode_share_manifest(&ShareManifest {
            v: 1,
            name: Some("My Shared Pack".into()),
            game: share::ShareGameHint {
                id: Some("skyrim".into()),
                title: Some("Skyrim SE".into()),
                ..Default::default()
            },
            game_versions: None,
            mods: vec![share::ShareModEntry {
                s: share::ShareModSource::Nexus,
                domain: Some("skyrimspecialedition".into()),
                mod_id: Some(1),
                file_id: Some(10),
                community: None,
                namespace: None,
                name: None,
                game_id: None,
                display_name: Some("SkyUI".into()),
                version: Some("5.2".into()),
                fomod: None,
                options: None,
                enabled: None,
            }],
        })
        .unwrap();
        let priv_code = share::encode_share_manifest(&ShareManifest {
            v: 1,
            name: Some("My Private Pack".into()),
            game: share::ShareGameHint {
                id: Some("skyrim".into()),
                title: Some("Skyrim SE".into()),
                ..Default::default()
            },
            game_versions: None,
            mods: vec![],
        })
        .unwrap();

        share::save_saved_collections(
            &paths,
            &share::SavedCollectionsStore {
                collections: vec![
                    share::SavedCollectionEntry {
                        id: "pub".into(),
                        name: "My Shared Pack".into(),
                        code: pub_code.clone(),
                        game: share::ShareGameHint {
                            id: Some("skyrim".into()),
                            title: Some("Skyrim SE".into()),
                            ..Default::default()
                        },
                        game_versions: None,
                        mod_count: 1,
                        nexus_count: 1,
                        thunderstore_count: 0,
                        modio_count: 0,
                        source_game_id: None,
                        share_id: None,
                        created_at: "2026-11-22T10:00:00Z".into(),
                        updated_at: "2026-11-22T10:00:00Z".into(),
                    },
                    share::SavedCollectionEntry {
                        id: "priv".into(),
                        name: "My Private Pack".into(),
                        code: priv_code.clone(),
                        game: share::ShareGameHint {
                            id: Some("skyrim".into()),
                            title: Some("Skyrim SE".into()),
                            ..Default::default()
                        },
                        game_versions: None,
                        mod_count: 0,
                        nexus_count: 0,
                        thunderstore_count: 0,
                        modio_count: 0,
                        source_game_id: None,
                        share_id: None,
                        created_at: "2026-11-22T10:00:00Z".into(),
                        updated_at: "2026-11-22T10:00:00Z".into(),
                    },
                ],
            },
        )
        .unwrap();

        let svc = P2pService::new(paths.clone());
        {
            let mut store = svc.store.lock().unwrap();
            store.display_name = "PlayerOne".into();
            store.display_name_set = true;
            store
                .collection_visibility
                .insert("saved:pub".into(), CollectionVisibility::AllFriends);
            store
                .collection_visibility
                .insert("saved:priv".into(), CollectionVisibility::Private);
            store.feed_events.push(FriendFeedEvent {
                id: "alice:ev-1".into(),
                peer_id: "peer_alice".into(),
                friend_display_name: "Alice".into(),
                is_self: false,
                kind: FriendFeedEventKind::CollectionUpdated,
                collection_id: "active:skyrim".into(),
                collection_kind: FriendCollectionKind::ActiveLoadout,
                collection_name: "Alice Loadout".into(),
                previous_name: None,
                game_title: Some("Skyrim SE".into()),
                mod_count: Some(3),
                share_hash: "h_alice".into(),
                diff: None,
                occurred_at: "2026-11-22T12:00:00Z".into(),
            });
            store.local_activity = vec![
                LocalActivityEvent {
                    id: "local-1".into(),
                    kind: FriendFeedEventKind::CollectionAdded,
                    collection_id: "saved:pub".into(),
                    collection_kind: FriendCollectionKind::SavedCollection,
                    collection_name: "My Shared Pack".into(),
                    previous_name: None,
                    game_title: Some("Skyrim SE".into()),
                    mod_count: Some(1),
                    share_hash: share::share_hash(&pub_code),
                    diff: None,
                    occurred_at: "2026-11-22T13:00:00Z".into(),
                },
                LocalActivityEvent {
                    id: "local-priv".into(),
                    kind: FriendFeedEventKind::CollectionAdded,
                    collection_id: "saved:priv".into(),
                    collection_kind: FriendCollectionKind::SavedCollection,
                    collection_name: "My Private Pack".into(),
                    previous_name: None,
                    game_title: Some("Skyrim SE".into()),
                    mod_count: Some(0),
                    share_hash: share::share_hash(&priv_code),
                    diff: None,
                    occurred_at: "2026-11-22T14:00:00Z".into(),
                },
            ];
            save_friends_store(&paths, &store).unwrap();
        }

        // Combined feed includes shared local event (sorted newest-first) and excludes Private.
        let state = svc.get_state().unwrap();
        assert_eq!(state.feed_events.len(), 2);
        assert_eq!(state.feed_events[0].id, "self:local-1");
        assert!(state.feed_events[0].is_self);
        assert_eq!(state.feed_events[0].friend_display_name, "PlayerOne");
        assert_eq!(state.feed_events[1].id, "alice:ev-1");
        assert!(!state.feed_events[1].is_self);

        // Clearing feed hides past local events from get_state() without wiping store.local_activity.
        let cleared = svc.clear_feed().unwrap();
        assert!(cleared.feed_events.is_empty());
        assert_eq!(svc.store.lock().unwrap().local_activity.len(), 2);

        // A new local activity event occurring after feed_cleared_at reappears in the feed.
        let after_clear_ts = (chrono::Utc::now() + chrono::Duration::seconds(5)).to_rfc3339();
        {
            let mut store = svc.store.lock().unwrap();
            store.local_activity.push(LocalActivityEvent {
                id: "local-2".into(),
                kind: FriendFeedEventKind::CollectionRenamed,
                collection_id: "saved:pub".into(),
                collection_kind: FriendCollectionKind::SavedCollection,
                collection_name: "Renamed Pack".into(),
                previous_name: Some("My Shared Pack".into()),
                game_title: Some("Skyrim SE".into()),
                mod_count: Some(1),
                share_hash: share::share_hash(&pub_code),
                diff: None,
                occurred_at: after_clear_ts,
            });
        }
        let after_new = svc.get_state().unwrap();
        assert_eq!(after_new.feed_events.len(), 1);
        assert_eq!(after_new.feed_events[0].id, "self:local-2");
        assert!(after_new.feed_events[0].is_self);
    }

    #[test]
    fn active_loadout_feed_option_and_saved_collection_deduplication() {
        let dir = tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        fs::create_dir_all(&paths.config_dir).unwrap();
        fs::create_dir_all(&paths.data_dir).unwrap();
        fs::create_dir_all(&paths.cache_dir).unwrap();

        let make_code = |name: &str, mod_ver: &str| {
            share::encode_share_manifest(&ShareManifest {
                v: 1,
                name: Some(name.into()),
                game: share::ShareGameHint {
                    id: Some("cyberpunk2077".into()),
                    title: Some("Cyberpunk 2077".into()),
                    ..Default::default()
                },
                game_versions: None,
                mods: vec![share::ShareModEntry {
                    s: share::ShareModSource::Nexus,
                    domain: Some("cyberpunk2077".into()),
                    mod_id: Some(1),
                    file_id: Some(if mod_ver == "1.0" { 10 } else { 20 }),
                    community: None,
                    namespace: None,
                    name: None,
                    game_id: None,
                    display_name: Some("ArchiveXL".into()),
                    version: Some(mod_ver.into()),
                    fomod: None,
                    options: None,
                    enabled: None,
                }],
            })
            .unwrap()
        };

        let inc_v1 = make_code("INC", "1.0");
        let active_v1 = make_code("Cyberpunk 2077 Loadout", "1.0");
        let inc_v2 = make_code("INC", "2.0");
        let active_v2 = make_code("Cyberpunk 2077 Loadout", "2.0");

        // 1. Recording local activity when active loadout matches a shared saved collection
        //    emits only the SavedCollection event (no duplicate ActiveLoadout event).
        let mut store = FriendsStore::default();
        store
            .collection_visibility
            .insert("saved:inc".into(), CollectionVisibility::AllFriends);
        record_local_activity_in_store(
            &mut store,
            vec![
                (
                    "saved:inc".into(),
                    FriendCollectionKind::SavedCollection,
                    "INC".into(),
                    share::share_hash(&inc_v1),
                    inc_v1.clone(),
                ),
                (
                    "active:cyberpunk2077".into(),
                    FriendCollectionKind::ActiveLoadout,
                    "Cyberpunk 2077 Loadout".into(),
                    share::share_hash(&active_v1),
                    active_v1.clone(),
                ),
            ],
            "2026-11-22T10:00:00Z",
        );
        assert!(store.local_activity.is_empty());

        record_local_activity_in_store(
            &mut store,
            vec![
                (
                    "saved:inc".into(),
                    FriendCollectionKind::SavedCollection,
                    "INC".into(),
                    share::share_hash(&inc_v2),
                    inc_v2.clone(),
                ),
                (
                    "active:cyberpunk2077".into(),
                    FriendCollectionKind::ActiveLoadout,
                    "Cyberpunk 2077 Loadout".into(),
                    share::share_hash(&active_v2),
                    active_v2.clone(),
                ),
            ],
            "2026-11-22T11:00:00Z",
        );
        assert_eq!(store.local_activity.len(), 1);
        assert_eq!(store.local_activity[0].collection_id, "saved:inc");
        assert_eq!(store.local_activity[0].collection_name, "INC");

        // 2. Historical/remote duplicate (ActiveLoadout + SavedCollection) collapses into SavedCollection.
        let diff = FriendFeedDiff {
            mods_updated: vec!["ArchiveXL".into()],
            mods_updated_count: 1,
            ..Default::default()
        };
        let mut feed = vec![
            FriendFeedEvent {
                id: "self:saved-1".into(),
                peer_id: "self".into(),
                friend_display_name: "Luu".into(),
                is_self: true,
                kind: FriendFeedEventKind::CollectionUpdated,
                collection_id: "saved:inc".into(),
                collection_kind: FriendCollectionKind::SavedCollection,
                collection_name: "INC".into(),
                previous_name: None,
                game_title: Some("Cyberpunk 2077".into()),
                mod_count: Some(70),
                share_hash: "h_inc".into(),
                diff: Some(diff.clone()),
                occurred_at: "2026-11-22T12:00:00Z".into(),
            },
            FriendFeedEvent {
                id: "self:active-1".into(),
                peer_id: "self".into(),
                friend_display_name: "Luu".into(),
                is_self: true,
                kind: FriendFeedEventKind::CollectionUpdated,
                collection_id: "active:cyberpunk2077".into(),
                collection_kind: FriendCollectionKind::ActiveLoadout,
                collection_name: "Cyberpunk 2077 Loadout".into(),
                previous_name: None,
                game_title: Some("Cyberpunk 2077".into()),
                mod_count: Some(70),
                share_hash: "h_active".into(),
                diff: Some(diff),
                occurred_at: "2026-11-22T12:00:10Z".into(),
            },
        ];
        dedupe_active_loadout_feed_events(&mut feed);
        assert_eq!(feed.len(), 1);
        assert_eq!(feed[0].collection_id, "saved:inc");

        // 3. Toggling share_active_loadouts_in_feed off persists and hides ActiveLoadout events.
        let svc = P2pService::new(paths.clone());
        assert!(svc.get_state().unwrap().share_active_loadouts_in_feed);
        let updated = svc.set_share_active_loadouts_in_feed(false).unwrap();
        assert!(!updated.share_active_loadouts_in_feed);
        let reloaded = P2pService::new(paths);
        assert!(!reloaded.get_state().unwrap().share_active_loadouts_in_feed);

        // When disabled, recording a standalone ActiveLoadout change emits nothing.
        let mut store_off = FriendsStore::default();
        store_off.share_active_loadouts_in_feed = false;
        record_local_activity_in_store(
            &mut store_off,
            vec![(
                "active:cyberpunk2077".into(),
                FriendCollectionKind::ActiveLoadout,
                "Cyberpunk 2077 Loadout".into(),
                share::share_hash(&active_v1),
                active_v1,
            )],
            "2026-11-22T10:00:00Z",
        );
        record_local_activity_in_store(
            &mut store_off,
            vec![(
                "active:cyberpunk2077".into(),
                FriendCollectionKind::ActiveLoadout,
                "Cyberpunk 2077 Loadout".into(),
                share::share_hash(&active_v2),
                active_v2,
            )],
            "2026-11-22T11:00:00Z",
        );
        assert!(store_off.local_activity.is_empty());
    }
}
