//! Heroic Games Launcher app resolution and GamesConfig/<appName>.json syncing.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};
use serde_json::{json, Map, Value};

use crate::config::LaunchEnvVar;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeroicApp {
    pub config_dir: PathBuf,
    pub runner: String,
    pub app_name: String,
}

#[derive(Debug, Clone, Default)]
pub struct HeroicLaunchConfig {
    pub env_vars: Vec<LaunchEnvVar>,
    pub wrapper: String,
    pub args: String,
}

pub fn heroic_config_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let p = PathBuf::from(appdata).join("heroic");
            if p.is_dir() {
                dirs.push(p);
            }
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            let config_home = std::env::var_os("XDG_CONFIG_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".config"));
            let native = config_home.join("heroic");
            if native.is_dir() {
                dirs.push(native);
            }
            let flatpak = home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic");
            if flatpak.is_dir() && !dirs.contains(&flatpak) {
                dirs.push(flatpak);
            }
        }
    }
    dirs
}

fn norm_path(p: &str) -> String {
    let trimmed = p.trim().trim_end_matches(['/', '\\']).replace('\\', "/");
    #[cfg(target_os = "windows")]
    {
        trimmed.to_lowercase()
    }
    #[cfg(not(target_os = "windows"))]
    {
        trimmed
    }
}

fn path_matches(candidate: &str, install_norm: &str) -> bool {
    let cand_norm = norm_path(candidate);
    !cand_norm.is_empty() && cand_norm == install_norm
}

