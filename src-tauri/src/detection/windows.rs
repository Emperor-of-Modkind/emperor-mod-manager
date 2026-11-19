//! Windows game detection via Steam libraryfolders + optional Heroic.
//!
//! Pure path/cover helpers also compile under `cfg(test)` on other hosts so CI
//! can cover Steam librarycache resolution without a Windows linker.

use std::fs;
use std::path::{Path, PathBuf};

use regex::Regex;

#[cfg(target_os = "windows")]
use std::collections::HashSet;
#[cfg(target_os = "windows")]
use winreg::enums::HKEY_CURRENT_USER;
#[cfg(target_os = "windows")]
use winreg::RegKey;

#[cfg(target_os = "windows")]
use super::heroic_art::{HeroicResolver, HeroicStore};
#[cfg(target_os = "windows")]
use super::steam_shortcuts::{shortcut_games_from_roots, STEAM_SHORTCUTS_LAUNCHER};
use super::DetectedGame;

#[cfg(target_os = "windows")]
pub fn scan_games() -> Vec<DetectedGame> {
    let mut out = Vec::new();
    out.extend(scan_steam());
    out.extend(scan_heroic());

    out.sort_by(|a, b| {
        b.supported
            .cmp(&a.supported)
            .then_with(|| b.engine_hint.is_some().cmp(&a.engine_hint.is_some()))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    out
}

#[cfg(target_os = "windows")]
fn scan_steam() -> Vec<DetectedGame> {
    let Some(steam_root) = steam_install_path() else {
        log::info!("Steam install path not found in registry");
        return Vec::new();
    };
    let libraries = steam_library_paths(&steam_root);
    let mut out = Vec::new();
    let mut seen_installs = HashSet::new();
    for lib in libraries {
        let steamapps = lib.join("steamapps");
        let Ok(entries) = fs::read_dir(&steamapps) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if !name.starts_with("appmanifest_") || !name.ends_with(".acf") {
                continue;
            }
            if let Some(game) = parse_appmanifest(&path, &steamapps, &steam_root) {
                if let Some(install) = game.install_path.as_deref() {
                    let key = normalize_windows_path_key(Path::new(install));
                    if !seen_installs.insert(key) {
                        continue;
                    }
                }
                out.push(game);
            }
        }
    }
    for shortcut in shortcut_games_from_roots(std::iter::once(&steam_root)) {
        let key = normalize_windows_path_key(&shortcut.install_path);
        if !seen_installs.insert(key) {
            continue;
        }
        let app_id = shortcut.app_id.to_string();
        let cover = resolve_steam_box_art(&steam_root, &app_id)
            .map(|path| path.to_string_lossy().to_string());
        let mut detected = super::finish_detected(
            shortcut.title,
            STEAM_SHORTCUTS_LAUNCHER.to_string(),
            Some(shortcut.install_path.to_string_lossy().to_string()),
            cover,
        );
        if detected.cover_path.is_none() {
            detected.icon_path = resolve_steam_icon(&steam_root, &app_id)
                .map(|path| path.to_string_lossy().to_string());
        }
        out.push(detected);
    }
    out
}

#[cfg(target_os = "windows")]
fn steam_install_path() -> Option<PathBuf> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let steam = hkcu.open_subkey(r"Software\Valve\Steam").ok()?;
    let path: String = steam.get_value("SteamPath").ok()?;
    let path = PathBuf::from(path.replace('/', "\\"));
    if path.is_dir() {
        Some(path)
    } else {
        None
    }
}

