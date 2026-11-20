import { useLayoutEffect, useRef, useState, type CSSProperties } from "react";
import { BROWSE_GRID_COLUMN_GAP_REM, gridColumnCount } from "./browseVirtual";

export const STAGED_MODS_SKELETON_ROWS = 8;

type SkeletonProps = {
  className?: string;
};

function Skeleton({ className }: SkeletonProps) {
  return (
    <span className={["skeleton", className].filter(Boolean).join(" ")} aria-hidden="true" />
  );
}

function LoadingLabel({ children }: { children: string }) {
  return <span className="visually-hidden">{children}</span>;
}

function readRootRem(): number {
  if (typeof document === "undefined") return 16;
  const next = Number.parseFloat(getComputedStyle(document.documentElement).fontSize);
  return next > 0 ? next : 16;
}

/** Staged-mod rows: checkbox, poster, title, source line, and action blocks. */
export function StagedModsSkeleton({ rows = STAGED_MODS_SKELETON_ROWS }: { rows?: number }) {
  return (
    <div className="skeleton-region" aria-busy="true">
      <LoadingLabel>Loading staged mods</LoadingLabel>
      <ul className="list mods">
        {Array.from({ length: rows }, (_, index) => (
          <li key={index} className="list-row-poster">
            <Skeleton className="skeleton-check" />
            <div className="mod-identity">
              <div className="list-poster skeleton" aria-hidden="true" />
              <div className="mod-identity-text">
                <Skeleton className="skeleton-line skeleton-line-title" />
                <Skeleton className="skeleton-line skeleton-line-meta" />
              </div>
            </div>
            <div className="actions">
              <Skeleton className="skeleton-action" />
              <Skeleton className="skeleton-action" />
              <Skeleton className="skeleton-action" />
            </div>
          </li>
        ))}
      </ul>
    </div>
  );
}

type BrowseGridSkeletonProps = {
  /** Exact card count. When omitted, `rows` is multiplied by the measured column count. */
  count?: number;
  rows?: number;
  minCardPx?: number;
};

/** Browse cards in the same auto-fill grid as the results pane. */
export function BrowseGridSkeleton({
  count,
  rows = 1,
  minCardPx = 160,
}: BrowseGridSkeletonProps) {
  const ref = useRef<HTMLDivElement>(null);
  const [width, setWidth] = useState(0);
  const [rem, setRem] = useState(16);
  useLayoutEffect(() => {
    if (count != null) return;
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const publish = () => {
      setWidth(el.clientWidth);
      setRem(readRootRem());
    };
    publish();
    const observer = new ResizeObserver(publish);
    observer.observe(el);
    return () => observer.disconnect();
  }, [count]);
  const columns = gridColumnCount(width, minCardPx, BROWSE_GRID_COLUMN_GAP_REM * rem);
  const cardCount = count ?? Math.max(columns, columns * rows);
  return (
    <div className="skeleton-region" aria-busy="true">
      <LoadingLabel>Searching</LoadingLabel>
      <div
        ref={ref}
        className="media-grid browse-grid"
        style={{ "--browse-card-min": `${minCardPx}px` } as CSSProperties}
      >
        {Array.from({ length: cardCount }, (_, index) => (
          <div key={index} className="media-card">
            <div className="media-cover-stack">
              <div className="media-cover skeleton" aria-hidden="true" />
            </div>
            <Skeleton className="skeleton-line skeleton-line-title" />
            <Skeleton className="skeleton-line skeleton-line-meta" />
          </div>
        ))}
      </div>
    </div>
  );
}

type BrowseListSkeletonProps = {
  count: number;
  posters?: boolean;
};

/** Compact browse rows, or poster rows when the list density is posters. */
export function BrowseListSkeleton({ count, posters = false }: BrowseListSkeletonProps) {
  return (
    <div className="skeleton-region" aria-busy="true">
      <LoadingLabel>Searching</LoadingLabel>
      <ul className="list">
        {Array.from({ length: count }, (_, index) => (
          <li
            key={index}
            className={
              posters ? "list-row-clickable list-row-poster" : "list-row-clickable"
            }
          >
            {posters ? <div className="list-poster skeleton" aria-hidden="true" /> : null}
            <div className="list-row-main">
              <Skeleton className="skeleton-line skeleton-line-title" />
              <Skeleton className="skeleton-line skeleton-line-meta" />
              {posters ? <Skeleton className="skeleton-line skeleton-line-detail" /> : null}
            </div>
            <Skeleton className="skeleton-action skeleton-action-wide" />
          </li>
        ))}
      </ul>
    </div>
  );
}

type BrowseResultsSkeletonProps = {
  view: "grid" | "list";
  posters?: boolean;
  count?: number;
  rows?: number;
  minCardPx?: number;
};

/** Grid or list placeholder for a browse results load. */
export function BrowseResultsSkeleton({
  view,
  posters = false,
  count,
  rows,
  minCardPx = 160,
}: BrowseResultsSkeletonProps) {
  if (view === "list") {
    return <BrowseListSkeleton count={count ?? rows ?? 8} posters={posters} />;
  }
  return <BrowseGridSkeleton count={count} rows={rows} minCardPx={minCardPx} />;
}
