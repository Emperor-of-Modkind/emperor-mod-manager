import { describe, expect, it } from "vitest";
import { computeFoldableLayout } from "./useFoldableLayout";

describe("computeFoldableLayout", () => {
  it("uses compact single-pane bottom nav on cover screens (<600px)", () => {
    const layout = computeFoldableLayout(390, 844, null);
    expect(layout.sizeClass).toBe("compact");
    expect(layout.posture).toBe("normal");
    expect(layout.navMode).toBe("bottom");
    expect(layout.isDualPane).toBe(false);
    expect(layout.hasHinge).toBe(false);
  });

  it("uses rail + dual-pane book posture on unfolded vertical-hinge foldables", () => {
    const layout = computeFoldableLayout(768, 844, {
      has_hinge: true,
      state: "flat",
      orientation: "vertical",
      is_separating: true,
      hinge_x: 380,
      hinge_y: 0,
      hinge_w: 8,
      hinge_h: 844,
    });
    expect(layout.sizeClass).toBe("medium");
    expect(layout.posture).toBe("book");
    expect(layout.navMode).toBe("rail");
    expect(layout.isDualPane).toBe(true);
    expect(layout.hingeRect).toEqual({ x: 380, y: 0, width: 8, height: 844 });
  });

  it("detects tabletop posture when horizontal hinge is half-opened", () => {
    const layout = computeFoldableLayout(720, 900, {
      has_hinge: true,
      state: "half_opened",
      orientation: "horizontal",
      is_separating: true,
      hinge_x: 0,
      hinge_y: 446,
      hinge_w: 720,
      hinge_h: 8,
    });
    expect(layout.posture).toBe("tabletop");
    expect(layout.hingeOrientation).toBe("horizontal");
    expect(layout.hingeRect?.y).toBe(446);
  });

  it("falls back to CSS Viewport Segments when native plugin event is absent", () => {
    const book = computeFoldableLayout(760, 820, null, 2, 1);
    expect(book.posture).toBe("book");
    expect(book.isDualPane).toBe(true);

    const tabletop = computeFoldableLayout(420, 860, null, 1, 2);
    expect(tabletop.posture).toBe("tabletop");
  });

  it("uses sidebar + dual-pane on expanded desktop/tablet viewports", () => {
    const layout = computeFoldableLayout(1200, 800, null);
    expect(layout.sizeClass).toBe("expanded");
    expect(layout.navMode).toBe("sidebar");
    expect(layout.isDualPane).toBe(true);
  });
});
