//! Desktop-side `emperor-remote/1` host service.
//!
//! Accepts `iroh` QUIC connections routed from the shared `iroh::Endpoint` for
//! `REMOTE_ALPN`, handles LAN-only 6-digit code pairing, serves full snapshots &
//! encrypted-in-transit account secrets, pushes throttled live events, and
//! dispatches authenticated remote RPC mutations into desktop `AppState`.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use anyhow::{anyhow, Result};
use base64::Engine;
use iroh::endpoint::Connection;
use iroh::{Endpoint, EndpointAddr, TransportAddr};
use tokio::net::UdpSocket;
use tokio::sync::broadcast;

use crate::config::{Paths, APP_VERSION};

use super::lan_discovery::{
    local_host_name, parse_probe, primary_lan_ip, LanAnnounce, DISCOVERY_MAGIC,
    DISCOVERY_MULTICAST_V4, DISCOVERY_PORT,
};
use super::pairing::PairingManager;
use super::protocol::{
    is_local_network_ip, read_frame, write_frame, DesktopSnapshot, MobilePairingSession,
    PairedDevice, RemoteAccountSecrets, RemoteBanner, RemoteEvent, RemoteRequest, RemoteResponse,
};

const STREAM_IO_TIMEOUT: Duration = Duration::from_secs(30);
const PROGRESS_THROTTLE: Duration = Duration::from_millis(250);
const MAX_COVER_BYTES: usize = 512 * 1024;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Abstraction over desktop `AppState` so the `iroh` host protocol can be
/// unit- and integration-tested end-to-end without spawning a Tauri window.
pub trait RemoteBackend: Send + Sync + 'static {
    fn build_snapshot(&self, allow_remote_launch: bool, rev: u64) -> DesktopSnapshot;
    fn account_secrets(&self) -> RemoteAccountSecrets;
    fn allow_relay(&self) -> bool;
    fn cover_thumb(&self, game_id: &str) -> Result<(String, Vec<u8>)>;
    fn execute_request(
        &self,
        req: RemoteRequest,
        allow_remote_launch: bool,
    ) -> BoxFuture<'_, Result<Option<serde_json::Value>>>;
    fn on_paired(&self, _device: &PairedDevice) {}
    fn on_pairing_failed(&self, _remaining_attempts: u32, _message: &str) {}
}

pub struct RemoteHostService {
    pairing: Mutex<PairingManager>,
    connected_peers: Mutex<HashSet<String>>,
    rev: AtomicU64,
    events_tx: broadcast::Sender<RemoteEvent>,
    last_progress_emit: Mutex<HashMap<String, Instant>>,
    banners: Mutex<Vec<RemoteBanner>>,
    lan_responder_started: AtomicBool,
    backend: Mutex<Option<Arc<dyn RemoteBackend>>>,
    endpoint: Mutex<Option<Endpoint>>,
}

impl RemoteHostService {
    pub fn new(paths: &Paths) -> Self {
        let (events_tx, _) = broadcast::channel(128);
        Self {
            pairing: Mutex::new(PairingManager::load(paths.paired_devices_file())),
            connected_peers: Mutex::new(HashSet::new()),
            rev: AtomicU64::new(1),
            events_tx,
            last_progress_emit: Mutex::new(HashMap::new()),
            banners: Mutex::new(Vec::new()),
            lan_responder_started: AtomicBool::new(false),
            backend: Mutex::new(None),
            endpoint: Mutex::new(None),
        }
    }

    pub fn attach_backend(&self, backend: Arc<dyn RemoteBackend>) {
        if let Ok(mut guard) = self.backend.lock() {
            *guard = Some(backend);
        }
    }

    pub fn attach_endpoint(self: &Arc<Self>, ep: Endpoint) {
        if let Ok(mut guard) = self.endpoint.lock() {
            *guard = Some(ep);
        }
        if !self.lan_responder_started.swap(true, Ordering::SeqCst) {
            let svc = Arc::clone(self);
            tokio::spawn(async move {
                svc.run_lan_responder().await;
            });
        }
    }

