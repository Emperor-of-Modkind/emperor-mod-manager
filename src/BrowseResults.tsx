import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  useSyncExternalStore,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  type RefObject,
} from "react";
import { useVirtualizer, type Virtualizer } from "@tanstack/react-virtual";
import {
  BROWSE_GRID_COLUMN_GAP_REM,
  BROWSE_GRID_PAD_END_REM,
  BROWSE_GRID_PAD_START_REM,
  BROWSE_GRID_ROW_GAP_REM,
  BROWSE_LIST_GAP_REM,
  browsePageCount,
  estimateGridRowHeight,
  gridCardWidth,
  gridColumnCount,
} from "./browseVirtual";
import { STAGGER_CAP, STAGGER_STEP, staggerStyle } from "./motion";

const OVERSCAN = 2;
const HYDRATE_ROWS_PER_FRAME = 2;

/**
 * Enables staggered entrance only for the initial visible batch after mount
 * (or when `staggerKey` changes), closing the window once the cascade finishes
 * so later scrolling or drag-and-drop never re-staggers rows.
 */
function useInitialStaggerBatch(
  enabled: boolean,
  active: boolean,
  staggerKey?: string,
): boolean {
  const [windowOpen, setWindowOpen] = useState(enabled);
  const prevKeyRef = useRef(staggerKey);
  useEffect(() => {
    if (prevKeyRef.current !== staggerKey) {
      prevKeyRef.current = staggerKey;
      if (enabled) setWindowOpen(true);
    }
  }, [enabled, staggerKey]);

  useEffect(() => {
    if (!enabled || !active || !windowOpen) return;
    const ms = Math.round(STAGGER_CAP * STAGGER_STEP * 1000) + 350;
    const id = window.setTimeout(() => setWindowOpen(false), ms);
    return () => window.clearTimeout(id);
  }, [active, enabled, windowOpen, staggerKey]);

  return enabled && windowOpen;
}

type ScrollRef = RefObject<HTMLElement | null>;

function readRootRem(): number {
  if (typeof document === "undefined") return 16;
  const next = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
  return next > 0 ? next : 16;
}

// Single shared root-rem store so multiple virtualizers never attach duplicate
// resize listeners or call getComputedStyle independently on every mount.
let cachedRootRem: number | null = null;
const rootRemListeners = new Set<() => void>();
let rootRemBound = false;
let rootRemRaf = 0;

function getRootRemSnapshot(): number {
  if (cachedRootRem == null) {
    cachedRootRem = readRootRem();
  }
  return cachedRootRem;
}

function handleRootRemResize() {
  if (rootRemRaf) return;
  rootRemRaf = window.requestAnimationFrame(() => {
    rootRemRaf = 0;
    const next = readRootRem();
    if (next !== cachedRootRem) {
      cachedRootRem = next;
      for (const listener of rootRemListeners) listener();
    }
  });
}

function subscribeRootRem(listener: () => void): () => void {
  rootRemListeners.add(listener);
  if (typeof window !== "undefined" && !rootRemBound) {
    rootRemBound = true;
    window.addEventListener("resize", handleRootRemResize);
  }
  return () => {
    rootRemListeners.delete(listener);
    if (rootRemListeners.size === 0 && rootRemBound && typeof window !== "undefined") {
      rootRemBound = false;
      window.removeEventListener("resize", handleRootRemResize);
      if (rootRemRaf) {
        window.cancelAnimationFrame(rootRemRaf);
        rootRemRaf = 0;
      }
      cachedRootRem = null;
    }
  };
}

function useRootRem(): number {
  return useSyncExternalStore(subscribeRootRem, getRootRemSnapshot, () => 16);
}

type ObservedRect = { width: number; height: number };

/** Never collapse a row to 0px while hidden or unlaid-out; keep cached or estimated height. */
function measureNonZeroRow(
  element: Element,
  entry: ResizeObserverEntry | undefined,
  instance: Virtualizer<HTMLElement, Element>,
): number {
  const fromEntry = entry?.borderBoxSize?.[0]?.blockSize;
  const measured =
    fromEntry != null
      ? Math.round(fromEntry)
      : element instanceof HTMLElement
        ? element.offsetHeight
        : Math.round(element.getBoundingClientRect().height);
  if (measured > 0) return measured;
  const index = instance.indexFromElement(element);
  const key = instance.options.getItemKey(index);
  return instance.itemSizeCache.get(key) ?? instance.options.estimateSize(index);
}

