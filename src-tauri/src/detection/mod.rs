//! Installed-game detection (Linux via lib_game_detector; Windows via Steam, the Epic Games Launcher, and Heroic).

use std::path::Path;

use serde::Serialize;

use crate::games::{match_plugin, GamePluginInfo};

mod catalog_art;
mod heroic_art;
pub mod reconcile;
mod steam_shortcuts;

pub(crate) use catalog_art::{
    apply_cached_catalog_covers, catalog_cover_url, is_deferred_icon_path, modio_cache_key,
    nexus_cache_key, now_unix_secs, write_cached_image, CacheLookup, CachedCatalogCover,
    CatalogCoverCache, MAX_CATALOG_COVER_BYTES,
};

#[cfg(target_os = "linux")]
mod linux;
// Compile under `test` on non-Windows so pure Steam and Epic helpers are covered in CI.
#[cfg(any(target_os = "windows", test))]
mod epic;
#[cfg(any(target_os = "windows", test))]
mod windows;

#[derive(Debug, Clone, Serialize)]
pub struct DetectedGame {
    pub id: String,
    pub title: String,
    pub install_path: Option<String>,
    pub launcher: String,
    pub supported: bool,
    pub plugin_id: Option<String>,
    pub nexus_domain: Option<String>,
    /// Local poster file. Never a launcher icon; those live in `icon_path`.
    pub cover_path: Option<String>,
    /// Remote poster (Heroic store art or a linked catalog tile).
    #[serde(default)]
    pub cover_url: Option<String>,
    /// Square launcher icon, used only when no poster is available.
    #[serde(default)]
    pub icon_path: Option<String>,
    /// When set (e.g. "unreal"), the install can be managed via a generic engine plugin.
    pub engine_hint: Option<String>,
}

pub(crate) fn make_id(title: &str, launcher: &str, path: &Option<String>) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(title.as_bytes());
    hasher.update(b"|");
    hasher.update(launcher.as_bytes());
    hasher.update(b"|");
    if let Some(p) = path {
        hasher.update(p.as_bytes());
    }
    hex::encode(hasher.finalize())[..16].to_string()
}

pub(crate) fn finish_detected(
    title: String,
    launcher: String,
    install_path: Option<String>,
    cover_path: Option<String>,
) -> DetectedGame {
    let plugin = match_plugin(&title, install_path.as_deref());
    let supported = plugin.is_some();
    let (plugin_id, nexus_domain) = match plugin {
        Some(GamePluginInfo {
            id, nexus_domain, ..
        }) => (Some(id.to_string()), Some(nexus_domain.to_string())),
        None => (None, None),
    };

    let engine_hint = if supported {
        None
    } else {
        install_path.as_deref().and_then(|p| {
            let path = Path::new(p);
            if crate::games::detect_ue_layout(path).is_some() {
                Some("unreal".to_string())
            } else if crate::games::looks_like_unity_install(path) {
                Some("bepinex".to_string())
            } else {
                None
            }
        })
    };

    let id = make_id(&title, &launcher, &install_path);
    DetectedGame {
        id,
        title,
        install_path,
        launcher,
        supported,
        plugin_id,
        nexus_domain,
        cover_path,
        cover_url: None,
        icon_path: None,
        engine_hint,
    }
}

/// Copy a freshly detected poster onto a managed game.
///
/// A local file wins and clears any remote URL. A remote poster clears a stored
/// path so an old Heroic icon does not cover it. When detection has neither,
/// the stored art is left alone (the icon can stay until a catalog tile replaces it).
pub fn sync_managed_cover(
    cover_path: &mut Option<String>,
    cover_url: &mut Option<String>,
    detected: &DetectedGame,
) -> bool {
    if detected.cover_path.is_some() {
        let changed = cover_path != &detected.cover_path || cover_url.is_some();
        *cover_path = detected.cover_path.clone();
        *cover_url = None;
        changed
    } else if detected.cover_url.is_some() {
        let changed = cover_url != &detected.cover_url || cover_path.is_some();
        *cover_url = detected.cover_url.clone();
        *cover_path = None;
        changed
    } else {
        false
    }
}

pub fn scan_games() -> Vec<DetectedGame> {
    #[cfg(target_os = "linux")]
    {
        return linux::scan_games();
    }
    #[cfg(target_os = "windows")]
    {
        return windows::scan_games();
    }
    #[cfg(not(any(target_os = "linux", target_os = "windows")))]
    {
        Vec::new()
    }
}
