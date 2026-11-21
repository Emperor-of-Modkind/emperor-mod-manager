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
const MAX_STREAMS_PER_CONN: usize = 8;
const MAX_PENDING_REQUESTS: usize = 50;
const MAX_JOIN_CODE_LEN: usize = 16 * 1024;
const MAX_JOIN_TICKET_BYTES: usize = 64 * 1024;
const MAX_DISPLAY_NAME_CHARS: usize = 64;
const MAX_REMOTE_COLLECTIONS: usize = 128;
const MAX_REMOTE_FRIENDS: usize = 256;
const MAX_COLLECTION_ID_CHARS: usize = 128;
const MAX_COLLECTION_NAME_CHARS: usize = 128;
const MAX_SHARE_HASH_CHARS: usize = 128;
const MAX_PEER_RELAY_ADDRS: usize = 2;
const MAX_PEER_IP_ADDRS: usize = 8;
const MAX_FEED_EVENTS: usize = 100;
const MAX_FEED_DIFF_NAMES: usize = 5;
const HELLO_PER_PEER_COOLDOWN: Duration = Duration::from_secs(5);
const HELLO_GLOBAL_WINDOW: Duration = Duration::from_secs(60);
const HELLO_GLOBAL_MAX_PER_WINDOW: usize = 20;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FriendCollectionKind {
    ActiveLoadout,
    SavedCollection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendShareRef {
    pub id: String,
    pub kind: FriendCollectionKind,
    pub name: String,
    /// Compact `#emperor1...` share code. May be empty in delta responses when
    /// `share_hash` matches the caller's cached hash.
    pub share_code: String,
    pub share_hash: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharedFriendRef {
    pub peer_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub addr: Option<EndpointAddr>,
}

fn default_allow_mutual_connections() -> bool {
    true
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cached_profile: Option<PeerProfileSummary>,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendFeedEvent {
    pub id: String,
    pub peer_id: String,
    pub friend_display_name: String,
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
    #[serde(default)]
    pub appear_offline: bool,
    #[serde(default)]
    pub feed_events: Vec<FriendFeedEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feed_last_read_at: Option<String>,
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
            appear_offline: false,
            feed_events: Vec::new(),
            feed_last_read_at: None,
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

fn sanitize_short_text(raw: &str, max_chars: usize) -> String {
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
    let b64_payload = data_url
        .strip_prefix("data:image/png;base64,")
        .or_else(|| data_url.strip_prefix("data:image/jpeg;base64,"))
        .or_else(|| data_url.strip_prefix("data:image/webp;base64,"))
        .or_else(|| data_url.strip_prefix("data:image/gif;base64,"))
        .ok_or_else(|| anyhow!("Profile picture must be a PNG, JPEG, WebP, or GIF image"))?;
    if b64_payload.is_empty() {
        bail!("Profile picture payload is empty");
    }
    base64::engine::general_purpose::STANDARD
        .decode(b64_payload.as_bytes())
        .context("Profile picture contains invalid base64 data")?;
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
    pub friends: Vec<FriendRecord>,
    pub pending_requests: Vec<PendingFriendRequest>,
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
    Denied(String),
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
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
    for f in &mut store.friends {
        f.online = false;
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

pub fn build_local_profile(
    paths: &Paths,
    peer_id: &str,
    store: &FriendsStore,
    known_hashes: Option<&HashMap<String, String>>,
    viewer_peer_id: Option<&str>,
) -> PeerProfileSummary {
    let mut collections = Vec::new();
    let now = now_rfc3339();

    if let Ok(cfg) = load_config(paths) {
        for game in &cfg.managed_games {
            let col_id = format!("active:{}", game.id);
            if !is_collection_visible_to(store, &col_id, viewer_peer_id) {
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
                    let unchanged = known_hashes
                        .and_then(|m| m.get(&col_id))
                        .is_some_and(|h| h == &hash);
                    collections.push(FriendShareRef {
                        id: col_id,
                        kind: FriendCollectionKind::ActiveLoadout,
                        name,
                        share_code: if unchanged { String::new() } else { code },
                        share_hash: hash,
                        updated_at: now.clone(),
                    });
                }
            }
        }
    }

    if let Ok(saved) = share::load_saved_collections(paths) {
        for entry in saved.collections {
            let col_id = format!("saved:{}", entry.id);
            if !is_collection_visible_to(store, &col_id, viewer_peer_id) {
                continue;
            }
            let hash = share::share_hash(&entry.code);
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
            });
        }
    }

    let friends = if is_friends_list_visible_to(store, viewer_peer_id) {
        Some(
            store
                .friends
                .iter()
                .filter(|f| {
                    f.status == FriendStatus::Connected
                        && Some(f.peer_id.as_str()) != viewer_peer_id
                        && f.cached_profile
                            .as_ref()
                            .is_none_or(|p| p.allow_mutual_connections)
                })
                .take(MAX_REMOTE_FRIENDS)
                .map(|f| SharedFriendRef {
                    peer_id: f.peer_id.clone(),
                    display_name: sanitize_display_name(&f.display_name, "Friend"),
                    addr: Some(join_ticket_addr(&f.addr)),
                })
                .collect(),
        )
    } else {
        None
    };

    PeerProfileSummary {
        peer_id: peer_id.to_string(),
        display_name: effective_display_name(store),
        profile_picture: store.profile_picture.clone(),
        updated_at: now,
        collections,
        friends,
        allow_mutual_connections: store.allow_mutual_connections,
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

pub struct P2pService {
    paths: Paths,
    store: Arc<Mutex<FriendsStore>>,
    endpoint: OnceCell<Endpoint>,
    app: Mutex<Option<AppHandle>>,
    incoming_sem: Arc<Semaphore>,
    hello_limiter: Mutex<HelloRateLimiter>,
}

impl P2pService {
    pub fn new(paths: Paths) -> Arc<Self> {
        let store = load_friends_store(&paths);
        Arc::new(Self {
            paths,
            store: Arc::new(Mutex::new(store)),
            endpoint: OnceCell::new(),
            app: Mutex::new(None),
            incoming_sem: Arc::new(Semaphore::new(MAX_CONCURRENT_INCOMING)),
            hello_limiter: Mutex::new(HelloRateLimiter::default()),
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

    async fn init_and_serve(self: Arc<Self>) -> Result<()> {
        let key = load_or_create_secret_key(&self.paths)?;
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
                        .map(|store| store.friends.iter().any(|f| f.peer_id == remote_id))
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
                            (true, was_pending || missing_profile, local_name, true)
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
                P2pRequest::GetProfile { known_hashes } => {
                    let (store_snapshot, status_flipped) = {
                        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                        let is_friend = store
                            .friends
                            .iter()
                            .any(|f| f.peer_id == remote_id && f.status == FriendStatus::Connected);
                        if !is_friend {
                            (None, false)
                        } else {
                            let mut flipped = false;
                            if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == remote_id) {
                                if !f.online {
                                    flipped = true;
                                }
                                f.online = true;
                                f.last_seen_at = Some(now_rfc3339());
                            }
                            (Some(store.clone()), flipped)
                        }
                    };
                    if status_flipped {
                        self.emit_changed();
                    }
                    match store_snapshot {
                        Some(snapshot_store) => {
                            let paths = self.paths.clone();
                            let local_id = ep.id().to_string();
                            let viewer = remote_id.clone();
                            let profile = tokio::task::spawn_blocking(move || {
                                build_local_profile(
                                    &paths,
                                    &local_id,
                                    &snapshot_store,
                                    Some(&known_hashes),
                                    Some(&viewer),
                                )
                            })
                            .await
                            .context("build local profile task failed")?;
                            write_frame(&mut send, &P2pResponse::Profile(profile)).await?;
                        }
                        None => {
                            write_frame(
                                &mut send,
                                &P2pResponse::Denied("Not connected as friends".into()),
                            )
                            .await?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub fn get_state(&self) -> Result<FriendsState> {
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
        Ok(FriendsState {
            endpoint_ready,
            appear_offline: store.appear_offline,
            peer_id,
            join_code,
            display_name: store.display_name.clone(),
            display_name_set: store.display_name_set,
            profile_picture: store.profile_picture.clone(),
            friends_list_visibility: store.friends_list_visibility.clone(),
            allow_mutual_connections: store.allow_mutual_connections,
            friends: store.friends.clone(),
            pending_requests: store.pending_requests.clone(),
            local_collections,
            feed_events: store.feed_events.clone(),
            feed_last_read_at: store.feed_last_read_at.clone(),
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

        let should_dial = {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            if let Some(existing) = store.friends.iter().find(|f| f.peer_id == canonical_id) {
                existing.status != FriendStatus::Connected
            } else {
                let mut discovered_addr: Option<EndpointAddr> = None;
                let mut discovered_name: Option<String> = None;
                for friend in &store.friends {
                    if friend.status != FriendStatus::Connected {
                        continue;
                    }
                    if let Some(ref profile) = friend.cached_profile {
                        if let Some(ref shared_friends) = profile.friends {
                            if let Some(entry) =
                                shared_friends.iter().find(|sf| sf.peer_id == canonical_id)
                            {
                                if discovered_name.is_none() && !entry.display_name.is_empty() {
                                    discovered_name = Some(entry.display_name.clone());
                                }
                                if let Some(ref a) = entry.addr {
                                    if let Ok(sanitized) = sanitize_peer_addr(parsed_id, a) {
                                        if !sanitized.addrs.is_empty() {
                                            discovered_addr = Some(sanitized);
                                            break;
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                let final_name = display_name
                    .as_deref()
                    .map(|n| sanitize_display_name(n, ""))
                    .filter(|n| !n.is_empty())
                    .or(discovered_name)
                    .unwrap_or_else(|| "Friend".to_string());
                let final_addr = discovered_addr.unwrap_or_else(|| EndpointAddr::new(parsed_id));
                store.friends.push(FriendRecord {
                    peer_id: canonical_id.clone(),
                    display_name: final_name,
                    profile_picture: None,
                    addr: final_addr,
                    status: FriendStatus::PendingOutgoing,
                    online: false,
                    last_seen_at: None,
                    cached_profile: None,
                });
                save_friends_store(&self.paths, &store)?;
                true
            }
        };

        self.emit_changed();
        if should_dial {
            let _ = self.dial_and_sync_peer(&canonical_id).await;
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
            if let Some(existing) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
                existing.addr = ticket.addr.clone();
                existing.display_name = ticket.name.clone();
            } else {
                store.friends.push(FriendRecord {
                    peer_id: peer_id.clone(),
                    display_name: ticket.name.clone(),
                    profile_picture: None,
                    addr: ticket.addr.clone(),
                    status: FriendStatus::PendingOutgoing,
                    online: false,
                    last_seen_at: None,
                    cached_profile: None,
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
                if let Some(ref r) = req {
                    if let Some(existing) = store.friends.iter_mut().find(|f| f.peer_id == peer_id)
                    {
                        existing.status = FriendStatus::Connected;
                        existing.addr = r.addr.clone();
                        existing.display_name = r.display_name.clone();
                    } else {
                        store.friends.push(FriendRecord {
                            peer_id: r.peer_id.clone(),
                            display_name: r.display_name.clone(),
                            profile_picture: r.profile_picture.clone(),
                            addr: r.addr.clone(),
                            status: FriendStatus::Connected,
                            online: false,
                            last_seen_at: Some(now_rfc3339()),
                            cached_profile: None,
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
            store.feed_events.clear();
            store.feed_last_read_at = Some(now_rfc3339());
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub async fn refresh_all_friends(&self) -> Result<FriendsState> {
        if self.is_appear_offline() {
            return self.get_state();
        }
        let peer_ids: Vec<String> = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.friends.iter().map(|f| f.peer_id.clone()).collect()
        };
        if !peer_ids.is_empty() {
            let _guard = CheckingGuard::new(self);
            futures_util::future::join_all(
                peer_ids.iter().map(|pid| self.dial_and_sync_peer(pid)),
            )
            .await;
        }
        self.get_state()
    }

    async fn heartbeat_friends(&self) -> Result<()> {
        if self.is_appear_offline() {
            return Ok(());
        }
        let peers: Vec<(String, FriendStatus)> = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store
                .friends
                .iter()
                .map(|f| (f.peer_id.clone(), f.status))
                .collect()
        };
        if !peers.is_empty() {
            let _guard = CheckingGuard::new(self);
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
        }
        Ok(())
    }

    async fn connect_peer(ep: &Endpoint, addr: &EndpointAddr) -> Result<iroh::endpoint::Connection> {
        match tokio::time::timeout(DIAL_TIMEOUT, ep.connect(addr.clone(), P2P_ALPN)).await {
            Ok(Ok(conn)) => Ok(conn),
            _ => {
                // Fallback to n0/pkarr discovery with just EndpointId if cached socket addrs are stale.
                let conn = tokio::time::timeout(
                    DIAL_TIMEOUT,
                    ep.connect(EndpointAddr::new(addr.id), P2P_ALPN),
                )
                .await
                .map_err(|_| anyhow!("connect timed out"))??;
                Ok(conn)
            }
        }
    }

    fn peer_context(&self, peer_id: &str) -> Result<Option<(EndpointAddr, String, HashMap<String, String>)>> {
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
        Ok(Some((
            friend.addr.clone(),
            effective_display_name(&store),
            hashes,
        )))
    }

    fn apply_fetched_profile(&self, peer_id: &str, mut profile: PeerProfileSummary) -> Result<bool> {
        if profile.peer_id != peer_id {
            bail!("Remote profile peer_id does not match authenticated peer");
        }
        profile.display_name = sanitize_display_name(&profile.display_name, "Friend");
        if let Some(ref pic) = profile.profile_picture {
            if let Err(e) = validate_profile_picture(pic) {
                log::warn!("Dropped invalid remote profile picture from {peer_id}: {e}");
                profile.profile_picture = None;
            }
        }
        profile.collections.truncate(MAX_REMOTE_COLLECTIONS);
        for col in &mut profile.collections {
            col.id = sanitize_short_text(&col.id, MAX_COLLECTION_ID_CHARS);
            col.name = sanitize_short_text(&col.name, MAX_COLLECTION_NAME_CHARS);
            col.share_hash = sanitize_short_text(&col.share_hash, MAX_SHARE_HASH_CHARS);
            if col.share_code.len() > share::MAX_SHARE_CODE_BYTES {
                log::warn!("Dropped oversized share_code from {peer_id} for collection {}", col.id);
                col.share_code.clear();
            }
        }
        profile.collections.retain(|c| !c.id.is_empty());

        let local_peer_id = self.endpoint.get().map(|ep| ep.id().to_string());
        if let Some(ref mut shared_friends) = profile.friends {
            let mut seen_peers = HashSet::new();
            let mut cleaned = Vec::with_capacity(shared_friends.len().min(MAX_REMOTE_FRIENDS));
            for entry in shared_friends.drain(..).take(MAX_REMOTE_FRIENDS) {
                let Ok(parsed_id) = entry.peer_id.trim().parse::<EndpointId>() else {
                    continue;
                };
                let canonical = parsed_id.to_string();
                if canonical == peer_id || local_peer_id.as_deref() == Some(canonical.as_str()) {
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
                cleaned.push(SharedFriendRef {
                    peer_id: canonical,
                    display_name: sanitized_name,
                    addr: sanitized_addr,
                });
            }
            *shared_friends = cleaned;
        }

        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
        let mut changed = false;
        let now = now_rfc3339();
        let mut new_feed_events: Vec<FriendFeedEvent> = Vec::new();
        let mut visible_col_ids: Option<HashSet<String>> = None;
        if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
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
                new_feed_events = collect_profile_feed_events(
                    peer_id,
                    &profile.display_name,
                    prev,
                    &profile,
                    &now,
                );
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
                || f.display_name != profile.display_name
                || f.profile_picture != profile.profile_picture
                || collections_changed
                || friends_changed;
            f.display_name = profile.display_name.clone();
            f.profile_picture = profile.profile_picture.clone();
            f.cached_profile = Some(profile);
            f.online = true;
            f.last_seen_at = Some(now);
        }
        if let Some(visible_ids) = visible_col_ids {
            // Prune feed items for collections no longer visible from this peer (e.g. made Private or unshared).
            let before_len = store.feed_events.len();
            store
                .feed_events
                .retain(|ev| ev.peer_id != peer_id || visible_ids.contains(&ev.collection_id));
            if store.feed_events.len() != before_len {
                changed = true;
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

    async fn sync_peer_profile_only(&self, peer_id: &str) -> Result<()> {
        let Some(ep) = self.endpoint.get() else {
            return Ok(());
        };
        let Some((addr, _, known_hashes)) = self.peer_context(peer_id)? else {
            return Ok(());
        };

        let profile_res = tokio::time::timeout(DIAL_TIMEOUT, async {
            let conn = Self::connect_peer(ep, &addr).await?;
            let (mut send, mut recv) = conn.open_bi().await?;
            write_frame(&mut send, &P2pRequest::GetProfile { known_hashes }).await?;
            let resp: P2pResponse = read_frame(&mut recv).await?;
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
        let Some((addr, local_name, known_hashes)) = self.peer_context(peer_id)? else {
            return Ok(());
        };
        let local_addr = ep.addr();

        // Single connection: stream 1 = Hello, stream 2 = GetProfile (if accepted).
        let session_res = tokio::time::timeout(DIAL_TIMEOUT, async {
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
                write_frame(&mut send2, &P2pRequest::GetProfile { known_hashes }).await?;
                Some(read_frame::<P2pResponse>(&mut recv2).await?)
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
                    }],
                    friends: None,
            allow_mutual_connections: true,
                }),
            }],
            pending_requests: vec![],
            hidden_collection_ids: HashSet::new(),
            collection_visibility: HashMap::new(),
            friends_list_visibility: CollectionVisibility::AllFriends,
            allow_mutual_connections: true,
            appear_offline: false,
            feed_events: vec![],
            feed_last_read_at: None,
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
                cached_profile: None,
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
            }],
            friends: None,
            allow_mutual_connections: true,
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
            }],
            friends: None,
            allow_mutual_connections: true,
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
            }],
            friends: None,
            allow_mutual_connections: true,
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
                cached_profile: None,
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
                cached_profile: None,
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
            }],
            friends: None,
            allow_mutual_connections: true,
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
            }],
            friends: None,
            allow_mutual_connections: true,
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
                },
                FriendShareRef {
                    id: "saved:cozy".into(),
                    kind: FriendCollectionKind::SavedCollection,
                    name: "Cozy Pack".into(),
                    share_code: code_saved.clone(),
                    share_hash: hash_saved.clone(),
                    updated_at: "2026-11-21T00:02:00Z".into(),
                },
            ],
            friends: None,
            allow_mutual_connections: true,
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
                },
                FriendShareRef {
                    id: "saved:cozy".into(),
                    kind: FriendCollectionKind::SavedCollection,
                    name: "Ultra Cozy Pack".into(),
                    share_code: String::new(),
                    share_hash: hash_saved.clone(),
                    updated_at: "2026-11-21T00:03:00Z".into(),
                },
            ],
            friends: None,
            allow_mutual_connections: true,
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
            }],
            friends: None,
            allow_mutual_connections: true,
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
                },
                FriendShareRef {
                    id: "saved:cozy".into(),
                    kind: FriendCollectionKind::SavedCollection,
                    name: "Back Again".into(),
                    share_code: code_saved,
                    share_hash: hash_saved,
                    updated_at: "2026-11-21T00:05:00Z".into(),
                },
            ],
            friends: None,
            allow_mutual_connections: true,
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
            cached_profile: None,
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
            cached_profile: None,
        });
        store.friends.push(FriendRecord {
            peer_id: pending_key.public().to_string(),
            display_name: "PendingPete".into(),
            profile_picture: None,
            addr: EndpointAddr::new(pending_key.public()),
            status: FriendStatus::PendingOutgoing,
            online: false,
            last_seen_at: None,
            cached_profile: None,
        });

        let bob_id = bob_key.public().to_string();
        let profile = build_local_profile(
            &paths,
            &self_key.public().to_string(),
            &store,
            None,
            Some(&bob_id),
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
                cached_profile: None,
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
                },
                // Self-reference (Bob listing Bob) -> dropped
                SharedFriendRef {
                    peer_id: bob_id.clone(),
                    display_name: "Bob".into(),
                    addr: None,
                },
                // Valid friend with bidi chars & mismatched addr -> name sanitized, bad addr dropped
                SharedFriendRef {
                    peer_id: carol_id.clone(),
                    display_name: "Carol\u{202E}\x07".into(),
                    addr: Some(EndpointAddr::new(bob_key.public())),
                },
                // Duplicate Carol -> dropped
                SharedFriendRef {
                    peer_id: carol_id.clone(),
                    display_name: "Carol Dup".into(),
                    addr: None,
                },
            ]),
            allow_mutual_connections: true,
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
            cached_profile: Some(PeerProfileSummary {
                peer_id: carol_id,
                display_name: "Carol".into(),
                profile_picture: None,
                updated_at: "t1".into(),
                collections: vec![],
                friends: None,
                allow_mutual_connections: false,
            }),
        });
        store.friends.push(FriendRecord {
            peer_id: dave_id.clone(),
            display_name: "Dave".into(),
            profile_picture: None,
            addr: EndpointAddr::new(dave_key.public()),
            status: FriendStatus::Connected,
            online: true,
            last_seen_at: None,
            cached_profile: Some(PeerProfileSummary {
                peer_id: dave_id.clone(),
                display_name: "Dave".into(),
                profile_picture: None,
                updated_at: "t1".into(),
                collections: vec![],
                friends: None,
                allow_mutual_connections: true,
            }),
        });

        let profile_for_alice = build_local_profile(
            &paths,
            &self_key.public().to_string(),
            &store,
            None,
            Some(&alice_id),
        );
        let shared = profile_for_alice.friends.unwrap();
        assert_eq!(shared.len(), 1);
        assert_eq!(shared[0].peer_id, dave_id);
        assert_eq!(shared[0].display_name, "Dave");
    }
}
