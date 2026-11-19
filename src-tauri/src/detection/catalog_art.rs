//! Remote library posters from a game's already-linked catalog ids.
//!
//! Nexus tile art is a 2:3 portrait. mod.io's game logo is landscape and is
//! only used when Nexus has no tile. Thunderstore communities have no game art.

use std::collections::HashMap;

/// Parent folder `icons`, or a Steam `{appid}_icon` file. These are not posters.
pub fn is_deferred_icon_path(path: &str) -> bool {
    let path = path.replace('\\', "/");
    let mut parts = path.rsplit('/');
    let file = parts.next().unwrap_or("").to_ascii_lowercase();
    let parent = parts.next().unwrap_or("").to_ascii_lowercase();
    parent == "icons" || file.contains("_icon.")
}

pub fn nexus_tile_url(game_id: u64) -> String {
    format!("https://images.nexusmods.com/images/games/v2/{game_id}/tile.jpg")
}

/// Nexus portrait tile when the domain is in the games list, otherwise a mod.io logo.
pub fn catalog_cover_url(
    nexus_domain: Option<&str>,
    nexus_ids: &HashMap<String, u64>,
    modio_logo: Option<&str>,
) -> Option<String> {
    if let Some(domain) = nexus_domain
        .map(str::trim)
        .filter(|domain| !domain.is_empty())
    {
        if let Some(id) = nexus_ids.get(&domain.to_ascii_lowercase()).copied() {
            if id > 0 {
                return Some(nexus_tile_url(id));
            }
        }
    }
    modio_logo
        .map(str::trim)
        .filter(|url| url.starts_with("https://") || url.starts_with("http://"))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_catalog_ids_stay_blank() {
        let ids = HashMap::new();
        assert!(catalog_cover_url(None, &ids, None).is_none());
        assert!(catalog_cover_url(Some("  "), &ids, None).is_none());
        assert!(catalog_cover_url(Some("cyberpunk2077"), &ids, None).is_none());
        assert!(catalog_cover_url(Some("cyberpunk2077"), &ids, Some("not a url")).is_none());
    }

    #[test]
    fn nexus_tile_beats_modio_logo() {
        let mut ids = HashMap::new();
        ids.insert("cyberpunk2077".to_string(), 952);
        let url = catalog_cover_url(
            Some("Cyberpunk2077"),
            &ids,
            Some("https://thumb.modcdn.io/logo.png"),
        )
        .unwrap();
        assert_eq!(
            url,
            "https://images.nexusmods.com/images/games/v2/952/tile.jpg"
        );
    }

    #[test]
    fn modio_logo_when_nexus_has_no_tile() {
        let ids = HashMap::new();
        let url = catalog_cover_url(None, &ids, Some("https://thumb.modcdn.io/logo.png")).unwrap();
        assert_eq!(url, "https://thumb.modcdn.io/logo.png");
    }

    #[test]
    fn deferred_icon_paths() {
        assert!(is_deferred_icon_path(
            "/home/luu/.config/heroic/icons/1423049311.png"
        ));
        assert!(is_deferred_icon_path(
            r"C:\Steam\appcache\librarycache\413150_icon.jpg"
        ));
        assert!(!is_deferred_icon_path(
            "/home/luu/.cache/emperor/covers/heroic-1423049311.jpg"
        ));
    }
}
