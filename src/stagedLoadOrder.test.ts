import { describe, expect, it } from "vitest";
import {
  reorderStagedMods,
  reorderStagedModToGroupIndex,
} from "./stagedLoadOrder";
import type { StagedMod } from "./types";

function staged(
  partial: Partial<StagedMod> & Pick<StagedMod, "id" | "name" | "order" | "enabled">,
): StagedMod {
  return {
    nexus_mod_id: 1,
    nexus_file_id: 1,
    version: "1",
    domain: "game",
    staging_path: "/tmp/mod",
    source: "nexus",
    ...partial,
  };
}

describe("reorderStagedMods", () => {
  const mods = [
    staged({ id: "b", name: "B", order: 3, enabled: true }),
    staged({ id: "a", name: "A", order: 1, enabled: true }),
    staged({ id: "c", name: "C", order: 4, enabled: true }),
    staged({ id: "d", name: "D", order: 2, enabled: false }),
    staged({ id: "e", name: "E", order: 5, enabled: false }),
  ];

  it("moves an enabled mod down past a disabled mod that sits between them", () => {
    const next = reorderStagedMods(mods, "a", 1);
    expect(next?.map((m) => m.id)).toEqual(["b", "d", "a", "c", "e"]);
    expect(next?.map((m) => m.order)).toEqual([1, 2, 3, 4, 5]);
    expect(mods.map((m) => m.order)).toEqual([3, 1, 4, 2, 5]);
  });

  it("keeps moving on a second click", () => {
    const once = reorderStagedMods(mods, "a", 1);
    const twice = reorderStagedMods(once ?? [], "a", 1);
    expect(twice?.map((m) => m.id)).toEqual(["b", "d", "c", "a", "e"]);
  });

  it("does not move the first or last row of a group", () => {
    expect(reorderStagedMods(mods, "a", -1)).toBeNull();
    expect(reorderStagedMods(mods, "c", 1)).toBeNull();
    expect(reorderStagedMods(mods, "d", -1)).toBeNull();
    expect(reorderStagedMods(mods, "e", 1)).toBeNull();
    expect(reorderStagedMods(mods, "missing", 1)).toBeNull();
  });

  it("moves a disabled mod within its own group", () => {
    const next = reorderStagedMods(mods, "e", -1);
    expect(next?.map((m) => [m.id, m.enabled])).toEqual([
      ["a", true],
      ["e", false],
      ["b", true],
      ["c", true],
      ["d", false],
    ]);
  });
});

describe("reorderStagedModToGroupIndex", () => {
  const mods = [
    staged({ id: "b", name: "B", order: 3, enabled: true }),
    staged({ id: "a", name: "A", order: 1, enabled: true }),
    staged({ id: "c", name: "C", order: 4, enabled: true }),
    staged({ id: "d", name: "D", order: 2, enabled: false }),
    staged({ id: "e", name: "E", order: 5, enabled: false }),
  ];

  it("matches two one-step moves when jumping to the end of the group", () => {
    const jumped = reorderStagedModToGroupIndex(mods, "a", 2);
    const once = reorderStagedMods(mods, "a", 1);
    const twice = reorderStagedMods(once ?? [], "a", 1);
    expect(jumped?.map((m) => m.id)).toEqual(twice?.map((m) => m.id));
    expect(jumped?.map((m) => m.id)).toEqual(["b", "d", "c", "a", "e"]);
  });

  it("returns null when the group index is unchanged", () => {
    expect(reorderStagedModToGroupIndex(mods, "a", 0)).toBeNull();
    expect(reorderStagedModToGroupIndex(mods, "missing", 1)).toBeNull();
  });

  it("clamps out-of-range group indices", () => {
    const next = reorderStagedModToGroupIndex(mods, "a", 99);
    expect(next?.map((m) => m.id)).toEqual(["b", "d", "c", "a", "e"]);
  });

  it("moves a disabled mod within its own group", () => {
    const next = reorderStagedModToGroupIndex(mods, "e", 0);
    expect(next?.map((m) => [m.id, m.enabled])).toEqual([
      ["a", true],
      ["e", false],
      ["b", true],
      ["c", true],
      ["d", false],
    ]);
  });

  it("moves an enabled mod upward across peers", () => {
    const next = reorderStagedModToGroupIndex(mods, "c", 0);
    expect(next?.map((m) => m.id)).toEqual(["c", "d", "a", "b", "e"]);
  });
});
