//! Kerbal Space Program — native `GameData/` folder loader (not BepInEx).

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{normalize_relative, GamePlugin, GamePluginInfo};

pub const KSP_ROOT_DIRS: &[&str] = &["GameData", "Ships", "Internals", "Parts", "Plugins"];

pub struct KerbalSpaceProgramPlugin;

impl GamePlugin for KerbalSpaceProgramPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "kerbalspaceprogram",
            display_name: "Kerbal Space Program",
            nexus_domain: "kerbalspaceprogram",
            match_names: &["kerbal space program", "kerbalspaceprogram"],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        KSP_ROOT_DIRS
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
        if first == "gamedata" {
            let rest: PathBuf = comps.collect();
            return Ok(install_path.join("GameData").join(rest));
        }
        if first == "ships" {
            return Ok(install_path.join(&normalized));
        }
        Ok(install_path.join("GameData").join(&normalized))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_into_gamedata() {
        let install = Path::new("/ksp");
        assert_eq!(
            KerbalSpaceProgramPlugin
                .resolve_deploy_root(install, Path::new("GameData/MechJeb2/Plugins/MechJeb2.dll"))
                .unwrap(),
            PathBuf::from("/ksp/GameData/MechJeb2/Plugins/MechJeb2.dll")
        );
        assert_eq!(
            KerbalSpaceProgramPlugin
                .resolve_deploy_root(install, Path::new("ModuleManager.dll"))
                .unwrap(),
            PathBuf::from("/ksp/GameData/ModuleManager.dll")
        );
    }
}
