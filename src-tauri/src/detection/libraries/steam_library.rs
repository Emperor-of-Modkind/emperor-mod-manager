//! Full Steam library discovery via local VDF caches (`appinfo.vdf`, `packageinfo.vdf`,
//! `localconfig.vdf`, `loginusers.vdf`) plus optional Steam Web API / public profile XML sync.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use regex::Regex;
use serde::{Deserialize, Serialize};
use sysinfo::Disks;

use super::{
    finish_library_game, LibraryGame, LibraryGameMeta, LibrarySection, LibrarySource,
    SteamSyncOptions,
};

const STEAM_CACHE_FILENAME: &str = "steam_owned_games.json";
const STEAM_CACHE_TTL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SteamLibraryFolderInfo {
    pub path: String,
    pub free_bytes: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteSteamGame {
    pub appid: u32,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SteamOwnedCache {
    fetched_at: u64,
    steam_id: String,
    source: String, // "api" | "xml"
    games: Vec<RemoteSteamGame>,
}

#[derive(Debug, Clone, Default)]
pub struct AppInfoEntry {
    pub name: String,
    pub app_type: String,
    pub oslist: Vec<String>,
    /// Relative CDN path under `store_item_assets/steam/apps/{appid}/` (often `{hash}/library_capsule.jpg`).
    pub library_capsule_rel: Option<String>,
    /// Relative CDN header path under `store_item_assets/steam/apps/{appid}/`.
    pub library_header_rel: Option<String>,
}

/// Discover all Steam installation root directories on the current OS.
pub fn steam_roots() -> Vec<PathBuf> {
    #[cfg(target_os = "linux")]
    {
        crate::detection::steam_shortcuts::linux_steam_roots()
    }
    #[cfg(target_os = "windows")]
    {
        use winreg::enums::HKEY_CURRENT_USER;
        use winreg::RegKey;
        let mut roots = Vec::new();
        if let Ok(hkcu) = RegKey::predef(HKEY_CURRENT_USER).open_subkey(r"Software\Valve\Steam") {
            if let Ok(path) = hkcu.get_value::<String, _>("SteamPath") {
                let p = PathBuf::from(path.replace('/', "\\"));
                if p.is_dir() {
                    roots.push(p);
                }
            }
        }
        roots
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}

/// Parse `steamapps/libraryfolders.vdf` under a Steam root and return all existing library folders.
pub fn parse_library_folders(steam_root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![steam_root.to_path_buf()];
    let vdf = steam_root.join("steamapps").join("libraryfolders.vdf");
    let Ok(text) = fs::read_to_string(&vdf) else {
        return libs;
    };
    let re = Regex::new(r#""path"\s+"([^"]+)""#).expect("libraryfolders regex");
    for cap in re.captures_iter(&text) {
        let raw = cap[1].replace("\\\\", "\\");
        let p = PathBuf::from(raw);
        if p.is_dir() && !libs.iter().any(|e| e == &p) {
            libs.push(p);
        }
    }
    libs
}

/// Return all Steam library folders along with available disk space for the install modal.
pub fn steam_library_folders_with_space() -> Vec<SteamLibraryFolderInfo> {
    let disks = Disks::new_with_refreshed_list();
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for root in steam_roots() {
        for lib in parse_library_folders(&root) {
            let key = lib.to_string_lossy().to_string();
            if !seen.insert(key.clone()) {
                continue;
            }
            let free_bytes = free_space_for_path(&lib, &disks);
            out.push(SteamLibraryFolderInfo {
                path: key,
                free_bytes,
            });
        }
    }
    out
}

/// Look up free disk space for `path` (walking up to the nearest existing ancestor).
pub fn free_space_for_path(path: &Path, disks: &Disks) -> Option<u64> {
    let mut probe = path.to_path_buf();
    while !probe.exists() {
        if !probe.pop() {
            break;
        }
    }
    let canonical = fs::canonicalize(&probe).unwrap_or(probe);
    let mut best: Option<(usize, u64)> = None;
    for disk in disks.list() {
        let mount = disk.mount_point();
        if canonical.starts_with(mount) {
            let len = mount.as_os_str().len();
            if best.is_none_or(|(best_len, _)| len > best_len) {
                best = Some((len, disk.available_space()));
            }
        }
    }
    best.map(|(_, bytes)| bytes)
}

/// Detect the most recently logged-in SteamID64 from `config/loginusers.vdf`.
pub fn detect_local_steam_id() -> Option<String> {
    for root in steam_roots() {
        let vdf = root.join("config").join("loginusers.vdf");
        if let Ok(text) = fs::read_to_string(&vdf) {
            if let Some(id) = parse_loginusers_steam_id(&text) {
                return Some(id);
            }
        }
    }
    None
}

pub fn parse_loginusers_steam_id(text: &str) -> Option<String> {
    let block_re = Regex::new(r#""(7656119\d{10})"\s*\{([^}]*)\}"#).ok()?;
    let recent_re = Regex::new(r#"(?i)"MostRecent"\s+"1""#).ok()?;
    let mut fallback = None;
    for cap in block_re.captures_iter(text) {
        let id = cap[1].to_string();
        let body = &cap[2];
        if recent_re.is_match(body) {
            return Some(id);
        }
        if fallback.is_none() {
            fallback = Some(id);
        }
    }
    fallback
}

/// Collect installed Steam app IDs from `steamapps/appmanifest_*.acf`.
fn collect_installed_app_ids(roots: &[PathBuf]) -> HashSet<u32> {
    let mut installed = HashSet::new();
    for root in roots {
        for lib in parse_library_folders(root) {
            let steamapps = lib.join("steamapps");
            let Ok(entries) = fs::read_dir(&steamapps) else {
                continue;
            };
            for entry in entries.flatten() {
                let name = entry.file_name();
                let Some(name_str) = name.to_str() else {
                    continue;
                };
                if let Some(rest) = name_str
                    .strip_prefix("appmanifest_")
                    .and_then(|s| s.strip_suffix(".acf"))
                {
                    if let Ok(id) = rest.parse::<u32>() {
                        installed.insert(id);
                    }
                }
            }
        }
    }
    installed
}

/// Extract app IDs referenced in `userdata/*/config/localconfig.vdf` under `"apps"`.
fn collect_localconfig_app_ids(roots: &[PathBuf]) -> HashSet<u32> {
    let mut ids = HashSet::new();
    let app_key_re = Regex::new(r#"^\s*"(\d{2,8})"\s*$"#).expect("localconfig appid regex");
    for root in roots {
        let userdata = root.join("userdata");
        let Ok(users) = fs::read_dir(&userdata) else {
            continue;
        };
        for user in users.flatten() {
            let cfg = user.path().join("config").join("localconfig.vdf");
            let Ok(text) = fs::read_to_string(&cfg) else {
                continue;
            };
            let mut in_apps = false;
            let mut depth: i32 = 0;
            let mut apps_depth: i32 = -1;
            for line in text.lines() {
                let trimmed = line.trim();
                if !in_apps && trimmed.eq_ignore_ascii_case("\"apps\"") {
                    in_apps = true;
                    apps_depth = depth + 1;
                    continue;
                }
                if trimmed == "{" {
                    depth += 1;
                    continue;
                }
                if trimmed == "}" {
                    depth -= 1;
                    if in_apps && depth < apps_depth {
                        in_apps = false;
                        apps_depth = -1;
                    }
                    continue;
                }
                if in_apps && depth == apps_depth {
                    if let Some(cap) = app_key_re.captures(trimmed) {
                        if let Ok(id) = cap[1].parse::<u32>() {
                            ids.insert(id);
                        }
                    }
                }
            }
        }
    }
    ids
}

/// Parse Steam's binary `appcache/packageinfo.vdf` to collect owned app IDs.
pub fn parse_packageinfo_app_ids(bytes: &[u8]) -> HashSet<u32> {
    let mut out = HashSet::new();
    let mut cur = Cursor::new(bytes);
    let Ok(magic) = read_u32_le(&mut cur) else {
        return out;
    };
    // Magic 0x06565527 or 0x06565528
    if magic != 0x0656_5527 && magic != 0x0656_5528 {
        return out;
    }
    if read_u32_le(&mut cur).is_err() {
        return out; // universe
    }
    loop {
        let Ok(pkg_id) = read_u32_le(&mut cur) else {
            break;
        };
        if pkg_id == 0xFFFF_FFFF {
            break;
        }
        // sha1 (20 bytes) + change_number (4 bytes)
        if skip_bytes(&mut cur, 24).is_err() {
            break;
        }
        // v28 adds 8-byte pics token
        if magic == 0x0656_5528 && skip_bytes(&mut cur, 8).is_err() {
            break;
        }
        if parse_bvdf_for_appids(&mut cur, None, false, &mut out).is_err() {
            break;
        }
    }
    out
}

fn parse_bvdf_for_appids(
    cur: &mut Cursor<&[u8]>,
    string_table: Option<&[String]>,
    in_appids: bool,
    out: &mut HashSet<u32>,
) -> std::io::Result<()> {
    loop {
        let tag = read_u8(cur)?;
        if tag == 0x08 {
            return Ok(());
        }
        let key = read_bvdf_key(cur, string_table)?;
        match tag {
            0x00 => {
                let next_in = in_appids || key.eq_ignore_ascii_case("appids");
                parse_bvdf_for_appids(cur, string_table, next_in, out)?;
            }
            0x01 => {
                let _ = read_cstr(cur)?;
            }
            0x02 => {
                let val = read_u32_le(cur)?;
                if in_appids && val > 0 {
                    out.insert(val);
                }
            }
            0x03 => {
                skip_bytes(cur, 4)?;
            }
            0x04 => {
                skip_bytes(cur, 4)?;
            }
            0x05 => {
                // wide string (UTF-16LE null terminated)
                loop {
                    let ch = read_u16_le(cur)?;
                    if ch == 0 {
                        break;
                    }
                }
            }
            0x06 => {
                skip_bytes(cur, 4)?;
            }
            0x07 => {
                skip_bytes(cur, 8)?;
            }
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unknown bvdf tag",
                ));
            }
        }
    }
}

/// Parse Steam's binary `appcache/appinfo.vdf` (v27 / v28 / v29) for game metadata.
pub fn parse_appinfo_games(bytes: &[u8]) -> HashMap<u32, AppInfoEntry> {
    let mut out = HashMap::new();
    let mut cur = Cursor::new(bytes);
    let Ok(magic) = read_u32_le(&mut cur) else {
        return out;
    };
    let version = match magic {
        0x0756_4427 => 27,
        0x0756_4428 => 28,
        0x0756_4429 => 29,
        _ => return out,
    };
    if read_u32_le(&mut cur).is_err() {
        return out; // universe
    }
    let string_table: Option<Vec<String>> = if version >= 29 {
        let Ok(offset) = read_u64_le(&mut cur) else {
            return out;
        };
        let saved = cur.position();
        if offset as usize >= bytes.len() {
            return out;
        }
        cur.set_position(offset);
        let Ok(count) = read_u32_le(&mut cur) else {
            return out;
        };
        let mut table = Vec::with_capacity((count as usize).min(250_000));
        for _ in 0..count {
            match read_cstr(&mut cur) {
                Ok(s) => table.push(s),
                Err(_) => return out,
            }
        }
        cur.set_position(saved);
        Some(table)
    } else {
        None
    };

    loop {
        let Ok(app_id) = read_u32_le(&mut cur) else {
            break;
        };
        if app_id == 0 {
            break;
        }
        let Ok(size) = read_u32_le(&mut cur) else {
            break;
        };
        let entry_end = cur.position().saturating_add(size as u64);
        if entry_end as usize > bytes.len() {
            break;
        }
        // Header inside entry:
        // info_state (4) + last_updated (4) + pics_token (8) + text_sha1 (20) + change_number (4) = 40
        // v28+ adds binary_sha1 (20) = 60 bytes total
        let header_skip = if version >= 28 { 60 } else { 40 };
        if skip_bytes(&mut cur, header_skip).is_err() {
            break;
        }
        let mut entry = AppInfoEntry::default();
        let mut path_stack: Vec<String> = Vec::with_capacity(8);
        let _ = parse_appinfo_bvdf(
            &mut cur,
            string_table.as_deref(),
            &mut path_stack,
            &mut entry,
        );
        if !entry.name.is_empty() && entry.app_type.eq_ignore_ascii_case("game") {
            out.insert(app_id, entry);
        }
        cur.set_position(entry_end);
    }
    out
}

fn parse_appinfo_bvdf(
    cur: &mut Cursor<&[u8]>,
    string_table: Option<&[String]>,
    path: &mut Vec<String>,
    out: &mut AppInfoEntry,
) -> std::io::Result<()> {
    loop {
        let tag = read_u8(cur)?;
        if tag == 0x08 {
            return Ok(());
        }
        let key = read_bvdf_key(cur, string_table)?;
        match tag {
            0x00 => {
                path.push(key.to_ascii_lowercase());
                parse_appinfo_bvdf(cur, string_table, path, out)?;
                path.pop();
            }
            0x01 => {
                let val = read_cstr(cur)?;
                record_appinfo_string(path, &key, val, out);
            }
            0x02 | 0x03 | 0x04 | 0x06 => skip_bytes(cur, 4)?,
            0x05 => loop {
                if read_u16_le(cur)? == 0 {
                    break;
                }
            },
            0x07 => skip_bytes(cur, 8)?,
            _ => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "unknown appinfo bvdf tag",
                ));
            }
        }
    }
}

fn record_appinfo_string(path: &[String], key: &str, val: String, out: &mut AppInfoEntry) {
    let trimmed = val.trim();
    if trimmed.is_empty() {
        return;
    }
    let key_lower = key.to_ascii_lowercase();
    let Some(common_idx) = path.iter().position(|p| p == "common") else {
        return;
    };
    let rel = &path[common_idx + 1..];
    match rel {
        [] => {
            if key_lower == "name" && out.name.is_empty() {
                out.name = trimmed.to_string();
            } else if key_lower == "type" {
                out.app_type = trimmed.to_string();
            } else if key_lower == "oslist" {
                out.oslist = trimmed
                    .split(',')
                    .map(|s| s.trim().to_ascii_lowercase())
                    .filter(|s| !s.is_empty())
                    .collect();
            }
        }
        [a, b, c] if a == "library_assets_full" && b == "library_capsule" && (c == "image" || c == "image2x") => {
            // Prefer `english` and prefer 1x `image` (or set if empty).
            if key_lower == "english" {
                if c == "image" || out.library_capsule_rel.is_none() {
                    out.library_capsule_rel = Some(trimmed.to_string());
                }
            } else if out.library_capsule_rel.is_none() {
                out.library_capsule_rel = Some(trimmed.to_string());
            }
        }
        [a, b, c] if a == "library_assets_full" && b == "library_header" && (c == "image" || c == "image2x") => {
            if key_lower == "english" {
                if c == "image" || out.library_header_rel.is_none() {
                    out.library_header_rel = Some(trimmed.to_string());
                }
            } else if out.library_header_rel.is_none() {
                out.library_header_rel = Some(trimmed.to_string());
            }
        }
        [a] if a == "header_image" => {
            if out.library_header_rel.is_none() || key_lower == "english" {
                out.library_header_rel = Some(trimmed.to_string());
            }
        }
        _ => {}
    }
}

fn read_bvdf_key(
    cur: &mut Cursor<&[u8]>,
    string_table: Option<&[String]>,
) -> std::io::Result<String> {
    if let Some(table) = string_table {
        let idx = read_u32_le(cur)? as usize;
        table.get(idx).cloned().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "string table OOB")
        })
    } else {
        read_cstr(cur)
    }
}

