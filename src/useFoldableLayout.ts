import { useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";

export type LayoutSizeClass = "compact" | "medium" | "expanded";
export type FoldPosture = "normal" | "book" | "tabletop";
export type NavMode = "bottom" | "rail" | "sidebar";

export type NativeFoldEvent = {
  has_hinge: boolean;
  state: "flat" | "half_opened";
  orientation: "vertical" | "horizontal" | "none";
  is_separating: boolean;
  hinge_x: number;
  hinge_y: number;
  hinge_w: number;
  hinge_h: number;
};

export type FoldableLayoutState = {
  width: number;
  height: number;
  sizeClass: LayoutSizeClass;
  posture: FoldPosture;
  navMode: NavMode;
  isDualPane: boolean;
  hasHinge: boolean;
  hingeOrientation: "vertical" | "horizontal" | "none";
  hingeRect: { x: number; y: number; width: number; height: number } | null;
};

export function computeFoldableLayout(
  width: number,
  height: number,
  nativeFold: NativeFoldEvent | null,
  cssHorizontalSegments = 1,
  cssVerticalSegments = 1,
): FoldableLayoutState {
  const sizeClass: LayoutSizeClass =
    width < 600 ? "compact" : width < 840 ? "medium" : "expanded";

  const hasNativeHinge = Boolean(nativeFold?.has_hinge);
  const hasCssVerticalHinge = cssHorizontalSegments >= 2;
  const hasCssHorizontalHinge = cssVerticalSegments >= 2;

  let posture: FoldPosture = "normal";
  let hingeOrientation: "vertical" | "horizontal" | "none" = "none";
  let hingeRect: { x: number; y: number; width: number; height: number } | null = null;

  if (hasNativeHinge && nativeFold) {
    hingeOrientation = nativeFold.orientation;
    hingeRect = {
      x: Math.max(0, nativeFold.hinge_x),
      y: Math.max(0, nativeFold.hinge_y),
      width: Math.max(0, nativeFold.hinge_w),
      height: Math.max(0, nativeFold.hinge_h),
    };
    if (nativeFold.orientation === "horizontal" && nativeFold.state === "half_opened") {
      posture = "tabletop";
    } else if (nativeFold.orientation === "vertical" && width >= 540) {
      posture = "book";
    }
  } else if (hasCssVerticalHinge && width >= 540) {
    posture = "book";
    hingeOrientation = "vertical";
    hingeRect = { x: Math.round(width / 2), y: 0, width: 0, height };
  } else if (hasCssHorizontalHinge) {
    posture = "tabletop";
    hingeOrientation = "horizontal";
    hingeRect = { x: 0, y: Math.round(height / 2), width, height: 0 };
  }

  const navMode: NavMode =
    sizeClass === "compact"
      ? "bottom"
      : sizeClass === "medium" || posture === "book"
        ? "rail"
        : "sidebar";

  const isDualPane =
    posture === "book" || (sizeClass !== "compact" && width >= 680);

  return {
    width,
    height,
    sizeClass,
    posture,
    navMode,
    isDualPane,
    hasHinge: hasNativeHinge || hasCssVerticalHinge || hasCssHorizontalHinge,
    hingeOrientation,
    hingeRect,
  };
}

function queryViewportSegments(): { horiz: number; vert: number } {
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return { horiz: 1, vert: 1 };
  }
  const horiz = window.matchMedia("(horizontal-viewport-segments: 2)").matches ? 2 : 1;
  const vert = window.matchMedia("(vertical-viewport-segments: 2)").matches ? 2 : 1;
  return { horiz, vert };
}

export function useFoldableLayout(): FoldableLayoutState {
  const [dims, setDims] = useState(() => ({
    width: typeof window !== "undefined" ? window.innerWidth : 1180,
    height: typeof window !== "undefined" ? window.innerHeight : 780,
    ...queryViewportSegments(),
  }));
  const [nativeFold, setNativeFold] = useState<NativeFoldEvent | null>(null);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const update = () => {
      setDims({
        width: window.innerWidth,
        height: window.innerHeight,
        ...queryViewportSegments(),
      });
    };
    window.addEventListener("resize", update);
    window.addEventListener("orientationchange", update);
    return () => {
      window.removeEventListener("resize", update);
      window.removeEventListener("orientationchange", update);
    };
  }, []);

  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | null = null;
    listen<NativeFoldEvent>("fold-state-changed", (event) => {
      if (active) setNativeFold(event.payload);
    })
      .then((fn) => {
        if (active) unlisten = fn;
        else fn();
      })
      .catch(() => {});
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const layout = useMemo(
    () =>
      computeFoldableLayout(
        dims.width,
        dims.height,
        nativeFold,
        dims.horiz,
        dims.vert,
      ),
    [dims, nativeFold],
  );

  useEffect(() => {
    if (typeof document === "undefined") return;
    const root = document.documentElement;
    root.dataset.layoutSize = layout.sizeClass;
    root.dataset.foldPosture = layout.posture;
    root.dataset.navMode = layout.navMode;
    root.dataset.dualPane = layout.isDualPane ? "true" : "false";
    const rect = layout.hingeRect;
    root.style.setProperty("--fold-hinge-x", `${rect ? rect.x : 0}px`);
    root.style.setProperty("--fold-hinge-y", `${rect ? rect.y : 0}px`);
    root.style.setProperty("--fold-hinge-w", `${rect ? rect.width : 0}px`);
    root.style.setProperty("--fold-hinge-h", `${rect ? rect.height : 0}px`);
  }, [layout]);

  return layout;
}