/** Ignore a 0×0 pane while the list is parked so hiding it cannot drop every row. */
function useStableRect(active: boolean) {
  const activeRef = useRef(active);
  activeRef.current = active;
  const lastNonZero = useRef<ObservedRect | null>(null);
  return useCallback(
    (
      instance: Virtualizer<HTMLElement, Element>,
      cb: (rect: ObservedRect) => void,
    ) => {
      const el = instance.scrollElement;
      if (!el || el === null || !("clientWidth" in el)) return () => {};
      let lastPublished: ObservedRect | null = null;
      const publish = () => {
        const rect = { width: el.clientWidth, height: el.clientHeight };
        if (
          !activeRef.current &&
          (rect.width <= 0 || rect.height <= 0) &&
          lastNonZero.current
        ) {
          return;
        }
        if (rect.width > 0 && rect.height > 0) {
          lastNonZero.current = rect;
        }
        if (
          lastPublished &&
          lastPublished.width === rect.width &&
          lastPublished.height === rect.height
        ) {
          return;
        }
        lastPublished = rect;
        cb(rect);
      };
      publish();
      const observer = new ResizeObserver(publish);
      observer.observe(el);
      return () => observer.disconnect();
    },
    [],
  );
}

function useScrollWidth(scrollRef: ScrollRef, active = true): number {
  const [width, setWidth] = useState(() => scrollRef.current?.clientWidth ?? 0);
  const activeRef = useRef(active);
  activeRef.current = active;
  useLayoutEffect(() => {
    if (!active) return;
    let frame = 0;
    let observer: ResizeObserver | null = null;
    let stopped = false;

    const publish = (next: number) => {
      if (!activeRef.current || next <= 0) return;
      setWidth((prev) => (prev === next ? prev : next));
    };

    const watch = () => {
      if (stopped) return;
      const el = scrollRef.current;
      if (!el) {
        frame = requestAnimationFrame(watch);
        return;
      }
      if (!observer) {
        observer = new ResizeObserver(() => publish(el.clientWidth));
        observer.observe(el);
      }
      publish(el.clientWidth);
    };

    watch();
    return () => {
      stopped = true;
      cancelAnimationFrame(frame);
      observer?.disconnect();
    };
  }, [active, scrollRef]);
  return width;
}

type BrowseVirtualGridProps<T> = {
  items: readonly T[];
  scrollRef: ScrollRef;
  minCardPx: number;
  getKey: (item: T) => string;
  renderItem: (item: T, index: number) => ReactNode;
  renderSkeletonItem?: (index: number) => ReactNode;
  fixedRowHeight?: boolean;
  onPageSize?: (count: number) => void;
  /** Parked pages leave the scroll flow. Keep the last measurements until shown again. */
  active?: boolean;
  /**
   * The grid sits below other content in the scrollport (library posters).
   * Browse results are the top of their own pane and must not use this: it
   * shortens the box and clips the last row.
   */
  trackScrollMargin?: boolean;
  /** Stagger the initial visible batch of cards in 1-by-1 on mount. */
  staggerEnter?: boolean;
  /** Changing this key re-arms the initial stagger batch. */
  staggerKey?: string;
};

