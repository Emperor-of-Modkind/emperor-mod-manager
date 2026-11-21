//! Shared deploy logic for Unity Mod Manager (UMM) / Owlcat RPG titles
//! (Pathfinder: Wrath of the Righteous, Warhammer 40,000: Rogue Trader).
//!
//! UMM mods (`Info.json`) deploy into `<Install>/Mods/<ModName>/`, while Owlcat native
//! `OwlcatModificationManifest.json` packs deploy into `%LOCALAPPDATA%Low/Owlcat Games/<Game>/Modifications/<ModName>/`
//! (falling back to `<Install>/Mods/<ModName>/`).

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

#[derive(Clone, Copy)]
struct UmmTitle {
    id: &'static str,
    display_name: &'static str,
    nexus_domain: &'static str,
    match_names: &'static [&'static str],
    steam_app_id: &'static str,
    owlcat_folder: &'static str,
}

const PATHFINDER_WOTR: UmmTitle = UmmTitle {
    id: "pathfinderwrathoftherighteous",
    display_name: "Pathfinder: Wrath of the Righteous",
    nexus_domain: "pathfinderwrathoftherighteous",
    match_names: &[
        "wrath of the righteous",
        "pathfinderwrathoftherighteous",
        "wotr",
    ],
    steam_app_id: "1184370",
    owlcat_folder: "Pathfinder Wrath Of The Righteous",
};

const ROGUE_TRADER: UmmTitle = UmmTitle {
    id: "warhammer40kroguetrader",
    display_name: "Warhammer 40,000: Rogue Trader",
    nexus_domain: "warhammer40kroguetrader",
    match_names: &[
        "rogue trader",
        "warhammer40kroguetrader",
    ],
    steam_app_id: "2186680",
    owlcat_folder: "Warhammer 40000 Rogue Trader",
};

macro_rules! umm_plugin {
    ($struct:ident, $title:expr) => {
        pub struct $struct;

        impl GamePlugin for $struct {
            fn info(&self) -> GamePluginInfo {
                GamePluginInfo {
                    id: $title.id,
                    display_name: $title.display_name,
                    nexus_domain: $title.nexus_domain,
                    match_names: $title.match_names,
                }
            }

            fn preserve_staging_root_names(&self) -> &[&str] {
                &["Mods", "UnityModManager"]
            }

            fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
                content_root.join("Info.json").is_file()
                    || content_root.join("info.json").is_file()
                    || content_root.join("OwlcatModificationManifest.json").is_file()
            }

            fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
                content_folder_wrap_name(content_root, staged_name)
            }

            fn preflight_warnings(&self, install_path: &Path) -> Vec<String> {
                let has_umm = install_path.join("UnityModManager").is_dir()
                    || install_path.join("winhttp.dll").is_file();
                if has_umm {
                    Vec::new()
                } else {
                    vec![format!(
                        "Unity Mod Manager (UMM) not detected in {}. Install UMM (or use Owlcat's native Modifications folder) so mods load.",
                        install_path.display()
                    )]
                }
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                Ok(resolve_umm_deploy(install_path, $title, relative))
            }
        }
    };
}

umm_plugin!(PathfinderWotrPlugin, PATHFINDER_WOTR);
umm_plugin!(Warhammer40kRogueTraderPlugin, ROGUE_TRADER);

fn resolve_umm_deploy(install_path: &Path, title: UmmTitle, relative: &Path) -> PathBuf {
    let normalized = normalize_relative(relative);
    let mut comps = normalized.components();
    let first = comps
        .next()
        .and_then(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .unwrap_or_default();
    if first == "unitymodmanager" {
        return install_path.join(&normalized);
    }
    let mods_root = owlcat_umm_mods_dir(install_path, title);
    if first == "mods" {
        let rest: PathBuf = comps.collect();
        return mods_root.join(rest);
    }
    mods_root.join(&normalized)
}

fn owlcat_umm_mods_dir(install_path: &Path, title: UmmTitle) -> PathBuf {
    // Rogue Trader reads UMM mods from AppData/LocalLow/Owlcat Games/.../UnityModManager
    // when present; otherwise fall back to <Install>/Mods.
    if title.id == "warhammer40kroguetrader" {
        if let Some(local) = super::user_data::appdata_local_dir(install_path, title.steam_app_id) {
            if let Some(appdata) = local.parent() {
                let locallow = appdata
                    .join("LocalLow")
                    .join("Owlcat Games")
                    .join(title.owlcat_folder)
                    .join("UnityModManager");
                if locallow.parent().is_some_and(|p| p.is_dir()) {
                    return locallow;
                }
            }
        }
    }
    install_path.join("Mods")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraps_info_json_and_deploys_to_mods() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Info.json"), "{}").unwrap();
        assert!(PathfinderWotrPlugin.should_wrap_as_mod_folder(tmp.path()));
        assert_eq!(
            PathfinderWotrPlugin
                .resolve_deploy_root(Path::new("/wotr"), Path::new("ToyBox/Info.json"))
                .unwrap(),
            PathBuf::from("/wotr/Mods/ToyBox/Info.json")
        );
    }
}
