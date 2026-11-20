//! Installed games from the Epic Games Launcher's `%PROGRAMDATA%` manifests.
//!
//! Manifest parsing takes a directory so CI can cover it without a Windows install.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::windows::normalize_windows_path_key;
use super::DetectedGame;

pub const EPIC_LAUNCHER: &str = "Epic Games Launcher";

#[cfg(target_os = "windows")]
pub fn scan_installed() -> Vec<DetectedGame> {
    let Some(dir) = epic_manifests_dir() else {
        log::info!("Epic Games Launcher manifests not found");
        return Vec::new();
    };
    games_from_manifests(&dir)
}

/// `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Manifests`
#[cfg(target_os = "windows")]
fn epic_manifests_dir() -> Option<PathBuf> {
    let programdata = std::env::var_os("PROGRAMDATA")?;
    let dir = PathBuf::from(programdata)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data")
        .join("Manifests");
    dir.is_dir().then_some(dir)
}

/// Games described by `*.item` manifests in `dir`.
///
/// A second manifest for an install folder already accepted is omitted.
pub fn games_from_manifests(dir: &Path) -> Vec<DetectedGame> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("item"))
        })
        .collect();
    files.sort();

    let mut out = Vec::new();
    let mut seen = HashSet::new();
    for path in files {
        let Some(game) = game_from_manifest(&path) else {
            continue;
        };
        if let Some(install) = game.install_path.as_deref() {
            let key = normalize_windows_path_key(Path::new(install));
            if !seen.insert(key) {
                continue;
            }
        }
        out.push(game);
    }
    out
}

fn game_from_manifest(path: &Path) -> Option<DetectedGame> {
    let value = read_manifest(path)?;
    if !is_installed_game(&value) {
        return None;
    }
    let install = json_str(&value, "InstallLocation")?;
    let install_path = PathBuf::from(&install);
    if !install_path.is_dir() {
        return None;
    }
    let title = manifest_title(&value)?;
    let mut detected = super::finish_detected(
        title,
        EPIC_LAUNCHER.to_string(),
        Some(install_path.to_string_lossy().to_string()),
        None,
    );
    detected.cover_url = vault_thumbnail(&value);
    Some(detected)
}

fn read_manifest(path: &Path) -> Option<Value> {
    let bytes = fs::read(path).ok()?;
    let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
    let text = String::from_utf8(bytes.to_vec()).ok()?;
    serde_json::from_str(&text).ok()
}

fn is_installed_game(value: &Value) -> bool {
    if value
        .get("bIsIncompleteInstall")
        .and_then(|flag| flag.as_bool())
        == Some(true)
    {
        return false;
    }
    match categories(value) {
        Some(categories) => categories
            .iter()
            .any(|category| category.eq_ignore_ascii_case("games")),
        None => launch_executable(value).is_some(),
    }
}

/// `AppCategories` when present, otherwise a comma-separated `TechnicalType`.
///
/// `None` means neither field was set, so the caller can fall back to
/// `LaunchExecutable`.
fn categories(value: &Value) -> Option<Vec<String>> {
    if let Some(categories) = value.get("AppCategories").filter(|value| !value.is_null()) {
        return Some(category_list(categories));
    }
    value
        .get("TechnicalType")
        .filter(|value| !value.is_null())
        .map(category_list)
}

