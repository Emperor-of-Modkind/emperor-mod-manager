import { useCallback, useEffect, useRef, useState, type RefObject } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowLeft,
  faCloudArrowDown,
  faMagnifyingGlass,
  faThumbsUp,
} from "@fortawesome/free-solid-svg-icons";

import { api } from "./api";
import { BrowseVirtualGrid } from "./BrowseResults";
import { MediaCard } from "./MediaCard";
import { Segment, SegmentTab } from "./motion";
import { BrowseGridSkeleton } from "./Skeleton";
import { SourceMark, sourceMarkName } from "./SourceMark";
import type {
  CatalogHit,
  CatalogSearchPage,
  CatalogSource,
  CollectionDetail,
  CollectionHit,
  CollectionSearchPage,
  ManagedGame,
  ModFileInfo,
  ModioFileInfo,
  RemoteDownloadRequest,
  TsPackageDetail,
} from "./types";

type BrowseMode = "mods" | "collections";
type SourceFilter = "all" | CatalogSource;

type SelectedModDetail = {
  hit: CatalogHit;
  description?: string | null;
  files?: ModFileInfo[];
  modioFiles?: ModioFileInfo[];
  tsDetail?: TsPackageDetail | null;
  loading: boolean;
  error: string | null;
};

type SelectedCollectionDetail = {
  hit: CollectionHit;
  detail?: CollectionDetail | null;
  loading: boolean;
  error: string | null;
};

type Props = {
  activeGame: ManagedGame | null;
  scrollRef: RefObject<HTMLElement | null>;
  online: boolean;
  onQueueDownload: (req: RemoteDownloadRequest, label: string) => Promise<void>;
  onError: (msg: string) => void;
  onPushBack?: (handler: () => void) => void;
};

