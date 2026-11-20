import { describe, expect, it } from "vitest";

import {
  MIN_CONTRAST,
  accentInk,
  contentRoles,
  contrastRatio,
  onAccent,
  parseColor,
  relativeLuminance,
  repairMuted,
} from "./contrast";

function ratio(a: string, b: string): number {
  const left = parseColor(a);
  const right = parseColor(b);
  if (!left || !right) throw new Error(`unparsed ${a} ${b}`);
  return contrastRatio(left, right);
}

function lum(hex: string): number {
  const rgb = parseColor(hex);
  if (!rgb) throw new Error(`unparsed ${hex}`);
  return relativeLuminance(rgb);
}

describe("onAccent", () => {
  it("uses a dark tone on mint and #00cc6a", () => {
    for (const fill of ["#00e0b1", "#00cc6a"]) {
      const on = onAccent(fill);
      expect(lum(on)).toBeLessThan(0.2);
      expect(ratio(on, fill)).toBeGreaterThanOrEqual(MIN_CONTRAST);
      expect(on).not.toBe("#000000");
    }
  });

  it("keeps light text on #0078d4 and brand teal", () => {
    for (const fill of ["#0078d4", "#0f6a5c"]) {
      const on = onAccent(fill);
      expect(lum(on)).toBeGreaterThan(0.7);
      expect(ratio(on, fill)).toBeGreaterThanOrEqual(MIN_CONTRAST);
    }
  });

  it("keeps a toolkit foreground that already contrasts", () => {
    expect(onAccent("#00e0b1", "#00382a")).toBe("#00382a");
  });

  it("rejects a toolkit foreground that fails contrast", () => {
    const on = onAccent("#00e0b1", "#ffffff");
    expect(on).not.toBe("#ffffff");
    expect(lum(on)).toBeLessThan(0.2);
    expect(ratio(on, "#00e0b1")).toBeGreaterThanOrEqual(MIN_CONTRAST);
  });
});

describe("repairMuted", () => {
  it("pulls a too-light muted color toward ink", () => {
    const repaired = repairMuted("#d0d0d0", "#f3f3f3", "#ffffff", "#1a1a1a");
    expect(ratio(repaired, "#f3f3f3")).toBeGreaterThanOrEqual(MIN_CONTRAST);
    expect(ratio(repaired, "#ffffff")).toBeGreaterThanOrEqual(MIN_CONTRAST);
    expect(lum(repaired)).toBeLessThan(lum("#d0d0d0"));
  });

  it("leaves a muted color that already passes", () => {
    expect(repairMuted("#5c5c5c", "#f3f3f3", "#ffffff", "#1a1a1a")).toBe("#5c5c5c");
  });
});

describe("accentInk", () => {
  it("darkens a light accent used as text on a light surface", () => {
    const ink = accentInk("#00cc6a", ["#f3f3f3", "#ffffff"]);
    expect(ratio(ink, "#f3f3f3")).toBeGreaterThanOrEqual(MIN_CONTRAST);
    expect(ratio(ink, "#ffffff")).toBeGreaterThanOrEqual(MIN_CONTRAST);
    expect(lum(ink)).toBeLessThan(lum("#00cc6a"));
  });
});

describe("contentRoles", () => {
  it("drops an OS foreground that fails and still clears hover contrast", () => {
    const roles = contentRoles({
      accent: "#00e0b1",
      bg: "#f3f3f3",
      panel: "#ffffff",
      ink: "#1a1a1a",
      muted: "#d0d0d0",
      osOnAccent: "#ffffff",
    });
    expect(roles.onAccent).not.toBe("#ffffff");
    expect(ratio(roles.onAccent, roles.accentHover)).toBeGreaterThanOrEqual(MIN_CONTRAST);
    expect(ratio(roles.muted, "#ffffff")).toBeGreaterThanOrEqual(MIN_CONTRAST);
  });
});
