//! Serverless peer-to-peer social system powered by `iroh`.
//!
//! Uses direct QUIC connections first (via socket addresses encoded in join
//! codes), with UDP holepunching and community relay fallback when NATs
//! require it. Collections are exchanged as live-updating Emperor Share Codes
//! (`#emperor1...`) to keep wire payloads tiny.

use std::{
    collections::{HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use iroh::{
    endpoint::{presets, Incoming},
    Endpoint, EndpointAddr, SecretKey,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter};
use tokio::sync::OnceCell;

use crate::{
    config::{load_config, Paths},
    mods,
    share::{self, ShareManifest},
};

pub const P2P_ALPN: &[u8] = b"emperor-p2p/1";
pub const JOIN_CODE_PREFIX: &str = "emperor-friend1:";
const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PeerProfileSummary {
    pub peer_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile_picture: Option<String>,
    pub updated_at: String,
    pub collections: Vec<FriendShareRef>,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    #[serde(default)]
    pub appear_offline: bool,
}

fn default_display_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Emperor User".to_string())
}

fn effective_display_name(store: &FriendsStore) -> String {
    let trimmed = store.display_name.trim();
    if trimmed.is_empty() {
        default_display_name()
    } else {
        trimmed.to_string()
    }
}

const MAX_PROFILE_PICTURE_BYTES: usize = 262_144;

fn validate_profile_picture(data_url: &str) -> Result<()> {
    if data_url.len() > MAX_PROFILE_PICTURE_BYTES {
        bail!("Profile picture exceeds 256 KB limit");
    }
    let valid_prefix = data_url.starts_with("data:image/png;base64,")
        || data_url.starts_with("data:image/jpeg;base64,")
        || data_url.starts_with("data:image/webp;base64,")
        || data_url.starts_with("data:image/gif;base64,");
    if !valid_prefix {
        bail!("Profile picture must be a PNG, JPEG, WebP, or GIF image");
    }
    Ok(())
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
    pub friends: Vec<FriendRecord>,
    pub pending_requests: Vec<PendingFriendRequest>,
    pub local_collections: Vec<LocalShareableCollection>,
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
    let payload = trimmed
        .strip_prefix(JOIN_CODE_PREFIX)
        .ok_or_else(|| anyhow!("Join code must start with {JOIN_CODE_PREFIX}"))?;
    let compressed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload.trim())
        .context("invalid base64 in join code")?;
    let mut decoder = DeflateDecoder::new(&compressed[..]);
    let mut json = Vec::new();
    decoder
        .read_to_end(&mut json)
        .context("failed to decompress join code")?;
    let ticket: FriendJoinTicket =
        serde_json::from_slice(&json).context("invalid join ticket payload")?;
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
    let mut store: FriendsStore = serde_json::from_str(&raw).unwrap_or_default();
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
    fs::write(&path, json).context("write friends store")
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
    fs::write(&path, key.to_bytes()).context("write p2p identity key")?;
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

    PeerProfileSummary {
        peer_id: peer_id.to_string(),
        display_name: effective_display_name(store),
        profile_picture: store.profile_picture.clone(),
        updated_at: now,
        collections,
    }
}

async fn write_frame<T: Serialize>(send: &mut iroh::endpoint::SendStream, msg: &T) -> Result<()> {
    let data = serde_json::to_vec(msg).context("serialize p2p frame")?;
    if data.len() > MAX_FRAME_BYTES {
        bail!("p2p frame too large");
    }
    let len = (data.len() as u32).to_be_bytes();
    send.write_all(&len).await?;
    send.write_all(&data).await?;
    send.finish()?;
    Ok(())
}

async fn read_frame<T: for<'de> Deserialize<'de>>(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<T> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > MAX_FRAME_BYTES {
        bail!("incoming p2p frame exceeds max size");
    }
    let mut buf = vec![0u8; len];
    recv.read_exact(&mut buf).await?;
    serde_json::from_slice(&buf).context("decode p2p frame")
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
}

impl P2pService {
    pub fn new(paths: Paths) -> Arc<Self> {
        let store = load_friends_store(&paths);
        Arc::new(Self {
            paths,
            store: Arc::new(Mutex::new(store)),
            endpoint: OnceCell::new(),
            app: Mutex::new(None),
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
            let svc = Arc::clone(&self);
            tauri::async_runtime::spawn(async move {
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
        let conn = incoming.await?;
        let remote_id = conn.remote_id().to_string();

        while let Ok((mut send, mut recv)) = conn.accept_bi().await {
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
                    let (accepted, needs_profile_pull, local_name) = {
                        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                        let local_name = effective_display_name(&store);
                        if let Some(friend) = store.friends.iter_mut().find(|f| f.peer_id == remote_id)
                        {
                            let was_pending = friend.status == FriendStatus::PendingOutgoing;
                            let missing_profile = friend.cached_profile.is_none();
                            friend.status = FriendStatus::Connected;
                            friend.display_name = display_name;
                            friend.addr = addr;
                            friend.online = true;
                            friend.last_seen_at = Some(now_rfc3339());
                            (true, was_pending || missing_profile, local_name)
                        } else {
                            if let Some(existing) = store
                                .pending_requests
                                .iter_mut()
                                .find(|r| r.peer_id == remote_id)
                            {
                                existing.display_name = display_name;
                                existing.addr = addr;
                            } else {
                                store.pending_requests.push(PendingFriendRequest {
                                    peer_id: remote_id.clone(),
                                    display_name,
                                    profile_picture: None,
                                    addr,
                                    received_at: now_rfc3339(),
                                });
                            }
                            (false, false, local_name)
                        }
                    };
                    if let Ok(store) = self.store.lock() {
                        let _ = save_friends_store(&self.paths, &store);
                    }
                    self.emit_changed();
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
                    let (snapshot, status_flipped) = {
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
                            let local_id = ep.id().to_string();
                            let profile = build_local_profile(
                                &self.paths,
                                &local_id,
                                &store,
                                Some(&known_hashes),
                                Some(&remote_id),
                            );
                            (Some(profile), flipped)
                        }
                    };
                    if status_flipped {
                        self.emit_changed();
                    }
                    match snapshot {
                        Some(profile) => {
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
                let addr = ep.addr();
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
            friends: store.friends.clone(),
            pending_requests: store.pending_requests.clone(),
            local_collections,
        })
    }

    pub fn set_display_name(&self, name: String) -> Result<FriendsState> {
        let trimmed = name.trim().to_string();
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
            for vis in store.collection_visibility.values_mut() {
                if let CollectionVisibility::SpecificFriends { peer_ids } = vis {
                    peer_ids.retain(|id| id != &peer_id);
                }
            }
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
        let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
        let mut changed = false;
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
            }
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
            changed = !f.online
                || f.display_name != profile.display_name
                || f.profile_picture != profile.profile_picture
                || collections_changed;
            f.display_name = profile.display_name.clone();
            f.profile_picture = profile.profile_picture.clone();
            f.cached_profile = Some(profile);
            f.online = true;
            f.last_seen_at = Some(now_rfc3339());
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
                let mut hello_changed = false;
                {
                    let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                    if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
                        if !f.online || f.display_name != display_name {
                            hello_changed = true;
                        }
                        f.online = true;
                        f.last_seen_at = Some(now_rfc3339());
                        f.display_name = display_name;
                        f.addr = remote_addr;
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
                }),
            }],
            pending_requests: vec![],
            hidden_collection_ids: HashSet::new(),
            collection_visibility: HashMap::new(),
            appear_offline: false,
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
}
