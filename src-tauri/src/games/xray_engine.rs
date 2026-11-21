//! Shared deploy logic for S.T.A.L.K.E.R. X-Ray Engine titles
//! (S.T.A.L.K.E.R.: Anomaly, Shadow of Chernobyl, Call of Pripyat).
//!
//! Mods overlay into `<Install>/gamedata/`, with `bin/` and `appdata/` preserved at root.

use std::path::{Component, Path, PathBuf};

use anyhow::Result;

use super::{normalize_relative, GamePlugin, GamePluginInfo};

pub const XRAY_ROOT_DIRS: &[&str] = &["gamedata", "bin", "appdata", "db"];

macro_rules! xray_plugin {
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
                XRAY_ROOT_DIRS
            }

            fn preflight_warnings(&self, install_path: &Path) -> Vec<String> {
                check_fsgame_ltx(install_path)
            }

            fn resolve_deploy_root(&self, install_path: &Path, relative: &Path) -> Result<PathBuf> {
                Ok(resolve_xray_deploy(install_path, relative))
            }
        }
    };
}

xray_plugin!(
    StalkerAnomalyPlugin,
    "stalkeranomaly",
    "S.T.A.L.K.E.R.: Anomaly",
    "stalkeranomaly",
    &["s.t.a.l.k.e.r. anomaly", "stalker anomaly", "stalkeranomaly"]
);

xray_plugin!(
    StalkerShadowOfChernobylPlugin,
    "stalkershadowofchernobyl",
    "S.T.A.L.K.E.R.: Shadow of Chernobyl",
    "stalkershadowofchernobyl",
    &[
        "shadow of chernobyl",
        "shadow of chornobyl",
        "stalkershadowofchernobyl",
    ]
);

xray_plugin!(
    StalkerCallOfPripyatPlugin,
    "stalkercallofpripyat",
    "S.T.A.L.K.E.R.: Call of Pripyat",
    "stalkercallofpripyat",
    &["call of pripyat", "stalkercallofpripyat"]
);

fn resolve_xray_deploy(install_path: &Path, relative: &Path) -> PathBuf {
    let normalized = normalize_relative(relative);
    let first = normalized
        .components()
        .next()
        .and_then(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().to_lowercase()),
            _ => None,
        })
        .unwrap_or_default();
    if XRAY_ROOT_DIRS.iter().any(|r| *r == first.as_str()) {
        return install_path.join(&normalized);
    }
    install_path.join("gamedata").join(&normalized)
}

fn check_fsgame_ltx(install_path: &Path) -> Vec<String> {
    let fsgame = install_path.join("fsgame.ltx");
    let Ok(content) = std::fs::read_to_string(&fsgame) else {
        return Vec::new();
    };
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("$game_data$") {
            let lower = trimmed.to_ascii_lowercase();
            if lower.contains("false") {
                return vec![
                    "fsgame.ltx has $game_data$ set to false. Set both flags to true so loose gamedata/ mods load."
                        .into(),
                ];
            }
        }
    }
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routes_gamedata_and_bare_configs() {
        let install = Path::new("/stalker");
        assert_eq!(
            StalkerAnomalyPlugin
                .resolve_deploy_root(install, Path::new("gamedata/configs/actor.ltx"))
                .unwrap(),
            PathBuf::from("/stalker/gamedata/configs/actor.ltx")
        );
        assert_eq!(
            StalkerShadowOfChernobylPlugin
                .resolve_deploy_root(install, Path::new("configs/actor.ltx"))
                .unwrap(),
            PathBuf::from("/stalker/gamedata/configs/actor.ltx")
        );
    }

    #[test]
    fn warns_when_fsgame_disables_gamedata() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("fsgame.ltx"),
            "$game_data$ = false| true| $fs_root$| gamedata\\\n",
        )
        .unwrap();
        assert!(!StalkerCallOfPripyatPlugin
            .preflight_warnings(tmp.path())
            .is_empty());
    }
}
