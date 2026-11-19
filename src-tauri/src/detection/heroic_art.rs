//! Heroic store posters.
//!
//! `lib_game_detector` points Heroic "box art" at `icons/{app_id}.*`, which is the
//! square icon. Heroic keeps the real poster URL on the library entry and caches
//! the bytes at `images-cache/{sha256(url)}` with no extension.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;
use sha2::{Digest, Sha256};

use super::is_deferred_icon_path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroicStore {
    Epic,
    Gog,
    Amazon,
    Sideload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherArt {
    pub cover_path: Option<String>,
    pub cover_url: Option<String>,
    pub icon_path: Option<String>,
}

impl LauncherArt {
    fn empty() -> Self {
        Self {
            cover_path: None,
            cover_url: None,
            icon_path: None,
        }
    }
}

#[derive(Debug, Clone)]
struct ArtRecord {
    url: String,
    title: Option<String>,
}

pub struct HeroicResolver {
    config_dir: PathBuf,
    covers_dir: Option<PathBuf>,
    index: Option<HeroicArtIndex>,
}

struct HeroicArtIndex {
    epic: HashMap<String, ArtRecord>,
    gog: HashMap<String, ArtRecord>,
    amazon: HashMap<String, ArtRecord>,
    sideload: HashMap<String, ArtRecord>,
    /// Normalized install path → (store, app id).
    installs: HashMap<String, (HeroicStore, String)>,
}

impl HeroicResolver {
    /// Locate a Heroic config directory and the app cache folder used for posters.
    pub fn discover() -> Option<Self> {
        let config_dir = heroic_config_dir()?;
        Some(Self::from_dir(config_dir, app_covers_dir()))
    }

    pub fn from_dir(config_dir: PathBuf, covers_dir: Option<PathBuf>) -> Self {
        Self {
            config_dir,
            covers_dir,
            index: None,
        }
    }

    pub fn resolve(
        &mut self,
        launcher: &str,
        install_path: Option<&str>,
        icon: Option<&Path>,
    ) -> LauncherArt {
        let store = store_from_launcher(launcher);
        let app_id = app_id_from_icon(icon).or_else(|| {
            install_path.and_then(|path| {
                self.index()
                    .installs
                    .get(&install_key(path))
                    .map(|(_, id)| id.clone())
            })
        });
        self.resolve_store(store, app_id.as_deref(), icon)
    }

    pub fn resolve_store(
        &mut self,
        store: Option<HeroicStore>,
        app_id: Option<&str>,
        icon: Option<&Path>,
    ) -> LauncherArt {
        let icon_path = icon
            .filter(|path| path.is_file())
            .map(|path| path.to_string_lossy().to_string());
        let Some(app_id) = app_id
            .map(str::trim)
            .filter(|id| !id.is_empty())
            .map(str::to_string)
            .or_else(|| app_id_from_icon(icon))
        else {
            let mut art = LauncherArt::empty();
            art.icon_path = icon_path;
            return art;
        };
        let Some(url) = self.index().best_url(store, &app_id) else {
            let mut art = LauncherArt::empty();
            art.icon_path = icon_path;
            return art;
        };
        if let Some(path) = self.materialize_cached(&app_id, &url) {
            return LauncherArt {
                cover_path: Some(path),
                cover_url: None,
                icon_path,
            };
        }
        LauncherArt {
            cover_path: None,
            cover_url: Some(url),
            icon_path,
        }
    }

    pub fn library_title(&mut self, store: HeroicStore, app_id: &str) -> Option<String> {
        self.index()
            .records(store)
            .get(app_id)
            .and_then(|record| record.title.clone())
            .filter(|title| !title.trim().is_empty())
    }

    fn index(&mut self) -> &HeroicArtIndex {
        if self.index.is_none() {
            self.index = Some(HeroicArtIndex::load(&self.config_dir));
        }
        self.index.as_ref().expect("heroic index")
    }

    fn materialize_cached(&self, app_id: &str, url: &str) -> Option<String> {
        let cache = cache_path(&self.config_dir, url);
        if !cache.is_file() {
            return None;
        }
        let covers = self.covers_dir.as_deref()?;
        let ext = image_extension(&cache, url);
        let dest = covers.join(format!("heroic-{}.{ext}", safe_app_id(app_id)));
        if dest.is_file() && !source_is_newer(&cache, &dest) {
            return Some(dest.to_string_lossy().to_string());
        }
        fs::create_dir_all(covers).ok()?;
        fs::copy(&cache, &dest).ok()?;
        Some(dest.to_string_lossy().to_string())
    }
}

impl HeroicArtIndex {
    fn load(config: &Path) -> Self {
        let mut index = Self {
            epic: HashMap::new(),
            gog: HashMap::new(),
            amazon: HashMap::new(),
            sideload: HashMap::new(),
            installs: HashMap::new(),
        };
        index.load_library(
            &config.join("store_cache").join("legendary_library.json"),
            HeroicStore::Epic,
        );
        index.load_library(
            &config.join("store_cache").join("gog_library.json"),
            HeroicStore::Gog,
        );
        index.load_library(
            &config.join("store_cache").join("nile_library.json"),
            HeroicStore::Amazon,
        );
        index.load_library(
            &config.join("sideload_apps").join("library.json"),
            HeroicStore::Sideload,
        );
        index.load_installs(
            &config
                .join("legendaryConfig")
                .join("legendary")
                .join("installed.json"),
            HeroicStore::Epic,
        );
        index.load_installs(
            &config.join("gog_store").join("installed.json"),
            HeroicStore::Gog,
        );
        index
    }

    fn records(&self, store: HeroicStore) -> &HashMap<String, ArtRecord> {
        match store {
            HeroicStore::Epic => &self.epic,
            HeroicStore::Gog => &self.gog,
            HeroicStore::Amazon => &self.amazon,
            HeroicStore::Sideload => &self.sideload,
        }
    }

    fn records_mut(&mut self, store: HeroicStore) -> &mut HashMap<String, ArtRecord> {
        match store {
            HeroicStore::Epic => &mut self.epic,
            HeroicStore::Gog => &mut self.gog,
            HeroicStore::Amazon => &mut self.amazon,
            HeroicStore::Sideload => &mut self.sideload,
        }
    }

    fn best_url(&self, store: Option<HeroicStore>, app_id: &str) -> Option<String> {
        if let Some(store) = store {
            return self
                .records(store)
                .get(app_id)
                .map(|record| record.url.clone());
        }
        for store in [
            HeroicStore::Gog,
            HeroicStore::Epic,
            HeroicStore::Amazon,
            HeroicStore::Sideload,
        ] {
            if let Some(record) = self.records(store).get(app_id) {
                return Some(record.url.clone());
            }
        }
        None
    }

    fn load_library(&mut self, path: &Path, store: HeroicStore) {
        for game in json_game_array(path) {
            let Some(app_id) = json_str(&game, &["app_name", "appName"]) else {
                continue;
            };
            let Some(url) = preferred_art_url(store, &game) else {
                continue;
            };
            self.records_mut(store).insert(
                app_id,
                ArtRecord {
                    url,
                    title: json_str(&game, &["title"]),
                },
            );
        }
    }

    fn load_installs(&mut self, path: &Path, store: HeroicStore) {
        let Ok(text) = fs::read_to_string(path) else {
            return;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            return;
        };
        if let Some(map) = value.as_object() {
            if map.get("installed").is_none()
                && map.get("library").is_none()
                && map.get("games").is_none()
            {
                for (key, val) in map {
                    self.note_install(store, key, val);
                }
                return;
            }
        }
        for game in json_array_value(&value) {
            let Some(app_id) = json_str(&game, &["app_name", "appName"]) else {
                continue;
            };
            self.note_install(store, &app_id, &game);
        }
    }

    fn note_install(&mut self, store: HeroicStore, key: &str, value: &Value) {
        let app_id = json_str(value, &["app_name", "appName"]).unwrap_or_else(|| key.to_string());
        let Some(install) = json_str(value, &["install_path", "installPath", "folder_name"]) else {
            return;
        };
        if install.trim().is_empty() {
            return;
        }
        self.installs.insert(install_key(&install), (store, app_id));
    }
}

pub fn is_heroic_launcher(launcher: &str) -> bool {
    launcher.to_ascii_lowercase().contains("heroic")
}

pub fn store_from_launcher(launcher: &str) -> Option<HeroicStore> {
    if !is_heroic_launcher(launcher) {
        return None;
    }
    let lower = launcher.to_ascii_lowercase();
    if lower.contains("epic") {
        Some(HeroicStore::Epic)
    } else if lower.contains("gog") {
        Some(HeroicStore::Gog)
    } else if lower.contains("amazon") {
        Some(HeroicStore::Amazon)
    } else if lower.contains("sideload") {
        Some(HeroicStore::Sideload)
    } else {
        None
    }
}

fn heroic_config_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        let appdata = std::env::var_os("APPDATA")?;
        let path = PathBuf::from(appdata).join("heroic");
        return path.is_dir().then_some(path);
    }
    #[cfg(not(target_os = "windows"))]
    {
        let home = std::env::var_os("HOME")?;
        let home = PathBuf::from(home);
        let config_home = std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| home.join(".config"));
        let native = config_home.join("heroic");
        if native.is_dir() {
            return Some(native);
        }
        let flatpak = home.join(".var/app/com.heroicgameslauncher.hgl/config/heroic");
        flatpak.is_dir().then_some(flatpak)
    }
}

