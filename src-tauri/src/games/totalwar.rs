//! Shared deploy logic for Creative Assembly Total War titles
//! (WARHAMMER III, THREE KINGDOMS, ROME II, SHOGUN 2).
//!
//! `.pack` files deploy to `<Install>/data/`, and load order is written to
//! `%APPDATA%/The Creative Assembly/<Game>/scripts/user.script.txt`.

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use super::{normalize_relative, GamePlugin, GamePluginInfo};

#[derive(Clone, Copy)]
struct TotalWarTitle {
    id: &'static str,
    display_name: &'static str,
    nexus_domain: &'static str,
    match_names: &'static [&'static str],
    steam_app_id: &'static str,
    appdata_folder: &'static str,
}

const WARHAMMER3: TotalWarTitle = TotalWarTitle {
    id: "totalwarwarhammer3",
    display_name: "Total War: WARHAMMER III",
    nexus_domain: "totalwarwarhammer3",
    match_names: &[
        "total war warhammer iii",
        "total war warhammer 3",
        "totalwarwarhammer3",
    ],
    steam_app_id: "1142710",
    appdata_folder: "Warhammer3",
};

const THREE_KINGDOMS: TotalWarTitle = TotalWarTitle {
    id: "totalwarthreekingdoms",
    display_name: "Total War: THREE KINGDOMS",
    nexus_domain: "totalwarthreekingdoms",
    match_names: &[
        "total war three kingdoms",
        "totalwarthreekingdoms",
    ],
    steam_app_id: "779340",
    appdata_folder: "ThreeKingdoms",
};

const ROME2: TotalWarTitle = TotalWarTitle {
    id: "totalwarrome2",
    display_name: "Total War: ROME II",
    nexus_domain: "totalwarrome2",
    match_names: &[
        "total war rome ii",
        "total war rome 2",
        "totalwarrome2",
    ],
    steam_app_id: "214950",
    appdata_folder: "Rome2",
};

const SHOGUN2: TotalWarTitle = TotalWarTitle {
    id: "totalwarshogun2",
    display_name: "Total War: SHOGUN 2",
    nexus_domain: "totalwarshogun2",
    match_names: &[
        "total war shogun 2",
        "totalwarshogun2",
    ],
    steam_app_id: "34330",
    appdata_folder: "Shogun2",
};

macro_rules! totalwar_plugin {
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
                &["data"]
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                Ok(resolve_tw_deploy(install_path, relative))
            }

            fn load_order_entries(&self, content_root: &Path, _staged_name: &str) -> Vec<String> {
                collect_pack_names(content_root)
            }

            fn after_deploy(
                &self,
                install_path: &Path,
                enabled_mod_folders: &[String],
            ) -> Result<Vec<String>> {
                write_user_script(install_path, $title, enabled_mod_folders)
            }

            fn on_revert(&self, install_path: &Path) -> Result<()> {
                revert_user_script(install_path, $title)
            }
        }
    };
}

totalwar_plugin!(TotalWarWarhammer3Plugin, WARHAMMER3);
totalwar_plugin!(TotalWarThreeKingdomsPlugin, THREE_KINGDOMS);
totalwar_plugin!(TotalWarRome2Plugin, ROME2);
totalwar_plugin!(TotalWarShogun2Plugin, SHOGUN2);

fn resolve_tw_deploy(install_path: &Path, relative: &Path) -> PathBuf {
    let normalized = normalize_relative(relative);
    let mut comps = normalized.components();
    let first = comps
        .next()
        .and_then(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .unwrap_or_default();
    if first == "data" {
        let rest: PathBuf = comps.collect();
        return install_path.join("data").join(rest);
    }
    install_path.join("data").join(&normalized)
}

fn collect_pack_names(content_root: &Path) -> Vec<String> {
    let mut packs = Vec::new();
    for entry in walkdir::WalkDir::new(content_root).into_iter().flatten() {
        if entry.file_type().is_file() {
            let name = entry.file_name().to_string_lossy();
            if name.to_ascii_lowercase().ends_with(".pack") {
                packs.push(name.into_owned());
            }
        }
    }
    packs.sort_by_key(|p| p.to_ascii_lowercase());
    packs.dedup();
    packs
}

fn user_script_path(install_path: &Path, title: TotalWarTitle) -> Option<PathBuf> {
    let roaming = super::user_data::appdata_roaming_dir(install_path, title.steam_app_id)?;
    Some(
        roaming
            .join("The Creative Assembly")
            .join(title.appdata_folder)
            .join("scripts")
            .join("user.script.txt"),
    )
}

fn write_user_script(
    install_path: &Path,
    title: TotalWarTitle,
    enabled_packs: &[String],
) -> Result<Vec<String>> {
    let Some(script_path) = user_script_path(install_path, title) else {
        return Ok(Vec::new());
    };
    if let Some(parent) = script_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let backup = script_path.with_extension("txt.emperor.bak");
    if script_path.is_file() && !backup.is_file() {
        let _ = std::fs::copy(&script_path, &backup);
    }
    let mut out = String::new();
    for pack in enabled_packs {
        out.push_str(&format!("mod \"{pack}\";\n"));
    }
    std::fs::write(&script_path, out)
        .with_context(|| format!("write {}", script_path.display()))?;
    Ok(Vec::new())
}

fn revert_user_script(install_path: &Path, title: TotalWarTitle) -> Result<()> {
    let Some(script_path) = user_script_path(install_path, title) else {
        return Ok(());
    };
    let backup = script_path.with_extension("txt.emperor.bak");
    if backup.is_file() {
        std::fs::rename(&backup, &script_path)?;
    } else if script_path.is_file() {
        let _ = std::fs::remove_file(&script_path);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_pack_into_data_dir() {
        assert_eq!(
            TotalWarWarhammer3Plugin
                .resolve_deploy_root(Path::new("/game"), Path::new("cool_mod.pack"))
                .unwrap(),
            PathBuf::from("/game/data/cool_mod.pack")
        );
        assert_eq!(
            TotalWarRome2Plugin
                .resolve_deploy_root(Path::new("/game"), Path::new("data/dei.pack"))
                .unwrap(),
            PathBuf::from("/game/data/dei.pack")
        );
    }
}