pub fn resolve_heroic_app_in_dir(config_dir: &Path, install_path: &str) -> Option<HeroicApp> {
    let install_norm = norm_path(install_path);
    if install_norm.is_empty() {
        return None;
    }

    // 1. GOG: gog_store/installed.json -> { "installed": [ { "appName": "...", "install_path": "..." } ] }
    let gog_path = config_dir.join("gog_store/installed.json");
    if let Ok(raw) = fs::read_to_string(&gog_path) {
        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
            if let Some(arr) = val.get("installed").and_then(Value::as_array) {
                for item in arr {
                    let ip = item
                        .get("install_path")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if path_matches(ip, &install_norm) {
                        if let Some(app_name) = item.get("appName").and_then(Value::as_str) {
                            return Some(HeroicApp {
                                config_dir: config_dir.to_path_buf(),
                                runner: "gog".to_string(),
                                app_name: app_name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 2. Epic (legendary): legendaryConfig/legendary/installed.json -> { "<appName>": { "install_path": "..." } }
    let epic_path = config_dir.join("legendaryConfig/legendary/installed.json");
    if let Ok(raw) = fs::read_to_string(&epic_path) {
        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
            if let Some(obj) = val.as_object() {
                for (app_name, item) in obj {
                    let ip = item
                        .get("install_path")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    if path_matches(ip, &install_norm) {
                        return Some(HeroicApp {
                            config_dir: config_dir.to_path_buf(),
                            runner: "legendary".to_string(),
                            app_name: app_name.clone(),
                        });
                    }
                }
            }
        }
    }

    // 3. Amazon (nile): nile_config/nile/installed.json
    let nile_path = config_dir.join("nile_config/nile/installed.json");
    if let Ok(raw) = fs::read_to_string(&nile_path) {
        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
            if let Some(arr) = val.as_array() {
                for item in arr {
                    let ip = item.get("path").and_then(Value::as_str).unwrap_or("");
                    if path_matches(ip, &install_norm) {
                        if let Some(app_name) = item.get("id").and_then(Value::as_str) {
                            return Some(HeroicApp {
                                config_dir: config_dir.to_path_buf(),
                                runner: "nile".to_string(),
                                app_name: app_name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    // 4. Sideload: sideload_apps/library.json -> { "games": [ { "app_name": "...", "folder_name": "..." } ] }
    let sideload_path = config_dir.join("sideload_apps/library.json");
    if let Ok(raw) = fs::read_to_string(&sideload_path) {
        if let Ok(val) = serde_json::from_str::<Value>(&raw) {
            if let Some(arr) = val.get("games").and_then(Value::as_array) {
                for item in arr {
                    let ip = item
                        .get("folder_name")
                        .or_else(|| item.get("install").and_then(|i| i.get("executable")))
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    let matches = path_matches(ip, &install_norm)
                        || Path::new(ip)
                            .parent()
                            .and_then(|p| p.to_str())
                            .is_some_and(|p| path_matches(p, &install_norm));
                    if matches {
                        if let Some(app_name) = item.get("app_name").and_then(Value::as_str) {
                            return Some(HeroicApp {
                                config_dir: config_dir.to_path_buf(),
                                runner: "sideload".to_string(),
                                app_name: app_name.to_string(),
                            });
                        }
                    }
                }
            }
        }
    }

    None
}

pub fn resolve_heroic_app(install_path: &str) -> Option<HeroicApp> {
    for dir in heroic_config_dirs() {
        if let Some(app) = resolve_heroic_app_in_dir(&dir, install_path) {
            return Some(app);
        }
    }
    None
}

pub fn games_config_path(config_dir: &Path, app_name: &str) -> PathBuf {
    config_dir.join("GamesConfig").join(format!("{app_name}.json"))
}

pub fn read_heroic_wine_prefix(config_dir: &Path, app_name: &str) -> Option<String> {
    let path = games_config_path(config_dir, app_name);
    let raw = fs::read_to_string(&path).ok()?;
    let val: Value = serde_json::from_str(&raw).ok()?;
    let prefix = val
        .get(app_name)
        .and_then(|s| s.get("winePrefix"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())?;
    Some(prefix.to_string())
}

pub fn read_heroic_launch_options(config_dir: &Path, app_name: &str) -> Result<HeroicLaunchConfig> {
    let path = games_config_path(config_dir, app_name);
    if !path.is_file() {
        return Ok(HeroicLaunchConfig::default());
    }
    let raw = fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
    let val: Value = serde_json::from_str(&raw).unwrap_or_else(|_| json!({}));
    let Some(section) = val.get(app_name).and_then(Value::as_object) else {
        return Ok(HeroicLaunchConfig::default());
    };

    let mut env_vars = Vec::new();
    if let Some(arr) = section.get("enviromentOptions").and_then(Value::as_array) {
        for item in arr {
            if let (Some(k), Some(v)) = (
                item.get("key").and_then(Value::as_str),
                item.get("value").and_then(Value::as_str),
            ) {
                if !k.trim().is_empty() {
                    env_vars.push(LaunchEnvVar {
                        name: k.to_string(),
                        value: v.to_string(),
                    });
                }
            }
        }
    }

    let wrapper = section
        .get("wrapperOptions")
        .and_then(Value::as_array)
        .and_then(|arr| arr.first())
        .and_then(|w| w.get("exe").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();

    let args = section
        .get("launcherArgs")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    Ok(HeroicLaunchConfig {
        env_vars,
        wrapper,
        args,
    })
}

pub fn merge_heroic_config_value(
    root: &mut Value,
    app_name: &str,
    env_vars: &[LaunchEnvVar],
    wrapper: Option<&str>,
    args: Option<&str>,
) {
    if !root.is_object() {
        *root = json!({});
    }
    let root_obj = root.as_object_mut().unwrap();
    let entry = root_obj
        .entry(app_name.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    let section = entry.as_object_mut().unwrap();

    // Merge enviromentOptions (note Heroic's spelling)
    let mut existing_env: Vec<(String, String)> = section
        .get("enviromentOptions")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|item| {
                    let k = item.get("key").and_then(Value::as_str)?.to_string();
                    let v = item.get("value").and_then(Value::as_str).unwrap_or("").to_string();
                    Some((k, v))
                })
                .collect()
        })
        .unwrap_or_default();

    for var in env_vars {
        let key = var.name.trim();
        if key.is_empty() {
            continue;
        }
        if let Some(pos) = existing_env.iter().position(|(k, _)| k == key) {
            existing_env[pos].1 = var.value.clone();
        } else {
            existing_env.push((key.to_string(), var.value.clone()));
        }
    }

    section.insert(
        "enviromentOptions".to_string(),
        Value::Array(
            existing_env
                .into_iter()
                .map(|(k, v)| json!({ "key": k, "value": v }))
                .collect(),
        ),
    );

    if let Some(w) = wrapper.map(str::trim).filter(|s| !s.is_empty()) {
        section.insert(
            "wrapperOptions".to_string(),
            json!([{ "exe": w, "args": "" }]),
        );
    }

    if let Some(a) = args.map(str::trim).filter(|s| !s.is_empty()) {
        section.insert("launcherArgs".to_string(), Value::String(a.to_string()));
    }
}

pub fn write_heroic_launch_options(
    config_dir: &Path,
    app_name: &str,
    env_vars: &[LaunchEnvVar],
    wrapper: Option<&str>,
    args: Option<&str>,
) -> Result<()> {
    let path = games_config_path(config_dir, app_name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut root = if path.is_file() {
        let raw = fs::read_to_string(&path)?;
        serde_json::from_str::<Value>(&raw).unwrap_or_else(|_| json!({}))
    } else {
        json!({})
    };
    merge_heroic_config_value(&mut root, app_name, env_vars, wrapper, args);
    fs::write(&path, serde_json::to_string_pretty(&root)?)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn resolves_gog_app_from_installed_json() {
        let tmp = tempdir().unwrap();
        let gog_dir = tmp.path().join("gog_store");
        fs::create_dir_all(&gog_dir).unwrap();
        fs::write(
            gog_dir.join("installed.json"),
            r#"{"installed":[{"appName":"1423049311","install_path":"/home/user/Games/Heroic/Cyberpunk 2077"}]}"#,
        )
        .unwrap();

        let resolved =
            resolve_heroic_app_in_dir(tmp.path(), "/home/user/Games/Heroic/Cyberpunk 2077/")
                .unwrap();
        assert_eq!(resolved.runner, "gog");
        assert_eq!(resolved.app_name, "1423049311");
    }

    #[test]
    fn merges_enviroment_options_preserving_existing_heroic_keys() {
        let mut root = json!({
            "1423049311": {
                "winePrefix": "/home/user/Games/Heroic/Prefixes/default/Cyberpunk 2077",
                "enviromentOptions": [
                    { "key": "DXVK_HUD", "value": "fps" }
                ]
            }
        });
        merge_heroic_config_value(
            &mut root,
            "1423049311",
            &[LaunchEnvVar {
                name: "WINEDLLOVERRIDES".into(),
                value: "winmm=n,b".into(),
            }],
            Some("gamemoderun"),
            Some("--launcher-skip"),
        );
        let section = root.get("1423049311").unwrap();
        assert_eq!(
            section.get("winePrefix").and_then(Value::as_str),
            Some("/home/user/Games/Heroic/Prefixes/default/Cyberpunk 2077")
        );
        let envs = section
            .get("enviromentOptions")
            .and_then(Value::as_array)
            .unwrap();
        assert_eq!(envs.len(), 2);
        assert_eq!(envs[1]["key"], "WINEDLLOVERRIDES");
        assert_eq!(envs[1]["value"], "winmm=n,b");
        assert_eq!(section["launcherArgs"], "--launcher-skip");
    }

    #[test]
    fn reads_wine_prefix_from_games_config() {
        let tmp = tempfile::tempdir().unwrap();
        let games_cfg = tmp.path().join("GamesConfig");
        fs::create_dir_all(&games_cfg).unwrap();
        fs::write(
            games_cfg.join("1423049311.json"),
            r#"{"1423049311":{"winePrefix":"/home/user/Games/Heroic/Prefixes/default/Cyberpunk 2077"}}"#,
        )
        .unwrap();
        assert_eq!(
            read_heroic_wine_prefix(tmp.path(), "1423049311").as_deref(),
            Some("/home/user/Games/Heroic/Prefixes/default/Cyberpunk 2077")
        );
    }
}
