import {
  memo,
  useCallback,
  useDeferredValue,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCheck, faCopy, faFilter, faPen } from "@fortawesome/free-solid-svg-icons";
import { api } from "./api";
import { BrowseVirtualGrid } from "./BrowseResults";
import { MediaCard } from "./MediaCard";
import { Collapse, Segment, SegmentTab } from "./motion";
import { ResultsReveal } from "./reveal";
import { formatCollectionGameVersions } from "./savedCollectionGroups";
import { BrowseGridSkeleton } from "./Skeleton";
import { SourceMark, sourceMarkName } from "./SourceMark";
import type {
  CatalogHit,
  CatalogSource,
  ManagedGame,
  ShareModEntry,
  ShareModMetadataHit,
} from "./types";

const SHARE_MOD_GRID_MIN_PX = 160;
const shareModMetaCache = new Map<string, ShareModMetadataHit>();
const shareModMetaPending = new Set<string>();

export type ShareModSort = "order" | "nameAsc" | "nameDesc";

export function shareModMetadataKey(m: ShareModEntry): string | null {
  if (m.s === "nexus") {
    const domain = m.domain?.trim().toLowerCase();
    if (!domain || !m.mod_id) return null;
    return `nexus:${domain}:${m.mod_id}`;
  }
  if (m.s === "thunderstore") {
    const community = m.community?.trim().toLowerCase();
    const namespace = m.namespace?.trim().toLowerCase();
    const name = m.name?.trim().toLowerCase();
    if (!community || !namespace || !name) return null;
    return `thunderstore:${community}:${namespace}:${name}`;
  }
  if (m.s === "modio") {
    if (!m.game_id || !m.mod_id) return null;
    return `modio:${m.game_id}:${m.mod_id}`;
  }
  return null;
}

export function catalogHitFromShareMod(
  m: ShareModEntry,
  meta?: ShareModMetadataHit | null,
): CatalogHit {
  const label = shareModLabel(m);
  const id =
    shareModMetadataKey(m) ??
    (m.s === "nexus"
      ? `nexus:${m.domain ?? ""}:${m.mod_id ?? 0}`
      : m.s === "thunderstore"
        ? `thunderstore:${m.community ?? ""}:${m.namespace ?? ""}:${m.name ?? ""}`
        : `modio:${m.game_id ?? 0}:${m.mod_id ?? 0}`);
  return {
    source: m.s,
    id,
    name: label,
    summary: meta?.summary ?? null,
    picture_url: meta?.picture_url ?? null,
    author: meta?.author ?? (m.s === "thunderstore" ? m.namespace ?? null : null),
    downloads: meta?.downloads ?? null,
    endorsements: meta?.endorsements ?? null,
    category: null,
    tags: [],
    mod_id: m.s === "nexus" ? (m.mod_id ?? null) : null,
    domain_name: m.s === "nexus" ? (m.domain ?? null) : null,
    community: m.s === "thunderstore" ? (m.community ?? null) : null,
    namespace: m.s === "thunderstore" ? (m.namespace ?? null) : null,
    package_name: m.s === "thunderstore" ? (m.name ?? null) : null,
    full_name:
      m.s === "thunderstore" && m.namespace && m.name
        ? `${m.namespace}-${m.name}`
        : null,
    package_url: null,
    rating_score: null,
    latest_version: m.version ?? null,
    modio_game_id: m.s === "modio" ? (m.game_id ?? null) : null,
    modio_mod_id: m.s === "modio" ? (m.mod_id ?? null) : null,
    profile_url: null,
  };
}

