//! Factorio — mods live in user `mods/` (`~/.factorio/mods` on Linux, `%APPDATA%/Factorio/mods` on Windows)
//! with enabled state in `mod-list.json`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

const STEAM_APP_ID: &str = "427520";

pub struct FactorioPlugin;

impl GamePlugin for FactorioPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "factorio",
            display_name: "Factorio",
            nexus_domain: "factorio",
            match_names: &["factorio"],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        &["mods"]
    }

    fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
        content_root.join("info.json").is_file()
    }

    fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
        content_folder_wrap_name(content_root, staged_name)
    }

    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
        let mods_dir = factorio_mods_dir(install_path);
        let normalized = normalize_relative(relative);
        let mut comps = normalized.components();
        let first = comps
            .next()
            .and_then(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
                _ => None,
            })
            .unwrap_or_default();
        if first == "mods" {
            let rest: PathBuf = comps.collect();
            return Ok(mods_dir.join(rest));
        }
        Ok(mods_dir.join(&normalized))
    }

    fn after_deploy(
        &self,
        install_path: &Path,
        enabled_mod_folders: &[String],
    ) -> Result<Vec<String>> {
        if enabled_mod_folders.is_empty() {
            return Ok(Vec::new());
        }
        let mods_dir = factorio_mods_dir(install_path);
        std::fs::create_dir_all(&mods_dir)?;
        let mod_list = mods_dir.join("mod-list.json");
        let backup = mods_dir.join("mod-list.json.emperor.bak");
        if mod_list.is_file() && !backup.is_file() {
            let _ = std::fs::copy(&mod_list, &backup);
        }
        let mut entries = vec![serde_json::json!({ "name": "base", "enabled": true })];
        for folder in enabled_mod_folders {
            let name = read_factorio_mod_name(&mods_dir.join(folder))
                .unwrap_or_else(|| folder.clone());
            entries.push(serde_json::json!({ "name": name, "enabled": true }));
        }
        let payload = serde_json::json!({ "mods": entries });
        std::fs::write(&mod_list, serde_json::to_string_pretty(&payload)?)
            .with_context(|| format!("write {}", mod_list.display()))?;
        Ok(Vec::new())
    }

    fn on_revert(&self, install_path: &Path) -> Result<()> {
        let mods_dir = factorio_mods_dir(install_path);
        let mod_list = mods_dir.join("mod-list.json");
        let backup = mods_dir.join("mod-list.json.emperor.bak");
        if backup.is_file() {
            std::fs::rename(&backup, &mod_list)?;
        }
        Ok(())
    }
}

fn factorio_mods_dir(install_path: &Path) -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            let native = home.join(".factorio").join("mods");
            if native.parent().is_some_and(|p| p.is_dir()) {
                return native;
            }
        }
    }
    if let Some(roaming) = super::user_data::appdata_roaming_dir(install_path, STEAM_APP_ID) {
        return roaming.join("Factorio").join("mods");
    }
    install_path.join("mods")
}

fn read_factorio_mod_name(mod_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(mod_dir.join("info.json")).ok()?;
    let val: serde_json::Value = serde_json::from_str(&raw).ok()?;
    val.get("name")?.as_str().map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_info_json_mods() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("info.json"), r#"{"name":"krastorio2"}"#).unwrap();
        assert!(FactorioPlugin.should_wrap_as_mod_folder(tmp.path()));
    }
}
