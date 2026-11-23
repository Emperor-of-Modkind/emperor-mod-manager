//! Desktop-side local-network pairing session manager and authorized device store.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use chrono::Utc;
use rand::{Rng, RngExt};

use super::protocol::{compute_pairing_proof, ct_eq, MobilePairingSession, PairedDevice};

pub const PAIRING_TTL: Duration = Duration::from_secs(5 * 60);
pub const MAX_PAIRING_ATTEMPTS: u32 = 5;

struct ActivePairing {
    code_digits: String,
    formatted_code: String,
    host_name: String,
    desktop_endpoint_id: String,
    lan_addrs: Vec<String>,
    created_at: Instant,
    expires_at_rfc3339: String,
    failed_attempts: u32,
    /// Per-mobile-peer pending challenge: `mobile_peer_id -> (nonce, device_name, device_model)`.
    challenges: HashMap<String, (String, String, String)>,
}

pub struct PairingManager {
    devices_path: PathBuf,
    devices: Vec<PairedDevice>,
    active: Option<ActivePairing>,
}

impl PairingManager {
    pub fn load(devices_path: PathBuf) -> Self {
        let devices = fs::read_to_string(&devices_path)
            .ok()
            .and_then(|s| serde_json::from_str::<Vec<PairedDevice>>(&s).ok())
            .unwrap_or_default();
        Self {
            devices_path,
            devices,
            active: None,
        }
    }

    fn save(&self) -> Result<()> {
        if let Some(parent) = self.devices_path.parent() {
            fs::create_dir_all(parent)?;
        }
        let body = serde_json::to_string_pretty(&self.devices)?;
        fs::write(&self.devices_path, body).context("write paired_devices.json")?;
        Ok(())
    }

    fn expire_if_needed(&mut self) {
        if let Some(active) = &self.active {
            if active.created_at.elapsed() >= PAIRING_TTL {
                self.active = None;
            }
        }
    }

    pub fn is_pairing_active(&mut self) -> bool {
        self.expire_if_needed();
        self.active.is_some()
    }

    pub fn start_session(
        &mut self,
        desktop_endpoint_id: String,
        host_name: String,
        lan_addrs: Vec<String>,
    ) -> MobilePairingSession {
        let num: u32 = rand::rng().random_range(0..1_000_000);
        let code_digits = format!("{num:06}");
        let formatted_code = format!("{}-{}", &code_digits[..3], &code_digits[3..]);
        let expires_at = (Utc::now() + chrono::Duration::seconds(PAIRING_TTL.as_secs() as i64))
            .to_rfc3339();

        let session = MobilePairingSession {
            code: formatted_code.clone(),
            expires_at: expires_at.clone(),
            ttl_secs: PAIRING_TTL.as_secs(),
            host_name: host_name.clone(),
            desktop_endpoint_id: desktop_endpoint_id.clone(),
            lan_addrs: lan_addrs.clone(),
            remaining_attempts: MAX_PAIRING_ATTEMPTS,
        };

        self.active = Some(ActivePairing {
            code_digits,
            formatted_code,
            host_name,
            desktop_endpoint_id,
            lan_addrs,
            created_at: Instant::now(),
            expires_at_rfc3339: expires_at,
            failed_attempts: 0,
            challenges: HashMap::new(),
        });

        session
    }

    pub fn active_session(&mut self) -> Option<MobilePairingSession> {
        self.expire_if_needed();
        let active = self.active.as_ref()?;
        let elapsed = active.created_at.elapsed();
        let remaining_secs = PAIRING_TTL.saturating_sub(elapsed).as_secs();
        Some(MobilePairingSession {
            code: active.formatted_code.clone(),
            expires_at: active.expires_at_rfc3339.clone(),
            ttl_secs: remaining_secs,
            host_name: active.host_name.clone(),
            desktop_endpoint_id: active.desktop_endpoint_id.clone(),
            lan_addrs: active.lan_addrs.clone(),
            remaining_attempts: MAX_PAIRING_ATTEMPTS.saturating_sub(active.failed_attempts),
        })
    }

