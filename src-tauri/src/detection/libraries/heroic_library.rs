//! Full Heroic Games Launcher library discovery (Epic / Legendary, GOG, Amazon / Nile)
//! from Heroic's local `store_cache/*.json` files.

use std::collections::HashSet;
use std::fs;
use std::path::Path;

use serde_json::Value;

use crate::detection::heroic_art::{HeroicResolver, HeroicStore};
use crate::launcher_install::find_heroic_cli_binary;
use crate::tools::heroic_launch::heroic_config_dirs;

use super::{
    finish_library_game, LibraryDlcOption, LibraryGame, LibraryGameMeta, LibrarySection,
    LibrarySource,
};

pub fn scan_heroic_libraries() -> Vec<LibrarySection> {
    let dirs = heroic_config_dirs();
    if dirs.is_empty() {
        return Vec::new();
    }
    let covers_dir = crate::config::Paths::resolve()
        .ok()
        .map(|p| p.cache_dir.join("covers"));

    let mut sections = Vec::new();
    for config_dir in dirs {
        let mut resolver = HeroicResolver::from_dir(config_dir.clone(), covers_dir.clone());

        if let Some(sec) = scan_store_in_dir(
            &config_dir,
            &mut resolver,
            LibrarySource::HeroicEpic,
            HeroicStore::Epic,
            "legendary",
            &config_dir.join("store_cache").join("legendary_library.json"),
            &config_dir
                .join("legendaryConfig")
                .join("legendary")
                .join("installed.json"),
        ) {
            sections.push(sec);
        }

        if let Some(sec) = scan_store_in_dir(
            &config_dir,
            &mut resolver,
            LibrarySource::HeroicGog,
            HeroicStore::Gog,
            "gog",
            &config_dir.join("store_cache").join("gog_library.json"),
            &config_dir.join("gog_store").join("installed.json"),
        ) {
            sections.push(sec);
        }

        if let Some(sec) = scan_store_in_dir(
            &config_dir,
            &mut resolver,
            LibrarySource::HeroicAmazon,
            HeroicStore::Amazon,
            "nile",
            &config_dir.join("store_cache").join("nile_library.json"),
            &config_dir
                .join("nile_config")
                .join("nile")
                .join("installed.json"),
        ) {
            sections.push(sec);
        }
    }
    sections
}

pub fn scan_store_in_dir(
    _config_dir: &Path,
    resolver: &mut HeroicResolver,
    source: LibrarySource,
    store: HeroicStore,
    runner: &str,
    library_path: &Path,
    installed_path: &Path,
) -> Option<LibrarySection> {
    let raw = fs::read_to_string(library_path).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;
    let items = json_array(&val);
    if items.is_empty() {
        return None;
    }

    let installed_ids = load_installed_ids(installed_path);
    let cli_bin = match runner {
        "legendary" => "legendary",
        "gog" => "gogdl",
        "nile" => "nile",
        _ => runner,
    };
    let can_cli_install = find_heroic_cli_binary(cli_bin).is_some();

    let mut total_owned = 0usize;
    let mut games: Vec<LibraryGame> = Vec::new();

    for item in items {
        if item.get("is_dlc").and_then(Value::as_bool) == Some(true) {
            continue;
        }
        if item.get("is_game").and_then(Value::as_bool) == Some(false) {
            continue;
        }
        let Some(app_name) = json_str(&item, &["app_name", "appName"]) else {
            continue;
        };
        let Some(title) = json_str(&item, &["title"]) else {
            continue;
        };
        total_owned += 1;

        let is_installed = item.get("is_installed").and_then(Value::as_bool) == Some(true)
            || installed_ids.contains(&app_name);
        if is_installed {
            continue;
        }

        let art = resolver.resolve_store(Some(store), Some(&app_name), None);
        let fallback_url = art.cover_url.or_else(|| preferred_store_url(store, &item));

        let mut platforms = vec!["windows".to_string()];
        if item.get("is_linux_native").and_then(Value::as_bool) == Some(true) {
            platforms.push("linux".to_string());
        }
        if item.get("is_mac_native").and_then(Value::as_bool) == Some(true) {
            platforms.push("macos".to_string());
        }

        let dlcs = parse_dlcs(&item);
        let languages = parse_languages(&item);
        let install_size_bytes = parse_size_bytes(&item);

        let metadata = LibraryGameMeta {
            heroic_runner: Some(runner.to_string()),
            dlcs,
            languages,
            ..LibraryGameMeta::default()
        };

        games.push(finish_library_game(
            source,
            app_name,
            title,
            art.cover_path,
            fallback_url,
            install_size_bytes,
            platforms,
            can_cli_install,
            metadata,
        ));
    }

    if total_owned == 0 {
        return None;
    }

    Some(LibrarySection {
        source,
        label: source.label().to_string(),
        total_owned,
        games,
        warning: None,
    })
}

