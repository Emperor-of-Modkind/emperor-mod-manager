#![allow(dead_code)]
//! Wire protocol, framing, and cryptographic pairing helpers for `emperor-remote/1`.

use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};

use anyhow::{anyhow, bail, Context, Result};
use iroh::EndpointAddr;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroize;

/// Dedicated iroh ALPN for mobile companion <-> desktop remote control.
/// Separate from `emperor-p2p/1` so paired mobile devices bypass social `appear_offline`.
pub const REMOTE_ALPN: &[u8] = b"emperor-remote/1";

/// Maximum JSON frame size (16 MiB, enough for full desktop snapshots & cover thumbnails).
pub const MAX_REMOTE_FRAME_BYTES: usize = 16 * 1024 * 1024;

const PAIRING_DOMAIN_SEP: &[u8] = b"emperor-mobile-pair-v1\0";

/// RFC 2104 HMAC-SHA256 using the existing `sha2` dependency.
pub fn hmac_sha256(key: &[u8], data: &[u8]) -> [u8; 32] {
    const BLOCK_SIZE: usize = 64;
    let mut k_block = [0u8; BLOCK_SIZE];
    if key.len() > BLOCK_SIZE {
        let hashed = Sha256::digest(key);
        k_block[..32].copy_from_slice(&hashed);
    } else {
        k_block[..key.len()].copy_from_slice(key);
    }

    let mut o_key_pad = [0x5cu8; BLOCK_SIZE];
    let mut i_key_pad = [0x36u8; BLOCK_SIZE];
    for i in 0..BLOCK_SIZE {
        o_key_pad[i] ^= k_block[i];
        i_key_pad[i] ^= k_block[i];
    }

    let mut inner = Sha256::new();
    inner.update(i_key_pad);
    inner.update(data);
    let inner_hash = inner.finalize();

    let mut outer = Sha256::new();
    outer.update(o_key_pad);
    outer.update(inner_hash);
    let out = outer.finalize();
    let mut result = [0u8; 32];
    result.copy_from_slice(&out);
    k_block.zeroize();
    o_key_pad.zeroize();
    i_key_pad.zeroize();
    result
}

/// Constant-time byte slice equality to prevent timing side-channels on MAC checks.
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (&x, &y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Normalize a user-entered pairing code by stripping spaces/hyphens.
pub fn normalize_pairing_code(raw: &str) -> String {
    raw.chars().filter(|c| c.is_ascii_digit()).collect()
}

/// Compute the hex-encoded HMAC-SHA256 pairing proof bound to the challenge nonce
/// and both peers' authenticated Ed25519 `EndpointId` strings.
pub fn compute_pairing_proof(
    code: &str,
    nonce: &str,
    desktop_endpoint_id: &str,
    mobile_endpoint_id: &str,
) -> String {
    let normalized = normalize_pairing_code(code);
    let mut msg = Vec::with_capacity(
        PAIRING_DOMAIN_SEP.len()
            + nonce.len()
            + desktop_endpoint_id.len()
            + mobile_endpoint_id.len()
            + 3,
    );
    msg.extend_from_slice(PAIRING_DOMAIN_SEP);
    msg.extend_from_slice(nonce.as_bytes());
    msg.push(0);
    msg.extend_from_slice(desktop_endpoint_id.as_bytes());
    msg.push(0);
    msg.extend_from_slice(mobile_endpoint_id.as_bytes());
    hex::encode(hmac_sha256(normalized.as_bytes(), &msg))
}

/// Returns true if `ip` belongs to a local/private network (RFC1918, link-local, ULA, or loopback).
pub fn is_local_network_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            v4.is_private() || v4.is_loopback() || v4.is_link_local()
        }
        IpAddr::V6(v6) => {
            if let Some(mapped) = v6.to_ipv4_mapped() {
                return mapped.is_private() || mapped.is_loopback() || mapped.is_link_local();
            }
            let seg0 = v6.segments()[0];
            v6.is_loopback()
                || (seg0 & 0xfe00) == 0xfc00 // Unique Local Address (fc00::/7)
                || (seg0 & 0xffc0) == 0xfe80 // Link-local (fe80::/10)
        }
    }
}

