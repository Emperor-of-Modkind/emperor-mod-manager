//! Shared deploy logic for Paradox Clausewitz Engine titles
//! (Hearts of Iron IV, Stellaris, Crusader Kings III, Europa Universalis IV, Victoria 3).
//!
//! Layout:
//! ```text
//! Documents/Paradox Interactive/<Game>/mod/<ModName>/...
//! Documents/Paradox Interactive/<Game>/mod/<ModName>.mod
//! Documents/Paradox Interactive/<Game>/dlc_load.json
//! ```

use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result};

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

#[derive(Clone, Copy)]
struct ParadoxTitle {
    id: &'static str,
    display_name: &'static str,
    nexus_domain: &'static str,
    match_names: &'static [&'static str],
    steam_app_id: &'static str,
    docs_folder: &'static str,
}

const HOI4: ParadoxTitle = ParadoxTitle {
    id: "heartsofironiv",
    display_name: "Hearts of Iron IV",
    nexus_domain: "heartsofironiv",
    match_names: &[
        "hearts of iron iv",
        "hearts of iron 4",
        "heartsofironiv",
        "hoi4",
    ],
    steam_app_id: "394360",
    docs_folder: "Hearts of Iron IV",
};

const STELLARIS: ParadoxTitle = ParadoxTitle {
    id: "stellaris",
    display_name: "Stellaris",
    nexus_domain: "stellaris",
    match_names: &["stellaris"],
    steam_app_id: "281990",
    docs_folder: "Stellaris",
};

const CK3: ParadoxTitle = ParadoxTitle {
    id: "crusaderkings3",
    display_name: "Crusader Kings III",
    nexus_domain: "crusaderkings3",
    match_names: &[
        "crusader kings iii",
        "crusader kings 3",
        "crusaderkings3",
        "ck3",
    ],
    steam_app_id: "1158310",
    docs_folder: "Crusader Kings III",
};

const EU4: ParadoxTitle = ParadoxTitle {
    id: "europauniversalisiv",
    display_name: "Europa Universalis IV",
    nexus_domain: "europauniversalisiv",
    match_names: &[
        "europa universalis iv",
        "europa universalis 4",
        "europauniversalisiv",
        "eu4",
    ],
    steam_app_id: "236850",
    docs_folder: "Europa Universalis IV",
};

const VIC3: ParadoxTitle = ParadoxTitle {
    id: "victoria3",
    display_name: "Victoria 3",
    nexus_domain: "victoria3",
    match_names: &["victoria 3", "victoria iii", "victoria3", "vic3"],
    steam_app_id: "529340",
    docs_folder: "Victoria 3",
};

macro_rules! paradox_plugin {
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
                &["mod"]
            }

            fn wrap_mod_folder_name(&self, content_root: &Path, staged_name: &str) -> String {
                content_folder_wrap_name(content_root, staged_name)
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                Ok(resolve_paradox_deploy(install_path, $title, relative))
            }

            fn after_deploy(
                &self,
                install_path: &Path,
                enabled_mod_folders: &[String],
            ) -> Result<Vec<String>> {
                write_descriptors_and_dlc_load(install_path, $title, enabled_mod_folders)
            }

            fn on_revert(&self, install_path: &Path) -> Result<()> {
                revert_dlc_load(install_path, $title)
            }
        }
    };
}

paradox_plugin!(HeartsOfIron4Plugin, HOI4);
paradox_plugin!(StellarisPlugin, STELLARIS);
paradox_plugin!(CrusaderKings3Plugin, CK3);
paradox_plugin!(EuropaUniversalis4Plugin, EU4);
paradox_plugin!(Victoria3Plugin, VIC3);

fn game_user_dir(install_path: &Path, title: ParadoxTitle) -> Option<PathBuf> {
    let docs = super::user_data::documents_dir(install_path, title.steam_app_id)?;
    Some(docs.join("Paradox Interactive").join(title.docs_folder))
}

