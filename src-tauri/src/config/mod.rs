//! Path helpers and persistent app configuration.

use std::{fs, path::PathBuf};

use anyhow::{Context, Result};
use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

pub const APP_NAME: &str = "emperor-mod-manager";
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const KEYRING_SERVICE: &str = "emperor-mod-manager";
pub const KEYRING_USER: &str = "nexus-api-key";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemePreference {
    Light,
    Dark,
    System,
}

impl Default for ThemePreference {
    fn default() -> Self {
        Self::System
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ThemeStyle {
    Brand,
    App,
    System,
}

impl Default for ThemeStyle {
    fn default() -> Self {
        Self::App
    }
}

/// Which Linux toolkit palette System style should read.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LinuxSystemBackend {
    Gtk,
    Qt,
}

impl Default for LinuxSystemBackend {
    fn default() -> Self {
        default_linux_system_backend()
    }
}

/// True when the session looks like KDE/Plasma.
pub fn is_kde_desktop() -> bool {
    if std::env::var_os("KDE_SESSION_VERSION").is_some() {
        return true;
    }
    let desktop = std::env::var("XDG_CURRENT_DESKTOP").unwrap_or_default();
    desktop
        .to_ascii_lowercase()
        .split(':')
        .any(|part| part == "kde")
}

fn default_linux_system_backend() -> LinuxSystemBackend {
    if is_kde_desktop() {
        LinuxSystemBackend::Qt
    } else {
        LinuxSystemBackend::Gtk
    }
}

pub const DEFAULT_ACCENT_COLOR: &str = "#0078d4";

/// Normalize a `#rrggbb` accent, or return the default when the value is invalid.
pub fn normalize_accent_color(raw: &str) -> String {
    let s = raw.trim();
    let hex = s.strip_prefix('#').unwrap_or(s);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("#{}", hex.to_ascii_lowercase())
    } else {
        DEFAULT_ACCENT_COLOR.to_string()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InstallClickBehavior {
    Stay,
    Downloads,
}

impl Default for InstallClickBehavior {
    fn default() -> Self {
        Self::Downloads
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AppConfig {
    pub managed_games: Vec<ManagedGame>,
    pub adult_content: bool,
    /// When true, staged mods that do not match the game version are called out.
    #[serde(default = "default_true")]
    pub game_version_warnings: bool,
    pub last_active_game_id: Option<String>,
    /// When true, the free-download WebView auto-clicks Mod Manager / Slow Download once ready.
    pub autoclick_free_download: bool,
    pub theme: ThemePreference,
    /// Brand (warm palette), App (monochrome + accent), or System (OS colors).
    #[serde(default)]
    pub theme_style: ThemeStyle,
    /// Accent used when `theme_style` is App (`#rrggbb`).
    #[serde(default = "default_accent_color")]
    pub accent_color: String,
    /// Linux System style toolkit: Gtk or Qt. Ignored on other platforms.
    #[serde(default = "default_linux_system_backend")]
    pub linux_system_backend: LinuxSystemBackend,
    /// Whether Install switches to the Downloads tab or keeps the current tab.
    pub install_click_behavior: InstallClickBehavior,
    /// How many finished downloads to keep in the Recent list.
    #[serde(default = "default_recent_downloads_limit")]
    pub recent_downloads_limit: u32,
    /// Auto-dismiss timeouts (seconds) for status banners; `0` means never.
    #[serde(default)]
    pub banner_timeouts: BannerTimeouts,
    /// Set after the first import of managed games from legacy nexus-manager config.
    #[serde(default)]
    pub legacy_games_merged: bool,
    /// Vulkan tool integration (lsfg-vk, AutoHDR-VK).
    #[serde(default)]
    pub tools: ToolsConfig,
    /// `None` until the first settings read decides it. Existing installs with an
    /// API key become complete; a saved `false` stays incomplete after a key is added.
    #[serde(default)]
    pub initial_setup_complete: Option<bool>,
    /// Nexus website session captured by the sign-in window (Stay signed in).
    #[serde(default)]
    pub nexus_website_signed_in: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolsConfig {
    pub lsfg_vk_repo: String,
    pub autohdr_vk_repo: String,
}

impl Default for ToolsConfig {
    fn default() -> Self {
        Self {
            lsfg_vk_repo: "LewisTansley/lsfg-vk".to_string(),
            autohdr_vk_repo: "LewisTansley/AutoHDR-VK".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GameToolOverrides {
    #[serde(default)]
    pub lsfg_vk: ToolGameState,
    #[serde(default)]
    pub autohdr_vk: ToolGameState,
    /// Windows/Linux executables used for profile matching (e.g. Game.exe).
    #[serde(default)]
    pub executables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolGameState {
    #[serde(default)]
    pub enabled: bool,
    /// When true, inherit global tool settings instead of per-game overrides.
    #[serde(default = "default_use_global")]
    pub use_global_settings: bool,
}

fn default_use_global() -> bool {
    true
}

fn default_true() -> bool {
    true
}

fn default_accent_color() -> String {
    DEFAULT_ACCENT_COLOR.to_string()
}

pub const MIN_RECENT_DOWNLOADS_LIMIT: u32 = 1;
pub const MAX_RECENT_DOWNLOADS_LIMIT: u32 = 500;

fn default_recent_downloads_limit() -> u32 {
    50
}

pub fn clamp_recent_downloads_limit(limit: u32) -> u32 {
    limit.clamp(MIN_RECENT_DOWNLOADS_LIMIT, MAX_RECENT_DOWNLOADS_LIMIT)
}

pub const MAX_BANNER_TIMEOUT_SECONDS: u32 = 86_400;

/// Per-color auto-dismiss timeouts in seconds (`0` means never auto-dismiss).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default)]
pub struct BannerTimeouts {
    pub info: u32,
    pub warn: u32,
    pub error: u32,
}

impl Default for BannerTimeouts {
    fn default() -> Self {
        Self {
            info: 10,
            warn: 300,
            error: 0,
        }
    }
}

pub fn clamp_banner_timeout_seconds(seconds: u32) -> u32 {
    if seconds == 0 {
        0
    } else {
        seconds.min(MAX_BANNER_TIMEOUT_SECONDS)
    }
}

pub fn clamp_banner_timeouts(timeouts: BannerTimeouts) -> BannerTimeouts {
    BannerTimeouts {
        info: clamp_banner_timeout_seconds(timeouts.info),
        warn: clamp_banner_timeout_seconds(timeouts.warn),
        error: clamp_banner_timeout_seconds(timeouts.error),
    }
}

/// Resolves a missing first-run flag. `None` means the field was never saved:
/// existing installs with an API key are complete, and everyone else stays in setup.
/// A saved `false` is left false even when an API key exists later.
pub fn resolve_initial_setup(complete: Option<bool>, has_api_key: bool) -> bool {
    match complete {
        Some(value) => value,
        None => has_api_key,
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            managed_games: Vec::new(),
            adult_content: true,
            game_version_warnings: true,
            last_active_game_id: None,
            autoclick_free_download: true,
            theme: ThemePreference::System,
            theme_style: ThemeStyle::App,
            accent_color: default_accent_color(),
            linux_system_backend: default_linux_system_backend(),
            install_click_behavior: InstallClickBehavior::Downloads,
            recent_downloads_limit: default_recent_downloads_limit(),
            banner_timeouts: BannerTimeouts::default(),
            legacy_games_merged: false,
            tools: ToolsConfig::default(),
            initial_setup_complete: None,
            nexus_website_signed_in: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LaunchEnvVar {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum LaunchCommandSource {
    #[default]
    Emperor,
    Launcher,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManagedGame {
    pub id: String,
    pub title: String,
    /// Nexus Mods domain; empty when Thunderstore-only.
    #[serde(default)]
    pub nexus_domain: String,
    pub install_path: String,
    pub launcher: String,
    pub plugin_id: String,
    #[serde(default)]
    pub cover_path: Option<String>,
    /// Remote poster used when `cover_path` is empty (Heroic or a mod catalog).
    #[serde(default)]
    pub cover_url: Option<String>,
    /// Optional Unreal project folder override (e.g. "Pal", "Phoenix").
    #[serde(default)]
    pub project_name: Option<String>,
    /// Thunderstore community identifier (e.g. "lethal-company").
    #[serde(default)]
    pub thunderstore_community: Option<String>,
    /// mod.io numeric game id when this title is catalogued there.
    #[serde(default)]
    pub modio_game_id: Option<u32>,
    /// Per-game Vulkan tool overrides (lsfg-vk, AutoHDR-VK).
    #[serde(default)]
    pub tool_overrides: Option<GameToolOverrides>,
    /// User-set game version. Empty means use the version read from the executable.
    #[serde(default)]
    pub game_version: Option<String>,
    /// Requirements hidden from the review until the user unignores them.
    #[serde(default)]
    pub ignored_requirements: Vec<IgnoredRequirement>,
    /// Optional relative or absolute executable override for launching non-Steam games.
    #[serde(default)]
    pub launch_executable: Option<String>,
    /// Whether Emperor applies its own launch commands (WINEDLLOVERRIDES, tool env) or defers to the launcher.
    #[serde(default)]
    pub launch_command_source: LaunchCommandSource,
    /// User-defined custom environment variables for launching modded.
    #[serde(default)]
    pub custom_env_vars: Vec<LaunchEnvVar>,
    /// Optional wrapper command (e.g. "gamemoderun").
    #[serde(default)]
    pub custom_wrapper: Option<String>,
    /// Optional extra launch arguments (e.g. "--launcher-skip").
    #[serde(default)]
    pub custom_launch_args: Option<String>,
}

/// A requirement the user chose to hide for one managed game.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IgnoredRequirement {
    pub key: String,
    pub name: String,
}

#[derive(Clone)]
pub struct Paths {
    pub config_dir: PathBuf,
    pub data_dir: PathBuf,
    pub cache_dir: PathBuf,
}

impl Paths {
    pub fn resolve() -> Result<Self> {
        let dirs = ProjectDirs::from("dev", "emperormodmanager", APP_NAME)
            .context("failed to resolve project directories")?;
        let paths = Self {
            config_dir: dirs.config_dir().to_path_buf(),
            data_dir: dirs.data_dir().to_path_buf(),
            cache_dir: dirs.cache_dir().to_path_buf(),
        };
        fs::create_dir_all(&paths.config_dir)?;
        fs::create_dir_all(&paths.data_dir)?;
        fs::create_dir_all(&paths.cache_dir)?;
        fs::create_dir_all(paths.downloads_dir())?;
        fs::create_dir_all(paths.assist_webview_dir())?;
        Ok(paths)
    }

    pub fn config_file(&self) -> PathBuf {
        self.config_dir.join("config.toml")
    }

    pub fn downloads_dir(&self) -> PathBuf {
        self.cache_dir.join("downloads")
    }

    /// Finished downloads shown in the Recent list.
    pub fn recent_downloads_file(&self) -> PathBuf {
        self.data_dir.join("recent_downloads.json")
    }

    /// Persistent WebKit profile for Download Assist (Nexus website cookies).
    pub fn assist_webview_dir(&self) -> PathBuf {
        self.data_dir.join("assist-webview")
    }

    pub fn game_data_dir(&self, game_id: &str) -> PathBuf {
        self.data_dir.join(game_id)
    }

    pub fn mods_dir(&self, game_id: &str) -> PathBuf {
        self.game_data_dir(game_id).join("mods")
    }

    pub fn loadorder_file(&self, game_id: &str) -> PathBuf {
        self.game_data_dir(game_id).join("loadorder.json")
    }

    pub fn collections_file(&self, game_id: &str) -> PathBuf {
        self.game_data_dir(game_id).join("collections.json")
    }

    pub fn deploy_manifest(&self, game_id: &str) -> PathBuf {
        self.game_data_dir(game_id).join("deployed.json")
    }

    pub fn last_launch_manifest(&self, game_id: &str) -> PathBuf {
        self.game_data_dir(game_id).join("last-launch-deployed.json")
    }

    pub fn logs_dir(&self) -> PathBuf {
        self.data_dir.join("logs")
    }

    pub fn log_file(&self) -> PathBuf {
        self.logs_dir().join("emperor.log")
    }

    /// Local library of Emperor share codes (not per-game staging).
    pub fn saved_collections_file(&self) -> PathBuf {
        self.data_dir.join("saved_collections.json")
    }

    pub fn tools_dir(&self) -> PathBuf {
        self.data_dir.join("tools")
    }

    pub fn tools_manifest_file(&self) -> PathBuf {
        self.tools_dir().join("manifest.json")
    }

    pub fn lsfg_vk_install_dir(&self) -> PathBuf {
        self.tools_dir().join("lsfg-vk")
    }

    pub fn autohdr_vk_install_dir(&self) -> PathBuf {
        self.tools_dir().join("autohdr-vk")
    }

    pub fn lsfg_vk_config_file(&self) -> PathBuf {
        self.lsfg_vk_install_dir().join("conf.toml")
    }

    pub fn autohdr_vk_config_file(&self) -> PathBuf {
        self.autohdr_vk_install_dir().join("conf.toml")
    }

    /// Cached legacy Mod Engine (`dinput8.dll`) packs per FromSoft plugin id.
    pub fn fromsoft_legacy_modengine_dir(&self, plugin_id: &str) -> PathBuf {
        self.tools_dir()
            .join("fromsoft-modengine-legacy")
            .join(plugin_id)
    }

    pub fn modengine2_install_dir(&self) -> PathBuf {
        self.tools_dir().join("modengine2")
    }

    pub fn me3_install_dir(&self) -> PathBuf {
        self.tools_dir().join("me3")
    }

    /// Cached BepInEx pack for one loader build (`mono-5.4.23.5`, a Thunderstore id, …).
    pub fn bepinex_cache_dir(&self, cache_id: &str) -> PathBuf {
        self.tools_dir().join("bepinex").join(cache_id)
    }

    /// Cached SMAPI installer for one GitHub release tag.
    pub fn smapi_cache_dir(&self, tag: &str) -> PathBuf {
        let safe: String = tag
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        let safe = if safe.is_empty() {
            "unknown".to_string()
        } else {
            safe
        };
        self.tools_dir().join("smapi").join(safe)
    }
}

pub fn load_config(paths: &Paths) -> Result<AppConfig> {
    let path = paths.config_file();
    if !path.exists() {
        let cfg = AppConfig::default();
        save_config(paths, &cfg)?;
        return Ok(cfg);
    }
    let raw =
        fs::read_to_string(&path).with_context(|| format!("reading config {}", path.display()))?;
    let cfg: AppConfig = toml::from_str(&raw).context("parsing config.toml")?;
    Ok(cfg)
}

pub fn save_config(paths: &Paths, cfg: &AppConfig) -> Result<()> {
    let path = paths.config_file();
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = toml::to_string_pretty(cfg).context("serializing config")?;
    fs::write(&path, raw).with_context(|| format!("writing config {}", path.display()))?;
    Ok(())
}

pub fn ensure_game_dirs(paths: &Paths, game_id: &str) -> Result<()> {
    fs::create_dir_all(paths.mods_dir(game_id))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_setup_flag_follows_api_key() {
        assert!(resolve_initial_setup(None, true));
        assert!(!resolve_initial_setup(None, false));
    }

    #[test]
    fn saved_incomplete_setup_stays_incomplete_when_api_key_exists() {
        assert!(!resolve_initial_setup(Some(false), true));
        assert!(resolve_initial_setup(Some(true), false));
    }

    #[test]
    fn old_config_omits_setup_flag() {
        let cfg: AppConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.initial_setup_complete, None);
        assert!(!cfg.nexus_website_signed_in);
    }

    #[test]
    fn theme_style_defaults_to_app() {
        let cfg: AppConfig = toml::from_str("").unwrap();
        assert_eq!(cfg.theme_style, ThemeStyle::App);
        assert_eq!(
            normalize_accent_color(&cfg.accent_color),
            DEFAULT_ACCENT_COLOR
        );
        assert_eq!(cfg.linux_system_backend, default_linux_system_backend());
    }

    #[test]
    fn normalize_accent_color_accepts_hex() {
        assert_eq!(normalize_accent_color("#AABBCC"), "#aabbcc");
        assert_eq!(normalize_accent_color("112233"), "#112233");
        assert_eq!(normalize_accent_color("nope"), DEFAULT_ACCENT_COLOR);
    }

    #[test]
    fn banner_timeouts_defaults_and_roundtrip_zero_as_never() {
        let cfg: AppConfig = toml::from_str("").unwrap();
        assert_eq!(
            cfg.banner_timeouts,
            BannerTimeouts {
                info: 10,
                warn: 300,
                error: 0,
            }
        );

        let mut custom = cfg.clone();
        custom.banner_timeouts = clamp_banner_timeouts(BannerTimeouts {
            info: 0,
            warn: 100_000,
            error: 15,
        });
        assert_eq!(
            custom.banner_timeouts,
            BannerTimeouts {
                info: 0,
                warn: MAX_BANNER_TIMEOUT_SECONDS,
                error: 15,
            }
        );

        let raw = toml::to_string_pretty(&custom).unwrap();
        let reloaded: AppConfig = toml::from_str(&raw).unwrap();
        assert_eq!(reloaded.banner_timeouts, custom.banner_timeouts);
    }
}
