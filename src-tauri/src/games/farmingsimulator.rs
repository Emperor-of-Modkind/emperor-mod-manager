//! Farming Simulator 22 & 25 — `.zip` or folder mods in `Documents/My Games/FarmingSimulator20XX/mods/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

#[derive(Clone, Copy)]
struct FarmingSimTitle {
    id: &'static str,
    display_name: &'static str,
    nexus_domain: &'static str,
    match_names: &'static [&'static str],
    steam_app_id: &'static str,
    my_games_folder: &'static str,
}

const FS25: FarmingSimTitle = FarmingSimTitle {
    id: "farmingsimulator25",
    display_name: "Farming Simulator 25",
    nexus_domain: "farmingsimulator25",
    match_names: &["farming simulator 25", "farmingsimulator25", "fs25"],
    steam_app_id: "2300320",
    my_games_folder: "FarmingSimulator2025",
};

const FS22: FarmingSimTitle = FarmingSimTitle {
    id: "farmingsimulator22",
    display_name: "Farming Simulator 22",
    nexus_domain: "farmingsimulator22",
    match_names: &["farming simulator 22", "farmingsimulator22", "fs22"],
    steam_app_id: "1248130",
    my_games_folder: "FarmingSimulator2022",
};

macro_rules! farmingsim_plugin {
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
                &["mods"]
            }

            fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
                content_root.join("modDesc.xml").is_file()
            }

            fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
                content_folder_wrap_name(content_root, staged_name)
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                let mods_dir = super::user_data::documents_dir(install_path, $title.steam_app_id)
                    .map(|docs| docs.join("My Games").join($title.my_games_folder).join("mods"))
                    .unwrap_or_else(|| install_path.join("mods"));
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
        }
    };
}

farmingsim_plugin!(FarmingSimulator25Plugin, FS25);
farmingsim_plugin!(FarmingSimulator22Plugin, FS22);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_zip_into_mods_dir() {
        let dest = FarmingSimulator25Plugin
            .resolve_deploy_root(Path::new("/game"), Path::new("FS25_Tractor.zip"))
            .unwrap();
        assert!(dest.ends_with("mods/FS25_Tractor.zip"));
    }
}
