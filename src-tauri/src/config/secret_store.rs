//! OS-agnostic authenticated encryption for API key fallback files when the OS keyring is unavailable.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use anyhow::{bail, Context, Result};
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use rand::Rng;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

const ENVELOPE_PREFIX: &str = "EMM_ENC_V1:";
const SEED_FILENAME: &str = ".emm_key_seed";
const DOMAIN_SEP: &[u8] = b"emperor-mod-manager:secret-store:v1:";

/// Encrypt and atomically store `secret` at `path` with `0o600` permissions on Unix.
pub fn store_encrypted_secret(path: &Path, secret: &str) -> Result<()> {
    let trimmed = secret.trim();
    if trimmed.is_empty() {
        bail!("secret cannot be empty");
    }
    let parent = path
        .parent()
        .context("secret path must have a parent directory")?;
    fs::create_dir_all(parent)?;

    let seed = load_or_create_seed(parent)?;
    let key_bytes = derive_key(&seed);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&*key_bytes));

    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let aad = file_aad(path);
    let ciphertext = cipher
        .encrypt(
            nonce,
            Payload {
                msg: trimmed.as_bytes(),
                aad: aad.as_bytes(),
            },
        )
        .map_err(|_| anyhow::anyhow!("failed to encrypt secret"))?;

    let mut combined = Zeroizing::new(Vec::with_capacity(12 + ciphertext.len()));
    combined.extend_from_slice(&nonce_bytes);
    combined.extend_from_slice(&ciphertext);

    let encoded = Zeroizing::new(format!("{ENVELOPE_PREFIX}{}", B64.encode(&*combined)));
    write_atomic_0600(path, encoded.as_bytes())
}

/// Load and decrypt a secret from `path`. Transparently migrates legacy plaintext files.
pub fn load_encrypted_secret(path: &Path) -> Result<Option<String>> {
    if !path.exists() {
        return Ok(None);
    }
    let raw = Zeroizing::new(fs::read_to_string(path)?);
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    let Some(payload_b64) = trimmed.strip_prefix(ENVELOPE_PREFIX) else {
        // Legacy plaintext file: return the key and transparently upgrade to encrypted storage.
        if let Err(e) = store_encrypted_secret(path, trimmed) {
            log::warn!("failed to migrate legacy secret file to encrypted format: {e}");
        }
        return Ok(Some(trimmed.to_string()));
    };

    let parent = path
        .parent()
        .context("secret path must have a parent directory")?;
    let seed = load_existing_seed(parent)?;
    let key_bytes = derive_key(&seed);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&*key_bytes));

    let decoded = Zeroizing::new(
        B64.decode(payload_b64)
            .context("invalid base64 in encrypted secret")?,
    );
    if decoded.len() < 12 + 16 {
        bail!("encrypted secret payload is truncated");
    }
    let (nonce_bytes, ciphertext) = decoded.split_at(12);
    let nonce = Nonce::from_slice(nonce_bytes);
    let aad = file_aad(path);

    let plaintext = Zeroizing::new(
        cipher
            .decrypt(
                nonce,
                Payload {
                    msg: ciphertext,
                    aad: aad.as_bytes(),
                },
            )
            .map_err(|_| anyhow::anyhow!("failed to decrypt secret (wrong key or tampered file)"))?,
    );

    let s = String::from_utf8(plaintext.to_vec()).context("decrypted secret is not valid UTF-8")?;
    let s = s.trim().to_string();
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(s))
    }
}

fn file_aad(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("secret")
        .to_string()
}

fn load_or_create_seed(dir: &Path) -> Result<Zeroizing<[u8; 32]>> {
    let seed_path = dir.join(SEED_FILENAME);
    if seed_path.exists() {
        return load_existing_seed(dir);
    }
    let mut seed = Zeroizing::new([0u8; 32]);
    rand::rng().fill_bytes(&mut *seed);
    write_atomic_0600(&seed_path, &*seed)?;
    Ok(seed)
}

