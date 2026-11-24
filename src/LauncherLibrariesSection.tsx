import { useCallback, useMemo, useState, type RefObject } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faChevronDown,
  faChevronRight,
  faFilter,
  faRotate,
} from "@fortawesome/free-solid-svg-icons";
import { BrowseVirtualGrid } from "./BrowseResults";
import { MediaCard } from "./MediaCard";
import { Collapse } from "./motion";
import type {
  LauncherInstallProgress,
  LibraryGame,
  LibrarySection,
  LibrarySource,
} from "./types";

const EXPANDED_STORAGE_KEY = "emm.librarySections.expanded";

function loadExpandedSources(): Record<string, boolean> {
  try {
    const raw = localStorage.getItem(EXPANDED_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw);
    if (parsed && typeof parsed === "object") return parsed as Record<string, boolean>;
  } catch {
    // ignore storage errors
  }
  return {};
}

function saveExpandedSources(map: Record<string, boolean>) {
  try {
    localStorage.setItem(EXPANDED_STORAGE_KEY, JSON.stringify(map));
  } catch {
    // ignore storage errors
  }
}

export type LauncherLibrariesSectionProps = {
  sections: LibrarySection[];
  loading: boolean;
  active: boolean;
  scrollRef: RefObject<HTMLElement | null>;
  installsByGameId: Record<string, LauncherInstallProgress>;
  resolveCoverSrc: (game: LibraryGame) => string | null;
  onSelectGame: (game: LibraryGame) => void;
  onCoverExhausted?: (game: LibraryGame) => void;
  onRefresh: (refreshRemote?: boolean) => void;
  onOpenSettings: () => void;
};

