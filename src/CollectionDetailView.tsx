import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCheck, faCopy, faPen } from "@fortawesome/free-solid-svg-icons";
import { BrowseVirtualList } from "./BrowseResults";
import { estimateListRowHeight } from "./browseVirtual";
import { ResultsReveal } from "./reveal";
import { formatCollectionGameVersions } from "./savedCollectionGroups";
import { BrowseListSkeleton } from "./Skeleton";
import type { ManagedGame, ShareModEntry } from "./types";

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
  scrollRef,
  active = true,
}: {
  mods: ShareModEntry[];
  scrollRef?: RefObject<HTMLElement | null>;
  active?: boolean;
}) {
  const rows = useMemo(() => mods.map((mod, index) => ({ mod, index })), [mods]);
  const getKey = useCallback(
    (row: { mod: ShareModEntry; index: number }) => `${row.mod.s}-${row.index}`,
    [],
  );
  const renderItem = useCallback((row: { mod: ShareModEntry; index: number }) => {
    const m = row.mod;
    return (
      <div>
        <strong>
          {shareModLabel(m)}
          {m.fomod ? (
            <>
              {" "}
              <span className="pill ok">FOMOD Config</span>
            </>
          ) : null}
        </strong>
        <small>
          {m.s}
          {m.version ? ` · v${m.version}` : ""}
          {m.s === "nexus" && m.mod_id != null
            ? ` · ${m.domain} ${m.mod_id}/${m.file_id}`
            : ""}
          {m.s === "thunderstore"
            ? ` · ${m.community}/${m.namespace}-${m.name}`
            : ""}
          {m.s === "modio" ? ` · game ${m.game_id} mod ${m.mod_id}` : ""}
        </small>
      </div>
    );
  }, []);

  if (mods.length === 0) {
    return (
      <ul className="list">
        <li className="empty">No mods in this share.</li>
      </ul>
    );
  }

  if (!scrollRef) {
    return (
      <ul className="list">
        {rows.map((row) => (
          <li key={getKey(row)}>{renderItem(row)}</li>
        ))}
      </ul>
    );
  }

  return (
    <BrowseVirtualList
      items={rows}
      scrollRef={scrollRef}
      active={active}
      staggerEnter
      estimateSize={estimateListRowHeight(false)}
      getKey={getKey}
      rowClassName=""
      renderItem={renderItem}
    />
  );
});

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
}: CollectionDetailViewProps) {
  const [copiedCode, setCopiedCode] = useState(false);

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
    <div>
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

      {loadingBody ? (
        loadingBody
      ) : (
        <>
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

          <div className="detail-body">
            {bodyTop}
            <h2>Contained mods</h2>
            <ResultsReveal
              show={showMods}
              skeleton={
                <BrowseListSkeleton
                  count={Math.min(12, skeletonCount ?? (stats.mod_count || 8))}
                />
              }
            >
              {showMods ? (
                <ShareModList
                  mods={mods}
                  scrollRef={scrollRef}
                  active={active}
                />
              ) : null}
            </ResultsReveal>
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
    </div>
  );
}
