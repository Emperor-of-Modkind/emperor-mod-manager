//! Pre-flight existing-mod resolution and version conflict planning for collections.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::{
    check_staged_mod_integrity, find_all_modio_mods, find_all_nexus_mod,
    find_all_thunderstore_packages, find_modio_file, find_nexus_file,
    find_thunderstore_package_version, InstalledCollections, LoadOrder, ModSource, StagedMod,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "lowercase")]
pub enum IncomingModSpec {
    Nexus {
        domain: String,
        mod_id: u64,
        file_id: u64,
        name: String,
        version: Option<String>,
        file_name: Option<String>,
    },
    Thunderstore {
        community: String,
        namespace: String,
        name: String,
        version: Option<String>,
        display_name: String,
    },
    Modio {
        modio_game_id: u32,
        modio_mod_id: u64,
        file_id: Option<u64>,
        name: String,
        version: Option<String>,
    },
}

impl IncomingModSpec {
    pub fn conflict_key(&self) -> String {
        match self {
            IncomingModSpec::Nexus {
                domain,
                mod_id,
                file_id,
                ..
            } => format!("nexus:{}:{mod_id}:{file_id}", domain.to_lowercase()),
            IncomingModSpec::Thunderstore {
                namespace,
                name,
                version,
                ..
            } => format!(
                "ts:{}:{}:{}",
                namespace.to_lowercase(),
                name.to_lowercase(),
                version.as_deref().unwrap_or("").trim().to_lowercase()
            ),
            IncomingModSpec::Modio {
                modio_game_id,
                modio_mod_id,
                file_id,
                version,
                ..
            } => format!(
                "modio:{modio_game_id}:{modio_mod_id}:{}:{}",
                file_id.unwrap_or(0),
                version.as_deref().unwrap_or("").trim().to_lowercase()
            ),
        }
    }

    pub fn mod_source(&self) -> ModSource {
        match self {
            IncomingModSpec::Nexus { .. } => ModSource::Nexus,
            IncomingModSpec::Thunderstore { .. } => ModSource::Thunderstore,
            IncomingModSpec::Modio { .. } => ModSource::Modio,
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            IncomingModSpec::Nexus { name, .. } => name,
            IncomingModSpec::Thunderstore { display_name, .. } => display_name,
            IncomingModSpec::Modio { name, .. } => name,
        }
    }

    pub fn version(&self) -> Option<&str> {
        match self {
            IncomingModSpec::Nexus { version, .. }
            | IncomingModSpec::Thunderstore { version, .. }
            | IncomingModSpec::Modio { version, .. } => version.as_deref(),
        }
    }

    pub fn file_id(&self) -> Option<u64> {
        match self {
            IncomingModSpec::Nexus { file_id, .. } => Some(*file_id),
            IncomingModSpec::Thunderstore { .. } => None,
            IncomingModSpec::Modio { file_id, .. } => *file_id,
        }
    }