fn load_installed_ids(path: &Path) -> HashSet<String> {
    let mut ids = HashSet::new();
    let Ok(raw) = fs::read_to_string(path) else {
        return ids;
    };
    let Ok(val) = serde_json::from_str::<Value>(&raw) else {
        return ids;
    };
    if let Some(obj) = val.as_object() {
        if obj.get("installed").is_none() && obj.get("games").is_none() {
            for key in obj.keys() {
                ids.insert(key.clone());
            }
            return ids;
        }
    }
    for item in json_array(&val) {
        if let Some(id) = json_str(&item, &["app_name", "appName", "id"]) {
            ids.insert(id);
        }
    }
    ids
}

fn json_array(value: &Value) -> Vec<Value> {
    if let Some(arr) = value.as_array() {
        return arr.clone();
    }
    for key in ["library", "games", "installed"] {
        if let Some(arr) = value.get(key).and_then(Value::as_array) {
            return arr.clone();
        }
    }
    Vec::new()
}

fn json_str(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|k| {
        value
            .get(*k)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_string)
    })
}

fn preferred_store_url(store: HeroicStore, item: &Value) -> Option<String> {
    let cover = json_str(item, &["art_cover"]).filter(|u| u.starts_with("http"));
    let square = json_str(item, &["art_square"]).filter(|u| u.starts_with("http"));
    match store {
        HeroicStore::Epic => square.or(cover),
        HeroicStore::Gog | HeroicStore::Amazon | HeroicStore::Sideload => cover.or(square),
    }
}

fn parse_dlcs(item: &Value) -> Vec<LibraryDlcOption> {
    let mut out = Vec::new();
    let arr = item
        .get("dlcList")
        .or_else(|| item.get("dlcs"))
        .and_then(Value::as_array);
    let Some(list) = arr else {
        return out;
    };
    for dlc in list {
        let Some(app_name) = json_str(dlc, &["app_name", "appName", "id"]) else {
            continue;
        };
        let title = json_str(dlc, &["title", "name"]).unwrap_or_else(|| app_name.clone());
        out.push(LibraryDlcOption {
            app_name,
            title,
            install_size_bytes: None,
        });
    }
    out
}

fn parse_languages(item: &Value) -> Vec<String> {
    let Some(arr) = item
        .get("languages")
        .or_else(|| item.get("extra").and_then(|e| e.get("languages")))
        .and_then(Value::as_array)
    else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect()
}

fn parse_size_bytes(item: &Value) -> Option<u64> {
    if let Some(n) = item
        .get("install")
        .and_then(|i| i.get("install_size"))
        .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
    {
        if n > 0 {
            return Some(n);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_heroic_epic_library_excludes_installed_and_reads_dlcs() {
        let dir = tempfile::tempdir().unwrap();
        let store_cache = dir.path().join("store_cache");
        let leg_cfg = dir.path().join("legendaryConfig").join("legendary");
        fs::create_dir_all(&store_cache).unwrap();
        fs::create_dir_all(&leg_cfg).unwrap();

        let lib_path = store_cache.join("legendary_library.json");
        fs::write(
            &lib_path,
            r#"{
                "library": [
                    {
                        "app_name": "Crab",
                        "title": "Control",
                        "is_installed": false,
                        "is_linux_native": false,
                        "art_square": "https://cdn.example/control-tall.jpg",
                        "dlcList": [
                            { "app_name": "CrabDLC1", "title": "The Foundation" }
                        ]
                    },
                    {
                        "app_name": "InstalledGame",
                        "title": "Already Installed",
                        "is_installed": false
                    },
                    {
                        "app_name": "SomeDLC",
                        "title": "Standalone DLC",
                        "is_dlc": true
                    }
                ]
            }"#,
        )
        .unwrap();

        let inst_path = leg_cfg.join("installed.json");
        fs::write(
            &inst_path,
            r#"{ "InstalledGame": { "install_path": "/games/installed" } }"#,
        )
        .unwrap();

        let mut resolver = HeroicResolver::from_dir(dir.path().to_path_buf(), None);
        let section = scan_store_in_dir(
            dir.path(),
            &mut resolver,
            LibrarySource::HeroicEpic,
            HeroicStore::Epic,
            "legendary",
            &lib_path,
            &inst_path,
        )
        .unwrap();

        assert_eq!(section.total_owned, 2);
        assert_eq!(section.games.len(), 1);
        assert_eq!(section.games[0].title, "Control");
        assert_eq!(section.games[0].external_id, "Crab");
        assert_eq!(
            section.games[0].cover_url.as_deref(),
            Some("https://cdn.example/control-tall.jpg")
        );
        assert_eq!(section.games[0].metadata.dlcs.len(), 1);
        assert_eq!(section.games[0].metadata.dlcs[0].title, "The Foundation");
    }
}
