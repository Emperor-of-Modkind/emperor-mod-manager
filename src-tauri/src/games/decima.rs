//! Shared deploy logic for Decima Engine titles
//! (Horizon Zero Dawn, Horizon Forbidden West, Death Stranding).
//!
//! Horizon titles load loose `.bin` patch archives from `<Install>/Packed_DX12/`.
//! Death Stranding loads loose `.bin` / patch files from `<Install>/data/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{normalize_relative, GamePlugin, GamePluginInfo};

macro_rules! decima_plugin {
    ($struct:ident, $id:expr, $display:expr, $domain:expr, $matches:expr, $target_dir:expr) => {
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
                &[$target_dir]
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
                if first == $target_dir.to_ascii_lowercase() {
                    let rest: PathBuf = comps.collect();
                    return Ok(install_path.join($target_dir).join(rest));
                }
                Ok(install_path.join($target_dir).join(&normalized))
            }
        }
    };
}

decima_plugin!(
    HorizonZeroDawnPlugin,
    "horizonzerodawn",
    "Horizon Zero Dawn",
    "horizonzerodawn",
    &["horizon zero dawn", "horizonzerodawn"],
    "Packed_DX12"
);

decima_plugin!(
    HorizonForbiddenWestPlugin,
    "horizonforbiddenwest",
    "Horizon Forbidden West",
    "horizonforbiddenwest",
    &["horizon forbidden west", "horizonforbiddenwest"],
    "Packed_DX12"
);

decima_plugin!(
    DeathStrandingPlugin,
    "deathstranding",
    "Death Stranding",
    "deathstranding",
    &["death stranding", "deathstranding"],
    "data"
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_bin_to_packed_dx12_or_data() {
        assert_eq!(
            HorizonZeroDawnPlugin
                .resolve_deploy_root(Path::new("/hzd"), Path::new("Patch_Aloy.bin"))
                .unwrap(),
            PathBuf::from("/hzd/Packed_DX12/Patch_Aloy.bin")
        );
        assert_eq!(
            HorizonForbiddenWestPlugin
                .resolve_deploy_root(Path::new("/hfw"), Path::new("Packed_DX12/Patch_Mod.bin"))
                .unwrap(),
            PathBuf::from("/hfw/Packed_DX12/Patch_Mod.bin")
        );
        assert_eq!(
            DeathStrandingPlugin
                .resolve_deploy_root(Path::new("/ds"), Path::new("patch.bin"))
                .unwrap(),
            PathBuf::from("/ds/data/patch.bin")
        );
    }
}
