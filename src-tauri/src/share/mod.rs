//! Self-contained Emperor share codes for cross-platform loadout sharing.

use std::io::{Read, Write};

use anyhow::{anyhow, bail, Context, Result};
use base64::Engine;
use flate2::{read::DeflateDecoder, write::DeflateEncoder, Compression};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::{
    config::{ManagedGame, Paths},
    mods::{LoadOrder, ModSource, StagedMod},
};

pub const SHARE_PREFIX: &str = "#emperor1\n";
/// Chat apps (Discord ~2K) often mangle or truncate pastes above this size.
const SOFT_SIZE_WARN: usize = 2 * 1024;
const ALTERED_SHARE_HINT: &str = "Share code looks altered or damaged (often by Discord/chat paste). Re-copy from Emperor or send an .emperor-share / text file instead.";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImportCodeKind {
    Emperor,
    ThunderstoreProfile,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShareModSource {
    Nexus,
    Thunderstore,
    Modio,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ShareGameHint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nexus_domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thunderstore_community: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub modio_game_id: Option<u32>,
    /// Display name of the game this share was exported from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Unreal project folder when the game uses the generic Unreal plugin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareModEntry {
    pub s: ShareModSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mod_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub community: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub namespace: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fomod: Option<crate::mods::fomod::FomodSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub options: Option<crate::mods::options::ModOptionSelection>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareManifest {
    pub v: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub game: ShareGameHint,
    /// Game versions this loadout was created against (install / override at export).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_versions: Option<Vec<String>>,
    pub mods: Vec<ShareModEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExportShareResult {
    pub code: String,
    pub warnings: Vec<String>,
    pub skipped: Vec<String>,
    pub mod_count: usize,
    pub saved: SavedCollectionEntry,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareDecodePreview {
    pub name: Option<String>,
    pub game: ShareGameHint,
    pub mods: Vec<ShareModEntry>,
    pub nexus_count: usize,
    pub thunderstore_count: usize,
    pub modio_count: usize,
    pub code_len: usize,
    pub size_warning: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectImportResult {
    pub kind: ImportCodeKind,
    pub preview: Option<ShareDecodePreview>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareAssistFile {
    pub domain: String,
    pub mod_id: u64,
    pub file_id: u64,
    pub name: String,
    pub version: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShareImportResult {
    pub collection_id: String,
    pub name: String,
    pub member_ids: Vec<String>,
    pub needs_assist: Vec<ShareAssistFile>,
    pub warnings: Vec<String>,
    /// Present when install finished without Assist (Premium or no Nexus files left).
    pub collection: Option<crate::mods::InstalledCollection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedCollectionEntry {
    pub id: String,
    pub name: String,
    pub code: String,
    pub created_at: String,
    pub updated_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_game_id: Option<String>,
    pub game: ShareGameHint,
    /// Game versions this share was created against (from manifest at save time).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game_versions: Option<Vec<String>>,
    pub mod_count: usize,
    pub nexus_count: usize,
    pub thunderstore_count: usize,
    pub modio_count: usize,
    /// `emperor-share:{hash}`. Filled when the list is sent to the UI so each
    /// code is not hashed over its own command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub share_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SavedCollectionsStore {
    pub collections: Vec<SavedCollectionEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedCollectionDetail {
    pub entry: SavedCollectionEntry,
    pub mods: Vec<ShareModEntry>,
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn content_hash(code: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(code.as_bytes());
    hex::encode(hasher.finalize())[..16].to_string()
}

pub fn emperor_share_id(code: &str) -> String {
    format!("emperor-share:{}", content_hash(code))
}

pub fn share_hash(code: &str) -> String {
    content_hash(code)
}

pub fn encode_share_manifest(manifest: &ShareManifest) -> Result<String> {
    let json = serde_json::to_vec(manifest).context("serialize share manifest")?;
    let mut encoder = DeflateEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(&json).context("deflate share")?;
    let compressed = encoder.finish().context("finish deflate")?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(compressed);
    Ok(format!("{SHARE_PREFIX}{encoded}"))
}

fn strip_bom(s: &str) -> &str {
    s.strip_prefix('\u{feff}').unwrap_or(s)
}

/// Discord and other chat apps wrap long pastes; strip all whitespace from the base64 body.
fn compact_base64_payload(encoded: &str) -> String {
    encoded.chars().filter(|c| !c.is_whitespace()).collect()
}

pub fn decode_share_code(raw: &str) -> Result<ShareManifest> {
    let trimmed = strip_bom(raw).trim();
    let encoded = trimmed
        .strip_prefix(SHARE_PREFIX)
        .or_else(|| {
            trimmed
                .strip_prefix("#emperor1")
                .map(|s| s.trim_start_matches(['\n', '\r', ' ']))
        })
        .ok_or_else(|| anyhow!("not an Emperor share code (missing #emperor1 prefix)"))?;
    let compact = compact_base64_payload(encoded);
    let compressed = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(compact.as_bytes())
        .or_else(|_| base64::engine::general_purpose::STANDARD.decode(compact.as_bytes()))
        .context("decode share payload")?;
    let mut decoder = DeflateDecoder::new(compressed.as_slice());
    let mut json = Vec::new();
    // Discord truncation often still base64-decodes; inflate may fail, or yield
    // partial bytes that then fail JSON parse — treat both as altered paste.
    decoder
        .read_to_end(&mut json)
        .map_err(|_| anyhow!(ALTERED_SHARE_HINT))?;
    let manifest: ShareManifest =
        serde_json::from_slice(&json).map_err(|_| anyhow!(ALTERED_SHARE_HINT))?;
    if manifest.v != 1 {
        bail!("unsupported share format version {}", manifest.v);
    }
    Ok(manifest)
}

pub fn preview_from_manifest(manifest: &ShareManifest, code_len: usize) -> ShareDecodePreview {
    let mut nexus_count = 0usize;
    let mut thunderstore_count = 0usize;
    let mut modio_count = 0usize;
    for m in &manifest.mods {
        match m.s {
            ShareModSource::Nexus => nexus_count += 1,
            ShareModSource::Thunderstore => thunderstore_count += 1,
            ShareModSource::Modio => modio_count += 1,
        }
    }
    ShareDecodePreview {
        name: manifest.name.clone(),
        game: manifest.game.clone(),
        mods: manifest.mods.clone(),
        nexus_count,
        thunderstore_count,
        modio_count,
        code_len,
        size_warning: code_len > SOFT_SIZE_WARN,
    }
}

pub fn detect_code_kind(raw: &str) -> ImportCodeKind {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return ImportCodeKind::Unknown;
    }
    if trimmed.starts_with("#emperor1") || decode_share_code(trimmed).is_ok() {
        return ImportCodeKind::Emperor;
    }
    if trimmed.starts_with("#r2modman") {
        return ImportCodeKind::ThunderstoreProfile;
    }
    // Legacy Thunderstore profile keys are short alphanumeric tokens (no whitespace).
    let compact: String = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    if compact.len() >= 4
        && compact.len() <= 64
        && compact
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
        && !compact.contains(':')
    {
        return ImportCodeKind::ThunderstoreProfile;
    }
    ImportCodeKind::Unknown
}

fn empty_opt(s: &str) -> Option<String> {
    let t = s.trim();
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

pub fn game_hint_from_managed(game: &ManagedGame) -> ShareGameHint {
    ShareGameHint {
        id: Some(game.id.clone()),
        plugin_id: Some(game.plugin_id.clone()),
        nexus_domain: empty_opt(&game.nexus_domain),
        thunderstore_community: game
            .thunderstore_community
            .as_ref()
            .and_then(|s| empty_opt(s)),
        modio_game_id: game.modio_game_id.filter(|id| *id > 0),
        title: empty_opt(&game.title),
        project_name: game.project_name.as_ref().and_then(|s| empty_opt(s)),
    }
}

pub fn catalogs_overlap(hint: &ShareGameHint, game: &ManagedGame) -> Result<(), String> {
    let mut any_share_catalog = false;
    let mut any_match = false;

    if let Some(d) = hint.nexus_domain.as_deref().filter(|s| !s.is_empty()) {
        any_share_catalog = true;
        let local = game.nexus_domain.trim();
        if !local.is_empty() {
            if !local.eq_ignore_ascii_case(d) {
                return Err(format!(
                    "Nexus domain mismatch: share is '{d}', game is '{local}'"
                ));
            }
            any_match = true;
        }
    }

    if let Some(c) = hint
        .thunderstore_community
        .as_deref()
        .filter(|s| !s.is_empty())
    {
        any_share_catalog = true;
        if let Some(local) = game
            .thunderstore_community
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            if !crate::thunderstore::community_slug_matches(c, local) {
                return Err(format!(
                    "Thunderstore community mismatch: share is '{c}', game is '{local}'"
                ));
            }
            any_match = true;
        }
    }

    if let Some(gid) = hint.modio_game_id.filter(|id| *id > 0) {
        any_share_catalog = true;
        if let Some(local) = game.modio_game_id.filter(|id| *id > 0) {
            if local != gid {
                return Err(format!(
                    "mod.io game id mismatch: share is {gid}, game is {local}"
                ));
            }
            any_match = true;
        }
    }

    if !any_share_catalog {
        return Err("Share code has no catalog identity (Nexus / Thunderstore / mod.io).".into());
    }
    if !any_match {
        return Err(
            "This share does not match any catalog configured on the selected game (Nexus domain, Thunderstore community, or mod.io id)."
                .into(),
        );
    }
    Ok(())
}

pub fn plugin_mismatch_warning(hint: &ShareGameHint, game: &ManagedGame) -> Option<String> {
    let share_plugin = hint.plugin_id.as_deref()?.trim();
    if share_plugin.is_empty() {
        return None;
    }
    if share_plugin != game.plugin_id {
        Some(format!(
            "Plugin differs: share is '{share_plugin}', game is '{}'. Deploy layout may differ.",
            game.plugin_id
        ))
    } else {
        None
    }
}

fn display_name(m: &StagedMod) -> String {
    if m.name.trim().is_empty() {
        m.id.clone()
    } else {
        m.name.clone()
    }
}

/// Convert enabled load-order mods into portable share entries.
/// Returns `(manifest, skipped labels, staged member ids included in the share)`.
pub fn build_manifest_from_loadorder(
    order: &LoadOrder,
    game: &ManagedGame,
    name: Option<String>,
) -> (ShareManifest, Vec<String>, Vec<String>) {
    build_manifest_from_loadorder_with_selections(order, game, name, true)
}

pub fn build_manifest_from_loadorder_with_selections(
    order: &LoadOrder,
    game: &ManagedGame,
    name: Option<String>,
    include_selections: bool,
) -> (ShareManifest, Vec<String>, Vec<String>) {
    build_manifest_inner(order, game, name, include_selections, None)
}

pub fn build_manifest_for_collection(
    order: &LoadOrder,
    game: &ManagedGame,
    collection_id: &str,
    name: Option<String>,
) -> (ShareManifest, Vec<String>, Vec<String>) {
    build_manifest_inner(order, game, name, true, Some(collection_id))
}

fn build_manifest_inner(
    order: &LoadOrder,
    game: &ManagedGame,
    name: Option<String>,
    include_selections: bool,
    collection_id: Option<&str>,
) -> (ShareManifest, Vec<String>, Vec<String>) {
    let mut mods = Vec::new();
    let mut skipped = Vec::new();
    let mut member_ids = Vec::new();

    let mut enabled: Vec<&StagedMod> = order
        .mods
        .iter()
        .filter(|m| match collection_id {
            Some(cid) => m.collection_ids.iter().any(|id| id == cid),
            None => m.enabled,
        })
        .collect();
    enabled.sort_by_key(|m| m.order);

    for m in enabled {
        if m.id.starts_with("overlay_") {
            skipped.push(format!("{} (config overlay)", display_name(m)));
            continue;
        }
        let mod_enabled = collection_id.and_then(|_| (!m.enabled).then_some(false));
        match m.source {
            ModSource::Nexus => {
                if m.nexus_mod_id == 0 || m.nexus_file_id == 0 {
                    skipped.push(format!(
                        "{} (manual/imported Nexus archive)",
                        display_name(m)
                    ));
                    continue;
                }
                mods.push(ShareModEntry {
                    s: ShareModSource::Nexus,
                    domain: Some(m.domain.clone()),
                    mod_id: Some(m.nexus_mod_id),
                    file_id: Some(m.nexus_file_id),
                    community: None,
                    namespace: None,
                    name: None,
                    game_id: None,
                    display_name: Some(display_name(m)),
                    version: m.version.clone(),
                    fomod: include_selections.then(|| m.fomod_selection.clone()).flatten(),
                    options: include_selections.then(|| m.option_selection.clone()).flatten(),
                    enabled: mod_enabled,
                });
                member_ids.push(m.id.clone());
            }
            ModSource::Thunderstore => {
                let (Some(ns), Some(pkg)) = (m.ts_namespace.as_deref(), m.ts_name.as_deref())
                else {
                    skipped.push(format!("{} (incomplete Thunderstore ids)", display_name(m)));
                    continue;
                };
                mods.push(ShareModEntry {
                    s: ShareModSource::Thunderstore,
                    domain: None,
                    mod_id: None,
                    file_id: None,
                    community: Some(m.domain.clone()),
                    namespace: Some(ns.to_string()),
                    name: Some(pkg.to_string()),
                    game_id: None,
                    display_name: Some(display_name(m)),
                    version: m.version.clone(),
                    fomod: include_selections.then(|| m.fomod_selection.clone()).flatten(),
                    options: include_selections.then(|| m.option_selection.clone()).flatten(),
                    enabled: mod_enabled,
                });
                member_ids.push(m.id.clone());
            }
            ModSource::Modio => {
                let (Some(gid), Some(mid)) = (m.modio_game_id, m.modio_mod_id) else {
                    skipped.push(format!("{} (incomplete mod.io ids)", display_name(m)));
                    continue;
                };
                mods.push(ShareModEntry {
                    s: ShareModSource::Modio,
                    domain: None,
                    mod_id: Some(mid),
                    file_id: m.modio_file_id,
                    community: None,
                    namespace: None,
                    name: None,
                    game_id: Some(gid),
                    display_name: Some(display_name(m)),
                    version: m.version.clone(),
                    fomod: include_selections.then(|| m.fomod_selection.clone()).flatten(),
                    options: include_selections.then(|| m.option_selection.clone()).flatten(),
                    enabled: mod_enabled,
                });
                member_ids.push(m.id.clone());
            }
        }
    }

    let manifest = ShareManifest {
        v: 1,
        name,
        game: game_hint_from_managed(game),
        game_versions: None,
        mods,
    };
    (manifest, skipped, member_ids)
}

pub fn entry_from_code(
    code: &str,
    name_override: Option<String>,
    source_game_id: Option<String>,
) -> Result<SavedCollectionEntry> {
    let manifest = decode_share_code(code)?;
    let preview = preview_from_manifest(&manifest, code.len());
    let name = name_override
        .or(manifest.name.clone())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| "Shared loadout".into());
    let now = now_rfc3339();
    let stored_code = if code.trim().starts_with("#emperor1") {
        code.trim().to_string()
    } else {
        encode_share_manifest(&manifest)?
    };
    Ok(SavedCollectionEntry {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        code: stored_code.clone(),
        created_at: now.clone(),
        updated_at: now,
        source_game_id,
        game: manifest.game.clone(),
        game_versions: manifest.game_versions.clone(),
        mod_count: preview.mods.len(),
        nexus_count: preview.nexus_count,
        thunderstore_count: preview.thunderstore_count,
        modio_count: preview.modio_count,
        share_id: Some(emperor_share_id(&stored_code)),
    })
}

pub fn load_saved_collections(paths: &Paths) -> Result<SavedCollectionsStore> {
    let file = paths.saved_collections_file();
    if !file.exists() {
        return Ok(SavedCollectionsStore::default());
    }
    let raw =
        std::fs::read_to_string(&file).with_context(|| format!("reading {}", file.display()))?;
    Ok(serde_json::from_str(&raw)?)
}

pub fn save_saved_collections(paths: &Paths, store: &SavedCollectionsStore) -> Result<()> {
    let file = paths.saved_collections_file();
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(store)?;
    std::fs::write(&file, raw)?;
    Ok(())
}

pub fn add_saved_collection(
    paths: &Paths,
    entry: SavedCollectionEntry,
) -> Result<SavedCollectionEntry> {
    let mut store = load_saved_collections(paths)?;
    store.collections.insert(0, entry.clone());
    save_saved_collections(paths, &store)?;
    Ok(entry)
}

pub struct RenamedSavedCollection {
    pub entry: SavedCollectionEntry,
    pub old_code: String,
}

pub fn rename_saved_collection(
    paths: &Paths,
    id: &str,
    name: &str,
) -> Result<RenamedSavedCollection> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Name cannot be empty."));
    }
    let mut store = load_saved_collections(paths)?;
    let entry = store
        .collections
        .iter_mut()
        .find(|c| c.id == id)
        .ok_or_else(|| anyhow!("saved collection not found"))?;
    let old_code = entry.code.clone();
    if entry.share_id.is_none() {
        entry.share_id = Some(emperor_share_id(old_code.trim()));
    }
    entry.name = trimmed.to_string();
    if let Ok(mut manifest) = decode_share_code(&old_code) {
        manifest.name = Some(trimmed.to_string());
        entry.code = encode_share_manifest(&manifest)?;
    }
    entry.updated_at = now_rfc3339();
    let cloned = entry.clone();
    save_saved_collections(paths, &store)?;
    Ok(RenamedSavedCollection {
        entry: cloned,
        old_code,
    })
}

pub fn rename_saved_collection_by_share(
    paths: &Paths,
    share_id: &str,
    old_code: Option<&str>,
    name: &str,
    new_code: Option<&str>,
) -> Result<Option<SavedCollectionEntry>> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("Name cannot be empty."));
    }
    let mut store = load_saved_collections(paths)?;
    let Some(entry) = store.collections.iter_mut().find(|c| {
        c.share_id.as_deref() == Some(share_id)
            || old_code.is_some_and(|code| c.code.trim() == code.trim())
    }) else {
        return Ok(None);
    };
    if entry.share_id.is_none() {
        entry.share_id = Some(share_id.to_string());
    }
    entry.name = trimmed.to_string();
    if let Some(code) = new_code {
        entry.code = code.to_string();
    } else if let Ok(mut manifest) = decode_share_code(&entry.code) {
        manifest.name = Some(trimmed.to_string());
        entry.code = encode_share_manifest(&manifest)?;
    }
    entry.updated_at = now_rfc3339();
    let cloned = entry.clone();
    save_saved_collections(paths, &store)?;
    Ok(Some(cloned))
}

pub fn update_saved_collection_snapshot(
    paths: &Paths,
    share_id: &str,
    old_code: Option<&str>,
    new_code: &str,
    game_versions: Option<Vec<String>>,
) -> Result<Option<SavedCollectionEntry>> {
    let manifest = decode_share_code(new_code)?;
    let preview = preview_from_manifest(&manifest, new_code.len());
    let mut store = load_saved_collections(paths)?;
    let Some(entry) = store.collections.iter_mut().find(|c| {
        c.share_id.as_deref() == Some(share_id)
            || old_code.is_some_and(|code| c.code == code)
    }) else {
        return Ok(None);
    };
    entry.code = new_code.to_string();
    entry.mod_count = preview.mods.len();
    entry.nexus_count = preview.nexus_count;
    entry.thunderstore_count = preview.thunderstore_count;
    entry.modio_count = preview.modio_count;
    if game_versions.is_some() {
        entry.game_versions = game_versions;
    }
    entry.share_id = Some(share_id.to_string());
    entry.updated_at = now_rfc3339();
    let cloned = entry.clone();
    save_saved_collections(paths, &store)?;
    Ok(Some(cloned))
}

pub fn delete_saved_collection(paths: &Paths, id: &str) -> Result<()> {
    let mut store = load_saved_collections(paths)?;
    let before = store.collections.len();
    store.collections.retain(|c| c.id != id);
    if store.collections.len() == before {
        bail!("saved collection not found");
    }
    save_saved_collections(paths, &store)
}

pub fn get_saved_collection(paths: &Paths, id: &str) -> Result<SavedCollectionDetail> {
    let store = load_saved_collections(paths)?;
    let entry = store
        .collections
        .into_iter()
        .find(|c| c.id == id)
        .ok_or_else(|| anyhow!("saved collection not found"))?;
    let manifest = decode_share_code(&entry.code)?;
    Ok(SavedCollectionDetail {
        entry,
        mods: manifest.mods,
    })
}

fn share_mod_identity(
    m: &ShareModEntry,
) -> (
    &ShareModSource,
    Option<&str>,
    Option<u64>,
    Option<u64>,
    Option<&str>,
    Option<&str>,
    Option<&str>,
    Option<u32>,
    Option<&str>,
) {
    (
        &m.s,
        m.domain.as_deref(),
        m.mod_id,
        m.file_id,
        m.community.as_deref(),
        m.namespace.as_deref(),
        m.name.as_deref(),
        m.game_id,
        m.version.as_deref(),
    )
}

pub fn manifest_mods_match(a: &[ShareModEntry], b: &[ShareModEntry]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut keys_a: Vec<_> = a.iter().map(share_mod_identity).collect();
    let mut keys_b: Vec<_> = b.iter().map(share_mod_identity).collect();
    keys_a.sort_by_key(|k| format!("{k:?}"));
    keys_b.sort_by_key(|k| format!("{k:?}"));
    keys_a == keys_b
}

pub fn is_loadout_saved_in_personal_collections(
    paths: &Paths,
    game: &ManagedGame,
    manifest: &ShareManifest,
    code: &str,
) -> Result<bool> {
    let store = load_saved_collections(paths)?;
    let trimmed = code.trim();
    let target_share_id = emperor_share_id(trimmed);
    for entry in &store.collections {
        let entry_trimmed = entry.code.trim();
        if entry_trimmed == trimmed || emperor_share_id(entry_trimmed) == target_share_id {
            return Ok(true);
        }
        if let Ok(saved_manifest) = decode_share_code(entry_trimmed) {
            let same_game = entry.source_game_id.as_deref() == Some(game.id.as_str())
                || catalogs_overlap(&saved_manifest.game, game).is_ok();
            if same_game && manifest_mods_match(&manifest.mods, &saved_manifest.mods) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

pub fn mod_label(entry: &ShareModEntry) -> String {
    entry
        .display_name
        .clone()
        .or_else(|| entry.name.clone())
        .unwrap_or_else(|| match entry.s {
            ShareModSource::Nexus => format!(
                "Nexus {}:{}",
                entry.mod_id.unwrap_or(0),
                entry.file_id.unwrap_or(0)
            ),
            ShareModSource::Thunderstore => format!(
                "{}-{}",
                entry.namespace.as_deref().unwrap_or("?"),
                entry.name.as_deref().unwrap_or("?")
            ),
            ShareModSource::Modio => format!("mod.io {}", entry.mod_id.unwrap_or(0)),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::StagedMod;

    fn sample_manifest() -> ShareManifest {
        ShareManifest {
            v: 1,
            name: Some("Test pack".into()),
            game: ShareGameHint {
                id: Some("game1".into()),
                plugin_id: Some("unity_bepinex".into()),
                nexus_domain: Some("skyrimspecialedition".into()),
                thunderstore_community: Some("lethal-company".into()),
                modio_game_id: Some(42),
                title: Some("Skyrim Special Edition".into()),
                project_name: Some("Pal".into()),
            },
            game_versions: Some(vec!["1.6.1170".into()]),
            mods: vec![
                ShareModEntry {
                    s: ShareModSource::Nexus,
                    domain: Some("skyrimspecialedition".into()),
                    mod_id: Some(10),
                    file_id: Some(20),
                    community: None,
                    namespace: None,
                    name: None,
                    game_id: None,
                    display_name: Some("Cool Mod".into()),
                    version: Some("1.0".into()),
                    fomod: None,
                    options: None,
                    enabled: None,
                },
                ShareModEntry {
                    s: ShareModSource::Thunderstore,
                    domain: None,
                    mod_id: None,
                    file_id: None,
                    community: Some("lethal-company".into()),
                    namespace: Some("Owner".into()),
                    name: Some("Pkg".into()),
                    game_id: None,
                    display_name: Some("Pkg".into()),
                    version: Some("2.0.0".into()),
                    fomod: None,
                    options: None,
                    enabled: None,
                },
                ShareModEntry {
                    s: ShareModSource::Modio,
                    domain: None,
                    mod_id: Some(7),
                    file_id: Some(8),
                    community: None,
                    namespace: None,
                    name: None,
                    game_id: Some(42),
                    display_name: Some("Mio".into()),
                    version: None,
                    fomod: None,
                    options: None,
                    enabled: None,
                },
            ],
        }
    }

    #[test]
    fn round_trip_encode_decode() {
        let m = sample_manifest();
        let code = encode_share_manifest(&m).unwrap();
        assert!(code.starts_with(SHARE_PREFIX));
        let back = decode_share_code(&code).unwrap();
        assert_eq!(back.v, 1);
        assert_eq!(back.name.as_deref(), Some("Test pack"));
        assert_eq!(back.mods.len(), 3);
        assert_eq!(back.mods[0].mod_id, Some(10));
        assert_eq!(back.mods[1].namespace.as_deref(), Some("Owner"));
        assert_eq!(back.mods[2].game_id, Some(42));
        assert_eq!(back.game.title.as_deref(), Some("Skyrim Special Edition"));
        assert_eq!(back.game.project_name.as_deref(), Some("Pal"));
    }

    #[test]
    fn decode_share_without_game_title() {
        let raw = r#"{"v":1,"name":"Old","game":{"plugin_id":"bepinex"},"mods":[]}"#;
        let manifest: ShareManifest = serde_json::from_str(raw).unwrap();
        assert_eq!(manifest.game.title, None);
        assert_eq!(manifest.game.project_name, None);
        let back = decode_share_code(&encode_share_manifest(&manifest).unwrap()).unwrap();
        assert_eq!(back.game.plugin_id.as_deref(), Some("bepinex"));
        assert_eq!(back.game.title, None);
        assert_eq!(back.game.project_name, None);
    }

    #[test]
    fn decode_tolerates_whitespace_wrapped_payload() {
        let code = encode_share_manifest(&sample_manifest()).unwrap();
        let (prefix, payload) = code.split_once('\n').expect("prefix newline");
        let mut wrapped = format!("{prefix}\n");
        for (i, c) in payload.chars().enumerate() {
            if i > 0 && i % 40 == 0 {
                wrapped.push('\n');
            }
            wrapped.push(c);
        }
        let bom_wrapped = format!("\u{feff}{wrapped}");
        let back = decode_share_code(&bom_wrapped).unwrap();
        assert_eq!(back.mods.len(), 3);
        assert_eq!(back.name.as_deref(), Some("Test pack"));
    }

    #[test]
    fn corrupt_payload_gives_discord_hint() {
        let code = encode_share_manifest(&sample_manifest()).unwrap();
        let (prefix, payload) = code.split_once('\n').expect("prefix newline");
        let compressed = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(payload.as_bytes())
            .unwrap();
        // Truncated DEFLATE may still "inflate" under raw deflate; either that or
        // the subsequent JSON parse must surface the Discord/file hint.
        let truncated = &compressed[..compressed.len() / 2];
        let corrupt = format!(
            "{prefix}\n{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(truncated)
        );
        let err = decode_share_code(&corrupt).unwrap_err().to_string();
        assert!(
            err.contains("Discord") || err.contains(".emperor-share"),
            "unexpected error: {err}"
        );

        let garbage = format!(
            "{prefix}\n{}",
            base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(b"not-a-deflate-stream!!!!")
        );
        let err = decode_share_code(&garbage).unwrap_err().to_string();
        assert!(
            err.contains("Discord") || err.contains(".emperor-share"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn detect_kinds() {
        let code = encode_share_manifest(&sample_manifest()).unwrap();
        assert_eq!(detect_code_kind(&code), ImportCodeKind::Emperor);
        assert_eq!(
            detect_code_kind("AbCd1234"),
            ImportCodeKind::ThunderstoreProfile
        );
        assert_eq!(
            detect_code_kind("#r2modman\nAAAA"),
            ImportCodeKind::ThunderstoreProfile
        );
        assert_eq!(detect_code_kind("not a code!!!"), ImportCodeKind::Unknown);
        assert_eq!(detect_code_kind(""), ImportCodeKind::Unknown);
    }

    #[test]
    fn skips_non_portable() {
        let game = ManagedGame {
            id: "g".into(),
            title: "G".into(),
            nexus_domain: "domain".into(),
            install_path: "/tmp".into(),
            launcher: "steam".into(),
            plugin_id: "p".into(),
            cover_path: None,
            cover_url: None,
            project_name: Some("Phoenix".into()),
            thunderstore_community: None,
            modio_game_id: None,
            tool_overrides: None,
            game_version: None,
            ignored_requirements: Vec::new(),
            launch_executable: None,
            launch_command_source: Default::default(),
            custom_env_vars: Vec::new(),
            custom_wrapper: None,
            custom_launch_args: None,
            wine_overrides_dismissed: false,
        };
        let order = LoadOrder {
            mods: vec![
                StagedMod {
                    id: "1_2".into(),
                    name: "Good".into(),
                    source: ModSource::Nexus,
                    nexus_mod_id: 1,
                    nexus_file_id: 2,
                    enabled: true,
                    order: 0,
                    domain: "domain".into(),
                    ..Default::default()
                },
                StagedMod {
                    id: "0_9".into(),
                    name: "Manual".into(),
                    source: ModSource::Nexus,
                    nexus_mod_id: 0,
                    nexus_file_id: 9,
                    enabled: true,
                    order: 1,
                    domain: "domain".into(),
                    ..Default::default()
                },
                StagedMod {
                    id: "disabled".into(),
                    name: "Off".into(),
                    source: ModSource::Nexus,
                    nexus_mod_id: 3,
                    nexus_file_id: 4,
                    enabled: false,
                    order: 2,
                    domain: "domain".into(),
                    ..Default::default()
                },
                StagedMod {
                    id: "overlay_x".into(),
                    name: "Overlay".into(),
                    source: ModSource::Thunderstore,
                    enabled: true,
                    order: 3,
                    ..Default::default()
                },
            ],
        };
        let (manifest, skipped, member_ids) = build_manifest_from_loadorder(&order, &game, None);
        assert_eq!(manifest.game.title.as_deref(), Some("G"));
        assert_eq!(manifest.game.project_name.as_deref(), Some("Phoenix"));
        assert_eq!(manifest.mods.len(), 1);
        assert_eq!(manifest.mods[0].mod_id, Some(1));
        assert_eq!(skipped.len(), 2);
        assert_eq!(member_ids, vec!["1_2".to_string()]);
    }

    #[test]
    fn catalogs_overlap_ok() {
        let game = ManagedGame {
            id: "g".into(),
            title: "G".into(),
            nexus_domain: "skyrimspecialedition".into(),
            install_path: "/tmp".into(),
            launcher: "steam".into(),
            plugin_id: "p".into(),
            cover_path: None,
            cover_url: None,
            project_name: None,
            thunderstore_community: Some("lethal-company".into()),
            modio_game_id: Some(42),
            tool_overrides: None,
            game_version: None,
            ignored_requirements: Vec::new(),
            launch_executable: None,
            launch_command_source: Default::default(),
            custom_env_vars: Vec::new(),
            custom_wrapper: None,
            custom_launch_args: None,
            wine_overrides_dismissed: false,
        };
        let hint = sample_manifest().game;
        assert!(catalogs_overlap(&hint, &game).is_ok());
    }

    #[test]
    fn catalogs_overlap_fail() {
        let game = ManagedGame {
            id: "g".into(),
            title: "G".into(),
            nexus_domain: "fallout4".into(),
            install_path: "/tmp".into(),
            launcher: "steam".into(),
            plugin_id: "p".into(),
            cover_path: None,
            cover_url: None,
            project_name: None,
            thunderstore_community: None,
            modio_game_id: None,
            tool_overrides: None,
            game_version: None,
            ignored_requirements: Vec::new(),
            launch_executable: None,
            launch_command_source: Default::default(),
            custom_env_vars: Vec::new(),
            custom_wrapper: None,
            custom_launch_args: None,
            wine_overrides_dismissed: false,
        };
        let hint = ShareGameHint {
            id: None,
            plugin_id: None,
            nexus_domain: Some("skyrimspecialedition".into()),
            thunderstore_community: None,
            modio_game_id: None,
            title: None,
            project_name: None,
        };
        assert!(catalogs_overlap(&hint, &game).is_err());
    }

    #[test]
    fn loadout_saved_in_personal_collections_matches_code_and_renamed_mods() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let game = ManagedGame {
            id: "skyrim-1".into(),
            title: "Skyrim SE".into(),
            nexus_domain: "skyrimspecialedition".into(),
            install_path: "/tmp".into(),
            launcher: "steam".into(),
            plugin_id: "skyrim".into(),
            cover_path: None,
            cover_url: None,
            project_name: None,
            thunderstore_community: None,
            modio_game_id: None,
            tool_overrides: None,
            game_version: None,
            ignored_requirements: Vec::new(),
            launch_executable: None,
            launch_command_source: Default::default(),
            custom_env_vars: Vec::new(),
            custom_wrapper: None,
            custom_launch_args: None,
            wine_overrides_dismissed: false,
        };
        let manifest = sample_manifest();
        let code = encode_share_manifest(&manifest).unwrap();
        assert!(!is_loadout_saved_in_personal_collections(&paths, &game, &manifest, &code).unwrap());

        // Save with a custom name (different encoded code because manifest.name differs).
        let mut named_manifest = manifest.clone();
        named_manifest.name = Some("My Renamed Loadout".into());
        let named_code = encode_share_manifest(&named_manifest).unwrap();
        let entry = entry_from_code(&named_code, Some("My Renamed Loadout".into()), Some(game.id.clone())).unwrap();
        add_saved_collection(&paths, entry).unwrap();

        assert!(is_loadout_saved_in_personal_collections(&paths, &game, &manifest, &code).unwrap());

        // Different mod list should not match.
        let mut other_manifest = manifest.clone();
        other_manifest.mods.pop();
        let other_code = encode_share_manifest(&other_manifest).unwrap();
        assert!(!is_loadout_saved_in_personal_collections(&paths, &game, &other_manifest, &other_code).unwrap());
    }

    #[test]
    fn share_manifest_round_trips_fomod_selection() {
        let mut selected = std::collections::BTreeMap::new();
        selected.insert("group_1".to_string(), vec!["plugin_a".to_string()]);
        let fomod = crate::mods::fomod::FomodSelection { selected };

        let mut manifest = sample_manifest();
        manifest.mods[0].fomod = Some(fomod.clone());
        let code = encode_share_manifest(&manifest).unwrap();
        let decoded = decode_share_code(&code).unwrap();
        assert_eq!(decoded.mods[0].fomod, Some(fomod));
        assert!(decoded.mods[1].fomod.is_none());
    }

    #[test]
    fn build_manifest_from_loadorder_includes_fomod_options_and_order() {
        let game = ManagedGame {
            id: "g".into(),
            title: "G".into(),
            nexus_domain: "skyrimspecialedition".into(),
            install_path: "/tmp".into(),
            launcher: "steam".into(),
            plugin_id: "p".into(),
            cover_path: None,
            cover_url: None,
            project_name: None,
            thunderstore_community: None,
            modio_game_id: None,
            tool_overrides: None,
            game_version: None,
            ignored_requirements: Vec::new(),
            launch_executable: None,
            launch_command_source: Default::default(),
            custom_env_vars: Vec::new(),
            custom_wrapper: None,
            custom_launch_args: None,
            wine_overrides_dismissed: false,
        };
        let mut selected = std::collections::BTreeMap::new();
        selected.insert("step_1".to_string(), vec!["opt_b".to_string()]);
        let fomod = crate::mods::fomod::FomodSelection { selected };
        let mut sub_choice = std::collections::BTreeMap::new();
        sub_choice.insert("group_1".to_string(), "choice_a".to_string());
        let options = crate::mods::options::ModOptionSelection {
            enabled_options: vec!["group_1".to_string()],
            sub_choice,
        };

        let order = LoadOrder {
            mods: vec![
                StagedMod {
                    id: "20_2".into(),
                    name: "Second In Vec, First In Order".into(),
                    source: ModSource::Nexus,
                    nexus_mod_id: 20,
                    nexus_file_id: 2,
                    enabled: true,
                    order: 1,
                    domain: "skyrimspecialedition".into(),
                    fomod_selection: Some(fomod.clone()),
                    ..Default::default()
                },
                StagedMod {
                    id: "10_1".into(),
                    name: "First In Vec, Second In Order".into(),
                    source: ModSource::Nexus,
                    nexus_mod_id: 10,
                    nexus_file_id: 1,
                    enabled: true,
                    order: 2,
                    domain: "skyrimspecialedition".into(),
                    option_selection: Some(options.clone()),
                    ..Default::default()
                },
            ],
        };
        // Reverse vec order to confirm sorting by `m.order`.
        let mut reversed = order;
        reversed.mods.reverse();

        let (manifest, skipped, member_ids) =
            build_manifest_from_loadorder(&reversed, &game, Some("Pack".into()));
        assert!(skipped.is_empty());
        assert_eq!(member_ids, vec!["20_2".to_string(), "10_1".to_string()]);
        assert_eq!(manifest.mods.len(), 2);
        assert_eq!(manifest.mods[0].mod_id, Some(20));
        assert_eq!(manifest.mods[0].fomod, Some(fomod));
        assert_eq!(manifest.mods[1].mod_id, Some(10));
        assert_eq!(manifest.mods[1].options, Some(options));
    }

    #[test]
    fn rename_saved_collection_updates_code_and_preserves_share_id() {
        let dir = tempfile::tempdir().unwrap();
        let paths = Paths {
            config_dir: dir.path().join("cfg"),
            data_dir: dir.path().join("data"),
            cache_dir: dir.path().join("cache"),
        };
        let manifest = sample_manifest();
        let initial_code = encode_share_manifest(&manifest).unwrap();
        let initial_share_id = emperor_share_id(&initial_code);
        let entry = entry_from_code(&initial_code, None, Some("g".into())).unwrap();
        let saved = add_saved_collection(&paths, entry).unwrap();

        let renamed = rename_saved_collection(&paths, &saved.id, "  Renamed Pack  ").unwrap();
        assert_eq!(renamed.old_code, initial_code);
        assert_eq!(renamed.entry.name, "Renamed Pack");
        assert_eq!(renamed.entry.share_id.as_deref(), Some(initial_share_id.as_str()));
        let decoded = decode_share_code(&renamed.entry.code).unwrap();
        assert_eq!(decoded.name.as_deref(), Some("Renamed Pack"));

        let via_share = rename_saved_collection_by_share(
            &paths,
            &initial_share_id,
            Some(&renamed.entry.code),
            "Synced Name",
            None,
        )
        .unwrap()
        .expect("saved entry matched by share_id");
        assert_eq!(via_share.name, "Synced Name");
        let decoded2 = decode_share_code(&via_share.code).unwrap();
        assert_eq!(decoded2.name.as_deref(), Some("Synced Name"));
    }

    #[test]
    fn build_manifest_for_collection_records_disabled_mods_and_changes_hash() {
        let game = ManagedGame {
            id: "skyrim_1".into(),
            plugin_id: "skyrimse".into(),
            title: "Skyrim SE".into(),
            nexus_domain: "skyrimspecialedition".into(),
            install_path: String::new(),
            launcher: "steam".into(),
            cover_path: None,
            cover_url: None,
            project_name: None,
            thunderstore_community: None,
            modio_game_id: None,
            tool_overrides: None,
            game_version: None,
            ignored_requirements: Vec::new(),
            launch_executable: None,
            launch_command_source: Default::default(),
            custom_env_vars: Vec::new(),
            custom_wrapper: None,
            custom_launch_args: None,
            wine_overrides_dismissed: false,
        };
        let mut order = LoadOrder {
            mods: vec![
                StagedMod {
                    id: "m1".into(),
                    name: "Mod One".into(),
                    source: ModSource::Nexus,
                    domain: "skyrimspecialedition".into(),
                    nexus_mod_id: 1,
                    nexus_file_id: 10,
                    enabled: true,
                    order: 1,
                    collection_ids: vec!["col-1".into()],
                    ..Default::default()
                },
                StagedMod {
                    id: "m2".into(),
                    name: "Mod Two".into(),
                    source: ModSource::Nexus,
                    domain: "skyrimspecialedition".into(),
                    nexus_mod_id: 2,
                    nexus_file_id: 20,
                    enabled: true,
                    order: 2,
                    collection_ids: vec!["col-1".into()],
                    ..Default::default()
                },
            ],
        };

        let (m_all_enabled, _, _) =
            build_manifest_for_collection(&order, &game, "col-1", Some("Pack".into()));
        assert_eq!(m_all_enabled.mods[0].enabled, None);
        assert_eq!(m_all_enabled.mods[1].enabled, None);
        let code_enabled = encode_share_manifest(&m_all_enabled).unwrap();

        order.mods[1].enabled = false;
        let (m_one_disabled, _, _) =
            build_manifest_for_collection(&order, &game, "col-1", Some("Pack".into()));
        assert_eq!(m_one_disabled.mods[0].enabled, None);
        assert_eq!(m_one_disabled.mods[1].enabled, Some(false));
        let code_disabled = encode_share_manifest(&m_one_disabled).unwrap();

        assert_ne!(share_hash(&code_enabled), share_hash(&code_disabled));
    }
}
