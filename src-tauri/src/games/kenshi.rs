//! Kenshi — mod folders in `<Install>/mods/<ModName>/` + load order in `<Install>/data/mods.cfg`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

pub struct KenshiPlugin;

impl GamePlugin for KenshiPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "kenshi",
            display_name: "Kenshi",
            nexus_domain: "kenshi",
            match_names: &["kenshi"],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        &["mods", "data"]
    }

    fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
        has_mod_file_at_root(content_root)
    }

    fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
        if let Some(stem) = first_mod_stem(content_root) {
            return stem;
        }
        content_folder_wrap_name(content_root, staged_name)
    }

    fn load_order_entries(&self, content_root: &Path, _staged_name: &str) -> Vec<String> {
        collect_mod_files(content_root)
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
        enabled_mod_files: &[String],
    ) -> Result<Vec<String>> {
        let data_dir = install_path.join("data");
        std::fs::create_dir_all(&data_dir)?;
        let cfg = data_dir.join("mods.cfg");
        let backup = data_dir.join("mods.cfg.emperor.bak");
        if cfg.is_file() && !backup.is_file() {
            let _ = std::fs::copy(&cfg, &backup);
        }
        let mut out = String::new();
        for entry in enabled_mod_files {
            out.push_str(entry);
            out.push('\n');
        }
        std::fs::write(&cfg, out).with_context(|| format!("write {}", cfg.display()))?;
        Ok(Vec::new())
    }

    fn on_revert(&self, install_path: &Path) -> Result<()> {
        let data_dir = install_path.join("data");
        let cfg = data_dir.join("mods.cfg");
        let backup = data_dir.join("mods.cfg.emperor.bak");
        if backup.is_file() {
            std::fs::rename(&backup, &cfg)?;
        }
        Ok(())
    }
}

fn has_mod_file_at_root(content_root: &Path) -> bool {
    first_mod_stem(content_root).is_some()
}

fn first_mod_stem(content_root: &Path) -> Option<String> {
    let entries = std::fs::read_dir(content_root).ok()?;
    for entry in entries.flatten() {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            let path = entry.path();
            if path
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case("mod"))
            {
                return path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(ToOwned::to_owned);
            }
        }
    }
    None
}

fn collect_mod_files(content_root: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in walkdir::WalkDir::new(content_root).max_depth(2).into_iter().flatten() {
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            if name.to_ascii_lowercase().ends_with(".mod") {
                found.push(name.into_owned());
            }
        }
    }
    found.sort_by_key(|n| n.to_ascii_lowercase());
    found.dedup();
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_by_mod_stem_and_writes_mods_cfg() {
        let tmp = tempfile::tempdir().unwrap();
        let staged = tmp.path().join("staged");
        std::fs::create_dir_all(&staged).unwrap();
        std::fs::write(staged.join("Reactive World.mod"), b"x").unwrap();

        assert!(KenshiPlugin.should_wrap_as_mod_folder(&staged));
        assert_eq!(
            KenshiPlugin.wrap_mod_folder_name(&staged, "Staged"),
            "Reactive World"
        );

        let install = tmp.path().join("kenshi");
        KenshiPlugin
            .after_deploy(&install, &["Reactive World.mod".into()])
            .unwrap();
        let cfg = std::fs::read_to_string(install.join("data").join("mods.cfg")).unwrap();
        assert_eq!(cfg, "Reactive World.mod\n");
    }
}
