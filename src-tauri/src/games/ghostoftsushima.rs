//! Ghost of Tsushima DIRECTOR'S CUT — loose `.psarc` mod archives in `<Install>/cache_pc/psarc/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{normalize_relative, GamePlugin, GamePluginInfo};

pub struct GhostOfTsushimaPlugin;

impl GamePlugin for GhostOfTsushimaPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "ghostoftsushima",
            display_name: "Ghost of Tsushima DIRECTOR'S CUT",
            nexus_domain: "ghostoftsushima",
            match_names: &["ghost of tsushima", "ghostoftsushima"],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        &["cache_pc", "psarc"]
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
        if first == "cache_pc" {
            return Ok(install_path.join(&normalized));
        }
        if first == "psarc" {
            let rest: PathBuf = comps.collect();
            return Ok(install_path.join("cache_pc").join("psarc").join(rest));
        }
        Ok(install_path.join("cache_pc").join("psarc").join(&normalized))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_psarc_into_cache_pc_psarc() {
        let install = Path::new("/got");
        assert_eq!(
            GhostOfTsushimaPlugin
                .resolve_deploy_root(install, Path::new("armor_mod.psarc"))
                .unwrap(),
            PathBuf::from("/got/cache_pc/psarc/armor_mod.psarc")
        );
        assert_eq!(
            GhostOfTsushimaPlugin
                .resolve_deploy_root(install, Path::new("cache_pc/psarc/armor_mod.psarc"))
                .unwrap(),
            PathBuf::from("/got/cache_pc/psarc/armor_mod.psarc")
        );
    }
}