fn app_covers_dir() -> Option<PathBuf> {
    let paths = crate::config::Paths::resolve().ok()?;
    let dir = paths.cache_dir.join("covers");
    fs::create_dir_all(&dir).ok()?;
    Some(dir)
}

fn preferred_art_url(store: HeroicStore, game: &Value) -> Option<String> {
    let cover = http_field(game, "art_cover");
    let square = http_field(game, "art_square");
    // Epic names the tall DieselGameBoxTall image `art_square` and the wide
    // landscape image `art_cover`. Portrait library cards want the tall one.
    match store {
        HeroicStore::Epic => square.or(cover),
        HeroicStore::Gog | HeroicStore::Amazon | HeroicStore::Sideload => cover.or(square),
    }
}

fn http_field(value: &Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    if text.starts_with("https://") || text.starts_with("http://") {
        Some(text.to_string())
    } else {
        None
    }
}

fn json_str(value: &Value, keys: &[&str]) -> Option<String> {
    keys.iter().find_map(|key| {
        value
            .get(*key)
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|text| !text.is_empty())
            .map(str::to_string)
    })
}

fn json_game_array(path: &Path) -> Vec<Value> {
    let Ok(text) = fs::read_to_string(path) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    json_array_value(&value)
}

fn json_array_value(value: &Value) -> Vec<Value> {
    if let Some(items) = value.as_array() {
        return items.clone();
    }
    for key in ["library", "games", "installed"] {
        if let Some(items) = value.get(key).and_then(|v| v.as_array()) {
            return items.clone();
        }
    }
    Vec::new()
}

