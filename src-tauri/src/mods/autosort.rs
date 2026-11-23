//! Requirement-based auto-sorting for staged mods across all games.
//!
//! Performs a minimal-disruption stable topological sort (Kahn's algorithm backed
//! by a min-heap keyed on each mod's original load-order position). Mods whose
//! requirements are already satisfied keep their relative manual order; only
//! mods that violate a dependency edge are moved.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap, HashSet};
use std::path::Path;

use serde::{Deserialize, Serialize};
use walkdir::WalkDir;

use super::StagedMod;
use crate::games::creation_engine::{is_plugin_file, plugin_masters};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoSortResult {
    /// New full staged mod ID order (`1..=n`).
    pub ordered_ids: Vec<String>,
    /// Previous staged mod ID order before sorting (for one-click Undo).
    pub previous_ordered_ids: Vec<String>,
    /// Number of mods whose position changed.
    pub moved_count: usize,
    /// Number of dependency edges considered among enabled mods.
    pub dependency_edges_used: usize,
    /// Non-fatal warnings (e.g. dependency cycles or offline requirement lookup).
    pub warnings: Vec<String>,
    /// Any detected dependency cycles (each inner Vec lists mod names in the cycle).
    pub cycles: Vec<Vec<String>>,
}

/// Sort staged mods so every mod loads after the mods it depends on.
///
/// - `extra_edges`: `(dependent_mod_id, required_mod_id)` pairs gathered from
///   external sources (Nexus requirements, Mod.io dependencies, Bethesda MAST
///   headers, etc.), combined with `StagedMod.depends_on`.
/// - Enabled mods are sorted using minimal-disruption stable topological sort;
///   disabled mods keep their relative order in their own group (matching the
///   UI's enabled/disabled load-order partitioning).
pub fn sort_staged_mods(mods: &[StagedMod], extra_edges: &[(String, String)]) -> AutoSortResult {
    let mut by_order: Vec<&StagedMod> = mods.iter().collect();
    by_order.sort_by_key(|m| m.order);

    let previous_ordered_ids: Vec<String> = by_order.iter().map(|m| m.id.clone()).collect();
    if by_order.len() <= 1 {
        return AutoSortResult {
            ordered_ids: previous_ordered_ids.clone(),
            previous_ordered_ids,
            moved_count: 0,
            dependency_edges_used: 0,
            warnings: Vec::new(),
            cycles: Vec::new(),
        };
    }

    let enabled: Vec<&StagedMod> = by_order.iter().copied().filter(|m| m.enabled).collect();
    let disabled: Vec<&StagedMod> = by_order.iter().copied().filter(|m| !m.enabled).collect();

    let (sorted_enabled, edges_used, warnings, cycles) = topo_sort_group(&enabled, extra_edges);

    // Re-interleave enabled and disabled slots just like `stagedLoadOrder.ts` so
    // disabled mods keep their absolute positions while enabled mods take the
    // newly sorted sequence.
    let mut ei = 0usize;
    let mut di = 0usize;
    let mut ordered_ids = Vec::with_capacity(by_order.len());
    for m in &by_order {
        if m.enabled {
            ordered_ids.push(sorted_enabled[ei].clone());
            ei += 1;
        } else {
            ordered_ids.push(disabled[di].id.clone());
            di += 1;
        }
    }

    let moved_count = previous_ordered_ids
        .iter()
        .zip(ordered_ids.iter())
        .filter(|(a, b)| a != b)
        .count();

    AutoSortResult {
        ordered_ids,
        previous_ordered_ids,
        moved_count,
        dependency_edges_used: edges_used,
        warnings,
        cycles,
    }
}