    pub fn current_rev(&self) -> u64 {
        self.rev.load(Ordering::SeqCst)
    }

    pub fn bump_rev(&self) -> u64 {
        self.rev.fetch_add(1, Ordering::SeqCst) + 1
    }

    pub fn emit_event(&self, event: RemoteEvent) {
        let _ = self.events_tx.send(event);
    }

    /// Emit throttled download progress (~4 Hz per download id) to connected mobile clients.
    pub fn emit_download_progress_throttled(&self, download_id: &str, item: serde_json::Value) {
        if self.events_tx.receiver_count() == 0 {
            return;
        }
        let now = Instant::now();
        if let Ok(mut map) = self.last_progress_emit.lock() {
            if let Some(prev) = map.get(download_id) {
                if now.duration_since(*prev) < PROGRESS_THROTTLE {
                    return;
                }
            }
            map.insert(download_id.to_string(), now);
            if map.len() > 256 {
                map.retain(|_, t| now.duration_since(*t) < Duration::from_secs(60));
            }
        }
        let _ = self.events_tx.send(RemoteEvent::DownloadProgress { item });
    }

    pub fn notify_state_invalidated(&self) {
        let rev = self.bump_rev();
        let _ = self.events_tx.send(RemoteEvent::SnapshotInvalidated { rev });
    }

    pub fn set_desktop_banners(&self, banners: Vec<RemoteBanner>) {
        if let Ok(mut guard) = self.banners.lock() {
            if *guard == banners {
                return;
            }
            *guard = banners.clone();
        }
        let rev = self.bump_rev();
        let _ = self
            .events_tx
            .send(RemoteEvent::BannersChanged { rev, banners });
    }

    pub fn desktop_banners(&self) -> Vec<RemoteBanner> {
        self.banners
            .lock()
            .map(|b| b.clone())
            .unwrap_or_default()
    }

    pub fn dismiss_desktop_banner(&self, id: u64) {
        let next = if let Ok(mut guard) = self.banners.lock() {
            guard.retain(|b| b.id != id);
            guard.clone()
        } else {
            return;
        };
        let rev = self.bump_rev();
        let _ = self
            .events_tx
            .send(RemoteEvent::BannersChanged { rev, banners: next });
    }

    pub fn start_pairing(&self) -> Result<MobilePairingSession> {
        let ep = self
            .endpoint
            .lock()
            .map_err(|e| anyhow!("{e}"))?
            .clone()
            .ok_or_else(|| anyhow!("iroh endpoint is still starting — try again in a moment"))?;
        let addr = ep.addr();
        let desktop_id = addr.id.to_string();
        let host_name = local_host_name();
        let lan_addrs = collect_lan_socket_addrs(&addr)
            .into_iter()
            .map(|s| s.to_string())
            .collect();

        let mut pairing = self.pairing.lock().map_err(|e| anyhow!("{e}"))?;
        Ok(pairing.start_session(desktop_id, host_name, lan_addrs))
    }

    pub fn active_pairing_session(&self) -> Option<MobilePairingSession> {
        self.pairing.lock().ok()?.active_session()
    }

    pub fn cancel_pairing(&self) {
        if let Ok(mut pairing) = self.pairing.lock() {
            pairing.cancel_session();
        }
    }

