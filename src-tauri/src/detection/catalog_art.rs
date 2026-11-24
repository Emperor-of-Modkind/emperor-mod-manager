//! Remote library posters from a game's already-linked catalog ids.
//!
//! Nexus tile art is a 2:3 portrait. mod.io's game logo is landscape and is
//! only used when Nexus has no tile. Thunderstore communities have no game art.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use super::DetectedGame;

pub const NEGATIVE_CACHE_TTL_SECS: u64 = 24 * 60 * 60;
pub const MAX_CATALOG_COVER_BYTES: usize = 10 * 1024 * 1024;
const INDEX_FILE_NAME: &str = "catalog-index.json";

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedCatalogCover {
    #[serde(default)]
    pub cover_path: Option<String>,
    #[serde(default)]
    pub cover_url: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub fetched_at: u64,
    #[serde(default)]
    pub missing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CacheLookup {
    Hit(CachedCatalogCover),
    Negative,
    Miss,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CatalogCoverCache {
    #[serde(default)]
    entries: HashMap<String, CachedCatalogCover>,
}

impl CatalogCoverCache {
    pub fn load(covers_dir: &Path) -> Self {
        let path = covers_dir.join(INDEX_FILE_NAME);
        let Ok(raw) = fs::read_to_string(&path) else {
            return Self::default();
        };
        serde_json::from_str(&raw).unwrap_or_default()
    }

    pub fn save(&self, covers_dir: &Path) -> std::io::Result<()> {
        fs::create_dir_all(covers_dir)?;
        let dest = covers_dir.join(INDEX_FILE_NAME);
        let tmp = covers_dir.join(format!("{INDEX_FILE_NAME}.tmp"));
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        fs::write(&tmp, json)?;
        fs::rename(&tmp, &dest)?;
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn insert(&mut self, key: String, entry: CachedCatalogCover) {
        self.entries.insert(key, entry);
    }

    pub fn lookup(
        &self,
        nexus_domain: Option<&str>,
        modio_game_id: Option<u32>,
        now_secs: u64,
    ) -> CacheLookup {
        if let Some(key) = nexus_domain.and_then(nexus_cache_key) {
            if let Some(result) = self.check_entry(&key, now_secs) {
                return result;
            }
        }
        if let Some(key) = modio_game_id.and_then(modio_cache_key) {
            if let Some(result) = self.check_entry(&key, now_secs) {
                return result;
            }
        }
        CacheLookup::Miss
    }

    fn check_entry(&self, key: &str, now_secs: u64) -> Option<CacheLookup> {
        let entry = self.entries.get(key)?;
        if entry.missing {
            return (now_secs.saturating_sub(entry.fetched_at) < NEGATIVE_CACHE_TTL_SECS)
                .then_some(CacheLookup::Negative);
        }
        let path = entry.cover_path.as_deref()?;
        Path::new(path)
            .is_file()
            .then(|| CacheLookup::Hit(entry.clone()))
    }
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

pub fn nexus_cache_key(domain: &str) -> Option<String> {
    let trimmed = domain.trim();
    (!trimmed.is_empty()).then(|| format!("nexus:{}", trimmed.to_ascii_lowercase()))
}

pub fn modio_cache_key(game_id: u32) -> Option<String> {
    (game_id > 0).then(|| format!("modio:{game_id}"))
}

pub fn image_extension_from_bytes(bytes: &[u8], url: &str) -> &'static str {
    if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "jpg"
    } else if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
        "png"
    } else if bytes.len() >= 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        "webp"
    } else if bytes.starts_with(b"GIF8") {
        "gif"
    } else {
        extension_from_url(url).unwrap_or("jpg")
    }
}

fn extension_from_url(url: &str) -> Option<&'static str> {
    let path = url.split('?').next().unwrap_or(url);
    let ext = path.rsplit('.').next()?.to_ascii_lowercase();
    match ext.as_str() {
        "jpg" | "jpeg" => Some("jpg"),
        "png" => Some("png"),
        "webp" => Some("webp"),
        "gif" => Some("gif"),
        _ => None,
    }
}

