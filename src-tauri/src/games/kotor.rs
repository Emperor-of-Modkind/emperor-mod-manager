//! Star Wars: Knights of the Old Republic I & II (Odyssey Engine) — loose files in `<Install>/Override/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{normalize_relative, GamePlugin, GamePluginInfo};

pub const KOTOR_ROOT_DIRS: &[&str] = &["Override", "Modules", "StreamMusic", "StreamSounds", "StreamVoice", "Movies", "Lips"];

macro_rules! kotor_plugin {
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

            fn preserve_staging_root_names(&self) -> &[&str] {
                KOTOR_ROOT_DIRS
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                let normalized = normalize_relative(relative);
                let first = normalized
                    .components()
                    .next()
                    .and_then(|c| match c {
                        Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
                        _ => None,
                    })
                    .unwrap_or_default();
                if KOTOR_ROOT_DIRS
                    .iter()
                    .any(|r| r.eq_ignore_ascii_case(&first))
                {
                    return Ok(install_path.join(&normalized));
                }
                Ok(install_path.join("Override").join(&normalized))
            }
        }
    };
}

kotor_plugin!(
    Kotor2Plugin,
    "kotor2",
    "Star Wars: Knights of the Old Republic II",
    "kotor2",
    &[
        "knights of the old republic ii",
        "knights of the old republic 2",
        "the sith lords",
        "kotor 2",
        "kotor2",
    ]
);

kotor_plugin!(
    KotorPlugin,
    "kotor",
    "Star Wars: Knights of the Old Republic",
    "kotor",
    &["knights of the old republic", "kotor"]
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_to_override_or_preserves_modules() {
        let install = Path::new("/kotor");
        assert_eq!(
            KotorPlugin
                .resolve_deploy_root(install, Path::new("p_bastilah.tga"))
                .unwrap(),
            PathBuf::from("/kotor/Override/p_bastilah.tga")
        );
        assert_eq!(
            Kotor2Plugin
                .resolve_deploy_root(install, Path::new("Modules/001ebo.mod"))
                .unwrap(),
            PathBuf::from("/kotor/Modules/001ebo.mod")
        );
    }
}