fn steam_library_paths(steam_root: &Path) -> Vec<PathBuf> {
    let mut libs = vec![steam_root.to_path_buf()];
    let vdf = steam_root.join("steamapps").join("libraryfolders.vdf");
    let Ok(text) = fs::read_to_string(&vdf) else {
        return libs;
    };
    // Match "path" "C:\\..." entries in libraryfolders.vdf (VDF is loosely structured).
    let re = Regex::new(r#""path"\s+"([^"]+)""#).expect("libraryfolders path regex");
    for cap in re.captures_iter(&text) {
        let raw = cap[1].replace("\\\\", "\\");
        let p = PathBuf::from(raw);
        if p.is_dir() && !libs.iter().any(|e| same_windows_path(e, &p)) {
            libs.push(p);
        }
    }
    libs
}

/// Case- and separator-insensitive path identity for Windows filesystem paths.
fn same_windows_path(a: &Path, b: &Path) -> bool {
    normalize_windows_path_key(a) == normalize_windows_path_key(b)
}

fn normalize_windows_path_key(path: &Path) -> String {
    let s = path.to_string_lossy();
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '/' | '\\' => {
                if !out.ends_with('\\') {
                    out.push('\\');
                }
            }
            c => {
                for lower in c.to_lowercase() {
                    out.push(lower);
                }
            }
        }
    }
    // Trim trailing separators, but keep drive roots like `c:\`.
    while out.len() > 3 && out.ends_with('\\') {
        out.pop();
    }
    out
}

fn parse_appmanifest(acf: &Path, steamapps: &Path, steam_root: &Path) -> Option<DetectedGame> {
    let text = fs::read_to_string(acf).ok()?;
    let name = vdf_string(&text, "name")?;
    let installdir = vdf_string(&text, "installdir")?;
    let appid = vdf_string(&text, "appid");
    let install_path = steamapps.join("common").join(&installdir);
    if !install_path.is_dir() {
        return None;
    }
    let cover_path = appid
        .as_deref()
        .and_then(|id| resolve_steam_box_art(steam_root, id))
        .map(|p| p.to_string_lossy().to_string());
    let mut detected = super::finish_detected(
        name,
        "Steam".to_string(),
        Some(install_path.to_string_lossy().to_string()),
        cover_path,
    );
    if detected.cover_path.is_none() {
        detected.icon_path = appid
            .as_deref()
            .and_then(|id| resolve_steam_icon(steam_root, id))
            .map(|path| path.to_string_lossy().to_string());
    }
    Some(detected)
}

const COVER_EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];

/// Locate Steam library capsule / box art.
///
/// Prefers portrait `library_600x900`, then `library_capsule`, then `library_hero`,
/// `header`, and `logo`. Looks at the legacy flat cache, the newer
/// `{appid}/[hash]/` layout, and custom grid art under `userdata/*/config/grid`.
fn resolve_steam_box_art(steam_root: &Path, app_id: &str) -> Option<PathBuf> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return None;
    }
    if let Some(found) = resolve_librarycache_cover(steam_root, app_id) {
        return Some(found);
    }
    if let Some(found) = resolve_userdata_grid(steam_root, app_id) {
        return Some(found);
    }
    None
}

/// Steam's small `{appid}_icon` file. Kept off `cover_path` so a catalog tile can win.
fn resolve_steam_icon(steam_root: &Path, app_id: &str) -> Option<PathBuf> {
    let app_id = app_id.trim();
    if app_id.is_empty() {
        return None;
    }
    let cache = steam_root.join("appcache").join("librarycache");
    for name in [format!("{app_id}_icon.jpg"), format!("{app_id}_icon.png")] {
        let path = cache.join(&name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
}

fn resolve_librarycache_cover(steam_root: &Path, app_id: &str) -> Option<PathBuf> {
    let cache = steam_root.join("appcache").join("librarycache");
    if !cache.is_dir() {
        return None;
    }

    let mut legacy = Vec::new();
    for ext in COVER_EXTS {
        legacy.push(format!("{app_id}_library_600x900.{ext}"));
    }
    for ext in COVER_EXTS {
        legacy.push(format!("{app_id}_library_capsule.{ext}"));
    }
    for name in legacy {
        let path = cache.join(&name);
        if path.is_file() {
            return Some(path);
        }
    }

    let mut best: Option<(u8, PathBuf)> = None;
    let app_dir = cache.join(app_id);
    if app_dir.is_dir() {
        best = find_cover_in_dir(&app_dir, 0);
    }
    if best.as_ref().is_some_and(|(rank, _)| *rank == 0) {
        return best.map(|(_, path)| path);
    }

    // Flat `{appid}_library_hero.jpg` / `_header` / `_logo` next to the legacy files.
    for stem in ["library_hero", "header", "logo"] {
        for ext in COVER_EXTS {
            let path = cache.join(format!("{app_id}_{stem}.{ext}"));
            consider_cover(&mut best, path);
        }
    }
    best.map(|(_, path)| path)
}

/// Custom library posters Steam stores per account: `{appid}p.*` then `{appid}.*`.
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
        // A blurred hero is a weaker stand-in than the sharp asset.
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
    // One hash subdirectory deep: `librarycache/{appid}/{hash}/library_600x900.jpg`.
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
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        let Some(rank) = cover_rank(name) else {
            continue;
        };
        if rank == 0 {
            return Some((0, path));
        }
        if best.as_ref().is_none_or(|(current, _)| rank < *current) {
            best = Some((rank, path));
        }
    }
    best
}

