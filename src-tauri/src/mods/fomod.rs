//! FOMOD (ModuleConfig.xml) installer support.
//!
//! Detects `fomod/ModuleConfig.xml` under a staged content root, presents steps
//! to the UI, stores the user's picks on the staged mod, and at deploy time
//! resolves selected plugins into a source→destination file map. Game plugins
//! still decide final install paths via `resolve_deploy_root`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use crate::games::normalize_relative;
use crate::mods::options::option_id;

/// Stable selection persisted on [`crate::mods::StagedMod`].
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct FomodSelection {
    /// Group id → selected plugin ids.
    #[serde(default)]
    pub selected: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FomodInstallerView {
    pub module_name: String,
    pub steps: Vec<FomodStepView>,
    pub selection: FomodSelection,
    /// True when this package needs a C# scripted installer we do not run.
    pub unsupported_scripted: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FomodStepView {
    pub id: String,
    pub name: String,
    pub visible: bool,
    pub groups: Vec<FomodGroupView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FomodGroupView {
    pub id: String,
    pub name: String,
    #[serde(rename = "type")]
    pub type_: FomodGroupType,
    pub plugins: Vec<FomodPluginView>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FomodGroupType {
    SelectAny,
    SelectAll,
    SelectExactlyOne,
    SelectAtMostOne,
    SelectAtLeastOne,
}

impl FomodGroupType {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "selectall" => Self::SelectAll,
            "selectexactlyone" => Self::SelectExactlyOne,
            "selectatmostone" => Self::SelectAtMostOne,
            "selectatleastone" => Self::SelectAtLeastOne,
            _ => Self::SelectAny,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FomodPluginView {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    /// Absolute path for the asset protocol, when present.
    pub image: Option<String>,
    pub plugin_type: FomodPluginType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum FomodPluginType {
    Optional,
    Required,
    Recommended,
    CouldBeUsable,
    NotUsable,
}

impl FomodPluginType {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "required" => Self::Required,
            "recommended" => Self::Recommended,
            "couldbeusable" => Self::CouldBeUsable,
            "notusable" => Self::NotUsable,
            _ => Self::Optional,
        }
    }

    fn is_forced_on(self) -> bool {
        matches!(self, Self::Required)
    }

    fn is_disabled(self) -> bool {
        matches!(self, Self::NotUsable)
    }
}

#[derive(Debug, Clone)]
struct FomodFileRef {
    source: String,
    destination: String,
    priority: i32,
    is_folder: bool,
}

#[derive(Debug, Clone)]
struct ConditionFlag {
    name: String,
    value: String,
}

#[derive(Debug, Clone)]
enum Dependency {
    And(Vec<Dependency>),
    Or(Vec<Dependency>),
    Flag {
        name: String,
        value: String,
    },
    /// Soft: only fails when we can prove the file is missing from the game.
    File {
        file: String,
        state: String,
    },
    /// Soft game version checks — treated as satisfied.
    GameVersion,
    /// Unknown leaf — treated as satisfied so we do not hard-block.
    Unknown,
}

#[derive(Debug, Clone)]
pub(crate) struct FomodPlugin {
    pub(crate) id: String,
    name: String,
    description: Option<String>,
    image: Option<String>,
    files: Vec<FomodFileRef>,
    flags: Vec<ConditionFlag>,
    plugin_type: FomodPluginType,
    /// conditionalPluginType patterns: (deps, type). First match wins; else default.
    conditional_types: Vec<(Dependency, FomodPluginType)>,
}

#[derive(Debug, Clone)]
pub(crate) struct FomodGroup {
    pub(crate) id: String,
    name: String,
    type_: FomodGroupType,
    pub(crate) plugins: Vec<FomodPlugin>,
}

#[derive(Debug, Clone)]
pub(crate) struct FomodStep {
    id: String,
    name: String,
    visible: Option<Dependency>,
    pub(crate) groups: Vec<FomodGroup>,
}

#[derive(Debug, Clone)]
struct ConditionalPattern {
    dependencies: Dependency,
    files: Vec<FomodFileRef>,
}

/// Parsed ModuleConfig used for selection + deploy.
#[derive(Debug, Clone)]
pub struct FomodModule {
    pub module_name: String,
    required: Vec<FomodFileRef>,
    pub(crate) steps: Vec<FomodStep>,
    conditional_installs: Vec<ConditionalPattern>,
    pub unsupported_scripted: bool,
}

/// One resolved file to link at deploy time.
#[derive(Debug, Clone)]
pub struct FomodMappedFile {
    pub source: PathBuf,
    pub dest_rel: PathBuf,
}

/// Look for ModuleConfig under the staged content root.
pub fn module_config_path(content_root: &Path) -> Option<PathBuf> {
    let dir = fomod_config_dir(content_root)?;
    for name in ["ModuleConfig.xml", "moduleconfig.xml", "ModuleConfig.XML"] {
        let p = dir.join(name);
        if p.is_file() {
            return Some(p);
        }
    }
    // Case-insensitive fallback for unusual archives.
    let Ok(entries) = fs::read_dir(&dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let lower = name.to_string_lossy().to_ascii_lowercase();
        if lower == "moduleconfig.xml" {
            return Some(entry.path());
        }
    }
    None
}

/// `fomod/` as stored in the archive. Windows installers often use `Fomod`.
fn fomod_config_dir(content_root: &Path) -> Option<PathBuf> {
    let exact = content_root.join("fomod");
    if exact.is_dir() {
        return Some(exact);
    }
    let entries = fs::read_dir(content_root).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case("fomod")
        {
            return Some(path);
        }
    }
    None
}

pub fn has_fomod(content_root: &Path) -> bool {
    module_config_path(content_root).is_some()
}

pub fn detect_scripted_installer(content_root: &Path) -> bool {
    let Some(fomod) = fomod_config_dir(content_root) else {
        return false;
    };
    let Ok(entries) = fs::read_dir(&fomod) else {
        return false;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
        if name.ends_with(".dll") || name == "script.cs" || name.ends_with(".cs") {
            return true;
        }
    }
    false
}

pub fn load_module(content_root: &Path) -> Result<Option<FomodModule>> {
    let Some(path) = module_config_path(content_root) else {
        return Ok(None);
    };
    let mut module = parse_module_config(&path)?;
    if detect_scripted_installer(content_root) {
        module.unsupported_scripted = true;
    }
    Ok(Some(module))
}

/// UI-facing view with absolute image paths and visibility against `selection`.
pub fn installer_view(
    content_root: &Path,
    stored: Option<&FomodSelection>,
) -> Result<Option<FomodInstallerView>> {
    let Some(module) = load_module(content_root)? else {
        return Ok(None);
    };
    let selection = effective_selection(&module, stored);
    let flags = collect_flags(&module, &selection);
    let steps = module
        .steps
        .iter()
        .map(|step| {
            let visible = step
                .visible
                .as_ref()
                .map(|d| eval_dependency(d, &flags, None))
                .unwrap_or(true);
            FomodStepView {
                id: step.id.clone(),
                name: step.name.clone(),
                visible,
                groups: step
                    .groups
                    .iter()
                    .map(|g| FomodGroupView {
                        id: g.id.clone(),
                        name: g.name.clone(),
                        type_: g.type_,
                        plugins: g
                            .plugins
                            .iter()
                            .map(|p| {
                                let plugin_type = resolve_plugin_type(p, &flags);
                                FomodPluginView {
                                    id: p.id.clone(),
                                    name: p.name.clone(),
                                    description: p.description.clone(),
                                    image: p
                                        .image
                                        .as_deref()
                                        .and_then(|rel| resolve_fomod_image(content_root, rel)),
                                    plugin_type,
                                }
                            })
                            .collect(),
                    })
                    .collect(),
            }
        })
        .collect();
    Ok(Some(FomodInstallerView {
        module_name: module.module_name,
        steps,
        selection,
        unsupported_scripted: module.unsupported_scripted,
    }))
}

pub fn default_selection(module: &FomodModule) -> FomodSelection {
    let mut selected = BTreeMap::new();
    // Empty flags for first pass defaults (visibility uses empty → steps without
    // deps show; steps gated on flags may be skipped until the user picks).
    let empty_flags = HashMap::new();
    for step in &module.steps {
        let visible = step
            .visible
            .as_ref()
            .map(|d| eval_dependency(d, &empty_flags, None))
            .unwrap_or(true);
        // Still seed defaults for invisible steps so later flag updates have a base.
        let _ = visible;
        for group in &step.groups {
            selected.insert(group.id.clone(), default_group_plugins(group));
        }
    }
    // Recompute with flags from the seeded selection so Recommended picks in
    // later steps that depend on earlier flags get sensible defaults.
    let mut selection = FomodSelection { selected };
    for _ in 0..4 {
        let flags = collect_flags(module, &selection);
        let mut next = BTreeMap::new();
        for step in &module.steps {
            let visible = step
                .visible
                .as_ref()
                .map(|d| eval_dependency(d, &flags, None))
                .unwrap_or(true);
            for group in &step.groups {
                if !visible {
                    // Keep prior picks for invisible steps so toggling visibility
                    // does not wipe them, but prefer required plugins.
                    if let Some(prev) = selection.selected.get(&group.id) {
                        next.insert(group.id.clone(), prev.clone());
                    } else {
                        next.insert(group.id.clone(), default_group_plugins(group));
                    }
                    continue;
                }
                let existing = selection
                    .selected
                    .get(&group.id)
                    .cloned()
                    .unwrap_or_default();
                next.insert(
                    group.id.clone(),
                    normalize_group_plugins(group, &existing, &flags, true),
                );
            }
        }
        selection.selected = next;
    }
    selection
}

pub fn normalize_selection(module: &FomodModule, selection: &FomodSelection) -> FomodSelection {
    let mut flags = HashMap::new();
    let mut selected = BTreeMap::new();
    for step in &module.steps {
        let visible = step
            .visible
            .as_ref()
            .map(|d| eval_dependency(d, &flags, None))
            .unwrap_or(true);
        if !visible {
            for group in &step.groups {
                if let Some(prev) = selection.selected.get(&group.id) {
                    selected.insert(group.id.clone(), prev.clone());
                } else {
                    selected.insert(group.id.clone(), default_group_plugins(group));
                }
            }
            continue;
        }
        for group in &step.groups {
            let has_entry = selection.selected.contains_key(&group.id);
            let existing = selection
                .selected
                .get(&group.id)
                .cloned()
                .unwrap_or_default();
            let normalized = normalize_group_plugins(group, &existing, &flags, !has_entry);
            for plugin in &group.plugins {
                if normalized.contains(&plugin.id) {
                    for flag in &plugin.flags {
                        flags.insert(flag.name.clone(), flag.value.clone());
                    }
                }
            }
            selected.insert(group.id.clone(), normalized);
        }
    }
    FomodSelection { selected }
}

pub fn effective_selection(
    module: &FomodModule,
    stored: Option<&FomodSelection>,
) -> FomodSelection {
    match stored {
        Some(s) => normalize_selection(module, s),
        None => default_selection(module),
    }
}

fn default_group_plugins(group: &FomodGroup) -> Vec<String> {
    normalize_group_plugins(group, &[], &HashMap::new(), true)
}

fn normalize_group_plugins(
    group: &FomodGroup,
    existing: &[String],
    flags: &HashMap<String, String>,
    seed_recommended: bool,
) -> Vec<String> {
    let plugins: Vec<_> = group
        .plugins
        .iter()
        .map(|p| (p, resolve_plugin_type(p, flags)))
        .collect();

    let mut chosen: Vec<String> = existing
        .iter()
        .filter(|id| {
            plugins
                .iter()
                .any(|(p, t)| p.id == **id && !t.is_disabled())
        })
        .cloned()
        .collect();

    for (p, t) in &plugins {
        if t.is_forced_on() && !chosen.contains(&p.id) {
            chosen.push(p.id.clone());
        }
    }
    chosen.retain(|id| plugins.iter().any(|(p, t)| p.id == *id && !t.is_disabled()));

    match group.type_ {
        FomodGroupType::SelectAll => plugins
            .iter()
            .filter(|(_, t)| !t.is_disabled())
            .map(|(p, _)| p.id.clone())
            .collect(),
        FomodGroupType::SelectExactlyOne => {
            if chosen.len() == 1 {
                return chosen;
            }
            if let Some((p, _)) = plugins
                .iter()
                .find(|(p, t)| chosen.contains(&p.id) && !t.is_disabled())
            {
                return vec![p.id.clone()];
            }
            if let Some((p, _)) = plugins
                .iter()
                .find(|(_, t)| matches!(t, FomodPluginType::Recommended))
            {
                return vec![p.id.clone()];
            }
            if let Some((p, _)) = plugins.iter().find(|(_, t)| !t.is_disabled()) {
                return vec![p.id.clone()];
            }
            Vec::new()
        }
        FomodGroupType::SelectAtMostOne => {
            if chosen.len() <= 1 {
                return chosen;
            }
            vec![chosen[0].clone()]
        }
        FomodGroupType::SelectAtLeastOne => {
            if !chosen.is_empty() {
                return chosen;
            }
            if let Some((p, _)) = plugins
                .iter()
                .find(|(_, t)| matches!(t, FomodPluginType::Recommended))
            {
                return vec![p.id.clone()];
            }
            if let Some((p, _)) = plugins.iter().find(|(_, t)| !t.is_disabled()) {
                return vec![p.id.clone()];
            }
            Vec::new()
        }
        FomodGroupType::SelectAny => {
            if chosen.is_empty() && seed_recommended {
                // Pre-select Recommended plugins for SelectAny when uninitialized.
                return plugins
                    .iter()
                    .filter(|(_, t)| matches!(t, FomodPluginType::Recommended) || t.is_forced_on())
                    .map(|(p, _)| p.id.clone())
                    .collect();
            }
            chosen
        }
    }
}

fn resolve_plugin_type(plugin: &FomodPlugin, flags: &HashMap<String, String>) -> FomodPluginType {
    for (deps, t) in &plugin.conditional_types {
        if eval_dependency(deps, flags, None) {
            return *t;
        }
    }
    plugin.plugin_type
}

fn collect_flags(module: &FomodModule, selection: &FomodSelection) -> HashMap<String, String> {
    let mut flags = HashMap::new();
    for step in &module.steps {
        let visible = step
            .visible
            .as_ref()
            .map(|d| eval_dependency(d, &flags, None))
            .unwrap_or(true);
        if !visible {
            continue;
        }
        for group in &step.groups {
            let Some(ids) = selection.selected.get(&group.id) else {
                continue;
            };
            for plugin in &group.plugins {
                if !ids.contains(&plugin.id) {
                    continue;
                }
                for flag in &plugin.flags {
                    flags.insert(flag.name.clone(), flag.value.clone());
                }
            }
        }
    }
    flags
}

fn eval_dependency(
    dep: &Dependency,
    flags: &HashMap<String, String>,
    game_files: Option<&HashSet<String>>,
) -> bool {
    match dep {
        Dependency::And(xs) => xs.iter().all(|d| eval_dependency(d, flags, game_files)),
        Dependency::Or(xs) => {
            xs.is_empty() || xs.iter().any(|d| eval_dependency(d, flags, game_files))
        }
        Dependency::Flag { name, value } => flags.get(name).map(|v| v == value).unwrap_or(false),
        Dependency::File { file, state } => {
            let lower = file.replace('\\', "/").to_ascii_lowercase();
            let present = game_files
                .map(|set| {
                    set.iter()
                        .any(|f| f.replace('\\', "/").to_ascii_lowercase() == lower)
                })
                .unwrap_or(true); // soft: unknown → satisfy
            match state.to_ascii_lowercase().as_str() {
                "missing" => !present,
                "inactive" => present, // soft: treat present as inactive-ok
                _ => present,          // Active / default
            }
        }
        Dependency::GameVersion | Dependency::Unknown => true,
    }
}

/// Build the deploy file list for the current selection.
pub fn build_file_map(
    content_root: &Path,
    module: &FomodModule,
    selection: &FomodSelection,
) -> Result<Vec<FomodMappedFile>> {
    let flags = collect_flags(module, selection);
    let mut by_dest: BTreeMap<String, MappedDest> = BTreeMap::new();

    let mut push_refs = |refs: &[FomodFileRef]| -> Result<()> {
        for r in refs {
            expand_file_ref(content_root, r, &mut by_dest)?;
        }
        Ok(())
    };

    push_refs(&module.required)?;

    for step in &module.steps {
        let visible = step
            .visible
            .as_ref()
            .map(|d| eval_dependency(d, &flags, None))
            .unwrap_or(true);
        if !visible {
            continue;
        }
        for group in &step.groups {
            let Some(ids) = selection.selected.get(&group.id) else {
                continue;
            };
            for plugin in &group.plugins {
                if !ids.contains(&plugin.id) {
                    continue;
                }
                push_refs(&plugin.files)?;
            }
        }
    }

    for pattern in &module.conditional_installs {
        if eval_dependency(&pattern.dependencies, &flags, None) {
            push_refs(&pattern.files)?;
        }
    }

    Ok(by_dest
        .into_iter()
        .map(|(_, slot)| FomodMappedFile {
            source: slot.source,
            dest_rel: slot.dest,
        })
        .collect())
}

/// One deploy destination. The map key is lowercase so collisions ignore case;
/// `dest` keeps the winning file's original casing.
struct MappedDest {
    source: PathBuf,
    dest: PathBuf,
    priority: i32,
}

fn expand_file_ref(
    content_root: &Path,
    r: &FomodFileRef,
    by_dest: &mut BTreeMap<String, MappedDest>,
) -> Result<()> {
    let Some(source) = resolve_fomod_source(content_root, &r.source) else {
        return Ok(());
    };
    if r.is_folder {
        if !source.is_dir() {
            // Missing optional folders are common in messy packs; skip quietly.
            return Ok(());
        }
        let dest_base = normalize_fomod_path(&r.destination);
        for entry in WalkDir::new(&source).into_iter().filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let rel = entry
                .path()
                .strip_prefix(&source)
                .map(|p| p.to_path_buf())
                .unwrap_or_else(|_| PathBuf::from(entry.file_name()));
            let rel = normalize_relative(&rel);
            // Skip nested fomod metadata if somehow included.
            if rel
                .components()
                .next()
                .is_some_and(|c| c.as_os_str().eq_ignore_ascii_case("fomod"))
            {
                continue;
            }
            let dest = if dest_base.as_os_str().is_empty() {
                rel
            } else {
                dest_base.join(rel)
            };
            insert_mapped(by_dest, entry.path().to_path_buf(), dest, r.priority);
        }
    } else {
        if !source.is_file() {
            return Ok(());
        }
        let dest = if r.destination.trim().is_empty() {
            PathBuf::from(
                Path::new(&r.source)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| r.source.clone()),
            )
        } else {
            let dest_path = normalize_fomod_path(&r.destination);
            // If destination looks like a directory (trailing slash or no extension
            // while source has one and dest basename differs), join basename.
            let dest_str = r.destination.replace('\\', "/");
            if dest_str.ends_with('/') || dest_str.ends_with('\\') {
                dest_path.join(source.file_name().unwrap_or_default())
            } else {
                dest_path
            }
        };
        insert_mapped(by_dest, source, dest, r.priority);
    }
    Ok(())
}

fn insert_mapped(
    by_dest: &mut BTreeMap<String, MappedDest>,
    source: PathBuf,
    dest: PathBuf,
    priority: i32,
) {
    let dest = normalize_relative(&dest);
    let key = dest
        .to_string_lossy()
        .replace('\\', "/")
        .to_ascii_lowercase();
    if key.is_empty() || key.starts_with("fomod/") || key == "fomod" {
        return;
    }
    match by_dest.get(&key) {
        Some(prev) if prev.priority > priority => {}
        _ => {
            by_dest.insert(
                key,
                MappedDest {
                    source,
                    dest,
                    priority,
                },
            );
        }
    }
}

fn normalize_fomod_path(raw: &str) -> PathBuf {
    let cleaned = raw.replace('\\', "/").trim_matches('/').to_string();
    normalize_relative(Path::new(&cleaned))
}

fn resolve_fomod_source(content_root: &Path, raw: &str) -> Option<PathBuf> {
    let relative = normalize_fomod_path(raw);
    if relative.as_os_str().is_empty() {
        return Some(content_root.to_path_buf());
    }
    let direct = content_root.join(&relative);
    if direct.exists() {
        return Some(direct);
    }
    resolve_path_case_insensitive(content_root, &relative)
}

/// Absolute path of a plugin thumbnail, or `None` when the file is not in the package.
///
/// ModuleConfig paths use Windows separators (`fomod\logo.png`). On Linux that is one
/// filename, so the webview requests a missing file and shows a broken-image icon.
fn resolve_fomod_image(content_root: &Path, rel: &str) -> Option<String> {
    let relative = normalize_fomod_path(rel);
    if relative.as_os_str().is_empty() {
        return None;
    }
    let direct = content_root.join(&relative);
    if direct.is_file() {
        return Some(direct.to_string_lossy().to_string());
    }
    let resolved = resolve_path_case_insensitive(content_root, &relative)?;
    resolved
        .is_file()
        .then(|| resolved.to_string_lossy().to_string())
}

/// Walk `relative` from `root`, using the on-disk spelling when only the case differs.
fn resolve_path_case_insensitive(root: &Path, relative: &Path) -> Option<PathBuf> {
    let mut current = root.to_path_buf();
    let mut matched = false;
    for component in relative.components() {
        let name = match component {
            std::path::Component::Normal(name) => name,
            std::path::Component::CurDir => continue,
            _ => return None,
        };
        matched = true;
        let exact = current.join(name);
        if exact.exists() {
            current = exact;
            continue;
        }
        let wanted = name.to_str()?;
        let mut found = None;
        for entry in fs::read_dir(&current).ok()?.flatten() {
            let file_name = entry.file_name();
            let Some(candidate) = file_name.to_str() else {
                continue;
            };
            if !candidate.eq_ignore_ascii_case(wanted) {
                continue;
            }
            if found.is_some() {
                return None;
            }
            found = Some(entry.path());
        }
        current = found?;
    }
    matched.then_some(current)
}

// --- XML parsing -----------------------------------------------------------

pub fn parse_module_config(path: &Path) -> Result<FomodModule> {
    let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
    parse_module_config_bytes(&bytes)
}

pub fn parse_module_config_bytes(bytes: &[u8]) -> Result<FomodModule> {
    let text = decode_xml_bytes(bytes)?;
    let mut reader = quick_xml::Reader::from_str(&text);
    reader.config_mut().trim_text(true);
    let mut buf = Vec::new();

    let mut module_name = String::new();
    let mut required = Vec::new();
    let mut steps = Vec::new();
    let mut conditional_installs = Vec::new();
    let mut unsupported_scripted = false;
    let mut step_i = 0usize;

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(quick_xml::events::Event::Start(e)) => {
                let tag = local_name(&e.name());
                match tag.as_str() {
                    "modulename" => {
                        module_name = read_text(&mut reader, &mut buf)?;
                    }
                    "requiredinstallfiles" => {
                        required =
                            read_file_refs_until(&mut reader, &mut buf, "requiredinstallfiles")?;
                    }
                    "installsteps" => {
                        steps = read_install_steps(&mut reader, &mut buf, &mut step_i)?;
                    }
                    "conditionalfileinstalls" => {
                        conditional_installs = read_conditional_installs(&mut reader, &mut buf)?;
                    }
                    "moduleconfigname" | "scriptedinstaller" | "moduleimage" => {
                        if tag == "scriptedinstaller" {
                            unsupported_scripted = true;
                        }
                        skip_tag(&mut reader, &mut buf, &tag)?;
                    }
                    "config" | "moduledependencies" => {
                        if tag == "moduledependencies" {
                            skip_tag(&mut reader, &mut buf, &tag)?;
                        }
                    }
                    _ => {}
                }
            }
            Ok(quick_xml::events::Event::Empty(e)) => {
                let tag = local_name(&e.name());
                if tag == "scriptedinstaller" {
                    unsupported_scripted = true;
                }
            }
            Ok(quick_xml::events::Event::Eof) => break,
            Err(err) => bail!("FOMOD XML error: {err}"),
            _ => {}
        }
        buf.clear();
    }

    if module_name.is_empty() {
        module_name = "Mod".into();
    }

    Ok(FomodModule {
        module_name,
        required,
        steps,
        conditional_installs,
        unsupported_scripted,
    })
}