fn read_u8(cur: &mut Cursor<&[u8]>) -> std::io::Result<u8> {
    let mut b = [0u8; 1];
    cur.read_exact(&mut b)?;
    Ok(b[0])
}

fn read_u16_le(cur: &mut Cursor<&[u8]>) -> std::io::Result<u16> {
    let mut b = [0u8; 2];
    cur.read_exact(&mut b)?;
    Ok(u16::from_le_bytes(b))
}

fn read_u32_le(cur: &mut Cursor<&[u8]>) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    cur.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}

fn read_u64_le(cur: &mut Cursor<&[u8]>) -> std::io::Result<u64> {
    let mut b = [0u8; 8];
    cur.read_exact(&mut b)?;
    Ok(u64::from_le_bytes(b))
}

fn skip_bytes(cur: &mut Cursor<&[u8]>, n: u64) -> std::io::Result<()> {
    let next = cur.position().saturating_add(n);
    if next as usize > cur.get_ref().len() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "skip past EOF",
        ));
    }
    cur.set_position(next);
    Ok(())
}

fn read_cstr(cur: &mut Cursor<&[u8]>) -> std::io::Result<String> {
    let slice = cur.get_ref();
    let start = cur.position() as usize;
    let rel_end = slice[start..]
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "unterminated cstr"))?;
    let s = String::from_utf8_lossy(&slice[start..start + rel_end]).into_owned();
    cur.set_position((start + rel_end + 1) as u64);
    Ok(s)
}