fn cache_path(config: &Path, url: &str) -> PathBuf {
    let digest = Sha256::digest(url.as_bytes());
    config.join("images-cache").join(hex::encode(digest))
}

fn app_id_from_icon(icon: Option<&Path>) -> Option<String> {
    let icon = icon?;
    if !is_deferred_icon_path(&icon.to_string_lossy()) {
        return None;
    }
    icon.file_stem()
        .and_then(|stem| stem.to_str())
        .map(str::trim)
        .filter(|stem| !stem.is_empty())
        .map(str::to_string)
}

fn safe_app_id(app_id: &str) -> String {
    let mut out = String::new();
    for c in app_id.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        out.push_str("game");
    }
    out
}

fn install_key(path: &str) -> String {
    let mut out = String::new();
    for c in path.trim().chars() {
        match c {
            '/' | '\\' => {
                if !out.ends_with('/') {
                    out.push('/');
                }
            }
            other => out.extend(other.to_lowercase()),
        }
    }
    while out.len() > 1 && out.ends_with('/') {
        out.pop();
    }
    out
}

fn image_extension(path: &Path, url: &str) -> &'static str {
    let mut header = [0u8; 16];
    let n = fs::File::open(path)
        .and_then(|mut file| {
            use std::io::Read;
            file.read(&mut header)
        })
        .unwrap_or(0);
    let bytes = &header[..n];
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