    pub fn cancel_session(&mut self) {
        self.active = None;
    }

    /// Issue a fresh 32-byte random challenge nonce for a mobile peer attempting to pair.
    pub fn create_challenge(
        &mut self,
        mobile_peer_id: &str,
        device_name: String,
        device_model: String,
    ) -> Result<(String, String)> {
        self.expire_if_needed();
        let active = self
            .active
            .as_mut()
            .ok_or_else(|| anyhow!("No active pairing modal on desktop"))?;

        let mut nonce_bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut nonce_bytes);
        let nonce = hex::encode(nonce_bytes);
        let safe_name = sanitize_label(&device_name, "Android Device");
        let safe_model = sanitize_label(&device_model, "Android");

        // Cap pending challenges to avoid unbounded memory growth on LAN flood.
        if active.challenges.len() >= 32 {
            active.challenges.clear();
        }
        active
            .challenges
            .insert(mobile_peer_id.to_string(), (nonce.clone(), safe_name, safe_model));
        Ok((nonce, active.host_name.clone()))
    }

    /// Verify the mobile peer's HMAC proof against the active 6-digit pairing code.
    /// Locks out and invalidates the session after `MAX_PAIRING_ATTEMPTS` failures.
    pub fn verify_challenge(
        &mut self,
        desktop_endpoint_id: &str,
        mobile_peer_id: &str,
        nonce: &str,
        proof_hex: &str,
    ) -> Result<PairedDevice> {
        self.expire_if_needed();
        let Some(active) = self.active.as_mut() else {
            bail!("Pairing session expired or not active");
        };

        let Some((expected_nonce, device_name, device_model)) =
            active.challenges.get(mobile_peer_id).cloned()
        else {
            bail!("No pending pairing challenge for this device");
        };

        if !ct_eq(expected_nonce.as_bytes(), nonce.as_bytes()) {
            bail!("Invalid pairing challenge nonce");
        }

        let expected_proof = compute_pairing_proof(
            &active.code_digits,
            &expected_nonce,
            desktop_endpoint_id,
            mobile_peer_id,
        );

        if !ct_eq(
            expected_proof.as_bytes(),
            proof_hex.trim().to_ascii_lowercase().as_bytes(),
        ) {
            active.failed_attempts += 1;
            active.challenges.remove(mobile_peer_id);
            let remaining = MAX_PAIRING_ATTEMPTS.saturating_sub(active.failed_attempts);
            if remaining == 0 {
                self.active = None;
                bail!("Too many invalid attempts — pairing code invalidated. Generate a new code on desktop.");
            }
            bail!("Incorrect pairing code ({remaining} attempts remaining)");
        }

        // Valid proof! Register or update the paired device and close the pairing session.
        let now = Utc::now().to_rfc3339();
        let device = if let Some(existing) = self
            .devices
            .iter_mut()
            .find(|d| d.peer_id == mobile_peer_id)
        {
            existing.device_name = device_name;
            existing.device_model = device_model;
            existing.last_seen_at = now;
            existing.clone()
        } else {
            let d = PairedDevice {
                peer_id: mobile_peer_id.to_string(),
                device_name,
                device_model,
                paired_at: now.clone(),
                last_seen_at: now,
                allow_remote_launch: false,
                connected: true,
            };
            self.devices.push(d.clone());
            d
        };

        self.active = None;
        self.save()?;
        Ok(device)
    }

    pub fn is_paired(&self, peer_id: &str) -> bool {
        self.devices.iter().any(|d| d.peer_id == peer_id)
    }

    pub fn get_device(&self, peer_id: &str) -> Option<&PairedDevice> {
        self.devices.iter().find(|d| d.peer_id == peer_id)
    }

    pub fn list_devices(&self) -> Vec<PairedDevice> {
        self.devices.clone()
    }

    pub fn touch_last_seen(&mut self, peer_id: &str) {
        if let Some(d) = self.devices.iter_mut().find(|d| d.peer_id == peer_id) {
            d.last_seen_at = Utc::now().to_rfc3339();
            let _ = self.save();
        }
    }

    pub fn revoke_device(&mut self, peer_id: &str) -> Result<bool> {
        let before = self.devices.len();
        self.devices.retain(|d| d.peer_id != peer_id);
        let removed = self.devices.len() != before;
        if removed {
            self.save()?;
        }
        Ok(removed)
    }

    pub fn set_device_permissions(
        &mut self,
        peer_id: &str,
        allow_remote_launch: bool,
    ) -> Result<PairedDevice> {
        let d = self
            .devices
            .iter_mut()
            .find(|d| d.peer_id == peer_id)
            .ok_or_else(|| anyhow!("Paired device not found"))?;
        d.allow_remote_launch = allow_remote_launch;
        let out = d.clone();
        self.save()?;
        Ok(out)
    }
}

