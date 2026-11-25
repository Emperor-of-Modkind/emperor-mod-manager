import { describe, expect, it } from "vitest";

import { stagedDropGroupIndex } from "./useStagedModDrag";

const display = [
  { id: "a", enabled: true },
  { id: "b", enabled: true },
  { id: "c", enabled: true },
  { id: "d", enabled: false },
];

describe("stagedDropGroupIndex", () => {
  it("inserts before the row whose midpoint the pointer is above", () => {
    expect(stagedDropGroupIndex(display, "b", true, 4, 0, 40)).toBe(0);
    expect(stagedDropGroupIndex(display, "b", true, 6, 0, 40)).toBe(1);
    expect(stagedDropGroupIndex(display, "b", true, 26, 0, 40)).toBe(2);
  });

  it("counts slots that are not mounted", () => {
    expect(stagedDropGroupIndex(display, "a", true, 35, 0, 40)).toBe(2);
  });

  it("uses the disabled block's own indexes", () => {
    const withDisabled = [...display, { id: "e", enabled: false }];
    expect(stagedDropGroupIndex(withDisabled, "d", false, 5, 0, 50)).toBe(0);
    expect(stagedDropGroupIndex(withDisabled, "d", false, 46, 0, 50)).toBe(1);
  });

  it("computes drop index against isolated enabled-only or disabled-only lists", () => {
    const enabledOnly = [
      { id: "a", enabled: true },
      { id: "b", enabled: true },
      { id: "c", enabled: true },
    ];
    expect(stagedDropGroupIndex(enabledOnly, "b", true, 4, 0, 30)).toBe(0);
    expect(stagedDropGroupIndex(enabledOnly, "b", true, 16, 0, 30)).toBe(1);
    expect(stagedDropGroupIndex(enabledOnly, "b", true, 28, 0, 30)).toBe(2);

    const disabledOnly = [
      { id: "d", enabled: false },
      { id: "e", enabled: false },
      { id: "f", enabled: false },
    ];
    expect(stagedDropGroupIndex(disabledOnly, "d", false, 100, 100, 30)).toBe(0);
    expect(stagedDropGroupIndex(disabledOnly, "d", false, 118, 100, 30)).toBe(1);
    expect(stagedDropGroupIndex(disabledOnly, "d", false, 128, 100, 30)).toBe(2);
  });
});
