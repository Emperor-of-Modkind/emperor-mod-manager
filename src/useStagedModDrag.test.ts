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
});
