//! 007 First Light (Glacier 2): Runtime `.rpkg` mods + `packagedefinition.txt` `@include`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};
use walkdir::WalkDir;

use super::{normalize_relative, GamePlugin, GamePluginInfo};

pub const FIRST_LIGHT_ROOT_DIRS: &[&str] = &["Runtime", "Retail"];

const PKG_DEF_NAME: &str = "packagedefinition.txt";
const PKG_DEF_BAK: &str = "packagedefinition.txt.emperor-bak";
const EXE_NAME: &str = "007FirstLight.exe";

const NON_GAME_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "webp", "gif", "bmp", "md", "txt", "pdf", "html", "htm",
];

/// 007 First Light packagedefinition XTEA (from First Light Mod Manager).
const XTEA_KEYS: [u32; 4] = [0x30F95282, 0x1F48C419, 0x295F8548, 0x2A78366D];
const XTEA_DELTA: u32 = 0x61C88647;
const XTEA_SUM: u32 = 0xC6EF3720;
const XTEA_HEADER: [u8; 16] = [
    0x22, 0x3D, 0x6F, 0x9A, 0xB3, 0xF8, 0xFE, 0xB6, 0x61, 0xD9, 0xCC, 0x1C, 0x62, 0xDE, 0x83, 0x41,
];

pub struct FirstLightPlugin;

impl GamePlugin for FirstLightPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "007firstlight",
            display_name: "007 First Light",
            nexus_domain: "007firstlight",
            match_names: &["007 first light", "007firstlight"],
        }
    }

    fn prefers_mod_folder(&self) -> bool {
        false
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        FIRST_LIGHT_ROOT_DIRS
    }

    fn should_deploy_file(&self, relative: &Path) -> bool {
        !is_skipped_deploy_file(relative)
    }

    fn preflight_warnings(&self, install_path: &Path) -> Vec<String> {
        let mut warnings = Vec::new();
        if !install_path.join("Runtime").is_dir() {
            warnings.push(
                "Runtime folder not found. 007 First Light mods deploy as .rpkg files under Runtime/."
                    .into(),
            );
        }
        if !install_path.join("Retail").join(EXE_NAME).is_file() {
            warnings.push(format!(
                "Retail/{EXE_NAME} not found. Confirm this is the 007 First Light install root \
                 (parent of Retail/ and Runtime/)."
            ));
        }
        warnings
    }

    fn load_order_entries(&self, content_root: &Path, _staged_name: &str) -> Vec<String> {
        collect_rpkg_stems(content_root)
    }

    fn after_deploy(
        &self,
        install_path: &Path,
        enabled_mod_folders: &[String],
    ) -> Result<Vec<String>> {
        sync_package_includes(install_path, enabled_mod_folders)
    }

    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
        Ok(resolve_first_light_deploy(install_path, relative))
    }
}

fn is_skipped_deploy_file(relative: &Path) -> bool {
    let Some(name) = relative.file_name().and_then(|n| n.to_str()) else {
        return false;
    };
    if name.eq_ignore_ascii_case(PKG_DEF_NAME)
        || name.eq_ignore_ascii_case("mod.json")
        || name.eq_ignore_ascii_case("readme.md")
        || name.eq_ignore_ascii_case("readme.txt")
        || name.eq_ignore_ascii_case("license")
        || name.eq_ignore_ascii_case("license.txt")
    {
        return true;
    }
    relative
        .extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            NON_GAME_EXTENSIONS
                .iter()
                .any(|n| ext.eq_ignore_ascii_case(n))
        })
        .unwrap_or(false)
}

fn collect_rpkg_stems(content_root: &Path) -> Vec<String> {
    let mut stems = Vec::new();
    for entry in WalkDir::new(content_root).into_iter().filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let Ok(relative) = path.strip_prefix(content_root) else {
            continue;
        };
        if is_skipped_deploy_file(relative) {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if !name.to_ascii_lowercase().ends_with(".rpkg") {
            continue;
        }
        let stem = name.trim_end_matches(".rpkg").trim_end_matches(".RPKG");
        if !stem.is_empty() && !stems.iter().any(|s: &String| s.eq_ignore_ascii_case(stem)) {
            stems.push(stem.to_string());
        }
    }
    stems
}