const COVER_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];

fn is_cover_image(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    COVER_EXTS
        .iter()
        .any(|ext| lower.ends_with(&format!(".{ext}")))
}

/// Lower is better. Portrait beats capsule, then hero, header, and logo.
fn cover_rank(name: &str) -> Option<u8> {
    if !is_cover_image(name) {
        return None;
    }
    let lower = name.to_ascii_lowercase();
    if lower.contains("library_600x900") {
        Some(0)
    } else if lower.contains("library_capsule") {
        Some(1)
    } else if lower.contains("library_hero") {
        Some(if lower.contains("blur") { 5 } else { 2 })
    } else if lower.contains("header") {
        Some(3)
    } else if lower.contains("logo") {
        Some(4)
    } else {
        None
    }
}

fn consider_cover(best: &mut Option<(u8, PathBuf)>, path: PathBuf) {
    if !path.is_file() {
        return;
    }
    let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
        return;
    };
    let Some(rank) = cover_rank(name) else {
        return;
    };
    if best.as_ref().is_none_or(|(current, _)| rank < *current) {
        *best = Some((rank, path));
    }
}

fn find_cover_in_dir(dir: &Path, depth: usize) -> Option<(u8, PathBuf)> {
    const MAX_DEPTH: usize = 1;
    let mut best: Option<(u8, PathBuf)> = None;
    let Ok(entries) = fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if depth < MAX_DEPTH {
                if let Some((rank, found)) = find_cover_in_dir(&path, depth + 1) {
                    if rank == 0 {
                        return Some((0, found));
                    }
                    if best.as_ref().is_none_or(|(current, _)| rank < *current) {
                        best = Some((rank, found));
                    }
                }
            }
            continue;
        }
        consider_cover(&mut best, path);
        if best.as_ref().is_some_and(|(rank, _)| *rank == 0) {
            return best;
        }
    }
    best
}