fn topo_sort_group(
    group: &[&StagedMod],
    extra_edges: &[(String, String)],
) -> (Vec<String>, usize, Vec<String>, Vec<Vec<String>>) {
    let n = group.len();
    if n <= 1 {
        return (
            group.iter().map(|m| m.id.clone()).collect(),
            0,
            Vec::new(),
            Vec::new(),
        );
    }

    let id_to_idx: HashMap<&str, usize> = group
        .iter()
        .enumerate()
        .map(|(i, m)| (m.id.as_str(), i))
        .collect();

    // Directed graph: edge `req -> dep` means `req` must load BEFORE `dep`.
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut in_degree: Vec<usize> = vec![0; n];
    let mut seen_edges: HashSet<(usize, usize)> = HashSet::new();

    let mut add_edge = |dependent_id: &str, required_id: &str| {
        let Some(&dep_idx) = id_to_idx.get(dependent_id) else {
            return;
        };
        let Some(&req_idx) = id_to_idx.get(required_id) else {
            return;
        };
        if dep_idx == req_idx {
            return;
        }
        if seen_edges.insert((req_idx, dep_idx)) {
            adj[req_idx].push(dep_idx);
            in_degree[dep_idx] += 1;
        }
    };

    for m in group {
        for req_id in &m.depends_on {
            add_edge(&m.id, req_id);
        }
    }
    for (dep_id, req_id) in extra_edges {
        add_edge(dep_id, req_id);
    }

    let edges_used = seen_edges.len();

    // Min-heap of ready indices (ordered by original position for minimal movement).
    let mut ready: BinaryHeap<Reverse<usize>> = BinaryHeap::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            ready.push(Reverse(i));
        }
    }

    let mut placed = vec![false; n];
    let mut result_indices = Vec::with_capacity(n);
    let mut warnings = Vec::new();
    let mut cycles: Vec<Vec<String>> = Vec::new();

    while result_indices.len() < n {
        if let Some(Reverse(u)) = ready.pop() {
            if placed[u] {
                continue;
            }
            placed[u] = true;
            result_indices.push(u);
            for &v in &adj[u] {
                if placed[v] {
                    continue;
                }
                in_degree[v] = in_degree[v].saturating_sub(1);
                if in_degree[v] == 0 {
                    ready.push(Reverse(v));
                }
            }
        } else {
            // Cycle detected among remaining unplaced nodes.
            // Pick the lowest-original-index unplaced node to break the cycle deterministically.
            let Some(start) = (0..n).find(|&i| !placed[i]) else {
                break;
            };
            let cycle_nodes = collect_cycle_names(start, &adj, &placed, group);
            if !cycle_nodes.is_empty() {
                warnings.push(format!(
                    "Dependency cycle detected involving: {}. Preserved relative order for this cycle.",
                    cycle_nodes.join(" -> ")
                ));
                cycles.push(cycle_nodes);
            }
            placed[start] = true;
            result_indices.push(start);
            for &v in &adj[start] {
                if placed[v] {
                    continue;
                }
                in_degree[v] = in_degree[v].saturating_sub(1);
                if in_degree[v] == 0 {
                    ready.push(Reverse(v));
                }
            }
        }
    }

    let ordered = result_indices
        .into_iter()
        .map(|i| group[i].id.clone())
        .collect();
    (ordered, edges_used, warnings, cycles)
}

fn collect_cycle_names(
    start: usize,
    adj: &[Vec<usize>],
    placed: &[bool],
    group: &[&StagedMod],
) -> Vec<String> {
    let mut visited = HashSet::new();
    let mut path = Vec::new();
    let mut cur = start;
    while visited.insert(cur) {
        path.push(group[cur].name.clone());
        let next = adj[cur].iter().copied().find(|&v| !placed[v]);
        match next {
            Some(v) => cur = v,
            None => break,
        }
    }
    path
}

