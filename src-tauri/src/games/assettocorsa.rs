//! Assetto Corsa — overlays into `<Install>/content/{cars,tracks,fonts,weather,...}` and `apps/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{normalize_relative, GamePlugin, GamePluginInfo};

pub const AC_ROOT_DIRS: &[&str] = &["content", "apps", "extension", "system", "cfg"];
const AC_CONTENT_SUBDIRS: &[&str] = &[
    "cars", "tracks", "fonts", "weather", "driver", "gui", "sfx", "texture", "ppfilters",
];

pub struct AssettoCorsaPlugin;

impl GamePlugin for AssettoCorsaPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "assettocorsa",
            display_name: "Assetto Corsa",
            nexus_domain: "assettocorsa",
            match_names: &["assetto corsa", "assettocorsa"],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        AC_ROOT_DIRS
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
        if AC_ROOT_DIRS.iter().any(|r| *r == first.as_str()) {
            return Ok(install_path.join(&normalized));
        }
        if AC_CONTENT_SUBDIRS.iter().any(|s| *s == first.as_str()) {
            return Ok(install_path.join("content").join(&normalized));
        }
        Ok(install_path.join(&normalized))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_content_and_bare_cars_tracks() {
        let install = Path::new("/ac");
        assert_eq!(
            AssettoCorsaPlugin
                .resolve_deploy_root(install, Path::new("content/cars/my_car/car.kn5"))
                .unwrap(),
            PathBuf::from("/ac/content/cars/my_car/car.kn5")
        );
        assert_eq!(
            AssettoCorsaPlugin
                .resolve_deploy_root(install, Path::new("tracks/monza_mod/track.kn5"))
                .unwrap(),
            PathBuf::from("/ac/content/tracks/monza_mod/track.kn5")
        );
    }
}
