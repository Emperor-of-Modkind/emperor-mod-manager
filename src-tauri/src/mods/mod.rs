//! Mod staging, extraction, load order, and deploy.

pub mod autosort;
pub mod fomod;
pub mod game_version;
pub mod options;
pub mod resolution;

use std::{
    collections::{HashMap, HashSet, VecDeque},
    fs,
    io::Read,
    path::{Component, Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::{
    config::Paths,
    games::{
        bepinex_pack_deploy_root, normalize_relative, normalize_staging_root, plugin_by_id,
        rewrite_ror2_bepinexpack_dest, DeployContext, GamePlugin,
    },
    mods::{
        fomod::{FomodPendingReview, FomodSelection},
        options::{IncludeFilter, ModOptionSelection},
    },
};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum ModSource {
    #[default]
    Nexus,
    Thunderstore,
    Modio,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedMod {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub source: ModSource,
    #[serde(default)]
    pub nexus_mod_id: u64,
    #[serde(default)]
    pub nexus_file_id: u64,
    pub version: Option<String>,
    /// Nexus domain, or Thunderstore community id.
    pub domain: String,
    pub staging_path: String,
    pub enabled: bool,
    pub order: u32,
    /// Thunderstore package namespace (owner).
    #[serde(default)]
    pub ts_namespace: Option<String>,
    /// Thunderstore package name (without namespace).
    #[serde(default)]
    pub ts_name: Option<String>,
    /// Thunderstore package UUID (optional).
    #[serde(default)]
    pub ts_package_uuid: Option<String>,
    #[serde(default)]
    pub modio_game_id: Option<u32>,
    #[serde(default)]
    pub modio_mod_id: Option<u64>,
    #[serde(default)]
    pub modio_file_id: Option<u64>,
    /// Collections that include this mod as a member or recorded pack dependency.
    #[serde(default)]
    pub collection_ids: Vec<String>,
    /// True when the user installed this outside a collection (browse/import).
    #[serde(default = "default_true")]
    pub independent: bool,
    /// Staged mod ids this mod requires.
    #[serde(default)]
    pub depends_on: Vec<String>,
    /// Files present in staging right after extraction. Staging is immutable, so
    /// a smaller count means something outside Emperor deleted staged files and
    /// the package has to be downloaded again.
    #[serde(default)]
    pub staged_file_count: Option<usize>,
    /// Which of the mod's own options are on. `None` means the mod declares no
    /// options, or the user has not touched the defaults yet.
    #[serde(default)]
    pub option_selection: Option<ModOptionSelection>,
    /// FOMOD installer picks. `None` means no ModuleConfig, or defaults not saved yet.
    #[serde(default)]
    pub fomod_selection: Option<FomodSelection>,
    /// Pending FOMOD settings added or invalidated by a mod update.
    /// Cleared once the user saves choices in the FOMOD configurator.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fomod_pending: Option<FomodPendingReview>,
    /// Name of the file that was staged, when it is distinct from the mod title.
    /// The file chooser's title is preferred; the archive name is the fallback.
    #[serde(default)]
    pub file_name: Option<String>,
    /// Catalog poster. `None` has not been looked up. `Some("")` has no image.
    #[serde(default)]
    pub picture_url: Option<String>,
    /// Version-line Nexus tags, including edition labels such as `AE`.
    /// `None` has not been looked up.
    #[serde(default)]
    pub supported_game_versions: Option<Vec<String>>,
    /// `0` is the old dotted-only tag cache and is fetched again.
    #[serde(default)]
    pub version_support_generation: u32,
    /// Optional plugin tool id (e.g. `"pz3d"`, `"zombiebuddy"`) when this mod
    /// was installed via the game's Tools modal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_tool_id: Option<String>,
}

fn default_true() -> bool {
    true
}

impl Default for StagedMod {
    fn default() -> Self {
        Self {
            id: String::new(),
            name: String::new(),
            source: ModSource::Nexus,
            nexus_mod_id: 0,
            nexus_file_id: 0,
            version: None,
            domain: String::new(),
            staging_path: String::new(),
            enabled: true,
            order: 0,
            ts_namespace: None,
            ts_name: None,
            ts_package_uuid: None,
            modio_game_id: None,
            modio_mod_id: None,
            modio_file_id: None,
            collection_ids: Vec::new(),
            independent: true,
            depends_on: Vec::new(),
            staged_file_count: None,
            option_selection: None,
            fomod_selection: None,
            fomod_pending: None,
            file_name: None,
            picture_url: None,
            supported_game_versions: None,
            version_support_generation: 0,
            plugin_tool_id: None,
        }
    }
}

/// Prefer a freshly known poster. An empty incoming value keeps the replaced row's URL.
fn resolve_picture_url(incoming: Option<String>, removed: &[StagedMod]) -> Option<String> {
    let incoming = incoming.filter(|url| !url.trim().is_empty());
    if incoming.is_some() {
        incoming
    } else {
        removed.iter().find_map(|m| m.picture_url.clone())
    }
}

/// Label that tells two files of one mod apart. The file chooser's title wins.
/// A blank name, or one that only repeats the mod title, is dropped so the UI
/// can show the version and the existing source line instead.
pub fn pick_staged_file_name(
    mod_name: &str,
    chooser_name: Option<&str>,
    archive_name: Option<&str>,
) -> Option<String> {
    fn distinct(mod_name: &str, candidate: Option<&str>) -> Option<String> {
        let name = candidate?.trim();
        if name.is_empty() || name.eq_ignore_ascii_case(mod_name.trim()) {
            None
        } else {
            Some(name.to_string())
        }
    }
    distinct(mod_name, chooser_name).or_else(|| distinct(mod_name, archive_name))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CollectionSource {
    #[default]
    Nexus,
    Thunderstore,
    Emperor,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "lowercase")]
pub enum CollectionKind {
    #[default]
    Collection,
    Modpack,
    Profile,
    Share,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FriendSyncMetadata {
    pub friend_peer_id: String,
    pub friend_collection_id: String,
    pub last_synced_hash: String,
    pub last_synced_at: String,
    #[serde(default = "default_true")]
    pub auto_sync: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledCollection {
    pub id: String,
    pub source: CollectionSource,
    pub kind: CollectionKind,
    pub name: String,
    #[serde(default)]
    pub slug: Option<String>,
    #[serde(default)]
    pub namespace: Option<String>,
    #[serde(default)]
    pub package_name: Option<String>,
    #[serde(default)]
    pub community: Option<String>,
    #[serde(default)]
    pub revision: Option<i64>,
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub profile_code: Option<String>,
    #[serde(default)]
    pub mod_ids: Vec<String>,
    #[serde(default)]
    pub installed_at: String,
    /// Game versions this collection revision was built for. `None` has not been looked up.
    #[serde(default)]
    pub game_versions: Option<Vec<String>>,
    /// Synced friend collection metadata when joined from a P2P friend.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub friend_sync: Option<FriendSyncMetadata>,
    /// Automatically save changes to load order, member mods, and FOMOD options.
    #[serde(default)]
    pub auto_save: bool,
    /// Snapshot hash of the last saved collection state (order, members, FOMOD/options).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_snapshot_hash: Option<String>,
    /// Computed on load: whether the collection has unsaved changes since last save.
    #[serde(default)]
    pub has_unsaved_changes: bool,
    /// Whether the user has disabled this collection's mods.
    #[serde(default)]
    pub disabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstalledCollections {
    pub collections: Vec<InstalledCollection>,
}

#[derive(Debug, Clone, Default)]
struct StagingProvenance {
    collection_ids: Vec<String>,
    independent: bool,
    depends_on: Vec<String>,
    enabled: bool,
    order: Option<u32>,
    option_selection: Option<ModOptionSelection>,
    fomod_selection: Option<FomodSelection>,
    fomod_pending: Option<FomodPendingReview>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LoadOrder {
    pub mods: Vec<StagedMod>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DeployManifest {
    /// Absolute paths we created/linked during last deploy.
    pub paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeployResult {
    pub file_count: usize,
    pub enabled_mods: usize,
    pub warnings: Vec<String>,
    /// Plugins omitted because a master is missing. Empty for games that do not
    /// write a plugin load order.
    #[serde(default)]
    pub plugin_skips: Vec<crate::games::PluginSkip>,
    /// Nexus requirements of the staged mods that contain those plugins.
    #[serde(default)]
    pub missing_requirements: crate::nexus::MissingRequirements,
}

/// Per-game lock around load-order read/modify/write. Installs run side by side
/// (Assist moves on before staging finishes), and the last save used to drop
/// or duplicate rows.
#[derive(Clone)]
pub struct GameOrderLocks {
    inner: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl GameOrderLocks {
    pub fn with_game<T>(&self, game_id: &str, f: impl FnOnce() -> T) -> T {
        let game_lock = {
            let mut map = self.inner.lock().unwrap_or_else(|e| e.into_inner());
            map.entry(game_id.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _guard = game_lock.lock().unwrap_or_else(|e| e.into_inner());
        f()
    }
}

static ORDER_LOCKS: OnceLock<GameOrderLocks> = OnceLock::new();
static COLLECTIONS_LOCKS: OnceLock<GameOrderLocks> = OnceLock::new();

pub fn shared_order_locks() -> GameOrderLocks {
    ORDER_LOCKS
        .get_or_init(|| GameOrderLocks {
            inner: Arc::new(Mutex::new(HashMap::new())),
        })
        .clone()
}

pub fn with_game_order<T>(game_id: &str, f: impl FnOnce() -> T) -> T {
    shared_order_locks().with_game(game_id, f)
}

fn shared_collections_locks() -> GameOrderLocks {
    COLLECTIONS_LOCKS
        .get_or_init(|| GameOrderLocks {
            inner: Arc::new(Mutex::new(HashMap::new())),
        })
        .clone()
}

/// Serialize `collections.json` read-modify-write per game.
/// Always acquire `with_game_order` *before* `with_game_collections` when both are needed.
pub fn with_game_collections<T>(game_id: &str, f: impl FnOnce() -> T) -> T {
    shared_collections_locks().with_game(game_id, f)
}

fn read_loadorder(paths: &Paths, game_id: &str) -> Result<LoadOrder> {
    let file = paths.loadorder_file(game_id);
    if !file.exists() {
        return Ok(LoadOrder::default());
    }
    let raw = fs::read_to_string(&file)?;
    Ok(serde_json::from_str(&raw)?)
}

fn write_loadorder(paths: &Paths, game_id: &str, order: &LoadOrder) -> Result<()> {
    crate::config::ensure_game_dirs(paths, game_id)?;
    let file = paths.loadorder_file(game_id);
    let raw = serde_json::to_string_pretty(order)?;
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, &raw)?;
    fs::rename(&tmp, &file)?;
    Ok(())
}

pub fn load_loadorder(paths: &Paths, game_id: &str) -> Result<LoadOrder> {
    with_game_order(game_id, || {
        let mut order = read_loadorder(paths, game_id)?;
        if collapse_load_order(paths, game_id, &mut order)? {
            write_loadorder(paths, game_id, &order)?;
        }
        Ok(order)
    })
}

pub fn save_loadorder(paths: &Paths, game_id: &str, order: &LoadOrder) -> Result<()> {
    with_game_order(game_id, || write_loadorder(paths, game_id, order))
}

fn mutate_load_order<T>(
    paths: &Paths,
    game_id: &str,
    f: impl FnOnce(&mut LoadOrder) -> Result<T>,
) -> Result<T> {
    with_game_order(game_id, || {
        let mut order = read_loadorder(paths, game_id)?;
        collapse_load_order(paths, game_id, &mut order)?;
        let out = f(&mut order)?;
        write_loadorder(paths, game_id, &order)?;
        Ok(out)
    })
}

fn read_collections(paths: &Paths, game_id: &str) -> Result<InstalledCollections> {
    let file = paths.collections_file(game_id);
    if !file.exists() {
        return Ok(InstalledCollections::default());
    }
    let raw = fs::read_to_string(&file)?;
    Ok(serde_json::from_str(&raw)?)
}

fn write_collections(
    paths: &Paths,
    game_id: &str,
    collections: &InstalledCollections,
) -> Result<()> {
    crate::config::ensure_game_dirs(paths, game_id)?;
    let file = paths.collections_file(game_id);
    let raw = serde_json::to_string_pretty(collections)?;
    let tmp = file.with_extension("json.tmp");
    fs::write(&tmp, &raw)?;
    fs::rename(&tmp, &file)?;
    Ok(())
}

pub fn load_collections(paths: &Paths, game_id: &str) -> Result<InstalledCollections> {
    with_game_collections(game_id, || read_collections(paths, game_id))
}

pub fn save_collections(
    paths: &Paths,
    game_id: &str,
    collections: &InstalledCollections,
) -> Result<()> {
    with_game_collections(game_id, || write_collections(paths, game_id, collections))
}

pub fn thunderstore_mod_id(namespace: &str, name: &str) -> String {
    format!("ts_{namespace}_{name}")
}

pub fn thunderstore_mod_versioned_id(namespace: &str, name: &str, version: &str) -> String {
    format!("ts_{namespace}_{name}@{version}")
}

pub fn modio_identity_id(modio_game_id: u32, modio_mod_id: u64) -> String {
    format!("modio_{modio_game_id}_{modio_mod_id}")
}

pub fn modio_file_identity_id(modio_game_id: u32, modio_mod_id: u64, modio_file_id: u64) -> String {
    format!("modio_{modio_game_id}_{modio_mod_id}_{modio_file_id}")
}

pub fn nexus_collection_id(slug: &str) -> String {
    format!("nexus:{slug}")
}

pub fn thunderstore_modpack_id(community: &str, namespace: &str, name: &str) -> String {
    format!("ts-modpack:{community}:{namespace}-{name}")
}

pub fn thunderstore_profile_id(code: &str) -> String {
    format!("ts-profile:{code}")
}

fn parse_version_parts(raw: &str) -> Option<Vec<u64>> {
    let s = raw.trim();
    if s.is_empty() {
        return None;
    }
    let mut parts = Vec::new();
    let mut saw_digit = false;
    for token in s.split(|c: char| !c.is_ascii_digit()) {
        if token.is_empty() {
            continue;
        }
        saw_digit = true;
        parts.push(token.parse().ok()?);
    }
    if saw_digit {
        Some(parts)
    } else {
        None
    }
}

/// True when `incoming` should replace `existing` (newest-wins).
pub fn incoming_is_newer(existing: Option<&str>, incoming: &str) -> bool {
    let incoming = incoming.trim();
    if incoming.is_empty() {
        return false;
    }
    let Some(existing) = existing.map(str::trim).filter(|s| !s.is_empty()) else {
        return true;
    };
    if existing == incoming {
        return false;
    }
    match (parse_version_parts(existing), parse_version_parts(incoming)) {
        (Some(a), Some(b)) => {
            let n = a.len().max(b.len());
            for i in 0..n {
                let x = a.get(i).copied().unwrap_or(0);
                let y = b.get(i).copied().unwrap_or(0);
                if y != x {
                    return y > x;
                }
            }
            false
        }
        (None, None) => true,
        (Some(_), None) => false,
        (None, Some(_)) => true,
    }
}

/// Minimal Nexus file metadata for update candidate selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NexusFileMeta {
    pub file_id: u64,
    pub version: Option<String>,
    pub category_name: Option<String>,
    pub uploaded_timestamp: Option<u64>,
    pub is_primary: bool,
    /// Chosen file label (`pick_staged_file_name`), when one exists.
    pub file_name: Option<String>,
    /// Nexus display title. Empty when the caller has no title to compare.
    pub name: String,
}

/// Available remote update for a staged mod.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedModUpdate {
    pub staged_id: String,
    pub available_version: Option<String>,
    pub source: ModSource,
    #[serde(default)]
    pub nexus_file_id: Option<u64>,
    #[serde(default)]
    pub ts_version: Option<String>,
    #[serde(default)]
    pub modio_file_id: Option<u64>,
    /// File label to store on the replacement row.
    #[serde(default)]
    pub file_name: Option<String>,
}

fn category_key(name: Option<&str>) -> String {
    name.unwrap_or("").trim().to_lowercase()
}

fn is_main_category(name: Option<&str>) -> bool {
    category_key(name).contains("main")
}

fn is_historical_category(name: Option<&str>) -> bool {
    let key = category_key(name);
    key.is_empty()
        || matches!(
            key.as_str(),
            "old_version" | "old version" | "archived" | "removed" | "deleted"
        )
}

fn nexus_file_is_newer(candidate: &NexusFileMeta, baseline: &NexusFileMeta) -> bool {
    match (candidate.uploaded_timestamp, baseline.uploaded_timestamp) {
        (Some(ta), Some(tb)) if ta != tb => ta > tb,
        _ => candidate.file_id > baseline.file_id,
    }
}

/// Lowercased title with version tokens removed, so "BepInEx 1.2" and "BepInEx" match.
fn file_variant_key(name: &str) -> String {
    let chars: Vec<char> = name.trim().to_lowercase().chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        let boundary = i == 0 || !chars[i - 1].is_ascii_alphanumeric();
        if boundary {
            if let Some(next) = skip_version_token(&chars, i) {
                if !out.is_empty() && !out.ends_with(' ') {
                    out.push(' ');
                }
                i = next;
                continue;
            }
        }
        let c = chars[i];
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.is_empty() && !out.ends_with(' ') {
            out.push(' ');
        }
        i += 1;
    }
    out.trim().to_string()
}

/// `v1.2`, `1.2.3`, `1-0-7`, `1.10.6a`, and `1.10.6-rc1`. A bare number stays, so loader majors are kept.
fn skip_version_token(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    let mut saw_v = false;
    if chars.get(i) == Some(&'v') && chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
        saw_v = true;
        i += 1;
    }
    if !chars.get(i).is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    while chars.get(i).is_some_and(|c| c.is_ascii_digit()) {
        i += 1;
    }
    let mut groups = 1;
    while matches!(chars.get(i), Some('.' | '-')) {
        let mut j = i + 1;
        if !chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
            break;
        }
        while chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
            j += 1;
        }
        groups += 1;
        i = j;
    }
    if !(saw_v || groups >= 2) {
        return None;
    }
    // Consume a single trailing letter hotfix suffix (e.g. `1.10.6a`).
    if chars.get(i).is_some_and(|c| c.is_ascii_alphabetic())
        && !chars.get(i + 1).is_some_and(|c| c.is_ascii_alphanumeric())
    {
        i += 1;
    } else if matches!(chars.get(i), Some('-' | '.')) {
        // Consume common prerelease / hotfix tags (e.g. `1.10.6-rc1`, `1.10.6-beta2`).
        let mut j = i + 1;
        let tag_start = j;
        while chars.get(j).is_some_and(|c| c.is_ascii_alphabetic()) {
            j += 1;
        }
        let tag: String = chars[tag_start..j].iter().collect();
        if matches!(
            tag.as_str(),
            "rc" | "beta" | "alpha" | "pre" | "dev" | "hotfix" | "rev"
        ) {
            while chars.get(j).is_some_and(|c| c.is_ascii_digit()) {
                j += 1;
            }
            if !chars.get(j).is_some_and(|c| c.is_ascii_alphanumeric()) {
                i = j;
            }
        }
    }
    Some(i)
}

fn same_file_variant(staged: &NexusFileMeta, other: &NexusFileMeta) -> bool {
    let staged_key = file_variant_key(&staged.name);
    let other_key = file_variant_key(&other.name);
    staged_key.is_empty() || other_key.is_empty() || staged_key == other_key
}

fn is_word_prefix(prefix: &str, full: &str) -> bool {
    if prefix.is_empty() || full.is_empty() {
        return false;
    }
    full == prefix
        || full
            .strip_prefix(prefix)
            .is_some_and(|rest| rest.starts_with(' '))
}

fn related_file_variant(staged: &NexusFileMeta, other: &NexusFileMeta) -> bool {
    let staged_key = file_variant_key(&staged.name);
    let other_key = file_variant_key(&other.name);
    if staged_key.is_empty() || other_key.is_empty() {
        return true;
    }
    is_word_prefix(&staged_key, &other_key) || is_word_prefix(&other_key, &staged_key)
}

fn normalized_version_key(version: Option<&str>) -> Option<Vec<u64>> {
    let mut parts = parse_version_parts(version?)?;
    while parts.len() > 1 && parts.last() == Some(&0) {
        parts.pop();
    }
    Some(parts)
}

fn is_valid_nexus_update(
    staged_file_id: u64,
    effective_staged_version: Option<&str>,
    staged: Option<&NexusFileMeta>,
    candidate: &NexusFileMeta,
) -> bool {
    if candidate.file_id == staged_file_id {
        return false;
    }
    // A newer upload of an older version is a different file in the same
    // category, not an update of the one that is installed.
    match version_against_staged(effective_staged_version, candidate.version.as_deref()) {
        VersionOrder::Newer => true,
        VersionOrder::Older => false,
        VersionOrder::UseUpload => match staged {
            Some(s) => nexus_file_is_newer(candidate, s),
            None => true,
        },
    }
}

fn compare_nexus_candidates(a: &NexusFileMeta, b: &NexusFileMeta) -> std::cmp::Ordering {
    normalized_version_key(a.version.as_deref())
        .cmp(&normalized_version_key(b.version.as_deref()))
        .then_with(|| a.uploaded_timestamp.cmp(&b.uploaded_timestamp))
        .then_with(|| a.file_id.cmp(&b.file_id))
}

fn best_nexus_update<'a, F>(
    files: &'a [NexusFileMeta],
    staged_file_id: u64,
    effective_staged_version: Option<&str>,
    staged: Option<&NexusFileMeta>,
    pred: F,
) -> Option<&'a NexusFileMeta>
where
    F: Fn(&NexusFileMeta) -> bool,
{
    files
        .iter()
        .filter(|f| {
            pred(f) && is_valid_nexus_update(staged_file_id, effective_staged_version, staged, f)
        })
        .max_by(|a, b| compare_nexus_candidates(a, b))
}

/// Pick a newer Nexus file that should replace the staged file, if any.
pub fn nexus_update_candidate<'a>(
    staged_file_id: u64,
    staged_version: Option<&str>,
    files: &'a [NexusFileMeta],
) -> Option<&'a NexusFileMeta> {
    if files.is_empty() {
        return None;
    }
    let staged = files.iter().find(|f| f.file_id == staged_file_id);
    let effective_staged_version = staged_version
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .or_else(|| staged.and_then(|s| s.version.as_deref()));

    if let Some(staged) = staged {
        if is_historical_category(staged.category_name.as_deref()) {
            // Authors commonly move superseded releases to OLD_VERSION or ARCHIVED
            // (often with category_name: None in the REST API) when publishing a new
            // version. Look for the active replacement rather than staying inside the
            // historical category.
            best_nexus_update(files, staged_file_id, effective_staged_version, Some(staged), |f| {
                is_main_category(f.category_name.as_deref()) && same_file_variant(staged, f)
            })
            .or_else(|| {
                best_nexus_update(
                    files,
                    staged_file_id,
                    effective_staged_version,
                    Some(staged),
                    |f| {
                        !is_historical_category(f.category_name.as_deref())
                            && same_file_variant(staged, f)
                    },
                )
            })
            .or_else(|| {
                best_nexus_update(
                    files,
                    staged_file_id,
                    effective_staged_version,
                    Some(staged),
                    |f| {
                        is_main_category(f.category_name.as_deref())
                            && related_file_variant(staged, f)
                    },
                )
            })
            .or_else(|| {
                best_nexus_update(
                    files,
                    staged_file_id,
                    effective_staged_version,
                    Some(staged),
                    |f| {
                        !is_historical_category(f.category_name.as_deref())
                            && related_file_variant(staged, f)
                    },
                )
            })
        } else {
            let cat = category_key(staged.category_name.as_deref());
            // A different title in the same category is another main file, not a
            // newer upload of the one that is installed. Don't fall through to
            // whichever file happens to be primary.
            best_nexus_update(files, staged_file_id, effective_staged_version, Some(staged), |f| {
                category_key(f.category_name.as_deref()) == cat && same_file_variant(staged, f)
            })
        }
    } else {
        best_nexus_update(files, staged_file_id, effective_staged_version, None, |f| {
            !is_historical_category(f.category_name.as_deref()) && f.is_primary
        })
        .or_else(|| {
            best_nexus_update(files, staged_file_id, effective_staged_version, None, |f| {
                is_main_category(f.category_name.as_deref())
            })
        })
        .or_else(|| {
            best_nexus_update(files, staged_file_id, effective_staged_version, None, |f| {
                !is_historical_category(f.category_name.as_deref())
            })
        })
    }
}

enum VersionOrder {
    Newer,
    Older,
    UseUpload,
}

fn version_against_staged(staged: Option<&str>, candidate: Option<&str>) -> VersionOrder {
    let staged = staged.unwrap_or("").trim();
    let candidate = candidate.unwrap_or("").trim();
    if staged.is_empty() || candidate.is_empty() {
        return VersionOrder::UseUpload;
    }
    if incoming_is_newer(Some(staged), candidate) {
        return VersionOrder::Newer;
    }
    if incoming_is_newer(Some(candidate), staged) {
        return VersionOrder::Older;
    }
    VersionOrder::UseUpload
}

/// Same mod.io file stays. A different file replaces the staged one, including
/// when the incoming version string is missing.
pub fn keep_existing_modio_file(
    same_file: bool,
    existing_version: Option<&str>,
    incoming_version: Option<&str>,
) -> bool {
    if same_file {
        return true;
    }
    match incoming_version.map(str::trim).filter(|s| !s.is_empty()) {
        Some(incoming) => !incoming_is_newer(existing_version, incoming),
        None => false,
    }
}

fn push_unique(list: &mut Vec<String>, value: String) {
    if !list.iter().any(|s| s == &value) {
        list.push(value);
    }
}

fn rewrite_depends_on(order: &mut LoadOrder, old_id: &str, new_id: &str) {
    if old_id == new_id {
        return;
    }
    for m in &mut order.mods {
        for dep in &mut m.depends_on {
            if dep == old_id {
                *dep = new_id.to_string();
            }
        }
        let mut seen = HashSet::new();
        m.depends_on
            .retain(|s| !s.is_empty() && seen.insert(s.clone()));
    }
}

fn rewrite_collection_mod_id(
    paths: &Paths,
    game_id: &str,
    old_id: &str,
    new_id: &str,
) -> Result<()> {
    if old_id == new_id || old_id.is_empty() {
        return Ok(());
    }
    with_game_collections(game_id, || {
        let mut store = read_collections(paths, game_id)?;
        let mut changed = false;
        for collection in &mut store.collections {
            let mut seen = HashSet::new();
            let mut next = Vec::new();
            for id in collection.mod_ids.drain(..) {
                let id = if id == old_id {
                    changed = true;
                    new_id.to_string()
                } else {
                    id
                };
                if seen.insert(id.clone()) {
                    next.push(id);
                } else {
                    changed = true;
                }
            }
            collection.mod_ids = next;
        }
        if changed {
            write_collections(paths, game_id, &store)?;
        }
        Ok(())
    })
}

fn nexus_file_key(staged: &StagedMod) -> Option<(String, u64, u64)> {
    if staged.source != ModSource::Nexus || staged.nexus_mod_id == 0 {
        return None;
    }
    Some((
        staged.domain.to_lowercase(),
        staged.nexus_mod_id,
        staged.nexus_file_id,
    ))
}

struct RowCollapse {
    rewrites: Vec<(String, String)>,
    unused_staging: Vec<PathBuf>,
}

/// Drop rows that share an id or the same Nexus file. The last row wins.
/// Different file ids of the same mod are left alone.
fn collapse_duplicate_rows(order: &mut LoadOrder) -> RowCollapse {
    let mut winner_for_id: HashMap<String, usize> = HashMap::new();
    let mut winner_for_file: HashMap<(String, u64, u64), usize> = HashMap::new();
    for (index, staged) in order.mods.iter().enumerate() {
        winner_for_id.insert(staged.id.clone(), index);
        if let Some(key) = nexus_file_key(staged) {
            winner_for_file.insert(key, index);
        }
    }
    let mut drop_idx = HashSet::new();
    let mut rewrites = Vec::new();
    for (index, staged) in order.mods.iter().enumerate() {
        let id_winner = winner_for_id.get(&staged.id).copied();
        let file_winner = nexus_file_key(staged).and_then(|key| winner_for_file.get(&key).copied());
        let winner = match (id_winner, file_winner) {
            (Some(id_at), Some(file_at)) => Some(id_at.max(file_at)),
            (Some(id_at), None) => Some(id_at),
            (None, Some(file_at)) => Some(file_at),
            (None, None) => None,
        };
        if let Some(winner) = winner {
            if winner != index {
                drop_idx.insert(index);
                let new_id = order.mods[winner].id.clone();
                if staged.id != new_id {
                    rewrites.push((staged.id.clone(), new_id));
                }
            }
        }
    }
    let kept_paths: HashSet<String> = order
        .mods
        .iter()
        .enumerate()
        .filter(|(index, _)| !drop_idx.contains(index))
        .map(|(_, staged)| staged.staging_path.clone())
        .collect();
    let unused_staging = order
        .mods
        .iter()
        .enumerate()
        .filter(|(index, _)| drop_idx.contains(index))
        .map(|(_, staged)| PathBuf::from(&staged.staging_path))
        .filter(|path| !kept_paths.contains(&path.to_string_lossy().to_string()))
        .collect();
    let mut kept = Vec::new();
    for (index, staged) in order.mods.drain(..).enumerate() {
        if !drop_idx.contains(&index) {
            kept.push(staged);
        }
    }
    order.mods = kept;
    RowCollapse {
        rewrites,
        unused_staging,
    }
}

fn collapse_load_order(paths: &Paths, game_id: &str, order: &mut LoadOrder) -> Result<bool> {
    let before = order.mods.len();
    let collapse = collapse_duplicate_rows(order);
    let changed = order.mods.len() != before;
    for (old_id, new_id) in &collapse.rewrites {
        rewrite_depends_on(order, old_id, new_id);
        rewrite_collection_mod_id(paths, game_id, old_id, new_id)?;
    }
    for path in collapse.unused_staging {
        if path.exists() {
            let _ = fs::remove_dir_all(path);
        }
    }
    Ok(changed)
}

fn provenance_from_removed(
    old: Option<StagedMod>,
    old_fomod_module: Option<&fomod::FomodModule>,
    new_staging: &Path,
    preserve_fomod_selection: bool,
) -> StagingProvenance {
    let Some(old) = old else {
        return StagingProvenance {
            independent: true,
            enabled: true,
            ..Default::default()
        };
    };
    let (fomod_selection, fomod_pending) = if preserve_fomod_selection {
        (old.fomod_selection, old.fomod_pending)
    } else if let Some(ref old_sel) = old.fomod_selection {
        match fomod::load_module_in_staging(new_staging).ok().flatten() {
            Some(ref new_module) if !new_module.unsupported_scripted => {
                let migrated = fomod::migrate_selection(
                    old_fomod_module,
                    old_sel,
                    old.fomod_pending.as_ref(),
                    new_module,
                );
                (Some(migrated.selection), migrated.pending)
            }
            _ => (None, None),
        }
    } else {
        (None, None)
    };
    StagingProvenance {
        collection_ids: old.collection_ids,
        independent: old.independent,
        depends_on: old.depends_on,
        enabled: old.enabled,
        order: Some(old.order),
        // Option ids are slugs of the manifest entries, so an update that keeps
        // the same options keeps the user's picks. Anything the new manifest
        // dropped is discarded when the selection is normalized.
        option_selection: old.option_selection,
        fomod_selection,
        fomod_pending,
    }
}

fn take_rows<F>(order: &mut LoadOrder, mut pred: F) -> Vec<StagedMod>
where
    F: FnMut(&StagedMod) -> bool,
{
    let mut kept = Vec::new();
    let mut taken = Vec::new();
    for staged in order.mods.drain(..) {
        if pred(&staged) {
            taken.push(staged);
        } else {
            kept.push(staged);
        }
    }
    order.mods = kept;
    taken
}

fn discard_unreferenced_staging(order: &LoadOrder, paths: &[PathBuf], keep: &Path) {
    let used: HashSet<PathBuf> = order
        .mods
        .iter()
        .map(|staged| PathBuf::from(&staged.staging_path))
        .collect();
    for path in paths {
        if path == keep || used.contains(path) {
            continue;
        }
        if path.exists() {
            let _ = fs::remove_dir_all(path);
        }
    }
}

fn finish_replacement(
    paths: &Paths,
    game_id: &str,
    order: &mut LoadOrder,
    removed: &[StagedMod],
    staged: StagedMod,
) -> Result<StagedMod> {
    let new_staging = PathBuf::from(&staged.staging_path);
    let old_paths: Vec<PathBuf> = removed
        .iter()
        .map(|old| PathBuf::from(&old.staging_path))
        .collect();
    for old in removed {
        if old.id != staged.id {
            rewrite_depends_on(order, &old.id, &staged.id);
            rewrite_collection_mod_id(paths, game_id, &old.id, &staged.id)?;
        }
    }
    discard_unreferenced_staging(order, &old_paths, &new_staging);
    order.mods.push(staged.clone());
    Ok(staged)
}

fn temp_extract_dir(dest: &Path) -> PathBuf {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "staging".into());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    dest.with_file_name(format!(".{nanos}.{name}"))
}

fn swap_extracted_dir(temp: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)?;
    }
    match fs::rename(temp, dest) {
        Ok(()) => Ok(()),
        Err(_) => {
            copy_dir_all(temp, dest)?;
            let _ = fs::remove_dir_all(temp);
            Ok(())
        }
    }
}