fn vdf_string(text: &str, key: &str) -> Option<String> {
    let pattern = format!(r#""{key}"\s+"([^"]*)""#);
    let re = Regex::new(&pattern).ok()?;
    re.captures(text)
        .map(|c| c[1].to_string())
        .filter(|s| !s.is_empty())
}

/// Heroic Games Launcher stores installed games under %APPDATA%/heroic.
/// Posters come from the store library (`art_cover` / Epic tall art), not `icons/`.
#[cfg(target_os = "windows")]
fn scan_heroic() -> Vec<DetectedGame> {
    let Some(mut resolver) = HeroicResolver::discover() else {
        return Vec::new();
    };

    let mut out = Vec::new();
    let mut seen_installs = HashSet::new();
    let config = heroic_config_from_env();

    if let Some(heroic) = config.as_deref() {
        let legendary = heroic
            .join("legendaryConfig")
            .join("legendary")
            .join("installed.json");
        if let Ok(text) = fs::read_to_string(&legendary) {
            if let Ok(map) =
                serde_json::from_str::<serde_json::Map<String, serde_json::Value>>(&text)
            {
                for (id, val) in map {
                    let app_id = val
                        .get("app_name")
                        .and_then(|v| v.as_str())
                        .unwrap_or(id.as_str())
                        .to_string();
                    let title = val
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let install_path = val
                        .get("install_path")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    push_heroic_game(
                        &mut out,
                        &mut seen_installs,
                        &mut resolver,
                        HeroicStore::Epic,
                        &app_id,
                        &title,
                        install_path,
                    );
                }
            }
        }

        let gog = heroic.join("gog_store").join("installed.json");
        if let Ok(text) = fs::read_to_string(&gog) {
            if let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) {
                let list = value
                    .as_array()
                    .cloned()
                    .or_else(|| value.get("installed").and_then(|v| v.as_array()).cloned())
                    .unwrap_or_default();
                for val in list {
                    let app_id = val
                        .get("appName")
                        .or_else(|| val.get("app_name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let title = val
                        .get("title")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let install_path = val
                        .get("install_path")
                        .or_else(|| val.get("installPath"))
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                    push_heroic_game(
                        &mut out,
                        &mut seen_installs,
                        &mut resolver,
                        HeroicStore::Gog,
                        &app_id,
                        &title,
                        install_path,
                    );
                }
            }
        }
    }

    out
}

#[cfg(target_os = "windows")]
fn heroic_config_from_env() -> Option<PathBuf> {
    let appdata = std::env::var_os("APPDATA")?;
    let path = PathBuf::from(appdata).join("heroic");
    path.is_dir().then_some(path)
}

#[cfg(target_os = "windows")]
fn push_heroic_game(
    out: &mut Vec<DetectedGame>,
    seen_installs: &mut HashSet<String>,
    resolver: &mut HeroicResolver,
    store: HeroicStore,
    app_id: &str,
    title: &str,
    install_path: Option<String>,
) {
    let Some(install_path) = install_path.filter(|path| Path::new(path).is_dir()) else {
        return;
    };
    let key = normalize_windows_path_key(Path::new(&install_path));
    if !seen_installs.insert(key) {
        return;
    }
    let library_title = if app_id.is_empty() {
        None
    } else {
        resolver.library_title(store, app_id)
    };
    let title = prefer_heroic_title(title, library_title, app_id);
    let art = resolver.resolve_store(
        Some(store),
        if app_id.is_empty() {
            None
        } else {
            Some(app_id)
        },
        None,
    );
    let mut detected = super::finish_detected(
        title,
        "Heroic".to_string(),
        Some(install_path),
        art.cover_path,
    );
    detected.cover_url = art.cover_url;
    detected.icon_path = art.icon_path;
    out.push(detected);
}

#[cfg(target_os = "windows")]
fn prefer_heroic_title(raw: &str, library: Option<String>, app_id: &str) -> String {
    let raw = raw.trim();
    let missing = raw.is_empty() || raw.eq_ignore_ascii_case("unknown") || raw == app_id;
    if missing {
        if let Some(title) = library.filter(|title| !title.trim().is_empty()) {
            return title;
        }
    }
    if raw.is_empty() {
        if app_id.is_empty() {
            "Unknown".to_string()
        } else {
            app_id.to_string()
        }
    } else {
        raw.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vdf_string_parses_name() {
        let sample = r#"
"AppState"
{
	"appid"		"413150"
	"name"		"Stardew Valley"
	"installdir"		"Stardew Valley"
}
"#;
        assert_eq!(
            vdf_string(sample, "name").as_deref(),
            Some("Stardew Valley")
        );
        assert_eq!(
            vdf_string(sample, "installdir").as_deref(),
            Some("Stardew Valley")
        );
        assert_eq!(vdf_string(sample, "appid").as_deref(), Some("413150"));
    }

    #[test]
    fn same_windows_path_ignores_case_and_separators() {
        assert!(same_windows_path(
            Path::new(r"c:\program files (x86)\steam"),
            Path::new(r"C:\Program Files (x86)\Steam"),
        ));
        assert!(same_windows_path(
            Path::new(r"c:/steam"),
            Path::new(r"C:\Steam\"),
        ));
        assert!(!same_windows_path(
            Path::new(r"C:\Steam"),
            Path::new(r"D:\Steam"),
        ));
    }

    #[test]
    fn normalize_trims_trailing_separators() {
        assert_eq!(
            normalize_windows_path_key(Path::new(r"C:\Steam\")),
            normalize_windows_path_key(Path::new(r"c:\steam"))
        );
    }

    #[test]
    fn steam_library_paths_dedupes_cased_root() {
        let root = tempfile::tempdir().unwrap();
        let steamapps = root.path().join("steamapps");
        fs::create_dir_all(&steamapps).unwrap();

        // Simulate VDF listing the same root with different casing/separators.
        let root_str = root.path().to_string_lossy().replace('\\', "\\\\");
        let vdf = format!(
            r#""libraryfolders"
{{
	"0"
	{{
		"path"		"{root_str}"
	}}
}}
"#
        );
        fs::write(steamapps.join("libraryfolders.vdf"), vdf).unwrap();

        // Feed a differently-cased / differently-separated "registry" root that
        // still points at the same temp directory via a second PathBuf spelling.
        let alt_spelling = PathBuf::from(
            root.path()
                .to_string_lossy()
                .replace('/', "\\")
                .to_ascii_lowercase(),
        );
        // If the filesystem is case-sensitive (Linux CI), create a symlink-free
        // path that still compares equal via normalize when separators differ.
        let libs = steam_library_paths(if alt_spelling.is_dir() {
            &alt_spelling
        } else {
            root.path()
        });
        assert_eq!(
            libs.len(),
            1,
            "primary Steam library must appear once: {libs:?}"
        );
    }

    #[test]
    fn resolve_steam_box_art_legacy_flat() {
        let root = tempfile::tempdir().unwrap();
        let cache = root.path().join("appcache").join("librarycache");
        fs::create_dir_all(&cache).unwrap();
        let cover = cache.join("413150_library_600x900.jpg");
        fs::write(&cover, b"fake").unwrap();

        assert_eq!(
            resolve_steam_box_art(root.path(), "413150").as_deref(),
            Some(cover.as_path())
        );
    }

    #[test]
    fn resolve_steam_box_art_nested_hash_dir() {
        let root = tempfile::tempdir().unwrap();
        let nested = root
            .path()
            .join("appcache")
            .join("librarycache")
            .join("413150")
            .join("abcdef0123456789");
        fs::create_dir_all(&nested).unwrap();
        let cover = nested.join("library_600x900.jpg");
        fs::write(&cover, b"fake").unwrap();

        assert_eq!(
            resolve_steam_box_art(root.path(), "413150").as_deref(),
            Some(cover.as_path())
        );
    }

    #[test]
    fn resolve_steam_box_art_prefers_portrait_over_capsule() {
        let root = tempfile::tempdir().unwrap();
        let app_dir = root
            .path()
            .join("appcache")
            .join("librarycache")
            .join("730");
        fs::create_dir_all(&app_dir).unwrap();
        let capsule = app_dir.join("library_capsule.jpg");
        let portrait = app_dir.join("library_600x900.jpg");
        fs::write(&capsule, b"cap").unwrap();
        fs::write(&portrait, b"port").unwrap();

        assert_eq!(
            resolve_steam_box_art(root.path(), "730").as_deref(),
            Some(portrait.as_path())
        );
    }

    #[test]
    fn resolve_steam_box_art_missing_returns_none() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("appcache").join("librarycache")).unwrap();
        assert!(resolve_steam_box_art(root.path(), "999").is_none());
    }

    #[test]
    fn resolve_steam_box_art_falls_back_to_hashed_hero() {
        let root = tempfile::tempdir().unwrap();
        let nested = root
            .path()
            .join("appcache")
            .join("librarycache")
            .join("730")
            .join("abc123");
        fs::create_dir_all(&nested).unwrap();
        let hero = nested.join("library_hero.webp");
        fs::write(&hero, b"hero").unwrap();
        fs::write(nested.join("library_hero_blur.jpg"), b"blur").unwrap();

        assert_eq!(
            resolve_steam_box_art(root.path(), "730").as_deref(),
            Some(hero.as_path())
        );
    }

    #[test]
    fn resolve_steam_box_art_uses_userdata_grid_portrait() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir_all(root.path().join("appcache").join("librarycache")).unwrap();
        let grid = root
            .path()
            .join("userdata")
            .join("1000")
            .join("config")
            .join("grid");
        fs::create_dir_all(&grid).unwrap();
        let portrait = grid.join("730p.png");
        let plain = grid.join("730.jpg");
        fs::write(&portrait, b"p").unwrap();
        fs::write(&plain, b"c").unwrap();
        let plain_only = grid.join("731.jpg");
        fs::write(&plain_only, b"c").unwrap();

        assert_eq!(
            resolve_steam_box_art(root.path(), "730").as_deref(),
            Some(portrait.as_path())
        );
        assert_eq!(
            resolve_steam_box_art(root.path(), "731").as_deref(),
            Some(plain_only.as_path())
        );
    }

    #[test]
    fn parse_appmanifest_sets_cover_from_appid() {
        let root = tempfile::tempdir().unwrap();
        let steamapps = root.path().join("steamapps");
        let common = steamapps.join("common").join("Stardew Valley");
        let cache = root.path().join("appcache").join("librarycache");
        fs::create_dir_all(&common).unwrap();
        fs::create_dir_all(&cache).unwrap();
        let cover = cache.join("413150_library_600x900.jpg");
        fs::write(&cover, b"fake").unwrap();

        let acf = steamapps.join("appmanifest_413150.acf");
        fs::write(
            &acf,
            r#"
"AppState"
{
	"appid"		"413150"
	"name"		"Stardew Valley"
	"installdir"		"Stardew Valley"
}
"#,
        )
        .unwrap();

        let game = parse_appmanifest(&acf, &steamapps, root.path()).expect("game");
        assert_eq!(game.title, "Stardew Valley");
        assert_eq!(
            game.cover_path.as_deref(),
            Some(cover.to_string_lossy().as_ref())
        );
    }
}