fn safe_cache_stem(cache_key: &str) -> String {
    let mut out = String::with_capacity(cache_key.len());
    for ch in cache_key.chars() {
        if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
            out.push(ch);
        } else {
            out.push('-');
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "unknown".to_string()
    } else {
        out
    }
}

pub fn cache_file_path(covers_dir: &Path, cache_key: &str, ext: &str) -> PathBuf {
    covers_dir.join(format!("catalog-{}.{ext}", safe_cache_stem(cache_key)))
}

pub fn write_cached_image(
    covers_dir: &Path,
    cache_key: &str,
    url: &str,
    bytes: &[u8],
) -> std::io::Result<String> {
    fs::create_dir_all(covers_dir)?;
    let ext = image_extension_from_bytes(bytes, url);
    let dest = cache_file_path(covers_dir, cache_key, ext);
    let tmp = covers_dir.join(format!(
        "catalog-{}.{ext}.tmp",
        safe_cache_stem(cache_key)
    ));
    fs::write(&tmp, bytes)?;
    fs::rename(&tmp, &dest)?;
    Ok(dest.to_string_lossy().to_string())
}

/// Attach previously cached catalog posters onto detected games that still lack a real local cover.
pub fn apply_cached_catalog_covers(games: &mut [DetectedGame], covers_dir: &Path) {
    let cache = CatalogCoverCache::load(covers_dir);
    if cache.is_empty() {
        return;
    }
    let now = now_unix_secs();
    for game in games {
        if game.cover_url.is_some() {
            continue;
        }
        let needs_poster = match game.cover_path.as_deref() {
            None => true,
            Some(path) => {
                game.icon_path.as_deref() == Some(path) || is_deferred_icon_path(path)
            }
        };
        if !needs_poster {
            continue;
        }
        if let CacheLookup::Hit(entry) =
            cache.lookup(game.nexus_domain.as_deref(), None, now)
        {
            game.cover_path = entry.cover_path;
        }
    }
}

