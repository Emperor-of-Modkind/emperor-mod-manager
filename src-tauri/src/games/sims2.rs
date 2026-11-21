//! The Sims 2 — custom content `.package` files live in `Documents/EA Games/The Sims 2/Downloads/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

const STEAM_APP_ID: &str = "3314070";

pub const SIMS2_ROOT_DIRS: &[&str] = &["Downloads", "SavedSims"];

pub struct Sims2Plugin;

impl GamePlugin for Sims2Plugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "sims2",
            display_name: "The Sims 2",
            nexus_domain: "thesims2",
            match_names: &["the sims 2", "sims 2", "sims2"],
        }
    }

    fn prefers_mod_folder(&self) -> bool {
        true
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        SIMS2_ROOT_DIRS
    }

    fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
        content_folder_wrap_name(content_root, staged_name)
    }

    fn should_deploy_file(&self, relative: &Path) -> bool {
        relative
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("package"))
    }

    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
        let user_dir = user_data_dir(install_path).unwrap_or_else(|| install_path.to_path_buf());
        let normalized = normalize_relative(relative);
        let mut comps = normalized.components();
        let first = comps
            .next()
            .and_then(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
                _ => None,
            })
            .unwrap_or_default();
        if first == "downloads" {
            let rest: PathBuf = comps.collect();
            return Ok(user_dir.join("Downloads").join(rest));
        }
        if first == "savedsims" {
            let rest: PathBuf = comps.collect();
            return Ok(user_dir.join("SavedSims").join(rest));
        }
        Ok(user_dir.join("Downloads").join(&normalized))
    }
}

fn user_data_dir(install_path: &Path) -> Option<PathBuf> {
    let docs = super::user_data::documents_dir(install_path, STEAM_APP_ID)?;
    let ea = docs.join("EA Games");
    for candidate in [
        "The Sims 2 Legacy",
        "The Sims™ 2 Ultimate Collection",
        "The Sims 2 Ultimate Collection",
        "The Sims 2",
    ] {
        let path = ea.join(candidate);
        if path.is_dir() {
            return Some(path);
        }
    }
    Some(ea.join("The Sims 2"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_package_under_downloads() {
        let dest = Sims2Plugin
            .resolve_deploy_root(Path::new("/game"), Path::new("MyMod/hair.package"))
            .unwrap();
        assert!(dest.ends_with("Downloads/MyMod/hair.package"));
        assert!(Sims2Plugin.should_deploy_file(Path::new("hair.package")));
        assert!(!Sims2Plugin.should_deploy_file(Path::new("readme.txt")));
    }
}
