//! Starsector — mod folders (with `mod_info.json`) in `<Install>/mods/<ModName>/`
//! and enabled mod IDs in `<Install>/mods/enabled_mods.json`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

pub struct StarsectorPlugin;

impl GamePlugin for StarsectorPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "starsector",
            display_name: "Starsector",
            nexus_domain: "starsector",
            match_names: &["starsector"],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        &["mods"]
    }

    fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
        content_root.join("mod_info.json").is_file()
    }

    fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
        content_folder_wrap_name(content_root, staged_name)
    }

    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
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
            return Ok(install_path.join("mods").join(rest));
        }
        Ok(install_path.join("mods").join(&normalized))
    }

    fn after_deploy(
        &self,
        install_path: &Path,
        enabled_mod_folders: &[String],
    ) -> Result<Vec<String>> {
        let mods_dir = install_path.join("mods");
        std::fs::create_dir_all(&mods_dir)?;
        let mut ids = Vec::new();
        for folder in enabled_mod_folders {
            if let Some(id) = read_starsector_mod_id(&mods_dir.join(folder)) {
                ids.push(id);
            }
        }
        if ids.is_empty() {
            return Ok(Vec::new());
        }
        let enabled_path = mods_dir.join("enabled_mods.json");
        let backup = mods_dir.join("enabled_mods.json.emperor.bak");
        if enabled_path.is_file() && !backup.is_file() {
            let _ = std::fs::copy(&enabled_path, &backup);
        }
        let payload = serde_json::json!({ "enabledMods": ids });
        std::fs::write(&enabled_path, serde_json::to_string_pretty(&payload)?)
            .with_context(|| format!("write {}", enabled_path.display()))?;
        Ok(Vec::new())
    }

    fn on_revert(&self, install_path: &Path) -> Result<()> {
        let mods_dir = install_path.join("mods");
        let enabled_path = mods_dir.join("enabled_mods.json");
        let backup = mods_dir.join("enabled_mods.json.emperor.bak");
        if backup.is_file() {
            std::fs::rename(&backup, &enabled_path)?;
        }
        Ok(())
    }
}

fn read_starsector_mod_id(mod_dir: &Path) -> Option<String> {
    let raw = std::fs::read_to_string(mod_dir.join("mod_info.json")).ok()?;
    // Starsector's mod_info.json allows `#` comments; strip them before JSON parsing.
    let stripped: String = raw
        .lines()
        .map(|line| line.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");
    let val: serde_json::Value = serde_json::from_str(&stripped).ok()?;
    val.get("id")?.as_str().map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_mod_info_and_writes_enabled_mods_json() {
        let tmp = tempfile::tempdir().unwrap();
        let mod_dir = tmp.path().join("mods").join("Nexerelin");
        std::fs::create_dir_all(&mod_dir).unwrap();
        std::fs::write(
            mod_dir.join("mod_info.json"),
            "{\n  # comment\n  \"id\": \"nexerelin\",\n  \"name\": \"Nexerelin\"\n}\n",
        )
        .unwrap();
        assert!(StarsectorPlugin.should_wrap_as_mod_folder(&mod_dir));
        StarsectorPlugin
            .after_deploy(tmp.path(), &["Nexerelin".into()])
            .unwrap();
        let raw = std::fs::read_to_string(tmp.path().join("mods").join("enabled_mods.json")).unwrap();
        assert!(raw.contains("\"nexerelin\""));
    }
}