function BrowseVirtualGridInner<T,>({
  items,
  scrollRef,
  minCardPx,
  getKey,
  renderItem,
  renderSkeletonItem,
  fixedRowHeight = false,
  onPageSize,
  active = true,
  trackScrollMargin = false,
  staggerEnter = false,
  staggerKey,
}: BrowseVirtualGridProps<T>) {
  const staggerActive = useInitialStaggerBatch(staggerEnter, active, staggerKey);
  const width = useScrollWidth(scrollRef, active);
  const rem = useRootRem();
  const columnGap = BROWSE_GRID_COLUMN_GAP_REM * rem;
  const columns = gridColumnCount(width, minCardPx, columnGap);
  const cardWidth = gridCardWidth(width, columns, columnGap);
  const rowGap = BROWSE_GRID_ROW_GAP_REM * rem;
  const gridRef = useRef<HTMLDivElement | null>(null);
  const [scrollMargin, setScrollMargin] = useState(0);
  const activeRef = useRef(active);
  activeRef.current = active;
  const onPageSizeRef = useRef(onPageSize);
  onPageSizeRef.current = onPageSize;
  const observeElementRect = useStableRect(active);
  const lastReportedPageSizeRef = useRef<number | null>(null);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!active || !el || width <= 0 || !onPageSizeRef.current) return;
    const report = () => {
      if (!activeRef.current || el.clientHeight <= 0) return;
      const count = browsePageCount(
        columns,
        el.clientHeight,
        estimateGridRowHeight(cardWidth, rem),
        rowGap,
      );
      if (lastReportedPageSizeRef.current === count) return;
      lastReportedPageSizeRef.current = count;
      onPageSizeRef.current?.(count);
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, [active, cardWidth, columns, rem, rowGap, scrollRef, width]);

  useLayoutEffect(() => {
    if (!trackScrollMargin || !active) return;
    const scroll = scrollRef.current;
    const grid = gridRef.current;
    if (!scroll || !grid) return;
    const publish = () => {
      if (!activeRef.current) return;
      const next =
        grid.getBoundingClientRect().top -
        scroll.getBoundingClientRect().top +
        scroll.scrollTop;
      setScrollMargin((prev) => (Math.abs(prev - next) < 0.5 ? prev : next));
    };
    publish();
    const observer = new ResizeObserver(publish);
    observer.observe(scroll);
    if (grid.parentElement) observer.observe(grid.parentElement);
    return () => observer.disconnect();
  }, [active, items.length, scrollRef, trackScrollMargin]);

  // Width 0 is one full-bleed column. Wait until the pane has been measured.
  const rowCount = width > 0 ? Math.ceil(items.length / columns) : 0;
  const getKeyRef = useRef(getKey);
  getKeyRef.current = getKey;
  const getRowKey = useCallback(
    (row: number) => {
      const item = items[row * columns];
      const key = item ? getKeyRef.current(item) : String(row);
      return `${columns}:${key}`;
    },
    [columns, items],
  );

  const virtualizer = useVirtualizer({
    count: rowCount,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => estimateGridRowHeight(cardWidth, rem),
    overscan: columns >= 6 ? 1 : OVERSCAN,
    gap: rowGap,
    paddingStart: BROWSE_GRID_PAD_START_REM * rem,
    paddingEnd: BROWSE_GRID_PAD_END_REM * rem,
    scrollMargin,
    observeElementRect,
    measureElement: fixedRowHeight ? undefined : measureNonZeroRow,
    initialOffset: () => scrollRef.current?.scrollTop ?? 0,
    getItemKey: getRowKey,
  });

  const prevLayoutRef = useRef<{ width: number; columns: number; rem: number } | null>(null);
  useLayoutEffect(() => {
    if (!active || width <= 0) return;
    const prev = prevLayoutRef.current;
    prevLayoutRef.current = { width, columns, rem };
    if (prev && (prev.width !== width || prev.columns !== columns || prev.rem !== rem)) {
      virtualizer.measure();
    }
  }, [active, columns, rem, virtualizer, width]);
  const margin = virtualizer.options.scrollMargin;
  const virtualRows = virtualizer.getVirtualItems();
  const [hydratedKeys, setHydratedKeys] = useState<Set<string>>(() => new Set());
  const activeWindowSignature = renderSkeletonItem
    ? virtualRows.map((row) => String(row.key)).join("|")
    : "";

  useEffect(() => {
    if (!renderSkeletonItem || !active || width <= 0) return;
    const windowKeys = activeWindowSignature ? activeWindowSignature.split("|") : [];
    const windowSet = new Set(windowKeys);

    // Prune rows that left the virtual window so scrolling back re-skeletonizes them.
    let needsPrune = false;
    for (const k of hydratedKeys) {
      if (!windowSet.has(k)) {
        needsPrune = true;
        break;
      }
    }
    const missing = windowKeys.filter((k) => !hydratedKeys.has(k));

    if (!needsPrune && missing.length === 0) return;

    // On initial mount (nothing in current window hydrated yet), hydrate the first batch immediately.
    const activeHydratedCount = windowKeys.length - missing.length;
    if (activeHydratedCount === 0 && missing.length > 0) {
      const next = new Set<string>();
      for (const k of missing.slice(0, HYDRATE_ROWS_PER_FRAME)) {
        next.add(k);
      }
      setHydratedKeys(next);
      return;
    }

    let cancelled = false;
    const id = window.requestAnimationFrame(() => {
      if (cancelled) return;
      setHydratedKeys((prev) => {
        const next = new Set<string>();
        for (const k of prev) {
          if (windowSet.has(k)) next.add(k);
        }
        let added = 0;
        for (const k of windowKeys) {
          if (!next.has(k)) {
            next.add(k);
            added += 1;
            if (added >= HYDRATE_ROWS_PER_FRAME) break;
          }
        }
        if (next.size === prev.size && added === 0) return prev;
        return next;
      });
    });
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(id);
    };
  }, [active, activeWindowSignature, hydratedKeys, renderSkeletonItem, width]);

  return (
    <div
      ref={gridRef}
      className="browse-virtual"
      style={{ height: virtualizer.getTotalSize() }}
    >
      {virtualRows.map((row) => {
        const start = row.index * columns;
        const slice = items.slice(start, start + columns);
        const rowKeyStr = String(row.key);
        const isHydrated = !renderSkeletonItem || hydratedKeys.has(rowKeyStr);
        return (
          <div
            key={row.key}
            data-index={row.index}
            ref={active && !fixedRowHeight ? virtualizer.measureElement : undefined}
            className="browse-virtual-row browse-virtual-grid-row"
            style={{
              top: row.start - margin,
              gridTemplateColumns: `repeat(${columns}, minmax(0, 1fr))`,
              columnGap,
            }}
          >
            {slice.map((item, offset) => {
              const itemIndex = start + offset;
              const style = isHydrated ? staggerStyle(itemIndex, staggerActive) : undefined;
              return (
                <div
                  key={getKey(item)}
                  className={style ? "browse-virtual-cell stagger-item" : "browse-virtual-cell"}
                  style={style}
                >
                  {isHydrated
                    ? renderItem(item, itemIndex)
                    : renderSkeletonItem(itemIndex)}
                </div>
              );
            })}
          </div>
        );
      })}
    </div>
  );
}

