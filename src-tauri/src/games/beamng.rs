//! BeamNG.drive — `.zip` mods in `<UserData>/BeamNG.drive/<ver>/mods/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{normalize_relative, GamePlugin, GamePluginInfo};

const STEAM_APP_ID: &str = "284160";

pub struct BeamNgDrivePlugin;

impl GamePlugin for BeamNgDrivePlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "beamngdrive",
            display_name: "BeamNG.drive",
            nexus_domain: "beamngdrive",
            match_names: &["beamng.drive", "beamng drive", "beamngdrive"],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        &["mods", "vehicles", "levels"]
    }

    fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
        let mods_dir = beamng_mods_dir(install_path);
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

fn beamng_mods_dir(install_path: &Path) -> PathBuf {
    if let Some(local) = super::user_data::appdata_local_dir(install_path, STEAM_APP_ID) {
        let base = local.join("BeamNG.drive");
        if let Some(ver_dir) = newest_version_dir(&base) {
            return ver_dir.join("mods");
        }
        return base.join("mods");
    }
    install_path.join("mods")
}

fn newest_version_dir(base: &Path) -> Option<PathBuf> {
    let mut dirs: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.is_dir()
                && p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.chars().next().is_some_and(|c| c.is_ascii_digit()))
        })
        .collect();
    dirs.sort();
    dirs.pop()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_zip_into_beamng_mods() {
        let dest = BeamNgDrivePlugin
            .resolve_deploy_root(Path::new("/beamng"), Path::new("cool_car.zip"))
            .unwrap();
        assert!(dest.ends_with("mods/cool_car.zip"));
    }
}
