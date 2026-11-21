//! Hades & Hades II — mods live under `<Install>/Content/Mods/<ModName>/`
//! (loaded via ModImporter for Hades or Hell2Modding / ReturnOfModding for Hades II).

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

macro_rules! hades_plugin {
    ($struct:ident, $id:expr, $display:expr, $domain:expr, $matches:expr) => {
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
                true
            }

            fn preserve_staging_root_names(&self) -> &[&str] {
                &["Content", "Mods", "ReturnOfModding"]
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
                if first == "content" || first == "returnofmodding" {
                    return Ok(install_path.join(&normalized));
                }
                if first == "mods" {
                    let rest: PathBuf = comps.collect();
                    return Ok(install_path.join("Content").join("Mods").join(rest));
                }
                Ok(install_path.join("Content").join("Mods").join(&normalized))
            }
        }
    };
}

hades_plugin!(
    Hades2Plugin,
    "hades2",
    "Hades II",
    "hades2",
    &["hades ii", "hades 2", "hades2"]
);

hades_plugin!(
    HadesPlugin,
    "hades",
    "Hades",
    "hades",
    &["hades"]
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_under_content_mods() {
        assert_eq!(
            Hades2Plugin
                .resolve_deploy_root(Path::new("/hades2"), Path::new("MyMod/main.lua"))
                .unwrap(),
            PathBuf::from("/hades2/Content/Mods/MyMod/main.lua")
        );
    }
}
