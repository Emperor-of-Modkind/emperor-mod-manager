import { describe, expect, it } from "vitest";

import { previewVisibleMods, stagedDropGroupIndex } from "./useStagedModDrag";
import type { StagedMod } from "./types";

const display = [
  { id: "a", enabled: true },
  { id: "b", enabled: true },
  { id: "c", enabled: true },
  { id: "d", enabled: false },
];

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

describe("stagedDropGroupIndex", () => {
  it("inserts before the row whose midpoint the pointer is above", () => {
    expect(stagedDropGroupIndex(display, "b", "enabled", 4, 0, 40)).toBe(0);
    expect(stagedDropGroupIndex(display, "b", "enabled", 6, 0, 40)).toBe(1);
    expect(stagedDropGroupIndex(display, "b", "enabled", 26, 0, 40)).toBe(2);
  });

  it("counts slots that are not mounted", () => {
    expect(stagedDropGroupIndex(display, "a", "enabled", 35, 0, 40)).toBe(2);
  });

  it("uses the disabled block's own indexes", () => {
    const withDisabled = [...display, { id: "e", enabled: false }];
    expect(stagedDropGroupIndex(withDisabled, "d", "disabled", 5, 0, 50)).toBe(0);
    expect(stagedDropGroupIndex(withDisabled, "d", "disabled", 46, 0, 50)).toBe(1);
  });

  it("computes drop index against isolated tools-only, enabled-only, or disabled-only lists", () => {
    const toolsOnly = [
      { id: "t1", enabled: true, plugin_tool_id: "tool-1" },
      { id: "t2", enabled: true, plugin_tool_id: "tool-2" },
    ];
    expect(stagedDropGroupIndex(toolsOnly, "t1", "tools", 4, 0, 20)).toBe(0);
    expect(stagedDropGroupIndex(toolsOnly, "t1", "tools", 16, 0, 20)).toBe(1);

    const enabledOnly = [
      { id: "a", enabled: true },
      { id: "b", enabled: true },
      { id: "c", enabled: true },
    ];
    expect(stagedDropGroupIndex(enabledOnly, "b", "enabled", 4, 0, 30)).toBe(0);
    expect(stagedDropGroupIndex(enabledOnly, "b", "enabled", 16, 0, 30)).toBe(1);
    expect(stagedDropGroupIndex(enabledOnly, "b", "enabled", 28, 0, 30)).toBe(2);

    const disabledOnly = [
      { id: "d", enabled: false },
      { id: "e", enabled: false },
      { id: "f", enabled: false },
    ];
    expect(stagedDropGroupIndex(disabledOnly, "d", "disabled", 100, 100, 30)).toBe(0);
    expect(stagedDropGroupIndex(disabledOnly, "d", "disabled", 118, 100, 30)).toBe(1);
    expect(stagedDropGroupIndex(disabledOnly, "d", "disabled", 128, 100, 30)).toBe(2);
  });
});

describe("previewVisibleMods", () => {
  const visible = [
    staged({ id: "t1", name: "Tool 1", order: 1, enabled: true, plugin_tool_id: "tool-1" }),
    staged({ id: "t2", name: "Tool 2", order: 2, enabled: true, plugin_tool_id: "tool-2" }),
    staged({ id: "a", name: "A", order: 3, enabled: true }),
    staged({ id: "b", name: "B", order: 4, enabled: true }),
    staged({ id: "d1", name: "D1", order: 5, enabled: false }),
    staged({ id: "d2", name: "D2", order: 6, enabled: false, plugin_tool_id: "tool-3" }),
  ];

  it("previews drags within tools, enabled, and disabled independently", () => {
    expect(previewVisibleMods(visible, "t1", 1).map((m) => m.id)).toEqual([
      "t2",
      "t1",
      "a",
      "b",
      "d1",
      "d2",
    ]);
    expect(previewVisibleMods(visible, "a", 1).map((m) => m.id)).toEqual([
      "t1",
      "t2",
      "b",
      "a",
      "d1",
      "d2",
    ]);
    expect(previewVisibleMods(visible, "d2", 0).map((m) => m.id)).toEqual([
      "t1",
      "t2",
      "a",
      "b",
      "d2",
      "d1",
    ]);
  });
});