    pub fn file_name(&self) -> Option<&str> {
        match self {
            IncomingModSpec::Nexus { file_name, .. } => file_name.as_deref(),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConflictAction {
    /// Disable the conflicting collection(s) and install the incoming version alongside.
    DisableOtherCollections,
    /// Disable only the conflicting staged mod(s) and install the incoming version alongside.
    DisableOldAndInstallNew,
    /// Replace the existing staged mod in-place with the incoming version.
    ReplaceExisting,
    /// Keep the existing staged mod and skip downloading the incoming version.
    KeepExisting,
    /// Nexus only: install the incoming file alongside the existing file with both enabled.
    KeepBothEnabled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictingCollectionRef {
    pub id: String,
    pub name: String,
    pub is_synced: bool,
    pub enabled_mod_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictingCollectionSummary {
    pub id: String,
    pub name: String,
    pub is_synced: bool,
    pub enabled_mod_count: usize,
    pub conflict_count: usize,
    pub conflict_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModVersionConflict {
    pub conflict_key: String,
    pub source: ModSource,
    pub incoming_name: String,
    pub incoming_version: Option<String>,
    pub incoming_file_id: Option<u64>,
    pub incoming_file_name: Option<String>,
    pub existing_mod_id: String,
    pub existing_name: String,
    pub existing_version: Option<String>,
    pub existing_file_id: Option<u64>,
    pub existing_file_name: Option<String>,
    pub existing_enabled: bool,
    pub existing_independent: bool,
    pub conflicting_collections: Vec<ConflictingCollectionRef>,
    pub recommended_action: ConflictAction,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CollectionConflictPlan {
    pub target_collection_id: Option<String>,
    pub target_collection_name: String,
    pub is_synced_target: bool,
    pub exact_match_count: usize,
    pub repair_count: usize,
    pub install_count: usize,
    pub auto_update_count: usize,
    pub conflicts: Vec<ModVersionConflict>,
    pub conflicting_collections: Vec<ConflictingCollectionSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct CollectionResolutionChoices {
    #[serde(default)]
    pub disable_collection_ids: Vec<String>,
    #[serde(default)]
    pub mod_actions: HashMap<String, ConflictAction>,
}

/// Concrete per-mod action computed during execution from the current load order + user choices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModExecutionPlan {
    /// Exact match with intact staging; reuse it and optionally disable conflicting rows.
    ReuseExact {
        staged_id: String,
        disable_other_ids: Vec<String>,
    },
    /// Exact match with damaged/missing staging; re-download and repair in place.
    RepairExact {
        staged_id: String,
        disable_other_ids: Vec<String>,
    },
    /// Install fresh without replacing any existing row.
    InstallFresh,
    /// Replace a specific existing staged row in-place.
    ReplaceStaged { replace_id: String },
    /// Disable conflicting staged row(s) and stage the incoming version alongside.
    DisableOldAndInstallNew { disable_ids: Vec<String> },
    /// Keep an existing different-version staged row and skip downloading the incoming version.
    KeepExisting { staged_id: String },
    /// Nexus companion/addon file: install alongside without disabling the existing row.
    KeepBothEnabled,
}

fn find_exact_match<'a>(order: &'a LoadOrder, spec: &IncomingModSpec) -> Option<&'a StagedMod> {
    match spec {
        IncomingModSpec::Nexus {
            mod_id, file_id, ..
        } => find_nexus_file(order, *mod_id, *file_id),
        IncomingModSpec::Thunderstore {
            namespace,
            name,
            version,
            ..
        } => find_thunderstore_package_version(order, namespace, name, version.as_deref()),
        IncomingModSpec::Modio {
            modio_game_id,
            modio_mod_id,
            file_id,
            version,
            ..
        } => find_modio_file(
            order,
            *modio_game_id,
            *modio_mod_id,
            *file_id,
            version.as_deref(),
        ),
    }
}

fn find_same_identity_rows<'a>(order: &'a LoadOrder, spec: &IncomingModSpec) -> Vec<&'a StagedMod> {
    match spec {
        IncomingModSpec::Nexus { domain, mod_id, .. } => find_all_nexus_mod(order, domain, *mod_id),
        IncomingModSpec::Thunderstore {
            namespace, name, ..
        } => find_all_thunderstore_packages(order, namespace, name),
        IncomingModSpec::Modio {
            modio_game_id,
            modio_mod_id,
            ..
        } => find_all_modio_mods(order, *modio_game_id, *modio_mod_id),
    }
}

fn is_exclusive_to_target(staged: &StagedMod, target_collection_id: Option<&str>) -> bool {
    let Some(target_id) = target_collection_id else {
        return false;
    };
    !staged.independent
        && !staged.collection_ids.is_empty()
        && staged.collection_ids.iter().all(|id| id == target_id)
}

fn other_collections_for_mod(
    staged: &StagedMod,
    order: &LoadOrder,
    collections: &InstalledCollections,
    target_collection_id: Option<&str>,
) -> Vec<ConflictingCollectionRef> {
    let enabled_ids: HashSet<&str> = order
        .mods
        .iter()
        .filter(|m| m.enabled)
        .map(|m| m.id.as_str())
        .collect();
    let mut out = Vec::new();
    for col in &collections.collections {
        if target_collection_id.is_some_and(|tid| tid == col.id) {
            continue;
        }
        let has_mod = col.mod_ids.iter().any(|id| id == &staged.id)
            || staged.collection_ids.iter().any(|id| id == &col.id);
        if !has_mod {
            continue;
        }
        let enabled_mod_count = col
            .mod_ids
            .iter()
            .filter(|id| enabled_ids.contains(id.as_str()))
            .count();
        out.push(ConflictingCollectionRef {
            id: col.id.clone(),
            name: col.name.clone(),
            is_synced: col.friend_sync.is_some(),
            enabled_mod_count,
        });
    }
    out
}

fn build_incoming_nexus_files(incoming: &[IncomingModSpec]) -> HashSet<(String, u64, u64)> {
    incoming
        .iter()
        .filter_map(|spec| match spec {
            IncomingModSpec::Nexus {
                domain,
                mod_id,
                file_id,
                ..
            } => Some((domain.to_lowercase(), *mod_id, *file_id)),
            _ => None,
        })
        .collect()
}

fn filter_conflicting_rows<'a>(
    same_identity: Vec<&'a StagedMod>,
    exact_id: Option<&str>,
    incoming_nexus_files: &HashSet<(String, u64, u64)>,
) -> Vec<&'a StagedMod> {
    same_identity
        .into_iter()
        .filter(|m| {
            if exact_id.is_some_and(|eid| eid == m.id) {
                return false;
            }
            // If this staged Nexus file is itself another member of the incoming
            // collection (multi-file mod / companion patch), it is not a conflict.
            if m.source == ModSource::Nexus
                && incoming_nexus_files.contains(&(
                    m.domain.to_lowercase(),
                    m.nexus_mod_id,
                    m.nexus_file_id,
                ))
            {
                return false;
            }
            true
        })
        .collect()
}

/// Analyze all incoming mods for a collection against the currently staged mods and
/// installed collections, returning exact matches, repairs, fresh installs, automatic
/// self-updates, and any version conflicts that need user input.
pub fn plan_collection_conflicts(
    order: &LoadOrder,
    collections: &InstalledCollections,
    target_collection_id: Option<&str>,
    target_name: &str,
    is_synced_target: bool,
    incoming: &[IncomingModSpec],
) -> CollectionConflictPlan {
    let incoming_nexus_files = build_incoming_nexus_files(incoming);
    let mut plan = CollectionConflictPlan {
        target_collection_id: target_collection_id.map(str::to_string),
        target_collection_name: target_name.to_string(),
        is_synced_target,
        ..Default::default()
    };

    let mut seen_keys: HashSet<String> = HashSet::new();
    let mut col_summary_map: HashMap<String, ConflictingCollectionSummary> = HashMap::new();

    for spec in incoming {
        let key = spec.conflict_key();
        if !seen_keys.insert(key.clone()) {
            continue;
        }

        let exact = find_exact_match(order, spec);
        let exact_id = exact.map(|m| m.id.as_str());
        let same_identity = find_same_identity_rows(order, spec);
        let other_versions = filter_conflicting_rows(same_identity, exact_id, &incoming_nexus_files);

        if let Some(exact_mod) = exact {
            let damaged = check_staged_mod_integrity(exact_mod).is_some();
            if damaged {
                plan.repair_count += 1;
            } else {
                plan.exact_match_count += 1;
            }

            // Even if an exact match is already staged (e.g. currently disabled), check
            // whether a *different* enabled version of the same mod is active.
            if let Some(active_other) = other_versions.iter().copied().find(|m| {
                m.enabled && !is_exclusive_to_target(m, target_collection_id)
            }) {
                let conflicting_cols =
                    other_collections_for_mod(active_other, order, collections, target_collection_id);
                let recommended = recommend_action(is_synced_target, &conflicting_cols);
                record_conflict_collections(&mut col_summary_map, &conflicting_cols, &key);
                plan.conflicts.push(make_conflict(
                    spec,
                    key,
                    active_other,
                    conflicting_cols,
                    recommended,
                ));
            }
            continue;
        }

        if other_versions.is_empty() {
            plan.install_count += 1;
            continue;
        }

        // If every other-version row belongs exclusively to the target collection itself,
        // treat this as a clean self-update rather than a user conflict.
        if other_versions
            .iter()
            .all(|m| is_exclusive_to_target(m, target_collection_id))
        {
            plan.auto_update_count += 1;
            continue;
        }

        // Only prompt when at least one non-exclusive other version is currently enabled.
        // If the user already disabled an old version / old collection, we can safely
        // install the incoming version alongside without nagging again.
        let enabled_others: Vec<&StagedMod> = other_versions
            .iter()
            .copied()
            .filter(|m| m.enabled && !is_exclusive_to_target(m, target_collection_id))
            .collect();

        if enabled_others.is_empty() {
            plan.install_count += 1;
            continue;
        }

        let primary = enabled_others[0];
        let conflicting_cols =
            other_collections_for_mod(primary, order, collections, target_collection_id);
        let recommended = recommend_action(is_synced_target, &conflicting_cols);
        record_conflict_collections(&mut col_summary_map, &conflicting_cols, &key);
        plan.conflicts
            .push(make_conflict(spec, key, primary, conflicting_cols, recommended));
    }

    let mut summaries: Vec<ConflictingCollectionSummary> = col_summary_map.into_values().collect();
    summaries.sort_by(|a, b| {
        b.conflict_count
            .cmp(&a.conflict_count)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    plan.conflicting_collections = summaries;
    plan
}

fn recommend_action(
    is_synced_target: bool,
    conflicting_cols: &[ConflictingCollectionRef],
) -> ConflictAction {
    if !conflicting_cols.is_empty() {
        if is_synced_target {
            ConflictAction::DisableOtherCollections
        } else {
            ConflictAction::DisableOldAndInstallNew
        }
    } else {
        ConflictAction::ReplaceExisting
    }
}

fn record_conflict_collections(
    map: &mut HashMap<String, ConflictingCollectionSummary>,
    refs: &[ConflictingCollectionRef],
    conflict_key: &str,
) {
    for r in refs {
        let entry = map
            .entry(r.id.clone())
            .or_insert_with(|| ConflictingCollectionSummary {
                id: r.id.clone(),
                name: r.name.clone(),
                is_synced: r.is_synced,
                enabled_mod_count: r.enabled_mod_count,
                conflict_count: 0,
                conflict_keys: Vec::new(),
            });
        if !entry.conflict_keys.iter().any(|k| k == conflict_key) {
            entry.conflict_keys.push(conflict_key.to_string());
            entry.conflict_count += 1;
        }
    }
}

fn make_conflict(
    spec: &IncomingModSpec,
    conflict_key: String,
    existing: &StagedMod,
    conflicting_collections: Vec<ConflictingCollectionRef>,
    recommended_action: ConflictAction,
) -> ModVersionConflict {
    let existing_file_id = match existing.source {
        ModSource::Nexus if existing.nexus_file_id > 0 => Some(existing.nexus_file_id),
        ModSource::Modio => existing.modio_file_id,
        _ => None,
    };
    ModVersionConflict {
        conflict_key,
        source: spec.mod_source(),
        incoming_name: spec.display_name().to_string(),
        incoming_version: spec.version().map(str::to_string),
        incoming_file_id: spec.file_id(),
        incoming_file_name: spec.file_name().map(str::to_string),
        existing_mod_id: existing.id.clone(),
        existing_name: existing.name.clone(),
        existing_version: existing.version.clone(),
        existing_file_id,
        existing_file_name: existing.file_name.clone(),
        existing_enabled: existing.enabled,
        existing_independent: existing.independent,
        conflicting_collections,
        recommended_action,
    }
}

/// Resolve the concrete execution plan for a single incoming mod, taking into account
/// the current load order, the target collection id, all incoming specs, and user choices.
pub fn resolve_mod_execution(
    order: &LoadOrder,
    target_collection_id: Option<&str>,
    incoming_all: &[IncomingModSpec],
    spec: &IncomingModSpec,
    choices: Option<&CollectionResolutionChoices>,
) -> ModExecutionPlan {
    let incoming_nexus_files = build_incoming_nexus_files(incoming_all);
    let key = spec.conflict_key();
    let user_action = choices.and_then(|c| c.mod_actions.get(&key).copied());

    let exact = find_exact_match(order, spec);
    let exact_id = exact.map(|m| m.id.as_str());
    let same_identity = find_same_identity_rows(order, spec);
    let other_versions = filter_conflicting_rows(same_identity, exact_id, &incoming_nexus_files);

    if let Some(exact_mod) = exact {
        let disable_other_ids: Vec<String> = match user_action {
            Some(ConflictAction::KeepBothEnabled) | Some(ConflictAction::KeepExisting) => {
                Vec::new()
            }
            _ => other_versions
                .iter()
                .filter(|m| m.enabled)
                .map(|m| m.id.clone())
                .collect(),
        };
        if check_staged_mod_integrity(exact_mod).is_some() {
            return ModExecutionPlan::RepairExact {
                staged_id: exact_mod.id.clone(),
                disable_other_ids,
            };
        }
        return ModExecutionPlan::ReuseExact {
            staged_id: exact_mod.id.clone(),
            disable_other_ids,
        };
    }

    if other_versions.is_empty() {
        return ModExecutionPlan::InstallFresh;
    }

    // Automatic self-update when existing version belongs exclusively to target collection.
    if other_versions
        .iter()
        .all(|m| is_exclusive_to_target(m, target_collection_id))
    {
        return ModExecutionPlan::ReplaceStaged {
            replace_id: other_versions[0].id.clone(),
        };
    }

    let enabled_others: Vec<&StagedMod> = other_versions
        .iter()
        .copied()
        .filter(|m| m.enabled)
        .collect();
    let primary = enabled_others
        .first()
        .copied()
        .unwrap_or(other_versions[0]);

    match user_action {
        Some(ConflictAction::ReplaceExisting) => ModExecutionPlan::ReplaceStaged {
            replace_id: primary.id.clone(),
        },
        Some(ConflictAction::DisableOtherCollections)
        | Some(ConflictAction::DisableOldAndInstallNew) => {
            ModExecutionPlan::DisableOldAndInstallNew {
                disable_ids: enabled_others.iter().map(|m| m.id.clone()).collect(),
            }
        }
        Some(ConflictAction::KeepExisting) => ModExecutionPlan::KeepExisting {
            staged_id: primary.id.clone(),
        },
        Some(ConflictAction::KeepBothEnabled) => ModExecutionPlan::KeepBothEnabled,
        None => {
            // If no explicit choice was passed (e.g., all other versions are already disabled),
            // install alongside without overwriting disabled versions.
            if enabled_others.is_empty() {
                ModExecutionPlan::InstallFresh
            } else {
                ModExecutionPlan::DisableOldAndInstallNew {
                    disable_ids: enabled_others.iter().map(|m| m.id.clone()).collect(),
                }
            }
        }
    }
}

/// Collect exact-match staged mod IDs from `incoming` so disabling conflicting collections
/// will not disable mods that the incoming collection also needs.
pub fn collect_exact_match_ids(order: &LoadOrder, incoming: &[IncomingModSpec]) -> Vec<String> {
    let mut ids = Vec::new();
    for spec in incoming {
        if let Some(m) = find_exact_match(order, spec) {
            if !ids.contains(&m.id) {
                ids.push(m.id.clone());
            }
        }
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mods::{
        CollectionKind, CollectionSource, FriendSyncMetadata, InstalledCollection,
    };

    fn make_staged_dir(dir: &std::path::Path) -> String {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("plugin.esp"), b"data").unwrap();
        dir.to_string_lossy().to_string()
    }

    #[test]
    fn exact_intact_match_reused_and_damaged_repaired() {
        let tmp = tempfile::tempdir().unwrap();
        let intact_dir = make_staged_dir(&tmp.path().join("intact"));
        let missing_dir = tmp.path().join("missing").to_string_lossy().to_string();

        let order = LoadOrder {
            mods: vec![
                StagedMod {
                    id: "100_1".into(),
                    name: "IntactMod".into(),
                    source: ModSource::Nexus,
                    nexus_mod_id: 100,
                    nexus_file_id: 1,
                    version: Some("1.0".into()),
                    domain: "skyrimspecialedition".into(),
                    staging_path: intact_dir,
                    staged_file_count: Some(1),
                    enabled: true,
                    ..Default::default()
                },
                StagedMod {
                    id: "200_2".into(),
                    name: "DamagedMod".into(),
                    source: ModSource::Nexus,
                    nexus_mod_id: 200,
                    nexus_file_id: 2,
                    version: Some("1.0".into()),
                    domain: "skyrimspecialedition".into(),
                    staging_path: missing_dir,
                    staged_file_count: Some(1),
                    enabled: true,
                    ..Default::default()
                },
            ],
        };
        let cols = InstalledCollections::default();
        let incoming = vec![
            IncomingModSpec::Nexus {
                domain: "skyrimspecialedition".into(),
                mod_id: 100,
                file_id: 1,
                name: "IntactMod".into(),
                version: Some("1.0".into()),
                file_name: None,
            },
            IncomingModSpec::Nexus {
                domain: "skyrimspecialedition".into(),
                mod_id: 200,
                file_id: 2,
                name: "DamagedMod".into(),
                version: Some("1.0".into()),
                file_name: None,
            },
            IncomingModSpec::Nexus {
                domain: "skyrimspecialedition".into(),
                mod_id: 300,
                file_id: 3,
                name: "BrandNew".into(),
                version: Some("1.0".into()),
                file_name: None,
            },
        ];

        let plan = plan_collection_conflicts(&order, &cols, None, "Test", false, &incoming);
        assert_eq!(plan.exact_match_count, 1);
        assert_eq!(plan.repair_count, 1);
        assert_eq!(plan.install_count, 1);
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn self_update_is_auto_replaced_without_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = make_staged_dir(&tmp.path().join("mod1"));
        let order = LoadOrder {
            mods: vec![StagedMod {
                id: "100_1".into(),
                name: "MyMod".into(),
                source: ModSource::Nexus,
                nexus_mod_id: 100,
                nexus_file_id: 1,
                version: Some("1.0".into()),
                domain: "skyrimspecialedition".into(),
                staging_path: staging,
                staged_file_count: Some(1),
                enabled: true,
                independent: false,
                collection_ids: vec!["friend:peer1:col1".into()],
                ..Default::default()
            }],
        };
        let cols = InstalledCollections {
            collections: vec![InstalledCollection {
                id: "friend:peer1:col1".into(),
                name: "Friends Co-op".into(),
                mod_ids: vec!["100_1".into()],
                friend_sync: Some(FriendSyncMetadata {
                    friend_peer_id: "peer1".into(),
                    friend_collection_id: "col1".into(),
                    last_synced_hash: "h1".into(),
                    last_synced_at: String::new(),
                    auto_sync: true,
                }),
                ..Default::default()
            }],
        };
        let incoming = vec![IncomingModSpec::Nexus {
            domain: "skyrimspecialedition".into(),
            mod_id: 100,
            file_id: 2,
            name: "MyMod".into(),
            version: Some("2.0".into()),
            file_name: None,
        }];

        let plan = plan_collection_conflicts(
            &order,
            &cols,
            Some("friend:peer1:col1"),
            "Friends Co-op",
            true,
            &incoming,
        );
        assert_eq!(plan.auto_update_count, 1);
        assert!(plan.conflicts.is_empty());
        assert_eq!(
            resolve_mod_execution(
                &order,
                Some("friend:peer1:col1"),
                &incoming,
                &incoming[0],
                None
            ),
            ModExecutionPlan::ReplaceStaged {
                replace_id: "100_1".into()
            }
        );
    }

    #[test]
    fn other_collection_conflict_recommends_disable_other_collection_for_synced_target() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = make_staged_dir(&tmp.path().join("ts1"));
        let order = LoadOrder {
            mods: vec![StagedMod {
                id: "ts_Owner_Pkg".into(),
                name: "Pkg".into(),
                source: ModSource::Thunderstore,
                version: Some("1.0.0".into()),
                domain: "lethal-company".into(),
                staging_path: staging,
                staged_file_count: Some(1),
                enabled: true,
                independent: false,
                ts_namespace: Some("Owner".into()),
                ts_name: Some("Pkg".into()),
                collection_ids: vec!["emperor-share:solo".into()],
                ..Default::default()
            }],
        };
        let cols = InstalledCollections {
            collections: vec![InstalledCollection {
                id: "emperor-share:solo".into(),
                source: CollectionSource::Emperor,
                kind: CollectionKind::Share,
                name: "Solo Loadout".into(),
                mod_ids: vec!["ts_Owner_Pkg".into()],
                ..Default::default()
            }],
        };
        let incoming = vec![IncomingModSpec::Thunderstore {
            community: "lethal-company".into(),
            namespace: "Owner".into(),
            name: "Pkg".into(),
            version: Some("2.0.0".into()),
            display_name: "Pkg".into(),
        }];

        let plan = plan_collection_conflicts(
            &order,
            &cols,
            Some("friend:peer1:coop"),
            "Friends Co-op",
            true,
            &incoming,
        );
        assert_eq!(plan.conflicts.len(), 1);
        assert_eq!(
            plan.conflicts[0].recommended_action,
            ConflictAction::DisableOtherCollections
        );
        assert_eq!(plan.conflicting_collections.len(), 1);
        assert_eq!(plan.conflicting_collections[0].id, "emperor-share:solo");
        assert!(!plan.conflicting_collections[0].is_synced);
    }

    #[test]
    fn nexus_companion_files_in_same_collection_are_not_conflicts() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = make_staged_dir(&tmp.path().join("main"));
        let order = LoadOrder {
            mods: vec![StagedMod {
                id: "500_10".into(),
                name: "BigMod".into(),
                source: ModSource::Nexus,
                nexus_mod_id: 500,
                nexus_file_id: 10,
                version: Some("1.0".into()),
                domain: "skyrimspecialedition".into(),
                staging_path: staging,
                staged_file_count: Some(1),
                enabled: true,
                independent: true,
                ..Default::default()
            }],
        };
        let cols = InstalledCollections::default();
        let incoming = vec![
            IncomingModSpec::Nexus {
                domain: "skyrimspecialedition".into(),
                mod_id: 500,
                file_id: 10,
                name: "BigMod".into(),
                version: Some("1.0".into()),
                file_name: Some("Main File".into()),
            },
            IncomingModSpec::Nexus {
                domain: "skyrimspecialedition".into(),
                mod_id: 500,
                file_id: 11,
                name: "BigMod".into(),
                version: Some("1.0".into()),
                file_name: Some("Optional Patch".into()),
            },
        ];

        let plan = plan_collection_conflicts(&order, &cols, None, "Pack", false, &incoming);
        assert_eq!(plan.exact_match_count, 1);
        assert_eq!(plan.install_count, 1);
        assert!(plan.conflicts.is_empty());
    }

    #[test]
    fn disabled_old_version_does_not_reprompt_conflict() {
        let tmp = tempfile::tempdir().unwrap();
        let staging = make_staged_dir(&tmp.path().join("old"));
        let order = LoadOrder {
            mods: vec![StagedMod {
                id: "modio_7_42".into(),
                name: "ModioThing".into(),
                source: ModSource::Modio,
                modio_game_id: Some(7),
                modio_mod_id: Some(42),
                modio_file_id: Some(100),
                version: Some("1.0".into()),
                staging_path: staging,
                staged_file_count: Some(1),
                enabled: false,
                independent: false,
                collection_ids: vec!["emperor-share:old".into()],
                ..Default::default()
            }],
        };
        let cols = InstalledCollections::default();
        let incoming = vec![IncomingModSpec::Modio {
            modio_game_id: 7,
            modio_mod_id: 42,
            file_id: Some(200),
            name: "ModioThing".into(),
            version: Some("2.0".into()),
        }];

        let plan = plan_collection_conflicts(&order, &cols, None, "New", true, &incoming);
        assert!(plan.conflicts.is_empty());
        assert_eq!(plan.install_count, 1);
    }
}