fn resolve_userdata_grid(steam_root: &Path, app_id: &str) -> Option<PathBuf> {
    let userdata = steam_root.join("userdata");
    let Ok(entries) = fs::read_dir(&userdata) else {
        return None;
    };
    let mut plain: Option<PathBuf> = None;
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let grid = entry.path().join("config").join("grid");
        if !grid.is_dir() {
            continue;
        }
        for ext in COVER_EXTS {
            let portrait = grid.join(format!("{app_id}p.{ext}"));
            if portrait.is_file() {
                return Some(portrait);
            }
        }
        if plain.is_none() {
            for ext in COVER_EXTS {
                let path = grid.join(format!("{app_id}.{ext}"));
                if path.is_file() {
                    plain = Some(path);
                    break;
                }
            }
        }
    }
    plain
}

/// Locate a local Steam library poster across legacy flat files, newer
/// `librarycache/{appid}/{hash}/` folders, and `userdata/*/config/grid/`.
pub fn local_steam_cover(roots: &[PathBuf], app_id: u32) -> Option<String> {
    let id_str = app_id.to_string();
    for root in roots {
        let cache = root.join("appcache").join("librarycache");
        if cache.is_dir() {
            for stem in ["library_600x900", "library_capsule"] {
                for ext in COVER_EXTS {
                    let flat = cache.join(format!("{id_str}_{stem}.{ext}"));
                    if flat.is_file() {
                        return Some(flat.to_string_lossy().to_string());
                    }
                }
            }
            let mut best: Option<(u8, PathBuf)> = None;
            let app_dir = cache.join(&id_str);
            if app_dir.is_dir() {
                best = find_cover_in_dir(&app_dir, 0);
            }
            if best.as_ref().is_some_and(|(rank, _)| *rank <= 1) {
                return best.map(|(_, p)| p.to_string_lossy().to_string());
            }
            for stem in ["library_hero", "header", "logo"] {
                for ext in COVER_EXTS {
                    consider_cover(&mut best, cache.join(format!("{id_str}_{stem}.{ext}")));
                }
            }
            if let Some((_, p)) = best {
                return Some(p.to_string_lossy().to_string());
            }
        }
        if let Some(grid) = resolve_userdata_grid(root, &id_str) {
            return Some(grid.to_string_lossy().to_string());
        }
    }
    None
}

