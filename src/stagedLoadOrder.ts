import type { StagedMod } from "./types";

/** Next or previous row with the same enabled state. Other rows are skipped. */
export function sameEnabledNeighborIndex(
  list: StagedMod[],
  index: number,
  dir: -1 | 1,
): number {
  const enabled = list[index]?.enabled;
  if (enabled === undefined) return -1;
  let j = index + dir;
  while (j >= 0 && j < list.length && list[j].enabled !== enabled) {
    j += dir;
  }
  return j >= 0 && j < list.length ? j : -1;
}

/** Sequence the load-order list is drawn from. Array position is not the order. */
export function stagedModsInLoadOrder(mods: StagedMod[]): StagedMod[] {
  return [...mods].sort((a, b) => a.order - b.order);
}

/**
 * Move one mod one step within its enabled or disabled group.
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
  const j = sameEnabledNeighborIndex(sorted, i, dir);
  if (j < 0) return null;
  [sorted[i], sorted[j]] = [sorted[j], sorted[i]];
  return sorted.map((m, index) => ({ ...m, order: index + 1 }));
}
