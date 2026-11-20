/** Column gap on `.media-grid`. */
export const BROWSE_GRID_COLUMN_GAP_REM = 0.85;
/** Row gap on `.media-grid`. */
export const BROWSE_GRID_ROW_GAP_REM = 1;
/** Top margin on `.media-grid`. */
export const BROWSE_GRID_PAD_START_REM = 0.5;
/** Bottom margin on `.media-grid`. */
export const BROWSE_GRID_PAD_END_REM = 1;
/** Row gap on `.list`. */
export const BROWSE_LIST_GAP_REM = 0.45;

/**
 * Column count for `repeat(auto-fill, minmax(minPx, 1fr))`.
 * At least one column, even when the container is narrower than `minPx`.
 */
export function gridColumnCount(width: number, minPx: number, gapPx: number): number {
  if (!(width > 0) || !(minPx > 0)) return 1;
  const gap = gapPx > 0 ? gapPx : 0;
  return Math.max(1, Math.floor((width + gap) / (minPx + gap)));
}

/** Width of one track once `gridColumnCount` has chosen how many fit. */
export function gridCardWidth(width: number, columns: number, gapPx: number): number {
  if (!(width > 0) || columns <= 1) return Math.max(width, 0);
  const gap = gapPx > 0 ? gapPx : 0;
  return Math.max(0, (width - gap * (columns - 1)) / columns);
}

/**
 * Row height for a browse card: 2/3 cover, two title lines, and one meta line.
 * Measured rows replace this when a title is only one line.
 */
export function estimateGridRowHeight(cardWidth: number, rootPx: number): number {
  const rem = rootPx > 0 ? rootPx : 16;
  if (!(cardWidth > 0)) return Math.round(16 * rem);
  const cover = cardWidth * 1.5;
  const title = 2 * 0.95 * 1.25 * rem;
  const meta = 0.78 * 1.3 * rem;
  const gaps = 2 * 0.45 * rem;
  const metaPull = 0.15 * rem;
  return Math.round(cover + title + meta + gaps - metaPull);
}

/** Starting list-row height before measurement. Posters reserve the 7.5rem cover. */
export function estimateListRowHeight(posters: boolean, rootPx = 16): number {
  const rem = rootPx > 0 ? rootPx : 16;
  return Math.round((posters ? 14 : 7) * rem);
}

/** Staged-mod row: 4.5rem poster plus the list's vertical padding. */
export function estimateStagedModRowHeight(rootPx = 16): number {
  const rem = rootPx > 0 ? rootPx : 16;
  return Math.round(5.9 * rem);
}

/** Used before the results pane has a size. Matches the catalog default. */
export const BROWSE_PAGE_FALLBACK = 24;
/** Keeps a very tall or dense small-card window from asking for an unbounded page. */
const BROWSE_PAGE_CAP = 48;

/**
 * Hits that fill the scrollport, plus one row past the fold.
 * Returns {@link BROWSE_PAGE_FALLBACK} until the pane has been measured.
 */
export function browsePageCount(
  columns: number,
  viewportHeight: number,
  rowHeight: number,
  rowGap: number,
): number {
  if (!(columns > 0) || !(viewportHeight > 0) || !(rowHeight > 0)) {
    return BROWSE_PAGE_FALLBACK;
  }
  const gap = rowGap > 0 ? rowGap : 0;
  const stride = rowHeight + gap;
  const visibleRows = Math.max(1, Math.ceil((viewportHeight + gap) / stride));
  return Math.min(BROWSE_PAGE_CAP, columns * (visibleRows + 1));
}

export type RectBox = {
  top: number;
  bottom: number;
  left: number;
  right: number;
};

export type SizeBox = {
  width: number;
  height: number;
};

/**
 * Sentinel Y position in the scroll container's content coordinates.
 * Unlike viewport-relative `getBoundingClientRect().top`, this does not shift
 * when the user scrolls while an append request is in flight.
 */
export function sentinelContentTop(
  sentinelTop: number,
  rootTop: number,
  scrollTop: number,
): number {
  return sentinelTop - rootTop + scrollTop;
}

/**
 * True when `sentinelRect` intersects the visible portion of the scroll
 * container, clamped against the browser viewport so an unconstrained scroll
 * container can never treat off-screen rows as visible and loop forever.
 */
export function isSentinelInScrollView(
  rootRect: Pick<RectBox, "top" | "left">,
  rootClient: SizeBox,
  sentinelRect: RectBox,
  viewport: SizeBox,
  margin = 0,
): boolean {
  if (
    !(rootClient.height > 0) ||
    !(rootClient.width > 0) ||
    !(viewport.height > 0) ||
    !(viewport.width > 0)
  ) {
    return false;
  }
  const viewTop = Math.max(rootRect.top, 0);
  const viewBottom = Math.min(rootRect.top + rootClient.height, viewport.height);
  const viewLeft = Math.max(rootRect.left, 0);
  const viewRight = Math.min(rootRect.left + rootClient.width, viewport.width);
  if (viewBottom <= viewTop || viewRight <= viewLeft) return false;
  return (
    sentinelRect.top < viewBottom + margin &&
    sentinelRect.bottom > viewTop - margin &&
    sentinelRect.left < viewRight + margin &&
    sentinelRect.right > viewLeft - margin
  );
}