fn take_row_by_id(order: &mut LoadOrder, id: &str) -> Option<StagedMod> {
    order
        .mods
        .iter()
        .position(|m| m.id == id)
        .map(|pos| order.mods.remove(pos))
}

pub fn find_thunderstore_package<'a>(
    order: &'a LoadOrder,
    namespace: &str,
    name: &str,
) -> Option<&'a StagedMod> {
    order.mods.iter().find(|m| {
        m.source == ModSource::Thunderstore
            && m.ts_namespace
                .as_deref()
                .is_some_and(|ns| ns.eq_ignore_ascii_case(namespace))
            && m.ts_name
                .as_deref()
                .is_some_and(|n| n.eq_ignore_ascii_case(name))
    })
}

pub fn find_all_thunderstore_packages<'a>(
    order: &'a LoadOrder,
    namespace: &str,
    name: &str,
) -> Vec<&'a StagedMod> {
    order
        .mods
        .iter()
        .filter(|m| {
            m.source == ModSource::Thunderstore
                && m.ts_namespace
                    .as_deref()
                    .is_some_and(|ns| ns.eq_ignore_ascii_case(namespace))
                && m.ts_name
                    .as_deref()
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
        .collect()
}

pub fn find_thunderstore_package_version<'a>(
    order: &'a LoadOrder,
    namespace: &str,
    name: &str,
    version: Option<&str>,
) -> Option<&'a StagedMod> {
    let want_ver = version.map(str::trim).filter(|s| !s.is_empty());
    let matches = find_all_thunderstore_packages(order, namespace, name);
    match want_ver {
        Some(v) => matches.into_iter().find(|m| {
            m.version
                .as_deref()
                .map(str::trim)
                .is_some_and(|mv| mv.eq_ignore_ascii_case(v))
        }),
        None => matches.into_iter().next(),
    }
}

pub fn find_modio_mod<'a>(
    order: &'a LoadOrder,
    modio_game_id: u32,
    modio_mod_id: u64,
) -> Option<&'a StagedMod> {
    order.mods.iter().find(|m| {
        m.source == ModSource::Modio
            && m.modio_game_id == Some(modio_game_id)
            && m.modio_mod_id == Some(modio_mod_id)
    })
}

pub fn find_all_modio_mods<'a>(
    order: &'a LoadOrder,
    modio_game_id: u32,
    modio_mod_id: u64,
) -> Vec<&'a StagedMod> {
    order
        .mods
        .iter()
        .filter(|m| {
            m.source == ModSource::Modio
                && m.modio_game_id == Some(modio_game_id)
                && m.modio_mod_id == Some(modio_mod_id)
        })
        .collect()
}

pub fn find_modio_file<'a>(
    order: &'a LoadOrder,
    modio_game_id: u32,
    modio_mod_id: u64,
    modio_file_id: Option<u64>,
    version: Option<&str>,
) -> Option<&'a StagedMod> {
    let matches = find_all_modio_mods(order, modio_game_id, modio_mod_id);
    if let Some(fid) = modio_file_id.filter(|&id| id > 0) {
        if let Some(found) = matches.iter().copied().find(|m| m.modio_file_id == Some(fid)) {
            return Some(found);
        }
        return None;
    }
    if let Some(want_ver) = version.map(str::trim).filter(|s| !s.is_empty()) {
        return matches.into_iter().find(|m| {
            m.version
                .as_deref()
                .map(str::trim)
                .is_some_and(|mv| mv.eq_ignore_ascii_case(want_ver))
        });
    }
    matches.into_iter().next()
}

pub fn find_nexus_file<'a>(
    order: &'a LoadOrder,
    nexus_mod_id: u64,
    nexus_file_id: u64,
) -> Option<&'a StagedMod> {
    order.mods.iter().find(|m| {
        m.source == ModSource::Nexus
            && m.nexus_mod_id == nexus_mod_id
            && m.nexus_file_id == nexus_file_id
    })
}

/// Any staged file of a Nexus mod, matched by domain and mod id.
pub fn find_nexus_mod<'a>(
    order: &'a LoadOrder,
    domain: &str,
    nexus_mod_id: u64,
) -> Option<&'a StagedMod> {
    order.mods.iter().find(|m| {
        m.source == ModSource::Nexus
            && m.nexus_mod_id == nexus_mod_id
            && (domain.is_empty() || m.domain.eq_ignore_ascii_case(domain))
    })
}

pub fn find_all_nexus_mod<'a>(
    order: &'a LoadOrder,
    domain: &str,
    nexus_mod_id: u64,
) -> Vec<&'a StagedMod> {
    order
        .mods
        .iter()
        .filter(|m| {
            m.source == ModSource::Nexus
                && m.nexus_mod_id == nexus_mod_id
                && (domain.is_empty() || m.domain.eq_ignore_ascii_case(domain))
        })
        .collect()
}

pub fn match_share_mod_entry<'a>(
    order: &'a LoadOrder,
    entry: &crate::share::ShareModEntry,
    fallback_modio_game_id: Option<u32>,
) -> Option<&'a StagedMod> {
    match entry.s {
        crate::share::ShareModSource::Nexus => {
            let mid = entry.mod_id.unwrap_or(0);
            let fid = entry.file_id.unwrap_or(0);
            find_nexus_file(order, mid, fid)
                .or_else(|| {
                    order
                        .mods
                        .iter()
                        .find(|m| m.source == ModSource::Nexus && m.nexus_mod_id == mid && m.enabled)
                })
                .or_else(|| {
                    order
                        .mods
                        .iter()
                        .find(|m| m.source == ModSource::Nexus && m.nexus_mod_id == mid)
                })
        }
        crate::share::ShareModSource::Thunderstore => {
            let ns = entry.namespace.as_deref()?;
            let name = entry.name.as_deref()?;
            find_thunderstore_package_version(order, ns, name, entry.version.as_deref())
                .or_else(|| {
                    find_all_thunderstore_packages(order, ns, name)
                        .into_iter()
                        .find(|m| m.enabled)
                })
                .or_else(|| find_thunderstore_package(order, ns, name))
        }
        crate::share::ShareModSource::Modio => {
            let gid = entry.game_id.or(fallback_modio_game_id).unwrap_or(0);
            let mid = entry.mod_id.unwrap_or(0);
            find_modio_file(order, gid, mid, entry.file_id, entry.version.as_deref())
                .or_else(|| {
                    find_all_modio_mods(order, gid, mid)
                        .into_iter()
                        .find(|m| m.enabled)
                })
                .or_else(|| find_modio_mod(order, gid, mid))
        }
    }
}

pub fn merge_mod_meta(
    paths: &Paths,
    game_id: &str,
    mod_uid: &str,
    extra_collection_ids: &[String],
    extra_depends_on: &[String],
    independent: Option<bool>,
    enabled: Option<bool>,
) -> Result<Option<StagedMod>> {
    mutate_load_order(paths, game_id, |order| {
        let Some(m) = order.mods.iter_mut().find(|m| m.id == mod_uid) else {
            return Ok(None);
        };
        for id in extra_collection_ids {
            push_unique(&mut m.collection_ids, id.clone());
        }
        for id in extra_depends_on {
            push_unique(&mut m.depends_on, id.clone());
        }
        if let Some(flag) = independent {
            m.independent = flag;
        }
        if let Some(flag) = enabled {
            m.enabled = flag;
        }
        Ok(Some(m.clone()))
    })
}

pub fn record_installed_collection(
    paths: &Paths,
    game_id: &str,
    collection: InstalledCollection,
    new_mod_ids: &[String],
) -> Result<InstalledCollection> {
    record_installed_collection_inner(paths, game_id, collection, new_mod_ids, false)
}

/// Record or update an installed collection, replacing its member list and
/// detaching/disabling any mods that were removed from the collection.
pub fn sync_installed_collection(
    paths: &Paths,
    game_id: &str,
    collection: InstalledCollection,
    new_mod_ids: &[String],
) -> Result<InstalledCollection> {
    record_installed_collection_inner(paths, game_id, collection, new_mod_ids, true)
}

fn record_installed_collection_inner(
    paths: &Paths,
    game_id: &str,
    mut collection: InstalledCollection,
    new_mod_ids: &[String],
    replace_members: bool,
) -> Result<InstalledCollection> {
    if collection.installed_at.is_empty() {
        collection.installed_at = chrono::Utc::now().to_rfc3339();
    }
    let new_set: HashSet<&str> = new_mod_ids.iter().map(|s| s.as_str()).collect();
    let member_set: HashSet<&str> = collection.mod_ids.iter().map(|s| s.as_str()).collect();
    let collection_id = collection.id.clone();
    let snapshot_sig = with_game_order(game_id, || {
        let mut order = read_loadorder(paths, game_id)?;
        collapse_load_order(paths, game_id, &mut order)?;
        for m in &mut order.mods {
            if member_set.contains(m.id.as_str()) {
                push_unique(&mut m.collection_ids, collection_id.clone());
                if new_set.contains(m.id.as_str()) {
                    m.independent = false;
                }
            } else if replace_members && m.collection_ids.iter().any(|id| id == &collection_id) {
                m.collection_ids.retain(|id| id != &collection_id);
                if !m.independent && m.collection_ids.is_empty() {
                    m.enabled = false;
                }
            }
        }
        write_loadorder(paths, game_id, &order)?;
        Ok::<String, anyhow::Error>(collection_snapshot_signature(&collection_id, &order))
    })?;

    with_game_collections(game_id, || {
        let mut store = read_collections(paths, game_id)?;
        if let Some(existing) = store.collections.iter_mut().find(|c| c.id == collection.id) {
            if collection.game_versions.is_none() && existing.revision == collection.revision {
                collection.game_versions = existing.game_versions.clone();
            }
            if collection.friend_sync.is_some() {
                collection.auto_save = false;
            } else if !collection.auto_save {
                collection.auto_save = existing.auto_save;
            }
            if !replace_members {
                let mut ids = existing.mod_ids.clone();
                for id in &collection.mod_ids {
                    push_unique(&mut ids, id.clone());
                }
                collection.mod_ids = ids;
            }
            collection.saved_snapshot_hash = Some(snapshot_sig);
            collection.has_unsaved_changes = false;
            *existing = collection.clone();
        } else {
            if collection.friend_sync.is_some() {
                collection.auto_save = false;
            }
            collection.saved_snapshot_hash = Some(snapshot_sig);
            collection.has_unsaved_changes = false;
            store.collections.push(collection.clone());
        }
        write_collections(paths, game_id, &store)?;
        Ok(collection)
    })
}

fn drop_collection_from_mods(order: &mut LoadOrder, collection_id: &str) {
    for m in &mut order.mods {
        m.collection_ids.retain(|id| id != collection_id);
    }
}

fn reachable_mod_ids(order: &LoadOrder, store: &InstalledCollections) -> HashSet<String> {
    let mut keep: HashSet<String> = HashSet::new();
    let mut queue: VecDeque<String> = VecDeque::new();
    for m in &order.mods {
        if m.independent && keep.insert(m.id.clone()) {
            queue.push_back(m.id.clone());
        }
    }
    for c in &store.collections {
        for id in &c.mod_ids {
            if order.mods.iter().any(|m| &m.id == id) && keep.insert(id.clone()) {
                queue.push_back(id.clone());
            }
        }
    }
    let by_id: HashMap<&str, &StagedMod> = order.mods.iter().map(|m| (m.id.as_str(), m)).collect();
    while let Some(id) = queue.pop_front() {
        if let Some(m) = by_id.get(id.as_str()) {
            for dep in &m.depends_on {
                if by_id.contains_key(dep.as_str()) && keep.insert(dep.clone()) {
                    queue.push_back(dep.clone());
                }
            }
        }
    }
    keep
}

fn delete_staged_row(m: &StagedMod) -> Result<()> {
    let staging = PathBuf::from(&m.staging_path);
    if staging.exists() {
        fs::remove_dir_all(&staging)?;
    }
    Ok(())
}

pub fn uninstall_collection(paths: &Paths, game_id: &str, collection_id: &str) -> Result<usize> {
    with_game_order(game_id, || {
        let mut order = read_loadorder(paths, game_id)?;
        collapse_load_order(paths, game_id, &mut order)?;
        with_game_collections(game_id, || {
            let mut store = read_collections(paths, game_id)?;
            if !store.collections.iter().any(|c| c.id == collection_id) {
                bail!("collection not found: {collection_id}");
            }
            store.collections.retain(|c| c.id != collection_id);
            drop_collection_from_mods(&mut order, collection_id);
            let keep = reachable_mod_ids(&order, &store);
            let mut removed = 0usize;
            let mut remaining = Vec::new();
            for m in order.mods.drain(..) {
                if keep.contains(&m.id) {
                    remaining.push(m);
                } else {
                    delete_staged_row(&m)?;
                    removed += 1;
                }
            }
            order.mods = remaining;
            for c in &mut store.collections {
                c.mod_ids
                    .retain(|id| order.mods.iter().any(|m| &m.id == id));
            }
            write_loadorder(paths, game_id, &order)?;
            write_collections(paths, game_id, &store)?;
            Ok(removed)
        })
    })
}

/// Drop an installed collection record and clear membership on staged mods,
/// keeping staging dirs. Missing collection id is a no-op.
pub fn forget_installed_collection(
    paths: &Paths,
    game_id: &str,
    collection_id: &str,
) -> Result<()> {
    with_game_order(game_id, || {
        with_game_collections(game_id, || {
            let mut store = read_collections(paths, game_id)?;
            if !store.collections.iter().any(|c| c.id == collection_id) {
                return Ok(());
            }
            store.collections.retain(|c| c.id != collection_id);
            let mut order = read_loadorder(paths, game_id)?;
            collapse_load_order(paths, game_id, &mut order)?;
            drop_collection_from_mods(&mut order, collection_id);
            for m in &mut order.mods {
                if m.collection_ids.is_empty() {
                    m.independent = true;
                }
            }
            write_loadorder(paths, game_id, &order)?;
            write_collections(paths, game_id, &store)?;
            Ok(())
        })
    })
}

#[derive(Serialize)]
struct CollectionSnapshotMember<'a> {
    id: &'a str,
    order: u32,
    enabled: bool,
    version: &'a Option<String>,
    nexus_file_id: u64,
    modio_file_id: Option<u64>,
    fomod_selection: &'a Option<FomodSelection>,
    option_selection: &'a Option<ModOptionSelection>,
}

pub fn collection_snapshot_signature(collection_id: &str, order: &LoadOrder) -> String {
    let mut members: Vec<&StagedMod> = order
        .mods
        .iter()
        .filter(|m| m.collection_ids.iter().any(|id| id == collection_id))
        .collect();
    members.sort_by_key(|m| m.order);
    let payload: Vec<CollectionSnapshotMember<'_>> = members
        .into_iter()
        .map(|m| CollectionSnapshotMember {
            id: m.id.as_str(),
            order: m.order,
            enabled: m.enabled,
            version: &m.version,
            nexus_file_id: m.nexus_file_id,
            modio_file_id: m.modio_file_id,
            fomod_selection: &m.fomod_selection,
            option_selection: &m.option_selection,
        })
        .collect();
    let json = serde_json::to_string(&payload).unwrap_or_default();
    crate::share::share_hash(&json)
}

pub fn mutate_collections<T>(
    paths: &Paths,
    game_id: &str,
    f: impl FnOnce(&mut InstalledCollections) -> Result<T>,
) -> Result<T> {
    with_game_collections(game_id, || {
        let mut store = read_collections(paths, game_id)?;
        let result = f(&mut store)?;
        write_collections(paths, game_id, &store)?;
        Ok(result)
    })
}

pub fn list_installed_collections(
    paths: &Paths,
    game_id: &str,
) -> Result<Vec<InstalledCollection>> {
    let order = load_loadorder(paths, game_id)?;
    let mut ordered_mods: Vec<&StagedMod> = order.mods.iter().collect();
    ordered_mods.sort_by_key(|m| m.order);

    let (collections, missing_links) = with_game_collections(game_id, || -> Result<_> {
        let mut store = read_collections(paths, game_id)?;
        if store.collections.is_empty() {
            return Ok((store.collections, Vec::new()));
        }
        let mut dirty = false;
        let mut missing_links: Vec<(String, String)> = Vec::new();
        for col in &mut store.collections {
            let mut seen = HashSet::new();
            let before_len = col.mod_ids.len();
            col.mod_ids.retain(|id| seen.insert(id.clone()));
            if col.mod_ids.len() != before_len {
                dirty = true;
            }
            for m in &ordered_mods {
                if m.collection_ids.iter().any(|id| id == &col.id) && seen.insert(m.id.clone()) {
                    col.mod_ids.push(m.id.clone());
                    dirty = true;
                }
            }
            if col.friend_sync.is_some() {
                if let Some(code) = col.profile_code.as_deref() {
                    if let Ok(manifest) = crate::share::decode_share_code(code) {
                        if col.mod_ids.len() < manifest.mods.len() {
                            for entry in &manifest.mods {
                                if let Some(staged) = match_share_mod_entry(&order, entry, None) {
                                    if seen.insert(staged.id.clone()) {
                                        col.mod_ids.push(staged.id.clone());
                                        dirty = true;
                                    }
                                    if !staged.collection_ids.iter().any(|id| id == &col.id) {
                                        missing_links.push((col.id.clone(), staged.id.clone()));
                                    }
                                }
                            }
                        }
                    }
                }
            }
            let sig = collection_snapshot_signature(&col.id, &order);
            if col.saved_snapshot_hash.is_none() {
                col.saved_snapshot_hash = Some(sig.clone());
                dirty = true;
            }
            if col.friend_sync.is_some() {
                if col.auto_save {
                    col.auto_save = false;
                    dirty = true;
                }
                col.has_unsaved_changes = false;
            } else {
                col.has_unsaved_changes = col.saved_snapshot_hash.as_deref() != Some(sig.as_str());
            }
        }
        if dirty {
            write_collections(paths, game_id, &store)?;
        }
        Ok((store.collections, missing_links))
    })?;

    if !missing_links.is_empty() {
        let _ = with_game_order(game_id, || {
            let mut order = read_loadorder(paths, game_id)?;
            collapse_load_order(paths, game_id, &mut order)?;
            let mut order_dirty = false;
            for (col_id, mod_id) in &missing_links {
                if let Some(m) = order.mods.iter_mut().find(|m| &m.id == mod_id) {
                    let before = m.collection_ids.len();
                    push_unique(&mut m.collection_ids, col_id.clone());
                    if m.collection_ids.len() != before {
                        order_dirty = true;
                    }
                }
            }
            if order_dirty {
                write_loadorder(paths, game_id, &order)?;
            }
            Ok::<(), anyhow::Error>(())
        });
    }

    Ok(collections)
}

pub fn set_collection_auto_save(
    paths: &Paths,
    game_id: &str,
    collection_id: &str,
    auto_save: bool,
) -> Result<InstalledCollection> {
    mutate_collections(paths, game_id, |store| {
        let col = store
            .collections
            .iter_mut()
            .find(|c| c.id == collection_id)
            .ok_or_else(|| anyhow::anyhow!("Collection not found: {collection_id}"))?;
        if col.friend_sync.is_some() {
            bail!(
                "cannot change auto-save for synced collection \"{}\"",
                col.name
            );
        }
        col.auto_save = auto_save;
        Ok(col.clone())
    })
}

pub struct RenamedInstalledCollection {
    pub collection: InstalledCollection,
    pub old_code: Option<String>,
}

pub fn rename_installed_collection(
    paths: &Paths,
    game_id: &str,
    collection_id: &str,
    name: &str,
) -> Result<RenamedInstalledCollection> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(anyhow::anyhow!("Name cannot be empty."));
    }
    mutate_collections(paths, game_id, |store| {
        let col = store
            .collections
            .iter_mut()
            .find(|c| c.id == collection_id)
            .ok_or_else(|| anyhow::anyhow!("Collection not found: {collection_id}"))?;
        let old_code = col.profile_code.clone();
        col.name = trimmed.to_string();
        if let Some(code) = col.profile_code.as_deref() {
            if let Ok(mut manifest) = crate::share::decode_share_code(code) {
                manifest.name = Some(trimmed.to_string());
                col.profile_code = Some(crate::share::encode_share_manifest(&manifest)?);
            }
        }
        let updated = col.clone();
        Ok(RenamedInstalledCollection {
            collection: updated,
            old_code,
        })
    })
}

pub fn sync_installed_collection_rename(
    paths: &Paths,
    game_id: &str,
    share_id: &str,
    old_code: &str,
    name: &str,
    new_code: &str,
) -> Result<bool> {
    with_game_collections(game_id, || {
        let mut store = read_collections(paths, game_id)?;
        let mut changed = false;
        let trimmed_old = old_code.trim();
        for col in &mut store.collections {
            let matches = col.id == share_id
                || (col.source == CollectionSource::Emperor
                    && col.profile_code.as_deref().map(str::trim) == Some(trimmed_old));
            if matches {
                col.name = name.to_string();
                if col.source == CollectionSource::Emperor {
                    col.profile_code = Some(new_code.to_string());
                }
                changed = true;
            }
        }
        if changed {
            write_collections(paths, game_id, &store)?;
        }
        Ok(changed)
    })
}

/// Return the name of the first synced friend collection (`friend_sync.is_some()`)
/// that includes `mod_uid` (via either `InstalledCollection.mod_ids` or
/// `StagedMod.collection_ids`), if any.
pub fn synced_collection_for_mod(
    paths: &Paths,
    game_id: &str,
    mod_uid: &str,
) -> Result<Option<String>> {
    let store = load_collections(paths, game_id)?;
    let synced_cols: Vec<&InstalledCollection> = store
        .collections
        .iter()
        .filter(|c| c.friend_sync.is_some())
        .collect();
    if synced_cols.is_empty() {
        return Ok(None);
    }
    for col in &synced_cols {
        if col.mod_ids.iter().any(|id| id == mod_uid) {
            return Ok(Some(col.name.clone()));
        }
    }
    let order = load_loadorder(paths, game_id)?;
    if let Some(m) = order.mods.iter().find(|m| m.id == mod_uid) {
        for col in &synced_cols {
            if m.collection_ids.iter().any(|cid| cid == &col.id) {
                return Ok(Some(col.name.clone()));
            }
        }
    }
    Ok(None)
}

/// Ensure that `ordered_ids` preserves the current relative load order of every
/// synced friend collection's member mods.
pub fn ensure_load_order_preserves_synced_collections(
    paths: &Paths,
    game_id: &str,
    ordered_ids: &[String],
) -> Result<()> {
    let store = load_collections(paths, game_id)?;
    let synced_cols: Vec<&InstalledCollection> = store
        .collections
        .iter()
        .filter(|c| c.friend_sync.is_some())
        .collect();
    if synced_cols.is_empty() {
        return Ok(());
    }
    let order = load_loadorder(paths, game_id)?;
    let mut current_sorted: Vec<&StagedMod> = order.mods.iter().collect();
    current_sorted.sort_by_key(|m| m.order);

    // Build the resulting full order the same way `set_load_order` does.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut next_order: Vec<&str> = Vec::with_capacity(order.mods.len());
    for id in ordered_ids {
        if order.mods.iter().any(|m| m.id == *id) && seen.insert(id.as_str()) {
            next_order.push(id.as_str());
        }
    }
    for m in &current_sorted {
        if seen.insert(m.id.as_str()) {
            next_order.push(m.id.as_str());
        }
    }

    for col in synced_cols {
        let mut member_set: HashSet<&str> = col.mod_ids.iter().map(|s| s.as_str()).collect();
        for m in &order.mods {
            if m.collection_ids.iter().any(|cid| cid == &col.id) {
                member_set.insert(m.id.as_str());
            }
        }
        if member_set.len() < 2 {
            continue;
        }
        let current_rel: Vec<&str> = current_sorted
            .iter()
            .filter(|m| member_set.contains(m.id.as_str()))
            .map(|m| m.id.as_str())
            .collect();
        let next_rel: Vec<&str> = next_order
            .iter()
            .copied()
            .filter(|id| member_set.contains(*id))
            .collect();
        if current_rel != next_rel {
            bail!(
                "cannot reorder mods in synced collection \"{}\"",
                col.name
            );
        }
    }
    Ok(())
}

/// Update which installed collections a staged mod belongs to, keeping both
/// `StagedMod.collection_ids` (plus `independent`) and `InstalledCollection.mod_ids`
/// in sync.
pub fn set_mod_collections(
    paths: &Paths,
    game_id: &str,
    mod_uid: &str,
    collection_ids: &[String],
) -> Result<StagedMod> {
    with_game_order(game_id, || {
        let mut order = read_loadorder(paths, game_id)?;
        collapse_load_order(paths, game_id, &mut order)?;
        with_game_collections(game_id, || {
            let mut store = read_collections(paths, game_id)?;

            let Some(m) = order.mods.iter_mut().find(|m| m.id == mod_uid) else {
                bail!("staged mod not found: {mod_uid}");
            };

            let mut target_ids: Vec<String> = Vec::new();
            for id in collection_ids {
                let Some(col) = store.collections.iter().find(|c| &c.id == id) else {
                    bail!("collection not found: {id}");
                };
                let already_member = m.collection_ids.iter().any(|existing| existing == id)
                    || col.mod_ids.iter().any(|existing| existing == mod_uid);
                if col.friend_sync.is_some() && !already_member {
                    bail!("cannot add a mod to synced collection \"{}\"", col.name);
                }
                push_unique(&mut target_ids, id.clone());
            }

            for col in &store.collections {
                if col.friend_sync.is_none() {
                    continue;
                }
                let currently_member = m.collection_ids.iter().any(|existing| existing == &col.id)
                    || col.mod_ids.iter().any(|existing| existing == mod_uid);
                if currently_member && !target_ids.iter().any(|id| id == &col.id) {
                    bail!("cannot remove a mod from synced collection \"{}\"", col.name);
                }
            }

            m.collection_ids = target_ids.clone();
            m.independent = target_ids.is_empty();
            let updated = m.clone();

            for c in &mut store.collections {
                if target_ids.iter().any(|id| id == &c.id) {
                    push_unique(&mut c.mod_ids, mod_uid.to_string());
                } else {
                    c.mod_ids.retain(|id| id != mod_uid);
                }
            }

            write_loadorder(paths, game_id, &order)?;
            write_collections(paths, game_id, &store)?;
            Ok(updated)
        })
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CollectionModIssueKind {
    MissingFromLoadOrder,
    MissingStagingDir,
    CorruptedStaging {
        actual_files: usize,
        expected_files: Option<usize>,
    },
    MissingFromManifest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionModIssue {
    pub mod_id: String,
    pub name: String,
    pub source: ModSource,
    pub issue: CollectionModIssueKind,
    pub can_retry: bool,
    pub domain: String,
    pub nexus_mod_id: u64,
    pub nexus_file_id: u64,
    pub ts_namespace: Option<String>,
    pub ts_name: Option<String>,
    pub modio_game_id: Option<u32>,
    pub modio_mod_id: Option<u64>,
    pub modio_file_id: Option<u64>,
    pub version: Option<String>,
    pub file_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollectionIntegrityReport {
    pub collection_id: String,
    pub collection_name: String,
    pub checked_count: usize,
    pub issues: Vec<CollectionModIssue>,
}

pub fn staged_mod_can_retry(staged: &StagedMod) -> bool {
    match staged.source {
        ModSource::Nexus => {
            !staged.domain.trim().is_empty() && staged.nexus_mod_id > 0 && staged.nexus_file_id > 0
        }
        ModSource::Thunderstore => {
            !staged.domain.trim().is_empty()
                && staged.ts_namespace.as_deref().is_some_and(|s| !s.trim().is_empty())
                && staged.ts_name.as_deref().is_some_and(|s| !s.trim().is_empty())
        }
        ModSource::Modio => {
            staged.modio_game_id.is_some_and(|id| id > 0)
                && staged.modio_mod_id.is_some_and(|id| id > 0)
        }
    }
}

pub fn issue_from_staged(staged: &StagedMod, issue: CollectionModIssueKind) -> CollectionModIssue {
    CollectionModIssue {
        mod_id: staged.id.clone(),
        name: staged.name.clone(),
        source: staged.source.clone(),
        issue,
        can_retry: staged_mod_can_retry(staged),
        domain: staged.domain.clone(),
        nexus_mod_id: staged.nexus_mod_id,
        nexus_file_id: staged.nexus_file_id,
        ts_namespace: staged.ts_namespace.clone(),
        ts_name: staged.ts_name.clone(),
        modio_game_id: staged.modio_game_id,
        modio_mod_id: staged.modio_mod_id,
        modio_file_id: staged.modio_file_id,
        version: staged.version.clone(),
        file_name: staged.file_name.clone(),
    }
}

pub fn check_staged_mod_integrity(staged: &StagedMod) -> Option<CollectionModIssueKind> {
    let staging = Path::new(&staged.staging_path);
    if !staging.is_dir() {
        return Some(CollectionModIssueKind::MissingStagingDir);
    }
    let actual_files = count_staged_files(staging);
    if actual_files == 0 {
        return Some(CollectionModIssueKind::CorruptedStaging {
            actual_files,
            expected_files: staged.staged_file_count,
        });
    }
    if let Some(expected) = staged.staged_file_count {
        if actual_files < expected {
            return Some(CollectionModIssueKind::CorruptedStaging {
                actual_files,
                expected_files: Some(expected),
            });
        }
    }
    None
}

/// Checks every mod ID recorded on an `InstalledCollection` against `order`
/// and its staging directory on disk.
pub fn check_collection_local_integrity(
    collection: &InstalledCollection,
    order: &LoadOrder,
) -> CollectionIntegrityReport {
    let by_id: HashMap<&str, &StagedMod> = order.mods.iter().map(|m| (m.id.as_str(), m)).collect();
    let mut issues = Vec::new();
    let mut checked_ids = HashSet::new();

    for mod_id in &collection.mod_ids {
        if !checked_ids.insert(mod_id.clone()) {
            continue;
        }
        let Some(staged) = by_id.get(mod_id.as_str()) else {
            issues.push(CollectionModIssue {
                mod_id: mod_id.clone(),
                name: mod_id.clone(),
                source: match collection.source {
                    CollectionSource::Thunderstore => ModSource::Thunderstore,
                    _ => ModSource::Nexus,
                },
                issue: CollectionModIssueKind::MissingFromLoadOrder,
                can_retry: false,
                domain: String::new(),
                nexus_mod_id: 0,
                nexus_file_id: 0,
                ts_namespace: None,
                ts_name: None,
                modio_game_id: None,
                modio_mod_id: None,
                modio_file_id: None,
                version: None,
                file_name: None,
            });
            continue;
        };
        if let Some(kind) = check_staged_mod_integrity(staged) {
            issues.push(issue_from_staged(staged, kind));
        }
    }

    CollectionIntegrityReport {
        collection_id: collection.id.clone(),
        collection_name: collection.name.clone(),
        checked_count: checked_ids.len(),
        issues,
    }
}

pub fn cache_supported_game_versions(
    paths: &Paths,
    game_id: &str,
    updates: &HashMap<String, Vec<String>>,
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    mutate_load_order(paths, game_id, |order| {
        for staged in &mut order.mods {
            if let Some(versions) = updates.get(&staged.id) {
                staged.supported_game_versions = Some(versions.clone());
                staged.version_support_generation = game_version::VERSION_SUPPORT_GENERATION;
            }
        }
        Ok(())
    })
}

pub fn cache_collection_game_versions(
    paths: &Paths,
    game_id: &str,
    updates: &HashMap<String, Vec<String>>,
) -> Result<()> {
    if updates.is_empty() {
        return Ok(());
    }
    mutate_collections(paths, game_id, |store| {
        for collection in &mut store.collections {
            if let Some(versions) = updates.get(&collection.id) {
                collection.game_versions = Some(versions.clone());
            }
        }
        Ok(())
    })
}

fn copy_dir_all(src: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;
    for entry in WalkDir::new(src) {
        let entry = entry?;
        let rel = entry.path().strip_prefix(src).unwrap_or(entry.path());
        if rel.as_os_str().is_empty() {
            continue;
        }
        let out = dest.join(rel);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&out)?;
        } else if entry.file_type().is_file() {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), &out)?;
        }
    }
    Ok(())
}

pub fn stage_overlay_dir(
    paths: &Paths,
    game_id: &str,
    id: &str,
    name: &str,
    source_dir: &Path,
) -> Result<StagedMod> {
    crate::config::ensure_game_dirs(paths, game_id)?;
    let safe = sanitize_filename::sanitize(name);
    let staging = paths.mods_dir(game_id).join(format!("{safe}_{id}"));
    let temp = temp_extract_dir(&staging);
    if let Err(e) = copy_dir_all(source_dir, &temp) {
        let _ = fs::remove_dir_all(&temp);
        return Err(e);
    }
    let committed = mutate_load_order(paths, game_id, |order| {
        let old_fomod_module = order
            .mods
            .iter()
            .find(|m| m.id == id)
            .and_then(|old| fomod::load_module_in_staging(Path::new(&old.staging_path)).ok().flatten());
        swap_extracted_dir(&temp, &staging)?;
        let removed = take_rows(order, |m| m.id == id);
        let provenance = provenance_from_removed(
            removed.first().cloned(),
            old_fomod_module.as_ref(),
            &staging,
            false,
        );
        let next_order = provenance
            .order
            .unwrap_or_else(|| order.mods.iter().map(|m| m.order).max().unwrap_or(0) + 1);
        let staged = StagedMod {
            id: id.to_string(),
            name: name.to_string(),
            source: ModSource::Thunderstore,
            version: None,
            domain: "profile".into(),
            staging_path: staging.to_string_lossy().to_string(),
            enabled: provenance.enabled,
            order: next_order,
            ts_namespace: Some("profile".into()),
            ts_name: Some(name.to_string()),
            collection_ids: provenance.collection_ids,
            independent: provenance.independent,
            depends_on: provenance.depends_on,
            option_selection: provenance.option_selection,
            fomod_selection: provenance.fomod_selection,
            fomod_pending: provenance.fomod_pending,
            picture_url: resolve_picture_url(None, &removed),
            ..Default::default()
        };
        finish_replacement(paths, game_id, order, &removed, staged)
    });
    if committed.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    committed
}

pub fn plugin_tool_mod_id(tool_id: &str) -> String {
    format!("tool:{tool_id}")
}

pub fn find_plugin_tool_mod(paths: &Paths, game_id: &str, tool_id: &str) -> Option<StagedMod> {
    let target_id = plugin_tool_mod_id(tool_id);
    let order = load_loadorder(paths, game_id).ok()?;
    order.mods.into_iter().find(|m| {
        m.plugin_tool_id.as_deref() == Some(tool_id) || m.id == target_id
    })
}

#[allow(clippy::too_many_arguments)]
pub fn stage_plugin_tool_mod(
    paths: &Paths,
    game_id: &str,
    tool_id: &str,
    name: &str,
    domain: &str,
    version: Option<String>,
    source_dir: &Path,
    depends_on: &[String],
) -> Result<StagedMod> {
    crate::config::ensure_game_dirs(paths, game_id)?;
    let id = plugin_tool_mod_id(tool_id);
    let safe = sanitize_filename::sanitize(name);
    let safe_tool = sanitize_filename::sanitize(tool_id);
    let staging = paths.mods_dir(game_id).join(format!("{safe}_tool_{safe_tool}"));
    let temp = temp_extract_dir(&staging);
    if let Err(e) = copy_dir_all(source_dir, &temp) {
        let _ = fs::remove_dir_all(&temp);
        return Err(e);
    }
    let committed = mutate_load_order(paths, game_id, |order| {
        let old_fomod_module = order
            .mods
            .iter()
            .find(|m| m.id == id || m.plugin_tool_id.as_deref() == Some(tool_id))
            .and_then(|old| {
                fomod::load_module_in_staging(Path::new(&old.staging_path))
                    .ok()
                    .flatten()
            });
        swap_extracted_dir(&temp, &staging)?;
        let removed = take_rows(order, |m| {
            m.id == id || m.plugin_tool_id.as_deref() == Some(tool_id)
        });
        let provenance = provenance_from_removed(
            removed.first().cloned(),
            old_fomod_module.as_ref(),
            &staging,
            false,
        );
        let next_order = provenance
            .order
            .unwrap_or_else(|| order.mods.iter().map(|m| m.order).max().unwrap_or(0) + 1);
        let mut merged_depends = provenance.depends_on;
        for dep in depends_on {
            if !merged_depends.iter().any(|existing| existing == dep) {
                merged_depends.push(dep.clone());
            }
        }
        let staged = StagedMod {
            id: id.clone(),
            name: name.to_string(),
            source: ModSource::Nexus,
            nexus_mod_id: 0,
            nexus_file_id: 0,
            version: version.clone(),
            domain: domain.to_string(),
            staging_path: staging.to_string_lossy().to_string(),
            enabled: provenance.enabled,
            order: next_order,
            collection_ids: provenance.collection_ids,
            independent: provenance.independent,
            depends_on: merged_depends,
            staged_file_count: Some(count_staged_files(&staging)),
            option_selection: provenance.option_selection,
            fomod_selection: provenance.fomod_selection,
            fomod_pending: provenance.fomod_pending,
            picture_url: resolve_picture_url(None, &removed),
            plugin_tool_id: Some(tool_id.to_string()),
            ..Default::default()
        };
        finish_replacement(paths, game_id, order, &removed, staged)
    });
    if committed.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    committed
}

/// Mod payloads that are single files rather than archives (e.g. Sims `.package`
/// downloads). These are copied into staging as-is.
pub const LOOSE_MOD_EXTENSIONS: &[&str] = &["package", "ts4script", "dbc", "sims3pack"];

pub fn is_loose_mod_file(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|ext| {
            LOOSE_MOD_EXTENSIONS
                .iter()
                .any(|known| ext.eq_ignore_ascii_case(known))
        })
}

