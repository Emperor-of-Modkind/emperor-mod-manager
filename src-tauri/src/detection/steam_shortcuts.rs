//! Install directories for non-Steam games added as Steam shortcuts.
//!
//! `lib_game_detector` lists these games but leaves `path_game_dir` empty, so
//! Unity and Unreal detection never runs. The shortcut's `StartDir` and `exe`
//! in `userdata/*/config/shortcuts.vdf` are the install folder.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use steam_shortcuts_util::parse_shortcuts;

/// Launcher label `lib_game_detector` uses for [`SupportedLaunchers::SteamShortcuts`].
pub const STEAM_SHORTCUTS_LAUNCHER: &str = "Steam (shortcuts)";

/// How many parents above an exe to search for a Unity or Unreal root.
///
/// A shipping Unreal exe lives in `<install>/<Project>/Binaries/Win64`, so the
/// install root is three levels up. A few extra steps cover nested launchers.
const EXE_WALK_LIMIT: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShortcutInstall {
    pub title: String,
    pub install_path: PathBuf,
    pub app_id: u32,
}

/// Same cleanup as `lib_game_detector`'s `clean_game_title`: drop ™ and ®.
pub fn clean_shortcut_title(title: &str) -> String {
    title.replace(['™', '®'], "")
}

/// Strip one pair of surrounding quotes. Steam stores some exe paths quoted.
pub fn unquote_shortcut_path(raw: &str) -> String {
    let trimmed = raw.trim();
    trimmed
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(trimmed)
        .trim()
        .to_string()
}

/// Pick the game folder from a shortcut's working directory and exe.
///
/// Prefer `StartDir` when that folder is already a Unity or Unreal install.
/// Otherwise walk up from the exe so `Binaries/Win64` shortcuts still resolve
/// to the install root. A bare launcher name such as `flatpak` is not a folder.
pub fn resolve_shortcut_install(start_dir: &str, exe: &str) -> Option<PathBuf> {
    let start = existing_dir(&unquote_shortcut_path(start_dir));
    if start
        .as_ref()
        .is_some_and(|dir| looks_like_engine_install(dir))
    {
        return start;
    }
    if let Some(root) = engine_root_from_exe(&unquote_shortcut_path(exe)) {
        return Some(root);
    }
    if let Some(start) = start {
        return Some(start);
    }
    exe_parent_dir(&unquote_shortcut_path(exe))
}

/// Every shortcut under `steam_roots` that points at a real directory.
pub fn shortcut_games_from_roots<I, P>(steam_roots: I) -> Vec<ShortcutInstall>
where
    I: IntoIterator<Item = P>,
    P: AsRef<Path>,
{
    let mut out = Vec::new();
    for root in steam_roots {
        for vdf in shortcut_vdf_paths(root.as_ref()) {
            out.extend(read_shortcuts_file(&vdf));
        }
    }
    out
}

/// First install path for each cleaned title.
pub fn installs_by_title(games: &[ShortcutInstall]) -> HashMap<String, PathBuf> {
    let mut map = HashMap::new();
    for game in games {
        map.entry(game.title.clone())
            .or_insert_with(|| game.install_path.clone());
    }
    map
}

/// Steam roots `lib_game_detector` searches, including the Flatpak copy.
#[cfg(target_os = "linux")]
pub fn linux_steam_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let Ok(home) = std::env::var("HOME") else {
        return roots;
    };
    let home = PathBuf::from(home);
    push_steam_root(&mut roots, home.join(".steam/root"));
    push_steam_root(&mut roots, home.join(".steam/steam"));
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| home.join(".local/share"));
    push_steam_root(&mut roots, data_home.join("Steam"));
    push_steam_root(
        &mut roots,
        home.join(".var/app/com.valvesoftware.Steam/data/Steam"),
    );
    roots
}

#[cfg(target_os = "linux")]
fn push_steam_root(roots: &mut Vec<PathBuf>, path: PathBuf) {
    if !path.is_dir() {
        return;
    }
    let canonical = path.canonicalize().unwrap_or(path);
    if roots.iter().any(|existing| existing == &canonical) {
        return;
    }
    roots.push(canonical);
}

fn shortcut_vdf_paths(steam_root: &Path) -> Vec<PathBuf> {
    let userdata = steam_root.join("userdata");
    let Ok(entries) = fs::read_dir(&userdata) else {
        return Vec::new();
    };
    let mut paths = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false) {
            continue;
        }
        let vdf = entry.path().join("config").join("shortcuts.vdf");
        if vdf.is_file() {
            paths.push(vdf);
        }
    }
    paths
}

fn read_shortcuts_file(path: &Path) -> Vec<ShortcutInstall> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) => {
            log::warn!(
                "Could not read Steam shortcuts at {}: {err}",
                path.display()
            );
            return Vec::new();
        }
    };
    let shortcuts = match parse_shortcuts(&bytes) {
        Ok(shortcuts) => shortcuts,
        Err(err) => {
            log::warn!(
                "Could not parse Steam shortcuts at {}: {err}",
                path.display()
            );
            return Vec::new();
        }
    };
    shortcuts
        .into_iter()
        .filter_map(|shortcut| {
            let install_path = resolve_shortcut_install(shortcut.start_dir, shortcut.exe)?;
            let title = clean_shortcut_title(shortcut.app_name);
            if title.trim().is_empty() {
                return None;
            }
            Some(ShortcutInstall {
                title,
                install_path,
                app_id: shortcut.app_id,
            })
        })
        .collect()
}

