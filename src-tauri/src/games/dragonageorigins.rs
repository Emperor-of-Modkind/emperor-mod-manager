//! Dragon Age: Origins — loose overrides in `Documents/BioWare/Dragon Age/packages/core/override/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

const STEAM_APP_ID: &str = "47810";

pub struct DragonAgeOriginsPlugin;

impl GamePlugin for DragonAgeOriginsPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "dragonage",
            display_name: "Dragon Age: Origins",
            nexus_domain: "dragonage",
            match_names: &["dragon age origins", "dragon age: origins", "dragonage"],
        }
    }

    fn prefers_mod_folder(&self) -> bool {
        true
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        &["override", "packages", "addins"]
    }

    fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
        content_folder_wrap_name(content_root, staged_name)
    }

    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
        let user_dir = super::user_data::documents_dir(install_path, STEAM_APP_ID)
            .map(|docs| docs.join("BioWare").join("Dragon Age"))
            .unwrap_or_else(|| install_path.to_path_buf());
        let override_dir = user_dir.join("packages").join("core").join("override");
        let normalized = normalize_relative(relative);
        let mut comps = normalized.components();
        let first = comps
            .next()
            .and_then(|c| match c {
                Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
                _ => None,
            })
            .unwrap_or_default();
        if first == "packages" || first == "addins" {
            return Ok(user_dir.join(&normalized));
        }
        if first == "override" {
            let rest: PathBuf = comps.collect();
            return Ok(override_dir.join(rest));
        }
        Ok(override_dir.join(&normalized))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_under_packages_core_override() {
        let dest = DragonAgeOriginsPlugin
            .resolve_deploy_root(Path::new("/dao"), Path::new("MyMod/item.uti"))
            .unwrap();
        assert!(dest.ends_with("packages/core/override/MyMod/item.uti"));
    }
}