fn category_list(value: &Value) -> Vec<String> {
    if let Some(list) = value.as_array() {
        return list
            .iter()
            .filter_map(|item| item.as_str())
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect();
    }
    value
        .as_str()
        .map(|text| {
            text.split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

fn launch_executable(value: &Value) -> Option<String> {
    json_str(value, "LaunchExecutable")
}

fn manifest_title(value: &Value) -> Option<String> {
    json_str(value, "DisplayName").or_else(|| json_str(value, "AppName"))
}

fn vault_thumbnail(value: &Value) -> Option<String> {
    let url = json_str(value, "VaultThumbnailUrl")?;
    if url.starts_with("https://") || url.starts_with("http://") {
        Some(url)
    } else {
        None
    }
}

fn json_str(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|item| item.as_str())
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_item(dir: &Path, name: &str, body: &str) {
        fs::write(dir.join(name), body).unwrap();
    }

    #[test]
    fn detects_a_game_from_display_name_and_thumbnail() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("Control");
        fs::create_dir_all(&install).unwrap();
        let install_text = install.to_string_lossy().replace('\\', "\\\\");
        write_item(
            root.path(),
            "control.item",
            &format!(
                r#"{{
                    "DisplayName": "Control",
                    "AppName": "Crab",
                    "InstallLocation": "{install_text}",
                    "AppCategories": ["games", "applications"],
                    "bIsIncompleteInstall": false,
                    "LaunchExecutable": "Control.exe",
                    "VaultThumbnailUrl": "https://cdn1.epicgames.com/control-tall.jpg"
                }}"#
            ),
        );

        let games = games_from_manifests(root.path());
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Control");
        assert_eq!(games[0].launcher, EPIC_LAUNCHER);
        assert_eq!(
            games[0].install_path.as_deref(),
            Some(install.to_string_lossy().as_ref())
        );
        assert!(games[0].cover_path.is_none());
        assert_eq!(
            games[0].cover_url.as_deref(),
            Some("https://cdn1.epicgames.com/control-tall.jpg")
        );
    }

    #[test]
    fn skips_dlc_engines_and_incomplete_installs() {
        let root = tempfile::tempdir().unwrap();
        for dir in ["DLC", "Engine", "Partial"] {
            fs::create_dir_all(root.path().join(dir)).unwrap();
        }
        let dlc = root
            .path()
            .join("DLC")
            .to_string_lossy()
            .replace('\\', "\\\\");
        let engine = root
            .path()
            .join("Engine")
            .to_string_lossy()
            .replace('\\', "\\\\");
        let partial = root
            .path()
            .join("Partial")
            .to_string_lossy()
            .replace('\\', "\\\\");
        write_item(
            root.path(),
            "dlc.item",
            &format!(
                r#"{{
                    "DisplayName": "Control DLC",
                    "AppName": "ControlDLC",
                    "InstallLocation": "{dlc}",
                    "AppCategories": ["addons"]
                }}"#
            ),
        );
        write_item(
            root.path(),
            "engine.item",
            &format!(
                r#"{{
                    "DisplayName": "Unreal Engine",
                    "AppName": "UE_5.4",
                    "InstallLocation": "{engine}",
                    "AppCategories": ["engines"]
                }}"#
            ),
        );
        write_item(
            root.path(),
            "partial.item",
            &format!(
                r#"{{
                    "DisplayName": "Half Installed",
                    "AppName": "Half",
                    "InstallLocation": "{partial}",
                    "AppCategories": ["games"],
                    "bIsIncompleteInstall": true,
                    "LaunchExecutable": "Game.exe"
                }}"#
            ),
        );

        assert!(games_from_manifests(root.path()).is_empty());
    }

    #[test]
    fn omits_a_second_manifest_for_the_same_install() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("Hades");
        fs::create_dir_all(&install).unwrap();
        let install_text = install.to_string_lossy().replace('\\', "\\\\");
        let body = |title: &str| {
            format!(
                r#"{{
                    "DisplayName": "{title}",
                    "AppName": "{title}",
                    "InstallLocation": "{install_text}",
                    "AppCategories": ["games"]
                }}"#
            )
        };
        write_item(root.path(), "a.item", &body("Hades"));
        write_item(root.path(), "b.item", &body("Hades Again"));

        let games = games_from_manifests(root.path());
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Hades");
        assert_eq!(
            games[0].install_path.as_deref(),
            Some(install.to_string_lossy().as_ref())
        );
    }

    #[test]
    fn ignores_a_thumbnail_that_is_not_http() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("Game");
        fs::create_dir_all(&install).unwrap();
        let install_text = install.to_string_lossy().replace('\\', "\\\\");
        write_item(
            root.path(),
            "game.item",
            &format!(
                r#"{{
                    "DisplayName": "Local Art",
                    "InstallLocation": "{install_text}",
                    "AppCategories": ["games"],
                    "VaultThumbnailUrl": "C:\\\\art.jpg"
                }}"#
            ),
        );

        let games = games_from_manifests(root.path());
        assert_eq!(games.len(), 1);
        assert!(games[0].cover_url.is_none());
    }

    #[test]
    fn launch_executable_counts_when_categories_are_missing() {
        let root = tempfile::tempdir().unwrap();
        let install = root.path().join("Legacy");
        fs::create_dir_all(&install).unwrap();
        let install_text = install.to_string_lossy().replace('\\', "\\\\");
        write_item(
            root.path(),
            "legacy.item",
            &format!(
                r#"{{
                    "AppName": "LegacyGame",
                    "InstallLocation": "{install_text}",
                    "LaunchExecutable": "Binaries\\Win64\\Game.exe"
                }}"#
            ),
        );
        write_item(
            root.path(),
            "empty.item",
            &format!(
                r#"{{
                    "DisplayName": "No Exe",
                    "InstallLocation": "{install_text}",
                    "LaunchExecutable": "  "
                }}"#
            ),
        );

        let games = games_from_manifests(root.path());
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "LegacyGame");
    }

    #[test]
    fn technical_type_is_used_when_categories_are_absent() {
        let root = tempfile::tempdir().unwrap();
        let game_dir = root.path().join("Game");
        let plugin_dir = root.path().join("Plugin");
        fs::create_dir_all(&game_dir).unwrap();
        fs::create_dir_all(&plugin_dir).unwrap();
        let game = game_dir.to_string_lossy().replace('\\', "\\\\");
        let plugin = plugin_dir.to_string_lossy().replace('\\', "\\\\");
        write_item(
            root.path(),
            "game.item",
            &format!(
                r#"{{
                    "DisplayName": "Typed Game",
                    "InstallLocation": "{game}",
                    "TechnicalType": "games,applications",
                    "LaunchExecutable": ""
                }}"#
            ),
        );
        write_item(
            root.path(),
            "plugin.item",
            &format!(
                r#"{{
                    "DisplayName": "Typed Plugin",
                    "InstallLocation": "{plugin}",
                    "TechnicalType": "plugins",
                    "LaunchExecutable": "Plugin.exe"
                }}"#
            ),
        );

        let games = games_from_manifests(root.path());
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Typed Game");
    }
}