fn resolve_first_light_deploy(install_path: &Path, relative: &Path) -> PathBuf {
    let normalized = normalize_relative(relative);
    let originals: Vec<_> = normalized
        .components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_os_string()),
            _ => None,
        })
        .collect();
    if originals.is_empty() {
        return install_path.join("Runtime");
    }
    let first = originals[0].to_string_lossy();
    let first_lower = first.to_lowercase();
    if first_lower == "runtime" || first_lower == "retail" {
        let mut out = install_path.to_path_buf();
        for orig in originals {
            out.push(orig);
        }
        return out;
    }
    let mut out = install_path.join("Runtime");
    for orig in originals {
        out.push(orig);
    }
    out
}

fn sync_package_includes(install_path: &Path, enabled_stems: &[String]) -> Result<Vec<String>> {
    let runtime = install_path.join("Runtime");
    if !runtime.is_dir() {
        return Ok(vec![
            "Runtime folder missing; skipped packagedefinition.txt @include sync.".into(),
        ]);
    }
    let pkg_path = runtime.join(PKG_DEF_NAME);
    if !pkg_path.is_file() {
        return Ok(vec![
            "packagedefinition.txt not found under Runtime/; skipped @include sync.".into(),
        ]);
    }

    let data = std::fs::read(&pkg_path)
        .with_context(|| format!("read {}", pkg_path.display()))?;
    let encrypted = is_xtea_packagedefinition(&data);
    let content = if encrypted {
        let decrypted = decrypt_buffer(&data[20..]);
        String::from_utf8(decrypted).context("decrypted packagedefinition.txt is not UTF-8")?
    } else {
        String::from_utf8(data.clone()).context("packagedefinition.txt is not UTF-8")?
    };

    ensure_package_backup(&runtime, &data)?;

    let new_content = rewrite_includes(&content, enabled_stems);
    if new_content == content {
        return Ok(Vec::new());
    }

    write_packagedefinition(&pkg_path, &new_content, encrypted)?;
    Ok(Vec::new())
}

fn ensure_package_backup(runtime: &Path, original: &[u8]) -> Result<()> {
    let bak = runtime.join(PKG_DEF_BAK);
    if bak.is_file() {
        return Ok(());
    }
    std::fs::write(&bak, original).with_context(|| format!("write {}", bak.display()))?;
    Ok(())
}

fn rewrite_includes(content: &str, enabled_stems: &[String]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.to_ascii_lowercase().starts_with("@include") {
            continue;
        }
        lines.push(line.to_string());
    }
    while lines.last().is_some_and(|l| l.trim().is_empty()) {
        lines.pop();
    }
    let mut body = lines.join("\n");
    if !body.is_empty() && !body.ends_with('\n') {
        body.push('\n');
    }
    for stem in enabled_stems {
        let stem = stem.trim();
        if stem.is_empty() {
            continue;
        }
        body.push_str(&format!("@include {stem}\n"));
    }
    body
}

fn is_xtea_packagedefinition(data: &[u8]) -> bool {
    data.len() >= 20 && data[..16] == XTEA_HEADER
}

fn decrypt_block_xtea(a: &mut u32, b: &mut u32, keys: &[u32; 4]) {
    let mut sum: u32 = XTEA_SUM;
    for _ in 0..32 {
        *b = b.wrapping_sub(
            ((*a << 4) ^ (*a >> 5))
                .wrapping_add(*a)
                ^ sum.wrapping_add(keys[((sum >> 11) & 3) as usize]),
        );
        sum = sum.wrapping_add(XTEA_DELTA);
        *a = a.wrapping_sub(
            ((*b << 4) ^ (*b >> 5))
                .wrapping_add(*b)
                ^ sum.wrapping_add(keys[(sum & 3) as usize]),
        );
    }
}

fn encrypt_block_xtea(a: &mut u32, b: &mut u32, keys: &[u32; 4]) {
    let mut sum: u32 = 0;
    for _ in 0..32 {
        *a = a.wrapping_add(
            ((*b << 4) ^ (*b >> 5))
                .wrapping_add(*b)
                ^ sum.wrapping_add(keys[(sum & 3) as usize]),
        );
        sum = sum.wrapping_sub(XTEA_DELTA);
        *b = b.wrapping_add(
            ((*a << 4) ^ (*a >> 5))
                .wrapping_add(*a)
                ^ sum.wrapping_add(keys[((sum >> 11) & 3) as usize]),
        );
    }
}