/// Ordered Steam CDN cover URLs: hashed `appinfo.vdf` capsule first, then
/// unhashed portrait capsules, hashed header, and finally `header.jpg`.
pub fn steam_cdn_cover_urls(app_id: u32, info: Option<&AppInfoEntry>) -> Vec<String> {
    let base = format!("https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/{app_id}");
    let mut urls: Vec<String> = Vec::with_capacity(5);
    let mut push_unique = |url: String| {
        if !urls.contains(&url) {
            urls.push(url);
        }
    };
    if let Some(rel) = info
        .and_then(|i| i.library_capsule_rel.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        push_unique(format!("{base}/{}", rel.trim_start_matches('/')));
    }
    push_unique(format!("{base}/library_600x900.jpg"));
    push_unique(format!("{base}/library_capsule.jpg"));
    if let Some(rel) = info
        .and_then(|i| i.library_header_rel.as_deref())
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        push_unique(format!("{base}/{}", rel.trim_start_matches('/')));
    }
    push_unique(format!("{base}/header.jpg"));
    urls
}

/// Parse Steam Web API `IPlayerService/GetOwnedGames/v1` JSON response.
pub fn parse_steam_web_api_owned(json: &str) -> Result<Vec<RemoteSteamGame>, String> {
    let value: serde_json::Value =
        serde_json::from_str(json).map_err(|e| format!("Invalid Steam API JSON: {e}"))?;
    let Some(games) = value
        .get("response")
        .and_then(|r| r.get("games"))
        .and_then(|g| g.as_array())
    else {
        return Ok(Vec::new());
    };
    let mut out = Vec::with_capacity(games.len());
    for g in games {
        let Some(appid) = g.get("appid").and_then(|v| v.as_u64()).map(|v| v as u32) else {
            continue;
        };
        let name = g
            .get("name")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .unwrap_or("")
            .to_string();
        if !name.is_empty() {
            out.push(RemoteSteamGame { appid, name });
        }
    }
    Ok(out)
}

