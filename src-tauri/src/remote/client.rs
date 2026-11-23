#![allow(dead_code)]
//! Mobile-side `iroh` remote client, offline snapshot cache, and encrypted API key store.

use std::fs;
use std::net::SocketAddr;
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use iroh::endpoint::{presets, Connection};
use iroh::{Endpoint, EndpointAddr, EndpointId, SecretKey, TransportAddr};
use serde::{Deserialize, Serialize};
use tokio::sync::OnceCell;
use zeroize::Zeroizing;

use crate::config::{secret_store, Paths};

use super::protocol::{
    compute_pairing_proof, read_frame, write_frame, DesktopSnapshot, MobileConnectionMode,
    MobileConnectionState, RemoteAccountSecrets, RemoteEvent, RemoteRequest, RemoteResponse,
    REMOTE_ALPN,
};

const DIAL_LAN_TIMEOUT: Duration = Duration::from_secs(4);
const DIAL_RELAY_TIMEOUT: Duration = Duration::from_secs(10);
const RPC_TIMEOUT: Duration = Duration::from_secs(20);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PairedHostRecord {
    pub desktop_peer_id: String,
    pub desktop_name: String,
    pub desktop_addr: EndpointAddr,
    pub allow_remote_launch: bool,
    pub paired_at: String,
    pub last_synced_at: Option<String>,
}

pub struct RemoteClientService {
    paths: Paths,
    endpoint: OnceCell<Endpoint>,
    paired_host: Mutex<Option<PairedHostRecord>>,
    cache: Mutex<Option<DesktopSnapshot>>,
    conn_state: Mutex<MobileConnectionState>,
    active_conn: tokio::sync::Mutex<Option<Connection>>,
}

impl RemoteClientService {
    pub fn new(paths: Paths) -> Self {
        let paired_host = fs::read_to_string(paths.paired_host_file())
            .ok()
            .and_then(|s| serde_json::from_str::<PairedHostRecord>(&s).ok());
        let cache = fs::read_to_string(paths.remote_cache_file())
            .ok()
            .and_then(|s| serde_json::from_str::<DesktopSnapshot>(&s).ok());

        let conn_state = match &paired_host {
            Some(host) => MobileConnectionState {
                paired: true,
                desktop_name: Some(host.desktop_name.clone()),
                desktop_peer_id: Some(host.desktop_peer_id.clone()),
                mode: MobileConnectionMode::Disconnected,
                last_synced_at: host.last_synced_at.clone(),
                last_error: None,
                allow_remote_launch: host.allow_remote_launch,
            },
            None => MobileConnectionState::default(),
        };

        Self {
            paths,
            endpoint: OnceCell::new(),
            paired_host: Mutex::new(paired_host),
            cache: Mutex::new(cache),
            conn_state: Mutex::new(conn_state),
            active_conn: tokio::sync::Mutex::new(None),
        }
    }

    pub async fn ensure_endpoint(&self) -> Result<Endpoint> {
        let ep = self
            .endpoint
            .get_or_try_init(|| async {
                let key = load_or_create_mobile_secret_key(&self.paths)?;
                Endpoint::builder(presets::N0)
                    .secret_key(key)
                    .bind()
                    .await
                    .context("bind mobile iroh endpoint")
            })
            .await?;
        Ok(ep.clone())
    }

    #[cfg(test)]
    pub async fn init_test_endpoint(&self) -> Result<Endpoint> {
        let ep = self
            .endpoint
            .get_or_try_init(|| async {
                let key = load_or_create_mobile_secret_key(&self.paths)?;
                Endpoint::builder(presets::N0DisableRelay)
                    .secret_key(key)
                    .bind()
                    .await
                    .context("bind mobile test iroh endpoint")
            })
            .await?;
        Ok(ep.clone())
    }

    pub fn connection_state(&self) -> MobileConnectionState {
        self.conn_state
            .lock()
            .map(|s| s.clone())
            .unwrap_or_default()
    }

    pub fn cached_snapshot(&self) -> Option<DesktopSnapshot> {
        self.cache.lock().ok().and_then(|c| c.clone())
    }