fn resolve_paradox_deploy(install_path: &Path, title: ParadoxTitle, relative: &Path) -> PathBuf {
    let mod_root = game_user_dir(install_path, title)
        .map(|d| d.join("mod"))
        .unwrap_or_else(|| install_path.join("mod"));
    let normalized = normalize_relative(relative);
    let mut comps = normalized.components();
    let first = comps
        .next()
        .and_then(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .unwrap_or_default();
    if first == "mod" {
        let rest: PathBuf = comps.collect();
        return mod_root.join(rest);
    }
    mod_root.join(&normalized)
}

fn write_descriptors_and_dlc_load(
    install_path: &Path,
    title: ParadoxTitle,
    enabled_mod_folders: &[String],
) -> Result<Vec<String>> {
    let Some(user_dir) = game_user_dir(install_path, title) else {
        return Ok(Vec::new());
    };
    let mod_root = user_dir.join("mod");
    std::fs::create_dir_all(&mod_root)?;

    let mut enabled_rel = Vec::new();
    for folder in enabled_mod_folders {
        let mod_dir = mod_root.join(folder);
        if !mod_dir.is_dir() {
            continue;
        }
        let descriptor_src = mod_dir.join("descriptor.mod");
        let descriptor_dst = mod_root.join(format!("{folder}.mod"));
        let path_line = format!("path=\"mod/{folder}\"");
        let content = if descriptor_src.is_file() {
            let raw = std::fs::read_to_string(&descriptor_src).unwrap_or_default();
            let mut filtered: Vec<String> = raw
                .lines()
                .filter(|line| !line.trim_start().starts_with("path="))
                .map(ToOwned::to_owned)
                .collect();
            filtered.push(path_line);
            filtered.join("\n") + "\n"
        } else {
            format!("name=\"{folder}\"\n{path_line}\n")
        };
        std::fs::write(&descriptor_dst, content)
            .with_context(|| format!("write {}", descriptor_dst.display()))?;
        enabled_rel.push(format!("mod/{folder}.mod"));
    }

    let dlc_load = user_dir.join("dlc_load.json");
    let backup = user_dir.join("dlc_load.json.emperor.bak");
    if dlc_load.is_file() && !backup.is_file() {
        let _ = std::fs::copy(&dlc_load, &backup);
    }
    let mut existing_dlcs: Vec<String> = Vec::new();
    if let Ok(raw) = std::fs::read_to_string(&dlc_load) {
        if let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(arr) = val.get("disabled_dlcs").and_then(|v| v.as_array()) {
                existing_dlcs = arr
                    .iter()
                    .filter_map(|v| v.as_str().map(ToOwned::to_owned))
                    .collect();
            }
        }
    }
    let payload = serde_json::json!({
        "disabled_dlcs": existing_dlcs,
        "enabled_mods": enabled_rel,
    });
    std::fs::write(&dlc_load, serde_json::to_string_pretty(&payload)?)
        .with_context(|| format!("write {}", dlc_load.display()))?;

    Ok(Vec::new())
}

fn revert_dlc_load(install_path: &Path, title: ParadoxTitle) -> Result<()> {
    let Some(user_dir) = game_user_dir(install_path, title) else {
        return Ok(());
    };
    let dlc_load = user_dir.join("dlc_load.json");
    let backup = user_dir.join("dlc_load.json.emperor.bak");
    if backup.is_file() {
        std::fs::rename(&backup, &dlc_load)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_under_paradox_mod_dir() {
        let dest = StellarisPlugin
            .resolve_deploy_root(Path::new("/game"), Path::new("Gigastructures/descriptor.mod"))
            .unwrap();
        assert!(dest.ends_with("mod/Gigastructures/descriptor.mod"));
    }

    #[test]
    fn strips_redundant_mod_prefix() {
        let dest = HeartsOfIron4Plugin
            .resolve_deploy_root(Path::new("/game"), Path::new("mod/Kaiserreich/descriptor.mod"))
            .unwrap();
        assert!(dest.ends_with("mod/Kaiserreich/descriptor.mod"));
        assert!(!dest.to_string_lossy().contains("mod/mod/"));
    }
}