/// Parse Steam community public profile XML (`steamcommunity.com/profiles/{id}/games?tab=all&xml=1`).
pub fn parse_steam_profile_xml(xml: &str) -> Result<Vec<RemoteSteamGame>, String> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);
    let mut in_game = false;
    let mut current_tag = String::new();
    let mut cur_id: Option<u32> = None;
    let mut cur_name: Option<String> = None;
    let mut out = Vec::new();

    loop {
        match reader.read_event() {
            Ok(Event::Start(e)) => {
                let name = e.name().as_ref().to_string();
                if name == "game" {
                    in_game = true;
                    cur_id = None;
                    cur_name = None;
                } else if in_game {
                    current_tag = name;
                }
            }
            Ok(Event::Text(e)) if in_game => {
                let raw = e.as_ref();
                let decoded = quick_xml::escape::unescape(raw)
                    .map(|c| c.into_owned())
                    .unwrap_or_else(|_| raw.to_string());
                let s = decoded.trim();
                if current_tag == "appID" {
                    cur_id = s.parse::<u32>().ok();
                } else if current_tag == "name" && !s.is_empty() {
                    cur_name = Some(s.to_string());
                }
            }
            Ok(Event::CData(e)) if in_game => {
                let s = e.as_ref().trim().to_string();
                if current_tag == "name" && !s.is_empty() {
                    cur_name = Some(s);
                }
            }
            Ok(Event::End(e)) => {
                let name = e.name().as_ref().to_string();
                if name == "game" {
                    if let (Some(appid), Some(title)) = (cur_id.take(), cur_name.take()) {
                        out.push(RemoteSteamGame { appid, name: title });
                    }
                    in_game = false;
                }
                current_tag.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("Failed to parse Steam XML: {e}")),
            _ => {}
        }
    }
    Ok(out)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

async fn fetch_remote_steam_games(
    cache_dir: &Path,
    steam_id: &str,
    api_key: Option<&str>,
    refresh: bool,
) -> Result<(Vec<RemoteSteamGame>, String), String> {
    let cache_path = cache_dir.join(STEAM_CACHE_FILENAME);
    if !refresh {
        if let Ok(raw) = fs::read_to_string(&cache_path) {
            if let Ok(cached) = serde_json::from_str::<SteamOwnedCache>(&raw) {
                let fresh = now_secs().saturating_sub(cached.fetched_at) < STEAM_CACHE_TTL_SECS;
                if fresh && cached.steam_id == steam_id && !cached.games.is_empty() {
                    return Ok((cached.games, cached.source));
                }
            }
        }
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| format!("HTTP client error: {e}"))?;

    let (games, source) = if let Some(key) = api_key.map(str::trim).filter(|k| !k.is_empty()) {
        let url = format!(
            "https://api.steampowered.com/IPlayerService/GetOwnedGames/v1/?key={key}&steamid={steam_id}&include_appinfo=1&include_played_free_games=1&format=json"
        );
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Steam Web API request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Steam Web API returned HTTP {}", resp.status()));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| format!("Steam Web API read error: {e}"))?;
        (parse_steam_web_api_owned(&body)?, "api".to_string())
    } else {
        let url = format!("https://steamcommunity.com/profiles/{steam_id}/games?tab=all&xml=1");
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|e| format!("Steam profile XML request failed: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("Steam profile returned HTTP {}", resp.status()));
        }
        let body = resp
            .text()
            .await
            .map_err(|e| format!("Steam profile read error: {e}"))?;
        let parsed = parse_steam_profile_xml(&body)?;
        if parsed.is_empty() {
            return Err(
                "Steam profile returned no games (make sure Game Details is Public, or add a Steam Web API key in Settings)."
                    .to_string(),
            );
        }
        (parsed, "xml".to_string())
    };

    let _ = fs::create_dir_all(cache_dir);
    let cache = SteamOwnedCache {
        fetched_at: now_secs(),
        steam_id: steam_id.to_string(),
        source: source.clone(),
        games: games.clone(),
    };
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = fs::write(&cache_path, json);
    }
    Ok((games, source))
}