    pub fn load_synced_nexus_api_key(&self) -> Result<Option<String>> {
        secret_store::load_encrypted_secret(&self.paths.mobile_nexus_key_file())
    }

    pub fn load_synced_modio_api_key(&self) -> Result<Option<String>> {
        secret_store::load_encrypted_secret(&self.paths.mobile_modio_key_file())
    }

    /// Pair with a desktop on the local network using the 6-digit code shown in the desktop modal.
    pub async fn pair_with_desktop(
        &self,
        desktop_endpoint_id: &str,
        lan_addrs: Vec<SocketAddr>,
        code: &str,
        device_name: String,
        device_model: String,
    ) -> Result<MobileConnectionState> {
        let ep = self.ensure_endpoint().await?;
        let target_id =
            EndpointId::from_str(desktop_endpoint_id.trim()).context("invalid desktop endpoint id")?;
        let mut target_addr = EndpointAddr::new(target_id);
        for sock in lan_addrs {
            target_addr.addrs.insert(TransportAddr::Ip(sock));
        }

        let conn = tokio::time::timeout(DIAL_LAN_TIMEOUT, ep.connect(target_addr, REMOTE_ALPN))
            .await
            .map_err(|_| anyhow!("Timed out connecting to desktop on local network"))??;

        let mobile_id = ep.id().to_string();

        // Step 1: PairInit -> PairChallenge
        let (nonce, _desktop_name) = match rpc_on_conn(
            &conn,
            &RemoteRequest::PairInit {
                device_name,
                device_model,
            },
        )
        .await?
        {
            RemoteResponse::PairChallenge {
                nonce,
                desktop_name,
            } => (nonce, desktop_name),
            RemoteResponse::Error { message, .. } => bail!("{message}"),
            other => bail!("unexpected pairing response: {other:?}"),
        };

        // Step 2: Compute HMAC-SHA256 proof bound to nonce + both Ed25519 EndpointIds
        let proof_hex = compute_pairing_proof(code, &nonce, desktop_endpoint_id, &mobile_id);
        let (desktop_name, desktop_addr, allow_remote_launch) = match rpc_on_conn(
            &conn,
            &RemoteRequest::PairVerify { nonce, proof_hex },
        )
        .await?
        {
            RemoteResponse::PairAccepted {
                desktop_name,
                desktop_addr,
                allow_remote_launch,
            } => (desktop_name, desktop_addr, allow_remote_launch),
            RemoteResponse::Error { message, .. } => bail!("{message}"),
            other => bail!("unexpected verify response: {other:?}"),
        };

        // Persist paired host record
        let record = PairedHostRecord {
            desktop_peer_id: desktop_endpoint_id.to_string(),
            desktop_name: desktop_name.clone(),
            desktop_addr,
            allow_remote_launch,
            paired_at: Utc::now().to_rfc3339(),
            last_synced_at: None,
        };
        self.save_paired_host(&record)?;

        {
            let mut active = self.active_conn.lock().await;
            *active = Some(conn);
        }
        self.update_conn_state(|s| {
            s.paired = true;
            s.desktop_name = Some(desktop_name);
            s.desktop_peer_id = Some(desktop_endpoint_id.to_string());
            s.mode = MobileConnectionMode::LanDirect;
            s.allow_remote_launch = allow_remote_launch;
            s.last_error = None;
        });

        // Immediately sync snapshot & securely store API keys
        let _ = self.sync_now().await;
        Ok(self.connection_state())
    }

