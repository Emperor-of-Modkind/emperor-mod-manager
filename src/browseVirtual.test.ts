import { describe, expect, it } from "vitest";

import {
  browsePageCount,
  estimateGridRowHeight,
  gridCardWidth,
  gridColumnCount,
  isSentinelInScrollView,
  sentinelContentTop,
} from "./browseVirtual";

describe("gridColumnCount", () => {
  it("matches auto-fill minmax tracks", () => {
    expect(gridColumnCount(800, 160, 13.6)).toBe(4);
  });

  it("fits an exact number of tracks and drops one when a pixel short", () => {
    expect(gridColumnCount(512, 160, 16)).toBe(3);
    expect(gridColumnCount(511, 160, 16)).toBe(2);
  });

  it("returns one column when the container is narrower than a card", () => {
    expect(gridColumnCount(100, 160, 16)).toBe(1);
  });

  it("returns one column for a non-positive width or minimum", () => {
    expect(gridColumnCount(0, 160, 16)).toBe(1);
    expect(gridColumnCount(-20, 160, 16)).toBe(1);
    expect(gridColumnCount(400, 0, 16)).toBe(1);
  });

  it("ignores a negative gap", () => {
    expect(gridColumnCount(500, 160, -5)).toBe(3);
  });
});

describe("gridCardWidth", () => {
  it("splits the row after column gaps", () => {
    expect(gridCardWidth(512, 3, 16)).toBe(160);
  });

  it("uses the full width for a single column", () => {
    expect(gridCardWidth(100, 1, 16)).toBe(100);
  });
});

describe("browsePageCount", () => {
  it("fills the pane and adds one row past the fold", () => {
    expect(browsePageCount(8, 900, 300, 16)).toBe(32);
  });

  it("falls back when the pane has not been measured", () => {
    expect(browsePageCount(8, 0, 300, 16)).toBe(24);
    expect(browsePageCount(0, 900, 300, 16)).toBe(24);
    expect(browsePageCount(8, 900, 0, 16)).toBe(24);
  });

  it("caps a very tall window", () => {
    expect(browsePageCount(20, 8000, 200, 16)).toBe(48);
  });
});

describe("estimateGridRowHeight", () => {
  it("reserves a 2/3 cover plus title and meta", () => {
    const height = estimateGridRowHeight(160, 16);
    expect(height).toBeGreaterThan(160 * 1.5);
    expect(height).toBeLessThan(160 * 1.5 + 120);
  });
});

describe("sentinelContentTop", () => {
  it("stays invariant when the user scrolls while a page is in flight", () => {
    const rootTop = 180;
    const beforeScroll = sentinelContentTop(900, rootTop, 200);
    // User scrolls down 350px: viewport sentinelTop drops by 350px, scrollTop rises by 350px.
    const duringScroll = sentinelContentTop(550, rootTop, 550);
    expect(duringScroll).toBe(beforeScroll);
    // New row appends 300px of content above the sentinel.
    const afterAppend = sentinelContentTop(850, rootTop, 550);
    expect(afterAppend).toBe(beforeScroll + 300);
  });
});

describe("isSentinelInScrollView", () => {
  const viewport = { width: 1280, height: 800 };

  it("returns false when the sentinel sits below a bounded scroll container", () => {
    expect(
      isSentinelInScrollView(
        { top: 180, left: 220 },
        { width: 1000, height: 600 },
        { top: 1500, bottom: 2000, left: 220, right: 1220 },
        viewport,
      ),
    ).toBe(false);
  });

  it("returns true once the sentinel scrolls into view", () => {
    expect(
      isSentinelInScrollView(
        { top: 180, left: 220 },
        { width: 1000, height: 600 },
        { top: 720, bottom: 1100, left: 220, right: 1220 },
        viewport,
      ),
    ).toBe(true);
  });

  it("clamps against the browser viewport so an unconstrained container cannot runaway-load", () => {
    // Broken flex chain: rootClient.height expanded to 50,000px, sentinel is at y=2000 (off-screen).
    expect(
      isSentinelInScrollView(
        { top: 180, left: 220 },
        { width: 1000, height: 50000 },
        { top: 2000, bottom: 2500, left: 220, right: 1220 },
        viewport,
      ),
    ).toBe(false);
  });

  it("returns false for zero-sized or parked containers", () => {
    expect(
      isSentinelInScrollView(
        { top: 0, left: 0 },
        { width: 0, height: 0 },
        { top: 0, bottom: 100, left: 0, right: 100 },
        viewport,
      ),
    ).toBe(false);
  });
});