fn load_existing_seed(dir: &Path) -> Result<Zeroizing<[u8; 32]>> {
    let seed_path = dir.join(SEED_FILENAME);
    let bytes = Zeroizing::new(
        fs::read(&seed_path).with_context(|| "missing encryption seed for fallback store")?,
    );
    if bytes.len() != 32 {
        bail!("invalid encryption seed length");
    }
    let mut seed = Zeroizing::new([0u8; 32]);
    seed.copy_from_slice(&bytes);
    Ok(seed)
}

fn derive_key(seed: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN_SEP);
    hasher.update(seed);
    for binding in machine_user_bindings() {
        hasher.update(&(binding.len() as u32).to_le_bytes());
        hasher.update(binding.as_bytes());
    }
    let digest = hasher.finalize();
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(&digest);
    out
}

fn machine_user_bindings() -> Vec<String> {
    let mut bindings = Vec::new();
    if let Some(base) = directories::BaseDirs::new() {
        bindings.push(base.home_dir().to_string_lossy().into_owned());
    }
    if let Ok(user) = std::env::var("USER").or_else(|_| std::env::var("USERNAME")) {
        bindings.push(user);
    }
    #[cfg(target_os = "linux")]
    {
        for path in ["/etc/machine-id", "/var/lib/dbus/machine-id"] {
            if let Ok(id) = fs::read_to_string(path) {
                let id = id.trim();
                if !id.is_empty() {
                    bindings.push(id.to_string());
                    break;
                }
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        if let Ok(hklm) = winreg::RegKey::predef(winreg::enums::HKEY_LOCAL_MACHINE)
            .open_subkey("SOFTWARE\\Microsoft\\Cryptography")
        {
            if let Ok(guid) = hklm.get_value::<String, _>("MachineGuid") {
                if !guid.trim().is_empty() {
                    bindings.push(guid.trim().to_string());
                }
            }
        }
    }
    bindings
}

fn write_atomic_0600(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .context("target path must have a parent directory")?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("secret");
    let mut suffix = [0u8; 8];
    rand::rng().fill_bytes(&mut suffix);
    let tmp_path: PathBuf = parent.join(format!(".tmp.{name}.{}", hex::encode(suffix)));

    let mut opts = OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut file = opts.open(&tmp_path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&tmp_path)?.permissions();
        perms.set_mode(0o600);
        fs::set_permissions(&tmp_path, perms)?;
    }

    fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn encrypted_secret_roundtrip_and_tamper_detection() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nexus_api_key");
        let secret = "nexus_test_api_key_1234567890abcdef";

        store_encrypted_secret(&path, secret).unwrap();

        let disk = fs::read_to_string(&path).unwrap();
        assert!(disk.starts_with(ENVELOPE_PREFIX));
        assert!(!disk.contains(secret));

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
            assert_eq!(mode, 0o600);
            let seed_mode = fs::metadata(dir.path().join(SEED_FILENAME))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(seed_mode, 0o600);
        }

        let loaded = load_encrypted_secret(&path).unwrap();
        assert_eq!(loaded.as_deref(), Some(secret));

        // Renaming to a different filename must fail AAD verification.
        let other = dir.path().join("modio_api_key");
        fs::copy(&path, &other).unwrap();
        assert!(load_encrypted_secret(&other).is_err());

        // Tampering with ciphertext must fail GCM tag verification.
        let mut tampered = disk.into_bytes();
        let last = tampered.len() - 2;
        tampered[last] ^= 0x01;
        fs::write(&path, tampered).unwrap();
        assert!(load_encrypted_secret(&path).is_err());
    }

    #[test]
    fn legacy_plaintext_migrates_to_encrypted_on_load() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("modio_api_key");
        let secret = "0123456789abcdef0123456789abcdef";
        fs::write(&path, format!("{secret}\n")).unwrap();

        let loaded = load_encrypted_secret(&path).unwrap();
        assert_eq!(loaded.as_deref(), Some(secret));

        let disk = fs::read_to_string(&path).unwrap();
        assert!(disk.starts_with(ENVELOPE_PREFIX));
        assert!(!disk.contains(secret));
        assert_eq!(load_encrypted_secret(&path).unwrap().as_deref(), Some(secret));
    }
}