    pub fn list_paired_devices(&self) -> Vec<PairedDevice> {
        let connected = self
            .connected_peers
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default();
        self.pairing
            .lock()
            .map(|p| {
                p.list_devices()
                    .into_iter()
                    .map(|mut d| {
                        d.connected = connected.contains(&d.peer_id);
                        d
                    })
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn revoke_paired_device(&self, peer_id: &str) -> Result<bool> {
        let removed = self
            .pairing
            .lock()
            .map_err(|e| anyhow!("{e}"))?
            .revoke_device(peer_id)?;
        if removed {
            let _ = self.events_tx.send(RemoteEvent::Revoked);
        }
        Ok(removed)
    }

    pub fn set_device_permissions(
        &self,
        peer_id: &str,
        allow_remote_launch: bool,
    ) -> Result<PairedDevice> {
        let mut device = self
            .pairing
            .lock()
            .map_err(|e| anyhow!("{e}"))?
            .set_device_permissions(peer_id, allow_remote_launch)?;
        let connected = self
            .connected_peers
            .lock()
            .map(|s| s.contains(peer_id))
            .unwrap_or(false);
        device.connected = connected;
        self.notify_state_invalidated();
        Ok(device)
    }

    async fn run_lan_responder(self: Arc<Self>) {
        let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, DISCOVERY_PORT);
        let Ok(sock) = UdpSocket::bind(bind_addr).await else {
            log::debug!("LAN discovery UDP port {DISCOVERY_PORT} busy; manual IP fallback remains available");
            return;
        };
        let _ = sock.join_multicast_v4(DISCOVERY_MULTICAST_V4, Ipv4Addr::UNSPECIFIED);

        let mut buf = vec![0u8; 2048];
        while let Ok((n, src)) = sock.recv_from(&mut buf).await {
            let Some(nonce) = parse_probe(&buf[..n]) else {
                continue;
            };
            let pairing_active = self
                .pairing
                .lock()
                .map(|mut p| p.is_pairing_active())
                .unwrap_or(false);
            let has_paired = self
                .pairing
                .lock()
                .map(|p| !p.list_devices().is_empty())
                .unwrap_or(false);
            if !pairing_active && !has_paired {
                continue;
            }
            let ep = match self.endpoint.lock().ok().and_then(|g| g.clone()) {
                Some(e) => e,
                None => continue,
            };
            let addr = ep.addr();
            let announce = LanAnnounce {
                magic: DISCOVERY_MAGIC.to_string(),
                nonce,
                host_name: local_host_name(),
                desktop_endpoint_id: addr.id.to_string(),
                lan_addrs: collect_lan_socket_addrs(&addr),
                pairing_active,
                app_version: APP_VERSION.to_string(),
            };
            if let Ok(bytes) = serde_json::to_vec(&announce) {
                let _ = sock.send_to(&bytes, src).await;
            }
        }
    }

    /// Handle an incoming `iroh` connection whose ALPN is `REMOTE_ALPN`.
    pub async fn handle_connection(self: Arc<Self>, conn: Connection) -> Result<()> {
        let remote_id = conn.remote_id().to_string();
        let is_initially_paired = self
            .pairing
            .lock()
            .map(|p| p.is_paired(&remote_id))
            .unwrap_or(false);

        if is_initially_paired {
            if let Ok(mut set) = self.connected_peers.lock() {
                set.insert(remote_id.clone());
            }
            if let Ok(mut p) = self.pairing.lock() {
                p.touch_last_seen(&remote_id);
            }
        }

        let result = self.serve_streams(&conn, &remote_id).await;

        if let Ok(mut set) = self.connected_peers.lock() {
            set.remove(&remote_id);
        }
        result
    }

    async fn serve_streams(self: &Arc<Self>, conn: &Connection, remote_id: &str) -> Result<()> {
        let ep = self
            .endpoint
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .ok_or_else(|| anyhow!("desktop endpoint not attached"))?;
        let desktop_id = ep.id().to_string();

        // Accept bi-directional QUIC streams concurrently until the connection closes.
        while let Ok((send, recv)) = conn.accept_bi().await {
            let svc = Arc::clone(self);
            let ep_clone = ep.clone();
            let desktop_id = desktop_id.clone();
            let remote_id = remote_id.to_string();
            tokio::spawn(async move {
                svc.handle_stream(send, recv, ep_clone, desktop_id, remote_id)
                    .await;
            });
        }
        Ok(())
    }

    async fn handle_stream(
        self: Arc<Self>,
        mut send: iroh::endpoint::SendStream,
        mut recv: iroh::endpoint::RecvStream,
        ep: Endpoint,
        desktop_id: String,
        remote_id: String,
    ) {
        let req: RemoteRequest =
            match tokio::time::timeout(STREAM_IO_TIMEOUT, read_frame(&mut recv)).await {
                Ok(Ok(r)) => r,
                _ => return,
            };

        match req {
            RemoteRequest::PairInit {
                device_name,
                device_model,
            } => {
                let resp = match self.pairing.lock() {
                    Ok(mut pairing) => {
                        match pairing.create_challenge(&remote_id, device_name, device_model) {
                            Ok((nonce, desktop_name)) => RemoteResponse::PairChallenge {
                                nonce,
                                desktop_name,
                            },
                            Err(e) => RemoteResponse::Error {
                                code: "pairing_inactive".into(),
                                message: e.to_string(),
                            },
                        }
                    }
                    Err(e) => RemoteResponse::Error {
                        code: "internal".into(),
                        message: e.to_string(),
                    },
                };
                let _ = write_frame(&mut send, &resp).await;
                let _ = send.finish();
            }

            RemoteRequest::PairVerify { nonce, proof_hex } => {
                let backend = self.backend.lock().ok().and_then(|g| g.clone());
                let resp = match self.pairing.lock() {
                    Ok(mut pairing) => {
                        match pairing.verify_challenge(&desktop_id, &remote_id, &nonce, &proof_hex)
                        {
                            Ok(device) => {
                                if let Ok(mut set) = self.connected_peers.lock() {
                                    set.insert(remote_id.clone());
                                }
                                if let Some(b) = backend.as_ref() {
                                    b.on_paired(&device);
                                }
                                RemoteResponse::PairAccepted {
                                    desktop_name: local_host_name(),
                                    desktop_addr: ep.addr(),
                                    allow_remote_launch: device.allow_remote_launch,
                                }
                            }
                            Err(e) => {
                                let remaining = pairing
                                    .active_session()
                                    .map(|s| s.remaining_attempts)
                                    .unwrap_or(0);
                                if let Some(b) = backend.as_ref() {
                                    b.on_pairing_failed(remaining, &e.to_string());
                                }
                                RemoteResponse::Error {
                                    code: "pairing_failed".into(),
                                    message: e.to_string(),
                                }
                            }
                        }
                    }
                    Err(e) => RemoteResponse::Error {
                        code: "internal".into(),
                        message: e.to_string(),
                    },
                };
                let _ = write_frame(&mut send, &resp).await;
                let _ = send.finish();
            }

            RemoteRequest::SubscribeEvents => {
                if !self.is_authorized(&remote_id) {
                    let _ = write_frame(
                        &mut send,
                        &RemoteResponse::Error {
                            code: "unauthorized".into(),
                            message: "Device is not paired".into(),
                        },
                    )
                    .await;
                    let _ = send.finish();
                    return;
                }
                let rev = self.current_rev();
                if write_frame(&mut send, &RemoteResponse::Subscribed { rev })
                    .await
                    .is_err()
                {
                    return;
                }
                let mut rx = self.events_tx.subscribe();
                while let Ok(evt) = rx.recv().await {
                    let revoked = !self.is_authorized(&remote_id);
                    let out = if revoked { RemoteEvent::Revoked } else { evt };
                    if write_frame(&mut send, &out).await.is_err() {
                        break;
                    }
                    if revoked {
                        let _ = send.finish();
                        break;
                    }
                }
            }

            other => {
                let device = self
                    .pairing
                    .lock()
                    .ok()
                    .and_then(|p| p.get_device(&remote_id).cloned());
                let Some(device) = device else {
                    let _ = write_frame(
                        &mut send,
                        &RemoteResponse::Error {
                            code: "unauthorized".into(),
                            message: "Device is not paired".into(),
                        },
                    )
                    .await;
                    let _ = send.finish();
                    return;
                };

                let backend = self.backend.lock().ok().and_then(|g| g.clone());
                let Some(backend) = backend else {
                    let _ = write_frame(
                        &mut send,
                        &RemoteResponse::Error {
                            code: "not_ready".into(),
                            message: "Desktop backend not initialized".into(),
                        },
                    )
                    .await;
                    let _ = send.finish();
                    return;
                };

                let resp = match other {
                    RemoteRequest::Ping => RemoteResponse::Pong {
                        rev: self.current_rev(),
                    },
                    RemoteRequest::GetSyncSnapshot { since_rev } => {
                        let rev = self.current_rev();
                        if since_rev == Some(rev) {
                            RemoteResponse::SnapshotUnchanged { rev }
                        } else {
                            let b = Arc::clone(&backend);
                            let allow = device.allow_remote_launch;
                            match tokio::task::spawn_blocking(move || b.build_snapshot(allow, rev))
                                .await
                            {
                                Ok(snap) => RemoteResponse::Snapshot(snap),
                                Err(e) => RemoteResponse::Error {
                                    code: "snapshot_failed".into(),
                                    message: e.to_string(),
                                },
                            }
                        }
                    }
                    RemoteRequest::GetAccountSecrets => {
                        RemoteResponse::AccountSecrets(backend.account_secrets())
                    }
                    RemoteRequest::GetCoverThumb { game_id } => {
                        let b = Arc::clone(&backend);
                        match tokio::task::spawn_blocking(move || b.cover_thumb(&game_id)).await {
                            Ok(Ok((mime, bytes))) if bytes.len() <= MAX_COVER_BYTES => {
                                RemoteResponse::CoverThumb {
                                    mime,
                                    data_b64: base64::engine::general_purpose::STANDARD
                                        .encode(bytes),
                                }
                            }
                            Ok(Ok(_)) => RemoteResponse::Error {
                                code: "cover_too_large".into(),
                                message: "Cover image exceeds thumbnail size limit".into(),
                            },
                            Ok(Err(e)) => RemoteResponse::Error {
                                code: "cover_missing".into(),
                                message: e.to_string(),
                            },
                            Err(e) => RemoteResponse::Error {
                                code: "cover_failed".into(),
                                message: e.to_string(),
                            },
                        }
                    }
                    req => {
                        let is_read_only = matches!(
                            req,
                            RemoteRequest::CatalogRpc { .. }
                                | RemoteRequest::CheckStagedModUpdates { .. }
                                | RemoteRequest::GetFomodInstaller { .. }
                                | RemoteRequest::GetFomodImage { .. }
                                | RemoteRequest::GetSavedCollectionDetail { .. }
                                | RemoteRequest::CheckCollectionIntegrity { .. }
                        );
                        match backend
                            .execute_request(req, device.allow_remote_launch)
                            .await
                        {
                            Ok(data) => {
                                let rev = if is_read_only {
                                    self.current_rev()
                                } else {
                                    let next = self.bump_rev();
                                    let _ = self
                                        .events_tx
                                        .send(RemoteEvent::SnapshotInvalidated { rev: next });
                                    next
                                };
                                RemoteResponse::Ok { data, rev }
                            }
                            Err(e) => RemoteResponse::Error {
                                code: "rpc_failed".into(),
                                message: e.to_string(),
                            },
                        }
                    }
                };

                let _ = write_frame(&mut send, &resp).await;
                let _ = send.finish();
            }
        }
    }

    fn is_authorized(&self, peer_id: &str) -> bool {
        self.pairing
            .lock()
            .map(|p| p.is_paired(peer_id))
            .unwrap_or(false)
    }
}

/// Extract local-network socket addresses from an `EndpointAddr`, supplementing
/// with `primary_lan_ip()` when the endpoint only bound wildcard/loopback sockets.
pub fn collect_lan_socket_addrs(addr: &EndpointAddr) -> Vec<SocketAddr> {
    let mut addrs: Vec<SocketAddr> = addr
        .addrs
        .iter()
        .filter_map(|ta| match ta {
            TransportAddr::Ip(sock) if is_local_network_ip(sock.ip()) && !sock.ip().is_loopback() => {
                Some(*sock)
            }
            _ => None,
        })
        .collect();

    if addrs.is_empty() {
        if let Some(ip) = primary_lan_ip() {
            if let Some(port) = addr.addrs.iter().find_map(|ta| match ta {
                TransportAddr::Ip(sock) => Some(sock.port()),
                _ => None,
            }) {
                addrs.push(SocketAddr::new(ip, port));
            }
        }
    }
    addrs.sort();
    addrs.dedup();
    addrs
}