function formatCount(n: number | null | undefined): string {
  if (n == null) return "";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

export function MobileBrowseView({
  activeGame,
  scrollRef,
  online,
  onQueueDownload,
  onError,
  onPushBack,
}: Props) {
  const [mode, setMode] = useState<BrowseMode>("mods");
  const [sourceFilter, setSourceFilter] = useState<SourceFilter>("all");
  const [sort, setSort] = useState("popular");
  const [query, setQuery] = useState("");
  const [submittedQuery, setSubmittedQuery] = useState("");
  const [loading, setLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [modPage, setModPage] = useState<CatalogSearchPage | null>(null);
  const [colPage, setColPage] = useState<CollectionSearchPage | null>(null);
  const [selectedMod, setSelectedMod] = useState<SelectedModDetail | null>(null);
  const [selectedCol, setSelectedCol] = useState<SelectedCollectionDetail | null>(null);
  const [installingId, setInstallingId] = useState<string | null>(null);

  const onErrorRef = useRef(onError);
  onErrorRef.current = onError;
  const pageSizeRef = useRef(24);
  const requestIdRef = useRef(0);
  const fetchingMoreRef = useRef(false);
  const sentinelRef = useRef<HTMLDivElement | null>(null);

  const handlePageSize = useCallback((count: number) => {
    pageSizeRef.current = Math.max(18, Math.min(48, count));
  }, []);

  const renderSkeletonCard = useCallback(
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

  const activeGameId = activeGame?.id ?? null;

  const runSearch = useCallback(
    async (offset = 0, append = false) => {
      if (!activeGameId) return;
      if (append) {
        if (fetchingMoreRef.current) return;
        fetchingMoreRef.current = true;
        setLoadingMore(true);
      } else {
        requestIdRef.current += 1;
        fetchingMoreRef.current = false;
        setLoadingMore(false);
        setLoading(true);
      }
      const reqId = requestIdRef.current;
      try {
        const count = pageSizeRef.current;
        if (mode === "mods") {
          const page = await api.mobileCatalogRpc<CatalogSearchPage>("search_catalog", {
            game_id: activeGameId,
            query: submittedQuery,
            source_filter: sourceFilter === "all" ? null : sourceFilter,
            sort,
            offset,
            count,
          });
          if (reqId !== requestIdRef.current) return;
          setModPage((prev) => {
            if (!append || !prev) return page;
            const seen = new Set(prev.items.map((i) => `${i.source}:${i.id}`));
            const fresh = page.items.filter((i) => !seen.has(`${i.source}:${i.id}`));
            return { ...page, items: [...prev.items, ...fresh] };
          });
        } else {
          const page = await api.mobileCatalogRpc<CollectionSearchPage>(
            "search_collection_catalog",
            {
              game_id: activeGameId,
              query: submittedQuery,
              source_filter: sourceFilter === "all" ? null : sourceFilter,
              sort,
              offset,
              count,
            },
          );
          if (reqId !== requestIdRef.current) return;
          setColPage((prev) => {
            if (!append || !prev) return page;
            const seen = new Set(prev.items.map((i) => `${i.source ?? "nexus"}:${i.slug}`));
            const fresh = page.items.filter(
              (i) => !seen.has(`${i.source ?? "nexus"}:${i.slug}`),
            );
            return { ...page, items: [...prev.items, ...fresh] };
          });
        }
      } catch (e) {
        if (reqId === requestIdRef.current) {
          onErrorRef.current(`Browse failed: ${String(e)}`);
        }
      } finally {
        if (append) {
          fetchingMoreRef.current = false;
          setLoadingMore(false);
        } else if (reqId === requestIdRef.current) {
          setLoading(false);
        }
      }
    },
    [activeGameId, mode, submittedQuery, sourceFilter, sort],
  );

  useEffect(() => {
    setSelectedMod(null);
    setSelectedCol(null);
    setModPage(null);
    setColPage(null);
    if (activeGameId) {
      void runSearch(0, false);
    }
  }, [activeGameId, mode, sourceFilter, sort, submittedQuery, runSearch]);

  const triggerLoadMore = useCallback(() => {
    if (loading || loadingMore || fetchingMoreRef.current) return;
    if (mode === "mods" && modPage?.has_more) {
      void runSearch(modPage.next_offset, true);
    } else if (mode === "collections" && colPage?.has_more && colPage.next_offset != null) {
      void runSearch(colPage.next_offset, true);
    }
  }, [loading, loadingMore, mode, modPage, colPage, runSearch]);

  // Automatic infinite scroll via IntersectionObserver sentinel + scroll fallback (folded & unfolded)
  useEffect(() => {
    const scrollEl = scrollRef.current;
    const sentinel = sentinelRef.current;
    if (!scrollEl || selectedMod || selectedCol) return;

    const checkScroll = () => {
      const remaining = scrollEl.scrollHeight - scrollEl.scrollTop - scrollEl.clientHeight;
      if (remaining <= 560) {
        triggerLoadMore();
      }
    };
    scrollEl.addEventListener("scroll", checkScroll, { passive: true });

    let observer: IntersectionObserver | null = null;
    if (sentinel && typeof IntersectionObserver !== "undefined") {
      observer = new IntersectionObserver(
        (entries) => {
          if (entries.some((e) => e.isIntersecting)) {
            triggerLoadMore();
          }
        },
        { root: scrollEl, rootMargin: "480px 0px" },
      );
      observer.observe(sentinel);
    }

    return () => {
      scrollEl.removeEventListener("scroll", checkScroll);
      observer?.disconnect();
    };
  }, [scrollRef, selectedMod, selectedCol, triggerLoadMore]);

  const openModDetail = useCallback(async (hit: CatalogHit) => {
    setSelectedMod({ hit, loading: true, error: null });
    onPushBack?.(() => setSelectedMod(null));
    try {
      if (hit.source === "nexus" && hit.domain_name && hit.mod_id != null) {
        const [modInfo, files] = await Promise.all([
          api.mobileCatalogRpc<{ description?: string | null; summary?: string | null }>(
            "get_mod",
            { domain: hit.domain_name, mod_id: hit.mod_id },
          ),
          api.mobileCatalogRpc<ModFileInfo[]>("mod_files", {
            domain: hit.domain_name,
            mod_id: hit.mod_id,
          }),
        ]);
        setSelectedMod({
          hit,
          description: modInfo?.summary || hit.summary,
          files: Array.isArray(files) ? files : [],
          loading: false,
          error: null,
        });
      } else if (
        hit.source === "thunderstore" &&
        hit.community &&
        hit.namespace &&
        hit.package_name
      ) {
        const tsDetail = await api.mobileCatalogRpc<TsPackageDetail>(
          "get_thunderstore_package",
          {
            community: hit.community,
            namespace: hit.namespace,
            name: hit.package_name,
          },
        );
        setSelectedMod({
          hit,
          description: tsDetail?.description || hit.summary,
          tsDetail,
          loading: false,
          error: null,
        });
      } else if (
        hit.source === "modio" &&
        hit.modio_game_id != null &&
        hit.modio_mod_id != null
      ) {
        const files = await api.mobileCatalogRpc<ModioFileInfo[]>("modio_files", {
          modio_game_id: hit.modio_game_id,
          mod_id: hit.modio_mod_id,
        });
        setSelectedMod({
          hit,
          description: hit.summary,
          modioFiles: Array.isArray(files) ? files : [],
          loading: false,
          error: null,
        });
      } else {
        setSelectedMod({ hit, description: hit.summary, loading: false, error: null });
      }
    } catch (e) {
      setSelectedMod({
        hit,
        description: hit.summary,
        loading: false,
        error: String(e),
      });
    }
  }, []);

  const openCollectionDetail = useCallback(async (hit: CollectionHit) => {
    setSelectedCol({ hit, loading: true, error: null });
    onPushBack?.(() => setSelectedCol(null));
    try {
      if (hit.source === "nexus" && hit.slug) {
        const detail = await api.mobileCatalogRpc<CollectionDetail>("get_collection", {
          slug: hit.slug,
          domain: hit.domain_name,
        });
        setSelectedCol({ hit, detail, loading: false, error: null });
      } else {
        setSelectedCol({ hit, detail: null, loading: false, error: null });
      }
    } catch (e) {
      setSelectedCol({ hit, detail: null, loading: false, error: String(e) });
    }
  }, []);

  const installNexusFile = async (hit: CatalogHit, file: ModFileInfo) => {
    if (!activeGame || !hit.domain_name || hit.mod_id == null) return;
    setInstallingId(`nexus:${file.file_id}`);
    try {
      await onQueueDownload(
        {
          kind: "nexus_mod",
          game_id: activeGame.id,
          domain: hit.domain_name,
          mod_id: hit.mod_id,
          file_id: file.file_id,
          mod_name: hit.name,
          file_name: file.file_name,
          version: file.version ?? hit.latest_version ?? null,
        },
        hit.name,
      );
    } finally {
      setInstallingId(null);
    }
  };

  const installThunderstore = async (hit: CatalogHit) => {
    if (!activeGame || !hit.community || !hit.namespace || !hit.package_name) return;
    setInstallingId(`ts:${hit.id}`);
    try {
      await onQueueDownload(
        {
          kind: "thunderstore_mod",
          game_id: activeGame.id,
          community: hit.community,
          namespace: hit.namespace,
          name: hit.package_name,
        },
        hit.name,
      );
    } finally {
      setInstallingId(null);
    }
  };

  const installModio = async (hit: CatalogHit, fileId?: number | null) => {
    if (!activeGame || hit.modio_game_id == null || hit.modio_mod_id == null) return;
    setInstallingId(`modio:${fileId ?? "latest"}`);
    try {
      await onQueueDownload(
        {
          kind: "modio_mod",
          game_id: activeGame.id,
          modio_game_id: hit.modio_game_id,
          mod_id: hit.modio_mod_id,
          file_id: fileId ?? null,
        },
        hit.name,
      );
    } finally {
      setInstallingId(null);
    }
  };

  const installCollection = async (hit: CollectionHit, revision?: number | null) => {
    if (!activeGame) return;
    setInstallingId(`col:${hit.slug}`);
    try {
      if (hit.source === "thunderstore" && hit.community && hit.namespace && hit.package_name) {
        await onQueueDownload(
          {
            kind: "thunderstore_mod",
            game_id: activeGame.id,
            community: hit.community,
            namespace: hit.namespace,
            name: hit.package_name,
          },
          hit.name,
        );
      } else {
        await onQueueDownload(
          {
            kind: "nexus_collection",
            game_id: activeGame.id,
            slug: hit.slug,
            revision: revision ?? null,
          },
          hit.name,
        );
      }
    } finally {
      setInstallingId(null);
    }
  };

  if (!activeGame) {
    return (
      <div className="mobile-empty-pane">
        <p className="muted">Choose a game in Library first to browse mods and collections.</p>
      </div>
    );
  }

  if (selectedMod) {
    const { hit } = selectedMod;
    return (
      <div className="mobile-detail-view">
        <button
          type="button"
          className="ghost mobile-back-btn"
          onClick={() => setSelectedMod(null)}
        >
          <FontAwesomeIcon icon={faArrowLeft} /> Back to Browse
        </button>

        <div className="mobile-detail-hero">
          {hit.picture_url ? (
            <img className="mobile-detail-cover" src={hit.picture_url} alt={hit.name} />
          ) : null}
          <div className="mobile-detail-hero-body">
            <div className="mobile-detail-source-row">
              <SourceMark source={hit.source} />
              <span className="muted">{hit.author || "Unknown author"}</span>
              {hit.latest_version ? <span className="pill">v{hit.latest_version}</span> : null}
            </div>
            <h2>{hit.name}</h2>
            <div className="mobile-detail-stats muted">
              {hit.downloads != null ? (
                <span>
                  <FontAwesomeIcon icon={faCloudArrowDown} /> {formatCount(hit.downloads)}
                </span>
              ) : null}
              {hit.endorsements != null ? (
                <span>
                  <FontAwesomeIcon icon={faThumbsUp} /> {formatCount(hit.endorsements)}
                </span>
              ) : null}
            </div>
            {selectedMod.description ? (
              <p className="mobile-detail-summary">{selectedMod.description}</p>
            ) : null}
          </div>
        </div>

        {selectedMod.loading ? (
          <p className="muted">Loading files from desktop…</p>
        ) : selectedMod.error ? (
          <p className="muted">{selectedMod.error}</p>
        ) : hit.source === "thunderstore" ? (
          <div className="mobile-detail-actions">
            <button
              type="button"
              className="primary"
              disabled={!online || installingId !== null}
              onClick={() => void installThunderstore(hit)}
            >
              <FontAwesomeIcon icon={faCloudArrowDown} />{" "}
              {installingId ? "Queueing…" : "Install on PC"}
            </button>
          </div>
        ) : hit.source === "modio" ? (
          <div className="mobile-detail-section">
            <div className="mobile-detail-actions">
              <button
                type="button"
                className="primary"
                disabled={!online || installingId !== null}
                onClick={() =>
                  void installModio(hit, selectedMod.modioFiles?.[0]?.file_id ?? null)
                }
              >
                <FontAwesomeIcon icon={faCloudArrowDown} />{" "}
                {installingId ? "Queueing…" : "Install Latest on PC"}
              </button>
            </div>
            {selectedMod.modioFiles && selectedMod.modioFiles.length > 0 ? (
              <div className="mobile-mod-list">
                {selectedMod.modioFiles.slice(0, 10).map((f) => (
                  <div key={f.file_id} className="mobile-mod-row">
                    <div className="mobile-mod-info">
                      <strong>{f.filename}</strong>
                      <span className="muted">
                        {f.version ? `v${f.version}` : "Release"}
                      </span>
                    </div>
                    <button
                      type="button"
                      className="secondary"
                      disabled={!online || installingId !== null}
                      onClick={() => void installModio(hit, f.file_id)}
                    >
                      Install
                    </button>
                  </div>
                ))}
              </div>
            ) : null}
          </div>
        ) : (
          <div className="mobile-detail-section">
            <h3>Files</h3>
            {selectedMod.files && selectedMod.files.length > 0 ? (
              <div className="mobile-mod-list">
                {selectedMod.files.slice(0, 15).map((f) => (
                  <div key={f.file_id} className="mobile-mod-row">
                    <div className="mobile-mod-info">
                      <strong>{f.name || f.file_name}</strong>
                      <span className="muted">
                        {f.version ? `v${f.version}` : ""}
                        {f.category_name ? ` · ${f.category_name}` : ""}
                      </span>
                    </div>
                    <button
                      type="button"
                      className="primary"
                      disabled={!online || installingId !== null}
                      onClick={() => void installNexusFile(hit, f)}
                    >
                      {installingId === `nexus:${f.file_id}` ? "Queueing…" : "Install"}
                    </button>
                  </div>
                ))}
              </div>
            ) : (
              <p className="muted">No downloadable files found.</p>
            )}
          </div>
        )}
      </div>
    );
  }

  if (selectedCol) {
    const { hit, detail } = selectedCol;
    return (
      <div className="mobile-detail-view">
        <button
          type="button"
          className="ghost mobile-back-btn"
          onClick={() => setSelectedCol(null)}
        >
          <FontAwesomeIcon icon={faArrowLeft} /> Back to Browse
        </button>

        <div className="mobile-detail-hero">
          {hit.tile_image_url || detail?.tile_image_url ? (
            <img
              className="mobile-detail-cover"
              src={hit.tile_image_url || detail?.tile_image_url || ""}
              alt={hit.name}
            />
          ) : null}
          <div className="mobile-detail-hero-body">
            <div className="mobile-detail-source-row">
              <SourceMark source={hit.source ?? "nexus"} />
              <span className="muted">{hit.author || "Collection"}</span>
              {hit.mod_count != null ? (
                <span className="pill">{hit.mod_count} mods</span>
              ) : null}
            </div>
            <h2>{detail?.name || hit.name}</h2>
            <p className="mobile-detail-summary">
              {detail?.summary ||
                hit.summary ||
                "Install this collection directly on your paired PC."}
            </p>
            <div className="mobile-detail-actions">
              <button
                type="button"
                className="primary"
                disabled={!online || installingId !== null}
                onClick={() => void installCollection(hit, detail?.revision_number)}
              >
                <FontAwesomeIcon icon={faCloudArrowDown} />{" "}
                {installingId ? "Queueing…" : "Install Collection on PC"}
              </button>
            </div>
          </div>
        </div>
      </div>
    );
  }

  const modItems = modPage?.items ?? [];
  const colItems = colPage?.items ?? [];

  return (
    <div className="mobile-browse-pane">
      <div className="mobile-toolbar-row">
        <h2>Browse · {activeGame.title}</h2>
        <Segment>
          <SegmentTab
            layoutId="mobile-browse-mode"
            active={mode === "mods"}
            onClick={() => setMode("mods")}
          >
            Mods
          </SegmentTab>
          <SegmentTab
            layoutId="mobile-browse-mode"
            active={mode === "collections"}
            onClick={() => setMode("collections")}
          >
            Collections
          </SegmentTab>
        </Segment>
      </div>

      <form
        className="mobile-search-form"
        onSubmit={(e) => {
          e.preventDefault();
          setSubmittedQuery(query.trim());
        }}
      >
        <div className="mobile-search-input-wrap">
          <FontAwesomeIcon icon={faMagnifyingGlass} className="muted" />
          <input
            type="search"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={`Search ${activeGame.title} ${mode}…`}
          />
        </div>
        <button type="submit" className="primary" disabled={loading}>
          Search
        </button>
      </form>

      <div className="mobile-filter-row">
        <div className="mobile-chip-group">
          {(["all", "nexus", "thunderstore", "modio"] as SourceFilter[]).map((src) => (
            <button
              key={src}
              type="button"
              className={`mobile-filter-chip ${sourceFilter === src ? "active" : ""}`}
              onClick={() => setSourceFilter(src)}
            >
              {src === "all"
                ? "All"
                : src === "nexus"
                  ? "Nexus"
                  : src === "thunderstore"
                    ? "Thunderstore"
                    : "mod.io"}
            </button>
          ))}
        </div>
        <select
          className="mobile-select mobile-sort-select"
          aria-label="Sort results"
          value={sort}
          onChange={(e) => setSort(e.target.value)}
        >
          <option value="popular">Popular</option>
          <option value="trending">Trending</option>
          <option value="recent">Recent</option>
          <option value="updated">Updated</option>
        </select>
      </div>

      {loading && modItems.length === 0 && colItems.length === 0 ? (
        <BrowseGridSkeleton rows={3} minCardPx={160} />
      ) : mode === "mods" ? (
        modItems.length === 0 ? (
          <p className="muted">No mods matched your search.</p>
        ) : (
          <>
            <BrowseVirtualGrid
              items={modItems}
              scrollRef={scrollRef}
              minCardPx={160}
              fixedRowHeight
              staggerEnter
              staggerKey={`${activeGame.id}:${mode}:${sourceFilter}:${sort}:${submittedQuery}`}
              fillTrailingSkeletons={Boolean(modPage?.has_more)}
              trackScrollMargin
              onPageSize={handlePageSize}
              getKey={(hit) => `${hit.source}:${hit.id}`}
              renderSkeletonItem={renderSkeletonCard}
              renderItem={(hit) => (
                <MediaCard
                  plain
                  title={hit.name}
                  imageSrc={hit.picture_url}
                  coverMarks={[{ source: hit.source, label: sourceMarkName(hit.source) }]}
                  meta={
                    <span className="muted">
                      {hit.author || hit.category || sourceMarkName(hit.source)}
                      {hit.downloads != null ? ` · ${formatCount(hit.downloads)}` : ""}
                    </span>
                  }
                  onClick={() => void openModDetail(hit)}
                />
              )}
            />
            {loadingMore ? <BrowseGridSkeleton rows={1} minCardPx={160} /> : null}
            <div ref={sentinelRef} className="browse-sentinel" aria-hidden="true" />
          </>
        )
      ) : colItems.length === 0 ? (
        <p className="muted">No collections matched your search.</p>
      ) : (
        <>
          <BrowseVirtualGrid
            items={colItems}
            scrollRef={scrollRef}
            minCardPx={160}
            fixedRowHeight
            staggerEnter
            staggerKey={`${activeGame.id}:${mode}:${sourceFilter}:${sort}:${submittedQuery}`}
            fillTrailingSkeletons={Boolean(colPage?.has_more)}
            trackScrollMargin
            onPageSize={handlePageSize}
            getKey={(hit) => `${hit.source ?? "nexus"}:${hit.slug}`}
            renderSkeletonItem={renderSkeletonCard}
            renderItem={(hit) => {
              const src = hit.source ?? "nexus";
              return (
                <MediaCard
                  plain
                  title={hit.name}
                  imageSrc={hit.tile_image_url ?? null}
                  coverMarks={[{ source: src, label: sourceMarkName(src) }]}
                  meta={
                    <span className="muted">
                      {hit.mod_count != null
                        ? `${hit.mod_count} mods`
                        : hit.author || "Collection"}
                    </span>
                  }
                  onClick={() => void openCollectionDetail(hit)}
                />
              );
            }}
          />
          {loadingMore ? <BrowseGridSkeleton rows={1} minCardPx={160} /> : null}
          <div ref={sentinelRef} className="browse-sentinel" aria-hidden="true" />
        </>
      )}
    </div>
  );
}
