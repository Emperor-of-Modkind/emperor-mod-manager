//! Shared deploy logic for Rockstar RAGE Engine titles
//! (Red Dead Redemption 2 and Grand Theft Auto V).
//!
//! - RDR2: ScriptHookRDR2 / ASI loaders (`*.asi`, `dinput8.dll`) at install root,
//!   Lenny's Mod Loader packs (`install.xml`) wrapped under `lml/<ModName>/`.
//! - GTA V: ScriptHookV / ASI loaders at install root, `scripts/`, `mods/`, `plugins/`, `lml/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

pub const RDR2_ROOT_DIRS: &[&str] = &["lml", "scripts", "plugins", "stream"];
pub const GTAV_ROOT_DIRS: &[&str] = &["scripts", "mods", "plugins", "lml", "lspdfr", "update"];

const ROOT_DLL_EXTENSIONS: &[&str] = &["asi", "dll", "ini", "dat"];

pub struct RedDeadRedemption2Plugin;
pub struct GrandTheftAuto5Plugin;

impl GamePlugin for RedDeadRedemption2Plugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "reddeadredemption2",
            display_name: "Red Dead Redemption 2",
            nexus_domain: "reddeadredemption2",
            match_names: &[
                "red dead redemption 2",
                "red dead redemption ii",
                "reddeadredemption2",
                "rdr2",
            ],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        RDR2_ROOT_DIRS
    }

    fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
        content_root.join("install.xml").is_file()
    }

    fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
        content_folder_wrap_name(content_root, staged_name)
    }

    fn preflight_warnings(&self, install_path: &Path) -> Vec<String> {
        let has_asi_loader = install_path.join("dinput8.dll").is_file()
            || install_path.join("version.dll").is_file();
        if has_asi_loader {
            Vec::new()
        } else {
            vec![
                "No ASI loader (dinput8.dll / version.dll) found in the RDR2 folder. ScriptHookRDR2 and Lenny's Mod Loader mods will not load until one is installed."
                    .into(),
            ]
        }
    }

    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
        let normalized = normalize_relative(relative);
        let first = first_component_lower(&normalized);
        if RDR2_ROOT_DIRS.iter().any(|d| *d == first.as_str()) {
            return Ok(install_path.join(&normalized));
        }
        if is_single_root_file(&normalized) {
            return Ok(install_path.join(&normalized));
        }
        Ok(install_path.join("lml").join(&normalized))
    }
}

impl GamePlugin for GrandTheftAuto5Plugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "gta5",
            display_name: "Grand Theft Auto V",
            nexus_domain: "gta5",
            match_names: &[
                "grand theft auto v",
                "grand theft auto 5",
                "gta v",
                "gta 5",
                "gta5",
            ],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        GTAV_ROOT_DIRS
    }

    fn preflight_warnings(&self, install_path: &Path) -> Vec<String> {
        let has_loader = install_path.join("dinput8.dll").is_file()
            || install_path.join("ScriptHookV.dll").is_file();
        if has_loader {
            Vec::new()
        } else {
            vec![
                "ScriptHookV / dinput8.dll not found in the GTA V folder. ASI and script mods require ScriptHookV to load."
                    .into(),
            ]
        }
    }

    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
        let normalized = normalize_relative(relative);
        let first = first_component_lower(&normalized);
        if GTAV_ROOT_DIRS.iter().any(|d| *d == first.as_str()) {
            return Ok(install_path.join(&normalized));
        }
        if is_single_root_file(&normalized) {
            return Ok(install_path.join(&normalized));
        }
        let ext = normalized
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
            .unwrap_or_default();
        if ext == "cs" || ext == "vb" || ext == "lua" {
            return Ok(install_path.join("scripts").join(&normalized));
        }
        Ok(install_path.join(&normalized))
    }
}

fn first_component_lower(path: &Path) -> String {
    path.components()
        .next()
        .and_then(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .unwrap_or_default()
}

fn is_single_root_file(path: &Path) -> bool {
    if path.components().count() != 1 {
        return false;
    }
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            ROOT_DLL_EXTENSIONS
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rdr2_routes_asi_to_root_and_lml_packs_to_lml() {
        let install = Path::new("/rdr2");
        assert_eq!(
            RedDeadRedemption2Plugin
                .resolve_deploy_root(install, Path::new("Rampage.asi"))
                .unwrap(),
            PathBuf::from("/rdr2/Rampage.asi")
        );
        assert_eq!(
            RedDeadRedemption2Plugin
                .resolve_deploy_root(install, Path::new("MyMod/install.xml"))
                .unwrap(),
            PathBuf::from("/rdr2/lml/MyMod/install.xml")
        );
        assert_eq!(
            RedDeadRedemption2Plugin
                .resolve_deploy_root(install, Path::new("lml/stream/ped.ymt"))
                .unwrap(),
            PathBuf::from("/rdr2/lml/stream/ped.ymt")
        );
    }

    #[test]
    fn gtav_routes_scripts_and_asi() {
        let install = Path::new("/gtav");
        assert_eq!(
            GrandTheftAuto5Plugin
                .resolve_deploy_root(install, Path::new("Menyoo.asi"))
                .unwrap(),
            PathBuf::from("/gtav/Menyoo.asi")
        );
        assert_eq!(
            GrandTheftAuto5Plugin
                .resolve_deploy_root(install, Path::new("scripts/MyMod.dll"))
                .unwrap(),
            PathBuf::from("/gtav/scripts/MyMod.dll")
        );
        assert_eq!(
            GrandTheftAuto5Plugin
                .resolve_deploy_root(install, Path::new("Trainer.cs"))
                .unwrap(),
            PathBuf::from("/gtav/scripts/Trainer.cs")
        );
    }
}