/// Scan staged mod directories for Creation Engine `.esm`/`.esp`/`.esl` plugins
/// and return `(dependent_mod_id, master_owner_mod_id)` edges derived from `MAST` headers.
pub fn collect_bethesda_mast_edges(mods: &[StagedMod], mods_dir: &Path) -> Vec<(String, String)> {
    // Map plugin filename (lowercase) -> owning staged mod id.
    let mut plugin_owner: HashMap<String, String> = HashMap::new();
    let mut mod_plugins: Vec<(String, Vec<std::path::PathBuf>)> = Vec::new();

    for m in mods.iter().filter(|m| m.enabled) {
        let dir = mods_dir.join(&m.id);
        if !dir.is_dir() {
            continue;
        }
        let mut files = Vec::new();
        for entry in WalkDir::new(&dir).max_depth(4).into_iter().flatten() {
            if !entry.file_type().is_file() {
                continue;
            }
            let name = entry.file_name().to_string_lossy();
            if is_plugin_file(&name) {
                plugin_owner
                    .entry(name.to_lowercase())
                    .or_insert_with(|| m.id.clone());
                files.push(entry.into_path());
            }
        }
        if !files.is_empty() {
            mod_plugins.push((m.id.clone(), files));
        }
    }

    let mut edges = Vec::new();
    for (mod_id, files) in mod_plugins {
        for path in files {
            let Some(masters) = plugin_masters(&path) else {
                continue;
            };
            for master in masters {
                if let Some(owner_id) = plugin_owner.get(&master.to_lowercase()) {
                    if owner_id != &mod_id {
                        edges.push((mod_id.clone(), owner_id.clone()));
                    }
                }
            }
        }
    }
    edges
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk(id: &str, order: u32, enabled: bool, depends_on: &[&str]) -> StagedMod {
        StagedMod {
            id: id.to_string(),
            name: id.to_string(),
            order,
            enabled,
            depends_on: depends_on.iter().map(|s| s.to_string()).collect(),
            ..StagedMod::default()
        }
    }

    #[test]
    fn already_satisfied_order_makes_zero_moves() {
        let mods = vec![
            mk("core", 1, true, &[]),
            mk("ui", 2, true, &["core"]),
            mk("cosmetic", 3, true, &[]),
        ];
        let res = sort_staged_mods(&mods, &[]);
        assert_eq!(res.ordered_ids, vec!["core", "ui", "cosmetic"]);
        assert_eq!(res.moved_count, 0);
        assert_eq!(res.dependency_edges_used, 1);
        assert!(res.warnings.is_empty());
    }

    #[test]
    fn moves_required_mod_before_dependent_preserving_unrelated_order() {
        // `dependent` is at slot 1, `unrelated` at slot 2, `base` at slot 3.
        // Only `base` needs to come before `dependent`; `unrelated` stays as early as possible.
        let mods = vec![
            mk("dependent", 1, true, &["base"]),
            mk("unrelated", 2, true, &[]),
            mk("base", 3, true, &[]),
        ];
        let res = sort_staged_mods(&mods, &[]);
        assert_eq!(res.ordered_ids, vec!["unrelated", "base", "dependent"]);
        assert!(res.moved_count > 0);
    }

    #[test]
    fn handles_transitive_chain_via_extra_edges() {
        let mods = vec![
            mk("c", 1, true, &[]),
            mk("b", 2, true, &[]),
            mk("a", 3, true, &[]),
        ];
        let extra = vec![
            ("c".to_string(), "b".to_string()),
            ("b".to_string(), "a".to_string()),
        ];
        let res = sort_staged_mods(&mods, &extra);
        assert_eq!(res.ordered_ids, vec!["a", "b", "c"]);
        assert_eq!(res.dependency_edges_used, 2);
    }

    #[test]
    fn keeps_disabled_mods_in_place() {
        let mods = vec![
            mk("dep", 1, true, &["base"]),
            mk("disabled_mid", 2, false, &[]),
            mk("base", 3, true, &[]),
        ];
        let res = sort_staged_mods(&mods, &[]);
        assert_eq!(res.ordered_ids, vec!["base", "disabled_mid", "dep"]);
    }

    #[test]
    fn breaks_cycles_deterministically_with_warning() {
        let mods = vec![
            mk("a", 1, true, &["b"]),
            mk("b", 2, true, &["a"]),
            mk("c", 3, true, &[]),
        ];
        let res = sort_staged_mods(&mods, &[]);
        assert_eq!(res.ordered_ids.len(), 3);
        assert!(!res.warnings.is_empty());
        assert!(!res.cycles.is_empty());
    }
}
