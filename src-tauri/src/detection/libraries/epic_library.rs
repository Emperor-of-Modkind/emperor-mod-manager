//! Uninstalled Epic Games Launcher library discovery via `%PROGRAMDATA%\Epic\EpicGamesLauncher\Data\Catalog\catcache.bin`.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
#[cfg(target_os = "windows")]
use std::path::PathBuf;

use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine;
use serde_json::Value;

use super::{
    finish_library_game, LibraryGame, LibraryGameMeta, LibrarySection, LibrarySource,
};

#[cfg(target_os = "windows")]
pub fn scan_epic_library() -> Option<LibrarySection> {
    let programdata = std::env::var_os("PROGRAMDATA")?;
    let base = PathBuf::from(programdata)
        .join("Epic")
        .join("EpicGamesLauncher")
        .join("Data");
    let catcache = base.join("Catalog").join("catcache.bin");
    let manifests = base.join("Manifests");
    scan_epic_catalog(&catcache, &manifests)
}

pub fn scan_epic_catalog(catcache_path: &Path, manifests_dir: &Path) -> Option<LibrarySection> {
    let raw = fs::read_to_string(catcache_path).ok()?;
    let decoded = B64.decode(raw.trim()).ok()?;
    let items: Vec<Value> = serde_json::from_slice(&decoded).ok()?;
    if items.is_empty() {
        return None;
    }

    let installed_apps = installed_app_names(manifests_dir);
    let mut total_owned = 0usize;
    let mut games: Vec<LibraryGame> = Vec::new();
    let mut seen_apps = HashSet::new();

    for item in items {
        if !is_catalog_game(&item) {
            continue;
        }
        let Some(title) = str_field(&item, "title") else {
            continue;
        };
        let Some(namespace) = str_field(&item, "namespace") else {
            continue;
        };
        let Some(catalog_item_id) = str_field(&item, "id") else {
            continue;
        };
        let app_name = first_release_app_id(&item).unwrap_or_else(|| catalog_item_id.clone());
        if !seen_apps.insert(app_name.clone()) {
            continue;
        }
        total_owned += 1;

        if installed_apps.contains(&app_name) {
            continue;
        }

        let cover_url = tall_key_image(&item);
        let external_id = format!("{namespace}:{catalog_item_id}:{app_name}");
        let metadata = LibraryGameMeta {
            epic_namespace: Some(namespace),
            epic_catalog_item_id: Some(catalog_item_id),
            epic_app_name: Some(app_name),
            ..LibraryGameMeta::default()
        };

        games.push(finish_library_game(
            LibrarySource::EpicLauncher,
            external_id,
            title,
            None,
            cover_url,
            None,
            vec!["windows".to_string()],
            false,
            metadata,
        ));
    }

    if total_owned == 0 {
        return None;
    }

    Some(LibrarySection {
        source: LibrarySource::EpicLauncher,
        label: LibrarySource::EpicLauncher.label().to_string(),
        total_owned,
        games,
        warning: None,
    })
}

fn installed_app_names(manifests_dir: &Path) -> HashSet<String> {
    let mut out = HashSet::new();
    let Ok(entries) = fs::read_dir(manifests_dir) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("item"))
        {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else {
            continue;
        };
        let bytes = bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(&bytes);
        let Ok(val) = serde_json::from_slice::<Value>(bytes) else {
            continue;
        };
        if let Some(app_name) = str_field(&val, "AppName") {
            out.insert(app_name);
        }
    }
    out
}

fn is_catalog_game(item: &Value) -> bool {
    let Some(cats) = item.get("categories").and_then(Value::as_array) else {
        return false;
    };
    let mut has_games = false;
    for c in cats {
        let path = c.get("path").and_then(Value::as_str).unwrap_or("");
        if path.eq_ignore_ascii_case("addons")
            || path.eq_ignore_ascii_case("engines")
            || path.eq_ignore_ascii_case("plugins")
        {
            return false;
        }
        if path.eq_ignore_ascii_case("games") {
            has_games = true;
        }
    }
    has_games
}

fn first_release_app_id(item: &Value) -> Option<String> {
    item.get("releaseInfo")
        .and_then(Value::as_array)?
        .iter()
        .find_map(|r| str_field(r, "appId"))
}

fn tall_key_image(item: &Value) -> Option<String> {
    let images = item.get("keyImages").and_then(Value::as_array)?;
    for preferred in ["DieselGameBoxTall", "OfferImageTall", "Thumbnail", "DieselGameBox"] {
        for img in images {
            if img.get("type").and_then(Value::as_str) == Some(preferred) {
                if let Some(url) = str_field(img, "url") {
                    if url.starts_with("https://") || url.starts_with("http://") {
                        return Some(url);
                    }
                }
            }
        }
    }
    None
}

fn str_field(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_catcache_and_excludes_installed_manifests() {
        let dir = tempfile::tempdir().unwrap();
        let manifests = dir.path().join("Manifests");
        fs::create_dir_all(&manifests).unwrap();
        fs::write(manifests.join("installed.item"), r#"{ "AppName": "Crab" }"#).unwrap();

        let catalog_json = r#"[
            {
                "id": "item1",
                "namespace": "ns1",
                "title": "Control",
                "categories": [{"path": "games"}],
                "releaseInfo": [{"appId": "Crab"}],
                "keyImages": [{"type": "DieselGameBoxTall", "url": "https://cdn.example/control.jpg"}]
            },
            {
                "id": "item2",
                "namespace": "ns2",
                "title": "Alan Wake 2",
                "categories": [{"path": "games"}],
                "releaseInfo": [{"appId": "Wake2"}],
                "keyImages": [{"type": "DieselGameBoxTall", "url": "https://cdn.example/aw2.jpg"}]
            },
            {
                "id": "item3",
                "namespace": "ns3",
                "title": "Unreal Engine",
                "categories": [{"path": "engines"}]
            }
        ]"#;
        let catcache = dir.path().join("catcache.bin");
        fs::write(&catcache, B64.encode(catalog_json)).unwrap();

        let section = scan_epic_catalog(&catcache, &manifests).unwrap();
        assert_eq!(section.total_owned, 2);
        assert_eq!(section.games.len(), 1);
        assert_eq!(section.games[0].title, "Alan Wake 2");
        assert_eq!(section.games[0].external_id, "ns2:item2:Wake2");
        assert_eq!(
            section.games[0].cover_url.as_deref(),
            Some("https://cdn.example/aw2.jpg")
        );
    }
}
