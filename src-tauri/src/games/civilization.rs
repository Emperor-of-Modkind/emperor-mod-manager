//! Sid Meier's Civilization VI & VII — mod folders in `Documents/My Games/Sid Meier's Civilization <VI|VII>/Mods/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

#[derive(Clone, Copy)]
struct CivTitle {
    id: &'static str,
    display_name: &'static str,
    nexus_domain: &'static str,
    match_names: &'static [&'static str],
    steam_app_id: &'static str,
    my_games_folder: &'static str,
}

const CIV7: CivTitle = CivTitle {
    id: "civilizationvii",
    display_name: "Sid Meier's Civilization VII",
    nexus_domain: "civilizationvii",
    match_names: &[
        "civilization vii",
        "civilization 7",
        "civilizationvii",
        "civ 7",
        "civ vii",
    ],
    steam_app_id: "1295660",
    my_games_folder: "Sid Meier's Civilization VII",
};

const CIV6: CivTitle = CivTitle {
    id: "civilizationvi",
    display_name: "Sid Meier's Civilization VI",
    nexus_domain: "civilizationvi",
    match_names: &[
        "civilization vi",
        "civilization 6",
        "civilizationvi",
        "civ 6",
        "civ vi",
    ],
    steam_app_id: "289070",
    my_games_folder: "Sid Meier's Civilization VI",
};

macro_rules! civ_plugin {
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

            fn prefers_mod_folder(&self) -> bool {
                true
            }

            fn preserve_staging_root_names(&self) -> &[&str] {
                &["Mods"]
            }

            fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
                content_folder_wrap_name(content_root, staged_name)
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                let mods_dir = super::user_data::documents_dir(install_path, $title.steam_app_id)
                    .map(|docs| docs.join("My Games").join($title.my_games_folder).join("Mods"))
                    .unwrap_or_else(|| install_path.join("Mods"));
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

civ_plugin!(Civilization7Plugin, CIV7);
civ_plugin!(Civilization6Plugin, CIV6);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_under_civ_mods() {
        let dest = Civilization6Plugin
            .resolve_deploy_root(Path::new("/civ6"), Path::new("BetterUI/BetterUI.modinfo"))
            .unwrap();
        assert!(dest.ends_with("Mods/BetterUI/BetterUI.modinfo"));
    }
}