    /// Connect (LAN direct first, then iroh relay fallback) if not already connected.
    pub async fn connect_or_get(&self) -> Result<Connection> {
        {
            let guard = self.active_conn.lock().await;
            if let Some(conn) = guard.as_ref() {
                if conn.close_reason().is_none() {
                    return Ok(conn.clone());
                }
            }
        }

        let host = self
            .paired_host
            .lock()
            .map_err(|e| anyhow!("{e}"))?
            .clone()
            .ok_or_else(|| anyhow!("No desktop paired"))?;

        self.update_conn_state(|s| {
            s.mode = MobileConnectionMode::Connecting;
            s.last_error = None;
        });

        let ep = self.ensure_endpoint().await?;

        // Try direct LAN / full EndpointAddr first, then relay-only fallback.
        let (conn, mode) = match tokio::time::timeout(
            DIAL_LAN_TIMEOUT,
            ep.connect(host.desktop_addr.clone(), REMOTE_ALPN),
        )
        .await
        {
            Ok(Ok(c)) => (c, MobileConnectionMode::LanDirect),
            _ => {
                let target_id = EndpointId::from_str(&host.desktop_peer_id)?;
                let fallback_addr = EndpointAddr::new(target_id);
                match tokio::time::timeout(
                    DIAL_RELAY_TIMEOUT,
                    ep.connect(fallback_addr, REMOTE_ALPN),
                )
                .await
                {
                    Ok(Ok(c)) => (c, MobileConnectionMode::IrohRelay),
                    Ok(Err(e)) => {
                        self.update_conn_state(|s| {
                            s.mode = MobileConnectionMode::Disconnected;
                            s.last_error = Some(e.to_string());
                        });
                        return Err(e.into());
                    }
                    Err(_) => {
                        self.update_conn_state(|s| {
                            s.mode = MobileConnectionMode::Disconnected;
                            s.last_error = Some("Desktop is offline or unreachable".into());
                        });
                        bail!("Desktop is offline or unreachable");
                    }
                }
            }
        };

        {
            let mut active = self.active_conn.lock().await;
            *active = Some(conn.clone());
        }
        self.update_conn_state(|s| {
            s.mode = mode;
            s.last_error = None;
        });
        Ok(conn)
    }

    /// Perform a snapshot + encrypted API key sync from the paired desktop.
    pub async fn sync_now(&self) -> Result<DesktopSnapshot> {
        let conn = self.connect_or_get().await?;
        let since_rev = self.cache.lock().ok().and_then(|c| c.as_ref().map(|s| s.rev));

        let snap = match rpc_on_conn(&conn, &RemoteRequest::GetSyncSnapshot { since_rev }).await? {
            RemoteResponse::Snapshot(s) => {
                self.save_snapshot(&s)?;
                s
            }
            RemoteResponse::SnapshotUnchanged { .. } => self
                .cached_snapshot()
                .ok_or_else(|| anyhow!("cache missing after unchanged response"))?,
            RemoteResponse::Error { code, message } => {
                if code == "unauthorized" {
                    let _ = self.unpair().await;
                }
                bail!("{message}");
            }
            other => bail!("unexpected sync response: {other:?}"),
        };

        // Sync API keys over the same authenticated QUIC connection and store encrypted at rest.
        if let Ok(RemoteResponse::AccountSecrets(secrets)) =
            rpc_on_conn(&conn, &RemoteRequest::GetAccountSecrets).await
        {
            self.persist_account_secrets(&secrets)?;
        }

        let now = Utc::now().to_rfc3339();
        if let Ok(mut guard) = self.paired_host.lock() {
            if let Some(host) = guard.as_mut() {
                host.last_synced_at = Some(now.clone());
                host.allow_remote_launch = snap.allow_remote_launch;
                let _ = fs::write(
                    self.paths.paired_host_file(),
                    serde_json::to_string_pretty(host).unwrap_or_default(),
                );
            }
        }
        self.update_conn_state(|s| {
            s.last_synced_at = Some(now);
            s.allow_remote_launch = snap.allow_remote_launch;
        });
        Ok(snap)
    }

    /// Send an authenticated RPC request to the paired desktop over `iroh`.
    pub async fn send_request(&self, req: &RemoteRequest) -> Result<RemoteResponse> {
        let conn = self.connect_or_get().await?;
        let resp = rpc_on_conn(&conn, req).await?;
        if let RemoteResponse::Error { code, .. } = &resp {
            if code == "unauthorized" {
                let _ = self.unpair().await;
            }
        }
        Ok(resp)
    }

