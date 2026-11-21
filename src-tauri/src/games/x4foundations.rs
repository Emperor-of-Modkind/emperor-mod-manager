//! X4: Foundations — mod folders (with `content.xml`) in `<Install>/extensions/<ModName>/`.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{content_folder_wrap_name, normalize_relative, GamePlugin, GamePluginInfo};

pub struct X4FoundationsPlugin;

impl GamePlugin for X4FoundationsPlugin {
    fn info(&self) -> GamePluginInfo {
        GamePluginInfo {
            id: "x4foundations",
            display_name: "X4: Foundations",
            nexus_domain: "x4foundations",
            match_names: &["x4 foundations", "x4: foundations", "x4foundations"],
        }
    }

    fn preserve_staging_root_names(&self) -> &[&str] {
        &["extensions"]
    }

    fn should_wrap_as_mod_folder(&self, content_root: &Path) -> bool {
        content_root.join("content.xml").is_file()
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
        if first == "extensions" {
            let rest: PathBuf = comps.collect();
            return Ok(install_path.join("extensions").join(rest));
        }
        Ok(install_path.join("extensions").join(&normalized))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deploys_content_xml_under_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("content.xml"), "<content/>").unwrap();
        assert!(X4FoundationsPlugin.should_wrap_as_mod_folder(tmp.path()));
        assert_eq!(
            X4FoundationsPlugin
                .resolve_deploy_root(Path::new("/x4"), Path::new("vro/content.xml"))
                .unwrap(),
            PathBuf::from("/x4/extensions/vro/content.xml")
        );
    }
}
