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
    pub addr: EndpointAddr,
    pub received_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FriendsStore {
    #[serde(default = "default_display_name")]
    pub display_name: String,
    #[serde(default)]
    pub friends: Vec<FriendRecord>,
    #[serde(default)]
    pub pending_requests: Vec<PendingFriendRequest>,
    #[serde(default)]
    pub hidden_collection_ids: HashSet<String>,
}

fn default_display_name() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Emperor User".to_string())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalShareableCollection {
    pub id: String,
    pub kind: FriendCollectionKind,
    pub name: String,
    pub game_title: String,
    pub mod_count: usize,
    pub hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendsState {
    pub endpoint_ready: bool,
    pub peer_id: Option<String>,
    pub join_code: Option<String>,
    pub display_name: String,
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

pub fn list_local_shareable_collections(
    paths: &Paths,
    hidden_ids: &HashSet<String>,
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
                    out.push(LocalShareableCollection {
                        hidden: hidden_ids.contains(&id),
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
            out.push(LocalShareableCollection {
                hidden: hidden_ids.contains(&id),
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
) -> PeerProfileSummary {
    let mut collections = Vec::new();
    let now = now_rfc3339();

    if let Ok(cfg) = load_config(paths) {
        for game in &cfg.managed_games {
            let col_id = format!("active:{}", game.id);
            if store.hidden_collection_ids.contains(&col_id) {
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
            if store.hidden_collection_ids.contains(&col_id) {
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
        display_name: store.display_name.clone(),
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
        self.emit_changed();

        while let Some(incoming) = ep.accept().await {
            let svc = Arc::clone(&self);
            tauri::async_runtime::spawn(async move {
                if let Err(e) = svc.handle_incoming(incoming).await {
                    log::debug!("P2P incoming stream error: {e:#}");
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

    async fn handle_incoming(&self, incoming: Incoming) -> Result<()> {
        let conn = incoming.await?;
        let remote_id = conn.remote_id().to_string();
        let (mut send, mut recv) = conn.accept_bi().await?;
        let req: P2pRequest = read_frame(&mut recv).await?;

        let Some(ep) = self.endpoint.get() else {
            write_frame(&mut send, &P2pResponse::Denied("Endpoint not ready".into())).await?;
            return Ok(());
        };
        let local_addr = ep.addr();

        match req {
            P2pRequest::Hello { display_name, addr } => {
                let (accepted, local_name) = {
                    let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                    let local_name = store.display_name.clone();
                    if let Some(friend) = store.friends.iter_mut().find(|f| f.peer_id == remote_id)
                    {
                        friend.status = FriendStatus::Connected;
                        friend.display_name = display_name;
                        friend.addr = addr;
                        friend.online = true;
                        friend.last_seen_at = Some(now_rfc3339());
                        (true, local_name)
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
                                peer_id: remote_id,
                                display_name,
                                addr,
                                received_at: now_rfc3339(),
                            });
                        }
                        (false, local_name)
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
            }
            P2pRequest::GetProfile { known_hashes } => {
                let snapshot = {
                    let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                    let is_friend = store
                        .friends
                        .iter()
                        .any(|f| f.peer_id == remote_id && f.status == FriendStatus::Connected);
                    if !is_friend {
                        None
                    } else {
                        if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == remote_id) {
                            f.online = true;
                            f.last_seen_at = Some(now_rfc3339());
                        }
                        let local_id = ep.id().to_string();
                        Some(build_local_profile(
                            &self.paths,
                            &local_id,
                            &store,
                            Some(&known_hashes),
                        ))
                    }
                };
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
                    name: store.display_name.clone(),
                };
                let code = encode_join_code(&ticket).ok();
                (true, Some(pid), code)
            }
            None => (false, None, None),
        };
        let local_collections =
            list_local_shareable_collections(&self.paths, &store.hidden_collection_ids);
        Ok(FriendsState {
            endpoint_ready,
            peer_id,
            join_code,
            display_name: store.display_name.clone(),
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
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub fn set_collection_visibility(&self, collection_id: String, hidden: bool) -> Result<FriendsState> {
        {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            if hidden {
                store.hidden_collection_ids.insert(collection_id);
            } else {
                store.hidden_collection_ids.remove(&collection_id);
            }
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
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
            save_friends_store(&self.paths, &store)?;
        }
        self.emit_changed();
        self.get_state()
    }

    pub async fn refresh_all_friends(&self) -> Result<FriendsState> {
        let peer_ids: Vec<String> = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            store.friends.iter().map(|f| f.peer_id.clone()).collect()
        };
        for pid in peer_ids {
            let _ = self.dial_and_sync_peer(&pid).await;
        }
        self.get_state()
    }

    async fn dial_and_sync_peer(&self, peer_id: &str) -> Result<()> {
        let Some(ep) = self.endpoint.get() else {
            return Ok(());
        };
        let (addr, local_name, local_addr, known_hashes) = {
            let store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
            let Some(friend) = store.friends.iter().find(|f| f.peer_id == peer_id) else {
                return Ok(());
            };
            let mut hashes = HashMap::new();
            if let Some(profile) = &friend.cached_profile {
                for c in &profile.collections {
                    hashes.insert(c.id.clone(), c.share_hash.clone());
                }
            }
            (
                friend.addr.clone(),
                store.display_name.clone(),
                ep.addr(),
                hashes,
            )
        };

        // 1. Send Hello so mutual state & addresses stay fresh.
        let hello_res = tokio::time::timeout(DIAL_TIMEOUT, async {
            let conn = ep.connect(addr.clone(), P2P_ALPN).await?;
            let (mut send, mut recv) = conn.open_bi().await?;
            write_frame(
                &mut send,
                &P2pRequest::Hello {
                    display_name: local_name,
                    addr: local_addr,
                },
            )
            .await?;
            let resp: P2pResponse = read_frame(&mut recv).await?;
            Ok::<_, anyhow::Error>(resp)
        })
        .await;

        let accepted = match hello_res {
            Ok(Ok(P2pResponse::HelloAck {
                accepted,
                display_name,
                addr: remote_addr,
            })) => {
                let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
                    f.online = true;
                    f.last_seen_at = Some(now_rfc3339());
                    f.display_name = display_name;
                    f.addr = remote_addr;
                    if accepted {
                        f.status = FriendStatus::Connected;
                    }
                }
                let _ = save_friends_store(&self.paths, &store);
                accepted
            }
            _ => {
                let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
                if let Some(f) = store.friends.iter_mut().find(|f| f.peer_id == peer_id) {
                    f.online = false;
                }
                let _ = save_friends_store(&self.paths, &store);
                self.emit_changed();
                return Ok(());
            }
        };

        if !accepted {
            self.emit_changed();
            return Ok(());
        }

        // 2. Fetch live share codes (with hash delta optimization).
        let profile_res = tokio::time::timeout(DIAL_TIMEOUT, async {
            let conn = ep.connect(addr, P2P_ALPN).await?;
            let (mut send, mut recv) = conn.open_bi().await?;
            write_frame(&mut send, &P2pRequest::GetProfile { known_hashes }).await?;
            let resp: P2pResponse = read_frame(&mut recv).await?;
            Ok::<_, anyhow::Error>(resp)
        })
        .await;

        if let Ok(Ok(P2pResponse::Profile(mut profile))) = profile_res {
            let mut store = self.store.lock().map_err(|e| anyhow!("{e}"))?;
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
                f.display_name = profile.display_name.clone();
                f.cached_profile = Some(profile);
                f.online = true;
                f.last_seen_at = Some(now_rfc3339());
            }
            let _ = save_friends_store(&self.paths, &store);
        }

        self.emit_changed();
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
            friends: vec![FriendRecord {
                peer_id: pid.clone(),
                display_name: "Alice".into(),
                addr: EndpointAddr::new(key.public()),
                status: FriendStatus::Connected,
                online: true,
                last_seen_at: Some("2026-11-20T00:00:00Z".into()),
                cached_profile: Some(PeerProfileSummary {
                    peer_id: pid.clone(),
                    display_name: "Alice".into(),
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
}