/// Write a length-prefixed JSON frame to an iroh QUIC send stream.
pub async fn write_frame<T: Serialize>(
    send: &mut iroh::endpoint::SendStream,
    msg: &T,
) -> Result<()> {
    let bytes = serde_json::to_vec(msg).context("serialize remote frame")?;
    if bytes.len() > MAX_REMOTE_FRAME_BYTES {
        bail!("remote frame exceeds limit ({} bytes)", bytes.len());
    }
    let len = (bytes.len() as u32).to_be_bytes();
    send.write_all(&len).await.context("write remote frame length")?;
    send.write_all(&bytes).await.context("write remote frame body")?;
    Ok(())
}

/// Read a length-prefixed JSON frame from an iroh QUIC recv stream.
pub async fn read_frame<T: for<'de> Deserialize<'de>>(
    recv: &mut iroh::endpoint::RecvStream,
) -> Result<T> {
    let mut len_buf = [0u8; 4];
    recv.read_exact(&mut len_buf)
        .await
        .map_err(|e| anyhow!("read remote frame length: {e}"))?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len == 0 || len > MAX_REMOTE_FRAME_BYTES {
        bail!("invalid remote frame length: {len}");
    }
    let mut body = vec![0u8; len];
    recv.read_exact(&mut body)
        .await
        .map_err(|e| anyhow!("read remote frame body: {e}"))?;
    serde_json::from_slice(&body).context("deserialize remote frame")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PairedDevice {
    pub peer_id: String,
    pub device_name: String,
    pub device_model: String,
    pub paired_at: String,
    pub last_seen_at: String,
    #[serde(default)]
    pub allow_remote_launch: bool,
    /// Runtime-only indicator populated when listing devices on desktop.
    #[serde(default)]
    pub connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobilePairingSession {
    /// Formatted 6-digit code, e.g. `"482-917"`.
    pub code: String,
    pub expires_at: String,
    pub ttl_secs: u64,
    pub host_name: String,
    pub desktop_endpoint_id: String,
    pub lan_addrs: Vec<String>,
    pub remaining_attempts: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DiscoveredDesktop {
    pub host_name: String,
    pub desktop_endpoint_id: String,
    pub lan_addrs: Vec<SocketAddr>,
    pub pairing_active: bool,
    pub app_version: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MobileConnectionMode {
    Disconnected,
    Connecting,
    LanDirect,
    IrohRelay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MobileConnectionState {
    pub paired: bool,
    pub desktop_name: Option<String>,
    pub desktop_peer_id: Option<String>,
    pub mode: MobileConnectionMode,
    pub last_synced_at: Option<String>,
    pub last_error: Option<String>,
    pub allow_remote_launch: bool,
}

impl Default for MobileConnectionState {
    fn default() -> Self {
        Self {
            paired: false,
            desktop_name: None,
            desktop_peer_id: None,
            mode: MobileConnectionMode::Disconnected,
            last_synced_at: None,
            last_error: None,
            allow_remote_launch: false,
        }
    }
}

/// Account metadata synced to mobile (safe to cache in `remote_cache.json`).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteAccountSummary {
    pub nexus_connected: bool,
    pub nexus_user: Option<serde_json::Value>,
    pub modio_connected: bool,
    pub p2p_display_name: String,
    pub p2p_profile_picture: Option<String>,
    pub theme: String,
    pub theme_style: String,
    pub accent_color: String,
    pub adult_content: bool,
    pub game_version_warnings: bool,
    pub auto_sort_before_deploy: bool,
}

/// Sensitive API keys synced ONLY over authenticated `iroh` QUIC and persisted
/// exclusively via `config::secret_store` (AES-256-GCM), never in `remote_cache.json`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RemoteAccountSecrets {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nexus_api_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modio_api_key: Option<String>,
}

impl Drop for RemoteAccountSecrets {
    fn drop(&mut self) {
        if let Some(k) = self.nexus_api_key.as_mut() {
            k.zeroize();
        }
        if let Some(k) = self.modio_api_key.as_mut() {
            k.zeroize();
        }
    }
}

/// Full desktop state snapshot synced to mobile.
/// Uses `serde_json::Value` for complex desktop domain types so the mobile binary
/// stays decoupled from desktop-only crates while keeping exact JSON schema parity.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DesktopSnapshot {
    pub rev: u64,
    pub synced_at: String,
    pub desktop_name: String,
    pub app_version: String,
    pub allow_remote_launch: bool,
    pub account: RemoteAccountSummary,
    pub active_game_id: Option<String>,
    pub managed_games: Vec<serde_json::Value>,
    pub mods_by_game: HashMap<String, Vec<serde_json::Value>>,
    pub installed_collections_by_game: HashMap<String, Vec<serde_json::Value>>,
    pub saved_collections: Vec<serde_json::Value>,
    pub bethesda_plugins_by_game: HashMap<String, serde_json::Value>,
    pub downloads: Vec<serde_json::Value>,
    pub friends: Option<serde_json::Value>,
    pub launch_states: HashMap<String, serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum RemoteDownloadRequest {
    NexusMod {
        game_id: String,
        domain: String,
        mod_id: u64,
        file_id: u64,
        mod_name: String,
        file_name: Option<String>,
        version: Option<String>,
    },
    ThunderstoreMod {
        game_id: String,
        community: String,
        namespace: String,
        name: String,
    },
    ModioMod {
        game_id: String,
        modio_game_id: u32,
        mod_id: u64,
        file_id: Option<u64>,
    },
    NexusCollection {
        game_id: String,
        slug: String,
        revision: Option<u64>,
    },
    ImportShareCode {
        game_id: String,
        code: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum RemoteDownloadControl {
    Pause { id: String },
    Resume { id: String },
    Cancel { id: String },
    CancelBatch { batch_id: String },
    Restart { id: String },
    Remove { id: String },
    ClearRecent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteRequest {
    // Pairing handshake (only allowed when desktop pairing modal is active on LAN)
    PairInit {
        device_name: String,
        device_model: String,
    },
    PairVerify {
        nonce: String,
        proof_hex: String,
    },

    // Authenticated sync & live stream
    Ping,
    GetSyncSnapshot {
        #[serde(default)]
        since_rev: Option<u64>,
    },
    GetAccountSecrets,
    SubscribeEvents,
    GetCoverThumb {
        game_id: String,
    },

    // Authenticated remote actions: downloads, collections, loadouts, launch
    QueueDownload(RemoteDownloadRequest),
    ControlDownload(RemoteDownloadControl),
    SetActiveGame {
        game_id: Option<String>,
    },
    SetModEnabled {
        game_id: String,
        mod_id: String,
        enabled: bool,
    },
    SetLoadOrder {
        game_id: String,
        ordered_ids: Vec<String>,
    },
    AutoSortStagedMods {
        game_id: String,
    },
    SetBethesdaPluginOrder {
        game_id: String,
        ordered_plugins: Vec<String>,
    },
    SortBethesdaPlugins {
        game_id: String,
    },
    SetModOptions {
        game_id: String,
        mod_id: String,
        selections: Vec<serde_json::Value>,
    },
    RemoveMod {
        game_id: String,
        mod_id: String,
    },
    UpdateStagedMod {
        game_id: String,
        mod_id: String,
    },
    SetCollectionEnabled {
        game_id: String,
        collection_id: String,
        enabled: bool,
    },
    SetModCollections {
        game_id: String,
        mod_id: String,
        collection_ids: Vec<String>,
    },
    SaveCollectionChanges {
        game_id: String,
        collection_id: String,
    },
    SetCollectionAutoSave {
        game_id: String,
        collection_id: String,
        auto_save: bool,
    },
    RenameInstalledCollection {
        game_id: String,
        collection_id: String,
        name: String,
    },
    RenameSavedCollection {
        id: String,
        name: String,
    },
    DeleteSavedCollection {
        id: String,
    },
    UninstallCollection {
        game_id: String,
        collection_id: String,
        remove_mods: bool,
    },
    SetCollectionVisibility {
        collection_id: String,
        visibility: String,
    },
    LaunchGame {
        game_id: String,
        modded: bool,
    },
    CloseGame {
        game_id: String,
    },
    /// Generic catalog / query RPC proxied to desktop when online.
    CatalogRpc {
        method: String,
        params: serde_json::Value,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RemoteResponse {
    PairChallenge {
        nonce: String,
        desktop_name: String,
    },
    PairAccepted {
        desktop_name: String,
        desktop_addr: EndpointAddr,
        allow_remote_launch: bool,
    },
    Pong {
        rev: u64,
    },
    Snapshot(DesktopSnapshot),
    SnapshotUnchanged {
        rev: u64,
    },
    AccountSecrets(RemoteAccountSecrets),
    CoverThumb {
        mime: String,
        data_b64: String,
    },
    Ok {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        data: Option<serde_json::Value>,
        rev: u64,
    },
    Subscribed {
        rev: u64,
    },
    Error {
        code: String,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum RemoteEvent {
    DownloadProgress {
        item: serde_json::Value,
    },
    DownloadsChanged {
        rev: u64,
        downloads: Vec<serde_json::Value>,
    },
    ModsChanged {
        rev: u64,
        game_id: String,
        mods: Vec<serde_json::Value>,
    },
    CollectionsChanged {
        rev: u64,
        game_id: String,
        installed: Vec<serde_json::Value>,
    },
    SavedCollectionsChanged {
        rev: u64,
        saved: Vec<serde_json::Value>,
    },
    GamesChanged {
        rev: u64,
        games: Vec<serde_json::Value>,
    },
    FriendsChanged {
        rev: u64,
        friends: serde_json::Value,
    },
    AccountChanged {
        rev: u64,
        account: RemoteAccountSummary,
    },
    LaunchStateChanged {
        rev: u64,
        game_id: String,
        state: serde_json::Value,
    },
    SnapshotInvalidated {
        rev: u64,
    },
    Revoked,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, Ipv6Addr};

    #[test]
    fn hmac_sha256_matches_rfc4231_test_case_1() {
        let key = [0x0bu8; 20];
        let data = b"Hi There";
        let mac = hmac_sha256(&key, data);
        assert_eq!(
            hex::encode(mac),
            "b0344c61d8db38535ca8afceaf0bf12b881dc200c9833da726e9376c2e32cff7"
        );
    }

    #[test]
    fn hmac_sha256_matches_rfc4231_test_case_2() {
        let mac = hmac_sha256(b"Jefe", b"what do ya want for nothing?");
        assert_eq!(
            hex::encode(mac),
            "5bdcc146bf60754e6a042426089575c75a003f089d2739839dec58b964ec3843"
        );
    }

    #[test]
    fn pairing_proof_normalizes_hyphens_and_spaces() {
        let a = compute_pairing_proof("482-917", "nonce123", "desk", "mob");
        let b = compute_pairing_proof("482 917", "nonce123", "desk", "mob");
        let c = compute_pairing_proof("482917", "nonce123", "desk", "mob");
        assert_eq!(a, b);
        assert_eq!(b, c);
        assert!(ct_eq(a.as_bytes(), c.as_bytes()));
        assert!(!ct_eq(a.as_bytes(), b"wrong"));
    }

    #[test]
    fn local_network_ip_classification() {
        assert!(is_local_network_ip(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 42))));
        assert!(is_local_network_ip(IpAddr::V4(Ipv4Addr::new(10, 0, 0, 5))));
        assert!(is_local_network_ip(IpAddr::V4(Ipv4Addr::new(172, 16, 5, 1))));
        assert!(is_local_network_ip(IpAddr::V4(Ipv4Addr::LOCALHOST)));
        assert!(!is_local_network_ip(IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8))));
        assert!(is_local_network_ip(IpAddr::V6(Ipv6Addr::LOCALHOST)));
        assert!(is_local_network_ip(IpAddr::V6(
            "fe80::1".parse::<Ipv6Addr>().unwrap()
        )));
        assert!(!is_local_network_ip(IpAddr::V6(
            "2606:4700:4700::1111".parse::<Ipv6Addr>().unwrap()
        )));
    }
}
