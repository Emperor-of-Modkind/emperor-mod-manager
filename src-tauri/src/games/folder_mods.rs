//! Clean native-folder games: Barotrauma (`LocalMods/`), Dwarf Fortress (`mods/`),
//! Darkest Dungeon (`mods/`), and Vintage Story (`Mods/`).

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

macro_rules! folder_mod_plugin {
    ($struct:ident, $id:expr, $display:expr, $domain:expr, $matches:expr, $mods_dir:expr, $prefer_wrap:expr) => {
        pub struct $struct;

        impl GamePlugin for $struct {
            fn info(&self) -> GamePluginInfo {
                GamePluginInfo {
                    id: $id,
                    display_name: $display,
                    nexus_domain: $domain,
                    match_names: $matches,
                }
            }

            fn prefers_mod_folder(&self) -> bool {
                $prefer_wrap
            }

            fn preserve_staging_root_names(&self) -> &[&str] {
                &[$mods_dir]
            }

            fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
                content_folder_wrap_name(content_root, staged_name)
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
                if first == $mods_dir.to_ascii_lowercase() {
                    let rest: PathBuf = comps.collect();
                    return Ok(install_path.join($mods_dir).join(rest));
                }
                Ok(install_path.join($mods_dir).join(&normalized))
            }
        }
    };
}

folder_mod_plugin!(
    BarotraumaPlugin,
    "barotrauma",
    "Barotrauma",
    "barotrauma",
    &["barotrauma"],
    "LocalMods",
    true
);

folder_mod_plugin!(
    DwarfFortressPlugin,
    "dwarffortress",
    "Dwarf Fortress",
    "dwarffortress",
    &["dwarf fortress", "dwarffortress"],
    "mods",
    true
);

folder_mod_plugin!(
    DarkestDungeonPlugin,
    "darkestdungeon",
    "Darkest Dungeon",
    "darkestdungeon",
    &["darkest dungeon", "darkestdungeon"],
    "mods",
    true
);

folder_mod_plugin!(
    VintageStoryPlugin,
    "vintagestory",
    "Vintage Story",
    "vintagestory",
    &["vintage story", "vintagestory"],
    "Mods",
    false
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_under_expected_mod_dirs() {
        assert_eq!(
            BarotraumaPlugin
                .resolve_deploy_root(Path::new("/baro"), Path::new("Neurotrauma/filelist.xml"))
                .unwrap(),
            PathBuf::from("/baro/LocalMods/Neurotrauma/filelist.xml")
        );
        assert_eq!(
            DwarfFortressPlugin
                .resolve_deploy_root(Path::new("/df"), Path::new("MyMod/info.txt"))
                .unwrap(),
            PathBuf::from("/df/mods/MyMod/info.txt")
        );
        assert_eq!(
            DarkestDungeonPlugin
                .resolve_deploy_root(Path::new("/dd"), Path::new("ClassMod/project.xml"))
                .unwrap(),
            PathBuf::from("/dd/mods/ClassMod/project.xml")
        );
        assert_eq!(
            VintageStoryPlugin
                .resolve_deploy_root(Path::new("/vs"), Path::new("PrimitiveSurvival.zip"))
                .unwrap(),
            PathBuf::from("/vs/Mods/PrimitiveSurvival.zip")
        );
    }
}