    /// Open a live event subscription stream from the paired desktop.
    pub async fn subscribe_events<F>(self: &Arc<Self>, mut on_event: F) -> Result<()>
    where
        F: FnMut(RemoteEvent) + Send + 'static,
    {
        let conn = self.connect_or_get().await?;
        let (mut send, mut recv) = conn.open_bi().await?;
        write_frame(&mut send, &RemoteRequest::SubscribeEvents).await?;
        let _ = send.finish();

        match read_frame::<RemoteResponse>(&mut recv).await? {
            RemoteResponse::Subscribed { .. } => {}
            RemoteResponse::Error { message, .. } => bail!("{message}"),
            other => bail!("unexpected subscribe response: {other:?}"),
        }

        while let Ok(evt) = read_frame::<RemoteEvent>(&mut recv).await {
            let is_revoked = matches!(evt, RemoteEvent::Revoked);
            on_event(evt);
            if is_revoked {
                let _ = self.unpair().await;
                break;
            }
        }
        Ok(())
    }

    /// Unpair from desktop and immediately wipe cached state & encrypted API keys.
    pub async fn unpair(&self) -> Result<()> {
        {
            let mut active = self.active_conn.lock().await;
            if let Some(conn) = active.take() {
                conn.close(0u32.into(), b"unpaired");
            }
        }
        if let Ok(mut guard) = self.paired_host.lock() {
            *guard = None;
        }
        if let Ok(mut guard) = self.cache.lock() {
            *guard = None;
        }
        let _ = fs::remove_file(self.paths.paired_host_file());
        let _ = fs::remove_file(self.paths.remote_cache_file());
        let _ = fs::remove_file(self.paths.mobile_nexus_key_file());
        let _ = fs::remove_file(self.paths.mobile_modio_key_file());
        self.update_conn_state(|s| {
            *s = MobileConnectionState::default();
        });
        Ok(())
    }

    fn persist_account_secrets(&self, secrets: &RemoteAccountSecrets) -> Result<()> {
        if let Some(key) = secrets.nexus_api_key.as_deref() {
            if !key.trim().is_empty() {
                secret_store::store_encrypted_secret(&self.paths.mobile_nexus_key_file(), key)?;
            }
        } else {
            let _ = fs::remove_file(self.paths.mobile_nexus_key_file());
        }
        if let Some(key) = secrets.modio_api_key.as_deref() {
            if !key.trim().is_empty() {
                secret_store::store_encrypted_secret(&self.paths.mobile_modio_key_file(), key)?;
            }
        } else {
            let _ = fs::remove_file(self.paths.mobile_modio_key_file());
        }
        Ok(())
    }

    fn save_paired_host(&self, record: &PairedHostRecord) -> Result<()> {
        fs::create_dir_all(&self.paths.config_dir)?;
        fs::write(
            self.paths.paired_host_file(),
            serde_json::to_string_pretty(record)?,
        )?;
        if let Ok(mut guard) = self.paired_host.lock() {
            *guard = Some(record.clone());
        }
        Ok(())
    }

    fn save_snapshot(&self, snap: &DesktopSnapshot) -> Result<()> {
        fs::create_dir_all(&self.paths.data_dir)?;
        fs::write(
            self.paths.remote_cache_file(),
            serde_json::to_string(snap)?,
        )?;
        if let Ok(mut guard) = self.cache.lock() {
            *guard = Some(snap.clone());
        }
        Ok(())
    }

    fn update_conn_state(&self, f: impl FnOnce(&mut MobileConnectionState)) {
        if let Ok(mut guard) = self.conn_state.lock() {
            f(&mut guard);
        }
    }
}

async fn rpc_on_conn(conn: &Connection, req: &RemoteRequest) -> Result<RemoteResponse> {
    let (mut send, mut recv) = tokio::time::timeout(RPC_TIMEOUT, conn.open_bi())
        .await
        .map_err(|_| anyhow!("Timed out opening iroh stream"))??;
    tokio::time::timeout(RPC_TIMEOUT, write_frame(&mut send, req))
        .await
        .map_err(|_| anyhow!("Timed out sending remote request"))??;
    let _ = send.finish();
    tokio::time::timeout(RPC_TIMEOUT, read_frame(&mut recv))
        .await
        .map_err(|_| anyhow!("Timed out waiting for desktop response"))?
}