fn stage_loose_file(file: &Path, dest: &Path) -> Result<()> {
    let name = file
        .file_name()
        .map(|n| sanitize_filename::sanitize(n.to_string_lossy().as_ref()))
        .filter(|n| !n.is_empty())
        .context("mod file has no usable file name")?;
    let target = dest.join(name);
    fs::copy(file, &target)
        .with_context(|| format!("copy {} to {}", file.display(), target.display()))?;
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ArchiveKind {
    Zip,
    SevenZ,
    Rar,
    /// A Sims `.package`; a payload, not a container.
    Dbpf,
}

impl ArchiveKind {
    fn label(self) -> &'static str {
        match self {
            Self::Zip => "zip",
            Self::SevenZ => "7z",
            Self::Rar => "rar",
            Self::Dbpf => "Sims package",
        }
    }
}

/// Identify a file by its header. Downloads are saved under a synthesized name,
/// so the extension says what the app guessed rather than what the file is.
fn detect_archive_kind(path: &Path) -> Option<ArchiveKind> {
    let mut file = fs::File::open(path).ok()?;
    let mut magic = [0u8; 8];
    let read = file.read(&mut magic).ok()?;
    let magic = &magic[..read];
    if magic.starts_with(b"PK\x03\x04")
        || magic.starts_with(b"PK\x05\x06")
        || magic.starts_with(b"PK\x07\x08")
    {
        return Some(ArchiveKind::Zip);
    }
    if magic.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return Some(ArchiveKind::SevenZ);
    }
    if magic.starts_with(b"Rar!\x1a\x07") {
        return Some(ArchiveKind::Rar);
    }
    if magic.starts_with(b"DBPF") {
        return Some(ArchiveKind::Dbpf);
    }
    None
}

fn extract_by_kind(kind: ArchiveKind, archive: &Path, dest: &Path) -> Result<()> {
    match kind {
        ArchiveKind::Zip => extract_zip(archive, dest),
        ArchiveKind::SevenZ => extract_7z(archive, dest),
        ArchiveKind::Rar => extract_rar(archive, dest),
        ArchiveKind::Dbpf => stage_loose_file(archive, dest),
    }
}

/// Wrap a failure with what the file actually is, since the caller only knows
/// the name the app invented for it.
fn unreadable_archive(
    archive: &Path,
    detected: Option<ArchiveKind>,
    cause: anyhow::Error,
) -> anyhow::Error {
    let name = archive
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| archive.display().to_string());
    let size = fs::metadata(archive).map(|m| m.len()).unwrap_or(0);
    match detected {
        Some(kind) => anyhow::anyhow!(
            "could not read {name} ({size} bytes, looks like {}): {cause:#}. \
             The download may be incomplete — remove it and download again.",
            kind.label()
        ),
        None => anyhow::anyhow!(
            "could not read {name} ({size} bytes): not a zip, 7z, or rar archive. \
             The download may be incomplete — remove it and download again."
        ),
    }
}

pub fn extract_archive(archive: &Path, dest: &Path) -> Result<()> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;

    // The header wins over the extension: Nexus downloads land as `.bin` and
    // mod.io downloads as `.zip` regardless of what they really contain.
    let detected = detect_archive_kind(archive);
    if let Some(kind) = detected {
        // A loose payload named like an archive is still a loose payload, but a
        // `.ts4script` is a real zip, so the extension decides between them.
        if kind == ArchiveKind::Zip && is_loose_mod_file(archive) {
            return stage_loose_file(archive, dest);
        }
        extract_by_kind(kind, archive, dest)
            .map_err(|e| unreadable_archive(archive, detected, e))?;
        repair_backslash_entries(dest)?;
        return Ok(());
    }

    if is_loose_mod_file(archive) {
        return stage_loose_file(archive, dest);
    }

    // Headerless or unrecognized: fall back to whatever the name claims.
    let ext = archive
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();
    let result = match ext.as_str() {
        "zip" => extract_zip(archive, dest),
        "7z" => extract_7z(archive, dest),
        "rar" => extract_rar(archive, dest),
        _ => Err(anyhow::anyhow!("unrecognized file header")),
    };
    result.map_err(|e| unreadable_archive(archive, detected, e))?;
    repair_backslash_entries(dest)?;
    Ok(())
}

/// Normalize a zip/archive entry name into a safe relative path.
/// Converts Windows `\` separators; rejects `..` and absolute paths.
pub fn safe_archive_entry_path(name: &str) -> Option<PathBuf> {
    let normalized = name.replace('\\', "/");
    let trimmed = normalized.trim_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    let mut out = PathBuf::new();
    for comp in Path::new(trimmed).components() {
        match comp {
            Component::Normal(part) => out.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    if out.as_os_str().is_empty() {
        return None;
    }
    Some(out)
}

fn archive_entry_is_dir(raw_name: &str) -> bool {
    raw_name.ends_with('/') || raw_name.ends_with('\\')
}

/// Move files whose names contain literal `\` into a normal directory tree.
///
/// Windows-built zips sometimes extract on Linux as flat names like
/// `BepInEx\plugins\Mod.dll` instead of nested folders.
pub fn repair_backslash_entries(root: &Path) -> Result<()> {
    if !root.is_dir() {
        return Ok(());
    }

    let entries: Vec<_> = fs::read_dir(root)?.filter_map(|e| e.ok()).collect();
    let mut repairs: Vec<(PathBuf, PathBuf)> = Vec::new();
    for entry in &entries {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if !name_str.contains('\\') {
            continue;
        }
        let rel = normalize_relative(Path::new(&*name_str));
        if rel.as_os_str().is_empty() {
            continue;
        }
        repairs.push((entry.path(), root.join(rel)));
    }

    repairs.sort_by_key(|(_, dest)| std::cmp::Reverse(dest.components().count()));
    for (src, dest) in repairs {
        if !src.exists() {
            continue;
        }
        let meta = fs::metadata(&src)?;
        if meta.is_file() && meta.len() == 0 && dest.is_dir() {
            fs::remove_file(&src)?;
            continue;
        }
        if dest.exists() && dest.is_dir() && meta.is_file() {
            let file_name = src
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            fs::rename(&src, dest.join(file_name)).with_context(|| {
                format!(
                    "repair backslash file {} into {}",
                    src.display(),
                    dest.display()
                )
            })?;
            continue;
        }
        if dest.exists() && dest.is_file() && meta.is_file() && meta.len() > 0 {
            fs::remove_file(&dest)?;
        } else if dest.exists() && dest.is_file() && meta.is_file() && meta.len() == 0 {
            fs::remove_file(&src)?;
            continue;
        }
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::rename(&src, &dest).with_context(|| {
            format!(
                "repair backslash path {} -> {}",
                src.display(),
                dest.display()
            )
        })?;
    }

    for entry in fs::read_dir(root)?.filter_map(|e| e.ok()) {
        if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            repair_backslash_entries(&entry.path())?;
        }
    }
    Ok(())
}

fn extract_zip(archive: &Path, dest: &Path) -> Result<()> {
    let file = fs::File::open(archive)?;
    let mut archive = zip::ZipArchive::new(file)?;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i)?;
        let raw_name = file.name();
        let is_dir = archive_entry_is_dir(raw_name);
        let rel = match safe_archive_entry_path(raw_name.trim_end_matches(['/', '\\'])) {
            Some(p) => p,
            None => continue,
        };
        let outpath = dest.join(rel);
        if is_dir {
            fs::create_dir_all(&outpath)?;
        } else {
            if let Some(parent) = outpath.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut outfile = fs::File::create(&outpath)?;
            std::io::copy(&mut file, &mut outfile)?;
        }
    }
    Ok(())
}

fn extract_7z(archive: &Path, dest: &Path) -> Result<()> {
    sevenz_rust::decompress_file(archive, dest).map_err(|e| anyhow::anyhow!("7z extract: {e}"))
}

fn extract_rar(archive: &Path, dest: &Path) -> Result<()> {
    let archive = unrar_ng::Archive::new(archive)
        .open_for_processing()
        .map_err(|e| anyhow::anyhow!("rar open: {e}"))?;
    archive
        .extract_all(dest)
        .map_err(|e| anyhow::anyhow!("rar extract: {e}"))?;
    Ok(())
}

pub fn stage_mod(
    paths: &Paths,
    game_id: &str,
    name: &str,
    domain: &str,
    mod_id: u64,
    file_id: u64,
    version: Option<String>,
    file_name: Option<String>,
    archive: &Path,
) -> Result<StagedMod> {
    stage_mod_inner(
        paths, game_id, None, name, domain, mod_id, file_id, version, file_name, archive,
    )
}

/// Stage a Nexus archive, replacing a specific staged row (e.g. updating to a new file id).
pub fn stage_mod_replacing(
    paths: &Paths,
    game_id: &str,
    replace_id: &str,
    name: &str,
    domain: &str,
    mod_id: u64,
    file_id: u64,
    version: Option<String>,
    file_name: Option<String>,
    archive: &Path,
) -> Result<StagedMod> {
    stage_mod_inner(
        paths,
        game_id,
        Some(replace_id),
        name,
        domain,
        mod_id,
        file_id,
        version,
        file_name,
        archive,
    )
}

fn stage_mod_inner(
    paths: &Paths,
    game_id: &str,
    replace_id: Option<&str>,
    name: &str,
    domain: &str,
    mod_id: u64,
    file_id: u64,
    version: Option<String>,
    file_name: Option<String>,
    archive: &Path,
) -> Result<StagedMod> {
    stage_filled_mod(
        paths,
        game_id,
        replace_id,
        name,
        domain,
        mod_id,
        file_id,
        version,
        file_name,
        |temp| extract_archive(archive, temp),
    )
}

/// Fill a temp directory, then commit it as a local Nexus-shaped staged mod.
fn stage_filled_mod(
    paths: &Paths,
    game_id: &str,
    replace_id: Option<&str>,
    name: &str,
    domain: &str,
    mod_id: u64,
    file_id: u64,
    version: Option<String>,
    file_name: Option<String>,
    fill: impl FnOnce(&Path) -> Result<()>,
) -> Result<StagedMod> {
    crate::config::ensure_game_dirs(paths, game_id)?;
    let safe = sanitize_filename::sanitize(name);
    let staging = paths
        .mods_dir(game_id)
        .join(format!("{safe}_{mod_id}_{file_id}"));
    let temp = temp_extract_dir(&staging);
    if let Err(e) = fill(&temp) {
        let _ = fs::remove_dir_all(&temp);
        return Err(e);
    }

    let new_id = format!("{mod_id}_{file_id}");
    let domain_key = domain.to_lowercase();
    let committed = mutate_load_order(paths, game_id, |order| {
        swap_extracted_dir(&temp, &staging)?;
        let mut removed = Vec::new();
        if let Some(id) = replace_id {
            if let Some(row) = take_row_by_id(order, id) {
                removed.push(row);
            }
        }
        removed.extend(take_rows(order, |m| m.id == new_id));
        removed.extend(take_rows(order, |m| {
            m.source == ModSource::Nexus
                && m.nexus_mod_id == mod_id
                && m.nexus_file_id == file_id
                && m.domain.to_lowercase() == domain_key
        }));
        let stored_file_name = file_name.clone().or_else(|| {
            removed.first().and_then(|old| {
                (old.nexus_mod_id == mod_id && old.nexus_file_id == file_id)
                    .then(|| old.file_name.clone())
                    .flatten()
            })
        });
        let same_file = mod_id != 0
            && file_id != 0
            && removed.first().is_some_and(|old| {
                old.source == ModSource::Nexus
                    && old.nexus_mod_id == mod_id
                    && old.nexus_file_id == file_id
                    && (version.is_none() || old.version == version)
            });
        let old_fomod_module = (!same_file).then(|| {
            removed.first().and_then(|old| {
                (Path::new(&old.staging_path) != staging.as_path())
                    .then(|| fomod::load_module_in_staging(Path::new(&old.staging_path)).ok().flatten())
                    .flatten()
            })
        }).flatten();
        let provenance = provenance_from_removed(
            removed.first().cloned(),
            old_fomod_module.as_ref(),
            &staging,
            same_file,
        );
        let next_order = provenance
            .order
            .unwrap_or_else(|| order.mods.iter().map(|m| m.order).max().unwrap_or(0) + 1);
        let staged = StagedMod {
            id: new_id.clone(),
            name: name.to_string(),
            source: ModSource::Nexus,
            nexus_mod_id: mod_id,
            nexus_file_id: file_id,
            version: version.clone(),
            domain: domain.to_string(),
            staging_path: staging.to_string_lossy().to_string(),
            enabled: provenance.enabled,
            order: next_order,
            ts_namespace: None,
            ts_name: None,
            ts_package_uuid: None,
            modio_game_id: None,
            modio_mod_id: None,
            modio_file_id: None,
            collection_ids: provenance.collection_ids,
            independent: provenance.independent,
            depends_on: provenance.depends_on,
            staged_file_count: Some(count_staged_files(&staging)),
            option_selection: provenance.option_selection,
            fomod_selection: provenance.fomod_selection,
            fomod_pending: provenance.fomod_pending,
            file_name: stored_file_name,
            picture_url: resolve_picture_url(None, &removed),
            supported_game_versions: removed
                .iter()
                .find_map(|old| {
                    (old.nexus_mod_id == mod_id).then(|| old.supported_game_versions.clone())
                })
                .flatten(),
            version_support_generation: removed
                .iter()
                .find_map(|old| {
                    (old.nexus_mod_id == mod_id).then_some(old.version_support_generation)
                })
                .unwrap_or(0),
            plugin_tool_id: None,
        };
        finish_replacement(paths, game_id, order, &removed, staged)
    });
    if committed.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    committed
}

/// Mods staged from one or more imported files or folders.
#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub mods: Vec<StagedMod>,
    pub errors: Vec<String>,
}

/// Stage any mix of archives, mod files, and folders for `game_id`, detecting
/// each path's type automatically.
pub fn import_paths(
    paths: &Paths,
    game_id: &str,
    targets: &[PathBuf],
    domain: &str,
    install_path: Option<&Path>,
) -> Result<ImportResult> {
    if targets.is_empty() {
        bail!("No files or folders selected");
    }

    let mut used_file_ids = load_used_import_file_ids(paths, game_id)?;
    let mut mods = Vec::new();
    let mut errors = Vec::new();

    for target in targets {
        if !target.exists() {
            errors.push(format!("Path not found: {}", target.display()));
            continue;
        }
        if target.is_dir() {
            match import_folder_into(
                paths,
                game_id,
                target,
                domain,
                install_path,
                &mut used_file_ids,
            ) {
                Ok(mut res) => {
                    mods.append(&mut res.mods);
                    errors.append(&mut res.errors);
                }
                Err(err) => {
                    log::warn!("import folder {}: {err:#}", target.display());
                    errors.push(format!("{}: {err}", folder_import_label(target)));
                }
            }
        } else if target.is_file() {
            stage_imported_file(
                paths,
                game_id,
                target,
                domain,
                &mut used_file_ids,
                &mut mods,
                &mut errors,
            );
        } else {
            errors.push(format!("Unsupported path: {}", target.display()));
        }
    }

    Ok(ImportResult { mods, errors })
}

/// Copy an already-extracted mod folder into staging, and stage each immediate
/// `.zip`, `.7z`, or `.rar` as its own mod. The source folder is left in place.
#[cfg(test)]
pub fn import_folder(
    paths: &Paths,
    game_id: &str,
    folder: &Path,
    domain: &str,
    install_path: Option<&Path>,
) -> Result<ImportResult> {
    let mut used_file_ids = load_used_import_file_ids(paths, game_id)?;
    import_folder_into(
        paths,
        game_id,
        folder,
        domain,
        install_path,
        &mut used_file_ids,
    )
}

fn load_used_import_file_ids(paths: &Paths, game_id: &str) -> Result<HashSet<u64>> {
    let order = read_loadorder(paths, game_id)?;
    Ok(order
        .mods
        .iter()
        .filter(|staged| staged.nexus_mod_id == 0)
        .map(|staged| staged.nexus_file_id)
        .collect())
}

fn stage_imported_file(
    paths: &Paths,
    game_id: &str,
    file: &Path,
    domain: &str,
    used_file_ids: &mut HashSet<u64>,
    mods: &mut Vec<StagedMod>,
    errors: &mut Vec<String>,
) {
    let file_id = allocate_import_file_id(used_file_ids);
    let label = archive_import_label(file);
    let imported_name = file
        .file_name()
        .map(|name| name.to_string_lossy().into_owned());
    match stage_mod(
        paths,
        game_id,
        &label,
        domain,
        0,
        file_id,
        None,
        pick_staged_file_name(&label, None, imported_name.as_deref()),
        file,
    ) {
        Ok(staged) => mods.push(staged),
        Err(err) => {
            let name = imported_name.unwrap_or_else(|| file.display().to_string());
            log::warn!("import file {name}: {err:#}");
            errors.push(format!("{name}: {err}"));
        }
    }
}

fn import_folder_into(
    paths: &Paths,
    game_id: &str,
    folder: &Path,
    domain: &str,
    install_path: Option<&Path>,
    used_file_ids: &mut HashSet<u64>,
) -> Result<ImportResult> {
    if !folder.exists() {
        bail!("Folder not found: {}", folder.display());
    }
    if !folder.is_dir() {
        bail!("Not a folder: {}", folder.display());
    }
    if let Some(install) = install_path.filter(|path| !path.as_os_str().is_empty()) {
        if same_import_path(folder, install) {
            bail!("Refusing to import the game install folder");
        }
    }
    let staging = paths.mods_dir(game_id);
    if same_import_path(folder, &staging)
        || import_path_contains(folder, &staging)
        || import_path_contains(&staging, folder)
    {
        bail!("Refusing to import the staging folder");
    }

    let (archives, remainder) = folder_import_entries(folder)?;
    if archives.is_empty() && remainder.is_empty() {
        bail!("Folder is empty");
    }

    let mut mods = Vec::new();
    let mut errors = Vec::new();
    for archive in &archives {
        stage_imported_file(
            paths,
            game_id,
            archive,
            domain,
            used_file_ids,
            &mut mods,
            &mut errors,
        );
    }

    if !remainder.is_empty() {
        let file_id = allocate_import_file_id(used_file_ids);
        let label = folder_import_label(folder);
        match stage_filled_mod(
            paths,
            game_id,
            None,
            &label,
            domain,
            0,
            file_id,
            None,
            None,
            |temp| copy_import_remainder(&remainder, temp),
        ) {
            Ok(staged) => mods.push(staged),
            Err(err) => {
                log::warn!("import folder {label}: {err:#}");
                errors.push(format!("{label}: {err}"));
            }
        }
    }

    Ok(ImportResult { mods, errors })
}

fn folder_import_entries(folder: &Path) -> Result<(Vec<PathBuf>, Vec<PathBuf>)> {
    let mut archives = Vec::new();
    let mut remainder = Vec::new();
    for entry in fs::read_dir(folder).with_context(|| format!("read {}", folder.display()))? {
        let entry = entry?;
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if name.is_empty() || is_hidden_import_name(&name) || is_junk_import_name(&name) {
            continue;
        }
        let path = entry.path();
        if path.is_file() && is_unextracted_archive(&path) {
            archives.push(path);
        } else if path.is_file() || path.is_dir() {
            remainder.push(path);
        }
    }
    archives.sort_by_key(|path| import_sort_key(path));
    remainder.sort_by_key(|path| import_sort_key(path));
    Ok((archives, remainder))
}

fn import_sort_key(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default()
}

fn is_hidden_import_name(name: &str) -> bool {
    name.starts_with('.')
}

fn is_junk_import_name(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "desktop.ini" | "thumbs.db" | ".ds_store"
    )
}

fn is_unextracted_archive(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| matches!(ext.to_ascii_lowercase().as_str(), "zip" | "7z" | "rar"))
}

fn archive_import_label(archive: &Path) -> String {
    archive
        .file_stem()
        .map(|name| name.to_string_lossy().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Imported mod".into())
}

fn folder_import_label(folder: &Path) -> String {
    folder
        .file_name()
        .map(|name| name.to_string_lossy().trim().to_string())
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "Imported mod".into())
}

fn allocate_import_file_id(used: &mut HashSet<u64>) -> u64 {
    let mut id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(1)
        .max(1);
    while used.contains(&id) {
        id = id.checked_add(1).unwrap_or(1);
    }
    used.insert(id);
    id
}

fn normalized_import_path(path: &Path) -> PathBuf {
    if let Ok(canonical) = fs::canonicalize(path) {
        return canonical;
    }
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|cwd| cwd.join(path))
            .unwrap_or_else(|_| path.to_path_buf())
    };
    let mut out = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other.as_os_str()),
        }
    }
    out
}

fn same_import_path(left: &Path, right: &Path) -> bool {
    normalized_import_path(left) == normalized_import_path(right)
}

fn import_path_contains(parent: &Path, child: &Path) -> bool {
    let parent = normalized_import_path(parent);
    let child = normalized_import_path(child);
    child.starts_with(&parent) && child != parent
}

fn copy_import_remainder(entries: &[PathBuf], dest: &Path) -> Result<()> {
    if dest.exists() {
        fs::remove_dir_all(dest)?;
    }
    fs::create_dir_all(dest)?;
    for path in entries {
        let name = path.file_name().context("import entry has no name")?;
        let out = dest.join(name);
        if path.is_dir() {
            copy_dir_all(path, &out)?;
        } else if path.is_file() {
            fs::copy(path, &out)
                .with_context(|| format!("copy {} to {}", path.display(), out.display()))?;
        }
    }
    Ok(())
}

/// Record a file label on a staged row that does not have one yet.
pub fn remember_file_name_if_missing(
    paths: &Paths,
    game_id: &str,
    staged_id: &str,
    file_name: &str,
) -> Result<bool> {
    let label = file_name.trim();
    if label.is_empty() {
        return Ok(false);
    }
    mutate_load_order(paths, game_id, |order| {
        let Some(row) = order.mods.iter_mut().find(|m| m.id == staged_id) else {
            return Ok(false);
        };
        if row
            .file_name
            .as_deref()
            .map(|s| !s.trim().is_empty())
            .unwrap_or(false)
        {
            return Ok(false);
        }
        row.file_name = Some(label.to_string());
        Ok(true)
    })
}

/// Files currently present in a staging folder.
pub fn count_staged_files(dir: &Path) -> usize {
    WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
        .count()
}

/// Staged mods that lost files after extraction. Older builds symlinked staging
/// into the game folder, so a loader deleting its own plugin directory deleted
/// the staged originals too; those packages must be downloaded again.
pub fn damaged_staging(order: &LoadOrder) -> Vec<&StagedMod> {
    order
        .mods
        .iter()
        .filter(|m| {
            let Some(expected) = m.staged_file_count else {
                return false;
            };
            let staging = Path::new(&m.staging_path);
            staging.is_dir() && count_staged_files(staging) < expected
        })
        .collect()
}