export const BrowseVirtualGrid = memo(BrowseVirtualGridInner) as typeof BrowseVirtualGridInner;

type BrowseVirtualListProps<T> = {
  items: readonly T[];
  scrollRef: ScrollRef;
  getKey: (item: T) => string;
  renderItem: (item: T, index: number) => ReactNode;
  rowClassName: string | ((item: T, index: number) => string);
  estimateSize: number;
  onPageSize?: (count: number) => void;
  /** Extra classes on the list, after `list browse-virtual`. */
  listClassName?: string;
  listRef?: RefObject<HTMLUListElement | null>;
  onRowPointerDown?: (item: T, event: ReactPointerEvent<HTMLLIElement>) => void;
  /** Parked pages leave the scroll flow. Keep the last in-flow offset until shown again. */
  active?: boolean;
  /** Stagger the initial visible batch of rows in 1-by-1 on mount. */
  staggerEnter?: boolean;
  /** Changing this key re-arms the initial stagger batch. */
  staggerKey?: string;
};

function BrowseVirtualListInner<T,>({
  items,
  scrollRef,
  getKey,
  renderItem,
  rowClassName,
  estimateSize,
  onPageSize,
  listClassName,
  listRef,
  onRowPointerDown,
  active = true,
  staggerEnter = false,
  staggerKey,
}: BrowseVirtualListProps<T>) {
  const staggerActive = useInitialStaggerBatch(staggerEnter, active, staggerKey);
  const width = useScrollWidth(scrollRef, active);
  const rem = useRootRem();
  const rowGap = BROWSE_LIST_GAP_REM * rem;
  const observeElementRect = useStableRect(active);
  const innerListRef = useRef<HTMLUListElement | null>(null);
  const [scrollMargin, setScrollMargin] = useState(0);
  const activeRef = useRef(active);
  activeRef.current = active;
  const onPageSizeRef = useRef(onPageSize);
  onPageSizeRef.current = onPageSize;

  const setList = useCallback(
    (el: HTMLUListElement | null) => {
      innerListRef.current = el;
      if (listRef) listRef.current = el;
    },
    [listRef],
  );

  useLayoutEffect(() => {
    if (!active) return;
    const scroll = scrollRef.current;
    const list = innerListRef.current;
    if (!scroll || !list) return;
    const publish = () => {
      if (!activeRef.current) return;
      const next =
        list.getBoundingClientRect().top -
        scroll.getBoundingClientRect().top +
        scroll.scrollTop;
      setScrollMargin((prev) => (Math.abs(prev - next) < 0.5 ? prev : next));
    };
    publish();
    const observer = new ResizeObserver(publish);
    observer.observe(scroll);
    if (list.parentElement) observer.observe(list.parentElement);
    return () => observer.disconnect();
  }, [active, items.length, scrollRef]);

  useLayoutEffect(() => {
    const el = scrollRef.current;
    if (!active || !el || !onPageSizeRef.current) return;
    const report = () => {
      if (!activeRef.current || el.clientHeight <= 0) return;
      onPageSizeRef.current?.(browsePageCount(1, el.clientHeight, estimateSize, rowGap));
    };
    report();
    const observer = new ResizeObserver(report);
    observer.observe(el);
    return () => observer.disconnect();
  }, [active, estimateSize, rowGap, scrollRef]);

  const getKeyRef = useRef(getKey);
  getKeyRef.current = getKey;
  const getRowKey = useCallback(
    (index: number) => {
      const item = items[index];
      const key = item ? getKeyRef.current(item) : String(index);
      return `${estimateSize}:${key}`;
    },
    [estimateSize, items],
  );

  const virtualizer = useVirtualizer({
    count: items.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => estimateSize,
    overscan: OVERSCAN,
    gap: rowGap,
    scrollMargin,
    observeElementRect,
    measureElement: measureNonZeroRow,
    initialOffset: () => scrollRef.current?.scrollTop ?? 0,
    getItemKey: getRowKey,
  });

  const prevLayoutRef = useRef<{ width: number; estimateSize: number; rem: number } | null>(null);
  useLayoutEffect(() => {
    if (!active || width <= 0) return;
    const prev = prevLayoutRef.current;
    prevLayoutRef.current = { width, estimateSize, rem };
    if (prev && (prev.width !== width || prev.estimateSize !== estimateSize || prev.rem !== rem)) {
      virtualizer.measure();
    }
  }, [active, estimateSize, rem, virtualizer, width]);
  const margin = virtualizer.options.scrollMargin;

  return (
    <ul
      ref={setList}
      className={["list browse-virtual", listClassName].filter(Boolean).join(" ")}
      style={{ height: virtualizer.getTotalSize() }}
    >
      {virtualizer.getVirtualItems().map((row) => {
        const item = items[row.index];
        if (!item) return null;
        const extra =
          typeof rowClassName === "function" ? rowClassName(item, row.index) : rowClassName;
        const rowStagger = staggerStyle(row.index, staggerActive);
        return (
          <li
            key={row.key}
            data-index={row.index}
            ref={active ? virtualizer.measureElement : undefined}
            className={`browse-virtual-row ${extra}${rowStagger ? " is-stagger" : ""}`}
            style={{
              transform: `translateY(${row.start - margin}px)`,
              ...rowStagger,
            }}
            onPointerDown={
              onRowPointerDown ? (event) => onRowPointerDown(item, event) : undefined
            }
          >
            {renderItem(item, row.index)}
          </li>
        );
      })}
    </ul>
  );
}

export const BrowseVirtualList = memo(BrowseVirtualListInner) as typeof BrowseVirtualListInner;