export function filterAndSortShareMods(
  mods: readonly ShareModEntry[],
  opts: {
    query: string;
    source: "all" | CatalogSource;
    sort: ShareModSort;
  },
): Array<{ mod: ShareModEntry; index: number }> {
  const q = opts.query.trim().toLowerCase();
  const rows: Array<{ mod: ShareModEntry; index: number }> = [];
  for (let index = 0; index < mods.length; index++) {
    const mod = mods[index];
    if (opts.source !== "all" && mod.s !== opts.source) continue;
    if (q) {
      const label = shareModLabel(mod).toLowerCase();
      const haystack = [
        label,
        mod.version ?? "",
        mod.domain ?? "",
        mod.community ?? "",
        mod.namespace ?? "",
        mod.name ?? "",
        mod.mod_id != null ? String(mod.mod_id) : "",
      ]
        .join(" ")
        .toLowerCase();
      if (!haystack.includes(q)) continue;
    }
    rows.push({ mod, index });
  }
  if (opts.sort === "nameAsc") {
    rows.sort((a, b) => shareModLabel(a.mod).localeCompare(shareModLabel(b.mod)));
  } else if (opts.sort === "nameDesc") {
    rows.sort((a, b) => shareModLabel(b.mod).localeCompare(shareModLabel(a.mod)));
  }
  return rows;
}

export function formatCollectionDate(iso: string | null | undefined): string | null {
  if (!iso) return null;
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return null;
    return d.toLocaleDateString();
  } catch {
    return null;
  }
}

export function shareModLabel(m: ShareModEntry): string {
  return (
    m.display_name ||
    m.name ||
    (m.s === "nexus"
      ? `Nexus ${m.mod_id}/${m.file_id}`
      : m.s === "thunderstore"
        ? `${m.namespace}-${m.name}`
        : `mod.io ${m.mod_id}`) ||
    "Mod"
  );
}

export function CollectionPlaceholderIcon() {
  return (
    <div className="media-placeholder" aria-hidden>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="3" y="6" width="18" height="12" rx="2" />
        <path d="M8 12h.01M12 10v4M16 12h.01" strokeLinecap="round" />
      </svg>
    </div>
  );
}