#[allow(dead_code, clippy::too_many_arguments)]
pub fn stage_thunderstore_mod(
    paths: &Paths,
    game_id: &str,
    community: &str,
    namespace: &str,
    name: &str,
    version: &str,
    package_uuid: Option<String>,
    display_name: &str,
    file_name: Option<String>,
    picture_url: Option<String>,
    archive: &Path,
) -> Result<StagedMod> {
    stage_thunderstore_mod_with_mode(
        paths,
        game_id,
        community,
        namespace,
        name,
        version,
        package_uuid,
        display_name,
        file_name,
        picture_url,
        archive,
        true,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn stage_thunderstore_mod_with_mode(
    paths: &Paths,
    game_id: &str,
    community: &str,
    namespace: &str,
    name: &str,
    version: &str,
    package_uuid: Option<String>,
    display_name: &str,
    file_name: Option<String>,
    picture_url: Option<String>,
    archive: &Path,
    replace_existing: bool,
    replace_id: Option<&str>,
) -> Result<StagedMod> {
    crate::config::ensure_game_dirs(paths, game_id)?;
    let safe = sanitize_filename::sanitize(display_name);
    let ns_safe = sanitize_filename::sanitize(namespace);
    let name_safe = sanitize_filename::sanitize(name);
    let ver_safe = sanitize_filename::sanitize(version);
    let staging = paths
        .mods_dir(game_id)
        .join(format!("{safe}_{ns_safe}_{name_safe}_{ver_safe}"));
    let temp = temp_extract_dir(&staging);
    if let Err(e) = extract_archive(archive, &temp) {
        let _ = fs::remove_dir_all(&temp);
        return Err(e);
    }

    let base_id = thunderstore_mod_id(namespace, name);
    let versioned_id = thunderstore_mod_versioned_id(namespace, name, version);
    let committed = mutate_load_order(paths, game_id, |order| {
        swap_extracted_dir(&temp, &staging)?;
        let (id, removed) = if replace_existing {
            let mut removed = Vec::new();
            if let Some(rid) = replace_id {
                if let Some(row) = take_row_by_id(order, rid) {
                    removed.push(row);
                }
            }
            removed.extend(take_rows(order, |m| m.id == base_id || m.id == versioned_id));
            if removed.is_empty() {
                removed.extend(take_rows(order, |m| {
                    m.source == ModSource::Thunderstore
                        && m.ts_namespace
                            .as_deref()
                            .is_some_and(|ns| ns.eq_ignore_ascii_case(namespace))
                        && m.ts_name
                            .as_deref()
                            .is_some_and(|n| n.eq_ignore_ascii_case(name))
                }));
            }
            (base_id.clone(), removed)
        } else {
            let other_exists = order.mods.iter().any(|m| {
                m.source == ModSource::Thunderstore
                    && m.ts_namespace
                        .as_deref()
                        .is_some_and(|ns| ns.eq_ignore_ascii_case(namespace))
                    && m.ts_name
                        .as_deref()
                        .is_some_and(|n| n.eq_ignore_ascii_case(name))
                    && !m
                        .version
                        .as_deref()
                        .map(str::trim)
                        .is_some_and(|v| v.eq_ignore_ascii_case(version.trim()))
            });
            let chosen_id = if other_exists {
                versioned_id.clone()
            } else {
                base_id.clone()
            };
            let removed = take_rows(order, |m| {
                m.id == chosen_id
                    || (m.source == ModSource::Thunderstore
                        && m.ts_namespace
                            .as_deref()
                            .is_some_and(|ns| ns.eq_ignore_ascii_case(namespace))
                        && m.ts_name
                            .as_deref()
                            .is_some_and(|n| n.eq_ignore_ascii_case(name))
                        && m.version
                            .as_deref()
                            .map(str::trim)
                            .is_some_and(|v| v.eq_ignore_ascii_case(version.trim())))
            });
            (chosen_id, removed)
        };
        let same_version = removed
            .first()
            .is_some_and(|old| old.version.as_deref() == Some(version));
        let old_fomod_module = (!same_version).then(|| {
            removed.first().and_then(|old| {
                (Path::new(&old.staging_path) != staging.as_path())
                    .then(|| fomod::load_module_in_staging(Path::new(&old.staging_path)).ok().flatten())
                    .flatten()
            })
        }).flatten();
        let provenance = provenance_from_removed(
            removed.first().cloned(),
            old_fomod_module.as_ref(),
            &staging,
            same_version,
        );
        let next_order = provenance
            .order
            .unwrap_or_else(|| order.mods.iter().map(|m| m.order).max().unwrap_or(0) + 1);
        let staged = StagedMod {
            id: id.clone(),
            name: display_name.to_string(),
            source: ModSource::Thunderstore,
            nexus_mod_id: 0,
            nexus_file_id: 0,
            version: Some(version.to_string()),
            domain: community.to_string(),
            staging_path: staging.to_string_lossy().to_string(),
            enabled: provenance.enabled,
            order: next_order,
            ts_namespace: Some(namespace.to_string()),
            ts_name: Some(name.to_string()),
            ts_package_uuid: package_uuid.clone(),
            modio_game_id: None,
            modio_mod_id: None,
            modio_file_id: None,
            collection_ids: provenance.collection_ids,
            independent: provenance.independent,
            depends_on: provenance.depends_on,
            staged_file_count: Some(count_staged_files(&staging)),
            option_selection: provenance.option_selection,
            fomod_selection: provenance.fomod_selection,
            fomod_pending: provenance.fomod_pending,
            file_name,
            picture_url: resolve_picture_url(picture_url, &removed),
            supported_game_versions: None,
            version_support_generation: 0,
            plugin_tool_id: None,
        };
        finish_replacement(paths, game_id, order, &removed, staged)
    });
    if committed.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    committed
}

pub fn stage_modio_mod(
    paths: &Paths,
    game_id: &str,
    modio_game_id: u32,
    modio_mod_id: u64,
    modio_file_id: u64,
    name: &str,
    version: Option<String>,
    file_name: Option<String>,
    picture_url: Option<String>,
    archive: &Path,
) -> Result<StagedMod> {
    stage_modio_mod_with_mode(
        paths,
        game_id,
        modio_game_id,
        modio_mod_id,
        modio_file_id,
        name,
        version,
        file_name,
        picture_url,
        archive,
        true,
        None,
    )
}

#[allow(clippy::too_many_arguments)]
pub fn stage_modio_mod_with_mode(
    paths: &Paths,
    game_id: &str,
    modio_game_id: u32,
    modio_mod_id: u64,
    modio_file_id: u64,
    name: &str,
    version: Option<String>,
    file_name: Option<String>,
    picture_url: Option<String>,
    archive: &Path,
    replace_existing: bool,
    replace_id: Option<&str>,
) -> Result<StagedMod> {
    crate::config::ensure_game_dirs(paths, game_id)?;
    let safe = sanitize_filename::sanitize(name);
    let staging = paths.mods_dir(game_id).join(format!(
        "{safe}_{modio_game_id}_{modio_mod_id}_{modio_file_id}"
    ));
    let temp = temp_extract_dir(&staging);
    if let Err(e) = extract_archive(archive, &temp) {
        let _ = fs::remove_dir_all(&temp);
        return Err(e);
    }

    let base_id = modio_identity_id(modio_game_id, modio_mod_id);
    let file_id_str = modio_file_identity_id(modio_game_id, modio_mod_id, modio_file_id);
    let committed = mutate_load_order(paths, game_id, |order| {
        swap_extracted_dir(&temp, &staging)?;
        let (id, removed) = if replace_existing {
            let mut removed = Vec::new();
            if let Some(rid) = replace_id {
                if let Some(row) = take_row_by_id(order, rid) {
                    removed.push(row);
                }
            }
            removed.extend(take_rows(order, |m| m.id == base_id || m.id == file_id_str));
            removed.extend(take_rows(order, |m| {
                m.source == ModSource::Modio
                    && m.modio_game_id == Some(modio_game_id)
                    && m.modio_mod_id == Some(modio_mod_id)
            }));
            (base_id.clone(), removed)
        } else {
            let other_exists = order.mods.iter().any(|m| {
                m.source == ModSource::Modio
                    && m.modio_game_id == Some(modio_game_id)
                    && m.modio_mod_id == Some(modio_mod_id)
                    && m.modio_file_id != Some(modio_file_id)
            });
            let chosen_id = if other_exists {
                file_id_str.clone()
            } else {
                base_id.clone()
            };
            let removed = take_rows(order, |m| {
                m.id == chosen_id
                    || (m.source == ModSource::Modio
                        && m.modio_game_id == Some(modio_game_id)
                        && m.modio_mod_id == Some(modio_mod_id)
                        && m.modio_file_id == Some(modio_file_id))
            });
            (chosen_id, removed)
        };
        let same_file = removed
            .first()
            .is_some_and(|old| old.modio_file_id == Some(modio_file_id));
        let old_fomod_module = (!same_file).then(|| {
            removed.first().and_then(|old| {
                (Path::new(&old.staging_path) != staging.as_path())
                    .then(|| fomod::load_module_in_staging(Path::new(&old.staging_path)).ok().flatten())
                    .flatten()
            })
        }).flatten();
        let provenance = provenance_from_removed(
            removed.first().cloned(),
            old_fomod_module.as_ref(),
            &staging,
            same_file,
        );
        let next_order = provenance
            .order
            .unwrap_or_else(|| order.mods.iter().map(|m| m.order).max().unwrap_or(0) + 1);
        let staged = StagedMod {
            id: id.clone(),
            name: name.to_string(),
            source: ModSource::Modio,
            nexus_mod_id: 0,
            nexus_file_id: 0,
            version: version.clone(),
            domain: format!("modio:{modio_game_id}"),
            staging_path: staging.to_string_lossy().to_string(),
            enabled: provenance.enabled,
            order: next_order,
            ts_namespace: None,
            ts_name: None,
            ts_package_uuid: None,
            modio_game_id: Some(modio_game_id),
            modio_mod_id: Some(modio_mod_id),
            modio_file_id: Some(modio_file_id),
            collection_ids: provenance.collection_ids,
            independent: provenance.independent,
            depends_on: provenance.depends_on,
            staged_file_count: Some(count_staged_files(&staging)),
            option_selection: provenance.option_selection,
            fomod_selection: provenance.fomod_selection,
            fomod_pending: provenance.fomod_pending,
            file_name,
            picture_url: resolve_picture_url(picture_url, &removed),
            supported_game_versions: None,
            version_support_generation: 0,
            plugin_tool_id: None,
        };
        finish_replacement(paths, game_id, order, &removed, staged)
    });
    if committed.is_err() {
        let _ = fs::remove_dir_all(&temp);
    }
    committed
}

/// Record posters for rows that do not have one yet. An empty string means the
/// catalog was checked and the mod has no image.
pub fn apply_staged_pictures(
    paths: &Paths,
    game_id: &str,
    pictures: &HashMap<String, String>,
) -> Result<Vec<StagedMod>> {
    mutate_load_order(paths, game_id, |order| {
        for staged in &mut order.mods {
            if staged.picture_url.is_some() {
                continue;
            }
            if let Some(url) = pictures.get(&staged.id) {
                staged.picture_url = Some(url.clone());
            }
        }
        let mut mods = order.mods.clone();
        mods.sort_by_key(|m| m.order);
        Ok(mods)
    })
}

/// True when a Thunderstore package (any version) is already staged.
#[allow(dead_code)]
pub fn has_thunderstore_package(
    paths: &Paths,
    game_id: &str,
    namespace: &str,
    name: &str,
) -> Result<bool> {
    let order = load_loadorder(paths, game_id)?;
    Ok(order.mods.iter().any(|m| {
        m.source == ModSource::Thunderstore
            && m.ts_namespace.as_deref() == Some(namespace)
            && m.ts_name.as_deref() == Some(name)
    }))
}

#[allow(dead_code)]
pub fn has_modio_mod(paths: &Paths, game_id: &str, modio_mod_id: u64) -> Result<bool> {
    let order = load_loadorder(paths, game_id)?;
    Ok(order
        .mods
        .iter()
        .any(|m| m.source == ModSource::Modio && m.modio_mod_id == Some(modio_mod_id)))
}

pub fn set_enabled(paths: &Paths, game_id: &str, mod_uid: &str, enabled: bool) -> Result<()> {
    mutate_load_order(paths, game_id, |order| {
        let Some(m) = order.mods.iter_mut().find(|m| m.id == mod_uid) else {
            bail!("mod not found: {mod_uid}");
        };
        m.enabled = enabled;
        Ok(())
    })
}

/// Enable or disable all staged mods belonging to `collection_id`.
/// When disabling, mods listed in `protect_mod_ids` or shared with another
/// collection that still has enabled mods are left enabled.
pub fn set_collection_enabled(
    paths: &Paths,
    game_id: &str,
    collection_id: &str,
    enabled: bool,
    protect_mod_ids: &[String],
) -> Result<usize> {
    let mut collections = load_collections(paths, game_id)?;
    let Some(target_idx) = collections
        .collections
        .iter()
        .position(|c| c.id == collection_id)
    else {
        bail!("collection not found: {collection_id}");
    };
    let target_member_ids: HashSet<String> = collections.collections[target_idx]
        .mod_ids
        .iter()
        .cloned()
        .collect();
    let protected: HashSet<&str> = protect_mod_ids.iter().map(|s| s.as_str()).collect();

    let changed = mutate_load_order(paths, game_id, |order| {
        let mut active_other_mod_ids: HashSet<String> = HashSet::new();
        if !enabled {
            let enabled_ids: HashSet<&str> = order
                .mods
                .iter()
                .filter(|m| m.enabled)
                .map(|m| m.id.as_str())
                .collect();
            for other in &collections.collections {
                if other.id == collection_id || other.disabled {
                    continue;
                }
                let mut other_member_ids: HashSet<String> =
                    other.mod_ids.iter().cloned().collect();
                for m in &order.mods {
                    if m.collection_ids.iter().any(|cid| cid == &other.id) {
                        other_member_ids.insert(m.id.clone());
                    }
                }
                let other_has_enabled = other_member_ids
                    .iter()
                    .any(|id| enabled_ids.contains(id.as_str()));
                if other_has_enabled {
                    active_other_mod_ids.extend(other_member_ids);
                }
            }
        }

        let mut changed = 0usize;
        for m in &mut order.mods {
            let belongs = target_member_ids.contains(&m.id)
                || m.collection_ids.iter().any(|cid| cid == collection_id);
            if !belongs {
                continue;
            }
            if enabled {
                if !m.enabled {
                    m.enabled = true;
                    changed += 1;
                }
            } else {
                if protected.contains(m.id.as_str()) || active_other_mod_ids.contains(&m.id) {
                    continue;
                }
                if m.enabled {
                    m.enabled = false;
                    changed += 1;
                }
            }
        }
        Ok(changed)
    })?;

    collections.collections[target_idx].disabled = !enabled;
    save_collections(paths, game_id, &collections)?;
    Ok(changed)
}

pub fn set_mod_options(
    paths: &Paths,
    game_id: &str,
    mod_uid: &str,
    selection: ModOptionSelection,
) -> Result<()> {
    mutate_load_order(paths, game_id, |order| {
        let Some(m) = order.mods.iter_mut().find(|m| m.id == mod_uid) else {
            bail!("mod not found: {mod_uid}");
        };
        m.option_selection = Some(selection);
        Ok(())
    })
}

pub fn set_fomod_selection(
    paths: &Paths,
    game_id: &str,
    mod_uid: &str,
    selection: FomodSelection,
) -> Result<()> {
    mutate_load_order(paths, game_id, |order| {
        let Some(m) = order.mods.iter_mut().find(|m| m.id == mod_uid) else {
            bail!("mod not found: {mod_uid}");
        };
        m.fomod_selection = Some(selection);
        m.fomod_pending = None;
        Ok(())
    })
}

pub fn set_load_order(paths: &Paths, game_id: &str, ordered_ids: &[String]) -> Result<()> {
    mutate_load_order(paths, game_id, |order| {
        let mut by_id: HashMap<String, StagedMod> =
            order.mods.drain(..).map(|m| (m.id.clone(), m)).collect();
        let mut new_mods = Vec::new();
        for (i, id) in ordered_ids.iter().enumerate() {
            if let Some(mut m) = by_id.remove(id) {
                m.order = (i as u32) + 1;
                new_mods.push(m);
            }
        }
        for (_, mut m) in by_id {
            m.order = (new_mods.len() as u32) + 1;
            new_mods.push(m);
        }
        order.mods = new_mods;
        Ok(())
    })
}

pub struct SyncedModSelection {
    pub mod_id: String,
    pub fomod_selection: Option<FomodSelection>,
    pub option_selection: Option<ModOptionSelection>,
    pub enabled: Option<bool>,
}

/// Reorders the specified collection mods to match `selections` in relative order
/// (preserving unrelated mods' positions) and applies FOMOD / option selections.
pub fn apply_collection_order_and_selections(
    paths: &Paths,
    game_id: &str,
    selections: &[SyncedModSelection],
) -> Result<()> {
    mutate_load_order(paths, game_id, |order| {
        let selection_map: HashMap<&str, &SyncedModSelection> = selections
            .iter()
            .map(|s| (s.mod_id.as_str(), s))
            .collect();
        let member_set: HashSet<&str> = selection_map.keys().copied().collect();

        let mut by_id: HashMap<String, StagedMod> = HashMap::new();
        let mut slots: Vec<Option<StagedMod>> = Vec::with_capacity(order.mods.len());
        let mut first_slot: Option<usize> = None;

        for mut m in order.mods.drain(..) {
            if member_set.contains(m.id.as_str()) {
                if first_slot.is_none() {
                    first_slot = Some(slots.len());
                }
                if let Some(sel) = selection_map.get(m.id.as_str()) {
                    if let Some(fomod) = &sel.fomod_selection {
                        m.fomod_selection = Some(fomod.clone());
                        m.fomod_pending = None;
                    }
                    if let Some(opts) = &sel.option_selection {
                        m.option_selection = Some(opts.clone());
                    }
                    m.enabled = sel.enabled.unwrap_or(true);
                }
                by_id.insert(m.id.clone(), m);
                slots.push(None);
            } else {
                slots.push(Some(m));
            }
        }

        let mut ordered_members: Vec<StagedMod> = Vec::with_capacity(selections.len());
        for sel in selections {
            if let Some(m) = by_id.remove(&sel.mod_id) {
                ordered_members.push(m);
            }
        }
        for (_, m) in by_id {
            ordered_members.push(m);
        }

        let mut member_iter = ordered_members.into_iter();
        let mut new_mods = Vec::with_capacity(slots.len() + member_iter.len());
        let insert_at = first_slot.unwrap_or(slots.len());
        for (idx, slot) in slots.into_iter().enumerate() {
            if idx == insert_at {
                // Fill the first member slot; subsequent None slots will be skipped
                // and remaining members are placed at the first slot to keep them contiguous
                // in exact friend load order.
                for m in member_iter.by_ref() {
                    new_mods.push(m);
                }
            }
            if let Some(m) = slot {
                new_mods.push(m);
            }
        }
        for m in member_iter {
            new_mods.push(m);
        }
        for (i, m) in new_mods.iter_mut().enumerate() {
            m.order = (i as u32) + 1;
        }
        order.mods = new_mods;
        Ok(())
    })
}

pub fn set_friend_collection_auto_sync(
    paths: &Paths,
    game_id: &str,
    collection_id: &str,
    auto_sync: bool,
) -> Result<()> {
    mutate_collections(paths, game_id, |store| {
        let col = store
            .collections
            .iter_mut()
            .find(|c| c.id == collection_id)
            .ok_or_else(|| anyhow::anyhow!("Collection not found: {collection_id}"))?;
        let Some(sync) = col.friend_sync.as_mut() else {
            bail!("Collection is not linked to a friend");
        };
        sync.auto_sync = auto_sync;
        Ok(())
    })
}

pub fn remove_mod(paths: &Paths, game_id: &str, mod_uid: &str) -> Result<()> {
    remove_mod_returning(paths, game_id, mod_uid).map(|_| ())
}

pub fn remove_mod_returning(
    paths: &Paths,
    game_id: &str,
    mod_uid: &str,
) -> Result<Option<StagedMod>> {
    with_game_order(game_id, || {
        let mut order = read_loadorder(paths, game_id)?;
        collapse_load_order(paths, game_id, &mut order)?;
        if let Some(pos) = order.mods.iter().position(|m| m.id == mod_uid) {
            let m = order.mods.remove(pos);
            delete_staged_row(&m)?;
            write_loadorder(paths, game_id, &order)?;
            mutate_collections(paths, game_id, |store| {
                for c in &mut store.collections {
                    c.mod_ids.retain(|id| id != mod_uid);
                }
                Ok(())
            })?;
            return Ok(Some(m));
        }
        Ok(None)
    })
}

pub fn remove_all_mods(paths: &Paths, game_id: &str, install_path: Option<&Path>) -> Result<()> {
    purge_deploy(paths, game_id, install_path)?;
    with_game_order(game_id, || {
        let order = read_loadorder(paths, game_id)?;
        for m in &order.mods {
            delete_staged_row(m)?;
        }
        write_loadorder(paths, game_id, &LoadOrder::default())?;
        save_collections(paths, game_id, &InstalledCollections::default())?;
        Ok(())
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LinkKind {
    Hardlinked,
    Copied,
}

/// Remove a deploy destination without following symlinks.
fn remove_deploy_path(path: &Path) -> Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let meta = fs::symlink_metadata(path)?;
    if meta.file_type().is_symlink() || meta.is_file() {
        fs::remove_file(path)?;
    } else if meta.is_dir() {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}

/// Ensure `dest` can be created as a directory (remove blocking files/symlinks).
fn ensure_deploy_dir(dest: &Path) -> Result<()> {
    if dest.exists() {
        let meta = fs::symlink_metadata(dest)?;
        if meta.file_type().is_symlink() || meta.is_file() {
            fs::remove_file(dest)?;
        }
    }
    fs::create_dir_all(dest)?;
    Ok(())
}

fn ensure_deploy_parent(dest: &Path) -> Result<()> {
    let Some(parent) = dest.parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    if parent.exists() {
        let meta = fs::symlink_metadata(parent)?;
        if meta.file_type().is_symlink() || meta.is_file() {
            fs::remove_file(parent)?;
        }
    }
    Ok(())
}

#[cfg(unix)]
fn symlink_points_into(path: &Path, root: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return false;
    }
    let Ok(target) = fs::read_link(path) else {
        return false;
    };
    let resolved = if target.is_absolute() {
        target
    } else {
        path.parent().map(|p| p.join(&target)).unwrap_or(target)
    };
    resolved.starts_with(root)
}

#[cfg(windows)]
fn symlink_points_into(path: &Path, root: &Path) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    if !meta.file_type().is_symlink() {
        return false;
    }
    let Ok(target) = fs::read_link(path) else {
        return false;
    };
    let resolved = if target.is_absolute() {
        target
    } else {
        path.parent().map(|p| p.join(&target)).unwrap_or(target)
    };
    resolved.starts_with(root)
}

#[cfg(not(any(unix, windows)))]
fn symlink_points_into(_path: &Path, _root: &Path) -> bool {
    false
}

fn prune_empty_dirs_up(install_path: &Path, mut dir: PathBuf) {
    while dir.starts_with(install_path) && dir != install_path {
        let is_empty = fs::read_dir(&dir)
            .map(|mut entries| entries.next().is_none())
            .unwrap_or(false);
        if is_empty {
            let _ = fs::remove_dir(&dir);
            if !dir.pop() {
                break;
            }
        } else {
            break;
        }
    }
}

/// Remove deployed symlinks under the game folder that point into app data.
/// Covers failed deploys where `deployed.json` was never written.
pub fn purge_install_symlinks(install_path: &Path, data_dir: &Path) -> Result<usize> {
    if !install_path.is_dir() {
        return Ok(0);
    }
    let mut symlinks: Vec<PathBuf> = Vec::new();
    for entry in WalkDir::new(install_path)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if symlink_points_into(path, data_dir) {
            symlinks.push(path.to_path_buf());
        }
    }
    symlinks.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
    let mut removed = 0usize;
    for path in symlinks {
        if fs::symlink_metadata(&path)
            .map(|m| m.file_type().is_symlink())
            .unwrap_or(false)
        {
            if let Some(parent) = path.parent() {
                fs::remove_file(&path)?;
                prune_empty_dirs_up(install_path, parent.to_path_buf());
                removed += 1;
            }
        }
    }
    Ok(removed)
}

/// Remove empty leftover directories under loader-owned folders. Mods that were
/// purged or removed leave their (now empty) folders behind, which pile up in
/// `BepInEx/plugins` and make an install look modded when it is not.
pub fn prune_empty_loader_dirs(plugin: &dyn GamePlugin, install_path: &Path) -> usize {
    let mut removed = 0usize;
    for root in plugin.prunable_dirs(install_path) {
        let mut dirs: Vec<PathBuf> = WalkDir::new(&root)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_dir() && e.path() != root)
            .map(|e| e.path().to_path_buf())
            .collect();
        // Deepest first so nested leftovers collapse in one pass.
        dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));
        for dir in dirs {
            let is_empty = fs::read_dir(&dir)
                .map(|mut entries| entries.next().is_none())
                .unwrap_or(false);
            if is_empty && fs::remove_dir(&dir).is_ok() {
                removed += 1;
            }
        }
    }
    removed
}

/// Manifest paths that no longer exist, i.e. files a game or loader deleted
/// after deploy. Returned paths are relative to the install when possible.
fn missing_deployed_paths(deployed: &DeployManifest, install_path: &Path) -> Vec<String> {
    deployed
        .paths
        .iter()
        .filter(|p| !Path::new(p).exists())
        .map(|p| {
            Path::new(p)
                .strip_prefix(install_path)
                .unwrap_or(Path::new(p))
                .to_string_lossy()
                .to_string()
        })
        .collect()
}

fn link_or_copy(src: &Path, dest: &Path) -> Result<LinkKind> {
    ensure_deploy_parent(dest)?;
    if dest.exists() {
        remove_deploy_path(dest)?;
    }
    if let Some(parent) = dest.parent() {
        ensure_deploy_dir(parent)?;
    }
    // Hardlink files; directories are created normally and children linked.
    if src.is_dir() {
        ensure_deploy_dir(dest)?;
        return Ok(LinkKind::Hardlinked);
    }
    // Never symlink into a game folder: a mod loader that deletes its own
    // directory (BepInEx prunes BepInEx/plugins/RoR2BepInExPack on startup)
    // follows the link and destroys the staged original.
    match fs::hard_link(src, dest) {
        Ok(()) => Ok(LinkKind::Hardlinked),
        Err(e) => {
            log::debug!(
                "hardlink failed for {} -> {} ({e}); copying instead",
                src.display(),
                dest.display()
            );
            fs::copy(src, dest)?;
            Ok(LinkKind::Copied)
        }
    }
}

pub fn has_active_deployment(paths: &Paths, game_id: &str) -> bool {
    let manifest_path = paths.deploy_manifest(game_id);
    fs::read_to_string(&manifest_path)
        .ok()
        .and_then(|raw| serde_json::from_str::<DeployManifest>(&raw).ok())
        .is_some_and(|m| !m.paths.is_empty())
}

pub fn purge_deploy(paths: &Paths, game_id: &str, install_path: Option<&Path>) -> Result<()> {
    let manifest_path = paths.deploy_manifest(game_id);
    if manifest_path.exists() {
        let raw = fs::read_to_string(&manifest_path)?;
        let manifest: DeployManifest = serde_json::from_str(&raw)?;
        // Remove files first, then empty dirs (reverse sort by path length)
        let mut entries = manifest.paths;
        entries.sort_by_key(|p| std::cmp::Reverse(p.len()));
        for p in entries {
            let path = PathBuf::from(&p);
            if path.is_symlink() || path.is_file() {
                let _ = fs::remove_file(&path);
            } else if path.is_dir() {
                let _ = fs::remove_dir(&path); // only if empty
            }
        }
        fs::write(
            &manifest_path,
            serde_json::to_string_pretty(&DeployManifest::default())?,
        )?;
    }
    if let Some(install) = install_path {
        purge_install_symlinks(install, &paths.data_dir)?;
    }
    Ok(())
}

pub fn deploy(
    paths: &Paths,
    game_id: &str,
    plugin_id: &str,
    install_path: &Path,
    project_name: Option<&str>,
) -> Result<DeployResult> {
    let plugin = plugin_by_id(plugin_id).context("unknown game plugin")?;
    let mut warnings = Vec::new();
    let base_ctx = DeployContext {
        project_name,
        content_root: None,
        source_dir: None,
    };

    if !install_path.is_dir() {
        bail!(
            "Game install path does not exist or is not a directory: {}",
            install_path.display()
        );
    }

    warnings.extend(plugin.prepare_deploy(install_path)?);

    purge_deploy(paths, game_id, Some(install_path))?;
    prune_empty_loader_dirs(plugin, install_path);

    let mut order = load_loadorder(paths, game_id)?;
    order.mods.sort_by_key(|m| m.order);

    let mut deployed = DeployManifest::default();
    let mut count = 0usize;
    let mut copied_files = 0usize;
    let mut skipped_files = 0usize;
    let enabled: Vec<_> = order.mods.iter().filter(|m| m.enabled).cloned().collect();
    let enabled_mods = enabled.len();
    let mut enabled_mod_folders = Vec::new();

    for staged in &enabled {
        let staging = PathBuf::from(&staged.staging_path);
        if !staging.exists() {
            let msg = format!("Missing staging for {}: {}", staged.name, staging.display());
            log::warn!("{msg}");
            warnings.push(msg);
            continue;
        }
        if let Some(expected) = staged.staged_file_count {
            let actual = count_staged_files(&staging);
            if actual < expected {
                warnings.push(format!(
                    "{} is missing {} of {expected} staged file(s); download it again before deploying.",
                    staged.name,
                    expected - actual
                ));
            }
        }
        if let Err(e) = repair_backslash_entries(&staging) {
            warnings.push(format!(
                "Could not normalize Windows-style paths for {}: {e:#}",
                staged.name
            ));
        }
        let root = normalize_staging_root(&staging, plugin)?;
        warnings.extend(plugin.staging_deploy_warnings(&root, &staged.name));
        warnings.extend(plugin.deploy_staged_extra(install_path, &root, &staged.name)?);
        let fomod_module = match fomod::load_module(&root) {
            Ok(m) => m,
            Err(e) => {
                warnings.push(format!(
                    "Could not read FOMOD installer for {}: {e:#}",
                    staged.name
                ));
                None
            }
        };
        if fomod_module
            .as_ref()
            .is_some_and(|m| m.unsupported_scripted)
        {
            warnings.push(format!(
                "{} uses a scripted FOMOD installer, which Emperor does not run yet.",
                staged.name
            ));
        }
        let fomod_map = fomod_module.as_ref().and_then(|module| {
            let selection = fomod::effective_selection(module, staged.fomod_selection.as_ref());
            match fomod::build_file_map(&root, module, &selection) {
                Ok(map) => Some(map),
                Err(e) => {
                    warnings.push(format!(
                        "Could not resolve FOMOD files for {}: {e:#}",
                        staged.name
                    ));
                    None
                }
            }
        });
        let filter = if fomod_map.is_some() {
            None
        } else {
            plugin
                .mod_options(&root)
                .map(|set| IncludeFilter::build(&set, staged.option_selection.as_ref()))
                .filter(|f| !f.is_passthrough())
        };
        let to_root = plugin.deploys_to_install_root(&root);
        enabled_mod_folders.extend(plugin.load_order_entries(&root, &staged.name));
        let deploy_root = if to_root {
            bepinex_pack_deploy_root(&root)
        } else {
            root.clone()
        };
        let stats = if let Some(map) = fomod_map.as_ref() {
            deploy_fomod_map(
                plugin,
                install_path,
                &deploy_root,
                &staged.name,
                project_name,
                map,
                &mut deployed,
            )?
        } else {
            deploy_tree(
                plugin,
                install_path,
                &deploy_root,
                &staged.name,
                project_name,
                filter.as_ref(),
                &mut deployed,
            )?
        };
        if stats.linked == 0 && !plugin.allows_empty_deploy(&root) {
            let msg = if fomod_map.is_some() || filter.is_some() {
                format!(
                    "No files deployed from {}; check its installer options.",
                    staged.name
                )
            } else {
                format!("No files deployed from {}", staged.name)
            };
            warnings.push(msg);
        }
        copied_files += stats.copied;
        skipped_files += stats.skipped;
        count += stats.linked;
    }

    if copied_files > 0 {
        warnings.push(format!(
            "{copied_files} file(s) were copied instead of hardlinked because staging and the game are on different drives. Keep them on one drive to save space."
        ));
    }

    if skipped_files > 0 {
        warnings.push(format!(
            "{skipped_files} file(s) were skipped because this game does not load them (documentation, previews, or loose archives)."
        ));
    }

    if enabled_mods == 0 {
        warnings.push("No enabled mods to deploy.".into());
    } else if count == 0 {
        warnings.push("Deploy finished with 0 files linked.".into());
    }

    warnings.extend(plugin.after_deploy(install_path, &enabled_mod_folders)?);
    if crate::games::loot::supports_loot(plugin.info().id) {
        if let Ok(sync_warnings) = crate::games::loot::sync_plugins_txt_from_saved_order(
            paths,
            game_id,
            plugin.info().id,
            install_path,
        ) {
            for w in sync_warnings {
                if !warnings.contains(&w) {
                    warnings.push(w);
                }
            }
        }
    }
    let plugin_skips = plugin.plugin_skips(install_path);

    // Warn about the tree we just produced, not the one we started from: a pack
    // that failed to land leaves an install that cannot load anything, and a
    // pre-purge check would report problems the deploy has already fixed.
    warnings.extend(plugin.preflight_warnings_ctx(install_path, &base_ctx));
    let missing = missing_deployed_paths(&deployed, install_path);
    if !missing.is_empty() {
        warnings.push(format!(
            "{} deployed file(s) are already missing from the game folder, starting with {}. Purge and deploy again.",
            missing.len(),
            missing[0]
        ));
    }

    crate::config::ensure_game_dirs(paths, game_id)?;
    let manifest_json = serde_json::to_string_pretty(&deployed)?;
    fs::write(paths.deploy_manifest(game_id), &manifest_json)?;
    let _ = fs::write(paths.last_launch_manifest(game_id), &manifest_json);
    Ok(DeployResult {
        file_count: count,
        enabled_mods,
        warnings,
        plugin_skips,
        missing_requirements: crate::nexus::MissingRequirements::default(),
    })
}

#[derive(Debug, Default, Clone, Copy)]
struct TreeStats {
    linked: usize,
    copied: usize,
    skipped: usize,
}

fn deploy_fomod_map(
    plugin: &dyn GamePlugin,
    install_path: &Path,
    content_root: &Path,
    mod_name: &str,
    project_name: Option<&str>,
    map: &[fomod::FomodMappedFile],
    deployed: &mut DeployManifest,
) -> Result<TreeStats> {
    let mut stats = TreeStats::default();
    let to_root = plugin.deploys_to_install_root(content_root);
    let wrap =
        !to_root && (plugin.prefers_mod_folder() || plugin.should_wrap_as_mod_folder(content_root));
    let folder_name = plugin.wrap_mod_folder_name(content_root, mod_name);

    for entry in map {
        if !entry.source.is_file() {
            continue;
        }
        let rel = normalize_relative(&entry.dest_rel);
        let ctx = DeployContext {
            project_name,
            content_root: Some(content_root),
            source_dir: rel.parent(),
        };
        if !plugin.should_deploy_file_ctx(&rel, &ctx) {
            if !plugin.allows_empty_deploy(content_root) {
                stats.skipped += 1;
            }
            continue;
        }
        let deploy_rel: PathBuf = if wrap {
            Path::new(&folder_name).join(&rel)
        } else {
            rel.clone()
        };
        let dest = if to_root {
            rewrite_ror2_bepinexpack_dest(&install_path.join(&deploy_rel))
        } else {
            plugin.resolve_deploy_root_ctx(install_path, &deploy_rel, &ctx)?
        };
        if let Some(parent) = dest.parent() {
            ensure_deploy_dir(parent)?;
        }
        if link_or_copy(&entry.source, &dest)? == LinkKind::Copied {
            stats.copied += 1;
        }
        deployed.paths.push(dest.to_string_lossy().to_string());
        stats.linked += 1;
    }
    Ok(stats)
}