fn decode_xml_bytes(bytes: &[u8]) -> Result<String> {
    if bytes.starts_with(&[0xFF, 0xFE]) || bytes.starts_with(&[0xFE, 0xFF]) {
        let (cow, _, _) = encoding_rs::UTF_16LE.decode(bytes);
        // BOM sniffing: encoding_rs handles both via UTF-16LE decode with BOM.
        // Prefer UTF_16BE when BE BOM present.
        if bytes.starts_with(&[0xFE, 0xFF]) {
            let (cow, _, _) = encoding_rs::UTF_16BE.decode(bytes);
            return Ok(cow.into_owned());
        }
        return Ok(cow.into_owned());
    }
    if bytes.len() >= 2 && bytes[1] == 0 && bytes[0] != 0 {
        // Likely UTF-16LE without BOM (common for ModuleConfig.xml).
        let (cow, _, _) = encoding_rs::UTF_16LE.decode(bytes);
        return Ok(cow.into_owned());
    }
    Ok(String::from_utf8_lossy(bytes).into_owned())
}

fn local_name(name: &quick_xml::name::QName<'_>) -> String {
    name.local_name().as_ref().to_ascii_lowercase()
}

fn attr_map(e: &quick_xml::events::BytesStart<'_>) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for attr in e.attributes().flatten() {
        let key = attr.key.local_name().as_ref().to_ascii_lowercase();
        // Values are already UTF-8; common FOMOD attrs rarely need entity expansion.
        map.insert(key, attr.value.as_ref().to_string());
    }
    map
}