/// Attach previously cached catalog posters onto uninstalled library games that lack
/// a local cover (or append the cached remote URL as a fallback when Steam/store art may 404).
pub fn apply_cached_catalog_covers_to_libraries(
    sections: &mut [super::libraries::LibrarySection],
    covers_dir: &Path,
) {
    let cache = CatalogCoverCache::load(covers_dir);
    if cache.is_empty() {
        return;
    }
    let now = now_unix_secs();
    for section in sections {
        for game in &mut section.games {
            if game.cover_path.is_some() {
                continue;
            }
            let CacheLookup::Hit(entry) =
                cache.lookup(game.nexus_domain.as_deref(), None, now)
            else {
                continue;
            };
            if game.cover_url.is_none() {
                if entry.cover_path.is_some() {
                    game.cover_path = entry.cover_path;
                } else if let Some(url) = entry.cover_url {
                    game.cover_url = Some(url);
                }
            } else if let Some(url) = entry.cover_url {
                if game.cover_url.as_deref() != Some(url.as_str())
                    && !game.cover_fallbacks.contains(&url)
                {
                    game.cover_fallbacks.push(url);
                }
            }
        }
    }
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

    #[test]
    fn image_extension_detects_magic_bytes_and_falls_back_to_url() {
        assert_eq!(
            image_extension_from_bytes(&[0xFF, 0xD8, 0xFF, 0xE0], "https://x/img"),
            "jpg"
        );
        assert_eq!(
            image_extension_from_bytes(&[0x89, b'P', b'N', b'G', 0x0D], "https://x/img.jpg"),
            "png"
        );
        assert_eq!(
            image_extension_from_bytes(b"RIFF\x00\x00\x00\x00WEBP", "https://x/img"),
            "webp"
        );
        assert_eq!(
            image_extension_from_bytes(b"GIF89a", "https://x/img"),
            "gif"
        );
        assert_eq!(
            image_extension_from_bytes(b"unknown", "https://x/logo.png?v=1"),
            "png"
        );
    }

    #[test]
    fn cache_round_trip_and_missing_file_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let covers_dir = dir.path().join("covers");
        let saved_path = write_cached_image(
            &covers_dir,
            "nexus:cyberpunk2077",
            "https://images.nexusmods.com/images/games/v2/952/tile.jpg",
            &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10],
        )
        .unwrap();

        let mut cache = CatalogCoverCache::default();
        cache.insert(
            "nexus:cyberpunk2077".to_string(),
            CachedCatalogCover {
                cover_path: Some(saved_path.clone()),
                cover_url: Some("https://images.nexusmods.com/images/games/v2/952/tile.jpg".into()),
                title: Some("Cyberpunk 2077".into()),
                fetched_at: 1000,
                missing: false,
            },
        );
        cache.save(&covers_dir).unwrap();

        let loaded = CatalogCoverCache::load(&covers_dir);
        assert_eq!(
            loaded.lookup(Some("Cyberpunk2077"), Some(42), 2000),
            CacheLookup::Hit(CachedCatalogCover {
                cover_path: Some(saved_path.clone()),
                cover_url: Some("https://images.nexusmods.com/images/games/v2/952/tile.jpg".into()),
                title: Some("Cyberpunk 2077".into()),
                fetched_at: 1000,
                missing: false,
            })
        );

        std::fs::remove_file(&saved_path).unwrap();
        assert_eq!(
            loaded.lookup(Some("Cyberpunk2077"), None, 2000),
            CacheLookup::Miss
        );
    }

    #[test]
    fn negative_cache_expires_after_ttl() {
        let mut cache = CatalogCoverCache::default();
        cache.insert(
            "nexus:unknown".to_string(),
            CachedCatalogCover {
                cover_path: None,
                cover_url: None,
                title: None,
                fetched_at: 1000,
                missing: true,
            },
        );
        assert_eq!(
            cache.lookup(Some("unknown"), None, 1000 + NEGATIVE_CACHE_TTL_SECS - 1),
            CacheLookup::Negative
        );
        assert_eq!(
            cache.lookup(Some("unknown"), None, 1000 + NEGATIVE_CACHE_TTL_SECS),
            CacheLookup::Miss
        );
    }

    #[test]
    fn apply_cached_catalog_covers_respects_real_local_covers() {
        let dir = tempfile::tempdir().unwrap();
        let covers_dir = dir.path().join("covers");
        let cached_path = write_cached_image(
            &covers_dir,
            "nexus:skyrimspecialedition",
            "https://images.nexusmods.com/images/games/v2/1704/tile.jpg",
            &[0xFF, 0xD8, 0xFF, 0xE0],
        )
        .unwrap();

        let mut cache = CatalogCoverCache::default();
        cache.insert(
            "nexus:skyrimspecialedition".to_string(),
            CachedCatalogCover {
                cover_path: Some(cached_path.clone()),
                cover_url: Some("https://images.nexusmods.com/images/games/v2/1704/tile.jpg".into()),
                title: None,
                fetched_at: now_unix_secs(),
                missing: false,
            },
        );
        cache.save(&covers_dir).unwrap();

        let mut games = vec![
            DetectedGame {
                id: "1".into(),
                title: "Skyrim SE (no cover)".into(),
                install_path: None,
                launcher: "Steam".into(),
                supported: true,
                plugin_id: None,
                nexus_domain: Some("skyrimspecialedition".into()),
                cover_path: Some("/home/u/.config/heroic/icons/123.png".into()),
                cover_url: None,
                icon_path: Some("/home/u/.config/heroic/icons/123.png".into()),
                engine_hint: None,
            },
            DetectedGame {
                id: "2".into(),
                title: "Skyrim SE (real local)".into(),
                install_path: None,
                launcher: "Steam".into(),
                supported: true,
                plugin_id: None,
                nexus_domain: Some("skyrimspecialedition".into()),
                cover_path: Some("/steam/librarycache/489830/library_600x900.jpg".into()),
                cover_url: None,
                icon_path: None,
                engine_hint: None,
            },
        ];

        apply_cached_catalog_covers(&mut games, &covers_dir);
        assert_eq!(games[0].cover_path.as_deref(), Some(cached_path.as_str()));
        assert_eq!(
            games[1].cover_path.as_deref(),
            Some("/steam/librarycache/489830/library_600x900.jpg")
        );
    }
}
