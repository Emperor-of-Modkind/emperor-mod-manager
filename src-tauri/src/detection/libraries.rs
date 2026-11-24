//! Uninstalled game library discovery across Steam, Heroic (Epic / GOG / Amazon),
//! the Epic Games Launcher, and Lutris.

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::ManagedGame;
use crate::games::{match_plugin, GamePluginInfo};

use super::reconcile::games_match;
use super::DetectedGame;

pub mod epic_library;
pub mod heroic_library;
#[cfg(any(target_os = "linux", test))]
pub mod lutris_library;
pub mod steam_library;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum LibrarySource {
    Steam,
    HeroicEpic,
    HeroicGog,
    HeroicAmazon,
    EpicLauncher,
    Lutris,
}

impl LibrarySource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::HeroicEpic => "Heroic — Epic",
            Self::HeroicGog => "Heroic — GOG",
            Self::HeroicAmazon => "Heroic — Amazon",
            Self::EpicLauncher => "Epic Games Launcher",
            Self::Lutris => "Lutris",
        }
    }

    pub fn launcher_name(self) -> &'static str {
        match self {
            Self::Steam => "Steam",
            Self::HeroicEpic => "Heroic (Epic)",
            Self::HeroicGog => "Heroic (GOG)",
            Self::HeroicAmazon => "Heroic (Amazon)",
            Self::EpicLauncher => "Epic Games Launcher",
            Self::Lutris => "Lutris",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryDlcOption {
    pub app_name: String,
    pub title: String,
    #[serde(default)]
    pub install_size_bytes: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct LibraryGameMeta {
    #[serde(default)]
    pub heroic_runner: Option<String>,
    #[serde(default)]
    pub epic_namespace: Option<String>,
    #[serde(default)]
    pub epic_catalog_item_id: Option<String>,
    #[serde(default)]
    pub epic_app_name: Option<String>,
    #[serde(default)]
    pub lutris_slug: Option<String>,
    #[serde(default)]
    pub lutris_service: Option<String>,
    #[serde(default)]
    pub dlcs: Vec<LibraryDlcOption>,
    #[serde(default)]
    pub languages: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryGame {
    pub id: String,
    pub title: String,
    pub source: LibrarySource,
    pub launcher: String,
    pub external_id: String,
    pub supported: bool,
    pub plugin_id: Option<String>,
    pub nexus_domain: Option<String>,
    pub cover_path: Option<String>,
    pub cover_url: Option<String>,
    #[serde(default)]
    pub cover_fallbacks: Vec<String>,
    pub install_size_bytes: Option<u64>,
    pub platforms: Vec<String>,
    pub can_cli_install: bool,
    pub metadata: LibraryGameMeta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibrarySection {
    pub source: LibrarySource,
    pub label: String,
    pub total_owned: usize,
    pub games: Vec<LibraryGame>,
    pub warning: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct SteamSyncOptions {
    pub steam_id_override: Option<String>,
    pub api_key: Option<String>,
    pub sync_enabled: bool,
    pub refresh_remote: bool,
}

pub(crate) fn make_library_id(source: LibrarySource, external_id: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(source.label().as_bytes());
    hasher.update(b"|");
    hasher.update(external_id.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

pub(crate) fn finish_library_game(
    source: LibrarySource,
    external_id: String,
    title: String,
    cover_path: Option<String>,
    cover_url: Option<String>,
    install_size_bytes: Option<u64>,
    platforms: Vec<String>,
    can_cli_install: bool,
    metadata: LibraryGameMeta,
) -> LibraryGame {
    let plugin = match_plugin(&title, None);
    let supported = plugin.is_some();
    let (plugin_id, nexus_domain) = match plugin {
        Some(GamePluginInfo {
            id, nexus_domain, ..
        }) => (Some(id.to_string()), Some(nexus_domain.to_string())),
        None => (None, None),
    };
    let id = make_library_id(source, &external_id);
    LibraryGame {
        id,
        title,
        source,
        launcher: source.launcher_name().to_string(),
        external_id,
        supported,
        plugin_id,
        nexus_domain,
        cover_path,
        cover_url,
        cover_fallbacks: Vec::new(),
        install_size_bytes,
        platforms,
        can_cli_install,
        metadata,
    }
}

/// Remove any library games that are already installed (in `detected` or `managed`)
/// and sort remaining uninstalled titles (Emperor-supported first, then alphabetical).
pub fn finalize_section(
    mut section: LibrarySection,
    detected: &[DetectedGame],
    managed: &[ManagedGame],
) -> LibrarySection {
    let mut seen_external = HashSet::new();
    section.games.retain(|game| {
        if !seen_external.insert(game.external_id.to_lowercase()) {
            return false;
        }
        let in_detected = detected.iter().any(|d| {
            games_match(&game.title, &game.launcher, &d.title, &d.launcher)
                || titles_match_loose(&game.title, &d.title, game.source, &d.launcher)
        });
        if in_detected {
            return false;
        }
        let in_managed = managed.iter().any(|m| {
            games_match(&game.title, &game.launcher, &m.title, &m.launcher)
                || titles_match_loose(&game.title, &m.title, game.source, &m.launcher)
        });
        !in_managed
    });
    section.games.sort_by(|a, b| {
        b.supported
            .cmp(&a.supported)
            .then_with(|| a.title.to_lowercase().cmp(&b.title.to_lowercase()))
    });
    section
}

/// Match titles across minor launcher-name variations (e.g. "Heroic (Epic)" vs "Heroic Games Launcher (Epic)").
fn titles_match_loose(
    lib_title: &str,
    other_title: &str,
    source: LibrarySource,
    other_launcher: &str,
) -> bool {
    if normalize_title_key(lib_title) != normalize_title_key(other_title) {
        return false;
    }
    let lower = other_launcher.to_ascii_lowercase();
    match source {
        LibrarySource::Steam => lower.contains("steam") && !lower.contains("shortcut"),
        LibrarySource::HeroicEpic => lower.contains("heroic") && lower.contains("epic"),
        LibrarySource::HeroicGog => lower.contains("heroic") && lower.contains("gog"),
        LibrarySource::HeroicAmazon => lower.contains("heroic") && lower.contains("amazon"),
        LibrarySource::EpicLauncher => lower.contains("epic") && !lower.contains("heroic"),
        LibrarySource::Lutris => lower.contains("lutris"),
    }
}

fn normalize_title_key(raw: &str) -> String {
    raw.chars()
        .filter(|c| c.is_alphanumeric() || c.is_whitespace())
        .flat_map(|c| c.to_lowercase())
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Scan all available launcher libraries, excluding installed/managed games.
pub async fn scan_all_libraries(
    cache_dir: &Path,
    detected: &[DetectedGame],
    managed: &[ManagedGame],
    steam_opts: SteamSyncOptions,
) -> Vec<LibrarySection> {
    let mut sections = Vec::new();

    if let Some(steam_section) = steam_library::scan_steam_library(cache_dir, &steam_opts).await {
        let finalized = finalize_section(steam_section, detected, managed);
        if !finalized.games.is_empty() || finalized.warning.is_some() {
            sections.push(finalized);
        }
    }

    for heroic_section in heroic_library::scan_heroic_libraries() {
        let finalized = finalize_section(heroic_section, detected, managed);
        if !finalized.games.is_empty() {
            sections.push(finalized);
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(epic_section) = epic_library::scan_epic_library() {
            let finalized = finalize_section(epic_section, detected, managed);
            if !finalized.games.is_empty() {
                sections.push(finalized);
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        if let Some(lutris_section) = lutris_library::scan_lutris_library() {
            let finalized = finalize_section(lutris_section, detected, managed);
            if !finalized.games.is_empty() {
                sections.push(finalized);
            }
        }
    }

    sections
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::detection::finish_detected;

    #[test]
    fn finalize_section_excludes_detected_and_sorts_supported_first() {
        let skyrim = finish_library_game(
            LibrarySource::Steam,
            "489830".into(),
            "The Elder Scrolls V: Skyrim Special Edition".into(),
            None,
            None,
            None,
            vec!["windows".into()],
            false,
            LibraryGameMeta::default(),
        );
        let indie = finish_library_game(
            LibrarySource::Steam,
            "999999".into(),
            "AAA Unrelated Indie".into(),
            None,
            None,
            None,
            vec!["windows".into()],
            false,
            LibraryGameMeta::default(),
        );
        let installed = finish_library_game(
            LibrarySource::Steam,
            "1091500".into(),
            "Cyberpunk 2077".into(),
            None,
            None,
            None,
            vec!["windows".into()],
            false,
            LibraryGameMeta::default(),
        );
        let section = LibrarySection {
            source: LibrarySource::Steam,
            label: "Steam".into(),
            total_owned: 3,
            games: vec![indie, installed, skyrim],
            warning: None,
        };
        let detected = vec![finish_detected(
            "Cyberpunk 2077".into(),
            "Steam".into(),
            Some("/games/cp2077".into()),
            None,
        )];
        let result = finalize_section(section, &detected, &[]);
        assert_eq!(result.games.len(), 2);
        assert!(result.games[0].supported);
        assert_eq!(
            result.games[0].title,
            "The Elder Scrolls V: Skyrim Special Edition"
        );
        assert_eq!(result.games[1].title, "AAA Unrelated Indie");
    }
}