export const ShareModList = memo(function ShareModList({
  mods,
  rows: prefilteredRows,
  scrollRef,
  active = true,
  onOpenMod,
}: {
  mods: ShareModEntry[];
  rows?: Array<{ mod: ShareModEntry; index: number }>;
  scrollRef?: RefObject<HTMLElement | null>;
  active?: boolean;
  onOpenMod?: (hit: CatalogHit) => void;
}) {
  const [, setMetaTick] = useState(0);
  const mountedRef = useRef(true);
  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    if (!active || mods.length === 0) return;
    const missing: ShareModEntry[] = [];
    for (const m of mods) {
      const key = shareModMetadataKey(m);
      if (!key || shareModMetaCache.has(key) || shareModMetaPending.has(key)) continue;
      shareModMetaPending.add(key);
      missing.push(m);
    }
    if (missing.length === 0) return;
    let cancelled = false;
    void api
      .resolveShareModMetadata(missing)
      .then((hits) => {
        for (const hit of hits) {
          shareModMetaCache.set(hit.key, hit);
        }
        if (!cancelled && mountedRef.current && hits.length > 0) {
          setMetaTick((t) => t + 1);
        }
      })
      .catch(() => {
        // Keep placeholder cards when offline or in unit tests without Tauri IPC.
      })
      .finally(() => {
        for (const m of missing) {
          const key = shareModMetadataKey(m);
          if (key) shareModMetaPending.delete(key);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [active, mods]);

  const rows = useMemo(
    () => prefilteredRows ?? mods.map((mod, index) => ({ mod, index })),
    [mods, prefilteredRows],
  );
  const getKey = useCallback(
    (row: { mod: ShareModEntry; index: number }) =>
      `${shareModMetadataKey(row.mod) ?? row.mod.s}-${row.index}`,
    [],
  );
  const renderCard = useCallback(
    (row: { mod: ShareModEntry; index: number }, plain: boolean) => {
      const m = row.mod;
      const key = shareModMetadataKey(m);
      const meta = key ? shareModMetaCache.get(key) : undefined;
      const detailParts: string[] = [];
      if (m.version) {
        detailParts.push(`v${m.version}`);
      }
      if (m.s === "nexus" && m.mod_id != null) {
        detailParts.push(`${m.domain} ${m.mod_id}/${m.file_id}`);
      } else if (m.s === "thunderstore") {
        detailParts.push(`${m.community}/${m.namespace}-${m.name}`);
      } else if (m.s === "modio") {
        detailParts.push(`game ${m.game_id} mod ${m.mod_id}`);
      }
      const handleOpen = onOpenMod
        ? () => onOpenMod(catalogHitFromShareMod(m, meta))
        : undefined;

      return (
        <MediaCard
          plain={plain}
          title={shareModLabel(m)}
          imageSrc={meta?.picture_url ?? null}
          badge={detailParts.length > 0 ? detailParts.join(" · ") : undefined}
          coverMarks={[{ source: m.s, label: sourceMarkName(m.s) }]}
          tags={m.fomod ? ["FOMOD Config"] : undefined}
          onClick={handleOpen}
        />
      );
    },
    [onOpenMod],
  );
  const renderGridItem = useCallback(
    (row: { mod: ShareModEntry; index: number }) => renderCard(row, true),
    [renderCard],
  );
  const renderGridSkeletonItem = useCallback(
    () => (
      <div className="media-card" aria-hidden="true">
        <div className="media-cover-stack">
          <div className="media-cover skeleton" />
        </div>
        <span className="skeleton skeleton-line skeleton-line-title" />
        <span className="skeleton skeleton-line skeleton-line-meta" />
      </div>
    ),
    [],
  );

  if (mods.length === 0) {
    return (
      <ul className="list">
        <li className="empty">No mods in this share.</li>
      </ul>
    );
  }

  if (rows.length === 0) {
    return <p className="note">No contained mods match the current filters.</p>;
  }

  if (!scrollRef) {
    return (
      <div
        className="media-grid browse-grid"
        style={{ "--browse-card-min": `${SHARE_MOD_GRID_MIN_PX}px` } as React.CSSProperties}
      >
        {rows.map((row) => (
          <div key={getKey(row)}>{renderCard(row, true)}</div>
        ))}
      </div>
    );
  }

  return (
    <BrowseVirtualGrid
      items={rows}
      scrollRef={scrollRef}
      active={active}
      staggerEnter
      minCardPx={SHARE_MOD_GRID_MIN_PX}
      fixedRowHeight
      trackScrollMargin
      getKey={getKey}
      renderItem={renderGridItem}
      renderSkeletonItem={renderGridSkeletonItem}
    />
  );
});

export function ContainedModsSection({
  mods,
  showMods,
  skeletonCount,
  scrollRef,
  active = true,
  onOpenMod,
  renderLayout,
}: {
  mods: ShareModEntry[];
  showMods: boolean;
  skeletonCount?: number;
  scrollRef?: RefObject<HTMLElement | null>;
  active?: boolean;
  onOpenMod?: (hit: CatalogHit) => void;
  renderLayout?: (controls: ReactNode, list: ReactNode) => ReactNode;
}) {
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [sourceFilter, setSourceFilter] = useState<"all" | CatalogSource>("all");
  const [sort, setSort] = useState<ShareModSort>("order");
  const deferredQuery = useDeferredValue(query);
  const segmentId = useId();

  const filtersActive =
    query.trim() !== "" || sourceFilter !== "all" || sort !== "order";

  const filteredRows = useMemo(
    () =>
      filterAndSortShareMods(mods, {
        query: deferredQuery,
        source: sourceFilter,
        sort,
      }),
    [deferredQuery, mods, sort, sourceFilter],
  );

  const controls = (
    <>
      <div className="detail-section-header">
        <h2>Contained mods</h2>
        <div className="mods-toolbar-actions-right">
          <button
            type="button"
            className={
              filtersActive
                ? "mods-toolbar-icon-btn mods-filters-toggle active"
                : "mods-toolbar-icon-btn mods-filters-toggle"
            }
            aria-label="Filters"
            aria-expanded={filtersOpen}
            aria-pressed={filtersActive}
            onClick={() => setFiltersOpen((open) => !open)}
          >
            <FontAwesomeIcon icon={faFilter} />
          </button>
        </div>
      </div>
      <Collapse show={filtersOpen}>
        <div className="filters">
          <div className="row">
            <input
              placeholder="Search contained mods…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              aria-label="Search contained mods"
            />
          </div>
          <label>
            Sort
            <select
              value={sort}
              onChange={(e) => setSort(e.target.value as ShareModSort)}
            >
              <option value="order">Collection order</option>
              <option value="nameAsc">Name (A–Z)</option>
              <option value="nameDesc">Name (Z–A)</option>
            </select>
          </label>
          <Segment className="source-filter">
            {(
              [
                ["all", "All"] as const,
                ["nexus", "Nexus"] as const,
                ["thunderstore", "Thunderstore"] as const,
                ["modio", "mod.io"] as const,
              ] as const
            ).map(([id, label]) => (
              <SegmentTab
                key={id}
                layoutId={`${segmentId}-source`}
                active={sourceFilter === id}
                aria-label={label}
                title={label}
                onClick={() => setSourceFilter(id)}
              >
                {id === "all" ? label : <SourceMark source={id} decorative />}
              </SegmentTab>
            ))}
          </Segment>
          {filtersActive && (
            <button
              type="button"
              className="filters-clear"
              onClick={() => {
                setQuery("");
                setSourceFilter("all");
                setSort("order");
              }}
            >
              Clear filters
            </button>
          )}
        </div>
      </Collapse>
      {filtersActive && showMods && mods.length > 0 && (
        <p className="note">
          Showing {filteredRows.length} of {mods.length}
        </p>
      )}
    </>
  );

  const list = (
      <ResultsReveal
        show={showMods}
        skeleton={
          <BrowseGridSkeleton
            count={Math.min(12, skeletonCount ?? 8)}
            minCardPx={SHARE_MOD_GRID_MIN_PX}
          />
        }
      >
        {showMods ? (
          <ShareModList
            mods={mods}
            rows={filteredRows}
            scrollRef={scrollRef}
            active={active}
            onOpenMod={onOpenMod}
          />
        ) : null}
      </ResultsReveal>
  );

  if (renderLayout) {
    return <>{renderLayout(controls, list)}</>;
  }

  return (
    <>
      {controls}
      {list}
    </>
  );
}

export type CollectionStatsSummary = {
  mod_count: number;
  nexus_count: number;
  thunderstore_count: number;
  modio_count: number;
  game_versions?: string[] | null;
  created_at?: string | null;
  updated_at?: string | null;
};

export function CollectionStats({ entry }: { entry: CollectionStatsSummary }) {
  const builtFor = formatCollectionGameVersions(entry.game_versions);
  const created = formatCollectionDate(entry.created_at);
  const updated = !created ? formatCollectionDate(entry.updated_at) : null;
  return (
    <div className="detail-stats">
      <span>
        {entry.mod_count} mod{entry.mod_count === 1 ? "" : "s"}
      </span>
      {entry.nexus_count ? <span>{entry.nexus_count} Nexus</span> : null}
      {entry.thunderstore_count ? (
        <span>{entry.thunderstore_count} Thunderstore</span>
      ) : null}
      {entry.modio_count ? <span>{entry.modio_count} mod.io</span> : null}
      {builtFor ? <span>Built for {builtFor}</span> : null}
      {created ? <span>Created {created}</span> : null}
      {updated ? <span>Updated {updated}</span> : null}
    </div>
  );
}

export function EditableCollectionTitle({
  title,
  onRename,
  as = "h1",
  stopRowClick = false,
}: {
  title: string;
  onRename?: (nextName: string) => Promise<void> | void;
  as?: "h1" | "strong";
  stopRowClick?: boolean;
}) {
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(title);
  const [saving, setSaving] = useState(false);
  const inputRef = useRef<HTMLInputElement | null>(null);
  const skipBlurCommitRef = useRef(false);
  const compact = as === "strong";

  useEffect(() => {
    if (!editing) setDraft(title);
  }, [title, editing]);

  useEffect(() => {
    if (editing) {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [editing]);

  function startEditing() {
    if (!onRename || saving) return;
    setDraft(title);
    setEditing(true);
  }

  function cancelEditing() {
    skipBlurCommitRef.current = true;
    setDraft(title);
    setEditing(false);
  }

  async function commitEditing() {
    if (!editing || saving || !onRename) return;
    const trimmed = draft.trim();
    if (!trimmed || trimmed === title) {
      setDraft(title);
      setEditing(false);
      return;
    }
    setSaving(true);
    try {
      await onRename(trimmed);
      setEditing(false);
    } finally {
      setSaving(false);
    }
  }

  if (editing) {
    return (
      <input
        ref={inputRef}
        type="text"
        className={
          compact ? "collection-title-input is-compact" : "collection-title-input"
        }
        aria-label="Collection name"
        value={draft}
        disabled={saving}
        onClick={stopRowClick ? (e) => e.stopPropagation() : undefined}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            void commitEditing();
          } else if (e.key === "Escape") {
            e.preventDefault();
            cancelEditing();
          }
        }}
        onBlur={() => {
          if (skipBlurCommitRef.current) {
            skipBlurCommitRef.current = false;
            return;
          }
          void commitEditing();
        }}
      />
    );
  }

  const titleEl = compact ? (
    <strong
      onDoubleClick={
        onRename
          ? (e) => {
              if (stopRowClick) e.stopPropagation();
              startEditing();
            }
          : undefined
      }
      title={onRename ? "Double-click to rename" : undefined}
    >
      {title}
    </strong>
  ) : (
    <h1
      onDoubleClick={onRename ? startEditing : undefined}
      title={onRename ? "Double-click to rename" : undefined}
    >
      {title}
    </h1>
  );

  const buttonEl = onRename ? (
    <button
      type="button"
      className={
        compact
          ? "icon-btn collection-rename-btn is-compact"
          : "icon-btn collection-rename-btn"
      }
      aria-label="Rename collection"
      title="Rename collection"
      onClick={(e) => {
        if (stopRowClick) e.stopPropagation();
        startEditing();
      }}
    >
      <FontAwesomeIcon icon={faPen} />
    </button>
  ) : null;

  if (compact) {
    return (
      <span className="collection-title-inline">
        {titleEl}
        {buttonEl}
      </span>
    );
  }

  return (
    <>
      {titleEl}
      {buttonEl}
    </>
  );
}

export type CollectionDetailViewProps = {
  title: string;
  onRename?: (nextName: string) => Promise<void> | void;
  backLabel: string;
  onBack: () => void;
  headerPills?: ReactNode;
  headerActions?: ReactNode;
  coverSrc?: string | null;
  gameTitle: string;
  metaPills?: ReactNode;
  stats: CollectionStatsSummary;
  versionWarning?: string | null;
  managed: ManagedGame[];
  selectedGameId: string;
  onSelectGameId: (gameId: string) => void;
  gameMatches?: (game: ManagedGame) => boolean;
  gameKnown?: boolean;
  actions: ReactNode;
  code: string;
  metaExtra?: ReactNode;
  bodyTop?: ReactNode;
  mods: ShareModEntry[];
  showMods: boolean;
  skeletonCount?: number;
  scrollRef?: RefObject<HTMLElement | null>;
  active: boolean;
  loadingBody?: ReactNode;
  noGamesFallback?: ReactNode;
  onOpenMod?: (hit: CatalogHit) => void;
};

export function CollectionDetailView({
  title,
  onRename,
  backLabel,
  onBack,
  headerPills,
  headerActions,
  coverSrc,
  gameTitle,
  metaPills,
  stats,
  versionWarning,
  managed,
  selectedGameId,
  onSelectGameId,
  gameMatches,
  gameKnown = false,
  actions,
  code,
  metaExtra,
  bodyTop,
  mods,
  showMods,
  skeletonCount,
  scrollRef,
  active,
  loadingBody,
  noGamesFallback,
  onOpenMod,
}: CollectionDetailViewProps) {
  const [copiedCode, setCopiedCode] = useState(false);
  const bodyScrollRef = useRef<HTMLDivElement | null>(null);
  const effectiveScrollRef = scrollRef ? bodyScrollRef : undefined;

  function handleCopyCode() {
    if (!code) return;
    void navigator.clipboard?.writeText(code).then(() => {
      setCopiedCode(true);
      window.setTimeout(() => setCopiedCode(false), 1800);
    });
  }

  const matchingGames = gameMatches ? managed.filter(gameMatches) : [];
  const showNoMatchFallback =
    managed.length === 0 || (matchingGames.length === 0 && gameKnown);
  const selectableGames =
    matchingGames.length > 1
      ? matchingGames
      : !gameKnown && matchingGames.length === 0
        ? managed
        : [];

  return (
    <div className="page-view">
      <div className="page-chrome">
      <div className="panel-head">
        <div className="detail-head-title">
          <button type="button" onClick={onBack}>
            {backLabel}
          </button>
          <EditableCollectionTitle title={title} onRename={onRename} />
          {headerPills}
        </div>
        {headerActions ? (
          <div className="panel-head-actions">{headerActions}</div>
        ) : null}
      </div>
      </div>

      {loadingBody ? (
        <div className="page-scroll" ref={bodyScrollRef}>
          {loadingBody}
        </div>
      ) : (
        <>
          <div className="page-chrome">
          <div className="detail-hero">
            <div className="detail-cover">
              {coverSrc ? (
                <img src={coverSrc} alt="" />
              ) : (
                <CollectionPlaceholderIcon />
              )}
            </div>
            <div className="detail-meta">
              <p className="detail-author">
                {gameTitle}
                {metaPills}
              </p>
              <CollectionStats entry={stats} />
              {versionWarning ? (
                <p className="mod-version-warn">{versionWarning}</p>
              ) : null}
              <div className="saved-collection-actions">
                {!showNoMatchFallback ? (
                  <>
                    {selectableGames.length > 0 ? (
                      <select
                        className="saved-collection-game-select"
                        aria-label="Install into"
                        value={selectedGameId}
                        onChange={(e) => onSelectGameId(e.target.value)}
                      >
                        <option value="">Select managed game…</option>
                        {selectableGames.map((g) => (
                          <option key={g.id} value={g.id}>
                            {g.title}
                          </option>
                        ))}
                      </select>
                    ) : null}
                    {actions}
                  </>
                ) : (
                  noGamesFallback ?? (
                    <span className="muted">
                      Manage this game in Library to install.
                    </span>
                  )
                )}
                {code ? (
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label="Copy code"
                    title="Copy code"
                    onClick={handleCopyCode}
                  >
                    <FontAwesomeIcon icon={copiedCode ? faCheck : faCopy} />
                  </button>
                ) : null}
              </div>
              {metaExtra}
            </div>
          </div>
          </div>

          <div className="detail-body page-view">
            <ContainedModsSection
              mods={mods}
              showMods={showMods}
              skeletonCount={skeletonCount ?? (stats.mod_count || 8)}
              scrollRef={effectiveScrollRef}
              active={active}
              onOpenMod={onOpenMod}
              renderLayout={(controls, list) => (
                <>
                  <div className="page-chrome">
                    {bodyTop}
                    {controls}
                  </div>
                  <div className="page-scroll" ref={bodyScrollRef}>
                    {list}
                    <h2>Code</h2>
                    <textarea
                      className="saved-collection-code"
                      rows={4}
                      readOnly
                      value={code}
                    />
                  </div>
                </>
              )}
            />
          </div>
        </>
      )}
    </div>
  );
}