fn sanitize_label(raw: &str, fallback: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .take(64)
        .collect();
    if cleaned.is_empty() {
        fallback.to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_happy_path_and_persistence() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("paired_devices.json");
        let mut mgr = PairingManager::load(path.clone());

        let session = mgr.start_session(
            "desktop-peer-1".into(),
            "My Desktop".into(),
            vec!["192.168.1.10:12345".into()],
        );
        assert_eq!(session.code.len(), 7); // XXX-XXX
        assert!(mgr.is_pairing_active());

        let (nonce, host_name) = mgr
            .create_challenge("mobile-peer-1", "Pixel Fold".into(), "Google Pixel Fold".into())
            .unwrap();
        assert_eq!(host_name, "My Desktop");

        let proof = compute_pairing_proof(&session.code, &nonce, "desktop-peer-1", "mobile-peer-1");
        let device = mgr
            .verify_challenge("desktop-peer-1", "mobile-peer-1", &nonce, &proof)
            .unwrap();

        assert_eq!(device.peer_id, "mobile-peer-1");
        assert_eq!(device.device_name, "Pixel Fold");
        assert!(!device.allow_remote_launch);
        assert!(!mgr.is_pairing_active());
        assert!(mgr.is_paired("mobile-peer-1"));

        // Reload from disk and verify persistence
        let mgr2 = PairingManager::load(path);
        assert!(mgr2.is_paired("mobile-peer-1"));
        assert_eq!(mgr2.list_devices().len(), 1);
    }

    #[test]
    fn pairing_locks_out_after_five_wrong_attempts() {
        let dir = tempfile::tempdir().unwrap();
        let mut mgr = PairingManager::load(dir.path().join("paired_devices.json"));
        let _session = mgr.start_session("desk".into(), "PC".into(), vec![]);

        for attempt in 1..=MAX_PAIRING_ATTEMPTS {
            let (nonce, _) = mgr
                .create_challenge("mob", "Phone".into(), "Model".into())
                .unwrap();
            let err = mgr
                .verify_challenge("desk", "mob", &nonce, "0000000000")
                .unwrap_err();
            if attempt < MAX_PAIRING_ATTEMPTS {
                assert!(err.to_string().contains("Incorrect pairing code"));
                assert!(mgr.is_pairing_active());
            } else {
                assert!(err.to_string().contains("invalidated"));
                assert!(!mgr.is_pairing_active());
            }
        }
        assert!(!mgr.is_paired("mob"));
    }

    #[test]
    fn revoke_and_permissions() {
        let dir = tempfile::tempdir().unwrap();
        let mut mgr = PairingManager::load(dir.path().join("paired_devices.json"));
        let session = mgr.start_session("desk".into(), "PC".into(), vec![]);
        let (nonce, _) = mgr.create_challenge("mob", "Fold".into(), "Fold".into()).unwrap();
        let proof = compute_pairing_proof(&session.code, &nonce, "desk", "mob");
        mgr.verify_challenge("desk", "mob", &nonce, &proof).unwrap();

        let updated = mgr.set_device_permissions("mob", true).unwrap();
        assert!(updated.allow_remote_launch);
        assert!(mgr.revoke_device("mob").unwrap());
        assert!(!mgr.is_paired("mob"));
    }
}