fn read_text(reader: &mut quick_xml::Reader<&[u8]>, buf: &mut Vec<u8>) -> Result<String> {
    let mut out = String::new();
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Text(t) => {
                out.push_str(t.as_ref());
            }
            quick_xml::events::Event::CData(t) => {
                out.push_str(t.as_ref());
            }
            quick_xml::events::Event::End(_) | quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(out)
}

fn skip_tag(reader: &mut quick_xml::Reader<&[u8]>, buf: &mut Vec<u8>, tag: &str) -> Result<()> {
    let mut depth = 1i32;
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                if local_name(&e.name()) == tag {
                    depth += 1;
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == tag {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(())
}

fn read_file_refs_until(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
    end_tag: &str,
) -> Result<Vec<FomodFileRef>> {
    let mut files = Vec::new();
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                if tag == "file" || tag == "folder" {
                    let attrs = attr_map(&e);
                    push_file_ref(&mut files, &tag, &attrs);
                    skip_tag(reader, buf, &tag)?;
                } else if tag != end_tag {
                    skip_tag(reader, buf, &tag)?;
                }
            }
            quick_xml::events::Event::Empty(e) => {
                let tag = local_name(&e.name());
                if tag == "file" || tag == "folder" {
                    let attrs = attr_map(&e);
                    push_file_ref(&mut files, &tag, &attrs);
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == end_tag {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(files)
}

fn push_file_ref(files: &mut Vec<FomodFileRef>, tag: &str, attrs: &HashMap<String, String>) {
    let source = attrs.get("source").cloned().unwrap_or_default();
    if source.is_empty() {
        return;
    }
    let destination = attrs.get("destination").cloned().unwrap_or_default();
    let priority = attrs
        .get("priority")
        .and_then(|p| p.parse().ok())
        .unwrap_or(0);
    files.push(FomodFileRef {
        source,
        destination,
        priority,
        is_folder: tag == "folder",
    });
}

fn read_install_steps(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
    step_i: &mut usize,
) -> Result<Vec<FomodStep>> {
    let mut steps = Vec::new();
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                if tag == "installstep" {
                    let attrs = attr_map(&e);
                    let name = attrs
                        .get("name")
                        .cloned()
                        .unwrap_or_else(|| format!("Step {}", *step_i + 1));
                    let id = option_id(*step_i, &name);
                    *step_i += 1;
                    let step = read_install_step(reader, buf, id, name)?;
                    steps.push(step);
                } else {
                    skip_tag(reader, buf, &tag)?;
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == "installsteps" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(steps)
}

fn read_install_step(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
    id: String,
    name: String,
) -> Result<FomodStep> {
    let mut visible = None;
    let mut groups = Vec::new();
    let mut group_i = 0usize;
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                match tag.as_str() {
                    "visible" => {
                        visible = Some(read_dependency_block(reader, buf, "visible")?);
                    }
                    "optionalfilegroups" => {
                        // continue reading groups inside
                    }
                    "group" => {
                        let attrs = attr_map(&e);
                        let gname = attrs
                            .get("name")
                            .cloned()
                            .unwrap_or_else(|| format!("Group {}", group_i + 1));
                        let type_ = FomodGroupType::parse(
                            attrs.get("type").map(|s| s.as_str()).unwrap_or("SelectAny"),
                        );
                        let gid = format!("{id}.{}", option_id(group_i, &gname));
                        group_i += 1;
                        let group = read_group(reader, buf, gid, gname, type_)?;
                        groups.push(group);
                    }
                    _ => skip_tag(reader, buf, &tag)?,
                }
            }
            quick_xml::events::Event::End(e) => {
                let tag = local_name(&e.name());
                if tag == "installstep" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(FomodStep {
        id,
        name,
        visible,
        groups,
    })
}

fn read_group(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
    id: String,
    name: String,
    type_: FomodGroupType,
) -> Result<FomodGroup> {
    let mut plugins = Vec::new();
    let mut plugin_i = 0usize;
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                match tag.as_str() {
                    "plugins" => {}
                    "plugin" => {
                        let attrs = attr_map(&e);
                        let pname = attrs
                            .get("name")
                            .cloned()
                            .unwrap_or_else(|| format!("Option {}", plugin_i + 1));
                        let pid = format!("{id}.{}", option_id(plugin_i, &pname));
                        plugin_i += 1;
                        plugins.push(read_plugin(reader, buf, pid, pname)?);
                    }
                    _ => skip_tag(reader, buf, &tag)?,
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == "group" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(FomodGroup {
        id,
        name,
        type_,
        plugins,
    })
}

fn read_plugin(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
    id: String,
    name: String,
) -> Result<FomodPlugin> {
    let mut description = None;
    let mut image = None;
    let mut files = Vec::new();
    let mut flags = Vec::new();
    let mut plugin_type = FomodPluginType::Optional;
    let mut conditional_types = Vec::new();

    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                match tag.as_str() {
                    "description" => {
                        description = Some(read_text(reader, buf)?);
                    }
                    "image" => {
                        let attrs = attr_map(&e);
                        image = attrs.get("path").cloned();
                        skip_tag(reader, buf, "image")?;
                    }
                    "files" => {
                        files = read_file_refs_until(reader, buf, "files")?;
                    }
                    "conditionflags" => {
                        flags = read_condition_flags(reader, buf)?;
                    }
                    "typedescriptor" => {
                        let (default_type, conditionals) = read_type_descriptor(reader, buf)?;
                        plugin_type = default_type;
                        conditional_types = conditionals;
                    }
                    _ => skip_tag(reader, buf, &tag)?,
                }
            }
            quick_xml::events::Event::Empty(e) => {
                let tag = local_name(&e.name());
                if tag == "image" {
                    let attrs = attr_map(&e);
                    image = attrs.get("path").cloned();
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == "plugin" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(FomodPlugin {
        id,
        name,
        description,
        image,
        files,
        flags,
        plugin_type,
        conditional_types,
    })
}

fn read_condition_flags(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Vec<ConditionFlag>> {
    let mut flags = Vec::new();
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                if tag == "flag" {
                    let attrs = attr_map(&e);
                    let name = attrs.get("name").cloned().unwrap_or_default();
                    let value = read_text(reader, buf)?;
                    if !name.is_empty() {
                        flags.push(ConditionFlag { name, value });
                    }
                } else {
                    skip_tag(reader, buf, &tag)?;
                }
            }
            quick_xml::events::Event::Empty(e) => {
                let tag = local_name(&e.name());
                if tag == "flag" {
                    let attrs = attr_map(&e);
                    let name = attrs.get("name").cloned().unwrap_or_default();
                    if !name.is_empty() {
                        flags.push(ConditionFlag {
                            name,
                            value: String::new(),
                        });
                    }
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == "conditionflags" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(flags)
}

fn read_type_descriptor(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<(FomodPluginType, Vec<(Dependency, FomodPluginType)>)> {
    let mut default_type = FomodPluginType::Optional;
    let mut conditionals = Vec::new();
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                match tag.as_str() {
                    "type" => {
                        let attrs = attr_map(&e);
                        default_type = FomodPluginType::parse(
                            attrs.get("name").map(|s| s.as_str()).unwrap_or("Optional"),
                        );
                        skip_tag(reader, buf, "type")?;
                    }
                    "dependencytype" => {
                        let (def, pats) = read_dependency_type(reader, buf)?;
                        default_type = def;
                        conditionals = pats;
                    }
                    _ => skip_tag(reader, buf, &tag)?,
                }
            }
            quick_xml::events::Event::Empty(e) => {
                let tag = local_name(&e.name());
                if tag == "type" {
                    let attrs = attr_map(&e);
                    default_type = FomodPluginType::parse(
                        attrs.get("name").map(|s| s.as_str()).unwrap_or("Optional"),
                    );
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == "typedescriptor" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok((default_type, conditionals))
}

fn read_dependency_type(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<(FomodPluginType, Vec<(Dependency, FomodPluginType)>)> {
    let mut default_type = FomodPluginType::Optional;
    let mut patterns = Vec::new();
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                match tag.as_str() {
                    "defaulttype" => {
                        // may contain <type name="..."/>
                        default_type = read_nested_type(reader, buf, "defaulttype")?;
                    }
                    "patterns" => {}
                    "pattern" => {
                        if let Some(p) = read_type_pattern(reader, buf)? {
                            patterns.push(p);
                        }
                    }
                    _ => skip_tag(reader, buf, &tag)?,
                }
            }
            quick_xml::events::Event::Empty(e) => {
                let tag = local_name(&e.name());
                if tag == "defaulttype" || tag == "type" {
                    let attrs = attr_map(&e);
                    if let Some(n) = attrs.get("name") {
                        default_type = FomodPluginType::parse(n);
                    }
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == "dependencytype" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok((default_type, patterns))
}

fn read_nested_type(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
    end_tag: &str,
) -> Result<FomodPluginType> {
    let mut t = FomodPluginType::Optional;
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) | quick_xml::events::Event::Empty(e) => {
                if local_name(&e.name()) == "type" {
                    let attrs = attr_map(&e);
                    t = FomodPluginType::parse(
                        attrs.get("name").map(|s| s.as_str()).unwrap_or("Optional"),
                    );
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == end_tag {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(t)
}

fn read_type_pattern(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Option<(Dependency, FomodPluginType)>> {
    let mut deps = None;
    let mut plugin_type = FomodPluginType::Optional;
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                let attrs = attr_map(&e);
                match tag.as_str() {
                    "dependencies" => {
                        deps = Some(read_dependency_with_attrs(reader, buf, &attrs)?);
                    }
                    "type" => {
                        plugin_type = FomodPluginType::parse(
                            attrs.get("name").map(|s| s.as_str()).unwrap_or("Optional"),
                        );
                        skip_tag(reader, buf, "type")?;
                    }
                    _ => skip_tag(reader, buf, &tag)?,
                }
            }
            quick_xml::events::Event::Empty(e) => {
                if local_name(&e.name()) == "type" {
                    let attrs = attr_map(&e);
                    plugin_type = FomodPluginType::parse(
                        attrs.get("name").map(|s| s.as_str()).unwrap_or("Optional"),
                    );
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == "pattern" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(deps.map(|d| (d, plugin_type)))
}

fn read_conditional_installs(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Vec<ConditionalPattern>> {
    let mut patterns = Vec::new();
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                match tag.as_str() {
                    "patterns" => {}
                    "pattern" => {
                        if let Some(p) = read_install_pattern(reader, buf)? {
                            patterns.push(p);
                        }
                    }
                    _ => skip_tag(reader, buf, &tag)?,
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == "conditionalfileinstalls" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(patterns)
}

fn read_install_pattern(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
) -> Result<Option<ConditionalPattern>> {
    let mut dependencies = None;
    let mut files = Vec::new();
    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                let attrs = attr_map(&e);
                match tag.as_str() {
                    "dependencies" => {
                        dependencies = Some(read_dependency_with_attrs(reader, buf, &attrs)?);
                    }
                    "files" => {
                        files = read_file_refs_until(reader, buf, "files")?;
                    }
                    _ => skip_tag(reader, buf, &tag)?,
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == "pattern" {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }
    Ok(dependencies.map(|dependencies| ConditionalPattern {
        dependencies,
        files,
    }))
}

fn read_dependency_block(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
    end_tag: &str,
) -> Result<Dependency> {
    // The opening tag may carry operator="And|Or". Default And.
    // We already consumed the Start of end_tag's outer in callers for "visible"
    // and "dependencies". For visible, children are the deps. Operator defaults to And.
    let mut children = Vec::new();

    // Operator on the outer visible/dependencies tag was already consumed;
    // default to And. Nested dependencies carry their own operator via
    // read_dependency_with_attrs.

    loop {
        match reader.read_event_into(buf)? {
            quick_xml::events::Event::Start(e) => {
                let tag = local_name(&e.name());
                let attrs = attr_map(&e);
                match tag.as_str() {
                    "dependencies" => {
                        let nested = read_dependency_with_attrs(reader, buf, &attrs)?;
                        children.push(nested);
                    }
                    "flagdependency" => {
                        children.push(Dependency::Flag {
                            name: attrs.get("flag").cloned().unwrap_or_default(),
                            value: attrs.get("value").cloned().unwrap_or_default(),
                        });
                        skip_tag(reader, buf, &tag)?;
                    }
                    "filedependency" => {
                        children.push(Dependency::File {
                            file: attrs.get("file").cloned().unwrap_or_default(),
                            state: attrs
                                .get("state")
                                .cloned()
                                .unwrap_or_else(|| "Active".into()),
                        });
                        skip_tag(reader, buf, &tag)?;
                    }
                    "gamedependency" | "foseDependency" | "fosedependency"
                    | "versiondependency" => {
                        children.push(Dependency::GameVersion);
                        skip_tag(reader, buf, &tag)?;
                    }
                    _ => {
                        children.push(Dependency::Unknown);
                        skip_tag(reader, buf, &tag)?;
                    }
                }
            }
            quick_xml::events::Event::Empty(e) => {
                let tag = local_name(&e.name());
                let attrs = attr_map(&e);
                match tag.as_str() {
                    "flagdependency" => {
                        children.push(Dependency::Flag {
                            name: attrs.get("flag").cloned().unwrap_or_default(),
                            value: attrs.get("value").cloned().unwrap_or_default(),
                        });
                    }
                    "filedependency" => {
                        children.push(Dependency::File {
                            file: attrs.get("file").cloned().unwrap_or_default(),
                            state: attrs
                                .get("state")
                                .cloned()
                                .unwrap_or_else(|| "Active".into()),
                        });
                    }
                    "gamedependency" | "fosedependency" | "versiondependency" => {
                        children.push(Dependency::GameVersion);
                    }
                    _ => children.push(Dependency::Unknown),
                }
            }
            quick_xml::events::Event::End(e) => {
                if local_name(&e.name()) == end_tag {
                    break;
                }
            }
            quick_xml::events::Event::Eof => break,
            _ => {}
        }
        buf.clear();
    }

    Ok(Dependency::And(children))
}

fn read_dependency_with_attrs(
    reader: &mut quick_xml::Reader<&[u8]>,
    buf: &mut Vec<u8>,
    attrs: &HashMap<String, String>,
) -> Result<Dependency> {
    let op = attrs
        .get("operator")
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_else(|| "and".into());
    let inner = read_dependency_block(reader, buf, "dependencies")?;
    let children = match inner {
        Dependency::And(c) => c,
        other => vec![other],
    };
    if op == "or" {
        Ok(Dependency::Or(children))
    } else {
        Ok(Dependency::And(children))
    }
}

/// Validate that a selection satisfies group constraints for visible steps.
pub fn selection_is_valid(module: &FomodModule, selection: &FomodSelection) -> Result<()> {
    let flags = collect_flags(module, selection);
    for step in &module.steps {
        let visible = step
            .visible
            .as_ref()
            .map(|d| eval_dependency(d, &flags, None))
            .unwrap_or(true);
        if !visible {
            continue;
        }
        for group in &step.groups {
            let ids = selection
                .selected
                .get(&group.id)
                .cloned()
                .unwrap_or_default();
            let usable: Vec<_> = group
                .plugins
                .iter()
                .filter(|p| !resolve_plugin_type(p, &flags).is_disabled())
                .collect();
            match group.type_ {
                FomodGroupType::SelectExactlyOne => {
                    if ids.len() != 1 {
                        bail!("{}: select exactly one option", group.name);
                    }
                }
                FomodGroupType::SelectAtLeastOne => {
                    if ids.is_empty() && !usable.is_empty() {
                        bail!("{}: select at least one option", group.name);
                    }
                }
                FomodGroupType::SelectAtMostOne => {
                    if ids.len() > 1 {
                        bail!("{}: select at most one option", group.name);
                    }
                }
                FomodGroupType::SelectAll | FomodGroupType::SelectAny => {}
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const IMAGE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Images</moduleName>
  <installSteps order="Explicit">
    <installStep name="Main">
      <optionalFileGroups>
        <group name="Main File" type="SelectAny">
          <plugins>
            <plugin name="With image">
              <image path="fomod\logo.png" />
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
            <plugin name="Missing image">
              <image path="fomod\missing.png" />
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;

    const SIMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<config xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
    xsi:noNamespaceSchemaLocation="http://qconsulting.ca/fo3/ModConfig5.0.xsd">
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
              <description>English voices</description>
              <files>
                <folder source="01 English" destination=""/>
              </files>
              <typeDescriptor>
                <type name="Recommended"/>
              </typeDescriptor>
            </plugin>
            <plugin name="Russian">
              <description>Russian voices</description>
              <files>
                <folder source="02 Russian" destination=""/>
              </files>
              <typeDescriptor>
                <type name="Optional"/>
              </typeDescriptor>
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
              <files>
                <folder source="10 All Voices" destination=""/>
              </files>
              <typeDescriptor>
                <type name="Recommended"/>
              </typeDescriptor>
            </plugin>
            <plugin name="Jackie only">
              <files>
                <folder source="11 Jackie Only" destination=""/>
              </files>
              <typeDescriptor>
                <type name="Optional"/>
              </typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;

    const FLAGS_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Flags Demo</moduleName>
  <installSteps order="Explicit">
    <installStep name="Choose">
      <optionalFileGroups>
        <group name="Opt" type="SelectExactlyOne">
          <plugins>
            <plugin name="A">
              <conditionFlags><flag name="opt_a">selected</flag></conditionFlags>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="B">
              <conditionFlags><flag name="opt_b">selected</flag></conditionFlags>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
    <installStep name="Texture A">
      <visible><flagDependency flag="opt_a" value="selected"/></visible>
      <optionalFileGroups>
        <group name="Tex" type="SelectExactlyOne">
          <plugins>
            <plugin name="Blue">
              <files><folder source="blue_a" destination=""/></files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
  <conditionalFileInstalls>
    <patterns>
      <pattern>
        <dependencies operator="And">
          <flagDependency flag="opt_a" value="selected"/>
        </dependencies>
        <files>
          <folder source="option_a" destination=""/>
        </files>
      </pattern>
    </patterns>
  </conditionalFileInstalls>
</config>"#;

    #[test]
    fn parses_true_cyberpsychosis_shaped_config() {
        let module = parse_module_config_bytes(SIMPLE_XML.as_bytes()).unwrap();
        assert_eq!(module.module_name, "True Cyberpsychosis");
        assert_eq!(module.required.len(), 1);
        assert_eq!(module.steps.len(), 2);
        assert_eq!(
            module.steps[0].groups[0].type_,
            FomodGroupType::SelectExactlyOne
        );
        assert_eq!(module.steps[0].groups[0].plugins.len(), 2);
        assert_eq!(
            module.steps[0].groups[0].plugins[0].plugin_type,
            FomodPluginType::Recommended
        );
    }

    #[test]
    fn default_selection_picks_recommended() {
        let module = parse_module_config_bytes(SIMPLE_XML.as_bytes()).unwrap();
        let sel = default_selection(&module);
        let lang = &module.steps[0].groups[0];
        let voices = &module.steps[1].groups[0];
        assert_eq!(
            sel.selected.get(&lang.id).unwrap(),
            &vec![lang.plugins[0].id.clone()]
        );
        assert_eq!(
            sel.selected.get(&voices.id).unwrap(),
            &vec![voices.plugins[0].id.clone()]
        );
    }

    #[test]
    fn file_map_includes_required_and_selected_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        for dir in [
            "00 Core",
            "01 English",
            "02 Russian",
            "10 All Voices",
            "11 Jackie Only",
        ] {
            let p = root.join(dir).join("r6").join("scripts");
            fs::create_dir_all(&p).unwrap();
            fs::write(p.join("x.reds"), b"x").unwrap();
        }
        fs::create_dir_all(root.join("fomod")).unwrap();
        fs::write(root.join("fomod").join("ModuleConfig.xml"), SIMPLE_XML).unwrap();

        let module = load_module(root).unwrap().unwrap();
        let sel = default_selection(&module);
        let map = build_file_map(root, &module, &sel).unwrap();
        let dests: HashSet<_> = map
            .iter()
            .map(|m| m.dest_rel.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(dests.contains("r6/scripts/x.reds") || dests.iter().any(|d| d.ends_with("x.reds")));
        // English + All voices + core all write the same relative path; priority ties keep one.
        assert!(!map.is_empty());
        // Russian must not contribute a unique path when English is selected — same dest.
        assert_eq!(map.len(), 1);
    }

    #[test]
    fn priority_higher_wins() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("a")).unwrap();
        fs::create_dir_all(root.join("b")).unwrap();
        fs::write(root.join("a").join("file.txt"), b"a").unwrap();
        fs::write(root.join("b").join("file.txt"), b"b").unwrap();

        let mut by_dest = BTreeMap::new();
        expand_file_ref(
            root,
            &FomodFileRef {
                source: "a".into(),
                destination: "".into(),
                priority: 1,
                is_folder: true,
            },
            &mut by_dest,
        )
        .unwrap();
        expand_file_ref(
            root,
            &FomodFileRef {
                source: "b".into(),
                destination: "".into(),
                priority: 5,
                is_folder: true,
            },
            &mut by_dest,
        )
        .unwrap();
        let slot = by_dest.get("file.txt").unwrap();
        assert_eq!(slot.priority, 5);
        assert_eq!(fs::read(&slot.source).unwrap(), b"b");
        assert_eq!(slot.dest, PathBuf::from("file.txt"));
    }

    #[test]
    fn file_map_preserves_destination_case() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let voice = root
            .join("red4ext")
            .join("plugins")
            .join("AudioXL")
            .join("sounds")
            .join("TrueCyberpsychosis");
        fs::create_dir_all(&voice).unwrap();
        fs::write(voice.join("tcp_heartbeat.ogg"), b"beat").unwrap();
        // Same relative path, different case, lower priority — must not win.
        let lower = root
            .join("other")
            .join("plugins")
            .join("audioxl")
            .join("sounds")
            .join("truecyberpsychosis");
        fs::create_dir_all(&lower).unwrap();
        fs::write(lower.join("tcp_heartbeat.ogg"), b"nope").unwrap();

        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Case</moduleName>
  <requiredInstallFiles>
    <folder source="red4ext" destination="red4ext" priority="1"/>
  </requiredInstallFiles>
  <installSteps order="Explicit">
    <installStep name="Extra">
      <optionalFileGroups>
        <group name="Opt" type="SelectExactlyOne">
          <plugins>
            <plugin name="Lower">
              <files>
                <folder source="other" destination="red4ext" priority="0"/>
              </files>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;
        fs::create_dir_all(root.join("fomod")).unwrap();
        fs::write(root.join("fomod").join("ModuleConfig.xml"), xml).unwrap();

        let module = load_module(root).unwrap().unwrap();
        let sel = default_selection(&module);
        let map = build_file_map(root, &module, &sel).unwrap();
        let hit = map
            .iter()
            .find(|m| {
                m.dest_rel
                    .to_string_lossy()
                    .replace('\\', "/")
                    .to_ascii_lowercase()
                    .ends_with("tcp_heartbeat.ogg")
            })
            .expect("heartbeat mapped");
        assert_eq!(
            hit.dest_rel.to_string_lossy().replace('\\', "/"),
            "red4ext/plugins/AudioXL/sounds/TrueCyberpsychosis/tcp_heartbeat.ogg"
        );
        assert_eq!(fs::read(&hit.source).unwrap(), b"beat");
    }

    #[test]
    fn flag_visibility_and_conditional_installs() {
        let module = parse_module_config_bytes(FLAGS_XML.as_bytes()).unwrap();
        let sel = default_selection(&module);
        let flags = collect_flags(&module, &sel);
        assert_eq!(flags.get("opt_a").map(|s| s.as_str()), Some("selected"));

        let view_step = &module.steps[1];
        assert!(eval_dependency(
            view_step.visible.as_ref().unwrap(),
            &flags,
            None
        ));

        // Switch to B
        let group = &module.steps[0].groups[0];
        let mut sel_b = sel.clone();
        sel_b
            .selected
            .insert(group.id.clone(), vec![group.plugins[1].id.clone()]);
        let flags_b = collect_flags(&module, &sel_b);
        assert!(!eval_dependency(
            view_step.visible.as_ref().unwrap(),
            &flags_b,
            None
        ));
    }

    #[test]
    fn detects_module_config_case_insensitively() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("fomod")).unwrap();
        fs::write(
            tmp.path().join("fomod").join("moduleconfig.xml"),
            SIMPLE_XML,
        )
        .unwrap();
        assert!(has_fomod(tmp.path()));
    }

    #[test]
    fn installer_view_resolves_windows_image_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let logo = root.join("fomod").join("logo.png");
        fs::create_dir_all(logo.parent().unwrap()).unwrap();
        fs::write(&logo, b"png").unwrap();
        fs::write(root.join("fomod").join("ModuleConfig.xml"), IMAGE_XML).unwrap();

        let view = installer_view(root, None).unwrap().unwrap();
        let plugins = &view.steps[0].groups[0].plugins;
        assert_eq!(PathBuf::from(plugins[0].image.clone().unwrap()), logo);
        assert!(plugins[1].image.is_none());
    }

    #[test]
    fn installer_view_resolves_case_mismatched_image_paths() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        let logo = root.join("Fomod").join("Logo.PNG");
        fs::create_dir_all(logo.parent().unwrap()).unwrap();
        fs::write(&logo, b"png").unwrap();
        fs::write(logo.parent().unwrap().join("ModuleConfig.xml"), IMAGE_XML).unwrap();

        let view = installer_view(root, None).unwrap().unwrap();
        let image = PathBuf::from(
            view.steps[0].groups[0].plugins[0]
                .image
                .clone()
                .expect("thumbnail path"),
        );
        assert_eq!(
            fs::canonicalize(&image).unwrap(),
            fs::canonicalize(&logo).unwrap()
        );
        #[cfg(unix)]
        assert_eq!(image, logo);
    }

    #[test]
    fn utf16_le_module_config_parses() {
        let mut bytes = vec![0xFF, 0xFE];
        for unit in SIMPLE_XML.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        let module = parse_module_config_bytes(&bytes).unwrap();
        assert_eq!(module.module_name, "True Cyberpsychosis");
    }

    #[test]
    fn build_file_map_resolves_case_mismatched_sources() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("data").join("meshes")).unwrap();
        fs::write(root.join("data").join("meshes").join("hero.nif"), b"mesh").unwrap();
        fs::create_dir_all(root.join("extras")).unwrap();
        fs::write(root.join("extras").join("patch.esp"), b"esp").unwrap();
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Case Source</moduleName>
  <requiredInstallFiles>
    <folder source="Data\Meshes" destination="Meshes"/>
    <file source="EXTRAS\Patch.ESP" destination="Patch.esp"/>
  </requiredInstallFiles>
</config>"#;
        fs::create_dir_all(root.join("Fomod")).unwrap();
        fs::write(root.join("Fomod").join("ModuleConfig.xml"), xml).unwrap();

        let module = load_module(root).unwrap().unwrap();
        let sel = default_selection(&module);
        let map = build_file_map(root, &module, &sel).unwrap();
        let dests: HashSet<String> = map
            .iter()
            .map(|m| m.dest_rel.to_string_lossy().replace('\\', "/"))
            .collect();
        assert!(dests.contains("Meshes/hero.nif"), "dests: {dests:?}");
        assert!(dests.contains("Patch.esp"), "dests: {dests:?}");
    }

    #[test]
    fn collect_flags_skips_invisible_steps() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Branch Flags</moduleName>
  <installSteps order="Explicit">
    <installStep name="Branch">
      <optionalFileGroups>
        <group name="Pick" type="SelectExactlyOne">
          <plugins>
            <plugin name="A">
              <conditionFlags><flag name="branch">a</flag></conditionFlags>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
            <plugin name="B">
              <conditionFlags><flag name="branch">b</flag></conditionFlags>
              <typeDescriptor><type name="Optional"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
    <installStep name="Only B">
      <visible><flagDependency flag="branch" value="b"/></visible>
      <optionalFileGroups>
        <group name="Sub" type="SelectExactlyOne">
          <plugins>
            <plugin name="B Patch">
              <conditionFlags><flag name="install_b">yes</flag></conditionFlags>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
  <conditionalFileInstalls>
    <patterns>
      <pattern>
        <dependencies operator="And">
          <flagDependency flag="install_b" value="yes"/>
        </dependencies>
        <files>
          <folder source="b_files" destination=""/>
        </files>
      </pattern>
    </patterns>
  </conditionalFileInstalls>
</config>"#;
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join("b_files")).unwrap();
        fs::write(root.join("b_files").join("b.txt"), b"b").unwrap();
        fs::create_dir_all(root.join("fomod")).unwrap();
        fs::write(root.join("fomod").join("ModuleConfig.xml"), xml).unwrap();

        let module = load_module(root).unwrap().unwrap();
        // Default selects Branch A; hidden Step 2 must not leak `install_b=yes`.
        let sel = default_selection(&module);
        let flags = collect_flags(&module, &sel);
        assert_eq!(flags.get("branch").map(String::as_str), Some("a"));
        assert!(!flags.contains_key("install_b"));
        let map = build_file_map(root, &module, &sel).unwrap();
        assert!(map.is_empty(), "hidden branch files must not deploy");

        // Switching to Branch B makes Step 2 visible and deploys b_files.
        let group = &module.steps[0].groups[0];
        let mut sel_b = sel;
        sel_b
            .selected
            .insert(group.id.clone(), vec![group.plugins[1].id.clone()]);
        let map_b = build_file_map(root, &module, &sel_b).unwrap();
        assert_eq!(map_b.len(), 1);
    }

    #[test]
    fn normalize_selection_propagates_step_flags_and_preserves_unchecked_select_any() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<config>
  <moduleName>Propagate</moduleName>
  <installSteps order="Explicit">
    <installStep name="Step 1">
      <optionalFileGroups>
        <group name="Base" type="SelectExactlyOne">
          <plugins>
            <plugin name="Core">
              <conditionFlags><flag name="core">on</flag></conditionFlags>
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
    <installStep name="Step 2">
      <optionalFileGroups>
        <group name="Extras" type="SelectAny">
          <plugins>
            <plugin name="Auto">
              <typeDescriptor>
                <dependencyType>
                  <defaultType name="Optional"/>
                  <patterns>
                    <pattern>
                      <dependencies operator="And">
                        <flagDependency flag="core" value="on"/>
                      </dependencies>
                      <type name="Required"/>
                    </pattern>
                  </patterns>
                </dependencyType>
              </typeDescriptor>
            </plugin>
            <plugin name="Optional Rec">
              <typeDescriptor><type name="Recommended"/></typeDescriptor>
            </plugin>
          </plugins>
        </group>
      </optionalFileGroups>
    </installStep>
  </installSteps>
</config>"#;
        let module = parse_module_config_bytes(xml.as_bytes()).unwrap();
        let g1 = &module.steps[0].groups[0];
        let g2 = &module.steps[1].groups[0];

        // Pass a stale Step 1 id + explicitly empty Step 2 selection.
        let mut raw = FomodSelection::default();
        raw.selected.insert(g1.id.clone(), vec!["stale-id".into()]);
        raw.selected.insert(g2.id.clone(), vec![]);

        let normalized = normalize_selection(&module, &raw);
        // Step 1 falls back to Core, setting core=on, which forces Auto (Required) on in Step 2,
        // while keeping Optional Rec unchecked because the user explicitly emptied SelectAny.
        assert_eq!(
            normalized.selected.get(&g1.id).unwrap(),
            &vec![g1.plugins[0].id.clone()]
        );
        assert_eq!(
            normalized.selected.get(&g2.id).unwrap(),
            &vec![g2.plugins[0].id.clone()]
        );
    }

    #[test]
    fn detects_scripted_installer_in_mixed_case_fomod_dir() {
        let tmp = tempfile::tempdir().unwrap();
        fs::create_dir_all(tmp.path().join("Fomod")).unwrap();
        fs::write(tmp.path().join("Fomod").join("script.cs"), b"class Script {}").unwrap();
        assert!(detect_scripted_installer(tmp.path()));
    }
}