export function LauncherLibrariesSection({
  sections,
  loading,
  active,
  scrollRef,
  installsByGameId,
  resolveCoverSrc,
  onSelectGame,
  onCoverExhausted,
  onRefresh,
  onOpenSettings,
}: LauncherLibrariesSectionProps) {
  const [filtersOpen, setFiltersOpen] = useState(false);
  const [search, setSearch] = useState("");
  const [supportedOnly, setSupportedOnly] = useState(false);
  const [expanded, setExpanded] = useState<Record<string, boolean>>(loadExpandedSources);

  const toggleSection = useCallback((source: LibrarySource) => {
    setExpanded((prev) => {
      const next = { ...prev, [source]: !prev[source] };
      saveExpandedSources(next);
      return next;
    });
  }, []);

  const query = search.trim().toLowerCase();
  const filtersActive = Boolean(query || supportedOnly);

  const filteredSections = useMemo(() => {
    return sections.map((sec) => {
      const games = sec.games.filter((g) => {
        if (supportedOnly && !g.supported) return false;
        if (query && !g.title.toLowerCase().includes(query)) return false;
        return true;
      });
      return { ...sec, filteredGames: games };
    });
  }, [sections, supportedOnly, query]);

  const totalUninstalled = useMemo(
    () => sections.reduce((sum, s) => sum + s.games.length, 0),
    [sections],
  );

  const totalFiltered = useMemo(
    () => filteredSections.reduce((sum, s) => sum + s.filteredGames.length, 0),
    [filteredSections],
  );

  const getKey = useCallback((g: LibraryGame) => g.id, []);
  const renderSkeleton = useCallback(
    () => (
      <div className="media-card" aria-hidden="true">
        <div className="media-cover-stack">
          <div className="media-cover skeleton" />
        </div>
      </div>
    ),
    [],
  );

  const renderGameCard = useCallback(
    (g: LibraryGame) => {
      const progress = installsByGameId[g.id];
      const isInstalling =
        progress &&
        (progress.phase === "starting" ||
          progress.phase === "downloading" ||
          progress.phase === "verifying");
      const badge = isInstalling
        ? progress.percent != null
          ? `${Math.round(progress.percent)}%`
          : "Installing…"
        : g.supported
          ? "Supported"
          : null;

      return (
        <MediaCard
          key={g.id}
          plain
          title={g.title}
          imageSrc={resolveCoverSrc(g)}
          fallbackImageSrcs={g.cover_fallbacks}
          onImageExhausted={onCoverExhausted ? () => onCoverExhausted(g) : undefined}
          badge={badge}
          hoverLabel={isInstalling ? "View progress" : "Install"}
          onClick={() => onSelectGame(g)}
        />
      );
    },
    [installsByGameId, onCoverExhausted, onSelectGame, resolveCoverSrc],
  );

  if (sections.length === 0 && !loading) {
    return null;
  }

  return (
    <div className="launcher-libraries-root" data-testid="launcher-libraries-root">
      <div className="launcher-libraries-header">
        <div>
          <h2>Launcher Libraries</h2>
          <p className="note">
            Uninstalled titles from your connected launchers ({totalUninstalled} available)
          </p>
        </div>
        <div className="launcher-libraries-toolbar">
          <button
            type="button"
            disabled={loading}
            onClick={() => onRefresh(true)}
            title="Rescan local launcher caches and refresh remote Steam sync"
          >
            <FontAwesomeIcon icon={faRotate} spin={loading} /> Sync libraries
          </button>
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
              type="search"
              placeholder="Filter uninstalled games…"
              value={search}
              onChange={(e) => setSearch(e.target.value)}
              aria-label="Filter uninstalled games"
            />
          </div>
          <button
            type="button"
            role="switch"
            aria-checked={supportedOnly}
            className="filter-switch-toggle"
            onClick={() => setSupportedOnly((v) => !v)}
          >
            <span className="switch-track" aria-hidden="true">
              <span className="switch-thumb" />
            </span>
            <span>Supported only</span>
          </button>
          {filtersActive && (
            <button
              type="button"
              className="filters-clear"
              onClick={() => {
                setSearch("");
                setSupportedOnly(false);
              }}
            >
              Clear filters
            </button>
          )}
        </div>
      </Collapse>

      {filtersActive && totalUninstalled > 0 && (
        <p className="note launcher-libraries-filter-summary">
          Showing {totalFiltered} of {totalUninstalled}
        </p>
      )}

      {filteredSections.map((sec) => {
        const isOpen = Boolean(expanded[sec.source]);
        const countLabel =
          query || supportedOnly
            ? `${sec.filteredGames.length} / ${sec.games.length}`
            : `${sec.games.length}`;
        return (
          <section
            key={sec.source}
            className="launcher-library-section"
            data-testid={`library-section-${sec.source}`}
          >
            <button
              type="button"
              className="launcher-library-toggle"
              aria-expanded={isOpen}
              onClick={() => toggleSection(sec.source)}
            >
              <FontAwesomeIcon icon={isOpen ? faChevronDown : faChevronRight} />
              <span className="launcher-library-title">{sec.label}</span>
              <span className="pill">{countLabel} uninstalled</span>
            </button>

            {isOpen && (
              <div className="launcher-library-body">
                {sec.warning && (
                  <p className="note launcher-library-warning">
                    {sec.warning}{" "}
                    {sec.source === "steam" && (
                      <button
                        type="button"
                        className="link-button"
                        onClick={onOpenSettings}
                      >
                        Open Settings
                      </button>
                    )}
                  </p>
                )}

                {sec.filteredGames.length === 0 ? (
                  <p className="note">
                    {sec.games.length === 0
                      ? "All detected titles from this library are already installed."
                      : "No uninstalled titles match your current filter."}
                  </p>
                ) : (
                  <div className="library-posters">
                    <BrowseVirtualGrid
                      items={sec.filteredGames}
                      scrollRef={scrollRef}
                      minCardPx={160}
                      fixedRowHeight
                      hasMeta={false}
                      active={active && isOpen}
                      trackScrollMargin
                      getKey={getKey}
                      renderItem={renderGameCard}
                      renderSkeletonItem={renderSkeleton}
                    />
                  </div>
                )}
              </div>
            )}
          </section>
        );
      })}
    </div>
  );
}