fn deploy_tree(
    plugin: &dyn GamePlugin,
    install_path: &Path,
    content_root: &Path,
    mod_name: &str,
    project_name: Option<&str>,
    filter: Option<&IncludeFilter>,
    deployed: &mut DeployManifest,
) -> Result<TreeStats> {
    let mut stats = TreeStats::default();
    let to_root = plugin.deploys_to_install_root(content_root);
    let wrap =
        !to_root && (plugin.prefers_mod_folder() || plugin.should_wrap_as_mod_folder(content_root));
    let folder_name = plugin.wrap_mod_folder_name(content_root, mod_name);

    for entry in WalkDir::new(content_root)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path == content_root {
            continue;
        }
        let source_rel = normalize_relative(path.strip_prefix(content_root)?);
        // Options decide membership before the plugin decides layout, and they
        // rewrite the path: an include folder is a container, so its contents
        // deploy as if they had been at the mod root all along.
        let rel = match filter {
            Some(f) => match f.map(&source_rel, entry.file_type().is_dir()) {
                Some(mapped) => mapped,
                None => continue,
            },
            None => source_rel.clone(),
        };
        let ctx = DeployContext {
            project_name,
            content_root: Some(content_root),
            source_dir: source_rel.parent(),
        };
        if entry.file_type().is_file() && !plugin.should_deploy_file_ctx(&rel, &ctx) {
            if !plugin.allows_empty_deploy(content_root) {
                stats.skipped += 1;
            }
            continue;
        }
        let deploy_rel: PathBuf = if wrap {
            Path::new(&folder_name).join(&rel)
        } else {
            rel
        };
        let dest = if to_root {
            rewrite_ror2_bepinexpack_dest(&install_path.join(&deploy_rel))
        } else {
            plugin.resolve_deploy_root_ctx(install_path, &deploy_rel, &ctx)?
        };
        if entry.file_type().is_dir() {
            ensure_deploy_dir(&dest)?;
            deployed.paths.push(dest.to_string_lossy().to_string());
            continue;
        }
        if !entry.file_type().is_file() {
            continue;
        }
        if link_or_copy(path, &dest)? == LinkKind::Copied {
            stats.copied += 1;
        }
        deployed.paths.push(dest.to_string_lossy().to_string());
        stats.linked += 1;
    }
    Ok(stats)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Paths;

    fn test_paths() -> (tempfile::TempDir, Paths) {
        let tmp = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: tmp.path().join("config"),
            data_dir: tmp.path().join("data"),
            cache_dir: tmp.path().join("cache"),
        };
        std::fs::create_dir_all(&paths.config_dir).unwrap();
        std::fs::create_dir_all(&paths.data_dir).unwrap();
        std::fs::create_dir_all(&paths.cache_dir).unwrap();
        (tmp, paths)
    }

    /// A one-entry zip, returned as raw bytes so tests can truncate it.
    fn zip_bytes() -> Vec<u8> {
        let mut buf = std::io::Cursor::new(Vec::new());
        let mut zip = zip::ZipWriter::new(&mut buf);
        zip.start_file::<_, ()>("readme.txt", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, b"hello").unwrap();
        zip.finish().unwrap();
        buf.into_inner()
    }

    /// Downloads land under a synthesized `.bin` name, so every extraction path
    /// has to work from the header alone.
    #[test]
    fn a_zip_named_bin_still_extracts() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("helldivers2_383_35491.bin");
        std::fs::write(&archive, zip_bytes()).unwrap();

        let dest = tmp.path().join("out");
        extract_archive(&archive, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("readme.txt")).unwrap(), b"hello");
    }

    #[test]
    fn a_sims_package_named_bin_is_staged_as_a_loose_file() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("sims4_99_1.bin");
        let mut body = b"DBPF".to_vec();
        body.extend_from_slice(&[0u8; 64]);
        std::fs::write(&archive, &body).unwrap();

        let dest = tmp.path().join("out");
        extract_archive(&archive, &dest).unwrap();
        assert_eq!(std::fs::read(dest.join("sims4_99_1.bin")).unwrap(), body);
    }

    /// A `.ts4script` is a real zip, so the loose-file rule must not swallow it.
    #[test]
    fn a_ts4script_is_copied_whole_rather_than_unpacked() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("mod.ts4script");
        std::fs::write(&archive, zip_bytes()).unwrap();

        let dest = tmp.path().join("out");
        extract_archive(&archive, &dest).unwrap();
        assert!(dest.join("mod.ts4script").exists());
        assert!(!dest.join("readme.txt").exists());
    }

    /// The bug this whole change exists for: a download cut short used to be
    /// reported as "unsupported archive type: .bin", which sent people looking
    /// for a format problem that was never there.
    #[test]
    fn a_truncated_zip_reports_the_file_not_a_bogus_archive_type() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("helldivers2_383_35491.bin");
        let full = zip_bytes();
        std::fs::write(&archive, &full[..full.len() / 2]).unwrap();

        let err = extract_archive(&archive, &tmp.path().join("out")).unwrap_err();
        let msg = err.to_string();
        assert!(!msg.contains("unsupported archive type"), "{msg}");
        assert!(msg.contains("helldivers2_383_35491.bin"), "{msg}");
        assert!(msg.contains("looks like zip"), "{msg}");
        assert!(msg.contains("download again"), "{msg}");
    }

    #[test]
    fn random_bytes_fail_with_the_size_included() {
        let tmp = tempfile::tempdir().unwrap();
        let archive = tmp.path().join("garbage.bin");
        std::fs::write(&archive, vec![0x42u8; 4096]).unwrap();

        let err = extract_archive(&archive, &tmp.path().join("out")).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("garbage.bin"), "{msg}");
        assert!(msg.contains("4096 bytes"), "{msg}");
        assert!(msg.contains("not a zip, 7z, or rar"), "{msg}");
    }

    #[test]
    fn headers_are_recognized_regardless_of_name() {
        let tmp = tempfile::tempdir().unwrap();
        let cases: &[(&str, &[u8], ArchiveKind)] = &[
            ("a.bin", b"PK\x03\x04rest", ArchiveKind::Zip),
            ("b.zip", b"7z\xBC\xAF\x27\x1C\x00\x04", ArchiveKind::SevenZ),
            ("c.bin", b"Rar!\x1a\x07\x01\x00", ArchiveKind::Rar),
            ("d.bin", b"DBPF\x02\x00\x00\x00", ArchiveKind::Dbpf),
        ];
        for (name, magic, expected) in cases {
            let path = tmp.path().join(name);
            std::fs::write(&path, magic).unwrap();
            assert_eq!(detect_archive_kind(&path), Some(*expected), "{name}");
        }

        let plain = tmp.path().join("e.bin");
        std::fs::write(&plain, b"nothing here").unwrap();
        assert_eq!(detect_archive_kind(&plain), None);
    }

    /// An Arsenal-style pack: root file, a plain option, and a two-variant group.
    fn stage_hd2_option_mod(paths: &Paths, game_id: &str) -> PathBuf {
        let staging = paths.mods_dir(game_id).join("ArsenalMod_1_1");
        std::fs::create_dir_all(staging.join("Head")).unwrap();
        std::fs::create_dir_all(staging.join("Body").join("Blue")).unwrap();
        std::fs::create_dir_all(staging.join("Body").join("Red")).unwrap();
        std::fs::create_dir_all(staging.join("Unused")).unwrap();
        std::fs::write(
            staging.join("manifest.json"),
            r#"{
                "Version": 1,
                "Name": "Arsenal Mod",
                "Description": "",
                "Options": [
                    { "Name": "Head", "Include": ["Head"] },
                    { "Name": "Body", "SubOptions": [
                        { "Name": "Blue", "Include": ["Body/Blue"] },
                        { "Name": "Red", "Include": ["Body/Red"] }
                    ] }
                ]
            }"#,
        )
        .unwrap();
        std::fs::write(staging.join("always.patch_0"), b"root").unwrap();
        std::fs::write(staging.join("Head").join("aaa.patch_0"), b"head").unwrap();
        std::fs::write(
            staging.join("Body").join("Blue").join("bbb.patch_0"),
            b"blue",
        )
        .unwrap();
        std::fs::write(staging.join("Body").join("Red").join("bbb.patch_0"), b"red").unwrap();
        std::fs::write(staging.join("Unused").join("ccc.patch_0"), b"unused").unwrap();
        staging
    }

    fn hd2_order(staging: &Path, selection: Option<ModOptionSelection>) -> LoadOrder {
        LoadOrder {
            mods: vec![StagedMod {
                id: "1_1".into(),
                name: "Arsenal Mod".into(),
                domain: "helldivers2".into(),
                staging_path: staging.to_string_lossy().into(),
                enabled: true,
                order: 1,
                option_selection: selection,
                ..Default::default()
            }],
        }
    }

    #[test]
    fn hd2_options_default_to_everything_on_with_the_first_variant() {
        let _gate = crate::games::hd2_test_gate();
        let (_tmp, paths) = test_paths();
        let game_id = "hd2_default";
        let install = tempfile::tempdir().unwrap();
        let staging = stage_hd2_option_mod(&paths, game_id);
        save_loadorder(&paths, game_id, &hd2_order(&staging, None)).unwrap();

        deploy(&paths, game_id, "helldivers2", install.path(), None).unwrap();

        let data = install.path().join("data");
        assert!(data.join("always.patch_0").exists());
        assert!(data.join("aaa.patch_0").exists());
        // Blue is the first sub-option, so Red must not be there.
        assert_eq!(std::fs::read(data.join("bbb.patch_0")).unwrap(), b"blue");
        // A folder the manifest never includes is packaging, not content.
        assert!(!data.join("ccc.patch_0").exists());
        // Include folders are containers; they leave no shell behind.
        assert!(!data.join("Head").exists());
        assert!(!data.join("Body").exists());
        assert!(!data.join("manifest.json").exists());
    }

    #[test]
    fn hd2_selection_picks_the_other_variant_and_drops_disabled_options() {
        let _gate = crate::games::hd2_test_gate();
        let (_tmp, paths) = test_paths();
        let game_id = "hd2_selected";
        let install = tempfile::tempdir().unwrap();
        let staging = stage_hd2_option_mod(&paths, game_id);
        let selection = ModOptionSelection {
            enabled_options: vec!["1-body".into()],
            sub_choice: [("1-body".to_string(), "1-body.1-red".to_string())]
                .into_iter()
                .collect(),
        };
        save_loadorder(&paths, game_id, &hd2_order(&staging, Some(selection))).unwrap();

        deploy(&paths, game_id, "helldivers2", install.path(), None).unwrap();

        let data = install.path().join("data");
        assert!(data.join("always.patch_0").exists());
        assert!(!data.join("aaa.patch_0").exists());
        assert_eq!(std::fs::read(data.join("bbb.patch_0")).unwrap(), b"red");
    }

    #[test]
    fn hd2_option_selection_survives_a_reinstall() {
        let (_tmp, paths) = test_paths();
        let game_id = "hd2_update";
        let staging = stage_hd2_option_mod(&paths, game_id);
        let selection = ModOptionSelection {
            enabled_options: vec!["1-body".into()],
            sub_choice: [("1-body".to_string(), "1-body.1-red".to_string())]
                .into_iter()
                .collect(),
        };
        let mut order = hd2_order(&staging, Some(selection.clone()));
        order.mods[0].nexus_mod_id = 1;
        order.mods[0].nexus_file_id = 1;
        save_loadorder(&paths, game_id, &order).unwrap();

        let archive = _tmp.path().join("update.zip");
        let file = std::fs::File::create(&archive).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file::<_, ()>("abc.patch_0", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, b"x").unwrap();
        zip.finish().unwrap();

        let staged = stage_mod_replacing(
            &paths,
            game_id,
            "1_1",
            "Arsenal Mod",
            "helldivers2",
            1,
            2,
            None,
            None,
            &archive,
        )
        .unwrap();
        assert_eq!(staged.option_selection, Some(selection));
    }

    #[test]
    fn deploy_fomod_selection_for_cyberpunk() {
        let (_tmp, paths) = test_paths();
        let game_id = "cp_fomod";
        let install = tempfile::tempdir().unwrap();

        let staging = paths.mods_dir(game_id).join("TCP_1_1");
        for (dir, body) in [
            ("00 Core", b"core" as &[u8]),
            ("01 English", b"en"),
            ("02 Russian", b"ru"),
            ("10 All Voices", b"all"),
            ("11 Jackie Only", b"jackie"),
        ] {
            let p = staging.join(dir).join("r6").join("scripts");
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(p.join("mod.reds"), body).unwrap();
        }
        std::fs::create_dir_all(staging.join("fomod")).unwrap();
        std::fs::write(
            staging.join("fomod").join("ModuleConfig.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>True Cyberpsychosis</moduleName>
  <requiredInstallFiles>
    <folder source="00 Core" destination="" priority="0"/>
  </requiredInstallFiles>
  <installSteps order="Explicit">
    <installStep name="Voice Language">
      <optionalFileGroups order="Explicit">
        <group name="Language" type="SelectExactlyOne">
          <plugins order="Explicit">
            <plugin name="English">
              <files><folder source="01 English" destination=""/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="Russian">
              <files><folder source="02 Russian" destination=""/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
    <installStep name="Voice Set">
      <optionalFileGroups order="Explicit">
        <group name="Voices" type="SelectExactlyOne">
          <plugins order="Explicit">
            <plugin name="All voices">
              <files><folder source="10 All Voices" destination="" priority="2"/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="Jackie only">
              <files><folder source="11 Jackie Only" destination="" priority="2"/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#,
        )
        .unwrap();

        let module = fomod::load_module(&staging).unwrap().unwrap();
        let mut selection = fomod::default_selection(&module);
        // Pick Russian + Jackie only.
        let lang = &module.steps[0].groups[0];
        let voices = &module.steps[1].groups[0];
        selection
            .selected
            .insert(lang.id.clone(), vec![lang.plugins[1].id.clone()]);
        selection
            .selected
            .insert(voices.id.clone(), vec![voices.plugins[1].id.clone()]);

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "1_1".into(),
                    name: "True Cyberpsychosis".into(),
                    nexus_mod_id: 1,
                    nexus_file_id: 1,
                    version: None,
                    domain: "cyberpunk2077".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    fomod_selection: Some(selection),
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert!(result.file_count >= 1, "warnings: {:?}", result.warnings);
        let deployed = install.path().join("r6/scripts/mod.reds");
        assert!(
            deployed.exists(),
            "missing {deployed:?}; warnings: {:?}",
            result.warnings
        );
        // Highest priority among overlapping dests is Jackie (priority 2).
        assert_eq!(std::fs::read(&deployed).unwrap(), b"jackie");
        // FOMOD metadata must not deploy.
        assert!(!install.path().join("fomod").exists());
        assert!(!install.path().join("mods/fomod").exists());
    }

    #[test]
    fn deploy_fomod_preserves_audioxl_case() {
        let (_tmp, paths) = test_paths();
        let game_id = "cp_fomod_case";
        let install = tempfile::tempdir().unwrap();

        let staging = paths.mods_dir(game_id).join("TCP_case");
        let voice = staging
            .join("red4ext")
            .join("plugins")
            .join("AudioXL")
            .join("sounds")
            .join("TrueCyberpsychosis");
        std::fs::create_dir_all(&voice).unwrap();
        std::fs::write(voice.join("tcp_heartbeat.ogg"), b"beat").unwrap();
        std::fs::create_dir_all(staging.join("fomod")).unwrap();
        std::fs::write(
            staging.join("fomod").join("ModuleConfig.xml"),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>True Cyberpsychosis</moduleName>
  <requiredInstallFiles>
    <folder source="red4ext" destination="red4ext"/>
  </requiredInstallFiles>
</config>"#,
        )
        .unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "case_1".into(),
                    name: "True Cyberpsychosis".into(),
                    nexus_mod_id: 1,
                    nexus_file_id: 1,
                    version: None,
                    domain: "cyberpunk2077".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert!(result.file_count >= 1, "warnings: {:?}", result.warnings);
        let deployed = install
            .path()
            .join("red4ext/plugins/AudioXL/sounds/TrueCyberpsychosis/tcp_heartbeat.ogg");
        assert!(
            deployed.is_file(),
            "missing {deployed:?}; warnings: {:?}",
            result.warnings
        );
        assert_eq!(std::fs::read(&deployed).unwrap(), b"beat");
        assert!(!install.path().join("red4ext/plugins/audioxl").exists());
    }

    #[test]
    fn fomod_update_clears_selection_and_redeploys_cleanly() {
        let (tmp, paths) = test_paths();
        let game_id = "cp_fomod_update";
        let install = tempfile::tempdir().unwrap();

        let write_fomod_zip = |path: &Path, entries: &[(&str, &[u8])]| {
            let file = std::fs::File::create(path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            for (name, body) in entries {
                zip.start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut zip, body).unwrap();
            }
            zip.finish().unwrap();
        };

        let v1_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Mod v1</moduleName>
  <installSteps order="Explicit">
    <installStep name="Variant">
      <optionalFileGroups>
        <group name="Style" type="SelectExactlyOne">
          <plugins>
            <plugin name="Classic">
              <files><folder source="Classic" destination=""/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="Alt">
              <files><folder source="Alt" destination=""/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;
        let v1_zip = tmp.path().join("v1.zip");
        write_fomod_zip(
            &v1_zip,
            &[
                ("fomod/ModuleConfig.xml", v1_xml.as_bytes()),
                ("Classic/r6/scripts/old_v1.reds", b"v1-classic"),
                ("Alt/r6/scripts/old_alt.reds", b"v1-alt"),
            ],
        );

        let staged_v1 = stage_mod(
            &paths,
            game_id,
            "My FOMOD",
            "cyberpunk2077",
            99,
            100,
            Some("1.0".into()),
            Some("v1.zip".into()),
            &v1_zip,
        )
        .unwrap();
        assert!(staged_v1.fomod_selection.is_none());

        let m1 = fomod::load_module(Path::new(&staged_v1.staging_path))
            .unwrap()
            .unwrap();
        let g1 = &m1.steps[0].groups[0];
        let mut sel1 = fomod::default_selection(&m1);
        sel1.selected
            .insert(g1.id.clone(), vec![g1.plugins[1].id.clone()]);
        set_fomod_selection(&paths, game_id, &staged_v1.id, sel1.clone()).unwrap();

        let res1 = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert_eq!(res1.file_count, 1);
        assert!(install.path().join("r6/scripts/old_alt.reds").is_file());

        // Re-staging the exact same Nexus file id preserves selection.
        let restaged = stage_mod(
            &paths,
            game_id,
            "My FOMOD",
            "cyberpunk2077",
            99,
            100,
            Some("1.0".into()),
            Some("v1.zip".into()),
            &v1_zip,
        )
        .unwrap();
        assert_eq!(restaged.fomod_selection, Some(sel1));

        // Updating to v2 (different file_id) with new options & case-mismatched folder clears selection.
        let v2_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Mod v2</moduleName>
  <installSteps order="Explicit">
    <installStep name="Variant">
      <optionalFileGroups>
        <group name="Style" type="SelectExactlyOne">
          <plugins>
            <plugin name="New Default">
              <files><folder source="NewDefault" destination=""/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="Updated Alt">
              <files><folder source="UpdatedAlt" destination=""/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;
        let v2_zip = tmp.path().join("v2.zip");
        write_fomod_zip(
            &v2_zip,
            &[
                ("Fomod/ModuleConfig.xml", v2_xml.as_bytes()),
                ("newdefault/r6/scripts/v2_default.reds", b"v2-def"),
                ("updatedalt/r6/scripts/v2_alt.reds", b"v2-alt"),
            ],
        );

        let staged_v2 = stage_mod_replacing(
            &paths,
            game_id,
            &restaged.id,
            "My FOMOD",
            "cyberpunk2077",
            99,
            200,
            Some("2.0".into()),
            Some("v2.zip".into()),
            &v2_zip,
        )
        .unwrap();
        let pending = staged_v2
            .fomod_pending
            .as_ref()
            .expect("renaming selected options must require FOMOD review");
        let m2 = fomod::load_module(Path::new(&staged_v2.staging_path))
            .unwrap()
            .unwrap();
        let g2 = &m2.steps[0].groups[0];
        assert!(pending.invalidated_group_ids.contains(&g2.id));

        let mut sel2 = fomod::default_selection(&m2);
        sel2.selected
            .insert(g2.id.clone(), vec![g2.plugins[1].id.clone()]);
        set_fomod_selection(&paths, game_id, &staged_v2.id, sel2).unwrap();
        let saved_v2 = load_loadorder(&paths, game_id)
            .unwrap()
            .mods
            .into_iter()
            .find(|m| m.id == staged_v2.id)
            .unwrap();
        assert!(saved_v2.fomod_pending.is_none());

        let res2 = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert_eq!(res2.file_count, 1, "warnings: {:?}", res2.warnings);
        assert!(
            !install.path().join("r6/scripts/old_alt.reds").exists(),
            "obsolete v1 file must be purged"
        );
        assert_eq!(
            std::fs::read(install.path().join("r6/scripts/v2_alt.reds")).unwrap(),
            b"v2-alt"
        );
    }

    #[test]
    fn fomod_update_preserves_compatible_selection_without_reconfiguration() {
        let (tmp, paths) = test_paths();
        let game_id = "cp_fomod_compat_update";
        let install = tempfile::tempdir().unwrap();

        let write_fomod_zip = |path: &Path, entries: &[(&str, &[u8])]| {
            let file = std::fs::File::create(path).unwrap();
            let mut zip = zip::ZipWriter::new(file);
            for (name, body) in entries {
                zip.start_file::<_, ()>(*name, zip::write::SimpleFileOptions::default())
                    .unwrap();
                std::io::Write::write_all(&mut zip, body).unwrap();
            }
            zip.finish().unwrap();
        };

        let v1_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Mod v1</moduleName>
  <installSteps order="Explicit">
    <installStep name="Variant">
      <optionalFileGroups>
        <group name="Style" type="SelectExactlyOne">
          <plugins>
            <plugin name="Classic">
              <files><folder source="Classic" destination=""/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="Alt">
              <files><folder source="Alt" destination=""/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;
        // v2 reorders options (Alt moves to index 0) with updated file contents.
        let v2_xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Mod v2</moduleName>
  <installSteps order="Explicit">
    <installStep name="Variant">
      <optionalFileGroups>
        <group name="Style" type="SelectExactlyOne">
          <plugins>
            <plugin name="Alt">
              <files><folder source="Alt" destination=""/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
            <plugin name="Classic">
              <files><folder source="Classic" destination=""/></files>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;

        let v1_zip = tmp.path().join("v1.zip");
        write_fomod_zip(
            &v1_zip,
            &[
                ("fomod/ModuleConfig.xml", v1_xml.as_bytes()),
                ("Classic/r6/scripts/classic.reds", b"v1-classic"),
                ("Alt/r6/scripts/alt.reds", b"v1-alt"),
            ],
        );
        let staged_v1 = stage_mod(
            &paths,
            game_id,
            "My FOMOD",
            "cyberpunk2077",
            99,
            100,
            Some("1.0".into()),
            Some("v1.zip".into()),
            &v1_zip,
        )
        .unwrap();
        let m1 = fomod::load_module(Path::new(&staged_v1.staging_path))
            .unwrap()
            .unwrap();
        let g1 = &m1.steps[0].groups[0];
        let mut sel1 = fomod::default_selection(&m1);
        sel1.selected
            .insert(g1.id.clone(), vec![g1.plugins[1].id.clone()]); // Alt
        set_fomod_selection(&paths, game_id, &staged_v1.id, sel1).unwrap();

        let v2_zip = tmp.path().join("v2.zip");
        write_fomod_zip(
            &v2_zip,
            &[
                ("fomod/ModuleConfig.xml", v2_xml.as_bytes()),
                ("Classic/r6/scripts/classic.reds", b"v2-classic"),
                ("Alt/r6/scripts/alt.reds", b"v2-alt"),
            ],
        );
        let staged_v2 = stage_mod_replacing(
            &paths,
            game_id,
            &staged_v1.id,
            "My FOMOD",
            "cyberpunk2077",
            99,
            200,
            Some("2.0".into()),
            Some("v2.zip".into()),
            &v2_zip,
        )
        .unwrap();

        assert!(
            staged_v2.fomod_pending.is_none(),
            "reordered options with no new settings should not require reconfiguration"
        );
        assert!(staged_v2.fomod_selection.is_some());

        // Deploying immediately uses the migrated "Alt" choice.
        let res = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert_eq!(res.file_count, 1, "warnings: {:?}", res.warnings);
        assert_eq!(
            std::fs::read(install.path().join("r6/scripts/alt.reds")).unwrap(),
            b"v2-alt"
        );
    }

    #[test]
    fn thunderstore_and_modio_fomod_updates_clear_selection_but_repairs_keep_it() {
        let (tmp, paths) = test_paths();
        let game_id = "fomod_ts_modio";
        let archive = tmp.path().join("mod.zip");
        write_zip(&archive, "plugin.dll", b"payload");

        let mut selection = fomod::FomodSelection::default();
        selection
            .selected
            .insert("0-step.0-group".into(), vec!["0-step.0-group.0-opt".into()]);

        // Thunderstore: same version preserves selection, new version clears it.
        let ts1 = stage_thunderstore_mod(
            &paths,
            game_id,
            "community",
            "Author",
            "Pkg",
            "1.0.0",
            None,
            "Pkg",
            None,
            None,
            &archive,
        )
        .unwrap();
        set_fomod_selection(&paths, game_id, &ts1.id, selection.clone()).unwrap();

        let ts_repair = stage_thunderstore_mod(
            &paths,
            game_id,
            "community",
            "Author",
            "Pkg",
            "1.0.0",
            None,
            "Pkg",
            None,
            None,
            &archive,
        )
        .unwrap();
        assert_eq!(ts_repair.fomod_selection, Some(selection.clone()));

        let ts_update = stage_thunderstore_mod(
            &paths,
            game_id,
            "community",
            "Author",
            "Pkg",
            "2.0.0",
            None,
            "Pkg",
            None,
            None,
            &archive,
        )
        .unwrap();
        assert!(ts_update.fomod_selection.is_none());

        // mod.io: same file_id preserves selection, new file_id clears it.
        let m1 = stage_modio_mod(
            &paths,
            game_id,
            7,
            42,
            1,
            "Widget",
            Some("1.0".into()),
            None,
            None,
            &archive,
        )
        .unwrap();
        set_fomod_selection(&paths, game_id, &m1.id, selection.clone()).unwrap();

        let m_same = stage_modio_mod(
            &paths,
            game_id,
            7,
            42,
            1,
            "Widget",
            Some("1.0".into()),
            None,
            None,
            &archive,
        )
        .unwrap();
        assert_eq!(m_same.fomod_selection, Some(selection));

        let m_update = stage_modio_mod(
            &paths,
            game_id,
            7,
            42,
            2,
            "Widget",
            Some("2.0".into()),
            None,
            None,
            &archive,
        )
        .unwrap();
        assert!(m_update.fomod_selection.is_none());
    }

    #[test]
    fn deploy_cyberpunk_preserves_bin_and_red4ext() {
        let (_tmp, paths) = test_paths();
        let game_id = "cp_test";
        let install = tempfile::tempdir().unwrap();

        let staging = paths.mods_dir(game_id).join("CET_1_1");
        std::fs::create_dir_all(staging.join("bin").join("x64").join("plugins")).unwrap();
        std::fs::write(staging.join("bin").join("x64").join("version.dll"), b"dll").unwrap();
        std::fs::create_dir_all(staging.join("red4ext").join("plugins")).unwrap();
        std::fs::write(staging.join("red4ext").join("RED4ext.dll"), b"dll").unwrap();

        let order = LoadOrder {
            mods: vec![StagedMod {
                id: "1_1".into(),
                name: "CET".into(),
                nexus_mod_id: 1,
                nexus_file_id: 1,
                version: None,
                domain: "cyberpunk2077".into(),
                staging_path: staging.to_string_lossy().into(),
                enabled: true,
                order: 1,
                ..Default::default()
            }],
        };
        save_loadorder(&paths, game_id, &order).unwrap();

        let result = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert!(result.file_count >= 2, "warnings: {:?}", result.warnings);
        assert!(install.path().join("bin/x64/version.dll").exists());
        assert!(install.path().join("red4ext/RED4ext.dll").exists());
        assert!(!install.path().join("mods/bin/x64/version.dll").exists());
        assert!(!install.path().join("mods/red4ext/RED4ext.dll").exists());
    }

    #[test]
    fn deploy_peels_wrapper_then_keeps_roots() {
        let (_tmp, paths) = test_paths();
        let game_id = "cp_wrap";
        let install = tempfile::tempdir().unwrap();

        let staging = paths.mods_dir(game_id).join("Wrap_2_2");
        let inner = staging.join("Cyber Engine Tweaks");
        std::fs::create_dir_all(inner.join("bin").join("x64")).unwrap();
        std::fs::write(inner.join("bin").join("x64").join("global.ini"), b"x").unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "2_2".into(),
                    name: "CET Wrap".into(),
                    nexus_mod_id: 2,
                    nexus_file_id: 2,
                    version: None,
                    domain: "cyberpunk2077".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert_eq!(result.file_count, 1, "{:?}", result.warnings);
        assert!(install.path().join("bin/x64/global.ini").exists());
    }

    #[test]
    fn deploy_flat_archive_and_backslash_paths() {
        let (_tmp, paths) = test_paths();
        let game_id = "cp_flat";
        let install = tempfile::tempdir().unwrap();

        let staging = paths.mods_dir(game_id).join("Flat_3_3");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("Cool.archive"), b"a").unwrap();
        // Literal backslash in filename as produced by some Windows zips on Linux.
        std::fs::write(staging.join(r"r6\scripts\Mod\mod.reds"), b"reds").unwrap();
        repair_backslash_entries(&staging).unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "3_3".into(),
                    name: "Flat".into(),
                    nexus_mod_id: 3,
                    nexus_file_id: 3,
                    version: None,
                    domain: "cyberpunk2077".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert!(result.file_count >= 2, "{:?}", result.warnings);
        assert!(install.path().join("archive/pc/mod/Cool.archive").exists());
        assert!(install.path().join("r6/scripts/Mod/mod.reds").exists());
    }

    #[test]
    fn repair_backslash_entries_builds_tree() {
        let staging = tempfile::tempdir().unwrap();
        std::fs::write(staging.path().join(r"BepInEx\plugins\"), b"").unwrap();
        std::fs::write(
            staging
                .path()
                .join(r"BepInEx\plugins\DarkMode\DarkMode.dll"),
            b"dll",
        )
        .unwrap();

        repair_backslash_entries(staging.path()).unwrap();

        assert!(staging.path().join("BepInEx/plugins").is_dir());
        assert!(staging
            .path()
            .join("BepInEx/plugins/DarkMode/DarkMode.dll")
            .is_file());
    }

    #[test]
    fn deploy_bepinex_repaired_backslash_plugin() {
        let (_tmp, paths) = test_paths();
        let game_id = "bw_bepinex";
        let install = tempfile::tempdir().unwrap();
        let staging = paths.mods_dir(game_id).join("DarkMode_1");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join(r"BepInEx\plugins\"), b"").unwrap();
        std::fs::write(
            staging.join(r"BepInEx\plugins\DarkMode\DarkMode.dll"),
            b"dll",
        )
        .unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "1".into(),
                    name: "DarkMode".into(),
                    source: ModSource::Thunderstore,
                    domain: "big-walk".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "bepinex", install.path(), None).unwrap();
        assert!(result.file_count >= 1, "{:?}", result.warnings);
        let plugins = install.path().join("BepInEx/plugins");
        assert!(
            plugins.is_dir(),
            "plugins should be a directory, not a symlink to a file"
        );
        assert!(
            plugins.join("DarkMode/DarkMode.dll").exists()
                || plugins.join("DarkMode/DarkMode/DarkMode.dll").exists()
        );
    }

    #[test]
    fn stages_bare_package_file_without_extracting() {
        let tmp = tempfile::tempdir().unwrap();
        let download = tmp.path().join("CoolHair.package");
        std::fs::write(&download, b"DBPF").unwrap();
        let staging = tmp.path().join("staged");

        extract_archive(&download, &staging).unwrap();
        assert_eq!(
            std::fs::read(staging.join("CoolHair.package")).unwrap(),
            b"DBPF"
        );
    }

    #[test]
    fn safe_archive_entry_path_normalizes_backslashes() {
        assert_eq!(
            safe_archive_entry_path(r"BepInEx\plugins\Mod.dll"),
            Some(PathBuf::from("BepInEx/plugins/Mod.dll"))
        );
        assert!(safe_archive_entry_path(r"..\escape").is_none());
    }

    #[test]
    #[cfg(unix)]
    fn purge_install_symlinks_without_manifest() {
        let (_tmp, paths) = test_paths();
        let install = tempfile::tempdir().unwrap();
        let staging = paths.mods_dir("bw").join("Mod_1");
        std::fs::create_dir_all(staging.join("BepInEx/plugins/Mod")).unwrap();
        std::fs::write(staging.join("BepInEx/plugins/Mod/Mod.dll"), b"dll").unwrap();
        std::fs::write(staging.join("README.md"), b"readme").unwrap();
        std::fs::create_dir_all(install.path().join("BepInEx")).unwrap();
        std::os::unix::fs::symlink(
            staging.join("BepInEx/plugins"),
            install.path().join("BepInEx/plugins"),
        )
        .unwrap();
        std::os::unix::fs::symlink(staging.join("README.md"), install.path().join("README.md"))
            .unwrap();

        let removed = purge_install_symlinks(install.path(), &paths.data_dir).unwrap();
        assert!(removed >= 2, "removed {removed}");
        assert!(!install.path().join("BepInEx/plugins").exists());
        assert!(!install.path().join("README.md").exists());
        assert!(staging.join("BepInEx/plugins/Mod/Mod.dll").is_file());
    }

    #[test]
    #[cfg(unix)]
    fn deploy_after_orphaned_symlinks() {
        let (_tmp, paths) = test_paths();
        let game_id = "bw_orphan";
        let install = tempfile::tempdir().unwrap();
        let staging = paths.mods_dir(game_id).join("BigDart_1");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("BigDart.dll"), b"dll").unwrap();
        std::fs::create_dir_all(install.path().join("BepInEx")).unwrap();
        std::os::unix::fs::symlink(&staging, install.path().join("BepInEx/plugins")).unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "1".into(),
                    name: "BigDart".into(),
                    source: ModSource::Thunderstore,
                    domain: "big-walk".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        purge_deploy(&paths, game_id, Some(install.path())).unwrap();
        let result = deploy(&paths, game_id, "bepinex", install.path(), None).unwrap();
        assert!(result.file_count >= 1, "{:?}", result.warnings);
        let plugins = install.path().join("BepInEx/plugins");
        assert!(plugins.is_dir());
        assert!(
            plugins.join("BigDart.dll").is_file()
                || plugins.join("BigDart/BigDart.dll").is_file()
                || plugins.join("BigDart_1/BigDart.dll").is_file()
        );
    }

    #[test]
    fn deploy_bepinex_plugins_under_bepinexpack() {
        let (_tmp, paths) = test_paths();
        let game_id = "bw_pack";
        let install = tempfile::tempdir().unwrap();

        let pack_staging = paths.mods_dir(game_id).join("BepInExPack_1");
        std::fs::create_dir_all(
            pack_staging
                .join("BepInExPack")
                .join("BepInEx")
                .join("core"),
        )
        .unwrap();
        std::fs::write(pack_staging.join("BepInExPack").join("winhttp.dll"), b"x").unwrap();
        std::fs::write(
            pack_staging.join("BepInExPack").join("doorstop_config.ini"),
            b"target_assembly = BepInEx\\core\\BepInEx.Unity.IL2CPP.dll\n",
        )
        .unwrap();

        let mod_staging = paths.mods_dir(game_id).join("BigDart_1");
        std::fs::create_dir_all(&mod_staging).unwrap();
        std::fs::write(mod_staging.join("BigDart.dll"), b"dll").unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    StagedMod {
                        id: "pack".into(),
                        name: "BepInExPack".into(),
                        source: ModSource::Thunderstore,
                        domain: "big-walk".into(),
                        staging_path: pack_staging.to_string_lossy().into(),
                        enabled: true,
                        order: 1,
                        ..Default::default()
                    },
                    StagedMod {
                        id: "dart".into(),
                        name: "BigDart".into(),
                        source: ModSource::Thunderstore,
                        domain: "big-walk".into(),
                        staging_path: mod_staging.to_string_lossy().into(),
                        enabled: true,
                        order: 2,
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "bepinex", install.path(), None).unwrap();
        assert!(result.file_count >= 2, "{:?}", result.warnings);
        assert!(install.path().join("winhttp.dll").is_file());
        assert!(install.path().join("BepInEx").join("core").is_dir());
        let plugins = install.path().join("BepInEx").join("plugins");
        assert!(plugins.is_dir(), "plugins should be under flat BepInEx");
        assert!(
            plugins.join("BigDart.dll").is_file()
                || plugins.join("BigDart/BigDart.dll").is_file()
                || plugins.join("BigDart_1/BigDart.dll").is_file()
        );
        assert!(!install
            .path()
            .join("BepInExPack")
            .join("winhttp.dll")
            .exists());
    }

    #[test]
    fn deploy_bepinexpack_flattens_thunderstore_layout() {
        let (_tmp, paths) = test_paths();
        let game_id = "bw_flat_pack";
        let install = tempfile::tempdir().unwrap();

        let pack_staging = paths.mods_dir(game_id).join("BepInExPack_1");
        std::fs::create_dir_all(
            pack_staging
                .join("BepInExPack")
                .join("BepInEx")
                .join("core"),
        )
        .unwrap();
        std::fs::write(pack_staging.join("BepInExPack").join("winhttp.dll"), b"x").unwrap();
        std::fs::write(pack_staging.join("manifest.json"), b"{}").unwrap();
        std::fs::write(pack_staging.join("icon.png"), b"png").unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "pack".into(),
                    name: "BepInExPack".into(),
                    source: ModSource::Thunderstore,
                    domain: "big-walk".into(),
                    staging_path: pack_staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        deploy(&paths, game_id, "bepinex", install.path(), None).unwrap();
        assert!(install.path().join("winhttp.dll").is_file());
        assert!(install.path().join("BepInEx").join("core").is_dir());
        assert!(!install.path().join("manifest.json").exists());
        assert!(!install.path().join("icon.png").exists());
        assert!(!install
            .path()
            .join("BepInExPack")
            .join("winhttp.dll")
            .exists());
    }

    #[test]
    fn deploy_ror2_plugins_prefix_without_wrap() {
        let (_tmp, paths) = test_paths();
        let game_id = "ror2_plugins";
        let install = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(install.path().join("BepInEx").join("core")).unwrap();

        let staging = paths.mods_dir(game_id).join("R2API_1");
        std::fs::create_dir_all(staging.join("plugins").join("R2API.Legacy")).unwrap();
        std::fs::write(
            staging
                .join("plugins")
                .join("R2API.Legacy")
                .join("R2API.dll"),
            b"dll",
        )
        .unwrap();
        std::fs::write(staging.join("manifest.json"), b"{}").unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "r2api".into(),
                    name: "R2API".into(),
                    source: ModSource::Thunderstore,
                    domain: "riskofrain2".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "riskofrain2", install.path(), None).unwrap();
        assert!(result.file_count >= 1, "{:?}", result.warnings);
        let dll = install
            .path()
            .join("BepInEx")
            .join("plugins")
            .join("R2API.Legacy")
            .join("R2API.dll");
        assert!(
            dll.is_file(),
            "expected flat plugins layout, got {:?}",
            std::fs::read_dir(install.path().join("BepInEx").join("plugins"))
                .map(|d| d
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name())
                    .collect::<Vec<_>>())
                .ok()
        );
        assert!(
            !install
                .path()
                .join("BepInEx")
                .join("plugins")
                .join("R2API")
                .join("plugins")
                .exists()
                && !install
                    .path()
                    .join("BepInEx")
                    .join("plugins")
                    .join("R2API_1")
                    .join("plugins")
                    .exists(),
            "must not double-wrap plugins/"
        );
    }

    #[test]
    fn deploy_ror2_bepinexpack_avoids_pruned_folder() {
        let (_tmp, paths) = test_paths();
        let game_id = "ror2_pack_prune";
        let install = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(install.path().join("BepInEx").join("core")).unwrap();

        let staging = paths.mods_dir(game_id).join("RoR2BepInExPack_1");
        std::fs::create_dir_all(staging.join("plugins").join("RoR2BepInExPack")).unwrap();
        std::fs::write(
            staging
                .join("plugins")
                .join("RoR2BepInExPack")
                .join("Newtonsoft.Json.dll"),
            b"json",
        )
        .unwrap();
        std::fs::write(
            staging
                .join("plugins")
                .join("RoR2BepInExPack")
                .join("RoR2BepInExPack.dll"),
            b"dll",
        )
        .unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "ror2pack".into(),
                    name: "RoR2BepInExPack".into(),
                    source: ModSource::Thunderstore,
                    domain: "riskofrain2".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "riskofrain2", install.path(), None).unwrap();
        assert!(result.file_count >= 2, "{:?}", result.warnings);
        let safe = install
            .path()
            .join("BepInEx")
            .join("plugins")
            .join("RiskofThunder-RoR2BepInExPack");
        assert!(
            safe.join("Newtonsoft.Json.dll").is_file(),
            "expected rewrite to RiskofThunder-RoR2BepInExPack, got {:?}",
            std::fs::read_dir(install.path().join("BepInEx").join("plugins"))
                .map(|d| d
                    .filter_map(|e| e.ok())
                    .map(|e| e.file_name())
                    .collect::<Vec<_>>())
                .ok()
        );
        assert!(safe.join("RoR2BepInExPack.dll").is_file());
        assert!(
            !install
                .path()
                .join("BepInEx")
                .join("plugins")
                .join("RoR2BepInExPack")
                .exists(),
            "must not deploy into the Preloader-pruned folder name"
        );
        assert!(
            !result
                .warnings
                .iter()
                .any(|w| w.contains("Preloader deletes")),
            "preflight should not warn after a correct deploy: {:?}",
            result.warnings
        );
    }

    #[test]
    fn deploy_bepinex_plugin_tree_under_bepinexpack() {
        let (_tmp, paths) = test_paths();
        let game_id = "bw_plugin_tree";
        let install = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(install.path().join("BepInEx").join("core")).unwrap();

        let mod_staging = paths.mods_dir(game_id).join("DarkMode_1");
        std::fs::create_dir_all(
            mod_staging
                .join("BepInEx")
                .join("plugins")
                .join("BigWalk.DarkMode"),
        )
        .unwrap();
        std::fs::write(
            mod_staging
                .join("BepInEx")
                .join("plugins")
                .join("BigWalk.DarkMode")
                .join("BigWalk.DarkMode.dll"),
            b"dll",
        )
        .unwrap();
        std::fs::write(mod_staging.join("manifest.json"), b"{}").unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "darkmode".into(),
                    name: "DarkMode".into(),
                    source: ModSource::Thunderstore,
                    domain: "big-walk".into(),
                    staging_path: mod_staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "bepinex", install.path(), None).unwrap();
        assert!(result.file_count >= 1, "{:?}", result.warnings);
        let correct = install
            .path()
            .join("BepInEx")
            .join("plugins")
            .join("BigWalk.DarkMode")
            .join("BigWalk.DarkMode.dll");
        assert!(
            correct.is_file(),
            "expected plugin at {}",
            correct.display()
        );
    }

    #[test]
    fn deploy_never_symlinks_staging_into_the_game() {
        let (_tmp, paths) = test_paths();
        let game_id = "bw_no_symlinks";
        let install = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(install.path().join("BepInEx").join("core")).unwrap();

        let staging = paths.mods_dir(game_id).join("DarkMode_1");
        std::fs::create_dir_all(staging.join("plugins")).unwrap();
        std::fs::write(staging.join("plugins").join("DarkMode.dll"), b"dll").unwrap();
        std::fs::write(staging.join("plugins").join("config.json"), b"{}").unwrap();
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "darkmode".into(),
                    name: "DarkMode".into(),
                    source: ModSource::Thunderstore,
                    domain: "big-walk".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        deploy(&paths, game_id, "bepinex", install.path(), None).unwrap();

        // A loader that deletes its own plugin folder must not reach staging.
        let plugins = install.path().join("BepInEx").join("plugins");
        for name in ["DarkMode.dll", "config.json"] {
            let deployed = plugins.join(name);
            assert!(
                !std::fs::symlink_metadata(&deployed)
                    .unwrap()
                    .file_type()
                    .is_symlink(),
                "{name} was symlinked into the game folder"
            );
        }
        std::fs::remove_dir_all(&plugins).unwrap();
        assert!(staging.join("plugins").join("DarkMode.dll").is_file());
        assert!(staging.join("plugins").join("config.json").is_file());
    }

    #[test]
    fn deploy_warns_when_staged_files_went_missing() {
        let (_tmp, paths) = test_paths();
        let game_id = "bw_damaged";
        let install = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(install.path().join("BepInEx").join("core")).unwrap();

        let staging = paths.mods_dir(game_id).join("Pack_1");
        std::fs::create_dir_all(staging.join("plugins")).unwrap();
        std::fs::write(staging.join("plugins").join("Pack.dll"), b"dll").unwrap();
        let order = LoadOrder {
            mods: vec![StagedMod {
                id: "pack".into(),
                name: "Pack".into(),
                source: ModSource::Thunderstore,
                domain: "big-walk".into(),
                staging_path: staging.to_string_lossy().into(),
                enabled: true,
                order: 1,
                // Extraction produced five files; four are gone.
                staged_file_count: Some(5),
                ..Default::default()
            }],
        };
        save_loadorder(&paths, game_id, &order).unwrap();

        assert_eq!(damaged_staging(&order).len(), 1);
        let result = deploy(&paths, game_id, "bepinex", install.path(), None).unwrap();
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Pack is missing 4 of 5")),
            "{:?}",
            result.warnings
        );
    }

    #[test]
    fn purge_removes_empty_plugin_folders_left_behind() {
        let (_tmp, paths) = test_paths();
        let game_id = "bw_prune";
        let install = tempfile::tempdir().unwrap();
        let plugins = install.path().join("BepInEx").join("plugins");
        std::fs::create_dir_all(install.path().join("BepInEx").join("core")).unwrap();
        std::fs::create_dir_all(plugins.join("GoneMod").join("nested")).unwrap();
        std::fs::create_dir_all(plugins.join("StillHere")).unwrap();
        std::fs::write(plugins.join("StillHere").join("mod.dll"), b"dll").unwrap();

        purge_deploy(&paths, game_id, Some(install.path())).unwrap();
        let removed = prune_empty_loader_dirs(
            crate::games::plugin_by_id("bepinex").unwrap(),
            install.path(),
        );

        assert_eq!(removed, 2);
        assert!(!plugins.join("GoneMod").exists());
        assert!(plugins.join("StillHere").join("mod.dll").is_file());
    }

    #[test]
    fn deploy_zero_enabled_reports_warning() {
        let (_tmp, paths) = test_paths();
        let game_id = "cp_empty";
        let install = tempfile::tempdir().unwrap();
        save_loadorder(&paths, game_id, &LoadOrder::default()).unwrap();
        let result = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert_eq!(result.file_count, 0);
        assert_eq!(result.enabled_mods, 0);
        assert!(!result.warnings.is_empty());
    }

    #[test]
    fn deploy_missing_staging_warns() {
        let (_tmp, paths) = test_paths();
        let game_id = "cp_missing";
        let install = tempfile::tempdir().unwrap();
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "9_9".into(),
                    name: "Gone".into(),
                    nexus_mod_id: 9,
                    nexus_file_id: 9,
                    version: None,
                    domain: "cyberpunk2077".into(),
                    staging_path: paths
                        .mods_dir(game_id)
                        .join("does_not_exist")
                        .to_string_lossy()
                        .into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();
        let result = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert_eq!(result.file_count, 0);
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("Missing staging")));
    }

    #[test]
    fn deploy_redmod_wraps_info_json() {
        let (_tmp, paths) = test_paths();
        let game_id = "cp_redmod";
        let install = tempfile::tempdir().unwrap();

        let staging = paths.mods_dir(game_id).join("Red_4_4");
        let inner = staging.join("MyRedMod");
        std::fs::create_dir_all(inner.join("archives")).unwrap();
        std::fs::write(inner.join("info.json"), r#"{"name":"MyRedMod"}"#).unwrap();
        std::fs::write(inner.join("archives").join("x.archive"), b"a").unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "4_4".into(),
                    name: "Nexus REDmod Title".into(),
                    nexus_mod_id: 4,
                    nexus_file_id: 4,
                    version: None,
                    domain: "cyberpunk2077".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "cyberpunk2077", install.path(), None).unwrap();
        assert!(result.file_count >= 2, "{:?}", result.warnings);
        assert!(install.path().join("mods/MyRedMod/info.json").exists());
        assert!(!install.path().join("mods/Nexus REDmod Title").exists());
    }

    fn stage_stardew(paths: &Paths, game_id: &str, id: &str, name: &str, staging: PathBuf) {
        save_loadorder(
            paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: id.into(),
                    name: name.into(),
                    nexus_mod_id: 1,
                    nexus_file_id: 1,
                    version: None,
                    domain: "stardewvalley".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();
    }

    #[test]
    fn deploy_stardew_wraps_manifest_mod() {
        let (_tmp, paths) = test_paths();
        let game_id = "sdv_wrap";
        let install = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(install.path().join("smapi-internal")).unwrap();

        let staging = paths.mods_dir(game_id).join("Cool_1_1");
        let inner = staging.join("CoolMod");
        std::fs::create_dir_all(&inner).unwrap();
        std::fs::write(inner.join("manifest.json"), r#"{"UniqueID":"A.Cool"}"#).unwrap();
        std::fs::write(inner.join("Cool.dll"), b"dll").unwrap();
        stage_stardew(&paths, game_id, "1_1", "Cool Mod", staging);

        let result = deploy(&paths, game_id, "stardewvalley", install.path(), None).unwrap();
        assert!(result.file_count >= 2, "{:?}", result.warnings);
        assert!(install.path().join("Mods/CoolMod/manifest.json").exists());
        assert!(install.path().join("Mods/CoolMod/Cool.dll").exists());
        assert!(!install.path().join("Mods/Cool Mod/manifest.json").exists());
        assert!(!result
            .warnings
            .iter()
            .any(|w| w.contains("SMAPI not found")));
    }

    #[test]
    fn deploy_stardew_mods_prefix_no_double_nest() {
        let (_tmp, paths) = test_paths();
        let game_id = "sdv_mods";
        let install = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(install.path().join("smapi-internal")).unwrap();

        let staging = paths.mods_dir(game_id).join("Pack_2_2");
        std::fs::create_dir_all(staging.join("Mods").join("CoolMod")).unwrap();
        std::fs::write(
            staging.join("Mods").join("CoolMod").join("manifest.json"),
            "{}",
        )
        .unwrap();
        stage_stardew(&paths, game_id, "2_2", "Pack", staging);

        let result = deploy(&paths, game_id, "stardewvalley", install.path(), None).unwrap();
        assert_eq!(result.file_count, 1, "{:?}", result.warnings);
        assert!(install.path().join("Mods/CoolMod/manifest.json").exists());
        assert!(!install
            .path()
            .join("Mods/Pack/Mods/CoolMod/manifest.json")
            .exists());
    }

    #[test]
    fn deploy_stardew_multi_mod_siblings() {
        let (_tmp, paths) = test_paths();
        let game_id = "sdv_multi";
        let install = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(install.path().join("smapi-internal")).unwrap();

        let staging = paths.mods_dir(game_id).join("Bundle_3_3");
        for name in ["ModA", "ModB"] {
            let dir = staging.join(name);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("manifest.json"), "{}").unwrap();
        }
        stage_stardew(&paths, game_id, "3_3", "Bundle", staging);

        let result = deploy(&paths, game_id, "stardewvalley", install.path(), None).unwrap();
        assert_eq!(result.file_count, 2, "{:?}", result.warnings);
        assert!(install.path().join("Mods/ModA/manifest.json").exists());
        assert!(install.path().join("Mods/ModB/manifest.json").exists());
    }

    #[test]
    fn deploy_sims4_wraps_mods_and_skips_unloadable_files() {
        let (_tmp, paths) = test_paths();
        let game_id = "sims4_deploy";
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp
            .path()
            .join("steamapps")
            .join("common")
            .join("The Sims 4");
        std::fs::create_dir_all(&install).unwrap();
        let user_dir = tmp
            .path()
            .join("steamapps")
            .join("compatdata")
            .join("1222670")
            .join("pfx")
            .join("drive_c")
            .join("users")
            .join("steamuser")
            .join("Documents")
            .join("Electronic Arts")
            .join("The Sims 4");
        std::fs::create_dir_all(&user_dir).unwrap();

        let staging = paths.mods_dir(game_id).join("MCCC_7_7");
        std::fs::create_dir_all(staging.join("scripts")).unwrap();
        std::fs::write(staging.join("mccc.package"), b"pkg").unwrap();
        std::fs::write(staging.join("scripts").join("core.ts4script"), b"zip").unwrap();
        std::fs::write(staging.join("README.txt"), b"docs").unwrap();
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "7_7".into(),
                    name: "MCCC".into(),
                    nexus_mod_id: 7,
                    nexus_file_id: 7,
                    version: None,
                    domain: "thesims4".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "sims4", &install, None).unwrap();
        let mods = user_dir.join("Mods");
        assert_eq!(result.file_count, 2, "{:?}", result.warnings);
        assert!(mods.join("MCCC").join("mccc.package").exists());
        // Flattened up from scripts/ so the game can still load it.
        assert!(mods.join("MCCC").join("scripts_core.ts4script").exists());
        assert!(!mods.join("MCCC").join("README.txt").exists());
        assert!(mods.join("Resource.cfg").is_file());
        assert!(
            result.warnings.iter().any(|w| w.contains("skipped")),
            "{:?}",
            result.warnings
        );

        // Purge must clean the user-data folder, not just the install.
        purge_deploy(&paths, game_id, Some(&install)).unwrap();
        assert!(!mods.join("MCCC").join("mccc.package").exists());
    }

    #[test]
    fn deploy_sims3_routes_packages_and_sims3packs() {
        let (_tmp, paths) = test_paths();
        let game_id = "sims3_deploy";
        let tmp = tempfile::tempdir().unwrap();
        let install = tmp
            .path()
            .join("steamapps")
            .join("common")
            .join("The Sims 3");
        std::fs::create_dir_all(&install).unwrap();
        let user_dir = tmp
            .path()
            .join("steamapps")
            .join("compatdata")
            .join("47890")
            .join("pfx")
            .join("drive_c")
            .join("users")
            .join("steamuser")
            .join("Documents")
            .join("Electronic Arts")
            .join("The Sims 3");
        std::fs::create_dir_all(&user_dir).unwrap();

        let staging = paths.mods_dir(game_id).join("HairSet_8_8");
        std::fs::create_dir_all(staging.join("extras")).unwrap();
        std::fs::write(staging.join("hair.package"), b"pkg").unwrap();
        std::fs::write(staging.join("extras").join("bonus.sims3pack"), b"s3p").unwrap();
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "8_8".into(),
                    name: "Hair Set".into(),
                    nexus_mod_id: 8,
                    nexus_file_id: 8,
                    version: None,
                    domain: "thesims3".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(&paths, game_id, "sims3", &install, None).unwrap();
        assert_eq!(result.file_count, 2, "{:?}", result.warnings);
        assert!(user_dir
            .join("Mods")
            .join("Packages")
            .join("Hair Set")
            .join("hair.package")
            .exists());
        // The Launcher only reads the top level of Downloads.
        assert!(user_dir.join("Downloads").join("bonus.sims3pack").exists());
        assert!(user_dir.join("Mods").join("Resource.cfg").is_file());
        assert!(
            result
                .warnings
                .iter()
                .any(|w| w.contains("Sims 3 Launcher")),
            "{:?}",
            result.warnings
        );
    }

    #[test]
    fn deploy_stardew_warns_without_smapi() {
        let (_tmp, paths) = test_paths();
        let game_id = "sdv_nosmapi";
        let install = tempfile::tempdir().unwrap();

        let staging = paths.mods_dir(game_id).join("M_4_4");
        std::fs::create_dir_all(&staging).unwrap();
        std::fs::write(staging.join("manifest.json"), "{}").unwrap();
        stage_stardew(&paths, game_id, "4_4", "M", staging);

        let result = deploy(&paths, game_id, "stardewvalley", install.path(), None).unwrap();
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("SMAPI not found")));
        assert!(install.path().join("Mods/M/manifest.json").exists());
    }

    #[test]
    fn deploy_stardew_smapi_installer_to_root() {
        let (_tmp, paths) = test_paths();
        let game_id = "sdv_smapi";
        let install = tempfile::tempdir().unwrap();

        let staging = paths.mods_dir(game_id).join("SMAPI_5_5");
        let inner = staging.join("SMAPI 4.0");
        std::fs::create_dir_all(inner.join("smapi-internal")).unwrap();
        std::fs::write(inner.join("StardewModdingAPI.exe"), b"exe").unwrap();
        std::fs::write(inner.join("smapi-internal").join("config.json"), b"{}").unwrap();
        stage_stardew(&paths, game_id, "5_5", "SMAPI", staging);

        let result = deploy(&paths, game_id, "stardewvalley", install.path(), None).unwrap();
        assert!(result.file_count >= 2, "{:?}", result.warnings);
        assert!(install.path().join("StardewModdingAPI.exe").exists());
        assert!(install.path().join("smapi-internal/config.json").exists());
        assert!(!install
            .path()
            .join("Mods/SMAPI/StardewModdingAPI.exe")
            .exists());
        assert!(result
            .warnings
            .iter()
            .any(|w| w.contains("SMAPI installer")));
    }

    #[test]
    fn deploy_darktide_writes_load_order_and_skips_dmf() {
        let (_tmp, paths) = test_paths();
        let game_id = "dt_test";
        let install = tempfile::tempdir().unwrap();
        // Already patched so we do not spawn dtkit-patch.
        std::fs::create_dir_all(install.path().join("bundle")).unwrap();
        std::fs::write(
            install.path().join("bundle/bundle_database.data"),
            b"xxxpatch_999yyy",
        )
        .unwrap();

        let holy_light_staging = paths.mods_dir(game_id).join("Holy Light_1_1");
        let holy_light_inner = holy_light_staging.join("HolyLight");
        std::fs::create_dir_all(&holy_light_inner).unwrap();
        std::fs::write(holy_light_inner.join("HolyLight.mod"), b"mod").unwrap();

        let dmf_staging = paths.mods_dir(game_id).join("DMF_2_2");
        std::fs::create_dir_all(dmf_staging.join("dmf")).unwrap();
        std::fs::write(dmf_staging.join("dmf").join("dmf.lua"), b"lua").unwrap();

        let health_staging = paths.mods_dir(game_id).join("Healthbars_3_3");
        let health_inner = health_staging.join("healthbars");
        std::fs::create_dir_all(&health_inner).unwrap();
        std::fs::write(health_inner.join("healthbars.mod"), b"mod").unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    StagedMod {
                        id: "1_1".into(),
                        name: "Holy Light".into(),
                        nexus_mod_id: 1,
                        nexus_file_id: 1,
                        version: None,
                        domain: "warhammer40kdarktide".into(),
                        staging_path: holy_light_staging.to_string_lossy().into(),
                        enabled: true,
                        order: 1,
                        ..Default::default()
                    },
                    StagedMod {
                        id: "2_2".into(),
                        name: "Darktide Mod Framework".into(),
                        nexus_mod_id: 2,
                        nexus_file_id: 2,
                        version: None,
                        domain: "warhammer40kdarktide".into(),
                        staging_path: dmf_staging.to_string_lossy().into(),
                        enabled: true,
                        order: 2,
                        ..Default::default()
                    },
                    StagedMod {
                        id: "3_3".into(),
                        name: "Healthbars".into(),
                        nexus_mod_id: 3,
                        nexus_file_id: 3,
                        version: None,
                        domain: "warhammer40kdarktide".into(),
                        staging_path: health_staging.to_string_lossy().into(),
                        enabled: false,
                        order: 3,
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();

        let result = deploy(
            &paths,
            game_id,
            "warhammer40kdarktide",
            install.path(),
            None,
        )
        .unwrap();
        assert!(result.file_count >= 2, "{:?}", result.warnings);
        assert!(install.path().join("mods/HolyLight/HolyLight.mod").exists());
        assert!(!install.path().join("mods/Holy Light").exists());
        assert!(install.path().join("mods/dmf/dmf.lua").exists());
        assert!(!install.path().join("mods/Healthbars").exists());

        let order_txt =
            std::fs::read_to_string(install.path().join("mods/mod_load_order.txt")).unwrap();
        assert!(order_txt.contains("HolyLight\n"));
        assert!(!order_txt.contains("Holy Light\n"));
        assert!(!order_txt.lines().any(|l| l.trim() == "dmf"));
        assert!(!order_txt.contains("Healthbars"));
        assert!(!order_txt.contains("Darktide Mod Framework"));
    }

    #[test]
    fn deploy_darktide_loader_root_layout() {
        let (_tmp, paths) = test_paths();
        let game_id = "dt_loader";
        let install = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(install.path().join("bundle")).unwrap();
        std::fs::write(
            install.path().join("bundle/bundle_database.data"),
            b"xxxpatch_999yyy",
        )
        .unwrap();

        let staging = paths.mods_dir(game_id).join("Loader_1_1");
        std::fs::create_dir_all(staging.join("tools")).unwrap();
        std::fs::create_dir_all(staging.join("binaries")).unwrap();
        std::fs::create_dir_all(staging.join("mods")).unwrap();
        std::fs::write(staging.join("tools/dtkit-patch.exe"), b"fake").unwrap();
        std::fs::write(staging.join("binaries/mod_loader"), b"x").unwrap();
        std::fs::write(staging.join("toggle_darktide_mods.bat"), b"bat").unwrap();
        std::fs::write(staging.join("mods/mod_load_order.txt"), b"-- template\n").unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "1_1".into(),
                    name: "Darktide Mod Loader".into(),
                    nexus_mod_id: 19,
                    nexus_file_id: 1,
                    version: None,
                    domain: "warhammer40kdarktide".into(),
                    staging_path: staging.to_string_lossy().into(),
                    enabled: true,
                    order: 1,
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let result = deploy(
            &paths,
            game_id,
            "warhammer40kdarktide",
            install.path(),
            None,
        )
        .unwrap();
        assert!(result.file_count >= 3, "{:?}", result.warnings);
        assert!(install.path().join("tools/dtkit-patch.exe").exists());
        assert!(install.path().join("binaries/mod_loader").exists());
        assert!(install.path().join("toggle_darktide_mods.bat").exists());
        // Loader is not wrapped → not listed in load order (rewritten empty aside from header).
        let order_txt =
            std::fs::read_to_string(install.path().join("mods/mod_load_order.txt")).unwrap();
        assert!(!order_txt
            .lines()
            .any(|l| !l.starts_with("--") && !l.trim().is_empty()));
    }

    #[test]
    fn incoming_version_newest_wins() {
        assert!(incoming_is_newer(None, "1.2.3"));
        assert!(!incoming_is_newer(Some("1.2.3"), "1.2.3"));
        assert!(incoming_is_newer(Some("1.2.3"), "1.2.10"));
        assert!(!incoming_is_newer(Some("1.2.10"), "1.2.3"));
        assert!(incoming_is_newer(Some("build"), "other"));
        assert!(!incoming_is_newer(Some("1.0.0"), "weird"));
        assert!(incoming_is_newer(Some("weird"), "1.0.0"));
    }

    fn nexus_file(
        file_id: u64,
        version: &str,
        category: &str,
        uploaded: u64,
        is_primary: bool,
    ) -> NexusFileMeta {
        NexusFileMeta {
            file_id,
            version: Some(version.into()),
            category_name: Some(category.into()),
            uploaded_timestamp: Some(uploaded),
            is_primary,
            file_name: None,
            name: String::new(),
        }
    }

    fn named(mut file: NexusFileMeta, name: &str) -> NexusFileMeta {
        file.name = name.to_string();
        file
    }

    #[test]
    fn pick_staged_file_name_prefers_the_chooser_title() {
        assert_eq!(
            pick_staged_file_name("Cool Mod", Some("Optional patch"), Some("cool.zip")).as_deref(),
            Some("Optional patch")
        );
        assert_eq!(
            pick_staged_file_name("Cool Mod", Some("Cool Mod"), Some("cool.zip")).as_deref(),
            Some("cool.zip")
        );
        assert_eq!(
            pick_staged_file_name("Cool Mod", Some("cool mod"), None),
            None
        );
    }

    #[test]
    fn remember_file_name_fills_a_row_saved_without_one() {
        let (tmp, paths) = test_paths();
        let game_id = "file_name_backfill";
        let archive = tmp.path().join("mod.zip");
        write_zip(&archive, "readme.txt", b"v1");
        let staged = stage_mod(
            &paths,
            game_id,
            "Cool Mod",
            "skyrim",
            1,
            10,
            Some("1.0".into()),
            None,
            &archive,
        )
        .unwrap();
        assert!(staged.file_name.is_none());
        assert!(
            remember_file_name_if_missing(&paths, game_id, &staged.id, "Optional patch").unwrap()
        );
        let order = load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods[0].file_name.as_deref(), Some("Optional patch"));
        assert!(!remember_file_name_if_missing(&paths, game_id, &staged.id, "other.zip").unwrap());
        let order = load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods[0].file_name.as_deref(), Some("Optional patch"));
    }

    #[test]
    fn nexus_same_file_no_update() {
        let files = vec![nexus_file(10, "1.0.0", "MAIN", 100, true)];
        assert!(nexus_update_candidate(10, Some("1.0.0"), &files).is_none());
    }

    #[test]
    fn nexus_newer_same_category_is_update() {
        let files = vec![
            nexus_file(10, "1.0.0", "MAIN", 100, false),
            nexus_file(20, "1.1.0", "MAIN", 200, true),
        ];
        let c = nexus_update_candidate(10, Some("1.0.0"), &files).unwrap();
        assert_eq!(c.file_id, 20);
        assert_eq!(c.version.as_deref(), Some("1.1.0"));
    }

    #[test]
    fn nexus_optional_not_replaced_by_main() {
        let files = vec![
            nexus_file(10, "1.0.0", "OPTIONAL", 100, false),
            nexus_file(20, "2.0.0", "MAIN", 200, true),
            nexus_file(11, "1.0.1", "OPTIONAL", 150, false),
        ];
        let c = nexus_update_candidate(10, Some("1.0.0"), &files).unwrap();
        assert_eq!(c.file_id, 11);
    }

    #[test]
    fn nexus_missing_staged_falls_back_to_primary() {
        let files = vec![
            nexus_file(20, "2.0.0", "MAIN", 200, true),
            nexus_file(30, "1.0.0", "OPTIONAL", 300, false),
        ];
        let c = nexus_update_candidate(99, Some("1.0.0"), &files).unwrap();
        assert_eq!(c.file_id, 20);
    }

    #[test]
    fn nexus_older_version_with_newer_upload_is_not_an_update() {
        let files = vec![
            nexus_file(10, "2.0.0", "MAIN", 100, true),
            nexus_file(20, "1.0.0", "MAIN", 300, false),
        ];
        assert!(nexus_update_candidate(10, Some("2.0.0"), &files).is_none());
    }

    #[test]
    fn nexus_same_version_reupload_is_an_update() {
        let files = vec![
            nexus_file(10, "1.0.0", "MAIN", 100, true),
            nexus_file(20, "1.0.0", "MAIN", 200, false),
        ];
        let c = nexus_update_candidate(10, Some("1.0.0"), &files).unwrap();
        assert_eq!(c.file_id, 20);
    }

    #[test]
    fn file_variant_key_strips_version_tokens() {
        assert_eq!(file_variant_key("BepInEx"), "bepinex");
        assert_eq!(file_variant_key("BepInEx 1.2"), "bepinex");
        assert_eq!(file_variant_key("Cool Mod v1.2.3"), "cool mod");
        assert_eq!(file_variant_key("Pack 1-0-7"), "pack");
        assert_eq!(file_variant_key("TweakXL 1.10.6a"), "tweakxl");
        assert_eq!(file_variant_key("Codeware 1.14.0-rc1"), "codeware");
        assert_eq!(file_variant_key("Codeware 1.14.0-beta2"), "codeware");
        assert_eq!(file_variant_key("BepInEx 5"), "bepinex 5");
        assert_eq!(file_variant_key("MelonLoader"), "melonloader");
        assert_eq!(file_variant_key(""), "");
    }

    #[test]
    fn nexus_sibling_main_is_not_an_update() {
        let files = vec![
            named(nexus_file(10, "1.0.0", "MAIN", 100, false), "BepInEx"),
            named(nexus_file(20, "3.0.0", "MAIN", 300, true), "MelonLoader"),
        ];
        assert!(nexus_update_candidate(10, Some("1.0.0"), &files).is_none());
    }

    #[test]
    fn nexus_same_title_update_ignores_newer_sibling() {
        let files = vec![
            named(nexus_file(10, "1.0.0", "MAIN", 100, false), "BepInEx"),
            named(nexus_file(20, "1.2.0", "MAIN", 200, false), "BepInEx 1.2"),
            named(nexus_file(30, "3.0.0", "MAIN", 400, true), "MelonLoader"),
        ];
        let c = nexus_update_candidate(10, Some("1.0.0"), &files).unwrap();
        assert_eq!(c.file_id, 20);
        assert_eq!(c.version.as_deref(), Some("1.2.0"));
    }

    #[test]
    fn nexus_old_version_or_archived_staged_updates_to_main() {
        let mut archived_null = named(nexus_file(10, "1.10.6", "OLD_VERSION", 100, false), "TweakXL");
        archived_null.category_name = None;

        for old in [
            named(nexus_file(10, "1.10.6", "OLD_VERSION", 100, false), "TweakXL"),
            named(nexus_file(10, "1.10.6", "ARCHIVED", 100, false), "TweakXL"),
            archived_null,
        ] {
            let files = vec![
                old,
                named(nexus_file(20, "1.11.0", "MAIN", 200, true), "TweakXL"),
                named(nexus_file(15, "1.10.7", "OLD_VERSION", 150, false), "TweakXL"),
            ];
            let c = nexus_update_candidate(10, Some("1.10.6"), &files).unwrap();
            assert_eq!(c.file_id, 20);
            assert_eq!(c.version.as_deref(), Some("1.11.0"));
        }
    }

    #[test]
    fn nexus_renamed_old_version_matches_main_by_prefix() {
        let files = vec![
            named(
                nexus_file(10, "1.13.0", "OLD_VERSION", 100, false),
                "Codeware (for Patch 2.13)",
            ),
            named(nexus_file(20, "1.14.0", "MAIN", 200, true), "Codeware"),
        ];
        let c = nexus_update_candidate(10, Some("1.13.0"), &files).unwrap();
        assert_eq!(c.file_id, 20);
        assert_eq!(c.version.as_deref(), Some("1.14.0"));
    }

    #[test]
    fn nexus_old_version_multi_variant_and_retired_optional_do_not_cross_update() {
        let multi_variant = vec![
            named(nexus_file(10, "1.0.0", "OLD_VERSION", 100, false), "BepInEx"),
            named(nexus_file(20, "1.2.0", "MAIN", 200, false), "BepInEx 1.2"),
            named(nexus_file(30, "3.0.0", "MAIN", 400, true), "MelonLoader"),
        ];
        let c = nexus_update_candidate(10, Some("1.0.0"), &multi_variant).unwrap();
        assert_eq!(c.file_id, 20);

        let retired_optional = vec![
            named(
                nexus_file(10, "1.0.0", "OLD_VERSION", 100, false),
                "Optional 4K Textures",
            ),
            named(
                nexus_file(20, "2.0.0", "MAIN", 200, true),
                "Cyberpunk Overhaul",
            ),
        ];
        assert!(nexus_update_candidate(10, Some("1.0.0"), &retired_optional).is_none());
    }

    #[test]
    fn nexus_newer_backport_upload_does_not_shadow_higher_version() {
        let files = vec![
            named(nexus_file(10, "1.10.8", "OLD_VERSION", 100, false), "TweakXL"),
            named(nexus_file(20, "1.11.0", "MAIN", 200, true), "TweakXL"),
            named(nexus_file(30, "1.10.7", "MAIN", 300, false), "TweakXL"),
        ];
        let c = nexus_update_candidate(10, Some("1.10.8"), &files).unwrap();
        assert_eq!(c.file_id, 20);
        assert_eq!(c.version.as_deref(), Some("1.11.0"));
    }

    #[test]
    fn nexus_missing_staged_ignores_historical_files() {
        let files = vec![
            named(nexus_file(20, "2.0.0", "OLD_VERSION", 200, false), "TweakXL"),
            named(nexus_file(30, "3.0.0", "ARCHIVED", 300, false), "TweakXL"),
        ];
        assert!(nexus_update_candidate(99, Some("1.0.0"), &files).is_none());
    }

    #[test]
    fn keep_existing_modio_file_replaces_a_different_file_without_a_version() {
        assert!(keep_existing_modio_file(true, Some("1.0"), None));
        assert!(!keep_existing_modio_file(false, Some("1.0"), None));
        assert!(keep_existing_modio_file(false, Some("2.0"), Some("1.0")));
        assert!(!keep_existing_modio_file(false, Some("1.0"), Some("2.0")));
    }

    fn write_zip(path: &Path, entry: &str, bytes: &[u8]) {
        let file = std::fs::File::create(path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        zip.start_file::<_, ()>(entry, zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut zip, bytes).unwrap();
        zip.finish().unwrap();
    }

    #[test]
    fn nexus_update_replaces_one_row_and_rewrites_references() {
        let (tmp, paths) = test_paths();
        let game_id = "nexus_update";
        let archive = tmp.path().join("mod.zip");
        write_zip(&archive, "readme.txt", b"v1");
        let staged = stage_mod(
            &paths,
            game_id,
            "Mod",
            "skyrim",
            1,
            10,
            Some("1.0".into()),
            Some("Optional patch".into()),
            &archive,
        )
        .unwrap();
        assert_eq!(staged.id, "1_10");
        assert_eq!(staged.file_name.as_deref(), Some("Optional patch"));

        let mut order = load_loadorder(&paths, game_id).unwrap();
        order.mods.push(StagedMod {
            id: "other".into(),
            name: "Other".into(),
            domain: "skyrim".into(),
            staging_path: paths
                .mods_dir(game_id)
                .join("other")
                .to_string_lossy()
                .into(),
            enabled: true,
            order: 2,
            depends_on: vec!["1_10".into()],
            ..Default::default()
        });
        order.mods.push(StagedMod {
            id: "1_20".into(),
            name: "Mod copy".into(),
            source: ModSource::Nexus,
            nexus_mod_id: 1,
            nexus_file_id: 20,
            domain: "skyrim".into(),
            staging_path: paths
                .mods_dir(game_id)
                .join("Mod_1_20")
                .to_string_lossy()
                .into(),
            enabled: true,
            order: 3,
            ..Default::default()
        });
        save_loadorder(&paths, game_id, &order).unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: "col".into(),
                    source: CollectionSource::Nexus,
                    kind: CollectionKind::Collection,
                    name: "Col".into(),
                    slug: None,
                    namespace: None,
                    package_name: None,
                    community: None,
                    revision: None,
                    version: None,
                    profile_code: None,
                    mod_ids: vec!["1_10".into(), "1_20".into()],
                    installed_at: "t".into(),
                    game_versions: None,
                    friend_sync: None, ..Default::default()
                }],
            },
        )
        .unwrap();

        write_zip(&archive, "readme.txt", b"v2");
        let updated = stage_mod_replacing(
            &paths,
            game_id,
            "1_10",
            "Mod",
            "skyrim",
            1,
            20,
            Some("2.0".into()),
            Some("Cool Mod-1-2-0.zip".into()),
            &archive,
        )
        .unwrap();
        assert_eq!(updated.id, "1_20");
        assert_eq!(updated.version.as_deref(), Some("2.0"));
        assert_eq!(updated.file_name.as_deref(), Some("Cool Mod-1-2-0.zip"));
        assert_eq!(updated.order, staged.order);

        let order = load_loadorder(&paths, game_id).unwrap();
        let nexus: Vec<_> = order.mods.iter().filter(|m| m.nexus_mod_id == 1).collect();
        assert_eq!(nexus.len(), 1);
        assert_eq!(nexus[0].nexus_file_id, 20);
        let other = order.mods.iter().find(|m| m.id == "other").unwrap();
        assert_eq!(other.depends_on, vec!["1_20".to_string()]);
        let collections = load_collections(&paths, game_id).unwrap();
        assert_eq!(collections.collections[0].mod_ids, vec!["1_20".to_string()]);
    }

    #[test]
    fn load_collapses_duplicate_ids_and_keeps_distinct_files() {
        let (_tmp, paths) = test_paths();
        let game_id = "dup_rows";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let raw = r#"{
            "mods": [
                {"id":"1_10","name":"Old","version":"1","domain":"skyrim","staging_path":"/no/old","enabled":true,"order":1,"source":"nexus","nexus_mod_id":1,"nexus_file_id":10},
                {"id":"1_11","name":"Optional","version":"1","domain":"skyrim","staging_path":"/no/opt","enabled":true,"order":2,"source":"nexus","nexus_mod_id":1,"nexus_file_id":11},
                {"id":"1_10","name":"New","version":"2","domain":"skyrim","staging_path":"/no/new","enabled":true,"order":3,"source":"nexus","nexus_mod_id":1,"nexus_file_id":10}
            ]
        }"#;
        std::fs::write(paths.loadorder_file(game_id), raw).unwrap();
        let order = load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods.len(), 2);
        assert_eq!(order.mods[0].name, "Optional");
        assert_eq!(order.mods[1].name, "New");
        assert!(order.mods.iter().all(|m| m.picture_url.is_none()));
    }

    #[test]
    fn modio_update_without_version_replaces_the_existing_row() {
        let (tmp, paths) = test_paths();
        let game_id = "modio_update";
        let archive = tmp.path().join("mod.zip");
        write_zip(&archive, "plugin.dll", b"old");
        stage_modio_mod(
            &paths,
            game_id,
            7,
            42,
            1,
            "Widget",
            Some("1.0".into()),
            Some("widget-1.zip".into()),
            Some("https://cdn.example/widget.png".into()),
            &archive,
        )
        .unwrap();
        write_zip(&archive, "plugin.dll", b"new");
        let updated = stage_modio_mod(
            &paths, game_id, 7, 42, 2, "Widget", None, None, None, &archive,
        )
        .unwrap();
        assert_eq!(updated.modio_file_id, Some(2));
        assert!(updated.version.is_none());
        assert_eq!(
            updated.picture_url.as_deref(),
            Some("https://cdn.example/widget.png")
        );
        let order = load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods.len(), 1);
        assert_eq!(order.mods[0].id, "modio_7_42");
        assert_eq!(order.mods[0].modio_file_id, Some(2));
        assert_eq!(
            order.mods[0].picture_url.as_deref(),
            Some("https://cdn.example/widget.png")
        );
    }

    #[test]
    fn apply_staged_pictures_fills_only_missing_rows() {
        let (_tmp, paths) = test_paths();
        let game_id = "posters";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let order = LoadOrder {
            mods: vec![
                StagedMod {
                    id: "known".into(),
                    name: "Known".into(),
                    order: 1,
                    picture_url: Some("https://cdn.example/kept.png".into()),
                    ..Default::default()
                },
                StagedMod {
                    id: "missing".into(),
                    name: "Missing".into(),
                    order: 2,
                    ..Default::default()
                },
            ],
        };
        save_loadorder(&paths, game_id, &order).unwrap();
        let mut pictures = HashMap::new();
        pictures.insert("known".into(), "https://cdn.example/new.png".into());
        pictures.insert("missing".into(), String::new());
        let mods = apply_staged_pictures(&paths, game_id, &pictures).unwrap();
        assert_eq!(
            mods.iter()
                .find(|m| m.id == "known")
                .unwrap()
                .picture_url
                .as_deref(),
            Some("https://cdn.example/kept.png")
        );
        assert_eq!(
            mods.iter()
                .find(|m| m.id == "missing")
                .unwrap()
                .picture_url
                .as_deref(),
            Some("")
        );
    }

    #[test]
    fn uninstall_keeps_shared_requirements() {
        let (_tmp, paths) = test_paths();
        let game_id = "col_test";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        for id in ["A", "B", "Z", "W"] {
            std::fs::create_dir_all(staging_root.join(id)).unwrap();
        }
        let staged =
            |id: &str, independent: bool, depends_on: Vec<String>, collection_ids: Vec<String>| {
                StagedMod {
                    id: id.into(),
                    name: id.into(),
                    staging_path: staging_root.join(id).to_string_lossy().into(),
                    independent,
                    depends_on,
                    collection_ids,
                    ..Default::default()
                }
            };
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    staged("A", false, vec!["Z".into()], vec!["col-a".into()]),
                    staged("Z", false, vec![], vec!["col-a".into()]),
                    staged("W", true, vec!["Z".into()], vec![]),
                    staged("B", false, vec![], vec!["col-b".into()]),
                ],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![
                    InstalledCollection {
                        id: "col-a".into(),
                        source: CollectionSource::Nexus,
                        kind: CollectionKind::Collection,
                        name: "A".into(),
                        slug: Some("a".into()),
                        namespace: None,
                        package_name: None,
                        community: None,
                        revision: None,
                        version: None,
                        profile_code: None,
                        mod_ids: vec!["A".into(), "Z".into()],
                        installed_at: "t".into(),
                        game_versions: None,
                        friend_sync: None, ..Default::default()
                    },
                    InstalledCollection {
                        id: "col-b".into(),
                        source: CollectionSource::Nexus,
                        kind: CollectionKind::Collection,
                        name: "B".into(),
                        slug: Some("b".into()),
                        namespace: None,
                        package_name: None,
                        community: None,
                        revision: None,
                        version: None,
                        profile_code: None,
                        mod_ids: vec!["B".into()],
                        installed_at: "t".into(),
                        game_versions: None,
                        friend_sync: None, ..Default::default()
                    },
                ],
            },
        )
        .unwrap();

        let removed = uninstall_collection(&paths, game_id, "col-a").unwrap();
        assert_eq!(removed, 1);
        let order = load_loadorder(&paths, game_id).unwrap();
        let ids: Vec<_> = order.mods.iter().map(|m| m.id.as_str()).collect();
        assert!(ids.contains(&"Z"));
        assert!(ids.contains(&"W"));
        assert!(ids.contains(&"B"));
        assert!(!ids.contains(&"A"));
    }

    #[test]
    fn forget_installed_collection_orphans_mods_and_keeps_staging() {
        let (_tmp, paths) = test_paths();
        let game_id = "forget_col_test";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        for id in ["A", "Z"] {
            std::fs::create_dir_all(staging_root.join(id)).unwrap();
        }
        let staged = |id: &str, independent: bool, collection_ids: Vec<String>| StagedMod {
            id: id.into(),
            name: id.into(),
            staging_path: staging_root.join(id).to_string_lossy().into(),
            independent,
            collection_ids,
            ..Default::default()
        };
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    staged("A", false, vec!["col-a".into()]),
                    staged("Z", false, vec!["col-a".into()]),
                ],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: "col-a".into(),
                    source: CollectionSource::Emperor,
                    kind: CollectionKind::Share,
                    name: "Share".into(),
                    slug: None,
                    namespace: None,
                    package_name: None,
                    community: None,
                    revision: None,
                    version: None,
                    profile_code: Some("#emperor1abc".into()),
                    mod_ids: vec!["A".into(), "Z".into()],
                    installed_at: "t".into(),
                    game_versions: None,
                    friend_sync: None, ..Default::default()
                }],
            },
        )
        .unwrap();

        forget_installed_collection(&paths, game_id, "col-a").unwrap();
        forget_installed_collection(&paths, game_id, "missing").unwrap();

        let installed = list_installed_collections(&paths, game_id).unwrap();
        assert!(installed.is_empty());
        let order = load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods.len(), 2);
        for m in &order.mods {
            assert!(m.collection_ids.is_empty(), "{}", m.id);
            assert!(m.independent, "{}", m.id);
            assert!(
                PathBuf::from(&m.staging_path).exists(),
                "staging should remain for {}",
                m.id
            );
        }
    }

    #[test]
    fn import_folder_stages_an_extracted_tree() {
        let (tmp, paths) = test_paths();
        let folder = tmp.path().join("Cool Mod");
        fs::create_dir_all(folder.join("data")).unwrap();
        fs::create_dir_all(folder.join("nested")).unwrap();
        fs::write(folder.join("data").join("mod.dll"), b"abc").unwrap();
        fs::write(folder.join("nested").join("inner.zip"), zip_bytes()).unwrap();
        fs::write(folder.join(".secret"), b"no").unwrap();

        let result = import_folder(&paths, "game", &folder, "skyrim", None).unwrap();

        assert!(result.errors.is_empty());
        assert_eq!(result.mods.len(), 1);
        assert_eq!(result.mods[0].name, "Cool Mod");
        assert_eq!(result.mods[0].nexus_mod_id, 0);
        assert!(result.mods[0].enabled);
        assert!(result.mods[0].independent);
        let staged = PathBuf::from(&result.mods[0].staging_path);
        assert_eq!(
            fs::read(staged.join("data").join("mod.dll")).unwrap(),
            b"abc"
        );
        assert!(staged.join("nested").join("inner.zip").is_file());
        assert!(!staged.join(".secret").exists());
        assert_eq!(
            fs::read(folder.join("data").join("mod.dll")).unwrap(),
            b"abc"
        );
    }

    #[test]
    fn import_folder_stages_each_archive() {
        let (tmp, paths) = test_paths();
        let folder = tmp.path().join("downloads");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("One.zip"), zip_bytes()).unwrap();
        fs::write(folder.join("Two.ZIP"), zip_bytes()).unwrap();
        fs::write(folder.join("Bad.rar"), b"not a rar").unwrap();

        let result = import_folder(&paths, "game", &folder, "skyrim", None).unwrap();

        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("Bad.rar"));
        assert_eq!(result.mods.len(), 2);
        let mut names: Vec<_> = result
            .mods
            .iter()
            .map(|staged| staged.name.as_str())
            .collect();
        names.sort();
        assert_eq!(names, vec!["One", "Two"]);
        assert_ne!(result.mods[0].id, result.mods[1].id);
        for staged in &result.mods {
            let path = PathBuf::from(&staged.staging_path);
            assert_eq!(fs::read(path.join("readme.txt")).unwrap(), b"hello");
        }
        assert!(folder.join("One.zip").is_file());
    }

    #[test]
    fn import_folder_splits_archive_from_extracted_files() {
        let (tmp, paths) = test_paths();
        let folder = tmp.path().join("Mixed");
        fs::create_dir_all(folder.join("nested")).unwrap();
        fs::write(folder.join("nested").join("mod.dll"), b"dll").unwrap();
        fs::write(folder.join("Extra.zip"), zip_bytes()).unwrap();
        fs::write(folder.join("desktop.ini"), b"junk").unwrap();

        let result = import_folder(&paths, "game", &folder, "skyrim", None).unwrap();

        assert!(result.errors.is_empty());
        assert_eq!(result.mods.len(), 2);
        let archive = result
            .mods
            .iter()
            .find(|staged| staged.name == "Extra")
            .unwrap();
        let extracted = result
            .mods
            .iter()
            .find(|staged| staged.name == "Mixed")
            .unwrap();
        assert!(PathBuf::from(&archive.staging_path)
            .join("readme.txt")
            .is_file());
        let staged = PathBuf::from(&extracted.staging_path);
        assert_eq!(
            fs::read(staged.join("nested").join("mod.dll")).unwrap(),
            b"dll"
        );
        assert!(!staged.join("Extra.zip").exists());
        assert!(!staged.join("desktop.ini").exists());
    }

    #[test]
    fn import_folder_keeps_package_files_together() {
        let (tmp, paths) = test_paths();
        let folder = tmp.path().join("Household");
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("a.package"), b"p").unwrap();
        fs::write(folder.join("b.PACKAGE"), b"q").unwrap();

        let result = import_folder(&paths, "game", &folder, "skyrim", None).unwrap();

        assert!(result.errors.is_empty());
        assert_eq!(result.mods.len(), 1);
        assert_eq!(result.mods[0].name, "Household");
        let staged = PathBuf::from(&result.mods[0].staging_path);
        assert_eq!(fs::read(staged.join("a.package")).unwrap(), b"p");
        assert_eq!(fs::read(staged.join("b.PACKAGE")).unwrap(), b"q");
    }

    #[test]
    fn import_folder_rejects_an_empty_folder() {
        let (tmp, paths) = test_paths();
        let folder = tmp.path().join("empty");
        fs::create_dir_all(&folder).unwrap();
        let err = import_folder(&paths, "game", &folder, "skyrim", None).unwrap_err();
        assert!(err.to_string().contains("empty"), "{err}");

        let junk = tmp.path().join("junk-only");
        fs::create_dir_all(&junk).unwrap();
        fs::write(junk.join("desktop.ini"), b"x").unwrap();
        fs::write(junk.join(".hidden"), b"x").unwrap();
        let err = import_folder(&paths, "game", &junk, "skyrim", None).unwrap_err();
        assert!(err.to_string().contains("empty"), "{err}");
    }

    #[test]
    fn import_folder_rejects_the_install_and_staging_dirs() {
        let (tmp, paths) = test_paths();
        let install = tmp.path().join("install");
        fs::create_dir_all(&install).unwrap();
        fs::write(install.join("game.exe"), b"x").unwrap();
        let err =
            import_folder(&paths, "game", &install, "skyrim", Some(install.as_path())).unwrap_err();
        assert!(err.to_string().contains("install"), "{err}");

        let staging = paths.mods_dir("game");
        fs::create_dir_all(&staging).unwrap();
        fs::write(staging.join("One.zip"), zip_bytes()).unwrap();
        let err = import_folder(&paths, "game", &staging, "skyrim", None).unwrap_err();
        assert!(err.to_string().contains("staging"), "{err}");
    }

    #[test]
    fn import_paths_stages_mixed_files_and_folders() {
        let (tmp, paths) = test_paths();
        let archive = tmp.path().join("Solo.zip");
        fs::write(&archive, zip_bytes()).unwrap();
        let loose = tmp.path().join("Loose.package");
        fs::write(&loose, b"pkg").unwrap();
        let folder = tmp.path().join("ExtractedMod");
        fs::create_dir_all(folder.join("data")).unwrap();
        fs::write(folder.join("data").join("plugin.dll"), b"dll").unwrap();
        let missing = tmp.path().join("does-not-exist.zip");

        let result = import_paths(
            &paths,
            "game",
            &[archive, loose, folder, missing],
            "skyrim",
            None,
        )
        .unwrap();

        assert_eq!(result.errors.len(), 1);
        assert!(result.errors[0].contains("does-not-exist.zip"));
        assert_eq!(result.mods.len(), 3);
        let mut names: Vec<_> = result.mods.iter().map(|m| m.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["ExtractedMod", "Loose", "Solo"]);
        let ids: HashSet<_> = result.mods.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), 3);
    }

    #[test]
    fn import_paths_rejects_empty_selection() {
        let (_tmp, paths) = test_paths();
        let err = import_paths(&paths, "game", &[], "skyrim", None).unwrap_err();
        assert!(err.to_string().contains("No files or folders"), "{err}");
    }

    /// Live check: redeploy managed Risk of Rain 2 and ensure RoR2BepInExPack
    /// ships Newtonsoft.Json. Opt-in via `EMPEROR_VERIFY_ROR2=1`.
    #[test]
    #[ignore = "writes to the live RoR2 install; set EMPEROR_VERIFY_ROR2=1"]
    fn verify_live_ror2_deploy_includes_newtonsoft() {
        if std::env::var_os("EMPEROR_VERIFY_ROR2").is_none() {
            return;
        }
        let paths = Paths::resolve().expect("resolve paths");
        let cfg = crate::config::load_config(&paths).expect("load config");
        let game = cfg
            .managed_games
            .iter()
            .find(|g| g.plugin_id == "riskofrain2")
            .expect("managed Risk of Rain 2");
        let install = std::path::Path::new(&game.install_path);
        let result = deploy(&paths, &game.id, "riskofrain2", install, None).expect("deploy");
        eprintln!(
            "deployed {} files from {} mods; warnings={:?}",
            result.file_count, result.enabled_mods, result.warnings
        );
        let safe = install
            .join("BepInEx")
            .join("plugins")
            .join("RiskofThunder-RoR2BepInExPack");
        let newtonsoft = safe.join("Newtonsoft.Json.dll");
        assert!(
            newtonsoft.is_file(),
            "expected {} after deploy",
            newtonsoft.display()
        );
        assert!(
            !install
                .join("BepInEx")
                .join("plugins")
                .join("RoR2BepInExPack")
                .exists(),
            "must not leave files in the Preloader-pruned folder"
        );
        let dll = safe.join("RoR2BepInExPack.dll");
        let meta = fs::metadata(&dll).expect("RoR2BepInExPack.dll metadata");
        assert!(
            meta.len() > 100_000,
            "expected RoR2BepInExPack 1.50+ (got {} bytes)",
            meta.len()
        );
    }

    #[test]
    fn apply_collection_order_and_selections_preserves_other_mods() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game_id = "skyrim_sync_test";
        let staged = |id: &str, ord: u32| StagedMod {
            id: id.into(),
            name: id.into(),
            domain: "skyrim".into(),
            staging_path: paths.mods_dir(game_id).join(id).to_string_lossy().into(),
            enabled: true,
            order: ord,
            ..Default::default()
        };
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![staged("local_top", 1), staged("a", 2), staged("b", 3), staged("local_bottom", 4)],
            },
        )
        .unwrap();

        let mut selected = std::collections::BTreeMap::new();
        selected.insert("g1".into(), vec!["p2".into()]);
        let fomod = FomodSelection { selected };

        apply_collection_order_and_selections(
            &paths,
            game_id,
            &[
                SyncedModSelection {
                    mod_id: "b".into(),
                    fomod_selection: Some(fomod.clone()),
                    option_selection: None,
                    enabled: None,
                },
                SyncedModSelection {
                    mod_id: "a".into(),
                    fomod_selection: None,
                    option_selection: None,
                    enabled: None,
                },
            ],
        )
        .unwrap();

        let after = load_loadorder(&paths, game_id).unwrap();
        let ids: Vec<_> = after.mods.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids, ["local_top", "b", "a", "local_bottom"]);
        assert_eq!(after.mods[1].fomod_selection, Some(fomod));
    }

    #[test]
    fn check_collection_local_integrity_reports_missing_and_corrupted_mods() {
        let dir = tempfile::tempdir().unwrap();
        let intact_dir = dir.path().join("intact");
        fs::create_dir_all(&intact_dir).unwrap();
        fs::write(intact_dir.join("a.dll"), b"ok").unwrap();
        fs::write(intact_dir.join("b.dll"), b"ok").unwrap();

        let damaged_dir = dir.path().join("damaged");
        fs::create_dir_all(&damaged_dir).unwrap();
        fs::write(damaged_dir.join("only_one.dll"), b"partial").unwrap();

        let empty_dir = dir.path().join("empty");
        fs::create_dir_all(&empty_dir).unwrap();

        let missing_dir = dir.path().join("missing_on_disk");

        let order = LoadOrder {
            mods: vec![
                StagedMod {
                    id: "10_1".into(),
                    name: "Intact Nexus Mod".into(),
                    source: ModSource::Nexus,
                    nexus_mod_id: 10,
                    nexus_file_id: 1,
                    domain: "skyrimspecialedition".into(),
                    staging_path: intact_dir.to_string_lossy().into(),
                    staged_file_count: Some(2),
                    ..Default::default()
                },
                StagedMod {
                    id: "ts_Owner_Damaged".into(),
                    name: "Damaged TS Mod".into(),
                    source: ModSource::Thunderstore,
                    domain: "riskofrain2".into(),
                    ts_namespace: Some("Owner".into()),
                    ts_name: Some("Damaged".into()),
                    staging_path: damaged_dir.to_string_lossy().into(),
                    staged_file_count: Some(3),
                    ..Default::default()
                },
                StagedMod {
                    id: "ts_Owner_Empty".into(),
                    name: "Empty TS Mod".into(),
                    source: ModSource::Thunderstore,
                    domain: "riskofrain2".into(),
                    ts_namespace: Some("Owner".into()),
                    ts_name: Some("Empty".into()),
                    staging_path: empty_dir.to_string_lossy().into(),
                    staged_file_count: None,
                    ..Default::default()
                },
                StagedMod {
                    id: "20_2".into(),
                    name: "Missing Dir Mod".into(),
                    source: ModSource::Nexus,
                    nexus_mod_id: 20,
                    nexus_file_id: 2,
                    domain: "skyrimspecialedition".into(),
                    staging_path: missing_dir.to_string_lossy().into(),
                    staged_file_count: Some(1),
                    ..Default::default()
                },
            ],
        };

        let collection = InstalledCollection {
            id: "nexus:test".into(),
            source: CollectionSource::Nexus,
            kind: CollectionKind::Collection,
            name: "Test Collection".into(),
            slug: Some("test".into()),
            namespace: None,
            package_name: None,
            community: None,
            revision: Some(1),
            version: None,
            profile_code: None,
            mod_ids: vec![
                "10_1".into(),
                "ts_Owner_Damaged".into(),
                "ts_Owner_Empty".into(),
                "20_2".into(),
                "999_9".into(),
            ],
            installed_at: String::new(),
            game_versions: None,
            friend_sync: None, ..Default::default()
        };

        let report = check_collection_local_integrity(&collection, &order);
        assert_eq!(report.checked_count, 5);
        assert_eq!(report.issues.len(), 4);
        assert_eq!(report.issues[0].mod_id, "ts_Owner_Damaged");
        assert_eq!(
            report.issues[0].issue,
            CollectionModIssueKind::CorruptedStaging {
                actual_files: 1,
                expected_files: Some(3)
            }
        );
        assert!(report.issues[0].can_retry);

        assert_eq!(report.issues[1].mod_id, "ts_Owner_Empty");
        assert_eq!(
            report.issues[1].issue,
            CollectionModIssueKind::CorruptedStaging {
                actual_files: 0,
                expected_files: None
            }
        );
        assert!(report.issues[1].can_retry);

        assert_eq!(report.issues[2].mod_id, "20_2");
        assert_eq!(report.issues[2].issue, CollectionModIssueKind::MissingStagingDir);
        assert!(report.issues[2].can_retry);

        assert_eq!(report.issues[3].mod_id, "999_9");
        assert_eq!(
            report.issues[3].issue,
            CollectionModIssueKind::MissingFromLoadOrder
        );
        assert!(!report.issues[3].can_retry);
    }

    #[test]
    fn set_mod_collections_moves_and_shares_between_collections() {
        let (_dir, paths) = test_paths();
        let game_id = "game-move";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        for id in ["A", "B"] {
            std::fs::create_dir_all(staging_root.join(id)).unwrap();
        }
        let staged = |id: &str, independent: bool, collection_ids: Vec<String>| StagedMod {
            id: id.into(),
            name: id.into(),
            staging_path: staging_root.join(id).to_string_lossy().into(),
            independent,
            collection_ids,
            ..Default::default()
        };
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    staged("A", false, vec!["col-a".into()]),
                    staged("B", false, vec!["col-b".into()]),
                ],
            },
        )
        .unwrap();
        let make_col = |id: &str, mod_ids: Vec<&str>| InstalledCollection {
            id: id.into(),
            source: CollectionSource::Nexus,
            kind: CollectionKind::Collection,
            name: id.into(),
            slug: None,
            namespace: None,
            package_name: None,
            community: None,
            revision: None,
            version: None,
            profile_code: None,
            mod_ids: mod_ids.into_iter().map(Into::into).collect(),
            installed_at: "t".into(),
            game_versions: None,
            friend_sync: None, ..Default::default()
        };
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![make_col("col-a", vec!["A"]), make_col("col-b", vec!["B"])],
            },
        )
        .unwrap();

        // Move A from col-a to col-b.
        let updated = set_mod_collections(&paths, game_id, "A", &["col-b".into()]).unwrap();
        assert_eq!(updated.collection_ids, vec!["col-b".to_string()]);
        assert!(!updated.independent);

        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert!(cols[0].mod_ids.is_empty());
        assert_eq!(cols[1].mod_ids, vec!["B".to_string(), "A".to_string()]);

        // Assign A to both col-a and col-b (deduplicating repeated ids).
        let updated = set_mod_collections(
            &paths,
            game_id,
            "A",
            &["col-a".into(), "col-b".into(), "col-a".into()],
        )
        .unwrap();
        assert_eq!(
            updated.collection_ids,
            vec!["col-a".to_string(), "col-b".to_string()]
        );
        assert!(!updated.independent);
        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert_eq!(cols[0].mod_ids, vec!["A".to_string()]);
        assert_eq!(cols[1].mod_ids, vec!["B".to_string(), "A".to_string()]);
    }

    #[test]
    fn set_mod_collections_to_empty_marks_mod_independent() {
        let (_dir, paths) = test_paths();
        let game_id = "game-unassign";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        std::fs::create_dir_all(staging_root.join("A")).unwrap();
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "A".into(),
                    name: "A".into(),
                    staging_path: staging_root.join("A").to_string_lossy().into(),
                    independent: false,
                    collection_ids: vec!["col-a".into()],
                    ..Default::default()
                }],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: "col-a".into(),
                    source: CollectionSource::Nexus,
                    kind: CollectionKind::Collection,
                    name: "A".into(),
                    slug: None,
                    namespace: None,
                    package_name: None,
                    community: None,
                    revision: None,
                    version: None,
                    profile_code: None,
                    mod_ids: vec!["A".into()],
                    installed_at: "t".into(),
                    game_versions: None,
                    friend_sync: None, ..Default::default()
                }],
            },
        )
        .unwrap();

        let updated = set_mod_collections(&paths, game_id, "A", &[]).unwrap();
        assert!(updated.collection_ids.is_empty());
        assert!(updated.independent);
        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert!(cols[0].mod_ids.is_empty());
    }

    #[test]
    fn set_mod_collections_rejects_unknown_collection_or_mod() {
        let (_dir, paths) = test_paths();
        let game_id = "game-err";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        std::fs::create_dir_all(staging_root.join("A")).unwrap();
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "A".into(),
                    name: "A".into(),
                    staging_path: staging_root.join("A").to_string_lossy().into(),
                    independent: false,
                    collection_ids: vec!["col-a".into()],
                    ..Default::default()
                }],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: "col-a".into(),
                    source: CollectionSource::Nexus,
                    kind: CollectionKind::Collection,
                    name: "A".into(),
                    slug: None,
                    namespace: None,
                    package_name: None,
                    community: None,
                    revision: None,
                    version: None,
                    profile_code: None,
                    mod_ids: vec!["A".into()],
                    installed_at: "t".into(),
                    game_versions: None,
                    friend_sync: None, ..Default::default()
                }],
            },
        )
        .unwrap();

        assert!(set_mod_collections(&paths, game_id, "A", &["missing-col".into()]).is_err());
        assert!(set_mod_collections(&paths, game_id, "missing-mod", &["col-a".into()]).is_err());

        let order = load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods[0].collection_ids, vec!["col-a".to_string()]);
        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert_eq!(cols[0].mod_ids, vec!["A".to_string()]);
    }

    #[test]
    fn set_mod_collections_rejects_adding_mod_to_synced_collection() {
        let (_dir, paths) = test_paths();
        let game_id = "game-synced-reject";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        for id in ["A", "B"] {
            std::fs::create_dir_all(staging_root.join(id)).unwrap();
        }
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    StagedMod {
                        id: "A".into(),
                        name: "A".into(),
                        staging_path: staging_root.join("A").to_string_lossy().into(),
                        independent: true,
                        collection_ids: vec![],
                        ..Default::default()
                    },
                    StagedMod {
                        id: "B".into(),
                        name: "B".into(),
                        staging_path: staging_root.join("B").to_string_lossy().into(),
                        independent: false,
                        collection_ids: vec!["col-local".into()],
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![
                    InstalledCollection {
                        id: "col-local".into(),
                        source: CollectionSource::Nexus,
                        kind: CollectionKind::Collection,
                        name: "Local Pack".into(),
                        mod_ids: vec!["B".into()],
                        installed_at: "t".into(),
                        friend_sync: None,
                        ..Default::default()
                    },
                    InstalledCollection {
                        id: "col-synced".into(),
                        source: CollectionSource::Emperor,
                        kind: CollectionKind::Share,
                        name: "Friend Pack".into(),
                        mod_ids: vec![],
                        installed_at: "t".into(),
                        friend_sync: Some(FriendSyncMetadata {
                            friend_peer_id: "peer-1".into(),
                            friend_collection_id: "saved:1".into(),
                            last_synced_hash: "h1".into(),
                            last_synced_at: "t1".into(),
                            auto_sync: false,
                        }),
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();

        let err_standalone =
            set_mod_collections(&paths, game_id, "A", &["col-synced".into()]).unwrap_err();
        assert!(
            err_standalone
                .to_string()
                .contains("cannot add a mod to synced collection \"Friend Pack\""),
            "unexpected error: {err_standalone}"
        );

        let err_other = set_mod_collections(
            &paths,
            game_id,
            "B",
            &["col-local".into(), "col-synced".into()],
        )
        .unwrap_err();
        assert!(
            err_other
                .to_string()
                .contains("cannot add a mod to synced collection \"Friend Pack\""),
            "unexpected error: {err_other}"
        );

        let order = load_loadorder(&paths, game_id).unwrap();
        assert!(order.mods[0].collection_ids.is_empty());
        assert!(order.mods[0].independent);
        assert_eq!(order.mods[1].collection_ids, vec!["col-local".to_string()]);
        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert_eq!(cols[0].mod_ids, vec!["B".to_string()]);
        assert!(cols[1].mod_ids.is_empty());
    }

    #[test]
    fn set_mod_collections_allows_keeping_existing_synced_collection_membership() {
        let (_dir, paths) = test_paths();
        let game_id = "game-synced-keep";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        std::fs::create_dir_all(staging_root.join("A")).unwrap();
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "A".into(),
                    name: "A".into(),
                    staging_path: staging_root.join("A").to_string_lossy().into(),
                    independent: false,
                    collection_ids: vec!["col-synced".into()],
                    ..Default::default()
                }],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![
                    InstalledCollection {
                        id: "col-local".into(),
                        source: CollectionSource::Nexus,
                        kind: CollectionKind::Collection,
                        name: "Local Pack".into(),
                        mod_ids: vec![],
                        installed_at: "t".into(),
                        friend_sync: None,
                        ..Default::default()
                    },
                    InstalledCollection {
                        id: "col-synced".into(),
                        source: CollectionSource::Emperor,
                        kind: CollectionKind::Share,
                        name: "Friend Pack".into(),
                        mod_ids: vec!["A".into()],
                        installed_at: "t".into(),
                        friend_sync: Some(FriendSyncMetadata {
                            friend_peer_id: "peer-1".into(),
                            friend_collection_id: "saved:1".into(),
                            last_synced_hash: "h1".into(),
                            last_synced_at: "t1".into(),
                            auto_sync: false,
                        }),
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();

        let updated = set_mod_collections(
            &paths,
            game_id,
            "A",
            &["col-synced".into(), "col-local".into()],
        )
        .unwrap();
        assert_eq!(
            updated.collection_ids,
            vec!["col-synced".to_string(), "col-local".to_string()]
        );
        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert_eq!(cols[0].mod_ids, vec!["A".to_string()]);
        assert_eq!(cols[1].mod_ids, vec!["A".to_string()]);
    }

    #[test]
    fn set_mod_collections_rejects_removing_mod_from_synced_collection() {
        let (_dir, paths) = test_paths();
        let game_id = "game-synced-remove";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        std::fs::create_dir_all(staging_root.join("A")).unwrap();
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![StagedMod {
                    id: "A".into(),
                    name: "A".into(),
                    staging_path: staging_root.join("A").to_string_lossy().into(),
                    independent: false,
                    collection_ids: vec!["col-synced".into()],
                    ..Default::default()
                }],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: "col-synced".into(),
                    source: CollectionSource::Emperor,
                    kind: CollectionKind::Share,
                    name: "Friend Pack".into(),
                    mod_ids: vec!["A".into()],
                    installed_at: "t".into(),
                    friend_sync: Some(FriendSyncMetadata {
                        friend_peer_id: "peer-1".into(),
                        friend_collection_id: "saved:1".into(),
                        last_synced_hash: "h1".into(),
                        last_synced_at: "t1".into(),
                        auto_sync: false,
                    }),
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let err = set_mod_collections(&paths, game_id, "A", &[]).unwrap_err();
        assert!(
            err.to_string()
                .contains("cannot remove a mod from synced collection \"Friend Pack\""),
            "unexpected error: {err}"
        );
        let order = load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods[0].collection_ids, vec!["col-synced".to_string()]);
        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert_eq!(cols[0].mod_ids, vec!["A".to_string()]);
    }

    #[test]
    fn synced_collection_for_mod_detects_synced_membership() {
        let (_dir, paths) = test_paths();
        let game_id = "game-synced-detect";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        for id in ["S1", "L1", "IND"] {
            std::fs::create_dir_all(staging_root.join(id)).unwrap();
        }
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    StagedMod {
                        id: "S1".into(),
                        name: "S1".into(),
                        staging_path: staging_root.join("S1").to_string_lossy().into(),
                        collection_ids: vec!["col-synced".into()],
                        ..Default::default()
                    },
                    StagedMod {
                        id: "L1".into(),
                        name: "L1".into(),
                        staging_path: staging_root.join("L1").to_string_lossy().into(),
                        collection_ids: vec!["col-local".into()],
                        ..Default::default()
                    },
                    StagedMod {
                        id: "IND".into(),
                        name: "IND".into(),
                        staging_path: staging_root.join("IND").to_string_lossy().into(),
                        independent: true,
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![
                    InstalledCollection {
                        id: "col-local".into(),
                        source: CollectionSource::Nexus,
                        kind: CollectionKind::Collection,
                        name: "Local Pack".into(),
                        mod_ids: vec!["L1".into()],
                        installed_at: "t".into(),
                        friend_sync: None,
                        ..Default::default()
                    },
                    InstalledCollection {
                        id: "col-synced".into(),
                        source: CollectionSource::Emperor,
                        kind: CollectionKind::Share,
                        name: "Friend Pack".into(),
                        mod_ids: vec!["S1".into()],
                        installed_at: "t".into(),
                        friend_sync: Some(FriendSyncMetadata {
                            friend_peer_id: "peer-1".into(),
                            friend_collection_id: "saved:1".into(),
                            last_synced_hash: "h1".into(),
                            last_synced_at: "t1".into(),
                            auto_sync: false,
                        }),
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();

        assert_eq!(
            synced_collection_for_mod(&paths, game_id, "S1").unwrap(),
            Some("Friend Pack".to_string())
        );
        assert_eq!(synced_collection_for_mod(&paths, game_id, "L1").unwrap(), None);
        assert_eq!(synced_collection_for_mod(&paths, game_id, "IND").unwrap(), None);
    }

    #[test]
    fn ensure_load_order_preserves_synced_collections_blocks_reordering_synced_members_but_allows_standalone() {
        let (_dir, paths) = test_paths();
        let game_id = "game-synced-order";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        for id in ["S1", "S2", "M1"] {
            std::fs::create_dir_all(staging_root.join(id)).unwrap();
        }
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    StagedMod {
                        id: "S1".into(),
                        name: "S1".into(),
                        staging_path: staging_root.join("S1").to_string_lossy().into(),
                        order: 1,
                        collection_ids: vec!["col-synced".into()],
                        ..Default::default()
                    },
                    StagedMod {
                        id: "S2".into(),
                        name: "S2".into(),
                        staging_path: staging_root.join("S2").to_string_lossy().into(),
                        order: 2,
                        collection_ids: vec!["col-synced".into()],
                        ..Default::default()
                    },
                    StagedMod {
                        id: "M1".into(),
                        name: "M1".into(),
                        staging_path: staging_root.join("M1").to_string_lossy().into(),
                        order: 3,
                        independent: true,
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: "col-synced".into(),
                    source: CollectionSource::Emperor,
                    kind: CollectionKind::Share,
                    name: "Friend Pack".into(),
                    mod_ids: vec!["S1".into(), "S2".into()],
                    installed_at: "t".into(),
                    friend_sync: Some(FriendSyncMetadata {
                        friend_peer_id: "peer-1".into(),
                        friend_collection_id: "saved:1".into(),
                        last_synced_hash: "h1".into(),
                        last_synced_at: "t1".into(),
                        auto_sync: false,
                    }),
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        // Moving standalone M1 between or before S1/S2 keeps S1 before S2 -> allowed.
        ensure_load_order_preserves_synced_collections(
            &paths,
            game_id,
            &["M1".into(), "S1".into(), "S2".into()],
        )
        .unwrap();
        ensure_load_order_preserves_synced_collections(
            &paths,
            game_id,
            &["S1".into(), "M1".into(), "S2".into()],
        )
        .unwrap();

        // Swapping S2 before S1 -> blocked.
        let err = ensure_load_order_preserves_synced_collections(
            &paths,
            game_id,
            &["S2".into(), "S1".into(), "M1".into()],
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("cannot reorder mods in synced collection \"Friend Pack\""),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn sync_installed_collection_replaces_members_and_disables_dropped_non_independent() {
        let (_dir, paths) = test_paths();
        let game_id = "skyrim";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();
        let staging_root = paths.mods_dir(game_id);
        std::fs::create_dir_all(&staging_root).unwrap();

        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    StagedMod {
                        id: "A".into(),
                        name: "A".into(),
                        enabled: true,
                        independent: false,
                        collection_ids: vec!["friend-col".into()],
                        staging_path: staging_root.join("A").to_string_lossy().into(),
                        ..Default::default()
                    },
                    StagedMod {
                        id: "B".into(),
                        name: "B".into(),
                        enabled: true,
                        independent: false,
                        collection_ids: vec!["friend-col".into()],
                        staging_path: staging_root.join("B").to_string_lossy().into(),
                        ..Default::default()
                    },
                    StagedMod {
                        id: "C".into(),
                        name: "C".into(),
                        enabled: true,
                        independent: true,
                        collection_ids: vec!["friend-col".into()],
                        staging_path: staging_root.join("C").to_string_lossy().into(),
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();

        let initial = InstalledCollection {
            id: "friend-col".into(),
            source: CollectionSource::Emperor,
            kind: CollectionKind::Share,
            name: "Friend Col".into(),
            slug: None,
            namespace: None,
            package_name: None,
            community: None,
            revision: None,
            version: None,
            profile_code: Some("code-1".into()),
            mod_ids: vec!["A".into(), "B".into(), "C".into()],
            installed_at: String::new(),
            game_versions: None,
            friend_sync: Some(FriendSyncMetadata {
                friend_peer_id: "peer-1".into(),
                friend_collection_id: "active:skyrim".into(),
                last_synced_hash: "h1".into(),
                last_synced_at: "t1".into(),
                auto_sync: true,
            }),
            ..Default::default()
        };
        record_installed_collection(&paths, game_id, initial.clone(), &[]).unwrap();

        // Sync with B and C dropped; only A remains.
        let mut updated = initial;
        updated.mod_ids = vec!["A".into()];
        updated.profile_code = Some("code-2".into());
        sync_installed_collection(&paths, game_id, updated, &[]).unwrap();

        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert_eq!(cols[0].mod_ids, vec!["A".to_string()]);
        assert_eq!(cols[0].friend_sync.as_ref().unwrap().auto_sync, true);

        let order = load_loadorder(&paths, game_id).unwrap();
        let mod_a = order.mods.iter().find(|m| m.id == "A").unwrap();
        let mod_b = order.mods.iter().find(|m| m.id == "B").unwrap();
        let mod_c = order.mods.iter().find(|m| m.id == "C").unwrap();
        assert!(mod_a.enabled);
        assert_eq!(mod_a.collection_ids, vec!["friend-col".to_string()]);
        assert!(!mod_b.enabled);
        assert!(mod_b.collection_ids.is_empty());
        // Independent mod C stays enabled even after being detached.
        assert!(mod_c.enabled);
        assert!(mod_c.collection_ids.is_empty());
    }

    #[test]
    fn list_installed_collections_reconciles_member_mods_and_tracks_unsaved_changes() {
        let (dir, paths) = test_paths();
        let game_id = "skyrim";
        crate::config::ensure_game_dirs(&paths, game_id).unwrap();

        let staged = |id: &str, order: u32, collection_ids: Vec<String>| StagedMod {
            id: id.into(),
            name: id.into(),
            order,
            enabled: true,
            collection_ids,
            independent: false,
            staging_path: dir.path().join(id).to_string_lossy().into(),
            ..Default::default()
        };

        let mut mods = Vec::new();
        for i in 0..5 {
            mods.push(staged(&format!("m{i}"), i, vec!["col-1".into()]));
        }
        save_loadorder(&paths, game_id, &LoadOrder { mods }).unwrap();

        // Simulate collections.json only having 2 of the 5 installed mods.
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: "col-1".into(),
                    source: CollectionSource::Emperor,
                    kind: CollectionKind::Share,
                    name: "INC".into(),
                    mod_ids: vec!["m0".into(), "m1".into()],
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert_eq!(cols[0].mod_ids.len(), 5);
        assert!(!cols[0].has_unsaved_changes);

        // Reordering marks the collection as having unsaved changes.
        set_load_order(
            &paths,
            game_id,
            &[
                "m4".into(),
                "m0".into(),
                "m1".into(),
                "m2".into(),
                "m3".into(),
            ],
        )
        .unwrap();
        let cols_after_reorder = list_installed_collections(&paths, game_id).unwrap();
        assert!(cols_after_reorder[0].has_unsaved_changes);

        // Toggling auto_save persists the flag.
        let updated = set_collection_auto_save(&paths, game_id, "col-1", true).unwrap();
        assert!(updated.auto_save);
    }

    #[test]
    fn rename_installed_collection_and_sync_update_name_and_profile_code() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game_id = "skyrim_1";
        let manifest = crate::share::ShareManifest {
            v: 1,
            name: Some("Initial Pack".into()),
            game: crate::share::ShareGameHint {
                title: Some("Skyrim SE".into()),
                nexus_domain: Some("skyrimspecialedition".into()),
                ..Default::default()
            },
            game_versions: None,
            mods: Vec::new(),
        };
        let initial_code = crate::share::encode_share_manifest(&manifest).unwrap();
        let share_id = crate::share::emperor_share_id(&initial_code);

        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: share_id.clone(),
                    source: CollectionSource::Emperor,
                    kind: CollectionKind::Share,
                    name: "Initial Pack".into(),
                    profile_code: Some(initial_code.clone()),
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let renamed =
            rename_installed_collection(&paths, game_id, &share_id, "  My Co-op Pack ").unwrap();
        assert_eq!(renamed.collection.name, "My Co-op Pack");
        assert_eq!(renamed.old_code.as_deref(), Some(initial_code.as_str()));
        let decoded = crate::share::decode_share_code(
            renamed.collection.profile_code.as_deref().unwrap(),
        )
        .unwrap();
        assert_eq!(decoded.name.as_deref(), Some("My Co-op Pack"));

        let mut next_manifest = manifest;
        next_manifest.name = Some("From Collections Tab".into());
        let next_code = crate::share::encode_share_manifest(&next_manifest).unwrap();
        let synced = sync_installed_collection_rename(
            &paths,
            game_id,
            &share_id,
            renamed.collection.profile_code.as_deref().unwrap(),
            "From Collections Tab",
            &next_code,
        )
        .unwrap();
        assert!(synced);
        let cols = load_collections(&paths, game_id).unwrap().collections;
        assert_eq!(cols[0].name, "From Collections Tab");
        assert_eq!(cols[0].profile_code.as_deref(), Some(next_code.as_str()));
    }

    #[test]
    fn auto_save_persists_across_relaunch_and_concurrent_updates() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game_id = "skyrim_1";
        let staged = |id: &str, ord: u32| StagedMod {
            id: id.into(),
            name: id.into(),
            domain: "skyrimspecialedition".into(),
            staging_path: paths.mods_dir(game_id).join(id).to_string_lossy().into(),
            enabled: true,
            order: ord,
            collection_ids: vec!["col-1".into()],
            ..Default::default()
        };
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![staged("m1", 1), staged("m2", 2)],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: "col-1".into(),
                    source: CollectionSource::Emperor,
                    kind: CollectionKind::Share,
                    name: "Coop".into(),
                    mod_ids: vec!["m1".into(), "m2".into()],
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let updated = set_collection_auto_save(&paths, game_id, "col-1", true).unwrap();
        assert!(updated.auto_save);

        // Concurrent/subsequent list_installed_collections and cache_collection_game_versions
        // (simulating relaunch & metadata refresh) must preserve auto_save = true.
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(|| {
                    let _ = list_installed_collections(&paths, game_id).unwrap();
                    let mut updates = HashMap::new();
                    updates.insert("col-1".to_string(), vec!["1.6.1170".to_string()]);
                    let _ = cache_collection_game_versions(&paths, game_id, &updates).unwrap();
                });
            }
        });

        let relaunched = list_installed_collections(&paths, game_id).unwrap();
        assert!(relaunched[0].auto_save);
        let raw = load_collections(&paths, game_id).unwrap();
        assert!(raw.collections[0].auto_save);
    }

    #[test]
    fn friend_sync_collections_disallow_auto_save_and_clear_unsaved_changes() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game_id = "skyrim_sync_autosave";
        let staged = |id: &str, order: u32| StagedMod {
            id: id.into(),
            name: id.into(),
            staging_path: dir.path().join(id).to_string_lossy().into_owned(),
            enabled: true,
            order,
            collection_ids: vec!["friend-col".into()],
            ..Default::default()
        };
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![staged("m1", 1), staged("m2", 2)],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: "friend-col".into(),
                    source: CollectionSource::Emperor,
                    kind: CollectionKind::Share,
                    name: "Friend Pack".into(),
                    mod_ids: vec!["m1".into(), "m2".into()],
                    auto_save: true,
                    friend_sync: Some(FriendSyncMetadata {
                        friend_peer_id: "peer-1".into(),
                        friend_collection_id: "fc-1".into(),
                        last_synced_hash: "h1".into(),
                        last_synced_at: "2026-11-23T21:00:00Z".into(),
                        auto_sync: true,
                    }),
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        // Listing normalizes persisted auto_save = true -> false and never reports unsaved changes.
        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert!(!cols[0].auto_save);
        assert!(!cols[0].has_unsaved_changes);

        // Reordering mods must not mark a friend_sync collection as having unsaved changes.
        set_load_order(&paths, game_id, &["m2".into(), "m1".into()]).unwrap();
        let cols_after = list_installed_collections(&paths, game_id).unwrap();
        assert!(!cols_after[0].has_unsaved_changes);
        assert!(!cols_after[0].auto_save);

        // Toggling auto_save on a friend_sync collection must fail.
        assert!(set_collection_auto_save(&paths, game_id, "friend-col", true).is_err());
    }

    #[test]
    fn apply_collection_order_and_selections_respects_enabled_flag() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game_id = "skyrim_1";
        let staged = |id: &str, ord: u32, enabled: bool| StagedMod {
            id: id.into(),
            name: id.into(),
            domain: "skyrimspecialedition".into(),
            staging_path: paths.mods_dir(game_id).join(id).to_string_lossy().into(),
            enabled,
            order: ord,
            ..Default::default()
        };
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![staged("a", 1, true), staged("b", 2, true)],
            },
        )
        .unwrap();

        apply_collection_order_and_selections(
            &paths,
            game_id,
            &[
                SyncedModSelection {
                    mod_id: "b".into(),
                    fomod_selection: None,
                    option_selection: None,
                    enabled: Some(false),
                },
                SyncedModSelection {
                    mod_id: "a".into(),
                    fomod_selection: None,
                    option_selection: None,
                    enabled: None,
                },
            ],
        )
        .unwrap();

        let after = load_loadorder(&paths, game_id).unwrap();
        assert_eq!(after.mods[0].id, "b");
        assert!(!after.mods[0].enabled);
        assert_eq!(after.mods[1].id, "a");
        assert!(after.mods[1].enabled);
    }

    #[test]
    fn set_collection_enabled_disables_exclusive_mods_and_preserves_protected_or_shared() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game_id = "skyrim_1";
        let mk = |id: &str, cols: Vec<&str>| StagedMod {
            id: id.into(),
            name: id.into(),
            domain: "skyrimspecialedition".into(),
            staging_path: paths.mods_dir(game_id).join(id).to_string_lossy().into(),
            enabled: true,
            order: 1,
            collection_ids: cols.into_iter().map(String::from).collect(),
            independent: false,
            ..Default::default()
        };
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    mk("exclusive", vec!["col_a"]),
                    mk("shared_active", vec!["col_a", "col_b"]),
                    mk("protected_incoming", vec!["col_a"]),
                ],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![
                    InstalledCollection {
                        id: "col_a".into(),
                        name: "Collection A".into(),
                        mod_ids: vec![
                            "exclusive".into(),
                            "shared_active".into(),
                            "protected_incoming".into(),
                        ],
                        ..Default::default()
                    },
                    InstalledCollection {
                        id: "col_b".into(),
                        name: "Collection B".into(),
                        mod_ids: vec!["shared_active".into()],
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();

        let changed = set_collection_enabled(
            &paths,
            game_id,
            "col_a",
            false,
            &["protected_incoming".into()],
        )
        .unwrap();
        assert_eq!(changed, 1);

        let after = load_loadorder(&paths, game_id).unwrap();
        assert!(!after.mods.iter().find(|m| m.id == "exclusive").unwrap().enabled);
        assert!(after.mods.iter().find(|m| m.id == "shared_active").unwrap().enabled);
        assert!(after.mods.iter().find(|m| m.id == "protected_incoming").unwrap().enabled);
        let cols_after = load_collections(&paths, game_id).unwrap();
        assert!(cols_after.collections.iter().find(|c| c.id == "col_a").unwrap().disabled);
        assert!(!cols_after.collections.iter().find(|c| c.id == "col_b").unwrap().disabled);

        // Re-enabling turns exclusive back on and clears disabled.
        let reenabled = set_collection_enabled(&paths, game_id, "col_a", true, &[]).unwrap();
        assert_eq!(reenabled, 1);
        let after_re = load_loadorder(&paths, game_id).unwrap();
        assert!(after_re.mods.iter().find(|m| m.id == "exclusive").unwrap().enabled);
        let cols_after_re = load_collections(&paths, game_id).unwrap();
        assert!(!cols_after_re.collections.iter().find(|c| c.id == "col_a").unwrap().disabled);
    }

    #[test]
    fn set_collection_enabled_ignores_disabled_other_collections_when_resolving_shared() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game_id = "skyrim_1";
        let mk = |id: &str, cols: Vec<&str>, enabled: bool| StagedMod {
            id: id.into(),
            name: id.into(),
            domain: "skyrimspecialedition".into(),
            staging_path: paths.mods_dir(game_id).join(id).to_string_lossy().into(),
            enabled,
            order: 1,
            collection_ids: cols.into_iter().map(String::from).collect(),
            independent: false,
            ..Default::default()
        };
        save_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    mk("shared_with_disabled", vec!["col_a", "col_b"], true),
                    mk("col_b_only", vec!["col_b"], false),
                ],
            },
        )
        .unwrap();
        save_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![
                    InstalledCollection {
                        id: "col_a".into(),
                        name: "Collection A".into(),
                        mod_ids: vec!["shared_with_disabled".into()],
                        disabled: false,
                        ..Default::default()
                    },
                    InstalledCollection {
                        id: "col_b".into(),
                        name: "Collection B".into(),
                        mod_ids: vec!["shared_with_disabled".into(), "col_b_only".into()],
                        disabled: true,
                        ..Default::default()
                    },
                ],
            },
        )
        .unwrap();

        let changed = set_collection_enabled(&paths, game_id, "col_a", false, &[]).unwrap();
        assert_eq!(changed, 1);

        let after = load_loadorder(&paths, game_id).unwrap();
        assert!(
            !after
                .mods
                .iter()
                .find(|m| m.id == "shared_with_disabled")
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn stage_thunderstore_mod_alongside_keeps_both_versions_coexisting() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game_id = "lc_1";
        let zip1 = dir.path().join("v1.zip");
        let zip2 = dir.path().join("v2.zip");
        for (p, content) in [(&zip1, b"v1" as &[u8]), (&zip2, b"v2" as &[u8])] {
            let f = fs::File::create(p).unwrap();
            let mut z = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            z.start_file("plugin.dll", opts).unwrap();
            std::io::Write::write_all(&mut z, content).unwrap();
            z.finish().unwrap();
        }

        let first = stage_thunderstore_mod_with_mode(
            &paths,
            game_id,
            "lethal-company",
            "Owner",
            "Pkg",
            "1.0.0",
            None,
            "Pkg",
            None,
            None,
            &zip1,
            true,
            None,
        )
        .unwrap();
        assert_eq!(first.id, "ts_Owner_Pkg");

        let second = stage_thunderstore_mod_with_mode(
            &paths,
            game_id,
            "lethal-company",
            "Owner",
            "Pkg",
            "2.0.0",
            None,
            "Pkg",
            None,
            None,
            &zip2,
            false,
            None,
        )
        .unwrap();
        assert_eq!(second.id, "ts_Owner_Pkg@2.0.0");

        let order = load_loadorder(&paths, game_id).unwrap();
        assert_eq!(order.mods.len(), 2);
        assert!(find_thunderstore_package_version(&order, "Owner", "Pkg", Some("1.0.0")).is_some());
        assert!(find_thunderstore_package_version(&order, "Owner", "Pkg", Some("2.0.0")).is_some());
    }

    #[test]
    fn list_installed_collections_self_heals_friend_sync_missing_members_from_profile_code() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game_id = "skyrim_heal";
        let col_id = "friend:peer1:saved:c1";

        let make_nexus = |id: &str, mod_id: u64, file_id: u64, cols: Vec<String>, order: u32| StagedMod {
            id: id.into(),
            name: id.into(),
            domain: "skyrimspecialedition".into(),
            nexus_mod_id: mod_id,
            nexus_file_id: file_id,
            source: ModSource::Nexus,
            staging_path: dir.path().join(id).to_string_lossy().into(),
            enabled: true,
            order,
            collection_ids: cols,
            independent: true,
            ..Default::default()
        };

        write_loadorder(
            &paths,
            game_id,
            &LoadOrder {
                mods: vec![
                    make_nexus("m1", 10, 100, vec![col_id.into()], 0),
                    make_nexus("m2", 20, 200, vec![], 1),
                    make_nexus("m3", 30, 300, vec![], 2),
                ],
            },
        )
        .unwrap();

        let manifest = crate::share::ShareManifest {
            v: 1,
            name: Some("Friend Pack".into()),
            game: crate::share::ShareGameHint {
                plugin_id: Some("skyrimse".into()),
                nexus_domain: Some("skyrimspecialedition".into()),
                title: Some("Skyrim SE".into()),
                ..Default::default()
            },
            game_versions: None,
            mods: vec![
                crate::share::ShareModEntry {
                    s: crate::share::ShareModSource::Nexus,
                    domain: Some("skyrimspecialedition".into()),
                    mod_id: Some(10),
                    file_id: Some(100),
                    community: None,
                    namespace: None,
                    name: None,
                    game_id: None,
                    display_name: Some("m1".into()),
                    version: None,
                    fomod: None,
                    options: None,
                    enabled: None,
                },
                crate::share::ShareModEntry {
                    s: crate::share::ShareModSource::Nexus,
                    domain: Some("skyrimspecialedition".into()),
                    mod_id: Some(20),
                    file_id: Some(200),
                    community: None,
                    namespace: None,
                    name: None,
                    game_id: None,
                    display_name: Some("m2".into()),
                    version: None,
                    fomod: None,
                    options: None,
                    enabled: None,
                },
                crate::share::ShareModEntry {
                    s: crate::share::ShareModSource::Nexus,
                    domain: Some("skyrimspecialedition".into()),
                    mod_id: Some(30),
                    file_id: Some(300),
                    community: None,
                    namespace: None,
                    name: None,
                    game_id: None,
                    display_name: Some("m3".into()),
                    version: None,
                    fomod: None,
                    options: None,
                    enabled: None,
                },
            ],
        };
        let share_code = crate::share::encode_share_manifest(&manifest).unwrap();

        write_collections(
            &paths,
            game_id,
            &InstalledCollections {
                collections: vec![InstalledCollection {
                    id: col_id.into(),
                    source: CollectionSource::Emperor,
                    kind: CollectionKind::Share,
                    name: "Friend Pack".into(),
                    profile_code: Some(share_code),
                    mod_ids: vec!["m1".into()],
                    friend_sync: Some(FriendSyncMetadata {
                        friend_peer_id: "peer1".into(),
                        friend_collection_id: "saved:c1".into(),
                        last_synced_hash: "hash1".into(),
                        last_synced_at: "2026-01-01T00:00:00Z".into(),
                        auto_sync: true,
                    }),
                    ..Default::default()
                }],
            },
        )
        .unwrap();

        let cols = list_installed_collections(&paths, game_id).unwrap();
        assert_eq!(cols.len(), 1);
        assert_eq!(cols[0].mod_ids, vec!["m1", "m2", "m3"]);

        let order = load_loadorder(&paths, game_id).unwrap();
        for id in ["m1", "m2", "m3"] {
            let m = order.mods.iter().find(|m| m.id == id).unwrap();
            assert!(
                m.collection_ids.iter().any(|c| c == col_id),
                "expected {id} to have collection_id {col_id}"
            );
        }
    }

    #[test]
    fn friend_sync_metadata_defaults_auto_sync_to_true_for_legacy_json() {
        let legacy: FriendSyncMetadata = serde_json::from_str(
            r#"{
                "friend_peer_id": "peer1",
                "friend_collection_id": "saved:c1",
                "last_synced_hash": "hash1",
                "last_synced_at": "2026-01-01T00:00:00Z"
            }"#,
        )
        .unwrap();
        assert!(legacy.auto_sync);

        let explicit_off: FriendSyncMetadata = serde_json::from_str(
            r#"{
                "friend_peer_id": "peer1",
                "friend_collection_id": "saved:c1",
                "last_synced_hash": "hash1",
                "last_synced_at": "2026-01-01T00:00:00Z",
                "auto_sync": false
            }"#,
        )
        .unwrap();
        assert!(!explicit_off.auto_sync);
    }
}