fn load_or_create_mobile_secret_key(paths: &Paths) -> Result<SecretKey> {
    let key_file = paths.mobile_secret_key_file();
    if let Ok(Some(hex_key)) = secret_store::load_encrypted_secret(&key_file) {
        let bytes = Zeroizing::new(hex::decode(hex_key.trim()).context("decode mobile iroh key")?);
        if bytes.len() == 32 {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            return Ok(SecretKey::from_bytes(&arr));
        }
    }
    let mut raw = [0u8; 32];
    rand::Rng::fill_bytes(&mut rand::rng(), &mut raw);
    let key = SecretKey::from_bytes(&raw);
    let encoded = Zeroizing::new(hex::encode(raw));
    secret_store::store_encrypted_secret(&key_file, &encoded)?;
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::remote::host::{BoxFuture, RemoteBackend, RemoteHostService};
    use crate::remote::protocol::{RemoteAccountSummary, RemoteDownloadRequest};

    struct MockDesktopBackend {
        mod_enabled: Mutex<bool>,
        queued_downloads: Mutex<Vec<String>>,
    }

    impl RemoteBackend for MockDesktopBackend {
        fn build_snapshot(&self, allow_remote_launch: bool, rev: u64) -> DesktopSnapshot {
            DesktopSnapshot {
                rev,
                synced_at: Utc::now().to_rfc3339(),
                desktop_name: "Test PC".into(),
                app_version: "0.9.2".into(),
                allow_remote_launch,
                account: RemoteAccountSummary {
                    nexus_connected: true,
                    modio_connected: true,
                    p2p_display_name: "EmperorUser".into(),
                    ..Default::default()
                },
                active_game_id: Some("skyrim".into()),
                managed_games: vec![serde_json::json!({ "id": "skyrim", "title": "Skyrim SE" })],
                ..Default::default()
            }
        }

        fn account_secrets(&self) -> RemoteAccountSecrets {
            RemoteAccountSecrets {
                nexus_api_key: Some("nexus-secret-test-key".into()),
                modio_api_key: Some("modio-secret-test-key".into()),
            }
        }

        fn allow_relay(&self) -> bool {
            true
        }

        fn cover_thumb(&self, _game_id: &str) -> Result<(String, Vec<u8>)> {
            Ok(("image/png".into(), vec![0x89, 0x50, 0x4e, 0x47]))
        }

        fn execute_request(
            &self,
            req: RemoteRequest,
            _allow_remote_launch: bool,
        ) -> BoxFuture<'_, Result<Option<serde_json::Value>>> {
            Box::pin(async move {
                match req {
                    RemoteRequest::SetModEnabled { enabled, .. } => {
                        *self.mod_enabled.lock().unwrap() = enabled;
                        Ok(Some(serde_json::json!({ "enabled": enabled })))
                    }
                    RemoteRequest::QueueDownload(RemoteDownloadRequest::ThunderstoreMod {
                        name,
                        ..
                    }) => {
                        self.queued_downloads.lock().unwrap().push(name);
                        Ok(Some(serde_json::json!({ "queued": true })))
                    }
                    _ => Ok(None),
                }
            })
        }
    }

    #[tokio::test]
    async fn end_to_end_iroh_pairing_sync_secrets_and_remote_mutations() {
        let desk_dir = tempfile::tempdir().unwrap();
        let desk_paths = Paths::from_dirs(
            desk_dir.path().join("cfg"),
            desk_dir.path().join("data"),
            desk_dir.path().join("cache"),
        )
        .unwrap();
        let mob_dir = tempfile::tempdir().unwrap();
        let mob_paths = Paths::from_dirs(
            mob_dir.path().join("cfg"),
            mob_dir.path().join("data"),
            mob_dir.path().join("cache"),
        )
        .unwrap();

        let desk_ep = Endpoint::builder(presets::N0DisableRelay)
            .alpns(vec![REMOTE_ALPN.to_vec()])
            .bind()
            .await
            .unwrap();
        let desk_addr = desk_ep.addr();
        let desk_id = desk_addr.id.to_string();
        let desk_lan_addrs: Vec<SocketAddr> = desk_addr
            .addrs
            .iter()
            .filter_map(|ta| match ta {
                TransportAddr::Ip(s) => Some(*s),
                _ => None,
            })
            .collect();

        let backend = Arc::new(MockDesktopBackend {
            mod_enabled: Mutex::new(false),
            queued_downloads: Mutex::new(Vec::new()),
        });
        let host = Arc::new(RemoteHostService::new(&desk_paths));
        host.attach_backend(backend.clone());
        host.attach_endpoint(desk_ep.clone());

        // Accept loop for the test desktop endpoint
        let host_accept = Arc::clone(&host);
        let ep_accept = desk_ep.clone();
        let server_task = tokio::spawn(async move {
            while let Some(incoming) = ep_accept.accept().await {
                let h = Arc::clone(&host_accept);
                tokio::spawn(async move {
                    if let Ok(conn) = incoming.await {
                        let _ = h.handle_connection(conn).await;
                    }
                });
            }
        });

        let client = Arc::new(RemoteClientService::new(mob_paths));
        client.init_test_endpoint().await.unwrap();

        // Start desktop pairing modal session
        let session = host.start_pairing().unwrap();

        // Wrong code must fail
        let bad = client
            .pair_with_desktop(
                &desk_id,
                desk_lan_addrs.clone(),
                "000-000",
                "Pixel Fold".into(),
                "Pixel Fold".into(),
            )
            .await;
        if session.code != "000-000" {
            assert!(bad.is_err());
        }

        // Correct code succeeds, syncs snapshot, and securely persists API keys!
        let state = client
            .pair_with_desktop(
                &desk_id,
                desk_lan_addrs,
                &session.code,
                "Pixel Fold".into(),
                "Google Pixel Fold".into(),
            )
            .await
            .unwrap();
        assert!(state.paired);
        assert_eq!(state.mode, MobileConnectionMode::LanDirect);

        let snap = client.cached_snapshot().expect("snapshot cached after pair");
        assert_eq!(snap.active_game_id.as_deref(), Some("skyrim"));
        assert_eq!(snap.managed_games.len(), 1);
        assert_eq!(
            client.load_synced_nexus_api_key().unwrap().as_deref(),
            Some("nexus-secret-test-key")
        );
        assert_eq!(
            client.load_synced_modio_api_key().unwrap().as_deref(),
            Some("modio-secret-test-key")
        );

        // Remote mutation: toggle mod & queue Thunderstore download over iroh
        let resp = client
            .send_request(&RemoteRequest::SetModEnabled {
                game_id: "skyrim".into(),
                mod_id: "m1".into(),
                enabled: true,
            })
            .await
            .unwrap();
        assert!(matches!(resp, RemoteResponse::Ok { .. }));
        assert!(*backend.mod_enabled.lock().unwrap());

        let dl_resp = client
            .send_request(&RemoteRequest::QueueDownload(
                RemoteDownloadRequest::ThunderstoreMod {
                    game_id: "lc".into(),
                    community: "lethal-company".into(),
                    namespace: "BepInEx".into(),
                    name: "BepInExPack".into(),
                },
            ))
            .await
            .unwrap();
        assert!(matches!(dl_resp, RemoteResponse::Ok { .. }));
        assert_eq!(
            backend.queued_downloads.lock().unwrap().as_slice(),
            &["BepInExPack"]
        );

        // Unpair wipes cached state AND encrypted API keys
        client.unpair().await.unwrap();
        assert!(!client.connection_state().paired);
        assert!(client.cached_snapshot().is_none());
        assert!(client.load_synced_nexus_api_key().unwrap().is_none());
        assert!(client.load_synced_modio_api_key().unwrap().is_none());

        server_task.abort();
    }
}