fn decrypt_buffer(data: &[u8]) -> Vec<u8> {
    let mut decrypted = Vec::with_capacity(data.len());
    let block_count = data.len() / 8;
    for i in 0..block_count {
        let offset = i * 8;
        let mut a = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        let mut b = u32::from_le_bytes([
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        decrypt_block_xtea(&mut a, &mut b, &XTEA_KEYS);
        decrypted.extend_from_slice(&a.to_le_bytes());
        decrypted.extend_from_slice(&b.to_le_bytes());
    }
    while decrypted.last() == Some(&0) {
        decrypted.pop();
    }
    decrypted
}

fn encrypt_buffer(data: &[u8]) -> Vec<u8> {
    let mut padded = data.to_vec();
    while padded.len() % 8 != 0 {
        padded.push(0);
    }
    let mut encrypted = Vec::with_capacity(padded.len());
    let block_count = padded.len() / 8;
    for i in 0..block_count {
        let offset = i * 8;
        let mut a = u32::from_le_bytes([
            padded[offset],
            padded[offset + 1],
            padded[offset + 2],
            padded[offset + 3],
        ]);
        let mut b = u32::from_le_bytes([
            padded[offset + 4],
            padded[offset + 5],
            padded[offset + 6],
            padded[offset + 7],
        ]);
        encrypt_block_xtea(&mut a, &mut b, &XTEA_KEYS);
        encrypted.extend_from_slice(&a.to_le_bytes());
        encrypted.extend_from_slice(&b.to_le_bytes());
    }
    encrypted
}

fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

fn write_packagedefinition(path: &Path, content: &str, encrypted: bool) -> Result<()> {
    let content_bytes = content.as_bytes();
    if encrypted {
        let encrypted_bytes = encrypt_buffer(content_bytes);
        let crc = crc32_ieee(content_bytes);
        let mut file_data = Vec::with_capacity(16 + 4 + encrypted_bytes.len());
        file_data.extend_from_slice(&XTEA_HEADER);
        file_data.extend_from_slice(&crc.to_le_bytes());
        file_data.extend_from_slice(&encrypted_bytes);
        std::fs::write(path, &file_data).with_context(|| format!("write {}", path.display()))?;
    } else {
        std::fs::write(path, content_bytes).with_context(|| format!("write {}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::games::match_plugin;

    #[test]
    fn xtea_roundtrip() {
        let plain = b"@partition name=chunk0\n@include chunk0patch203\n";
        let enc = encrypt_buffer(plain);
        let dec = decrypt_buffer(&enc);
        assert_eq!(&dec[..plain.len()], plain.as_slice());
    }

    #[test]
    fn encrypted_packagedefinition_roundtrip_file() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = tmp.path().join("Runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        std::fs::create_dir_all(tmp.path().join("Retail")).unwrap();
        std::fs::write(tmp.path().join("Retail").join(EXE_NAME), b"x").unwrap();

        let body = "@partition name=chunk0 parent=none\n";
        let path = runtime.join(PKG_DEF_NAME);
        write_packagedefinition(&path, body, true).unwrap();
        assert!(is_xtea_packagedefinition(&std::fs::read(&path).unwrap()));

        FirstLightPlugin
            .after_deploy(tmp.path(), &["chunk0patch203".into()])
            .unwrap();
        let data = std::fs::read(&path).unwrap();
        assert!(is_xtea_packagedefinition(&data));
        let text = String::from_utf8(decrypt_buffer(&data[20..])).unwrap();
        assert!(text.contains("@partition name=chunk0 parent=none"));
        assert!(text.contains("@include chunk0patch203"));
        assert!(runtime.join(PKG_DEF_BAK).is_file());
    }

    #[test]
    fn include_sync_adds_and_clears() {
        let tmp = tempfile::tempdir().unwrap();
        let runtime = tmp.path().join("Runtime");
        std::fs::create_dir_all(&runtime).unwrap();
        let body = "@partition name=chunk0 parent=none\n@include oldpatch\n";
        std::fs::write(runtime.join(PKG_DEF_NAME), body).unwrap();

        FirstLightPlugin
            .after_deploy(tmp.path(), &["chunk0patch203".into(), "chunk0patch204".into()])
            .unwrap();
        let after = std::fs::read_to_string(runtime.join(PKG_DEF_NAME)).unwrap();
        assert!(after.contains("@partition name=chunk0 parent=none"));
        assert!(after.contains("@include chunk0patch203"));
        assert!(after.contains("@include chunk0patch204"));
        assert!(!after.contains("@include oldpatch"));

        FirstLightPlugin.after_deploy(tmp.path(), &[]).unwrap();
        let cleared = std::fs::read_to_string(runtime.join(PKG_DEF_NAME)).unwrap();
        assert!(cleared.contains("@partition name=chunk0 parent=none"));
        assert!(!cleared.contains("@include"));
    }

    #[test]
    fn missing_packagedefinition_warns() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("Runtime")).unwrap();
        let warnings = FirstLightPlugin.after_deploy(tmp.path(), &[]).unwrap();
        assert_eq!(warnings.len(), 1);
        assert!(warnings[0].contains("packagedefinition.txt"));
    }

    #[test]
    fn deploys_bare_and_runtime_rpkg() {
        let bare = FirstLightPlugin
            .resolve_deploy_root(Path::new("/game"), Path::new("chunk0patch203.rpkg"))
            .unwrap();
        assert_eq!(
            bare,
            PathBuf::from("/game/Runtime/chunk0patch203.rpkg")
        );

        let nested = FirstLightPlugin
            .resolve_deploy_root(Path::new("/game"), Path::new("Runtime/chunk0patch203.rpkg"))
            .unwrap();
        assert_eq!(
            nested,
            PathBuf::from("/game/Runtime/chunk0patch203.rpkg")
        );

        let retail = FirstLightPlugin
            .resolve_deploy_root(Path::new("/game"), Path::new("Retail/tool.dll"))
            .unwrap();
        assert_eq!(retail, PathBuf::from("/game/Retail/tool.dll"));
    }

    #[test]
    fn skips_packagedefinition_and_junk() {
        assert!(!FirstLightPlugin.should_deploy_file(Path::new("packagedefinition.txt")));
        assert!(!FirstLightPlugin.should_deploy_file(Path::new("Runtime/packagedefinition.txt")));
        assert!(!FirstLightPlugin.should_deploy_file(Path::new("mod.json")));
        assert!(!FirstLightPlugin.should_deploy_file(Path::new("preview.png")));
        assert!(!FirstLightPlugin.should_deploy_file(Path::new("readme.md")));
        assert!(FirstLightPlugin.should_deploy_file(Path::new("chunk0patch203.rpkg")));
        assert!(FirstLightPlugin.should_deploy_file(Path::new("Runtime/chunk0patch203.rpkg")));
    }

    #[test]
    fn load_order_entries_collects_rpkg_stems() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("Runtime")).unwrap();
        std::fs::write(tmp.path().join("Runtime").join("chunk0patch203.rpkg"), b"x").unwrap();
        std::fs::write(tmp.path().join("chunk0patch204.rpkg"), b"x").unwrap();
        std::fs::write(tmp.path().join("mod.json"), b"{}").unwrap();
        std::fs::write(tmp.path().join("packagedefinition.txt"), b"x").unwrap();

        let entries = FirstLightPlugin.load_order_entries(tmp.path(), "SkipIntro");
        assert!(entries.iter().any(|e| e == "chunk0patch203"));
        assert!(entries.iter().any(|e| e == "chunk0patch204"));
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn preflight_warns_on_empty_install() {
        let tmp = tempfile::tempdir().unwrap();
        let warnings = FirstLightPlugin.preflight_warnings(tmp.path());
        assert!(warnings.iter().any(|w| w.contains("Runtime")));
        assert!(warnings.iter().any(|w| w.contains(EXE_NAME)));
    }

    #[test]
    fn match_007_first_light() {
        assert_eq!(
            match_plugin("007 First Light", None).unwrap().id,
            "007firstlight"
        );
        assert_eq!(
            match_plugin("007FirstLight", None).unwrap().id,
            "007firstlight"
        );
        assert!(match_plugin("First Light", None).is_none());
    }
}
