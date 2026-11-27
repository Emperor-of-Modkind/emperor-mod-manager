import type { StagedMod } from "./types";

export type StagedModGroup = "tools" | "enabled" | "disabled";

/** Section group for a staged mod in the Library staged mods accordion. */
export function stagedModGroup(
  mod: Pick<StagedMod, "enabled" | "plugin_tool_id">,
): StagedModGroup {
  if (!mod.enabled) return "disabled";
  return mod.plugin_tool_id ? "tools" : "enabled";
}

/** Next or previous row in the same accordion group. Other rows are skipped. */
export function sameGroupNeighborIndex(
  list: StagedMod[],
  index: number,
  dir: -1 | 1,
): number {
  const row = list[index];
  if (!row) return -1;
  const group = stagedModGroup(row);
  let j = index + dir;
  while (j >= 0 && j < list.length && stagedModGroup(list[j]) !== group) {
    j += dir;
  }
  return j >= 0 && j < list.length ? j : -1;
}

/** Sequence the load-order list is drawn from. Array position is not the order. */
export function stagedModsInLoadOrder(mods: StagedMod[]): StagedMod[] {
  return [...mods].sort((a, b) => a.order - b.order);
}

/**
 * Move one mod one step within its accordion group (tools, enabled, or disabled).
 * The neighbor is chosen from `order`, then every row is numbered `1..n`.
 * Returns null when the mod is missing or already at the end of its group.
 */
export function reorderStagedMods(
  mods: StagedMod[],
  modId: string,
  dir: -1 | 1,
): StagedMod[] | null {
  const sorted = stagedModsInLoadOrder(mods);
  const i = sorted.findIndex((m) => m.id === modId);
  if (i < 0) return null;
  const j = sameGroupNeighborIndex(sorted, i, dir);
  if (j < 0) return null;
  [sorted[i], sorted[j]] = [sorted[j], sorted[i]];
  return sorted.map((m, index) => ({ ...m, order: index + 1 }));
}

/**
 * Move one mod to a new index within its accordion group (tools, enabled, or disabled).
 * Rows in other groups keep their absolute slots; same-group slots
 * consume the reordered subsequence. Equivalent to repeated one-step swaps.
 * Returns null when the mod is missing or the group index is unchanged.
 */
export function reorderStagedModToGroupIndex(
  mods: StagedMod[],
  modId: string,
  targetGroupIndex: number,
): StagedMod[] | null {
  const sorted = stagedModsInLoadOrder(mods);
  const fromAbs = sorted.findIndex((m) => m.id === modId);
  if (fromAbs < 0) return null;
  const groupKey = stagedModGroup(sorted[fromAbs]);
  const group = sorted.filter((m) => stagedModGroup(m) === groupKey);
  const fromGroup = group.findIndex((m) => m.id === modId);
  if (fromGroup < 0) return null;
  const clamped = Math.max(0, Math.min(group.length - 1, targetGroupIndex));
  if (clamped === fromGroup) return null;

  const nextGroup = [...group];
  const [moved] = nextGroup.splice(fromGroup, 1);
  nextGroup.splice(clamped, 0, moved);

  let gi = 0;
  const next = sorted.map((m) =>
    stagedModGroup(m) === groupKey ? nextGroup[gi++] : m,
  );
  return next.map((m, index) => ({ ...m, order: index + 1 }));
}
