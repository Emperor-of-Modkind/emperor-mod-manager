//! LOOT integration for Bethesda / Creation Engine titles via `libloot`.
//!
//! Handles:
//! - Discovering `.esm` / `.esp` / `.esl` plugins from the game's `Data/` folder
//!   and enabled staged mods.
//! - Downloading & caching official LOOT masterlists + prelude over HTTP from GitHub.
//! - Sorting plugins with `libloot::Game::sort_plugins` (with offline/MAST fallback).
//! - Extracting plugin flags (ESM/ESL/Medium), masters, missing masters, LOOT
//!   messages (info/warn/error), and dirty-plugin notices.
//! - Persisting per-game plugin load order & enabled states in `plugins_order.json`
//!   and syncing `Plugins.txt`.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use libloot::{
    metadata::{select_message_content, MessageType},
    EvalMode, Game as LootGame, GameType, MergeMode,
};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::creation_engine::{
    creation_engine_paths, is_plugin_file, list_plugin_files, plugin_masters,
    write_plugins_file_with_order,
};
use crate::config::Paths;
use crate::mods::StagedMod;

const LOOT_BRANCHES: &[&str] = &["v0.29", "v0.26", "v0.21"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedPluginEntry {
    pub name: String,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PluginOrderState {
    pub plugins: Vec<SavedPluginEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LootPluginMessage {
    /// `"info"`, `"warn"`, or `"error"`.
    pub kind: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginEntry {
    pub name: String,
    pub enabled: bool,
    pub order: usize,
    pub is_master: bool,
    pub is_light: bool,
    pub is_medium: bool,
    pub is_vanilla: bool,
    pub owner_mod_id: Option<String>,
    pub owner_mod_name: Option<String>,
    pub masters: Vec<String>,
    pub missing_masters: Vec<String>,
    pub messages: Vec<LootPluginMessage>,
    pub dirty_info: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasterlistStatus {
    pub cached: bool,
    pub updated_at: Option<String>,
    pub repo: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BethesdaPluginsState {
    pub plugins: Vec<PluginEntry>,
    pub masterlist: MasterlistStatus,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LootSortResult {
    pub state: BethesdaPluginsState,
    pub previous_order: Vec<SavedPluginEntry>,
    pub moved_count: usize,
    pub used_masterlist: bool,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MasterlistMeta {
    updated_at: String,
    branch: String,
}

pub fn supports_loot(plugin_id: &str) -> bool {
    loot_game_spec(plugin_id).is_some()
}

struct LootGameSpec {
    game_type: GameType,
    repo: &'static str,
}

fn loot_game_spec(plugin_id: &str) -> Option<LootGameSpec> {
    Some(match plugin_id {
        "skyrimspecialedition" => LootGameSpec {
            game_type: GameType::SkyrimSE,
            repo: "skyrimse",
        },
        "skyrim" => LootGameSpec {
            game_type: GameType::Skyrim,
            repo: "skyrim",
        },
        "fallout4" => LootGameSpec {
            game_type: GameType::Fallout4,
            repo: "fallout4",
        },
        "starfield" => LootGameSpec {
            game_type: GameType::Starfield,
            repo: "starfield",
        },
        "newvegas" => LootGameSpec {
            game_type: GameType::FalloutNV,
            repo: "falloutnv",
        },
        "fallout3" => LootGameSpec {
            game_type: GameType::Fallout3,
            repo: "fallout3",
        },
        "oblivion" => LootGameSpec {
            game_type: GameType::Oblivion,
            repo: "oblivion",
        },
        "oblivionremastered" => LootGameSpec {
            game_type: GameType::OblivionRemastered,
            repo: "oblivion-remastered",
        },
        "morrowind" => LootGameSpec {
            game_type: GameType::Morrowind,
            repo: "morrowind",
        },
        _ => return None,
    })
}

pub fn load_saved_plugin_order(paths: &Paths, game_id: &str) -> PluginOrderState {
    let file = paths.plugins_order_file(game_id);
    std::fs::read_to_string(&file)
        .ok()
        .and_then(|s| serde_json::from_str::<PluginOrderState>(&s).ok())
        .unwrap_or_default()
}

pub fn save_plugin_order_file(
    paths: &Paths,
    game_id: &str,
    plugins: &[SavedPluginEntry],
) -> Result<()> {
    let file = paths.plugins_order_file(game_id);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let state = PluginOrderState {
        plugins: plugins.to_vec(),
    };
    let json = serde_json::to_string_pretty(&state)?;
    std::fs::write(&file, json).with_context(|| format!("write {}", file.display()))?;
    Ok(())
}

/// Sync `Plugins.txt` for a Bethesda game using the saved `plugins_order.json`
/// if one exists. Safe to call after deploy or after editing plugin order.
pub fn sync_plugins_txt_from_saved_order(
    paths: &Paths,
    game_id: &str,
    plugin_id: &str,
    install_path: &Path,
) -> Result<Vec<String>> {
    let saved = load_saved_plugin_order(paths, game_id);
    if saved.plugins.is_empty() {
        return Ok(Vec::new());
    }
    let Some(ce) = creation_engine_paths(plugin_id, install_path) else {
        return Ok(Vec::new());
    };
    let Some(plugins_txt) = ce.plugins_txt else {
        return Ok(Vec::new());
    };
    if !ce.data_dir.is_dir() {
        return Ok(Vec::new());
    }
    let pairs: Vec<(String, bool)> = saved
        .plugins
        .into_iter()
        .map(|p| (p.name, p.enabled))
        .collect();
    write_plugins_file_with_order(
        &plugins_txt,
        &ce.data_dir,
        ce.required_masters,
        ce.asterisk_enabled,
        &pairs,
    )
}

fn masterlist_path(paths: &Paths, repo: &str) -> PathBuf {
    paths.loot_dir().join(repo).join("masterlist.yaml")
}

fn masterlist_meta_path(paths: &Paths, repo: &str) -> PathBuf {
    paths.loot_dir().join(repo).join("meta.json")
}

fn prelude_path(paths: &Paths) -> PathBuf {
    paths.loot_dir().join("prelude.yaml")
}

pub fn masterlist_status(paths: &Paths, plugin_id: &str) -> MasterlistStatus {
    let Some(spec) = loot_game_spec(plugin_id) else {
        return MasterlistStatus {
            cached: false,
            updated_at: None,
            repo: String::new(),
        };
    };
    let ml = masterlist_path(paths, spec.repo);
    let cached = ml.is_file();
    let updated_at = std::fs::read_to_string(masterlist_meta_path(paths, spec.repo))
        .ok()
        .and_then(|s| serde_json::from_str::<MasterlistMeta>(&s).ok())
        .map(|m| m.updated_at);
    MasterlistStatus {
        cached,
        updated_at,
        repo: spec.repo.to_string(),
    }
}

async fn download_loot_yaml(client: &reqwest::Client, repo: &str, filename: &str) -> Result<(String, String)> {
    let mut last_err = None;
    for branch in LOOT_BRANCHES {
        let url = format!("https://raw.githubusercontent.com/loot/{repo}/{branch}/{filename}");
        match client.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                let text = resp.text().await.context("read LOOT YAML response")?;
                if !text.trim().is_empty() {
                    return Ok((text, (*branch).to_string()));
                }
            }
            Ok(resp) => {
                last_err = Some(anyhow!("HTTP {} for {}", resp.status(), url));
            }
            Err(e) => {
                last_err = Some(anyhow!("request failed for {}: {}", url, e));
            }
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow!("failed to fetch {repo}/{filename}")))
}

/// Download and cache the official LOOT prelude and game masterlist from GitHub.
pub async fn update_masterlist(paths: &Paths, plugin_id: &str) -> Result<MasterlistStatus> {
    let spec = loot_game_spec(plugin_id)
        .ok_or_else(|| anyhow!("LOOT is not supported for plugin '{plugin_id}'"))?;

    let client = reqwest::Client::builder()
        .user_agent("emperor-mod-manager")
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .context("build HTTP client")?;

    // Prelude is shared across all games.
    if let Ok((prelude_text, _)) = download_loot_yaml(&client, "prelude", "prelude.yaml").await {
        let p_path = prelude_path(paths);
        if let Some(parent) = p_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&p_path, prelude_text)?;
    }

    let (ml_text, branch) = download_loot_yaml(&client, spec.repo, "masterlist.yaml").await?;
    let ml_path = masterlist_path(paths, spec.repo);
    if let Some(parent) = ml_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&ml_path, ml_text)?;

    let now = Utc::now().to_rfc3339();
    let meta = MasterlistMeta {
        updated_at: now.clone(),
        branch,
    };
    let _ = std::fs::write(
        masterlist_meta_path(paths, spec.repo),
        serde_json::to_string_pretty(&meta)?,
    );

    Ok(MasterlistStatus {
        cached: true,
        updated_at: Some(now),
        repo: spec.repo.to_string(),
    })
}

struct DiscoveredPlugin {
    name: String,
    path: PathBuf,
    owner_mod_id: Option<String>,
    owner_mod_name: Option<String>,
    is_vanilla: bool,
}

/// Discover all `.esm`/`.esp`/`.esl` plugins from the game's `Data/` directory
/// AND enabled staged mods (so plugins are visible even before Deploy).
fn discover_plugins(
    plugin_id: &str,
    install_path: &Path,
    mods_dir: &Path,
    staged_mods: &[StagedMod],
) -> Vec<DiscoveredPlugin> {
    let Some(ce) = creation_engine_paths(plugin_id, install_path) else {
        return Vec::new();
    };
    let required_lower: HashSet<String> = ce
        .required_masters
        .iter()
        .map(|s| s.to_lowercase())
        .collect();

    // First map enabled staged mods -> their plugin files (in load order so later
    // mods win ownership if two mods ship the same plugin filename).
    let mut staged_sorted: Vec<&StagedMod> = staged_mods.iter().filter(|m| m.enabled).collect();
    staged_sorted.sort_by_key(|m| m.order);

    let mut by_lower: HashMap<String, DiscoveredPlugin> = HashMap::new();

    // Scan game Data/ folder first (vanilla + any already-deployed plugins).
    if ce.data_dir.is_dir() {
        for name in list_plugin_files(&ce.data_dir) {
            let lower = name.to_lowercase();
            let path = ce.data_dir.join(&name);
            let is_vanilla = required_lower.contains(&lower);
            by_lower.insert(
                lower,
                DiscoveredPlugin {
                    name,
                    path,
                    owner_mod_id: None,
                    owner_mod_name: None,
                    is_vanilla,
                },
            );
        }
    }

    // Overlay enabled staged mod plugins so ownership is attributed and
    // undeployed staged plugins are also discovered.
    for m in staged_sorted {
        let dir = mods_dir.join(&m.id);
        if !dir.is_dir() {
            continue;
        }
        for entry in WalkDir::new(&dir).max_depth(4).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().into_owned();
            if !is_plugin_file(&name) {
                continue;
            }
            let lower = name.to_lowercase();
            let is_vanilla = required_lower.contains(&lower);
            by_lower.insert(
                lower,
                DiscoveredPlugin {
                    name,
                    path: entry.into_path(),
                    owner_mod_id: Some(m.id.clone()),
                    owner_mod_name: Some(m.name.clone()),
                    is_vanilla,
                },
            );
        }
    }

    by_lower.into_values().collect()
}

fn open_loot_game(
    spec: &LootGameSpec,
    plugin_id: &str,
    install_path: &Path,
    discovered: &[DiscoveredPlugin],
    paths: &Paths,
) -> Result<(LootGame, bool)> {
    let ce = creation_engine_paths(plugin_id, install_path)
        .ok_or_else(|| anyhow!("Not a Creation Engine title: {plugin_id}"))?;

    let mut game = if let Some(local_dir) = &ce.local_dir {
        let _ = std::fs::create_dir_all(local_dir);
        LootGame::with_local_path(spec.game_type, install_path, local_dir)
            .or_else(|_| LootGame::new(spec.game_type, install_path))
            .map_err(|e| anyhow!("libloot init failed: {e}"))?
    } else {
        LootGame::new(spec.game_type, install_path)
            .map_err(|e| anyhow!("libloot init failed: {e}"))?
    };

    // Add staged plugin parent dirs as additional data paths so libloot can
    // inspect plugins that haven't been deployed to Data/ yet.
    let mut extra_dirs: Vec<PathBuf> = Vec::new();
    let mut seen_dirs: HashSet<PathBuf> = HashSet::new();
    seen_dirs.insert(ce.data_dir.clone());
    for p in discovered {
        if let Some(parent) = p.path.parent() {
            let pb = parent.to_path_buf();
            if seen_dirs.insert(pb.clone()) {
                extra_dirs.push(pb);
            }
        }
    }
    if !extra_dirs.is_empty() {
        let _ = game.set_additional_data_paths(extra_dirs);
    }

    let valid_paths: Vec<&Path> = discovered
        .iter()
        .map(|p| p.path.as_path())
        .filter(|path| game.is_valid_plugin(path))
        .collect();
    if !valid_paths.is_empty() {
        // Prefer full plugin load (gives CRCs for dirty-plugin checks & overlap checks),
        // falling back to headers-only if a plugin fails full parse.
        if game.load_plugins(&valid_paths).is_err() {
            let _ = game.load_plugin_headers(&valid_paths);
        }
    }

    let ml = masterlist_path(paths, spec.repo);
    let pr = prelude_path(paths);
    let mut loaded_masterlist = false;
    if ml.is_file() {
        let db = game.database();
        if let Ok(mut db_guard) = db.write() {
            let res = if pr.is_file() {
                db_guard.load_masterlist_with_prelude(&ml, &pr)
            } else {
                db_guard.load_masterlist(&ml)
            };
            if res.is_ok() {
                loaded_masterlist = true;
            }
        };
    }

    Ok((game, loaded_masterlist))
}

fn merge_ordered_names(
    discovered: &[DiscoveredPlugin],
    saved: &[SavedPluginEntry],
    required_masters: &[&str],
) -> Vec<(String, bool)> {
    let by_lower: HashMap<String, &DiscoveredPlugin> = discovered
        .iter()
        .map(|d| (d.name.to_lowercase(), d))
        .collect();

    let mut out: Vec<(String, bool)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();

    // 1. Saved order first (preserving user/LOOT order and enabled state).
    for entry in saved {
        let lower = entry.name.to_lowercase();
        if let Some(disc) = by_lower.get(&lower) {
            if seen.insert(lower) {
                let enabled = if disc.is_vanilla { true } else { entry.enabled };
                out.push((disc.name.clone(), enabled));
            }
        }
    }

    // 2. If there was no saved order, put required vanilla masters at the very top.
    if saved.is_empty() {
        for master in required_masters {
            let lower = master.to_lowercase();
            if let Some(disc) = by_lower.get(&lower) {
                if seen.insert(lower) {
                    out.push((disc.name.clone(), true));
                }
            }
        }
    }

    // 3. Any remaining newly discovered plugins: masters first, then alphabetically.
    let mut remaining: Vec<&DiscoveredPlugin> = discovered
        .iter()
        .filter(|d| !seen.contains(&d.name.to_lowercase()))
        .collect();
    remaining.sort_by(|a, b| {
        let a_esm = a.name.to_lowercase().ends_with(".esm");
        let b_esm = b.name.to_lowercase().ends_with(".esm");
        b_esm
            .cmp(&a_esm)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    for disc in remaining {
        let lower = disc.name.to_lowercase();
        if seen.insert(lower) {
            out.push((disc.name.clone(), true));
        }
    }

    out
}

fn build_plugin_entries(
    ordered: &[(String, bool)],
    discovered: &[DiscoveredPlugin],
    loot_game: Option<&LootGame>,
    loaded_masterlist: bool,
) -> Vec<PluginEntry> {
    let disc_by_lower: HashMap<String, &DiscoveredPlugin> = discovered
        .iter()
        .map(|d| (d.name.to_lowercase(), d))
        .collect();
    let present_lower: HashSet<String> = discovered
        .iter()
        .map(|d| d.name.to_lowercase())
        .collect();

    let db_arc = loot_game.map(|g| g.database());
    let db_guard = db_arc.as_ref().and_then(|arc| arc.read().ok());

    ordered
        .iter()
        .enumerate()
        .map(|(idx, (name, enabled))| {
            let lower = name.to_lowercase();
            let disc = disc_by_lower.get(&lower);
            let loot_plugin = loot_game.and_then(|g| g.plugin(name));

            let is_master = loot_plugin
                .as_ref()
                .map(|p| p.is_master())
                .unwrap_or_else(|| lower.ends_with(".esm"));
            let is_light = loot_plugin
                .as_ref()
                .map(|p| p.is_light_plugin())
                .unwrap_or_else(|| lower.ends_with(".esl"));
            let is_medium = loot_plugin
                .as_ref()
                .map(|p| p.is_medium_plugin())
                .unwrap_or(false);

            let masters = loot_plugin
                .as_ref()
                .and_then(|p| p.masters().ok())
                .or_else(|| disc.and_then(|d| plugin_masters(&d.path)))
                .unwrap_or_default();

            let missing_masters: Vec<String> = masters
                .iter()
                .filter(|m| !present_lower.contains(&m.to_lowercase()))
                .cloned()
                .collect();

            let mut messages = Vec::new();
            let mut dirty_info = Vec::new();

            if loaded_masterlist {
                if let Some(db) = db_guard.as_ref() {
                    if let Ok(Some(meta)) =
                        db.plugin_metadata(name, MergeMode::WithUserMetadata, EvalMode::Evaluate)
                    {
                        for msg in meta.messages() {
                            let kind = match msg.message_type() {
                                MessageType::Say => "info",
                                MessageType::Warn => "warn",
                                MessageType::Error => "error",
                            };
                            if let Some(content) = select_message_content(msg.content(), "en") {
                                let text = content.text().trim().to_string();
                                if !text.is_empty() {
                                    messages.push(LootPluginMessage {
                                        kind: kind.to_string(),
                                        text,
                                    });
                                }
                            }
                        }

                        let plugin_crc = loot_plugin.as_ref().and_then(|p| p.crc());
                        for dirty in meta.dirty_info() {
                            let crc_matches =
                                plugin_crc.is_some_and(|crc| crc == dirty.crc());
                            if crc_matches {
                                let mut parts = Vec::new();
                                if dirty.itm_count() > 0 {
                                    parts.push(format!("{} ITM", dirty.itm_count()));
                                }
                                if dirty.deleted_reference_count() > 0 {
                                    parts.push(format!(
                                        "{} UDR",
                                        dirty.deleted_reference_count()
                                    ));
                                }
                                if dirty.deleted_navmesh_count() > 0 {
                                    parts.push(format!(
                                        "{} deleted navmesh",
                                        dirty.deleted_navmesh_count()
                                    ));
                                }
                                let detail = if parts.is_empty() {
                                    "Dirty plugin records detected".to_string()
                                } else {
                                    parts.join(", ")
                                };
                                dirty_info.push(format!(
                                    "{detail} — clean with {}",
                                    dirty.cleaning_utility()
                                ));
                            }
                        }
                    }
                }
            }

            PluginEntry {
                name: name.clone(),
                enabled: *enabled,
                order: idx + 1,
                is_master,
                is_light,
                is_medium,
                is_vanilla: disc.map(|d| d.is_vanilla).unwrap_or(false),
                owner_mod_id: disc.and_then(|d| d.owner_mod_id.clone()),
                owner_mod_name: disc.and_then(|d| d.owner_mod_name.clone()),
                masters,
                missing_masters,
                messages,
                dirty_info,
            }
        })
        .collect()
}

/// List all Bethesda plugins for a managed game with LOOT metadata (from cached
/// masterlist if available).
pub fn list_bethesda_plugins(
    paths: &Paths,
    game_id: &str,
    plugin_id: &str,
    install_path: &Path,
    staged_mods: &[StagedMod],
) -> Result<BethesdaPluginsState> {
    let spec = loot_game_spec(plugin_id)
        .ok_or_else(|| anyhow!("LOOT is not supported for plugin '{plugin_id}'"))?;
    let ce = creation_engine_paths(plugin_id, install_path)
        .ok_or_else(|| anyhow!("Not a Creation Engine title: {plugin_id}"))?;

    let discovered = discover_plugins(
        plugin_id,
        install_path,
        &paths.mods_dir(game_id),
        staged_mods,
    );
    let saved = load_saved_plugin_order(paths, game_id);
    let ordered = merge_ordered_names(&discovered, &saved.plugins, ce.required_masters);

    let mut warnings = Vec::new();
    let (loot_game, loaded_masterlist) =
        match open_loot_game(&spec, plugin_id, install_path, &discovered, paths) {
            Ok((g, loaded)) => (Some(g), loaded),
            Err(e) => {
                warnings.push(format!("Could not initialize libloot: {e}"));
                (None, false)
            }
        };

    let entries = build_plugin_entries(&ordered, &discovered, loot_game.as_ref(), loaded_masterlist);
    Ok(BethesdaPluginsState {
        plugins: entries,
        masterlist: masterlist_status(paths, plugin_id),
        warnings,
    })
}

/// Persist manual plugin order / enabled states and immediately sync `Plugins.txt`.
pub fn set_bethesda_plugin_order(
    paths: &Paths,
    game_id: &str,
    plugin_id: &str,
    install_path: &Path,
    staged_mods: &[StagedMod],
    plugins: Vec<SavedPluginEntry>,
) -> Result<BethesdaPluginsState> {
    save_plugin_order_file(paths, game_id, &plugins)?;
    let _ = sync_plugins_txt_from_saved_order(paths, game_id, plugin_id, install_path);
    list_bethesda_plugins(paths, game_id, plugin_id, install_path, staged_mods)
}

/// Sort Bethesda plugins using `libloot` (auto-fetching the masterlist on first
/// sort if not cached, falling back cleanly if offline).
pub async fn sort_bethesda_plugins(
    paths: &Paths,
    game_id: &str,
    plugin_id: &str,
    install_path: &Path,
    staged_mods: &[StagedMod],
) -> Result<LootSortResult> {
    let spec = loot_game_spec(plugin_id)
        .ok_or_else(|| anyhow!("LOOT is not supported for plugin '{plugin_id}'"))?;
    let ce = creation_engine_paths(plugin_id, install_path)
        .ok_or_else(|| anyhow!("Not a Creation Engine title: {plugin_id}"))?;

    let mut warnings = Vec::new();

    // Auto-fetch masterlist if not cached yet.
    if !masterlist_path(paths, spec.repo).is_file() {
        if let Err(e) = update_masterlist(paths, plugin_id).await {
            warnings.push(format!(
                "Could not download LOOT masterlist ({e}). Sorting using plugin headers only."
            ));
        }
    }

    let discovered = discover_plugins(
        plugin_id,
        install_path,
        &paths.mods_dir(game_id),
        staged_mods,
    );
    let saved = load_saved_plugin_order(paths, game_id);
    let current = merge_ordered_names(&discovered, &saved.plugins, ce.required_masters);
    let previous_order: Vec<SavedPluginEntry> = current
        .iter()
        .map(|(name, enabled)| SavedPluginEntry {
            name: name.clone(),
            enabled: *enabled,
        })
        .collect();
    let enabled_map: HashMap<String, bool> = current
        .iter()
        .map(|(n, e)| (n.to_lowercase(), *e))
        .collect();

    let (loot_game, loaded_masterlist) =
        open_loot_game(&spec, plugin_id, install_path, &discovered, paths)?;

    // Only pass plugins that libloot actually loaded.
    let loaded_names: HashSet<String> = loot_game
        .loaded_plugins()
        .into_iter()
        .map(|p| p.name().to_lowercase())
        .collect();
    let to_sort: Vec<&str> = current
        .iter()
        .map(|(n, _)| n.as_str())
        .filter(|n| loaded_names.contains(&n.to_lowercase()))
        .collect();

    let sorted_names = if to_sort.is_empty() {
        current.iter().map(|(n, _)| n.clone()).collect()
    } else {
        match loot_game.sort_plugins(&to_sort) {
            Ok(sorted) => sorted,
            Err(e) => {
                warnings.push(format!("LOOT sort failed ({e}); keeping current order."));
                current.iter().map(|(n, _)| n.clone()).collect()
            }
        }
    };

    // Reconstruct full list: sorted plugins + any unloaded plugins preserved at end.
    let mut final_ordered: Vec<(String, bool)> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for name in sorted_names {
        let lower = name.to_lowercase();
        if seen.insert(lower.clone()) {
            let enabled = *enabled_map.get(&lower).unwrap_or(&true);
            final_ordered.push((name, enabled));
        }
    }
    for (name, enabled) in &current {
        let lower = name.to_lowercase();
        if seen.insert(lower) {
            final_ordered.push((name.clone(), *enabled));
        }
    }

    let new_saved: Vec<SavedPluginEntry> = final_ordered
        .iter()
        .map(|(name, enabled)| SavedPluginEntry {
            name: name.clone(),
            enabled: *enabled,
        })
        .collect();

    let moved_count = previous_order
        .iter()
        .zip(new_saved.iter())
        .filter(|(a, b)| !a.name.eq_ignore_ascii_case(&b.name))
        .count();

    save_plugin_order_file(paths, game_id, &new_saved)?;
    if let Err(e) = sync_plugins_txt_from_saved_order(paths, game_id, plugin_id, install_path) {
        warnings.push(format!("Sorted plugins, but could not update Plugins.txt: {e}"));
    }

    let entries = build_plugin_entries(
        &final_ordered,
        &discovered,
        Some(&loot_game),
        loaded_masterlist,
    );
    let state = BethesdaPluginsState {
        plugins: entries,
        masterlist: masterlist_status(paths, plugin_id),
        warnings: warnings.clone(),
    };

    Ok(LootSortResult {
        state,
        previous_order,
        moved_count,
        used_masterlist: loaded_masterlist,
        warnings,
    })
}
