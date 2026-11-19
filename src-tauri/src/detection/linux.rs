use lib_game_detector::data::SupportedLaunchers;
use lib_game_detector::get_detector;

use super::heroic_art::{is_heroic_launcher, HeroicResolver, LauncherArt};
use super::steam_shortcuts::{
    clean_shortcut_title, installs_by_title, linux_steam_roots, shortcut_games_from_roots,
    STEAM_SHORTCUTS_LAUNCHER,
};
use super::{finish_detected, DetectedGame};

fn launcher_label(source: &SupportedLaunchers) -> String {
    format!("{source}")
}

pub fn scan_games() -> Vec<DetectedGame> {
    let detector = get_detector();
    let games = detector.get_all_detected_games();
    let shortcut_installs = installs_by_title(&shortcut_games_from_roots(linux_steam_roots()));
    let mut heroic = HeroicResolver::discover();
    let mut out = Vec::with_capacity(games.len());

    for game in games {
        let launcher = if game.source == SupportedLaunchers::SteamShortcuts {
            STEAM_SHORTCUTS_LAUNCHER.to_string()
        } else {
            launcher_label(&game.source)
        };
        let mut install_path = game
            .path_game_dir
            .as_ref()
            .map(|p| p.to_string_lossy().to_string());
        // Steam shortcuts are detected by title only. Fill the folder from shortcuts.vdf
        // so Unity and Unreal installs can be managed.
        if install_path.is_none() && game.source == SupportedLaunchers::SteamShortcuts {
            let title = clean_shortcut_title(&game.title);
            if let Some(path) = shortcut_installs.get(&title) {
                install_path = Some(path.to_string_lossy().to_string());
            }
        }
        let art = launcher_art(
            &launcher,
            install_path.as_deref(),
            game.path_box_art.as_deref(),
            game.path_icon.as_deref(),
            heroic.as_mut(),
        );
        let mut detected = finish_detected(game.title, launcher, install_path, art.cover_path);
        detected.cover_url = art.cover_url;
        detected.icon_path = art.icon_path;
        out.push(detected);
    }

    out.sort_by(|a, b| {
        b.supported
            .cmp(&a.supported)
            .then_with(|| b.engine_hint.is_some().cmp(&a.engine_hint.is_some()))
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    out
}

fn launcher_art(
    launcher: &str,
    install_path: Option<&str>,
    box_art: Option<&std::path::Path>,
    icon: Option<&std::path::Path>,
    heroic: Option<&mut HeroicResolver>,
) -> LauncherArt {
    if is_heroic_launcher(launcher) {
        // Detector paths for Heroic are the square icon under icons/.
        let icon = icon.or(box_art);
        return match heroic {
            Some(resolver) => resolver.resolve(launcher, install_path, icon),
            None => LauncherArt {
                cover_path: None,
                cover_url: None,
                icon_path: icon
                    .filter(|path| path.is_file())
                    .map(|path| path.to_string_lossy().to_string()),
            },
        };
    }
    let cover_path = box_art
        .filter(|path| path.is_file())
        .map(|path| path.to_string_lossy().to_string());
    let icon_path = if cover_path.is_none() {
        icon.filter(|path| path.is_file())
            .map(|path| path.to_string_lossy().to_string())
    } else {
        None
    };
    LauncherArt {
        cover_path,
        cover_url: None,
        icon_path,
    }
}