fn looks_like_engine_install(dir: &Path) -> bool {
    crate::games::detect_ue_layout(dir).is_some() || crate::games::looks_like_unity_install(dir)
}

fn existing_dir(raw: &str) -> Option<PathBuf> {
    if is_non_path_target(raw) {
        return None;
    }
    let path = PathBuf::from(raw);
    if path.is_dir() && path.parent().is_some() {
        Some(path)
    } else {
        None
    }
}

fn exe_parent_dir(exe: &str) -> Option<PathBuf> {
    if is_non_path_target(exe) {
        return None;
    }
    let parent = Path::new(exe).parent()?;
    if parent.as_os_str().is_empty() || parent.parent().is_none() || !parent.is_dir() {
        return None;
    }
    Some(parent.to_path_buf())
}

fn engine_root_from_exe(exe: &str) -> Option<PathBuf> {
    if is_non_path_target(exe) {
        return None;
    }
    let mut dir = Path::new(exe).parent()?;
    for _ in 0..EXE_WALK_LIMIT {
        if dir.as_os_str().is_empty() {
            break;
        }
        if dir.is_dir() && looks_like_engine_install(dir) {
            return Some(dir.to_path_buf());
        }
        dir = dir.parent()?;
    }
    None
}

/// `flatpak` is a launch command, not a game directory.
fn is_non_path_target(raw: &str) -> bool {
    if raw.is_empty() {
        return true;
    }
    Path::new(raw)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(raw)
        .eq_ignore_ascii_case("flatpak")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use steam_shortcuts_util::{shortcuts_to_bytes, Shortcut};

    fn write_shortcuts(steam_root: &Path, shortcuts: &[Shortcut]) {
        let dir = steam_root.join("userdata").join("1").join("config");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("shortcuts.vdf"),
            shortcuts_to_bytes(&shortcuts.to_vec()),
        )
        .unwrap();
    }

    fn unity_install(root: &Path) -> PathBuf {
        let install = root.join("CardShop");
        fs::create_dir_all(install.join("Game_Data").join("Managed")).unwrap();
        fs::write(install.join("UnityPlayer.dll"), b"unity").unwrap();
        fs::write(install.join("Game.exe"), b"exe").unwrap();
        install
    }

    fn unreal_install(root: &Path) -> (PathBuf, PathBuf) {
        let install = root.join("Hellraiser");
        let exe = install
            .join("Hellraiser")
            .join("Binaries")
            .join("Win64")
            .join("Hellraiser.exe");
        fs::create_dir_all(install.join("Hellraiser").join("Content").join("Paks")).unwrap();
        fs::create_dir_all(exe.parent().unwrap()).unwrap();
        fs::write(&exe, b"exe").unwrap();
        (install, exe)
    }

    #[test]
    fn clean_shortcut_title_strips_marks() {
        assert_eq!(clean_shortcut_title("Game™®"), "Game");
    }

    #[test]
    fn quoted_unity_shortcut_resolves_and_flatpak_does_not() {
        let tmp = tempfile::tempdir().unwrap();
        let unity = unity_install(tmp.path());
        let exe = format!("\"{}\"", unity.join("Game.exe").display());
        let start = unity.display().to_string();
        let unity_shortcut = Shortcut::new("0", "Card Shop™", &exe, &start, "", "", "");
        let flatpak = Shortcut::new("1", "RetroDECK", "\"flatpak\"", "", "", "", "");
        write_shortcuts(tmp.path(), &[unity_shortcut, flatpak]);

        let games = shortcut_games_from_roots([tmp.path()]);
        assert_eq!(games.len(), 1);
        assert_eq!(games[0].title, "Card Shop");
        assert_eq!(games[0].install_path, unity);
        assert!(crate::games::looks_like_unity_install(
            &games[0].install_path
        ));

        let detected = super::super::finish_detected(
            games[0].title.clone(),
            STEAM_SHORTCUTS_LAUNCHER.to_string(),
            Some(games[0].install_path.to_string_lossy().to_string()),
            None,
        );
        assert_eq!(detected.engine_hint.as_deref(), Some("bepinex"));
        assert!(!detected.supported);

        let by_title = installs_by_title(&games);
        assert_eq!(
            by_title.get("Card Shop").map(PathBuf::as_path),
            Some(unity.as_path())
        );
        assert!(by_title.get("RetroDECK").is_none());
    }

    #[test]
    fn unreal_exe_under_binaries_resolves_to_install_root() {
        let tmp = tempfile::tempdir().unwrap();
        let (install, exe) = unreal_install(tmp.path());
        let win64 = exe.parent().unwrap();
        let quoted = format!("\"{}\"", exe.display());
        let resolved = resolve_shortcut_install(&win64.display().to_string(), &quoted).unwrap();
        assert_eq!(resolved, install);

        let layout = crate::games::detect_ue_layout(&resolved).unwrap();
        assert_eq!(layout.project_name, "Hellraiser");

        let detected = super::super::finish_detected(
            "Clive Barker's Hellraiser - Revival".to_string(),
            STEAM_SHORTCUTS_LAUNCHER.to_string(),
            Some(resolved.to_string_lossy().to_string()),
            None,
        );
        assert_eq!(detected.engine_hint.as_deref(), Some("unreal"));

        let start_is_root =
            resolve_shortcut_install(&install.display().to_string(), &quoted).unwrap();
        assert_eq!(start_is_root, install);
    }

    #[test]
    fn flatpak_target_without_a_directory_has_no_install() {
        assert!(resolve_shortcut_install("", "\"flatpak\"").is_none());
        assert!(resolve_shortcut_install("", "flatpak").is_none());
    }
}