/// Scan the user's Steam library (local VDF cache + optional remote Steam sync).
pub async fn scan_steam_library(
    cache_dir: &Path,
    opts: &SteamSyncOptions,
) -> Option<LibrarySection> {
    let roots = steam_roots();
    if roots.is_empty() && !opts.sync_enabled {
        return None;
    }

    let installed_ids = collect_installed_app_ids(&roots);
    let mut appinfo_map: HashMap<u32, AppInfoEntry> = HashMap::new();
    let mut owned_ids: HashSet<u32> = collect_localconfig_app_ids(&roots);

    for root in &roots {
        let pkg_path = root.join("appcache").join("packageinfo.vdf");
        if let Ok(bytes) = fs::read(&pkg_path) {
            owned_ids.extend(parse_packageinfo_app_ids(&bytes));
        }
        let app_path = root.join("appcache").join("appinfo.vdf");
        if let Ok(bytes) = fs::read(&app_path) {
            for (id, entry) in parse_appinfo_games(&bytes) {
                appinfo_map.entry(id).or_insert(entry);
            }
        }
    }

    let mut games_by_id: HashMap<u32, (String, Vec<String>)> = HashMap::new();
    // Add local owned games that exist in appinfo as type=="game"
    for &appid in &owned_ids {
        if let Some(info) = appinfo_map.get(&appid) {
            games_by_id.insert(appid, (info.name.clone(), info.oslist.clone()));
        }
    }

    let mut warning: Option<String> = None;
    let effective_steam_id = opts
        .steam_id_override
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .or_else(detect_local_steam_id);

    if opts.sync_enabled {
        if let Some(steam_id) = effective_steam_id.as_deref() {
            match fetch_remote_steam_games(
                cache_dir,
                steam_id,
                opts.api_key.as_deref(),
                opts.refresh_remote,
            )
            .await
            {
                Ok((remote_games, _src)) => {
                    for rg in remote_games {
                        let oslist = appinfo_map
                            .get(&rg.appid)
                            .map(|i| i.oslist.clone())
                            .unwrap_or_else(|| vec!["windows".to_string()]);
                        games_by_id.entry(rg.appid).or_insert((rg.name, oslist));
                    }
                }
                Err(err) => {
                    warning = Some(format!(
                        "Steam remote sync failed ({err}). Showing locally cached titles."
                    ));
                }
            }
        } else {
            warning = Some(
                "Steam sync is enabled, but no SteamID64 was found. Set your SteamID64 in Settings."
                    .to_string(),
            );
        }
    } else if !roots.is_empty() {
        warning = Some(
            "Showing locally cached Steam titles. Enable Steam Library Sync in Settings for your complete owned library."
                .to_string(),
        );
    }

    let total_owned = games_by_id.len();
    let mut games: Vec<LibraryGame> = Vec::new();
    for (appid, (title, oslist)) in games_by_id {
        if installed_ids.contains(&appid) {
            continue;
        }
        let cover_path = local_steam_cover(&roots, appid);
        let mut cdn_urls = if cover_path.is_none() {
            steam_cdn_cover_urls(appid, appinfo_map.get(&appid))
        } else {
            Vec::new()
        };
        let cover_url = if cdn_urls.is_empty() {
            None
        } else {
            Some(cdn_urls.remove(0))
        };
        let platforms = if oslist.is_empty() {
            vec!["windows".to_string()]
        } else {
            oslist
        };
        let mut game = finish_library_game(
            LibrarySource::Steam,
            appid.to_string(),
            title,
            cover_path,
            cover_url,
            None,
            platforms,
            false,
            LibraryGameMeta::default(),
        );
        game.cover_fallbacks = cdn_urls;
        games.push(game);
    }

    Some(LibrarySection {
        source: LibrarySource::Steam,
        label: LibrarySource::Steam.label().to_string(),
        total_owned,
        games,
        warning,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_loginusers_prefers_most_recent() {
        let vdf = r#"
        "users"
        {
            "76561198000000001"
            {
                "AccountName" "old"
                "MostRecent" "0"
            }
            "76561198000000002"
            {
                "AccountName" "current"
                "MostRecent" "1"
            }
        }
        "#;
        assert_eq!(
            parse_loginusers_steam_id(vdf).as_deref(),
            Some("76561198000000002")
        );
    }

    #[test]
    fn parses_steam_web_api_json() {
        let json = r#"{
            "response": {
                "game_count": 2,
                "games": [
                    { "appid": 489830, "name": "The Elder Scrolls V: Skyrim Special Edition" },
                    { "appid": 1091500, "name": "Cyberpunk 2077" }
                ]
            }
        }"#;
        let games = parse_steam_web_api_owned(json).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].appid, 489830);
        assert_eq!(games[1].name, "Cyberpunk 2077");
    }

    #[test]
    fn parses_steam_profile_xml() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
        <gamesList>
          <games>
            <game>
              <appID>489830</appID>
              <name><![CDATA[The Elder Scrolls V: Skyrim Special Edition]]></name>
            </game>
            <game>
              <appID>377160</appID>
              <name>Fallout 4</name>
            </game>
          </games>
        </gamesList>"#;
        let games = parse_steam_profile_xml(xml).unwrap();
        assert_eq!(games.len(), 2);
        assert_eq!(games[0].appid, 489830);
        assert_eq!(
            games[0].name,
            "The Elder Scrolls V: Skyrim Special Edition"
        );
        assert_eq!(games[1].appid, 377160);
        assert_eq!(games[1].name, "Fallout 4");
    }

    #[test]
    fn parses_binary_appinfo_v28_game_and_skips_tool() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x0756_4428u32.to_le_bytes()); // v28
        buf.extend_from_slice(&1u32.to_le_bytes()); // universe

        // Helper to build one v28 entry
        let push_entry = |buf: &mut Vec<u8>, appid: u32, name: &str, app_type: &str| {
            let mut body = vec![0u8; 60]; // 60-byte v28 entry header
            // root -> "appinfo" (0x00) -> "common" (0x00) -> name, type
            body.push(0x00);
            body.extend_from_slice(b"appinfo\0");
            body.push(0x00);
            body.extend_from_slice(b"common\0");
            body.push(0x01);
            body.extend_from_slice(b"name\0");
            body.extend_from_slice(name.as_bytes());
            body.push(0);
            body.push(0x01);
            body.extend_from_slice(b"type\0");
            body.extend_from_slice(app_type.as_bytes());
            body.push(0);
            body.push(0x01);
            body.extend_from_slice(b"oslist\0");
            body.extend_from_slice(b"windows,linux\0");
            body.push(0x08); // end common
            body.push(0x08); // end appinfo
            body.push(0x08); // end root

            buf.extend_from_slice(&appid.to_le_bytes());
            buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
            buf.extend_from_slice(&body);
        };

        push_entry(&mut buf, 489830, "Skyrim SE", "Game");
        push_entry(&mut buf, 228980, "Steamworks Common Redistributables", "Tool");
        buf.extend_from_slice(&0u32.to_le_bytes()); // terminator

        let map = parse_appinfo_games(&buf);
        assert_eq!(map.len(), 1);
        let skyrim = map.get(&489830).unwrap();
        assert_eq!(skyrim.name, "Skyrim SE");
        assert_eq!(skyrim.oslist, vec!["windows", "linux"]);
    }

    #[test]
    fn parses_hashed_library_capsule_and_header_from_appinfo() {
        let mut buf = Vec::new();
        buf.extend_from_slice(&0x0756_4428u32.to_le_bytes());
        buf.extend_from_slice(&1u32.to_le_bytes());

        let mut body = vec![0u8; 60];
        body.push(0x00);
        body.extend_from_slice(b"appinfo\0");
        body.push(0x00);
        body.extend_from_slice(b"common\0");
        body.push(0x01);
        body.extend_from_slice(b"name\0Half-Life 2\0");
        body.push(0x01);
        body.extend_from_slice(b"type\0Game\0");
        body.push(0x00);
        body.extend_from_slice(b"library_assets_full\0");
        body.push(0x00);
        body.extend_from_slice(b"library_capsule\0");
        body.push(0x00);
        body.extend_from_slice(b"image\0");
        body.push(0x01);
        body.extend_from_slice(b"english\0abcdef1234/library_capsule.jpg\0");
        body.push(0x08); // end image
        body.push(0x08); // end library_capsule
        body.push(0x00);
        body.extend_from_slice(b"library_header\0");
        body.push(0x00);
        body.extend_from_slice(b"image\0");
        body.push(0x01);
        body.extend_from_slice(b"english\0fedcba9876/library_header.jpg\0");
        body.push(0x08); // end image
        body.push(0x08); // end library_header
        body.push(0x08); // end library_assets_full
        body.push(0x08); // end common
        body.push(0x08); // end appinfo
        body.push(0x08); // end root

        buf.extend_from_slice(&220u32.to_le_bytes());
        buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
        buf.extend_from_slice(&body);
        buf.extend_from_slice(&0u32.to_le_bytes());

        let map = parse_appinfo_games(&buf);
        let hl2 = map.get(&220).unwrap();
        assert_eq!(
            hl2.library_capsule_rel.as_deref(),
            Some("abcdef1234/library_capsule.jpg")
        );
        assert_eq!(
            hl2.library_header_rel.as_deref(),
            Some("fedcba9876/library_header.jpg")
        );

        let urls = steam_cdn_cover_urls(220, Some(hl2));
        assert_eq!(
            urls[0],
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/220/abcdef1234/library_capsule.jpg"
        );
        assert_eq!(
            urls[1],
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/220/library_600x900.jpg"
        );
        assert_eq!(
            urls[3],
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/220/fedcba9876/library_header.jpg"
        );
        assert_eq!(
            urls[4],
            "https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/220/header.jpg"
        );
    }

    #[test]
    fn local_steam_cover_finds_hashed_librarycache_and_userdata_grid() {
        let dir = tempfile::tempdir().unwrap();
        let hashed = dir
            .path()
            .join("appcache")
            .join("librarycache")
            .join("220")
            .join("3f8c1a9b0e")
            .join("library_capsule.jpg");
        fs::create_dir_all(hashed.parent().unwrap()).unwrap();
        fs::write(&hashed, b"img").unwrap();

        let found = local_steam_cover(&[dir.path().to_path_buf()], 220);
        assert_eq!(found.as_deref(), Some(hashed.to_string_lossy().as_ref()));

        let grid = dir
            .path()
            .join("userdata")
            .join("12345")
            .join("config")
            .join("grid")
            .join("570p.png");
        fs::create_dir_all(grid.parent().unwrap()).unwrap();
        fs::write(&grid, b"img").unwrap();

        let found_grid = local_steam_cover(&[dir.path().to_path_buf()], 570);
        assert_eq!(found_grid.as_deref(), Some(grid.to_string_lossy().as_ref()));
    }
}