fn source_is_newer(source: &Path, dest: &Path) -> bool {
    let Ok(source) = source.metadata() else {
        return false;
    };
    let Ok(dest) = dest.metadata() else {
        return true;
    };
    if source.len() != dest.len() {
        return true;
    }
    match (source.modified(), dest.modified()) {
        (Ok(source), Ok(dest)) => source > dest,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_library(dir: &Path, relative: &str, body: &str) {
        let path = dir.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, body).unwrap();
    }

    #[test]
    fn gog_art_cover_beats_the_icon_file() {
        let root = tempfile::tempdir().unwrap();
        let covers = tempfile::tempdir().unwrap();
        let url = "https://images.gog.com/cyberpunk-cover.jpg";
        write_library(
            root.path(),
            "store_cache/gog_library.json",
            &format!(
                r#"{{"games":[{{"app_name":"1423049311","title":"Cyberpunk 2077","art_cover":"{url}","art_square":"https://images.gog.com/square.jpg"}}]}}"#
            ),
        );
        let cache = cache_path(root.path(), url);
        fs::create_dir_all(cache.parent().unwrap()).unwrap();
        fs::write(&cache, [0xFF, 0xD8, 0xFF, 0xD9]).unwrap();
        let icon = root.path().join("icons").join("1423049311.png");
        fs::create_dir_all(icon.parent().unwrap()).unwrap();
        fs::write(&icon, b"png-icon").unwrap();

        let mut resolver =
            HeroicResolver::from_dir(root.path().to_path_buf(), Some(covers.path().to_path_buf()));
        let art = resolver.resolve(
            "Heroic Games Launcher (GOG)",
            Some("/home/luu/Games/Heroic/Cyberpunk 2077"),
            Some(&icon),
        );

        let cover = art.cover_path.expect("copied poster");
        assert!(cover.ends_with("heroic-1423049311.jpg"), "{cover}");
        assert!(!is_deferred_icon_path(&cover));
        assert_ne!(cover, icon.to_string_lossy());
        assert!(art.cover_url.is_none());
        assert_eq!(art.icon_path.as_deref(), Some(icon.to_str().unwrap()));
        assert_eq!(fs::read(&cover).unwrap()[..3], [0xFF, 0xD8, 0xFF]);
    }

    #[test]
    fn epic_prefers_the_tall_art_url() {
        let root = tempfile::tempdir().unwrap();
        let tall = "https://cdn1.epicgames.com/tall-1200x1600";
        let wide = "https://cdn1.epicgames.com/wide-2560x1440";
        write_library(
            root.path(),
            "store_cache/legendary_library.json",
            &format!(
                r#"{{"library":[{{"app_name":"abc","art_cover":"{wide}","art_square":"{tall}"}}]}}"#
            ),
        );
        let mut resolver = HeroicResolver::from_dir(root.path().to_path_buf(), None);
        let art = resolver.resolve_store(Some(HeroicStore::Epic), Some("abc"), None);
        assert_eq!(art.cover_url.as_deref(), Some(tall));
        assert!(art.cover_path.is_none());
        assert!(art.icon_path.is_none());
    }

    #[test]
    fn install_path_finds_gog_app_when_the_icon_is_missing() {
        let root = tempfile::tempdir().unwrap();
        let url = "https://images.gog.com/cover.jpg";
        write_library(
            root.path(),
            "store_cache/gog_library.json",
            &format!(r#"{{"games":[{{"app_name":"99","title":"Example","art_cover":"{url}"}}]}}"#),
        );
        write_library(
            root.path(),
            "gog_store/installed.json",
            r#"{"installed":[{"appName":"99","install_path":"/games/Example"}]}"#,
        );
        let mut resolver = HeroicResolver::from_dir(root.path().to_path_buf(), None);
        let art = resolver.resolve("Heroic Games Launcher (GOG)", Some("/games/Example"), None);
        assert_eq!(art.cover_url.as_deref(), Some(url));
        assert!(art.cover_path.is_none());
        assert_eq!(
            resolver.library_title(HeroicStore::Gog, "99").as_deref(),
            Some("Example")
        );
    }
}
