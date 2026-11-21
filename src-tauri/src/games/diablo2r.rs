//! Diablo II: Resurrected — mods live under `<Install>/mods/<ModName>/<ModName>.mpq/`
//! and are loaded with `-mod <ModName> -txt`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

pub struct Diablo2ResurrectedPlugin;

impl GamePlugin for Diablo2ResurrectedPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "diablo2resurrected",
            display_name: "Diablo II: Resurrected",
            nexus_domain: "diablo2resurrected",
            match_names: &[
                "diablo ii resurrected",
                "diablo 2 resurrected",
                "diablo2resurrected",
                "d2r",
            ],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        &["mods", "data"]
    }

    fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
        content_root.join("modinfo.json").is_file()
    }

    fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
        content_folder_wrap_name(content_root, staged_name)
    }

    fn preflight_warnings(&self, _install_path: &Path) -> Vec<String> {
        vec![
            "Launch Diablo II: Resurrected with `-mod <ModName> -txt` (in Battle.net or Steam launch options) to load deployed mods."
                .into(),
        ]
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_under_mods() {
        assert_eq!(
            Diablo2ResurrectedPlugin
                .resolve_deploy_root(Path::new("/d2r"), Path::new("mods/mymod/mymod.mpq/modinfo.json"))
                .unwrap(),
            PathBuf::from("/d2r/mods/mymod/mymod.mpq/modinfo.json")
        );
    }
}
