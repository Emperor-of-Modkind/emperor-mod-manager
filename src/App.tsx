import {
  memo,
  startTransition,
  useCallback,
  useDeferredValue,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowDown,
  faArrowUp,
  faBars,
  faCaretDown,
  faCopy,
  faEye,
  faEyeSlash,
  faFileImport,
  faFilter,
  faGear,
  faLink,
  faMagnifyingGlass,
  faPlay,
  faPlus,
  faRightLeft,
  faStop,
  faTrashCan,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import { useReducedMotion } from "motion/react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import appLogo from "./assets/app-logo.png";
import { AccountConnections } from "./AccountConnections";
import {
  CollectionDetailView,
  CollectionStats,
  ContainedModsSection,
  EditableCollectionTitle,
} from "./CollectionDetailView";
import { CollectionsGameList } from "./CollectionsGameList";
import {
  CollectionSharingModal,
  formatVisibilityLabel,
  resolveCollectionVisibility,
  visibilityIcon,
} from "./CollectionSharingModal";
import { FriendsPage } from "./FriendsPage";
import { SettingsSection } from "./SettingsSection";
import { SocialSettingsSection } from "./SocialSettingsSection";
import { Throbber } from "./Throbber";
import { CheckingFriendUpdatesToast, CheckingUpdatesToast, UpdateToast } from "./UpdateToast";
import { api } from "./api";
import { setupCanFinish } from "./setupGate";
import {
  createQueueEntry,
  DownloadsWorkspace,
  formatBytes,
  type ActiveDownloadBatch,
  type AssistQueueEntry,
  type AssistQueueState,
} from "./downloads";
import { GameToolsPanel, ToolsErrorBoundary, ToolsWorkspace } from "./tools";
import {
  catalogFilterMismatchMessage,
  clampCatalogSourceFilter,
  type CatalogSourceFilter,
} from "./catalogSources";
import { SourceMark, sourceMarkName } from "./SourceMark";
import { ImageCarousel } from "./ImageCarousel";
import {
  NavGroup,
  NavTab,
  Collapse,
  PopIn,
  Reveal,
  Segment,
  SegmentTab,
  ViewSwitch,
  staggerStyle,
} from "./motion";
import { MediaCard, PlaceholderIcon, useDecodedImageReveal } from "./MediaCard";
import { RichText } from "./RichText";
import { BrowseVirtualGrid, BrowseVirtualList } from "./BrowseResults";
import {
  BrowseGridSkeleton,
  BrowseListSkeleton,
  BrowseResultsSkeleton,
  CollectionDetailSkeleton,
  ModDetailSkeleton,
  StagedModsSkeleton,
} from "./Skeleton";
import { ResultsReveal, useReveal } from "./reveal";
import {
  ParkedTabGate,
  TtlTracker,
  createThrottledBatcher,
  shallowRecordOfObjectsEqual,
  stringSetEqual,
  useChunkedCount,
  useNavSettledEffect,
} from "./perf";
import {
  estimateListRowHeight,
  estimateStagedModRowHeight,
  isSentinelInScrollView,
  sentinelContentTop,
} from "./browseVirtual";
import { AppNavHistory, type NavEntry } from "./appNavHistory";
import { attachMouseNavigationListeners } from "./mouseNavigation";
import { browseGameVersionOptions } from "./gameVersion";
import {
  catalogHitFromModDeepLink,
  managedGameForModDeepLink,
  modShareUrlFromHit,
  type EmperorOpenMod,
} from "./modDeepLink";
import { chosenFileLabel, stagedModSourceLine } from "./stagedLabel";
import {
  reorderStagedMods,
  reorderStagedModToGroupIndex,
  sameEnabledNeighborIndex,
  stagedModsInLoadOrder,
} from "./stagedLoadOrder";
import { useStagedModDrag } from "./useStagedModDrag";
import { Modal } from "./Modal";
import {
  CatalogMatchDialog,
  type CatalogMatchEngine,
  type CatalogMatchSubmit,
} from "./CatalogMatchDialog";
import { LoadLogDialog } from "./LoadLogDialog";
import { FomodConfigureBanner, FomodDialog } from "./FomodDialog";
import {
  cancelFomodPass,
  finishFomodPass,
  fomodModsForDeploy,
  fomodPassCurrent,
  fomodPassProgress,
  startFomodPass,
  type FomodPass,
} from "./fomodQueue";
import { ModOptionsDialog } from "./ModOptionsDialog";
import { MoveModCollectionDialog } from "./MoveModCollectionDialog";
import { QuickInstallDialog } from "./QuickInstallDialog";
import {
  disabledRequirementWarnings,
  IgnoredRequirementsDialog,
  MissingRequirementsStrip,
  RequirementsDialog,
  requirementsNeedAttention,
  withoutIgnoredDisabled,
  withoutIgnoredRequirements,
  withoutIgnoredVersions,
} from "./RequirementsDialog";
import {
  StatusBanner,
  bannerPersistenceFor,
  bannerTimeoutFor,
  deployStatusNotice,
  dismissBanner,
  dismissBannersForDownload,
  filterPersistableBanners,
  pruneExpiredBanners,
  pushBanner,
  remainingBannerTimeoutSeconds,
  type BannerAction,
  type BannerKind,
  type StackedBanner,
  type StatusNotice,
} from "./StatusBanner";
import { WineOverrideNotice } from "./WineOverrideNotice";
import { CollectionIntegrityDialog } from "./CollectionIntegrityDialog";
import {
  applyCatalogCovers,
  catalogCoverRequestKey,
  catalogCoverRequests,
  collectionCatalogCoverRequests,
  mergeCollectionCatalogCovers,
  mergeCollectionCatalogTitles,
} from "./catalogCovers";
import { nexusModUnavailable, nexusUnavailableNotice } from "./nexusAvailability";
import type {
  AuthorSuggestion,
  BrowseMeta,
  BrowseSearchOpts,
  CatalogHit,
  CatalogSuggestion,
  CollectionDetail,
  CollectionHit,
  CollectionIntegrityReport,
  CollectionModFile,
  CollectionModIssue,
  CollectionVisibility,
  DetectedGame,
  DownloadItem,
  FriendsState,
  GameHealth,
  GameInfo,
  GameVersionReport,
  IgnoredRequirement,
  InstalledCollection,
  FriendCollectionSyncStatus,
  ManagedGame,
  ModDetail,
  ModFileInfo,
  MissingRequirements,
  LoadDiagnosis,
  NexusRequirementDownload,
  Settings,
  StagedMod,
  StagedModUpdate,
  WineOverrideAdvice,
  TagFilterState,
  ThemePreference,
  ThemeStyle,
  LinuxSystemBackend,
  SystemPalette,
  InstallClickBehavior,
  TsPackageDetail,
  ModioModDetail,
  ModioFileInfo,
  OrphanScan,
  CollectionVersionAdvice,
  GamePluginInfo,
  CatalogSource,
  SavedCollectionEntry,
  SavedCollectionDetail,
  ShareImportResult,
  ShareModEntry,
  AppInfo,
  AppInstallVia,
  AppUpdateStatus,
  GameLaunchState,
  LaunchCommandSource,
  LaunchEnvVar,
  LaunchPreview,
} from "./types";
import {
  formatCollectionGameVersions,
  groupSavedCollectionsByGame,
  savedCollectionGameKey,
  savedCollectionMatchesGame,
} from "./savedCollectionGroups";
import "./App.css";
import { accentInk, contentRoles } from "./contrast";

type Tab = "setup" | "library" | "collections" | "friends" | "browse" | "downloads" | "tools" | "settings";
type ViewMode = "list" | "grid";
type BrowseListDensity = "compact" | "posters";
type DetailTab = "info" | "files";
type BrowseTagMap = Record<string, TagFilterState>;

const BROWSE_GRID_MIN_PX = 120;
const BROWSE_GRID_MAX_PX = 280;
const BROWSE_GRID_DEFAULT_PX = 160;

function clampBrowseGridSize(size: number): number {
  if (!Number.isFinite(size)) return BROWSE_GRID_DEFAULT_PX;
  return Math.min(BROWSE_GRID_MAX_PX, Math.max(BROWSE_GRID_MIN_PX, Math.round(size)));
}

type SaveImportPrompt = {
  gameId: string;
  code: string;
  name?: string | null;
  previewName?: string | null;
  modCount?: number;
};

const EMPTY_IGNORED: IgnoredRequirement[] = [];

const UNREAL_PLUGIN_IDS = new Set([
  "unreal",
  "stalker2heartofchornobyl",
  "palworld",
  "hogwartslegacy",
  "readyornot",
  "subnautica2",
]);

function isUnrealPlugin(pluginId: string): boolean {
  return UNREAL_PLUGIN_IDS.has(pluginId);
}

const CATALOG_HIGH_CONFIDENCE_SCORE = 90;

function catalogHasHit(s: CatalogSuggestion | null | undefined): boolean {
  return Boolean(
    s?.nexus_domain ||
      s?.thunderstore_community ||
      (s?.modio_game_id != null && s.modio_game_id > 0),
  );
}

function catalogBestScore(s: CatalogSuggestion | null | undefined): number {
  if (!s) {
    return 0;
  }
  const scores = [
    ...(s.nexus_candidates ?? []).map((c) => c.score),
    ...(s.thunderstore_candidates ?? []).map((c) => c.score),
    ...(s.modio_candidates ?? []).map((c) => c.score),
  ];
  if (scores.length === 0) {
    return catalogHasHit(s) ? CATALOG_HIGH_CONFIDENCE_SCORE : 0;
  }
  return Math.max(...scores);
}

/** One-click manage only when a unique high-confidence match exists. */
function catalogReadyToQuickManage(s: CatalogSuggestion | null | undefined): boolean {
  return Boolean(
    s &&
      catalogHasHit(s) &&
      !s.ambiguous &&
      catalogBestScore(s) >= CATALOG_HIGH_CONFIDENCE_SCORE,
  );
}

export type CatalogMatchCardKind = "ready" | "review" | "needs";

function catalogMatchCardKind(
  s: CatalogSuggestion | null | undefined,
): CatalogMatchCardKind {
  if (catalogReadyToQuickManage(s)) {
    return "ready";
  }
  if (catalogHasHit(s) || (s?.ambiguous && catalogBestScore(s) > 0)) {
    return "review";
  }
  return "needs";
}

function catalogMatchBadge(kind: CatalogMatchCardKind): string | null {
  if (kind === "needs") return "Needs catalog";
  if (kind === "review") return "Review match";
  return null;
}

function catalogMatchHover(kind: CatalogMatchCardKind): string {
  if (kind === "ready") return "Manage";
  if (kind === "review") return "Choose catalog";
  return "Find catalog";
}

function catalogHints(
  s: CatalogSuggestion | null | undefined,
): { source: CatalogSource; label: string }[] {
  if (!s) {
    return [];
  }
  const tags: { source: CatalogSource; label: string }[] = [];
  if (s.nexus_name || s.nexus_domain) {
    tags.push({
      source: "nexus",
      label: `Nexus: ${s.nexus_name ?? s.nexus_domain}`,
    });
  }
  if (s.thunderstore_name || s.thunderstore_community) {
    tags.push({
      source: "thunderstore",
      label: `Thunderstore: ${s.thunderstore_name ?? s.thunderstore_community}`,
    });
  }
  if (s.modio_name || (s.modio_game_id != null && s.modio_game_id > 0)) {
    tags.push({
      source: "modio",
      label: `mod.io: ${s.modio_name ?? String(s.modio_game_id)}`,
    });
  }
  return tags;
}

function blankCatalogHit(source: CatalogHit["source"], id: string, name: string): CatalogHit {
  return {
    source,
    id,
    name,
    summary: null,
    picture_url: null,
    author: null,
    downloads: null,
    endorsements: null,
    category: null,
    tags: [],
    mod_id: null,
    domain_name: null,
    community: null,
    namespace: null,
    package_name: null,
    full_name: null,
    package_url: null,
    rating_score: null,
    latest_version: null,
    modio_game_id: null,
    modio_mod_id: null,
    profile_url: null,
  };
}

/** Fill sparse catalog hits (e.g. deep links) from a loaded Nexus mod detail. */
export function enrichCatalogHitFromNexus(hit: CatalogHit, detail: ModDetail): CatalogHit {
  return {
    ...hit,
    name: detail.name || hit.name,
    summary: detail.summary ?? hit.summary,
    picture_url: detail.picture_url ?? hit.picture_url,
    author: detail.author ?? hit.author,
    downloads: detail.downloads ?? hit.downloads,
    endorsements: detail.endorsements ?? hit.endorsements,
    category: detail.category ?? hit.category,
    domain_name: detail.domain_name || hit.domain_name,
    mod_id: detail.mod_id || hit.mod_id,
  };
}

/** Fill sparse catalog hits from a loaded Thunderstore package (mirrors catalog_from_ts). */
export function enrichCatalogHitFromThunderstore(
  hit: CatalogHit,
  detail: TsPackageDetail,
): CatalogHit {
  return {
    ...hit,
    name: detail.name || hit.name,
    summary: detail.description ?? hit.summary,
    picture_url: detail.icon_url ?? hit.picture_url,
    author: detail.namespace || hit.author,
    downloads: detail.downloads ?? hit.downloads,
    category: detail.categories[0] ?? hit.category,
    tags: detail.categories.length > 0 ? detail.categories : hit.tags,
    community: detail.community || hit.community,
    namespace: detail.namespace || hit.namespace,
    package_name: detail.name || hit.package_name,
    full_name: detail.full_name || hit.full_name,
    package_url: detail.package_url || hit.package_url,
    rating_score: detail.rating_score ?? hit.rating_score,
    latest_version: detail.latest_version ?? hit.latest_version,
  };
}

/** Fill sparse catalog hits from a loaded mod.io mod detail. */
export function enrichCatalogHitFromModio(hit: CatalogHit, detail: ModioModDetail): CatalogHit {
  return {
    ...hit,
    name: detail.name || hit.name,
    summary: detail.summary || hit.summary,
    picture_url: detail.picture_url ?? hit.picture_url,
    author: detail.author ?? hit.author,
    downloads: detail.downloads ?? hit.downloads,
    tags: detail.tags.length > 0 ? detail.tags : hit.tags,
    modio_game_id: detail.game_id || hit.modio_game_id,
    modio_mod_id: detail.mod_id || hit.modio_mod_id,
    profile_url: detail.profile_url || hit.profile_url,
  };
}

/** Catalog page for a staged mod, or null when it has no Nexus / Thunderstore / mod.io id. */
export function catalogHitFromStaged(mod: StagedMod, game: ManagedGame): CatalogHit | null {
  const source = mod.source ?? "nexus";
  if (source === "thunderstore") {
    const community = mod.domain || game.thunderstore_community || "";
    if (!community || !mod.ts_namespace || !mod.ts_name) return null;
    return {
      ...blankCatalogHit(
        "thunderstore",
        `thunderstore:${community}:${mod.ts_namespace}:${mod.ts_name}`,
        mod.name,
      ),
      community,
      namespace: mod.ts_namespace,
      package_name: mod.ts_name,
      full_name: `${mod.ts_namespace}-${mod.ts_name}`,
      latest_version: mod.version,
    };
  }
  if (source === "modio") {
    if (mod.modio_game_id == null || mod.modio_mod_id == null) return null;
    return {
      ...blankCatalogHit("modio", `modio:${mod.modio_game_id}:${mod.modio_mod_id}`, mod.name),
      modio_game_id: mod.modio_game_id,
      modio_mod_id: mod.modio_mod_id,
      latest_version: mod.version,
    };
  }
  const domain = mod.domain || game.nexus_domain;
  if (!domain || !mod.nexus_mod_id) return null;
  return {
    ...blankCatalogHit("nexus", `nexus:${domain}:${mod.nexus_mod_id}`, mod.name),
    mod_id: mod.nexus_mod_id,
    domain_name: domain,
    latest_version: mod.version,
  };
}

/** Catalog page for a recent download, or null when the row has no catalog id. */
export function catalogHitFromDownload(
  item: Pick<DownloadItem, "label" | "page">,
): CatalogHit | null {
  const page = item.page;
  if (!page) return null;
  if (page.source === "thunderstore") {
    if (!page.community || !page.namespace || !page.package_name) return null;
    return {
      ...blankCatalogHit(
        "thunderstore",
        `thunderstore:${page.community}:${page.namespace}:${page.package_name}`,
        item.label,
      ),
      community: page.community,
      namespace: page.namespace,
      package_name: page.package_name,
      full_name: `${page.namespace}-${page.package_name}`,
    };
  }
  if (page.source === "modio") {
    if (!page.modio_game_id || !page.modio_mod_id) return null;
    return {
      ...blankCatalogHit(
        "modio",
        `modio:${page.modio_game_id}:${page.modio_mod_id}`,
        item.label,
      ),
      modio_game_id: page.modio_game_id,
      modio_mod_id: page.modio_mod_id,
    };
  }
  if (!page.domain || !page.mod_id) return null;
  return {
    ...blankCatalogHit("nexus", `nexus:${page.domain}:${page.mod_id}`, item.label),
    mod_id: page.mod_id,
    domain_name: page.domain,
  };
}

/** Catalog ids for staged mods, matching browse hit ids. Enabled and disabled both count. */
export function installedCatalogIds(mods: StagedMod[], game: ManagedGame): Set<string> {
  const ids = new Set<string>();
  for (const mod of mods) {
    const hit = catalogHitFromStaged(mod, game);
    if (hit) ids.add(hit.id);
  }
  return ids;
}

/** Staged rows for a browse catalog id. Several files of one mod all match. */
export function stagedModsForCatalogId(
  mods: StagedMod[],
  game: ManagedGame,
  catalogId: string,
): StagedMod[] {
  return mods.filter((mod) => catalogHitFromStaged(mod, game)?.id === catalogId);
}

/** Nexus / mod.io file ids already staged for a browse catalog id. */
export function installedFileIdsForCatalog(
  mods: StagedMod[],
  game: ManagedGame,
  catalogId: string,
): Set<number> {
  const ids = new Set<number>();
  for (const mod of stagedModsForCatalogId(mods, game, catalogId)) {
    const source = mod.source ?? "nexus";
    if (source === "modio") {
      if (mod.modio_file_id != null && mod.modio_file_id > 0) {
        ids.add(mod.modio_file_id);
      }
    } else if (source === "nexus" && mod.nexus_file_id > 0) {
      ids.add(mod.nexus_file_id);
    }
  }
  return ids;
}


type BrowseDetail =
  | { kind: "mod"; hit: CatalogHit; tab: DetailTab }
  | { kind: "collection"; hit: CollectionHit; tab: DetailTab };

/** Cycle: unset → include → exclude → unset */
function cycleTagState(current: TagFilterState | undefined): TagFilterState | undefined {
  if (!current) return "include";
  if (current === "include") return "exclude";
  return undefined;
}

function tagListsFromMap(map: BrowseTagMap): {
  tagsInclude: string[];
  tagsExclude: string[];
} {
  const tagsInclude: string[] = [];
  const tagsExclude: string[] = [];
  for (const [tag, state] of Object.entries(map)) {
    if (state === "include") tagsInclude.push(tag);
    else if (state === "exclude") tagsExclude.push(tag);
  }
  return { tagsInclude, tagsExclude };
}

/** If `q` starts with `@`, return the author prefix (may be empty while typing). */
function parseAuthorQuery(q: string): string | null {
  const trimmed = q.trimStart();
  if (!trimmed.startsWith("@")) return null;
  return trimmed.slice(1).trimStart().replace(/\s+$/, "");
}

function setTagInMap(map: BrowseTagMap, tag: string): BrowseTagMap {
  const next = { ...map };
  const cycled = cycleTagState(map[tag]);
  if (!cycled) delete next[tag];
  else next[tag] = cycled;
  return next;
}

type StatusSummary = {
  primary: string;
  secondary: string;
};

function statusSummary(
  busy: string | null,
  assistQueue: AssistQueueState | null,
  downloads: DownloadItem[],
  activeBatch: ActiveDownloadBatch | null,
): StatusSummary | null {
  if (assistQueue) {
    const total = assistQueue.entries.length;
    const progress = `${Math.min(assistQueue.head + 1, total)}/${total}`;
    const entry = assistQueue.entries[assistQueue.head];

    if (entry) {
      let primary = "Download Assist";
      if (busy?.startsWith("Starting")) primary = "Starting";
      else if (busy?.startsWith("Importing")) primary = "Importing";
      else if (busy?.startsWith("Waiting for Nexus")) primary = "Sign in";
      else if (busy && !busy.startsWith("Download Assist")) {
        const word = busy.split(/\s+/)[0]?.replace(/…$/, "");
        if (word) primary = word;
      }
      return { primary, secondary: `${entry.name} · ${progress}` };
    }

    return { primary: "Queue", secondary: `${progress} — opening next…` };
  }

  if (busy) {
    const patterns: [RegExp, string][] = [
      [/^Downloading\s+(.+?)…?$/i, "Downloading"],
      [/^Installing\s+(?:collection\s+)?(.+?)…?$/i, "Installing"],
      [/^Loading\s+(?:collection\s+)?(.+?)…?$/i, "Loading"],
      [/^Download Assist\s*[—\-]\s*(.+)$/i, "Download Assist"],
    ];
    for (const [re, primary] of patterns) {
      const match = busy.match(re);
      if (match?.[1]) return { primary, secondary: match[1] };
    }

    const space = busy.indexOf(" ");
    if (space > 0) {
      return {
        primary: busy.slice(0, space).replace(/…$/, ""),
        secondary: busy.slice(space + 1).replace(/…$/, ""),
      };
    }
    return { primary: busy, secondary: "" };
  }

  const inFlight = downloads.filter(
    (d) => d.status === "downloading" ||
              d.status === "extracting" ||
              d.status === "paused",
  );
  if (inFlight.length === 0) return null;

  let secondary: string;
  if (inFlight.length === 1) {
    secondary = inFlight[0].label;
  } else if (activeBatch?.label) {
    secondary = activeBatch.label;
  } else {
    secondary = `${inFlight.length} remaining`;
  }
  return { primary: "Downloading", secondary };
}

const MOD_SORTS = [
  { value: "endorsements", label: "Most popular" },
  { value: "downloads", label: "Most downloaded" },
  { value: "createdAt", label: "Newest" },
  { value: "updatedAt", label: "Recently updated" },
  { value: "relevance", label: "Best match" },
  { value: "trending", label: "Trending" },
] as const;

const COLLECTION_SORTS = [
  { value: "endorsements", label: "Most popular" },
  { value: "downloads", label: "Most downloaded" },
  { value: "createdAt", label: "Newest" },
  { value: "updatedAt", label: "Recently updated" },
  { value: "relevance", label: "Best match" },
  { value: "rating", label: "Highest rated" },
] as const;

type ModsStatusFilter = "all" | "enabled" | "disabled";
type ModsSort = "loadOrder" | "nameAsc" | "nameDesc";

/** Sentinel for the staging Collection filter: mods with no collection membership. */
const MODS_COLLECTION_FILTER_NONE = "__none__";

const STAGED_MOD_SORTS = [
  { value: "loadOrder", label: "Load order" },
  { value: "nameAsc", label: "Name A–Z" },
  { value: "nameDesc", label: "Name Z–A" },
] as const;

function collectionHitSource(c: CollectionHit): "nexus" | "thunderstore" {
  return c.source ?? "nexus";
}

function collectionHitKey(c: CollectionHit): string {
  return c.id || `${collectionHitSource(c)}:${c.slug}`;
}

function stagedModSource(m: StagedMod): "nexus" | "thunderstore" | "modio" {
  return m.source ?? "nexus";
}

function formatInstalledAt(iso: string | null | undefined): string | null {
  if (!iso?.trim()) return null;
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return null;
    return `Installed ${d.toLocaleString()}`;
  } catch {
    return null;
  }
}

function installedCollectionSourceLabel(c: InstalledCollection): string {
  if (c.source === "emperor") return "Emperor share";
  if (c.source === "thunderstore") {
    return c.kind === "profile" ? "Thunderstore profile" : "Thunderstore modpack";
  }
  return "Nexus collection";
}

function collectionsForMod(
  mod: StagedMod,
  installedCollections: InstalledCollection[],
): { id: string; name: string }[] {
  if (!mod.collection_ids?.length || installedCollections.length === 0) return [];
  const byId = new Map(installedCollections.map((c) => [c.id, c.name]));
  const out: { id: string; name: string }[] = [];
  for (const id of mod.collection_ids) {
    const name = byId.get(id);
    if (name) out.push({ id, name });
  }
  return out;
}

function stagedModSearchText(m: StagedMod, collectionNames: string[] = []): string {
  return [
    m.name,
    m.version ?? "",
    stagedModSource(m),
    String(m.nexus_mod_id),
    String(m.nexus_file_id),
    m.ts_namespace ?? "",
    m.ts_name ?? "",
    m.modio_mod_id != null ? String(m.modio_mod_id) : "",
    m.modio_file_id != null ? String(m.modio_file_id) : "",
    ...collectionNames,
  ]
    .join(" ")
    .toLowerCase();
}

function filterAndSortStagedMods(
  list: StagedMod[],
  opts: {
    query: string;
    status: ModsStatusFilter;
    source: CatalogSourceFilter;
    collectionId: string;
    sort: ModsSort;
    collectionNamesByModId?: Map<string, string[]>;
  },
): StagedMod[] {
  const q = opts.query.trim().toLowerCase();
  let filtered = list;
  if (q) {
    filtered = filtered.filter((m) =>
      stagedModSearchText(
        m,
        opts.collectionNamesByModId?.get(m.id) ?? [],
      ).includes(q),
    );
  }
  if (opts.status === "enabled") {
    filtered = filtered.filter((m) => m.enabled);
  } else if (opts.status === "disabled") {
    filtered = filtered.filter((m) => !m.enabled);
  }
  if (opts.source !== "all") {
    filtered = filtered.filter((m) => stagedModSource(m) === opts.source);
  }
  if (opts.collectionId === MODS_COLLECTION_FILTER_NONE) {
    filtered = filtered.filter((m) => !(m.collection_ids ?? []).length);
  } else if (opts.collectionId) {
    filtered = filtered.filter((m) =>
      (m.collection_ids ?? []).includes(opts.collectionId),
    );
  }

  const sortPartition = (partition: StagedMod[]) => {
    const next = [...partition];
    if (opts.sort === "nameAsc") {
      next.sort(
        (a, b) =>
          a.name.localeCompare(b.name, undefined, { sensitivity: "base" }) ||
          a.order - b.order,
      );
    } else if (opts.sort === "nameDesc") {
      next.sort(
        (a, b) =>
          b.name.localeCompare(a.name, undefined, { sensitivity: "base" }) ||
          a.order - b.order,
      );
    } else {
      next.sort((a, b) => a.order - b.order);
    }
    return next;
  };

  if (opts.status === "enabled" || opts.status === "disabled") {
    return sortPartition(filtered);
  }
  return [
    ...sortPartition(filtered.filter((m) => m.enabled)),
    ...sortPartition(filtered.filter((m) => !m.enabled)),
  ];
}

const DEFAULT_ACCENT = "#0078d4";
const ACCENT_PRESETS = [
  "#0078d4",
  "#8764b8",
  "#038387",
  "#00cc6a",
  "#c239b3",
  "#e74856",
  "#0f6a5c",
  "#d35400",
] as const;

const SYSTEM_PALETTE_VARS = [
  "--bg",
  "--bg-2",
  "--panel",
  "--ink",
  "--muted",
  "--line",
  "--accent",
  "--accent-2",
  "--input-bg",
] as const;

function resolveTheme(pref: ThemePreference): "light" | "dark" {
  if (pref === "light" || pref === "dark") return pref;
  if (typeof window === "undefined" || typeof window.matchMedia !== "function") {
    return "dark";
  }
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function normalizeAccent(raw: string | undefined | null): string {
  const s = (raw ?? "").trim();
  const hex = s.startsWith("#") ? s.slice(1) : s;
  if (/^[0-9a-fA-F]{6}$/.test(hex)) return `#${hex.toLowerCase()}`;
  return DEFAULT_ACCENT;
}

function clearInlinePaletteVars() {
  const root = document.documentElement;
  for (const key of SYSTEM_PALETTE_VARS) {
    root.style.removeProperty(key);
  }
}

function applySystemPaletteVars(palette: SystemPalette) {
  const root = document.documentElement;
  root.style.setProperty("--bg", palette.bg);
  root.style.setProperty("--bg-2", palette.bg_2);
  root.style.setProperty("--panel", palette.panel);
  root.style.setProperty("--ink", palette.ink);
  root.style.setProperty("--muted", palette.muted);
  root.style.setProperty("--line", palette.line);
  root.style.setProperty("--accent", palette.accent);
  root.style.setProperty(
    "--accent-2",
    `color-mix(in srgb, ${palette.accent} 72%, #ff8c00)`,
  );
  root.style.setProperty("--input-bg", palette.input_bg);
}

function readToken(name: string): string {
  return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
}

/** Resolve a token that may itself be another variable, such as the system sidebar. */
function usedColor(name: string, fallback: string): string {
  const specified = readToken(name);
  if (specified && !specified.includes("var(") && !specified.includes("color-mix")) {
    return specified;
  }
  if (!document.body) return fallback;
  const probe = document.createElement("span");
  probe.style.color = `var(${name})`;
  document.body.appendChild(probe);
  const computed = getComputedStyle(probe).color;
  probe.remove();
  return computed || fallback;
}

/** On-accent, accent-as-text, and a muted floor from the tokens now on :root. */
function applyContentRoles(osOnAccent?: string | null) {
  const accent = readToken("--accent");
  const bg = readToken("--bg");
  const panel = readToken("--panel");
  const ink = readToken("--ink");
  const muted = readToken("--muted");
  if (!accent || !bg || !panel || !ink || !muted) return;
  const roles = contentRoles({
    accent,
    bg,
    panel,
    ink,
    muted,
    osOnAccent,
  });
  const root = document.documentElement;
  root.style.setProperty("--on-accent", roles.onAccent);
  root.style.setProperty("--accent-ink", roles.accentInk);
  root.style.setProperty("--accent-hover", roles.accentHover);
  root.style.setProperty("--logo", accentInk(accent, [usedColor("--sidebar-bg", bg)]));
  if (roles.muted.toLowerCase() !== muted.toLowerCase()) {
    root.style.setProperty("--muted", roles.muted);
  }
}

async function applyAppearance(
  pref: ThemePreference,
  style: ThemeStyle,
  accentRaw?: string | null,
) {
  const root = document.documentElement;
  const accent = normalizeAccent(accentRaw);
  root.dataset.themeStyle = style;

  if (style === "system") {
    try {
      const palette = await api.getSystemPalette();
      const resolved =
        pref === "system" ? (palette.dark ? "dark" : "light") : resolveTheme(pref);
      root.dataset.theme = resolved;
      applySystemPaletteVars(palette);
      applyContentRoles(palette.on_accent);
      return;
    } catch {
      /* fall through to app monochrome */
    }
  }

  root.dataset.theme = resolveTheme(pref);
  clearInlinePaletteVars();
  if (style === "app") {
    root.style.setProperty("--accent", accent);
    root.style.setProperty(
      "--accent-2",
      `color-mix(in srgb, ${accent} 72%, #ff8c00)`,
    );
  }
  applyContentRoles(null);
}

// Match config defaults before settings load to avoid a Brand-palette flash.
if (typeof document !== "undefined") {
  document.documentElement.dataset.themeStyle = "app";
  document.documentElement.dataset.theme = resolveTheme("system");
  document.documentElement.style.setProperty("--accent", DEFAULT_ACCENT);
  document.documentElement.style.setProperty(
    "--accent-2",
    `color-mix(in srgb, ${DEFAULT_ACCENT} 72%, #ff8c00)`,
  );
  applyContentRoles(null);
}

function mediaSrc(localPath?: string | null, remoteUrl?: string | null): string | null {
  if (remoteUrl) return remoteUrl;
  if (localPath) {
    try {
      return convertFileSrc(localPath);
    } catch {
      return null;
    }
  }
  return null;
}

function posterSrc(value?: string | null): string | null {
  if (!value) return null;
  if (/^https?:\/\//i.test(value)) return value;
  return mediaSrc(value);
}

type GameArt = {
  cover_path?: string | null;
  cover_url?: string | null;
  icon_path?: string | null;
};

/** Local poster, then a remote catalog/store poster, then a launcher icon. */
function gameCoverSrc(game?: GameArt | null): string | null {
  if (!game) return null;
  const local = mediaSrc(game.cover_path);
  if (local) return local;
  if (game.cover_url) return game.cover_url;
  return mediaSrc(game.icon_path);
}

function managedArt(game: DetectedGame): {
  coverPath: string | null;
  coverUrl: string | null;
} {
  return {
    coverPath: game.cover_path ?? (game.cover_url ? null : game.icon_path ?? null),
    coverUrl: game.cover_url ?? null,
  };
}

function shortLauncher(launcher: string): string {
  const l = launcher.toLowerCase();
  if (l.includes("steam")) return "Steam";
  if (l.includes("heroic")) return "Heroic";
  if (l.includes("epic")) return "Epic";
  if (l.includes("lutris")) return "Lutris";
  if (l.includes("bottles")) return "Bottles";
  return launcher.split("(")[0]?.trim() || launcher;
}

const ListPoster = memo(function ListPoster({
  src,
  decoding,
}: {
  src: string | null;
  decoding?: "async";
}) {
  const {
    imgRef,
    activeSrc,
    status,
    showSkeleton,
    handleImgLoad,
    markFailed,
    handleTransitionEnd,
  } = useDecodedImageReveal(src);

  return (
    <div className="list-poster" aria-hidden="true">
      {showSkeleton ? (
        <div className="list-poster-skeleton skeleton" aria-hidden="true" />
      ) : null}
      {src && status !== "error" ? (
        activeSrc ? (
          <img
            ref={imgRef}
            key={activeSrc}
            className={status === "ready" ? "cover-defer is-ready" : "cover-defer"}
            src={activeSrc}
            alt=""
            decoding={decoding ?? "async"}
            onLoad={handleImgLoad}
            onError={markFailed}
            onTransitionEnd={handleTransitionEnd}
          />
        ) : null
      ) : (
        <PlaceholderIcon />
      )}
    </div>
  );
});

function openExternal(url: string) {
  void (async () => {
    try {
      await openUrl(url);
    } catch (e) {
      console.error("Failed to open URL", e);
    }
  })();
}

function openPathInExplorer(path: string) {
  void (async () => {
    try {
      await revealItemInDir(path);
    } catch (e) {
      console.error("Failed to open path", e);
    }
  })();
}

function formatTimestamp(ts: number | null | undefined): string | null {
  if (ts == null || ts <= 0) return null;
  try {
    return new Date(ts * 1000).toLocaleDateString();
  } catch {
    return null;
  }
}

function formatIsoDate(iso: string | null | undefined): string | null {
  if (!iso) return null;
  try {
    const d = new Date(iso);
    if (Number.isNaN(d.getTime())) return null;
    return d.toLocaleDateString();
  } catch {
    return null;
  }
}

function formatVersionLabel(version: string | null | undefined): string | null {
  const trimmed = version?.trim();
  if (!trimmed) return null;
  return /^v/i.test(trimmed) ? trimmed : `v${trimmed}`;
}

function posterModDetail(hit: CatalogHit): string | null {
  const parts = [
    hit.category?.trim() || null,
    formatVersionLabel(hit.latest_version),
    hit.source === "thunderstore" && hit.rating_score != null
      ? `rating ${hit.rating_score.toLocaleString()}`
      : null,
  ].filter((part): part is string => Boolean(part));
  return parts.length > 0 ? parts.join(" · ") : null;
}

function posterCollectionDetail(hit: CollectionHit): string | null {
  const updated = formatIsoDate(hit.updated_at);
  let rating: string | null = null;
  if (hit.overall_rating != null) {
    rating = hit.overall_rating.toFixed(1);
    if (hit.overall_rating_count != null) {
      rating += ` · ${hit.overall_rating_count.toLocaleString()} ratings`;
    }
  }
  const revision =
    formatVersionLabel(hit.latest_version) ??
    (hit.revision_number != null ? `rev ${hit.revision_number}` : null);
  const parts = [
    hit.category?.trim() || null,
    rating,
    updated ? `updated ${updated}` : null,
    revision,
  ].filter((part): part is string => Boolean(part));
  return parts.length > 0 ? parts.join(" · ") : null;
}

function groupCollectionMods(files: CollectionModFile[]) {
  const map = new Map<
    number,
    { mod_id: number; mod_name: string; domain_name: string; files: CollectionModFile[] }
  >();
  for (const f of files) {
    const existing = map.get(f.mod_id);
    if (existing) {
      existing.files.push(f);
    } else {
      map.set(f.mod_id, {
        mod_id: f.mod_id,
        mod_name: f.mod_name,
        domain_name: f.domain_name,
        files: [f],
      });
    }
  }
  return [...map.values()];
}

function categoryKey(name: string | null | undefined): string {
  return (name ?? "").trim().toLowerCase();
}

function isHistoricalModFile(file: Pick<ModFileInfo, "category_name">): boolean {
  const key = categoryKey(file.category_name);
  return (
    key === "" ||
    key === "old_version" ||
    key === "old version" ||
    key === "archived"
  );
}

function isMainCategoryFile(file: Pick<ModFileInfo, "category_name">): boolean {
  return categoryKey(file.category_name) === "main";
}

function isOptionalCategoryFile(file: Pick<ModFileInfo, "category_name">): boolean {
  return categoryKey(file.category_name) === "optional";
}

export function pickMainModFile(files: ModFileInfo[]): ModFileInfo | null {
  const current = files.filter((file) => !isHistoricalModFile(file));
  return (
    current.find((file) => file.is_primary) ??
    current.find((file) => isMainCategoryFile(file)) ??
    current[0] ??
    null
  );
}

function NexusFileRow(props: {
  file: ModFileInfo;
  installLabel: string;
  installed?: boolean;
  onInstall: (file: ModFileInfo) => void;
}) {
  const { file, installLabel, installed, onInstall } = props;
  const description = file.description?.trim() || null;
  return (
    <li>
      <div>
        <strong>
          {file.name}
          {installed ? <span className="pill ok">Installed</span> : null}
        </strong>
        <small>
          {file.category_name ?? "file"}
          {file.version ? ` · v${file.version}` : ""}
          {file.is_primary ? " · primary" : ""}
          {file.size_kb != null ? ` · ${(file.size_kb / 1024).toFixed(1)} MB` : ""}
        </small>
        {description ? <p className="summary">{description}</p> : null}
      </div>
      <button type="button" onClick={() => onInstall(file)}>
        {installLabel}
      </button>
    </li>
  );
}

const HISTORICAL_FILES_PAGE_SIZE = 25;

export const NexusModFileList = memo(function NexusModFileList(props: {
  files: ModFileInfo[];
  installLabel: string;
  installedFileIds?: ReadonlySet<number>;
  onInstall: (file: ModFileInfo) => void;
}) {
  const { files, installLabel, installedFileIds, onInstall } = props;
  const [archiveLimit, setArchiveLimit] = useState(HISTORICAL_FILES_PAGE_SIZE);
  const { current, historical } = useMemo(() => {
    const cur: ModFileInfo[] = [];
    const hist: ModFileInfo[] = [];
    for (const file of files) {
      if (isHistoricalModFile(file)) hist.push(file);
      else cur.push(file);
    }
    return { current: cur, historical: hist };
  }, [files]);
  const visibleHistorical = useMemo(
    () => historical.slice(0, archiveLimit),
    [historical, archiveLimit],
  );
  return (
    <>
      {files.length === 0 ? (
        <ul className="list">
          <li className="empty">No files found for this mod.</li>
        </ul>
      ) : current.length > 0 ? (
        <ul className="list">
          {current.map((file) => (
            <NexusFileRow
              key={file.file_id}
              file={file}
              installLabel={installLabel}
              installed={installedFileIds?.has(file.file_id)}
              onInstall={onInstall}
            />
          ))}
        </ul>
      ) : null}
      {historical.length > 0 && (
        <details className="file-archive">
          <summary>Archived and old versions ({historical.length})</summary>
          <ul className="list">
            {visibleHistorical.map((file) => (
              <NexusFileRow
                key={file.file_id}
                file={file}
                installLabel={installLabel}
                installed={installedFileIds?.has(file.file_id)}
                onInstall={onInstall}
              />
            ))}
          </ul>
          {historical.length > visibleHistorical.length && (
            <div className="list-load-more">
              <button
                type="button"
                className="linkish"
                onClick={() => setArchiveLimit((n) => n + HISTORICAL_FILES_PAGE_SIZE)}
              >
                Show more ({historical.length - visibleHistorical.length} remaining)
              </button>
            </div>
          )}
        </details>
      )}
    </>
  );
});

function presentName(value: string | null | undefined): string | null {
  const trimmed = value?.trim();
  return trimmed ? trimmed : null;
}

function NexusUnavailableCallout({
  detail,
  files = false,
}: {
  detail: ModDetail | null;
  files?: boolean;
}) {
  const notice = nexusUnavailableNotice(detail, { files });
  if (!notice) return null;
  return (
    <div className="mod-unavailable">
      <strong>{notice.title}</strong>
      {notice.reason ? <p>{notice.reason}</p> : null}
      <p>{notice.body}</p>
      {notice.files ? <p>{notice.files}</p> : null}
    </div>
  );
}

function normalizeGameKey(title: string, launcher: string): string {
  return `${title.trim().toLowerCase()}|${launcher.trim().toLowerCase()}`;
}

function managedHealthBadge(health: GameHealth | undefined): string | null {
  if (!health || health.status === "ok") {
    return null;
  }
  if (health.status === "missing") {
    return "Install not found";
  }
  return "Moved — relink available";
}

const BrowseModGridCard = memo(function BrowseModGridCard({
  mod: m,
  sort,
  installed,
  isPremium,
  onOpen,
  onQuickInstall,
  onUninstall,
}: {
  mod: CatalogHit;
  sort?: string;
  installed: boolean;
  isPremium: boolean;
  onOpen: (mod: CatalogHit, tab: "info" | "files") => void;
  onQuickInstall: (mod: CatalogHit) => void;
  onUninstall: (mod: CatalogHit) => void;
}) {
  const handleOpenInfo = useCallback(() => onOpen(m, "info"), [m, onOpen]);
  const coverMarks = useMemo(
    () => [{ source: m.source, label: sourceMarkName(m.source) }],
    [m.source],
  );
  const overlay = useMemo(() => {
    if (sort === "downloads") {
      if (m.downloads != null) return `${m.downloads.toLocaleString()} downloads`;
      if (m.source === "nexus" && m.endorsements != null) {
        return `${m.endorsements.toLocaleString()} endorsements`;
      }
      return null;
    }
    if (m.source === "nexus" && m.endorsements != null) {
      return `${m.endorsements.toLocaleString()} endorsements`;
    }
    return m.downloads != null ? `${m.downloads.toLocaleString()} downloads` : null;
  }, [m.downloads, m.endorsements, m.source, sort]);
  const coverActions = useMemo(() => {
    if (m.source === "thunderstore") {
      return installed ? (
        <button
          type="button"
          className="danger"
          onClick={(e) => {
            e.stopPropagation();
            onUninstall(m);
          }}
        >
          Uninstall
        </button>
      ) : (
        <button
          onClick={(e) => {
            e.stopPropagation();
            onOpen(m, "info");
          }}
        >
          Install
        </button>
      );
    }
    if (m.source === "nexus") {
      return (
        <>
          {installed ? (
            <button
              type="button"
              className="danger"
              onClick={(e) => {
                e.stopPropagation();
                onUninstall(m);
              }}
            >
              Uninstall
            </button>
          ) : (
            <button
              type="button"
              onClick={(e) => {
                e.stopPropagation();
                onQuickInstall(m);
              }}
            >
              {isPremium ? "Install" : "Download Assist"}
            </button>
          )}
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onOpen(m, "files");
            }}
          >
            Files
          </button>
        </>
      );
    }
    return (
      <>
        {installed ? (
          <button
            type="button"
            className="danger"
            onClick={(e) => {
              e.stopPropagation();
              onUninstall(m);
            }}
          >
            Uninstall
          </button>
        ) : null}
        <button
          onClick={(e) => {
            e.stopPropagation();
            onOpen(m, "files");
          }}
        >
          Files
        </button>
      </>
    );
  }, [installed, isPremium, m, onOpen, onQuickInstall, onUninstall]);

  return (
    <MediaCard
      plain
      title={m.name}
      imageSrc={mediaSrc(null, m.picture_url)}
      badge={m.author ?? undefined}
      overlay={overlay}
      coverMarks={coverMarks}
      installed={installed}
      onClick={handleOpenInfo}
      coverActions={coverActions}
    />
  );
});

const BrowseCollectionGridCard = memo(function BrowseCollectionGridCard({
  collection: c,
  onOpen,
  onInstall,
}: {
  collection: CollectionHit;
  onOpen: (collection: CollectionHit, tab: "info" | "files") => void;
  onInstall: (collection: CollectionHit) => void;
}) {
  const source = collectionHitSource(c);
  const handleOpen = useCallback(() => onOpen(c, "info"), [c, onOpen]);
  const meta = useMemo(
    () => (
      <>
        <SourceMark source={source} />
        {c.mod_count != null ? ` · ${c.mod_count.toLocaleString()} mods` : ""}
        {c.file_size != null && c.file_size > 0
          ? ` · ${formatBytes(c.file_size)}`
          : ""}
      </>
    ),
    [c.file_size, c.mod_count, source],
  );
  const overlay = useMemo(
    () =>
      c.endorsements != null
        ? `${c.endorsements.toLocaleString()} endorsements`
        : c.total_downloads != null
          ? `${c.total_downloads.toLocaleString()} downloads`
          : null,
    [c.endorsements, c.total_downloads],
  );
  const coverActions = useMemo(
    () => (
      <button
        onClick={(e) => {
          e.stopPropagation();
          onInstall(c);
        }}
      >
        Install
      </button>
    ),
    [c, onInstall],
  );

  return (
    <MediaCard
      plain
      title={c.name}
      imageSrc={mediaSrc(null, c.tile_image_url)}
      badge={source === "thunderstore" ? "Modpack" : "Collection"}
      overlay={overlay}
      meta={meta}
      onClick={handleOpen}
      coverActions={coverActions}
    />
  );
});

function isActiveDownloadStatus(status: string): boolean {
  return status === "downloading" || status === "extracting" || status === "paused";
}

function isFinishedDownloadStatus(status: string): boolean {
  return (
    status === "done" ||
    status === "staged" ||
    status === "failed" ||
    status === "cancelled"
  );
}

function hasActiveDownloads(list: DownloadItem[]): boolean {
  return list.some((d) => isActiveDownloadStatus(d.status));
}

function measureHostBounds(host: HTMLElement): {
  x: number;
  y: number;
  width: number;
  height: number;
} | null {
  const rect = host.getBoundingClientRect();
  if (rect.width < 8 || rect.height < 8) return null;
  const cs = getComputedStyle(host);
  const bl = parseFloat(cs.borderLeftWidth) || 0;
  const bt = parseFloat(cs.borderTopWidth) || 0;
  const br = parseFloat(cs.borderRightWidth) || 0;
  const bb = parseFloat(cs.borderBottomWidth) || 0;

  let left = rect.left + bl;
  let top = rect.top + bt;
  let right = rect.right - br;
  let bottom = rect.bottom - bb;

  const clipsOverflow = (value: string) =>
    value === "hidden" || value === "clip" || value === "auto" || value === "scroll";

  let ancestor: HTMLElement | null = host.parentElement;
  while (ancestor) {
    const style = getComputedStyle(ancestor);
    if (
      clipsOverflow(style.overflow) ||
      clipsOverflow(style.overflowX) ||
      clipsOverflow(style.overflowY)
    ) {
      const a = ancestor.getBoundingClientRect();
      left = Math.max(left, a.left);
      top = Math.max(top, a.top);
      right = Math.min(right, a.right);
      bottom = Math.min(bottom, a.bottom);
    }
    ancestor = ancestor.parentElement;
  }

  left = Math.max(left, 0);
  top = Math.max(top, 0);
  right = Math.min(right, window.innerWidth);
  bottom = Math.min(bottom, window.innerHeight);

  const width = Math.max(0, Math.floor(right - left));
  const height = Math.max(0, Math.floor(bottom - top));
  if (width < 8 || height < 8) return null;
  return { x: Math.round(left), y: Math.round(top), width, height };
}

function installViaLabel(via: AppInstallVia): string {
  switch (via) {
    case "pacman":
      return "pacman";
    case "deb":
      return "apt";
    case "appimage":
      return "AppImage replacement";
    case "portable":
      return "a direct binary replacement";
    case "nsis":
      return "a silent Windows update";
  }
}

/** Short checks finish before this, so a small staged list never flashes the toast. */
const MOD_UPDATE_CHECK_TOAST_DELAY_MS = 400;

/** Re-scan installed games while the Library tab stays open. */
const LIBRARY_SCAN_INTERVAL_MS = 60_000;

function sameSnapshot(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}



function SavedCollectionDetailBody({
  entry,
  mods,
  showMods,
  scrollRef,
  active,
  onOpenMod,
}: {
  entry: SavedCollectionEntry;
  mods: ShareModEntry[];
  showMods: boolean;
  scrollRef: RefObject<HTMLElement | null>;
  active: boolean;
  onOpenMod?: (hit: CatalogHit) => void;
}) {
  return (
    <>
      <ContainedModsSection
        mods={mods}
        showMods={showMods}
        skeletonCount={entry.mod_count || 8}
        scrollRef={scrollRef}
        active={active}
        onOpenMod={onOpenMod}
      />
      <h2>Code</h2>
      <textarea
        className="saved-collection-code"
        rows={4}
        readOnly
        value={entry.code}
      />
    </>
  );
}

export default function App() {
  const [tab, setTabState] = useState<Tab | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [customBannerTimeouts, setCustomBannerTimeouts] = useState<
    Partial<Record<BannerKind, boolean>>
  >({});
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [appUpdate, setAppUpdate] = useState<AppUpdateStatus | null>(null);
  const [checkingAppUpdate, setCheckingAppUpdate] = useState(false);
  const [checkingModUpdates, setCheckingModUpdates] = useState(false);
  const [checkingFriendUpdates, setCheckingFriendUpdates] = useState(false);
  const [installingAppUpdate, setInstallingAppUpdate] = useState(false);
  const [appUpdateMessage, setAppUpdateMessage] = useState<string | null>(null);
  const [dismissedUpdateVersion, setDismissedUpdateVersion] = useState<string | null>(null);
  const [error, setErrorMessage] = useState<string | null>(null);
  const [banners, setBanners] = useState<StackedBanner[]>([]);
  const nextBannerIdRef = useRef(1);
  const sessionStartMsRef = useRef(Date.now());
  const bannersHydratedRef = useRef(false);
  const dismissBannerById = useCallback((id: number) => {
    setBanners((prev) => dismissBanner(prev, id));
  }, []);
  const dismissDownloadBanners = useCallback(
    (downloadId: string) => {
      if (settings?.dismiss_banner_on_retry === false) return;
      setBanners((prev) => dismissBannersForDownload(prev, downloadId));
    },
    [settings?.dismiss_banner_on_retry],
  );
  const setError = useCallback(
    (
      message: string | null,
      action?: BannerAction | null,
      downloadId?: string | null,
    ) => {
      setErrorMessage(message);
      if (message) {
        const id = nextBannerIdRef.current++;
        setBanners((prev) =>
          pushBanner(
            prev,
            {
              kind: "error",
              message,
              action: action ?? null,
              downloadId: downloadId ?? null,
              createdAt: Date.now(),
            },
            id,
          ),
        );
      }
    },
    [],
  );
  const setNotice = useCallback((next: StatusNotice | null) => {
    if (!next) {
      setBanners((prev) => prev.filter((b) => b.kind !== "info"));
      return;
    }
    const id = nextBannerIdRef.current++;
    setBanners((prev) =>
      pushBanner(
        prev,
        {
          kind: next.kind === "ok" ? "info" : "warn",
          message: next.message,
          details: next.details,
          action: next.action,
          createdAt: Date.now(),
        },
        id,
      ),
    );
  }, []);

  useEffect(() => {
    if (!settings || bannersHydratedRef.current) return;
    bannersHydratedRef.current = true;
    const persistence = bannerPersistenceFor(settings);
    if (persistence === "off") {
      void api.savePersistedBanners([]).catch(() => {});
      return;
    }
    let cancelled = false;
    void api
      .getPersistedBanners()
      .then((saved) => {
        if (cancelled || saved.length === 0) return;
        const restored: StackedBanner[] = saved
          .filter((b) =>
            persistence === "errors_and_warnings"
              ? b.kind === "error" || b.kind === "warn"
              : b.kind === "error",
          )
          .map((b) => ({
            id: b.id,
            kind: b.kind,
            message: b.message,
            details: b.details?.length ? b.details : undefined,
            downloadId: b.download_id ?? null,
            createdAt: b.created_at,
          }));
        const active = pruneExpiredBanners(restored, settings, sessionStartMsRef.current);
        if (active.length === 0) {
          void api.savePersistedBanners([]).catch(() => {});
          return;
        }
        const maxId = active.reduce((max, b) => Math.max(max, b.id), 0);
        nextBannerIdRef.current = Math.max(nextBannerIdRef.current, maxId + 1);
        setBanners((prev) => {
          let merged = [...active];
          for (const current of [...prev].reverse()) {
            merged = pushBanner(merged, current, current.id);
          }
          return merged;
        });
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [settings]);

  useEffect(() => {
    if (!settings || !bannersHydratedRef.current) return;
    const persistence = bannerPersistenceFor(settings);
    const toSave = filterPersistableBanners(banners, persistence);
    void api.savePersistedBanners(toSave).catch(() => {});
  }, [banners, settings]);
  const [busy, setBusy] = useState<string | null>(null);
  const [apiKeyInput, setApiKeyInput] = useState("");
  const [detected, setDetected] = useState<DetectedGame[]>([]);
  const [managed, setManaged] = useState<ManagedGame[]>([]);
  const [gameHealth, setGameHealth] = useState<GameHealth[]>([]);
  const [relinkPrompt, setRelinkPrompt] = useState<{
    game: ManagedGame;
    health: GameHealth;
  } | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [mods, setMods] = useState<StagedMod[]>([]);
  const [modsReadyGameId, setModsReadyGameId] = useState<string | null>(null);
  const [wineAdvice, setWineAdvice] = useState<WineOverrideAdvice | null>(null);
  const [wineApplyBusy, setWineApplyBusy] = useState(false);
  const [missingRequirements, setMissingRequirements] =
    useState<MissingRequirements | null>(null);
  const [requirementsModal, setRequirementsModal] =
    useState<MissingRequirements | null>(null);
  const [ignoredRequirementsOpen, setIgnoredRequirementsOpen] = useState(false);
  const [ignoreSaving, setIgnoreSaving] = useState(false);
  const [gameVersionReport, setGameVersionReport] =
    useState<GameVersionReport | null>(null);
  const [gameVersionDraft, setGameVersionDraft] = useState("");
  const [requirementScopeGameId, setRequirementScopeGameId] = useState<
    string | null
  >(null);
  const [quickInstall, setQuickInstall] = useState<{
    hit: CatalogHit;
    mains: ModFileInfo[];
    optionals: ModFileInfo[];
  } | null>(null);
  const [modUpdates, setModUpdates] = useState<Record<string, StagedModUpdate>>(
    {},
  );
  const [modsWithOptions, setModsWithOptions] = useState<Set<string>>(new Set());
  const [modOptionsFor, setModOptionsFor] = useState<StagedMod | null>(null);
  const [fomodNeedingIds, setFomodNeedingIds] = useState<string[]>([]);
  const [fomodDialog, setFomodDialog] = useState<{
    mod: StagedMod;
    pass: FomodPass<StagedMod> | null;
  } | null>(null);
  const fomodDialogRef = useRef(fomodDialog);
  const pendingUpdatedFomodIdsRef = useRef<Set<string>>(new Set());
  const openUpdatedFomodsRef = useRef<
    (list: StagedMod[], needing: string[], updatedIds: string[]) => void
  >(() => {});
  const [loadDiagnosis, setLoadDiagnosis] = useState<LoadDiagnosis | null>(null);
  const [modsQuery, setModsQuery] = useState("");
  const [modsStatusFilter, setModsStatusFilter] =
    useState<ModsStatusFilter>("all");
  const [modsSourceFilter, setModsSourceFilter] =
    useState<CatalogSourceFilter>("all");
  const [modsSort, setModsSort] = useState<ModsSort>("loadOrder");
  const [modsCollectionFilter, setModsCollectionFilter] = useState("");
  const [collectionIntegrityById, setCollectionIntegrityById] = useState<
    Record<string, CollectionIntegrityReport>
  >({});
  const checkingCollectionIntegrityKeysRef = useRef<Set<string>>(new Set());
  const [integrityDialogReport, setIntegrityDialogReport] =
    useState<CollectionIntegrityReport | null>(null);
  const [integrityRetrying, setIntegrityRetrying] = useState(false);
  const [modsFiltersOpen, setModsFiltersOpen] = useState(false);
  const [moveCollectionMod, setMoveCollectionMod] = useState<StagedMod | null>(null);
  const [moveCollectionBusy, setMoveCollectionBusy] = useState(false);
  const [installedCollections, setInstalledCollections] = useState<
    InstalledCollection[]
  >([]);
  const [friendSyncStates, setFriendSyncStates] = useState<
    Record<string, FriendCollectionSyncStatus>
  >({});
  const [syncingCollectionIds, setSyncingCollectionIds] = useState<
    Record<string, boolean>
  >({});
  const [savingCollectionIds, setSavingCollectionIds] = useState<
    Record<string, boolean>
  >({});
  const autoSyncInFlightRef = useRef<Set<string>>(new Set());
  const autoSyncNotifiedHashRef = useRef<Map<string, string>>(new Map());
  const [profileImportOpen, setProfileImportOpen] = useState(false);
  const [profileCode, setProfileCode] = useState("");
  const [shareImportOpen, setShareImportOpen] = useState(false);
  const [shareImportCode, setShareImportCode] = useState("");
  const [collectionImportOpen, setCollectionImportOpen] = useState(false);
  const [collectionImportCode, setCollectionImportCode] = useState("");
  const [saveImportPrompt, setSaveImportPrompt] = useState<SaveImportPrompt | null>(
    null,
  );
  const [saveLoadoutPrompt, setSaveLoadoutPrompt] = useState<{
    gameId: string;
    gameTitle: string;
    modCount: number;
  } | null>(null);
  const [saveLoadoutName, setSaveLoadoutName] = useState("");
  const [savedCollections, setSavedCollections] = useState<SavedCollectionEntry[]>(
    [],
  );
  const [friendsState, setFriendsState] = useState<FriendsState | null>(null);
  const [sharingTarget, setSharingTarget] = useState<{
    id: string;
    name: string;
    visibility: CollectionVisibility;
  } | null>(null);
  const [savedDetail, setSavedDetail] = useState<SavedCollectionDetail | null>(
    null,
  );
  const [savedInstallGameId, setSavedInstallGameId] = useState("");
  const [expandedCollectionGameKey, setExpandedCollectionGameKey] = useState<
    string | null
  >(null);
  const [installedEmperorShareIds, setInstalledEmperorShareIds] = useState(
    () => new Set<string>(),
  );
  const [savedCollectionAdvice, setSavedCollectionAdvice] = useState<
    Record<string, CollectionVersionAdvice>
  >({});
  const [gamePlugins, setGamePlugins] = useState<GamePluginInfo[]>([]);
  const [gameSavedDetail, setGameSavedDetail] = useState<SavedCollectionDetail | null>(
    null,
  );
  const [orphanScan, setOrphanScan] = useState<OrphanScan | null>(null);
  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
  const [downloadsReady, setDownloadsReady] = useState(false);
  const [searchQuery, setSearchQuery] = useState("");
  const [modHits, setModHits] = useState<CatalogHit[]>([]);
  const [collectionHits, setCollectionHits] = useState<CollectionHit[]>([]);
  const [browseMode, setBrowseMode] = useState<"mods" | "collections">("mods");
  const [sourceFilter, setSourceFilter] = useState<CatalogSourceFilter>("all");
  const [browseView, setBrowseView] = useState<ViewMode>("grid");
  const [browseGridMin, setBrowseGridMin] = useState(BROWSE_GRID_DEFAULT_PX);
  const deferredBrowseGridMin = useDeferredValue(browseGridMin);
  const browseGridSaveTimerRef = useRef<number | null>(null);
  const browseGridPendingRef = useRef<number | null>(null);
  const [browseListDensity, setBrowseListDensity] =
    useState<BrowseListDensity>("posters");
  const [browseSort, setBrowseSort] = useState("endorsements");
  const [browseCategory, setBrowseCategory] = useState("");
  const [browseVersion, setBrowseVersion] = useState("");
  const [browseAuthor, setBrowseAuthor] = useState("");
  const [browseAuthorDraft, setBrowseAuthorDraft] = useState("");
  const [authorSuggestions, setAuthorSuggestions] = useState<AuthorSuggestion[]>(
    [],
  );
  const [authorSuggestOpen, setAuthorSuggestOpen] = useState(false);
  const [authorSuggestIndex, setAuthorSuggestIndex] = useState(0);
  const [authorSuggestLoading, setAuthorSuggestLoading] = useState(false);
  const [browseTags, setBrowseTags] = useState<BrowseTagMap>({});
  const [tagQuery, setTagQuery] = useState("");
  const deferredTagQuery = useDeferredValue(tagQuery);
  const [tagsOpen, setTagsOpen] = useState(false);
  const [tagsPageLimit, setTagsPageLimit] = useState(40);
  const [expandedGroupLimits, setExpandedGroupLimits] = useState<Record<string, number>>({});
  const [browseTotalCount, setBrowseTotalCount] = useState(0);
  const [browseHasMore, setBrowseHasMore] = useState(false);
  const [browsePageCount, setBrowsePageCount] = useState(24);
  const [browseSearched, setBrowseSearched] = useState(false);
  const [browseLoading, setBrowseLoading] = useState(false);
  const [browseResultsPending, setBrowseResultsPending] = useState(false);
  const [libraryListsReady, setLibraryListsReady] = useState(false);
  const [savedCollectionsReady, setSavedCollectionsReady] = useState(false);
  const [savedDetailPending, setSavedDetailPending] = useState(false);
  const [gameSavedDetailPending, setGameSavedDetailPending] = useState(false);
  const savedOpenRef = useRef(0);
  const gameSavedOpenRef = useRef(0);
  const [browseViewSettled, setBrowseViewSettled] = useState<string | null>(null);
  const [libraryShellSettled, setLibraryShellSettled] = useState<string | null>(null);
  const [collectionsViewSettled, setCollectionsViewSettled] = useState<string | null>(null);
  const [browseMeta, setBrowseMeta] = useState<BrowseMeta | null>(null);
  const [browseDetail, setBrowseDetail] = useState<BrowseDetail | null>(null);
  const [browseReturnTab, setBrowseReturnTab] = useState<Tab | null>(null);
  const [libraryDetail, setLibraryDetail] = useState<{
    game: ManagedGame;
  } | null>(null);
  const [gameModal, setGameModal] = useState<
    "info" | "collections" | "tools" | "launch" | null
  >(null);
  const [unmanageTarget, setUnmanageTarget] = useState<ManagedGame | null>(null);
  const [uninstallCollectionTarget, setUninstallCollectionTarget] =
    useState<InstalledCollection | null>(null);
  const [unmanageStatus, setUnmanageStatus] = useState<{
    has_portable_mods: boolean;
    mod_count: number;
    skipped_count: number;
    already_saved: boolean;
    default_name: string;
  } | null>(null);
  const [unmanageStatusLoading, setUnmanageStatusLoading] = useState(false);
  const [gameSettingsMenuOpen, setGameSettingsMenuOpen] = useState(false);
  const [importModalOpen, setImportModalOpen] = useState(false);
  const [importDropActive, setImportDropActive] = useState(false);
  const [launchMenuOpen, setLaunchMenuOpen] = useState(false);
  const [launchStates, setLaunchStates] = useState<Record<string, GameLaunchState>>({});
  const [activeDeploymentByGame, setActiveDeploymentByGame] = useState<Record<string, boolean>>({});
  const [launchPreview, setLaunchPreview] = useState<LaunchPreview | null>(null);
  const [newEnvName, setNewEnvName] = useState("");
  const [newEnvValue, setNewEnvValue] = useState("");
  const [wrapperDraft, setWrapperDraft] = useState("");
  const [argsDraft, setArgsDraft] = useState("");
  const [gameInfo, setGameInfo] = useState<GameInfo | null>(null);
  const [modDetail, setModDetail] = useState<ModDetail | null>(null);
  const [modDetailLoading, setModDetailLoading] = useState(false);
  const [modFilesLoading, setModFilesLoading] = useState(false);
  const [tsDetail, setTsDetail] = useState<TsPackageDetail | null>(null);
  const [tsVersion, setTsVersion] = useState("");
  const [modioDetail, setModioDetail] = useState<ModioModDetail | null>(null);
  const [modioFiles, setModioFiles] = useState<ModioFileInfo[]>([]);
  const [modFiles, setModFiles] = useState<ModFileInfo[]>([]);
  const [modioKeyInput, setModioKeyInput] = useState("");
  const [modioGameIdEdit, setModioGameIdEdit] = useState("");
  const [collectionModFiles, setCollectionModFiles] = useState<CollectionModFile[]>([]);
  const [collectionDetail, setCollectionDetail] = useState<CollectionDetail | null>(null);
  const [collectionDetailLoading, setCollectionDetailLoading] = useState(false);
  const [collectionFilesLoading, setCollectionFilesLoading] = useState(false);
  const [includeOptional, setIncludeOptional] = useState(false);
  const [assistQueue, setAssistQueue] = useState<AssistQueueState | null>(null);
  const [activeBatch, setActiveBatch] = useState<ActiveDownloadBatch | null>(null);
  const [assistHint, setAssistHint] = useState<string | null>(null);
  const [assistActive, setAssistActive] = useState(false);
  const [assistLoginBanner, setAssistLoginBanner] = useState(false);
  const [catalogMatchPrompt, setCatalogMatchPrompt] = useState<{
    game: DetectedGame;
    engine: CatalogMatchEngine;
  } | null>(null);
  const [catalogMatchSuggestion, setCatalogMatchSuggestion] =
    useState<CatalogSuggestion | null>(null);
  const [catalogMatchLookingUp, setCatalogMatchLookingUp] = useState(false);
  const [catalogMatchProject, setCatalogMatchProject] = useState("");
  const [catalogSuggestions, setCatalogSuggestions] = useState<
    Record<string, CatalogSuggestion>
  >({});
  const catalogSuggestionsRef = useRef(catalogSuggestions);
  catalogSuggestionsRef.current = catalogSuggestions;
  const [collectionCatalogCovers, setCollectionCatalogCovers] = useState<
    Record<string, string>
  >({});
  const [collectionCatalogTitles, setCollectionCatalogTitles] = useState<
    Record<string, string>
  >({});
  const collectionCoverAttemptedRef = useRef<Set<string>>(new Set());
  const [catalogLookupBusy, setCatalogLookupBusy] = useState(false);
  const catalogSuggestGen = useRef(0);
  const catalogEnrichGen = useRef(0);
  const [ueProjectEdit, setUeProjectEdit] = useState("");
  const [ueDomainEdit, setUeDomainEdit] = useState("");
  const [tsCommunityEdit, setTsCommunityEdit] = useState("");
  const assistQueueRef = useRef<AssistQueueState | null>(null);
  const activeBatchRef = useRef<ActiveDownloadBatch | null>(null);
  activeBatchRef.current = activeBatch;
  const assistHostRef = useRef<HTMLDivElement | null>(null);
  const sidebarAssistHostRef = useRef<HTMLDivElement | null>(null);
  const assistActiveRef = useRef(false);
  const nexusLoginHostRef = useRef<HTMLDivElement | null>(null);
  const nexusLoginOpenRef = useRef(false);
  const nexusLoginAlreadyOpenRef = useRef(false);
  const nexusLoginGenRef = useRef(0);
  const [nexusLoginOpen, setNexusLoginOpen] = useState(false);
  const [webLoginProvider, setWebLoginProvider] = useState<"nexus" | "modio" | "nexus-key">("nexus");
  const [nexusLoginConfirming, setNexusLoginConfirming] = useState(false);
  const tabRef = useRef(tab);
  tabRef.current = tab;
  const libraryKeptRef = useRef(false);
  const collectionsKeptRef = useRef(false);
  const friendsKeptRef = useRef(false);
  const browseKeptRef = useRef(false);
  const downloadsKeptRef = useRef(false);
  const toolsKeptRef = useRef(false);
  const settingsKeptRef = useRef(false);
  if (tab === "library") libraryKeptRef.current = true;
  if (tab === "collections") collectionsKeptRef.current = true;
  if (tab === "friends") friendsKeptRef.current = true;
  if (tab === "browse") browseKeptRef.current = true;
  if (tab === "downloads") downloadsKeptRef.current = true;
  if (tab === "tools") toolsKeptRef.current = true;
  if (tab === "settings") settingsKeptRef.current = true;
  const navTtlRef = useRef(new TtlTracker());
  const browseMetaCacheRef = useRef<Map<string, BrowseMeta>>(new Map());
  const gameInfoCacheRef = useRef<Map<string, GameInfo>>(new Map());
  const gameInfoReqRef = useRef(0);
  const appInfoLoadedRef = useRef(false);
  const coverAttemptedRef = useRef<Set<string>>(new Set());
  const resolvedCatalogCoversRef = useRef<Map<string, string>>(new Map());
  const assistVisibleSentRef = useRef<boolean | null>(null);
  const collectionsMetaGenRef = useRef(0);
  const browseDetailReqRef = useRef(0);
  assistActiveRef.current = assistActive;
  const setupCompleteRef = useRef(false);
  const navHistoryRef = useRef<AppNavHistory | null>(null);
  const navBusyRef = useRef(false);
  const skipHistoryPushRef = useRef(false);
  const [navCanBack, setNavCanBack] = useState(false);
  const browseReturnTabRef = useRef(browseReturnTab);
  browseReturnTabRef.current = browseReturnTab;
  const browseDetailRef = useRef(browseDetail);
  browseDetailRef.current = browseDetail;
  const libraryDetailRef = useRef(libraryDetail);
  libraryDetailRef.current = libraryDetail;
  const gameSavedDetailRef = useRef(gameSavedDetail);
  gameSavedDetailRef.current = gameSavedDetail;
  const savedDetailRef = useRef(savedDetail);
  savedDetailRef.current = savedDetail;
  const managedRef = useRef(managed);
  managedRef.current = managed;

  function rememberBrowseScroll() {
    if (!browseResultsVisibleRef.current) return;
    if (browseScrollEpochRef.current !== browseScrollArmedEpochRef.current) return;
    const top = browseResultsRef.current?.scrollTop;
    if (top != null) browseScrollRef.current = top;
  }

  function scrollBrowseHome() {
    browseScrollRef.current = 0;
    browseResultsRef.current?.scrollTo(0, 0);
  }

  function setTab(next: Tab) {
    if (!setupCompleteRef.current && next !== "setup") return;
    if (next !== tabRef.current) rememberBrowseScroll();
    setTabState(next);
  }
  const assistSyncGenRef = useRef(0);
  const downloadStartedAtRef = useRef<number>(0);
  const downloadProgressAtRef = useRef<Map<string, { bytes: number; at: number }>>(new Map());
  const tagPickerRef = useRef<HTMLDivElement | null>(null);
  const authorSuggestRef = useRef<HTMLDivElement | null>(null);
  const authorSuggestReqRef = useRef(0);
  const tagSearchRef = useRef<HTMLInputElement | null>(null);
  const gameSettingsMenuRef = useRef<HTMLDivElement | null>(null);
  const launchMenuRef = useRef<HTMLDivElement | null>(null);
  const mainScrollRef = useRef<HTMLElement | null>(null);
  const browseResultsRef = useRef<HTMLDivElement | null>(null);
  const libraryHomeScrollRef = useRef<HTMLDivElement | null>(null);
  const libraryDetailScrollRef = useRef<HTMLDivElement | null>(null);
  const browseDetailScrollRef = useRef<HTMLDivElement | null>(null);
  const browseScrollRef = useRef(0);
  const browseScrollEpochRef = useRef(0);
  const browseScrollArmedEpochRef = useRef(0);
  const browseResultsVisibleRef = useRef(false);
  const browsePrefetchErrorRef = useRef<string | null>(null);
  const browseSentinelRef = useRef<HTMLDivElement | null>(null);
  const [browseSentinelEl, setBrowseSentinelEl] = useState<HTMLDivElement | null>(null);
  const setBrowseSentinel = useCallback((el: HTMLDivElement | null) => {
    browseSentinelRef.current = el;
    setBrowseSentinelEl(el);
  }, []);
  const browseLoadingMoreRef = useRef(false);
  const browseAppendStuckRef = useRef(false);
  const browseSentinelTopRef = useRef<number | null>(null);
  const browseNextOffsetRef = useRef(0);
  const browseAppendFailedRef = useRef(false);
  const browseRequestIdRef = useRef(0);
  const browseResultsRequestRef = useRef(0);
  const browseModeRef = useRef(browseMode);
  browseModeRef.current = browseMode;
  const searchQueryRef = useRef(searchQuery);
  searchQueryRef.current = searchQuery;
  const browseHasMoreRef = useRef(browseHasMore);
  browseHasMoreRef.current = browseHasMore;
  const browsePageCountRef = useRef(browsePageCount);
  browsePageCountRef.current = browsePageCount;
  const onBrowsePageSize = useCallback((count: number) => {
    browsePageCountRef.current = count;
    setBrowsePageCount((prev) => (prev === count ? prev : count));
  }, []);
  const browseOptsRef = useRef<BrowseSearchOpts>({
    sort: browseSort,
    category: browseCategory || null,
    tagsInclude: [],
    tagsExclude: [],
    gameVersion: browseVersion || null,
    author: browseAuthor || null,
    offset: 0,
  });

  const activeGame = useMemo(
    () => managed.find((g) => g.id === activeId) ?? managed[0] ?? null,
    [managed, activeId],
  );
  const activeGameCoverSrc = gameCoverSrc(activeGame);
  const installedIds = useMemo(
    () => (activeGame ? installedCatalogIds(mods, activeGame) : new Set<string>()),
    [mods, activeGame],
  );
  const detailInstalledFileIds = useMemo(() => {
    if (!activeGame || !browseDetail || browseDetail.kind !== "mod") {
      return new Set<number>();
    }
    return installedFileIdsForCatalog(mods, activeGame, browseDetail.hit.id);
  }, [activeGame, browseDetail, mods]);
  const activeGameRef = useRef(activeGame);
  activeGameRef.current = activeGame;
  const prevActiveGameIdRef = useRef<string | null>(null);
  const pendingDownloadPageRef = useRef<{ hit: CatalogHit; gameId: string } | null>(null);
  const pendingEmperorModRef = useRef<{ hit: CatalogHit; gameId: string } | null>(null);
  const openEmperorModLinkRef = useRef<(payload: EmperorOpenMod) => void>(() => {});
  const [pendingFriendInviteCode, setPendingFriendInviteCode] = useState<string | null>(
    null,
  );
  const openEmperorFriendLinkRef = useRef<(code: string) => void>(() => {});
  openEmperorFriendLinkRef.current = (code: string) => {
    setPendingFriendInviteCode(code);
    setTab("friends");
  };
  const lastEmperorOpenRef = useRef<{ key: string; at: number } | null>(null);
  const modUpdatesReqRef = useRef(0);
  const checkingModUpdatesTimerRef = useRef<number | null>(null);
  const checkingFriendUpdatesTimerRef = useRef<number | null>(null);
  const modFilesReqRef = useRef(0);
  const modFilesModIdRef = useRef<number | null>(null);
  const wineAdviceReqRef = useRef(0);
  const missingReqRef = useRef(0);
  const gameVersionReqRef = useRef(0);
  const pendingRequirementLinksRef = useRef<Map<string, string[]>>(new Map());
  const pictureFillInFlightRef = useRef(false);
  const pictureFillSkippedRef = useRef(new Set<string>());
  const [pictureFillTick, setPictureFillTick] = useState(0);
  const activeRequirementsGameId = activeGame?.id ?? null;
  // Clear these caches while rendering so the mods page paints without the
  // previous game's requirements or version check.
  const fomodScopeCleared = requirementScopeGameId !== activeRequirementsGameId;
  if (fomodScopeCleared) {
    setRequirementScopeGameId(activeRequirementsGameId);
    setMissingRequirements(null);
    setGameVersionReport(null);
    setGameVersionDraft("");
    setRequirementsModal(null);
    setIgnoredRequirementsOpen(false);
    setFomodNeedingIds([]);
    setFomodDialog(null);
    pendingUpdatedFomodIdsRef.current.clear();
    missingReqRef.current += 1;
    gameVersionReqRef.current += 1;
  }
  const openFomod = fomodScopeCleared ? null : fomodDialog;
  fomodDialogRef.current = openFomod;
  const shownFomodNeedingIds = fomodScopeCleared ? [] : fomodNeedingIds;

  const clearModFiles = useCallback(() => {
    modFilesReqRef.current += 1;
    modFilesModIdRef.current = null;
    setModFiles([]);
  }, []);

  const loadModFiles = useCallback(async (domain: string, modId: number) => {
    const reqId = ++modFilesReqRef.current;
    modFilesModIdRef.current = null;
    setModFiles([]);
    setModFilesLoading(true);
    try {
      const files = await api.modFiles(domain, modId);
      if (reqId !== modFilesReqRef.current) return;
      modFilesModIdRef.current = modId;
      setModFiles(files);
    } finally {
      if (reqId === modFilesReqRef.current) setModFilesLoading(false);
    }
  }, []);

  const closeBrowseDetail = useCallback(() => {
    browseDetailReqRef.current += 1;
    setBrowseDetail(null);
    setModDetail(null);
    setModDetailLoading(false);
    setModFilesLoading(false);
    setTsDetail(null);
    setTsVersion("");
    setModioDetail(null);
    setModioFiles([]);
    clearModFiles();
    setCollectionModFiles([]);
    setCollectionDetail(null);
    setCollectionDetailLoading(false);
    setCollectionFilesLoading(false);
  }, [clearModFiles]);

  const resetBrowseState = useCallback(() => {
    browseRequestIdRef.current += 1;
    browseResultsRequestRef.current = browseRequestIdRef.current;
    browseNextOffsetRef.current = 0;
    browseLoadingMoreRef.current = false;
    browseAppendStuckRef.current = false;
    browseSentinelTopRef.current = null;
    browseAppendFailedRef.current = false;
    browseScrollEpochRef.current += 1;
    browseScrollRef.current = 0;
    browseResultsVisibleRef.current = false;
    browsePrefetchErrorRef.current = null;
    setBrowseLoading(false);
    setBrowseResultsPending(false);
    closeBrowseDetail();
    setSearchQuery("");
    setModHits([]);
    setCollectionHits([]);
    setBrowseSearched(false);
    setBrowseTotalCount(0);
    setBrowseHasMore(false);
    setBrowseCategory("");
    setBrowseVersion("");
    setBrowseAuthor("");
    setBrowseAuthorDraft("");
    setAuthorSuggestions([]);
    setAuthorSuggestOpen(false);
    setAuthorSuggestIndex(0);
    setAuthorSuggestLoading(false);
    setBrowseTags({});
    setTagQuery("");
    setTagsOpen(false);
    setProfileImportOpen(false);
    setProfileCode("");
    setBrowseReturnTab(null);
  }, [closeBrowseDetail]);

  function navTab(): NavEntry["tab"] {
    return tabRef.current ?? "setup";
  }

  function captureNavEntry(): NavEntry {
    const detail = browseDetailRef.current;
    const library = libraryDetailRef.current;
    return {
      tab: navTab(),
      browseReturnTab: browseReturnTabRef.current,
      browseDetail: detail
        ? detail.kind === "mod"
          ? { kind: "mod", hit: detail.hit, tab: detail.tab }
          : { kind: "collection", hit: detail.hit, tab: detail.tab }
        : null,
      libraryDetail: library ? { gameId: library.game.id } : null,
      gameSavedDetailId: gameSavedDetailRef.current?.entry.id ?? null,
      savedDetailId: savedDetailRef.current?.entry.id ?? null,
    };
  }

  function syncNavCanBack() {
    setNavCanBack(navHistoryRef.current?.canBack() ?? false);
  }

  function pushNavTarget(next: NavEntry) {
    const journal = navHistoryRef.current;
    if (!journal || skipHistoryPushRef.current) return;
    journal.replaceCurrent(captureNavEntry());
    journal.push(next);
    syncNavCanBack();
  }

  async function restoreNavEntry(entry: NavEntry) {
    navBusyRef.current = true;
    skipHistoryPushRef.current = true;
    try {
      closeBrowseDetail();
      setLibraryDetail(null);
      setGameInfo(null);
      gameSavedOpenRef.current += 1;
      savedOpenRef.current += 1;
      setGameSavedDetail(null);
      setGameSavedDetailPending(false);
      setSavedDetail(null);
      setSavedDetailPending(false);
      setBrowseReturnTab(entry.browseReturnTab);

      if (entry.tab !== tabRef.current) {
        setTabState(entry.tab);
      }

      if (entry.libraryDetail) {
        const game = managedRef.current.find((g) => g.id === entry.libraryDetail!.gameId);
        if (game) {
          setLibraryDetail({ game });
          setGameModal(null);
          setGameSettingsMenuOpen(false);
          void loadGameInfo(game.nexus_domain);
        }
      }

      if (entry.gameSavedDetailId) {
        await openGameSavedCollection(entry.gameSavedDetailId, { skipHistoryPush: true });
      }

      if (entry.savedDetailId) {
        await openSavedCollection(entry.savedDetailId, { skipHistoryPush: true });
      }

      if (entry.browseDetail) {
        if (entry.browseDetail.kind === "mod") {
          await openMod(entry.browseDetail.hit, entry.browseDetail.tab, {
            skipHistoryPush: true,
          });
        } else {
          await openCollection(entry.browseDetail.hit, entry.browseDetail.tab, {
            skipHistoryPush: true,
          });
        }
      } else if (entry.browseReturnTab) {
        setBrowseReturnTab(null);
      }
    } finally {
      skipHistoryPushRef.current = false;
      navBusyRef.current = false;
    }
  }

  async function navBack() {
    if (navBusyRef.current) return;
    const journal = navHistoryRef.current;
    if (!journal) return;
    const entry = journal.back();
    if (!entry) return;
    syncNavCanBack();
    await restoreNavEntry(entry);
  }

  async function navForward() {
    if (navBusyRef.current) return;
    const journal = navHistoryRef.current;
    if (!journal) return;
    const entry = journal.forward();
    if (!entry) return;
    syncNavCanBack();
    await restoreNavEntry(entry);
  }

  function tryCloseOverlayOnBack(): boolean {
    if (quickInstall) {
      setQuickInstall(null);
      return true;
    }
    if (openFomod) {
      closeFomodDialog("cancel");
      return true;
    }
    if (modOptionsFor) {
      setModOptionsFor(null);
      return true;
    }
    if (loadDiagnosis) {
      setLoadDiagnosis(null);
      return true;
    }
    if (requirementsModal) {
      setRequirementsModal(null);
      return true;
    }
    if (ignoredRequirementsOpen) {
      setIgnoredRequirementsOpen(false);
      return true;
    }
    if (relinkPrompt) {
      setRelinkPrompt(null);
      return true;
    }
    if (nexusLoginOpenRef.current) {
      closeNexusLogin();
      return true;
    }
    if (tagsOpen) {
      setTagsOpen(false);
      return true;
    }
    if (gameModal) {
      setGameModal(null);
      return true;
    }
    if (launchMenuOpen) {
      setLaunchMenuOpen(false);
      return true;
    }
    if (gameSettingsMenuOpen) {
      setGameSettingsMenuOpen(false);
      return true;
    }
    if (modsFiltersOpen) {
      setModsFiltersOpen(false);
      return true;
    }
    if (profileImportOpen) {
      setProfileImportOpen(false);
      return true;
    }
    if (shareImportOpen) {
      setShareImportOpen(false);
      return true;
    }
    return false;
  }

  const tryCloseOverlayOnBackRef = useRef(tryCloseOverlayOnBack);
  tryCloseOverlayOnBackRef.current = tryCloseOverlayOnBack;
  const navBackRef = useRef(navBack);
  navBackRef.current = navBack;
  const navForwardRef = useRef(navForward);
  navForwardRef.current = navForward;

  const resetLibraryEphemeralState = useCallback(() => {
    setShareImportOpen(false);
    setShareImportCode("");
    setModsFiltersOpen(false);
    setGameSettingsMenuOpen(false);
  }, []);

  const unmanaged = useMemo(() => {
    const managedIds = new Set(managed.map((g) => g.id));
    const managedKeys = new Set(
      managed.map((g) => normalizeGameKey(g.title, g.launcher)),
    );
    return detected.filter((g) => {
      if (managedIds.has(g.id)) {
        return false;
      }
      if (managedKeys.has(normalizeGameKey(g.title, g.launcher))) {
        return false;
      }
      return true;
    });
  }, [detected, managed]);

  const gameHealthById = useMemo(() => {
    const map = new Map<string, GameHealth>();
    for (const entry of gameHealth) {
      map.set(entry.game_id, entry);
    }
    return map;
  }, [gameHealth]);

  const collectionsByModId = useMemo(() => {
    const map = new Map<string, ReturnType<typeof collectionsForMod>>();
    for (const m of mods) {
      const matched = collectionsForMod(m, installedCollections);
      if (matched.length) map.set(m.id, matched);
    }
    return map;
  }, [mods, installedCollections]);

  const collectionNamesByModId = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const [id, list] of collectionsByModId) {
      map.set(
        id,
        list.map((c) => c.name),
      );
    }
    return map;
  }, [collectionsByModId]);

  const deferredModsQuery = useDeferredValue(modsQuery);

  const visibleMods = useMemo(
    () =>
      filterAndSortStagedMods(mods, {
        query: deferredModsQuery,
        status: modsStatusFilter,
        source: modsSourceFilter,
        collectionId: modsCollectionFilter,
        sort: modsSort,
        collectionNamesByModId,
      }),
    [
      mods,
      deferredModsQuery,
      modsStatusFilter,
      modsSourceFilter,
      modsCollectionFilter,
      modsSort,
      collectionNamesByModId,
    ],
  );

  const enabledModCount = useMemo(
    () => mods.reduce((count, m) => count + (m.enabled ? 1 : 0), 0),
    [mods],
  );

  const groupedCollectionMods = useMemo(
    () => groupCollectionMods(collectionModFiles),
    [collectionModFiles],
  );

  const modsFiltersActive =
    Boolean(modsQuery.trim()) ||
    modsStatusFilter !== "all" ||
    modsSourceFilter !== "all" ||
    Boolean(modsCollectionFilter) ||
    modsSort !== "loadOrder";

  const canReorderMods =
    modsSort === "loadOrder" &&
    !modsQuery.trim() &&
    modsStatusFilter === "all" &&
    modsSourceFilter === "all" &&
    !modsCollectionFilter;

  const modsInLoadOrder = useMemo(() => stagedModsInLoadOrder(mods), [mods]);

  const fomodNeeding = useMemo(() => {
    const byId = new Map(mods.map((mod) => [mod.id, mod]));
    return shownFomodNeedingIds.flatMap((id) => {
      const mod = byId.get(id);
      // A refresh that started before Finish can still list a mod whose choice is already stored.
      if (!mod || mod.fomod_selection) return [];
      return [mod];
    });
  }, [mods, shownFomodNeedingIds]);

  const modIndexById = useMemo(() => {
    const map = new Map<string, number>();
    modsInLoadOrder.forEach((m, i) => map.set(m.id, i));
    return map;
  }, [modsInLoadOrder]);

  const reduceMotion = useReducedMotion() === true;

  const commitModDrag = useCallback(
    async (modId: string, targetGroupIndex: number) => {
      if (!activeGame || !canReorderMods) return;
      const next = reorderStagedModToGroupIndex(mods, modId, targetGroupIndex);
      if (!next) return;
      await api.setLoadOrder(
        activeGame.id,
        next.map((m) => m.id),
      );
      setMods(next);
      void api.listInstalledCollections(activeGame.id).then(setInstalledCollections);
    },
    [activeGame, canReorderMods, mods],
  );

  const modDrag = useStagedModDrag({
    canReorder: canReorderMods,
    visibleMods,
    onCommit: commitModDrag,
  });

  const stagedUpdateCount = useMemo(
    () => mods.reduce((n, m) => n + (modUpdates[m.id] ? 1 : 0), 0),
    [mods, modUpdates],
  );

  const unmanagedSuggestKey = unmanaged
    .map((game) => `${game.id}\0${game.title}`)
    .join("\n");
  const unmanagedForSuggestRef = useRef(unmanaged);
  unmanagedForSuggestRef.current = unmanaged;

  useEffect(() => {
    const games = unmanagedForSuggestRef.current;
    if (games.length === 0) {
      setCatalogSuggestions({});
      setCatalogLookupBusy(false);
      return;
    }
    const requests = games.map((g) => ({ id: g.id, title: g.title }));
    const gen = ++catalogEnrichGen.current;
    setCatalogLookupBusy(true);
    void (async () => {
      try {
        const map = await api.suggestCatalogIdsBatch(requests);
        if (catalogEnrichGen.current !== gen) {
          return;
        }
        setCatalogSuggestions(map);
      } catch {
        if (catalogEnrichGen.current !== gen) {
          return;
        }
        setCatalogSuggestions({});
      } finally {
        if (catalogEnrichGen.current === gen) {
          setCatalogLookupBusy(false);
        }
      }
    })();
  }, [unmanagedSuggestKey]);

  const themePref: ThemePreference = settings?.theme ?? "system";
  const themeStyle: ThemeStyle = settings?.theme_style ?? "app";
  const accentColor = normalizeAccent(settings?.accent_color);
  const linuxSystemBackend: LinuxSystemBackend =
    settings?.linux_system_backend ?? "gtk";

  const refreshSettings = useCallback(async () => {
    const s = await api.getSettings();
    setupCompleteRef.current = s.initial_setup_complete === true;
    setSettings(s);
    setActiveId(s.last_active_game_id);
    if (
      typeof s.browse_grid_size === "number" &&
      browseGridPendingRef.current === null
    ) {
      setBrowseGridMin(clampBrowseGridSize(s.browse_grid_size));
    }
    await applyAppearance(
      s.theme ?? "system",
      s.theme_style ?? "app",
      s.accent_color,
    );
    return s;
  }, []);

  const handleBrowseGridMinChange = useCallback((nextRaw: number) => {
    const next = clampBrowseGridSize(nextRaw);
    setBrowseGridMin(next);
    browseGridPendingRef.current = next;
    if (browseGridSaveTimerRef.current !== null) {
      window.clearTimeout(browseGridSaveTimerRef.current);
    }
    browseGridSaveTimerRef.current = window.setTimeout(() => {
      browseGridSaveTimerRef.current = null;
      const pending = browseGridPendingRef.current;
      browseGridPendingRef.current = null;
      if (pending !== null) {
        void api.setBrowseGridSize(pending).catch(() => {});
      }
    }, 250);
  }, []);

  useEffect(() => {
    return () => {
      if (browseGridSaveTimerRef.current !== null) {
        window.clearTimeout(browseGridSaveTimerRef.current);
        browseGridSaveTimerRef.current = null;
      }
      const pending = browseGridPendingRef.current;
      browseGridPendingRef.current = null;
      if (pending !== null) {
        void api.setBrowseGridSize(pending).catch(() => {});
      }
    };
  }, []);

  const loadAppInfo = useCallback(async () => {
    if (appInfoLoadedRef.current) return;
    try {
      const info = await api.getAppInfo();
      appInfoLoadedRef.current = true;
      setAppInfo(info);
    } catch {
      /* ignore */
    }
  }, []);

  const loadGameInfo = useCallback(async (domain: string | null | undefined) => {
    const key = domain?.trim() ?? "";
    const req = ++gameInfoReqRef.current;
    if (!key) {
      setGameInfo(null);
      return;
    }
    const cached = gameInfoCacheRef.current.get(key);
    if (cached) {
      setGameInfo(cached);
      return;
    }
    setGameInfo(null);
    try {
      const info = await api.getGame(key);
      gameInfoCacheRef.current.set(key, info);
      if (req === gameInfoReqRef.current) {
        setGameInfo(info);
      }
    } catch {
      /* Nexus metadata is optional for local game info */
    }
  }, []);

  const checkForAppUpdate = useCallback(async (quiet = false) => {
    setCheckingAppUpdate(true);
    if (!quiet) {
      setAppUpdateMessage(null);
      setError(null);
    }
    try {
      const status = await api.checkAppUpdate();
      navTtlRef.current.mark("app-update");
      setAppUpdate(status);
      setAppInfo((prev) =>
        prev
          ? { ...prev, version: status.current_version }
          : {
              name: "emperor-mod-manager",
              version: status.current_version,
              platform: "unknown",
              linux_only: false,
            },
      );
      if (!quiet) {
        if (status.update_available) {
          setAppUpdateMessage(`Update available: v${status.latest_version}`);
        } else if (status.latest_version) {
          setAppUpdateMessage("You’re up to date.");
        } else {
          setAppUpdateMessage("No releases found on GitHub.");
        }
      }
    } catch (e) {
      if (!quiet) {
        setAppUpdateMessage(null);
        setError(String(e));
      }
    } finally {
      setCheckingAppUpdate(false);
    }
  }, []);

  const installAppUpdate = useCallback(async () => {
    setInstallingAppUpdate(true);
    setAppUpdateMessage("Downloading…");
    setError(null);
    try {
      const result = await api.installAppUpdate();
      setAppUpdateMessage(result.message);
      if (!result.will_exit) {
        setNotice({ kind: "ok", message: result.message });
      }
    } catch (e) {
      setAppUpdateMessage(null);
      setError(String(e));
    } finally {
      setInstallingAppUpdate(false);
    }
  }, []);

  const refreshGameHealth = useCallback(async () => {
    try {
      const health = await api.getGameHealth();
      startTransition(() => {
        setGameHealth((prev) => (sameSnapshot(prev, health) ? prev : health));
      });
      return health;
    } catch {
      startTransition(() => {
        setGameHealth((prev) => (prev.length === 0 ? prev : []));
      });
      return [];
    }
  }, []);

  const refreshManaged = useCallback(async () => {
    const list = await api.listManaged();
    startTransition(() => {
      setManaged((prev) => (sameSnapshot(prev, list) ? prev : list));
    });
    return list;
  }, []);

  const refreshWineAdvice = useCallback(async (gameId: string) => {
    const req = ++wineAdviceReqRef.current;
    try {
      const advice = await api.wineOverrideAdvice(gameId);
      if (req !== wineAdviceReqRef.current) return;
      if (activeGameRef.current?.id !== gameId) return;
      setWineAdvice(advice);
    } catch (e) {
      console.warn("wine_override_advice failed", e);
    }
  }, []);

  const refreshMods = useCallback(async (gameId: string) => {
    let revealed = false;
    try {
      const list = await api.listMods(gameId);
      if (activeGameRef.current?.id !== gameId) return null;
      startTransition(() => {
        if (activeGameRef.current?.id !== gameId) return;
        setMods(list);
        setModsReadyGameId(gameId);
      });
      revealed = true;
      // Reading a manifest per mod is a disk walk, so ask once per refresh
      // instead of on every row render. The list is already visible.
      try {
        const withOptions = await api.listModsWithOptions(gameId);
        if (activeGameRef.current?.id !== gameId) return null;
        setModsWithOptions(new Set(withOptions));
      } catch (e) {
        console.warn("list_mods_with_options failed", e);
      }
      let needing: string[] = [];
      try {
        needing = await api.listModsNeedingFomod(gameId);
        if (activeGameRef.current?.id !== gameId) return null;
        setFomodNeedingIds(needing);
      } catch (e) {
        console.warn("list_mods_needing_fomod failed", e);
      }
      void refreshWineAdvice(gameId);
      return { list, needing };
    } finally {
      if (!revealed && activeGameRef.current?.id === gameId) {
        setModsReadyGameId(gameId);
      }
    }
  }, [refreshWineAdvice]);

  const openModInstaller = useCallback(async (m: StagedMod) => {
    if (!activeGameRef.current) return;
    const gameId = activeGameRef.current.id;
    try {
      const fomod = await api.getFomodInstaller(gameId, m.id);
      if (fomod) {
        const next = { mod: m, pass: null };
        fomodDialogRef.current = next;
        setFomodDialog(next);
        return;
      }
    } catch (e) {
      console.warn("get_fomod_installer failed", e);
    }
    setModOptionsFor(m);
  }, []);

  const refreshModUpdates = useCallback(async (gameId: string) => {
    const req = ++modUpdatesReqRef.current;
    if (checkingModUpdatesTimerRef.current != null) {
      window.clearTimeout(checkingModUpdatesTimerRef.current);
      checkingModUpdatesTimerRef.current = null;
    }
    checkingModUpdatesTimerRef.current = window.setTimeout(() => {
      if (req !== modUpdatesReqRef.current) return;
      checkingModUpdatesTimerRef.current = null;
      setCheckingModUpdates(true);
    }, MOD_UPDATE_CHECK_TOAST_DELAY_MS);
    try {
      const updates = await api.checkStagedModUpdates(gameId);
      if (req !== modUpdatesReqRef.current) return null;
      if (activeGameRef.current?.id !== gameId) return null;
      const map: Record<string, StagedModUpdate> = {};
      for (const u of updates) {
        map[u.staged_id] = u;
      }
      setModUpdates(map);
      const list = await api.listMods(gameId);
      if (req !== modUpdatesReqRef.current) return null;
      if (activeGameRef.current?.id !== gameId) return null;
      const named = new Map(
        list.filter((m) => m.file_name).map((m) => [m.id, m.file_name] as const),
      );
      setMods((prev) => {
        let changed = false;
        const next = prev.map((m) => {
          const fileName = named.get(m.id);
          if (!fileName || m.file_name) return m;
          changed = true;
          return { ...m, file_name: fileName };
        });
        return changed ? next : prev;
      });
      return updates;
    } catch (e) {
      console.warn("check_staged_mod_updates failed", e);
      return null;
    } finally {
      if (req === modUpdatesReqRef.current) {
        if (checkingModUpdatesTimerRef.current != null) {
          window.clearTimeout(checkingModUpdatesTimerRef.current);
          checkingModUpdatesTimerRef.current = null;
        }
        setCheckingModUpdates(false);
      }
    }
  }, []);

  const refreshMissingRequirements = useCallback(async (gameId: string) => {
    const req = ++missingReqRef.current;
    try {
      const result = await api.missingNexusRequirements(gameId);
      if (req !== missingReqRef.current) return;
      if (activeGameRef.current?.id !== gameId) return;
      setMissingRequirements(result);
    } catch (e) {
      if (req !== missingReqRef.current) return;
      if (activeGameRef.current?.id !== gameId) return;
      setMissingRequirements({
        nexus: [],
        external: [],
        dlc: [],
        error: String(e),
      });
    }
  }, []);

  const refreshGameVersions = useCallback(async (gameId: string) => {
    const req = ++gameVersionReqRef.current;
    try {
      const report = await api.checkModGameVersions(gameId);
      if (req !== gameVersionReqRef.current) return;
      if (activeGameRef.current?.id !== gameId) return;
      setGameVersionReport(report);
    } catch (e) {
      console.warn("check_mod_game_versions failed", e);
    }
  }, []);

  const refreshFriendSyncStates = useCallback(async (gameId: string) => {
    try {
      const states = await api.getFriendCollectionSyncStates(gameId);
      if (activeGameRef.current?.id !== gameId) return;
      const map: Record<string, FriendCollectionSyncStatus> = {};
      for (const s of states) {
        map[s.collection_id] = s;
      }
      setFriendSyncStates(map);
    } catch {
      // Non-fatal: friend sync status is best-effort.
    }
  }, []);

  const refreshCollections = useCallback(async (gameId: string) => {
    const list = await api.listInstalledCollections(gameId);
    if (activeGameRef.current?.id !== gameId) return;
    setInstalledCollections(list);
    void refreshFriendSyncStates(gameId);
    try {
      const reports = await api.checkInstalledCollectionsLocal(gameId);
      if (activeGameRef.current?.id !== gameId) return;
      const map: Record<string, CollectionIntegrityReport> = {};
      for (const r of reports) {
        map[r.collection_id] = r;
      }
      setCollectionIntegrityById(map);
    } catch {
      // Non-fatal: local integrity badge is best-effort.
    }

    for (const c of list) {
      const checkKey = `${gameId}:${c.id}`;
      if (checkingCollectionIntegrityKeysRef.current.has(checkKey)) continue;
      checkingCollectionIntegrityKeysRef.current.add(checkKey);
      void api
        .checkCollectionIntegrity(gameId, c.id, true)
        .then((report) => {
          if (activeGameRef.current?.id !== gameId) return;
          setCollectionIntegrityById((prev) => ({
            ...prev,
            [c.id]: report,
          }));
        })
        .catch(() => {
          // Non-fatal: background remote integrity check is best-effort.
        })
        .finally(() => {
          checkingCollectionIntegrityKeysRef.current.delete(checkKey);
        });
    }
  }, [refreshFriendSyncStates]);

  const refreshOrphanScan = useCallback(async () => {
    const scan = await api.scanModOrphans();
    setOrphanScan(scan);
    return scan;
  }, []);

  const refreshDownloads = useCallback(async () => {
    try {
      const list = await api.listDownloads();
      const now = Date.now();
      for (const d of list) {
        if (isActiveDownloadStatus(d.status)) {
          const entry = downloadProgressAtRef.current.get(d.id);
          if (!entry) {
            downloadProgressAtRef.current.set(d.id, {
              bytes: d.bytes_downloaded,
              at: now,
            });
          }
        }
      }
      setDownloads(list);
    } finally {
      setDownloadsReady(true);
    }
  }, []);

  const fillCatalogCovers = useCallback(
    async (
      found: DetectedGame[],
      managedList: ManagedGame[],
      suggestions?: Record<string, CatalogSuggestion | null | undefined>,
    ) => {
      const requests = catalogCoverRequests(found, managedList, suggestions).filter(
        (req) => {
          const key = `${req.id}:${req.nexus_domain ?? ""}:${req.modio_game_id ?? ""}`;
          return !coverAttemptedRef.current.has(key);
        },
      );
      if (requests.length === 0) return;
      for (const req of requests) {
        coverAttemptedRef.current.add(
          `${req.id}:${req.nexus_domain ?? ""}:${req.modio_game_id ?? ""}`,
        );
      }
      try {
        const hits = await api.resolveCatalogCovers(requests);
        if (hits.length === 0) return;
        for (const hit of hits) {
          resolvedCatalogCoversRef.current.set(hit.id, hit.cover_url);
        }
        const urls = new Map(hits.map((hit) => [hit.id, hit.cover_url]));
        startTransition(() => {
          setDetected((prev) => applyCatalogCovers(prev, urls));
        });
        await refreshManaged();
      } catch (e) {
        console.warn("resolve_catalog_covers failed", e);
      }
    },
    [refreshManaged],
  );

  useEffect(() => {
    if (Object.keys(catalogSuggestions).length === 0) return;
    void fillCatalogCovers(detected, managed, catalogSuggestions);
  }, [catalogSuggestions, detected, managed, fillCatalogCovers]);

  const scanInFlightRef = useRef<Promise<void> | null>(null);

  const performScan = useCallback(
    async (quiet: boolean) => {
      if (!quiet) {
        setBusy("Scanning installed games…");
        setError(null);
        setNotice(null);
      }
      try {
        const found = applyCatalogCovers(
          await api.scanGames(),
          resolvedCatalogCoversRef.current,
        );
        startTransition(() => {
          setDetected((prev) => (sameSnapshot(prev, found) ? prev : found));
        });
        const managedList = await refreshManaged();
        await fillCatalogCovers(found, managedList, catalogSuggestionsRef.current);
        await refreshGameHealth();
        navTtlRef.current.mark("library-scan");
      } catch (e) {
        if (quiet) {
          console.warn("scan_games failed", e);
        } else {
          setError(String(e));
        }
      } finally {
        if (!quiet) setBusy(null);
        setLibraryListsReady(true);
      }
    },
    [fillCatalogCovers, refreshGameHealth, refreshManaged, setError],
  );

  const scan = useCallback(async () => {
    navTtlRef.current.invalidate("library-scan");
    for (;;) {
      const pending = scanInFlightRef.current;
      if (!pending) break;
      setBusy("Scanning installed games…");
      try {
        await pending;
      } catch {
        /* the in-flight scan reports its own failure */
      }
    }
    const job = performScan(false);
    scanInFlightRef.current = job;
    try {
      await job;
    } finally {
      if (scanInFlightRef.current === job) scanInFlightRef.current = null;
    }
  }, [performScan]);

  const scanQuiet = useCallback(
    (force = false) => {
      if (scanInFlightRef.current) return;
      if (!force && navTtlRef.current.isFresh("library-scan", LIBRARY_SCAN_INTERVAL_MS)) {
        return;
      }
      const job = performScan(true);
      scanInFlightRef.current = job;
      void job.finally(() => {
        if (scanInFlightRef.current === job) scanInFlightRef.current = null;
      });
    },
    [performScan],
  );

  useNavSettledEffect(
    tab === "library",
    150,
    () => {
      if (!document.hidden) scanQuiet(false);
      const tick = () => {
        if (document.hidden) return;
        scanQuiet(true);
      };
      const id = window.setInterval(tick, LIBRARY_SCAN_INTERVAL_MS);
      const onVisible = () => {
        if (document.hidden || tabRef.current !== "library") return;
        scanQuiet(false);
      };
      document.addEventListener("visibilitychange", onVisible);
      return () => {
        window.clearInterval(id);
        document.removeEventListener("visibilitychange", onVisible);
      };
    },
    [scanQuiet],
  );

  useEffect(() => {
    let cancelled = false;
    (async () => {
      let choseTab = false;
      try {
        const s = await refreshSettings();
        if (!cancelled && tabRef.current == null) {
          setTab(s.initial_setup_complete ? "library" : "setup");
          choseTab = true;
        }
        // Start before the next await so the Library tab effect sees this
        // scan in flight and does not run a second one.
        const initialScan = scan();
        await refreshManaged();
        await refreshOrphanScan();
        await initialScan;
        if (s.has_api_key) {
          try {
            await api.validateUser();
            await refreshSettings();
          } catch {
            /* key may be stale */
          }
        }
        if (!cancelled) {
          try {
            const pending = await api.takePendingEmperorModLink();
            if (pending) openEmperorModLinkRef.current(pending);
          } catch {
            /* cold-start deep link is best-effort */
          }
          try {
            const pendingFriend = await api.takePendingEmperorFriendLink();
            if (pendingFriend) openEmperorFriendLinkRef.current(pendingFriend);
          } catch {
            /* cold-start friend deep link is best-effort */
          }
        }
      } catch (e) {
        if (cancelled) return;
        setError(String(e));
        if (!choseTab && tabRef.current == null) setTab("setup");
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [refreshManaged, refreshOrphanScan, refreshSettings, scan]);

  useEffect(() => {
    if (!settings) return;
    if (!settings.initial_setup_complete) {
      if (tab !== "setup") setTab("setup");
      return;
    }
    if (tab === "setup") setTab("library");
  }, [settings, tab]);

  useEffect(() => {
    if (!settings?.initial_setup_complete) return;
    void checkForAppUpdate(true);
  }, [settings?.initial_setup_complete, checkForAppUpdate]);

  useNavSettledEffect(
    tab === "settings",
    150,
    () => {
      void loadAppInfo();
      if (!navTtlRef.current.isFresh("app-update", 10 * 60_000)) {
        void checkForAppUpdate(true);
      }
    },
    [loadAppInfo, checkForAppUpdate],
  );

  useEffect(() => {
    const unlisten = listen<string>("app-update-progress", (event) => {
      setAppUpdateMessage(event.payload);
    });
    return () => {
      unlisten.then((drop) => drop());
    };
  }, []);

  useEffect(() => {
    void applyAppearance(themePref, themeStyle, accentColor);
    if (themePref !== "system" && themeStyle !== "system") return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const onChange = () => {
      void applyAppearance(themePref, themeStyle, accentColor);
    };
    mq.addEventListener("change", onChange);
    const onFocus = () => {
      if (themeStyle === "system") {
        void applyAppearance(themePref, themeStyle, accentColor);
      }
    };
    window.addEventListener("focus", onFocus);
    return () => {
      mq.removeEventListener("change", onChange);
      window.removeEventListener("focus", onFocus);
    };
  }, [themePref, themeStyle, accentColor, linuxSystemBackend]);

  useEffect(() => {
    const nextId = activeGame?.id ?? null;
    const prevId = prevActiveGameIdRef.current;
    const gameChanged = prevId !== nextId;

    if (gameChanged) {
      prevActiveGameIdRef.current = nextId;
      modUpdatesReqRef.current += 1;
      if (checkingModUpdatesTimerRef.current != null) {
        window.clearTimeout(checkingModUpdatesTimerRef.current);
        checkingModUpdatesTimerRef.current = null;
      }
      setCheckingModUpdates(false);
      wineAdviceReqRef.current += 1;
      setWineAdvice(null);
      if (prevId !== null) {
        resetBrowseState();
        resetLibraryEphemeralState();
      }
      savedOpenRef.current += 1;
      gameSavedOpenRef.current += 1;
      setSavedDetail(null);
      setSavedDetailPending(false);
      setSavedInstallGameId(nextId ?? "");
      setGameSavedDetail(null);
      setGameSavedDetailPending(false);
      setMods([]);
      setModsReadyGameId(null);
    }

    if (activeGame) {
      refreshMods(activeGame.id).catch((e) => setError(String(e)));
      refreshCollections(activeGame.id).catch((e) => setError(String(e)));
      setModUpdates({});
    } else {
      setModUpdates({});
      setWineAdvice(null);
    }

    const pending = pendingDownloadPageRef.current;
    if (pending && activeGame?.id === pending.gameId) {
      pendingDownloadPageRef.current = null;
      setBrowseReturnTab("downloads");
      setTab("browse");
      void openMod(pending.hit);
    }
    const pendingEmperor = pendingEmperorModRef.current;
    if (pendingEmperor && activeGame?.id === pendingEmperor.gameId) {
      pendingEmperorModRef.current = null;
      setTab("browse");
      void openMod(pendingEmperor.hit, "info", {
        navOverrides: { tab: "browse" },
      });
    }
  }, [
    activeGame,
    refreshMods,
    refreshCollections,
    resetBrowseState,
    resetLibraryEphemeralState,
  ]);

  useEffect(() => {
    if (!activeGame || !libraryDetail) return;
    if (libraryDetail.game.id === activeGame.id) return;
    setLibraryDetail({ ...libraryDetail, game: activeGame });
    void loadGameInfo(activeGame.nexus_domain);
  }, [activeGame, libraryDetail, loadGameInfo]);

  const browseResultsVisible = tab === "browse" && browseDetail == null;
  browseResultsVisibleRef.current = browseResultsVisible;
  if (browseResultsVisible) {
    browseScrollArmedEpochRef.current = browseScrollEpochRef.current;
  }
  useLayoutEffect(() => {
    const main = mainScrollRef.current;
    if (browseResultsVisible) {
      main?.scrollTo(0, 0);
      browseResultsRef.current?.scrollTo(0, browseScrollRef.current);
    } else {
      main?.scrollTo(0, 0);
    }
  }, [browseResultsVisible, activeGame?.id]);

  useLayoutEffect(() => {
    libraryDetailScrollRef.current?.scrollTo(0, 0);
  }, [libraryDetail?.game.id]);

  useLayoutEffect(() => {
    browseDetailScrollRef.current?.scrollTo(0, 0);
  }, [browseDetail]);

  const libraryShellKey = libraryDetail ? `game:${libraryDetail.game.id}` : "library-home";
  const collectionsViewKey = savedDetail
    ? `saved:${savedDetail.entry.id}`
    : "saved-list";
  const browseChromeReady =
    browseDetail == null && browseViewSettled === "browse-results";
  const showBrowseGrid = useReveal(
    browseResultsVisible && browseChromeReady && !browseResultsPending,
  );
  const showLibraryHome = useReveal(
    libraryShellSettled === "library-home" && libraryListsReady,
  );
  const showMods = useReveal(
    libraryDetail != null &&
      libraryShellSettled === libraryShellKey &&
      activeGame != null &&
      modsReadyGameId === activeGame.id,
  );
  const showCollectionsList = useReveal(
    collectionsViewSettled === "saved-list" && savedCollectionsReady,
  );
  const showSavedMods = useReveal(
    savedDetail != null &&
      collectionsViewSettled === collectionsViewKey &&
      !savedDetailPending,
  );
  const showGameSavedMods = useReveal(
    gameSavedDetail != null && !gameSavedDetailPending,
  );
  const browseSkeletonCount = (() => {
    const loaded = browseMode === "mods" ? modHits.length : collectionHits.length;
    return loaded > 0 ? Math.min(browsePageCount, loaded) : browsePageCount;
  })();

  useLayoutEffect(() => {
    if (!showBrowseGrid) return;
    browseResultsRef.current?.scrollTo(0, browseScrollRef.current);
  }, [showBrowseGrid]);

  useNavSettledEffect(
    tab === "library" && libraryDetail != null && Boolean(activeGame?.id),
    150,
    () => {
      const gameId = activeGame?.id;
      if (!gameId || pictureFillInFlightRef.current) return;
      const pendingIds = mods
        .filter(
          (m) =>
            m.picture_url == null &&
            !pictureFillSkippedRef.current.has(`${gameId}:${m.id}`),
        )
        .map((m) => m.id);
      if (pendingIds.length === 0) return;
      pictureFillInFlightRef.current = true;
      const requested = new Set(pendingIds);
      void api
        .fillStagedPictures(gameId)
        .then((list) => {
          if (activeGameRef.current?.id !== gameId) return;
          const pictures = new Map(list.map((m) => [m.id, m.picture_url]));
          startTransition(() => {
            setMods((current) =>
              current.map((m) => {
                if (m.picture_url != null || !requested.has(m.id)) return m;
                const url = pictures.get(m.id);
                if (url == null) {
                  pictureFillSkippedRef.current.add(`${gameId}:${m.id}`);
                  return m;
                }
                return { ...m, picture_url: url };
              }),
            );
          });
        })
        .catch((e) => {
          console.warn("fill_staged_pictures failed", e);
          for (const id of requested) {
            pictureFillSkippedRef.current.add(`${gameId}:${id}`);
          }
        })
        .finally(() => {
          pictureFillInFlightRef.current = false;
          setPictureFillTick((n) => n + 1);
        });
    },
    [activeGame?.id, mods, pictureFillTick],
  );

  useNavSettledEffect(
    tab === "library" && libraryDetail != null && Boolean(activeGame),
    150,
    () => {
      if (!activeGame) return;
      const gameId = activeGame.id;
      const ttlKey = `mod-updates:${gameId}`;
      let cancelled = false;
      const poll = (force: boolean) => {
        if (cancelled) return;
        if (!force && navTtlRef.current.isFresh(ttlKey, 15 * 60_000)) return;
        navTtlRef.current.mark(ttlKey);
        void refreshModUpdates(gameId);
      };
      poll(false);
      const id = window.setInterval(() => poll(true), 15 * 60 * 1000);
      return () => {
        cancelled = true;
        window.clearInterval(id);
      };
    },
    [activeGame?.id, refreshModUpdates],
  );

  const stagedDisabledRequirements = useMemo(
    () => disabledRequirementWarnings(mods, missingRequirements),
    [mods, missingRequirements],
  );
  const ignoredRequirements = activeGame?.ignored_requirements ?? EMPTY_IGNORED;
  const showGameVersionWarnings = settings?.game_version_warnings !== false;
  const versionWarnings = useMemo(() => {
    if (!showGameVersionWarnings) return [];
    const enabled = new Set(mods.filter((mod) => mod.enabled).map((mod) => mod.id));
    const names = new Map(mods.map((mod) => [mod.id, mod.name]));
    return withoutIgnoredVersions(
      (gameVersionReport?.mods ?? [])
        .filter((row) => row.status === "mismatch" && enabled.has(row.staged_id))
        .map((row) => ({
          stagedId: row.staged_id,
          name: names.get(row.staged_id) ?? "Mod",
          message: row.message,
        })),
      ignoredRequirements,
    );
  }, [mods, gameVersionReport, showGameVersionWarnings, ignoredRequirements]);
  const versionWarningByModId = useMemo(() => {
    const map = new Map<string, (typeof versionWarnings)[number]>();
    for (const row of versionWarnings) {
      map.set(row.stagedId, row);
    }
    return map;
  }, [versionWarnings]);
  const modalDisabledRequirements = useMemo(
    () =>
      requirementsModal
        ? disabledRequirementWarnings(mods, requirementsModal)
        : [],
    [mods, requirementsModal],
  );
  const visibleMissingRequirements = useMemo(
    () =>
      missingRequirements
        ? withoutIgnoredRequirements(missingRequirements, ignoredRequirements)
        : null,
    [missingRequirements, ignoredRequirements],
  );
  const visibleDisabledRequirements = useMemo(
    () => withoutIgnoredDisabled(stagedDisabledRequirements, ignoredRequirements),
    [stagedDisabledRequirements, ignoredRequirements],
  );

  const nexusModsKey = useMemo(
    () =>
      mods
        .filter((m) => stagedModSource(m) === "nexus" && m.nexus_mod_id)
        .map((m) => `${m.domain}:${m.nexus_mod_id}:${m.nexus_file_id}`)
        .join("|"),
    [mods],
  );

  useEffect(() => {
    if (!activeGame || !nexusModsKey) {
      setMissingRequirements(null);
    }
  }, [activeGame, nexusModsKey]);

  useNavSettledEffect(
    tab === "library" &&
      libraryDetail != null &&
      Boolean(activeGame) &&
      Boolean(nexusModsKey),
    missingRequirements ? 150 : 0,
    () => {
      if (!activeGame) return;
      void refreshMissingRequirements(activeGame.id);
    },
    [activeGame?.id, nexusModsKey, refreshMissingRequirements],
  );

  const stagedVersionKey = useMemo(
    () => mods.map((mod) => `${mod.id}:${mod.nexus_mod_id}:${mod.collection_ids?.join(",") ?? ""}`).join("|"),
    [mods],
  );

  useEffect(() => {
    setGameVersionDraft(gameVersionReport?.current ?? "");
  }, [gameVersionReport]);

  useNavSettledEffect(
    tab === "library" && libraryDetail != null && Boolean(activeGame),
    gameVersionReport ? 150 : 0,
    () => {
      if (!activeGame) return;
      void refreshGameVersions(activeGame.id);
    },
    [activeGame?.id, stagedVersionKey, refreshGameVersions],
  );

  useEffect(() => {
    setModsQuery("");
    setModsStatusFilter("all");
    setModsSourceFilter("all");
    setModsSort("loadOrder");
    setModsCollectionFilter("");
    setModsFiltersOpen(false);
    setGameSettingsMenuOpen(false);
  }, [libraryDetail?.game.id]);

  useEffect(() => {
    setSourceFilter((prev) =>
      clampCatalogSourceFilter(prev, activeGame, browseMode),
    );
  }, [activeGame, browseMode]);

  useEffect(() => {
    setLibraryDetail((prev) => {
      if (!prev) return prev;
      const still = managed.find((g) => g.id === prev.game.id);
      if (!still) return null;
      if (still === prev.game) return prev;
      return { ...prev, game: still };
    });
  }, [managed]);

  useEffect(() => {
    if (!activeGame || tab !== "browse") return;
    const domain = activeGame.nexus_domain;
    if (!domain) {
      setBrowseMeta(null);
      return;
    }
    const cached = browseMetaCacheRef.current.get(domain);
    if (cached) {
      setBrowseMeta((prev) => (prev === cached ? prev : cached));
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const meta = await api.browseMeta(domain);
        browseMetaCacheRef.current.set(domain, meta);
        if (!cancelled) setBrowseMeta(meta);
      } catch {
        if (!cancelled) setBrowseMeta(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeGame?.nexus_domain, tab]);

  useEffect(() => {
    if (tab !== "browse") setTagsOpen(false);
  }, [tab]);

  useEffect(() => {
    if (!tagsOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (tagPickerRef.current && !tagPickerRef.current.contains(e.target as Node)) {
        setTagsOpen(false);
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setTagsOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    const id = window.setTimeout(() => tagSearchRef.current?.focus(), 0);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
      window.clearTimeout(id);
    };
  }, [tagsOpen]);

  useEffect(() => {
    if (!authorSuggestOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (
        authorSuggestRef.current &&
        !authorSuggestRef.current.contains(e.target as Node)
      ) {
        setAuthorSuggestOpen(false);
      }
    };
    document.addEventListener("mousedown", onPointerDown);
    return () => document.removeEventListener("mousedown", onPointerDown);
  }, [authorSuggestOpen]);

  useEffect(() => {
    if (tab !== "browse" || browseMode !== "mods" || !activeGame) {
      setAuthorSuggestions([]);
      setAuthorSuggestOpen(false);
      setAuthorSuggestLoading(false);
      return;
    }
    const prefix = parseAuthorQuery(searchQuery);
    if (prefix === null) {
      setAuthorSuggestions([]);
      setAuthorSuggestOpen(false);
      setAuthorSuggestLoading(false);
      return;
    }
    if (prefix.length < 2) {
      authorSuggestReqRef.current += 1;
      setAuthorSuggestions([]);
      setAuthorSuggestOpen(false);
      setAuthorSuggestLoading(false);
      return;
    }
    const reqId = ++authorSuggestReqRef.current;
    setAuthorSuggestLoading(true);
    const timer = window.setTimeout(() => {
      void (async () => {
        try {
          const hits = await api.suggestAuthors(activeGame.id, prefix);
          if (authorSuggestReqRef.current !== reqId) return;
          setAuthorSuggestions(hits);
          setAuthorSuggestIndex(0);
          setAuthorSuggestOpen(true);
        } catch {
          if (authorSuggestReqRef.current !== reqId) return;
          setAuthorSuggestions([]);
          setAuthorSuggestOpen(false);
        } finally {
          if (authorSuggestReqRef.current === reqId) {
            setAuthorSuggestLoading(false);
          }
        }
      })();
    }, 300);
    return () => {
      window.clearTimeout(timer);
    };
  }, [tab, browseMode, activeGame?.id, searchQuery]);

  useEffect(() => {
    if (!launchMenuOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (
        launchMenuRef.current &&
        !launchMenuRef.current.contains(e.target as Node)
      ) {
        setLaunchMenuOpen(false);
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setLaunchMenuOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [launchMenuOpen]);

  useEffect(() => {
    let active = true;
    const unlistenPromise = listen<GameLaunchState>("game-launch-state", (event) => {
      if (!active) return;
      const next = event.payload;
      setLaunchStates((prev) => ({ ...prev, [next.game_id]: next }));
      if (next.phase === "idle") {
        void api
          .hasActiveDeployment(next.game_id)
          .then((hasDep) => {
            if (active) {
              setActiveDeploymentByGame((prev) => ({ ...prev, [next.game_id]: hasDep }));
            }
          })
          .catch(() => {});
        if (next.error) {
          setError(next.error);
        } else if (next.mode === "modded") {
          setNotice({
            kind: "ok",
            message: "Game closed; reverted mod deployment.",
          });
        }
        void refreshWineAdvice(next.game_id);
      } else if (next.mode === "modded" && next.phase === "running") {
        setActiveDeploymentByGame((prev) => ({ ...prev, [next.game_id]: true }));
      }
    });
    return () => {
      active = false;
      void unlistenPromise.then((u) => u());
    };
  }, []);

  useEffect(() => {
    const gameId = activeGame?.id;
    if (!gameId) return;
    let active = true;
    void api
      .getLaunchState(gameId)
      .then((st) => {
        if (active) setLaunchStates((prev) => ({ ...prev, [gameId]: st }));
      })
      .catch(() => {});
    void api
      .hasActiveDeployment(gameId)
      .then((hasDep) => {
        if (active) setActiveDeploymentByGame((prev) => ({ ...prev, [gameId]: hasDep }));
      })
      .catch(() => {});
    return () => {
      active = false;
    };
  }, [activeGame?.id]);

  useEffect(() => {
    if (!gameSettingsMenuOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (
        gameSettingsMenuRef.current &&
        !gameSettingsMenuRef.current.contains(e.target as Node)
      ) {
        setGameSettingsMenuOpen(false);
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setGameSettingsMenuOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [gameSettingsMenuOpen]);

  useEffect(() => {
    navHistoryRef.current = new AppNavHistory(captureNavEntry());
    syncNavCanBack();
  }, []);

  useEffect(() => {
    if (skipHistoryPushRef.current || navBusyRef.current) return;
    navHistoryRef.current?.replaceCurrent(captureNavEntry());
    syncNavCanBack();
  }, [tab, browseDetail, browseReturnTab, libraryDetail, gameSavedDetail, savedDetail]);

  useEffect(() => {
    return attachMouseNavigationListeners((direction) => {
      if (!setupCompleteRef.current) return;
      if (navBusyRef.current) return;
      if (direction === "back") {
        if (tryCloseOverlayOnBackRef.current()) return;
        void navBackRef.current();
        return;
      }
      void navForwardRef.current();
    });
  }, []);

  useEffect(() => {
    // Re-bind whenever the sentinel element mounts (after ViewSwitch / useReveal / new search).
    if (tab !== "browse" || browseDetail || !browseHasMore) return;
    const sentinel = browseSentinelEl;
    const root = browseResultsRef.current;
    if (!sentinel || !root) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries.some((e) => e.isIntersecting);
        if (!visible) {
          browseAppendFailedRef.current = false;
          browseAppendStuckRef.current = false;
          return;
        }
        if (
          browseLoadingMoreRef.current ||
          browseAppendStuckRef.current ||
          !browseHasMoreRef.current ||
          browseAppendFailedRef.current ||
          !isBrowseSentinelNearView(0)
        ) {
          return;
        }
        browseSentinelTopRef.current = sentinelContentTop(
          sentinel.getBoundingClientRect().top,
          root.getBoundingClientRect().top,
          root.scrollTop,
        );
        void runSearch(
          browseModeRef.current,
          searchQueryRef.current,
          browseOptsRef.current,
          true,
        );
      },
      { root, rootMargin: "0px", threshold: 0 },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
    // Re-bind when the view or sentinel node changes, not when a page lands. A new length
    // would observe again and immediately append while the sentinel is stuck.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runSearch reads latest via refs
  }, [tab, browseDetail, browseMode, browseView, browseHasMore, browseSentinelEl]);

  const isPremium = settings?.user?.is_premium ?? false;

  const resolveAssistMeasureHost = useCallback((): HTMLElement | null => {
    if (nexusLoginOpenRef.current) return nexusLoginHostRef.current;
    if (tabRef.current === "downloads") return assistHostRef.current;
    if (assistActiveRef.current) return sidebarAssistHostRef.current;
    return null;
  }, []);

  const syncAssistHost = useCallback(async () => {
    const gen = assistSyncGenRef.current;
    const host = resolveAssistMeasureHost();
    if (!host) {
      if (!assistActiveRef.current && !nexusLoginOpenRef.current) {
        if (assistVisibleSentRef.current !== false) {
          assistVisibleSentRef.current = false;
          try {
            await api.setAssistVisible(false);
          } catch {
            /* ignore */
          }
        }
      }
      return;
    }
    const bounds = measureHostBounds(host);
    if (!bounds) return;

    try {
      await api.setAssistBounds(bounds);
      if (gen !== assistSyncGenRef.current) return;
      if (
        nexusLoginOpenRef.current ||
        tabRef.current === "downloads" ||
        assistActiveRef.current
      ) {
        assistVisibleSentRef.current = true;
        await api.setAssistVisible(true);
      }
    } catch {
      /* host may not be ready yet */
    }
  }, [resolveAssistMeasureHost]);

  const waitForAssistMeasureHost = useCallback(
    async (maxMs = 2500) => {
      const stayOnPage = settings?.install_click_behavior === "stay";
      const useSidebar =
        stayOnPage && tabRef.current !== "downloads";
      if (!useSidebar) {
        setTab("downloads");
      }
      const deadline = Date.now() + maxMs;
      while (Date.now() < deadline) {
        await new Promise<void>((resolve) => {
          requestAnimationFrame(() => resolve());
        });
        const host = useSidebar
          ? sidebarAssistHostRef.current
          : assistHostRef.current;
        if (!host) continue;
        const bounds = measureHostBounds(host);
        if (!bounds) continue;
        try {
          await api.setAssistBounds(bounds);
          return true;
        } catch {
          /* retry */
        }
      }
      return false;
    },
    [settings?.install_click_behavior],
  );

  useEffect(() => {
    if (nexusLoginOpen) return;
    if (!assistActive && tab !== "downloads") {
      assistSyncGenRef.current += 1;
      if (assistVisibleSentRef.current !== false) {
        assistVisibleSentRef.current = false;
        api.setAssistVisible(false).catch(() => {});
      }
      return;
    }
    let rafId = 0;
    const syncSoon = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        if (nexusLoginOpenRef.current) return;
        void syncAssistHost();
      });
    };
    syncSoon();
    const onResize = () => syncSoon();
    window.addEventListener("resize", onResize);
    window.addEventListener("scroll", onResize, { capture: true, passive: true });
    window.visualViewport?.addEventListener("resize", onResize);
    window.visualViewport?.addEventListener("scroll", onResize);
    const observer = new ResizeObserver(() => syncSoon());
    const fullHost = assistHostRef.current;
    const miniHost = sidebarAssistHostRef.current;
    if (fullHost) observer.observe(fullHost);
    if (miniHost) observer.observe(miniHost);
    return () => {
      assistSyncGenRef.current += 1;
      if (rafId) window.cancelAnimationFrame(rafId);
      window.removeEventListener("resize", onResize);
      window.removeEventListener("scroll", onResize, true);
      window.visualViewport?.removeEventListener("resize", onResize);
      window.visualViewport?.removeEventListener("scroll", onResize);
      observer.disconnect();
    };
  }, [tab, syncAssistHost, assistActive, assistQueue?.head, nexusLoginOpen]);

  const syncNexusLoginHost = useCallback(async () => {
    const host = nexusLoginHostRef.current;
    if (!host || !nexusLoginOpenRef.current) return;
    const bounds = measureHostBounds(host);
    if (!bounds || !nexusLoginOpenRef.current) return;
    try {
      await api.setAssistBounds(bounds);
      if (!nexusLoginOpenRef.current) return;
      await api.setAssistVisible(true);
    } catch {
      /* host may not be ready yet */
    }
  }, []);

  const closeNexusLogin = useCallback(() => {
    // Invalidate any in-flight open before tearing down the native webview.
    nexusLoginGenRef.current += 1;
    nexusLoginOpenRef.current = false;
    setNexusLoginConfirming(false);
    setNexusLoginOpen(false);
    api.closeDownloadAssist().catch(() => {});
  }, []);

  const confirmNexusLogin = useCallback(async () => {
    if (!nexusLoginOpenRef.current || nexusLoginConfirming) return;
    setNexusLoginConfirming(true);
    try {
      await api.confirmNexusWebsiteLogin();
      // Backend emits nexus-login-complete; listener closes the modal.
    } catch (e) {
      setError(String(e));
    } finally {
      setNexusLoginConfirming(false);
    }
  }, [nexusLoginConfirming]);

  useEffect(() => {
    if (!nexusLoginOpen) return;
    const gen = nexusLoginGenRef.current + 1;
    nexusLoginGenRef.current = gen;
    nexusLoginOpenRef.current = true;
    const stillOpen = () =>
      gen === nexusLoginGenRef.current && nexusLoginOpenRef.current;
    const syncSoon = () => {
      requestAnimationFrame(() => {
        if (!stillOpen()) return;
        void syncNexusLoginHost();
      });
    };
    syncSoon();
    const open = async () => {
      await new Promise<void>((resolve) => {
        requestAnimationFrame(() => resolve());
      });
      if (!stillOpen()) return;
      await syncNexusLoginHost();
      if (!stillOpen()) return;
      if (nexusLoginAlreadyOpenRef.current) {
        nexusLoginAlreadyOpenRef.current = false;
        return;
      }
      try {
        if (webLoginProvider === "modio") {
          await api.openModioWebsiteLogin();
        } else {
          await api.openNexusWebsiteLogin();
        }
        // Open can finish after the user dismissed the modal; destroy the orphan.
        if (!stillOpen()) {
          await api.closeDownloadAssist().catch(() => {});
          return;
        }
        await syncNexusLoginHost();
      } catch (e) {
        if (stillOpen()) {
          nexusLoginOpenRef.current = false;
          setNexusLoginOpen(false);
          setError(String(e));
        }
      }
    };
    void open();
    window.addEventListener("resize", syncSoon);
    window.addEventListener("scroll", syncSoon, true);
    window.visualViewport?.addEventListener("resize", syncSoon);
    window.visualViewport?.addEventListener("scroll", syncSoon);
    const observer = nexusLoginHostRef.current
      ? new ResizeObserver(() => syncSoon())
      : null;
    if (nexusLoginHostRef.current && observer) {
      observer.observe(nexusLoginHostRef.current);
    }
    return () => {
      window.removeEventListener("resize", syncSoon);
      window.removeEventListener("scroll", syncSoon, true);
      window.visualViewport?.removeEventListener("resize", syncSoon);
      window.visualViewport?.removeEventListener("scroll", syncSoon);
      observer?.disconnect();
      // Backstop: if this session was cancelled, ensure the native webview is gone.
      if (!nexusLoginOpenRef.current) {
        api.closeDownloadAssist().catch(() => {});
      }
    };
  }, [nexusLoginOpen, webLoginProvider, syncNexusLoginHost]);

  useEffect(() => {
    const unlistenLogin = listen("nexus-login-complete", () => {
      if (!nexusLoginOpenRef.current) return;
      nexusLoginGenRef.current += 1;
      nexusLoginOpenRef.current = false;
      setNexusLoginConfirming(false);
      setNexusLoginOpen(false);
      // Do not close the assist webview here: Rust hides it and reuses the
      // authenticated session in the background to capture the API key.
      void refreshSettings();
      setNotice({
        kind: "ok",
        message: "Nexus website session saved. Syncing API key in background…",
      });
    });
    const unlistenPrompt = listen("nexus-api-key-prompt", () => {
      nexusLoginAlreadyOpenRef.current = true;
      setWebLoginProvider("nexus-key");
      setNexusLoginOpen(true);
    });
    const unlistenKey = listen("nexus-api-key-captured", () => {
      if (nexusLoginOpenRef.current) {
        nexusLoginGenRef.current += 1;
        nexusLoginOpenRef.current = false;
        setNexusLoginConfirming(false);
        setNexusLoginOpen(false);
      }
      void refreshSettings();
      setNotice({
        kind: "ok",
        message: "Nexus account connected and API key saved.",
      });
    });
    const unlistenModio = listen("modio-api-key-captured", () => {
      if (nexusLoginOpenRef.current) {
        nexusLoginGenRef.current += 1;
        nexusLoginOpenRef.current = false;
        setNexusLoginConfirming(false);
        setNexusLoginOpen(false);
      }
      void refreshSettings();
      setNotice({
        kind: "ok",
        message: "mod.io account connected and API key saved.",
      });
    });
    return () => {
      unlistenLogin.then((drop) => drop());
      unlistenPrompt.then((drop) => drop());
      unlistenKey.then((drop) => drop());
      unlistenModio.then((drop) => drop());
    };
  }, [refreshSettings]);

  const setAssistQueueState = useCallback((next: AssistQueueState | null) => {
    assistQueueRef.current = next;
    setAssistQueue(next);
  }, []);

  const finishAssistQueue = useCallback(async () => {
    const q = assistQueueRef.current;
    setAssistQueueState(null);
    setAssistActive(false);
    setAssistHint(null);
    setAssistLoginBanner(false);
    setBusy(null);
    const list = await api.listDownloads();
    setDownloads(list);
    if (q && !q.cancelled) {
      const stillActive = list.some(
        (d) =>
          d.batch_id === q.batchId &&
          (d.status === "downloading" ||
              d.status === "extracting" ||
              d.status === "paused"),
      );
      if (stillActive) {
        setActiveBatch({
          id: q.batchId,
          label: q.label,
          collection: q.collection,
          emperorShare: q.emperorShare,
        });
      } else {
        setActiveBatch((prev) => (prev?.id === q.batchId ? null : prev));
      }
    } else if (q?.cancelled) {
      setActiveBatch((prev) => (prev?.id === q.batchId ? null : prev));
    }
    const game = activeGameRef.current;
    if (game) {
      await refreshMods(game.id);
      await refreshCollections(game.id);
    }
    if (!q) return;
    if (q.collection && !q.cancelled) {
      try {
        await api.recordNexusCollection({
          gameId: q.entries[0]?.gameId ?? game?.id ?? "",
          slug: q.collection.slug,
          name: q.collection.name,
          revision: q.collection.revision,
          files: q.collection.files,
          existingModIds: q.collection.existingModIds,
        });
        if (game) await refreshCollections(game.id);
      } catch (e) {
        if (!String(e).includes("No staged mods matched")) {
          setError(String(e));
        }
      }
    }
    if (q.emperorShare && !q.cancelled) {
      try {
        const targetGameId = q.entries[0]?.gameId ?? game?.id ?? "";
        if (q.emperorShare.friendSync && q.emperorShare.code) {
          await api.finalizeFriendCollectionSync({
            gameId: targetGameId,
            peerId: q.emperorShare.friendSync.peerId,
            friendCollectionId: q.emperorShare.friendSync.friendCollectionId,
            shareCode: q.emperorShare.code,
          });
        } else {
          await api.recordEmperorShare({
            gameId: targetGameId,
            collectionId: q.emperorShare.collectionId,
            name: q.emperorShare.name,
            code: q.emperorShare.code,
            files: q.emperorShare.files,
            memberIds: q.emperorShare.memberIds,
            existingModIds: q.emperorShare.existingModIds,
          });
        }
        if (game) await refreshCollections(game.id);
      } catch (e) {
        if (!String(e).includes("No staged mods matched")) {
          setError(String(e));
        }
      }
    }
    if (q.cancelled) {
      setError(`Download queue cancelled after ${q.completed}/${q.entries.length} mods.`);
      return;
    }
    if (q.failures.length > 0) {
      const preview = q.failures
        .slice(0, 3)
        .map((f) => `${f.name}: ${f.reason}`)
        .join("; ");
      setError(
        `${q.label ?? "Queue"}: ${q.completed} started, ${q.failures.length} failed. ${preview}${
          q.failures.length > 3 ? "…" : ""
        }`,
      );
    } else if (q.completed > 0) {
      setError(null);
    }
  }, [refreshMods, refreshCollections, setAssistQueueState]);

  const openAssistAtHead = useCallback(
    async (q: AssistQueueState) => {
      const entry = q.entries[q.head];
      if (!entry) {
        await finishAssistQueue();
        return;
      }
      setAssistActive(true);
      assistActiveRef.current = true;
      await waitForAssistMeasureHost();
      await syncAssistHost();
      setBusy(`Download Assist — ${entry.name}`);
      await api.openDownloadAssist({
        gameId: entry.gameId,
        domain: entry.domain,
        modId: entry.modId,
        fileId: entry.fileId,
        name: entry.name,
        batchId: q.batchId,
        replaceStagedId: entry.replaceStagedId ?? null,
        version: entry.version ?? null,
        fileName: entry.fileName ?? null,
      });
      await syncAssistHost();
    },
    [finishAssistQueue, syncAssistHost, waitForAssistMeasureHost],
  );

  const advanceAssistAfterStart = useCallback(async () => {
    const q = assistQueueRef.current;
    if (!q) {
      await refreshDownloads();
      setAssistActive(false);
      setBusy(null);
      return;
    }
    if (q.cancelled) {
      await finishAssistQueue();
      return;
    }
    q.completed += 1;
    q.head += 1;
    setAssistQueueState({ ...q });
    await refreshDownloads();
    if (q.head >= q.entries.length) {
      await finishAssistQueue();
      return;
    }
    try {
      await openAssistAtHead(q);
    } catch (e) {
      q.failures.push({
        name: q.entries[q.head]?.name ?? "Unknown",
        reason: String(e),
      });
      await finishAssistQueue();
    }
  }, [finishAssistQueue, openAssistAtHead, refreshDownloads, setAssistQueueState]);

  const advanceAssistAfterFailure = useCallback(
    async (reason: string) => {
      const q = assistQueueRef.current;
      if (!q) {
        setAssistActive(false);
        setBusy(null);
        return;
      }
      if (q.cancelled) {
        await finishAssistQueue();
        return;
      }
      const entry = q.entries[q.head];
      q.failures.push({ name: entry?.name ?? "Unknown", reason });
      q.head += 1;
      setAssistQueueState({ ...q });
      if (q.head >= q.entries.length) {
        await finishAssistQueue();
        return;
      }
      try {
        await openAssistAtHead(q);
      } catch (e) {
        q.failures.push({
          name: q.entries[q.head]?.name ?? "Unknown",
          reason: String(e),
        });
        await finishAssistQueue();
      }
    },
    [finishAssistQueue, openAssistAtHead, setAssistQueueState],
  );

  const skipAssistCurrent = useCallback(async () => {
    const q = assistQueueRef.current;
    if (!q || q.cancelled) return;
    const entry = q.entries[q.head];
    if (entry) {
      q.failures.push({ name: entry.name, reason: "Skipped by user" });
    }
    q.head += 1;
    setAssistQueueState({ ...q });
    try {
      await api.closeDownloadAssist();
    } catch {
      /* ignore */
    }
    if (q.head >= q.entries.length) {
      await finishAssistQueue();
      return;
    }
    try {
      await openAssistAtHead(q);
    } catch (e) {
      setError(String(e));
    }
  }, [finishAssistQueue, openAssistAtHead, setAssistQueueState]);

  const cancelAssistQueue = useCallback(async () => {
    const q = assistQueueRef.current;
    if (!q) return;
    q.cancelled = true;
    setAssistQueueState({ ...q });
    try {
      await api.closeDownloadAssist();
    } catch {
      /* ignore */
    }
    try {
      await api.cancelDownloadBatch(q.batchId);
    } catch {
      /* ignore */
    }
    await finishAssistQueue();
  }, [finishAssistQueue, setAssistQueueState]);

  const cancelRemainingBatch = useCallback(async () => {
    const batch = activeBatchRef.current;
    if (!batch) return;
    try {
      await api.cancelDownloadBatch(batch.id);
    } catch (e) {
      setError(String(e));
    }
    setActiveBatch(null);
    await refreshDownloads();
  }, [refreshDownloads]);

  const cancelOneDownload = useCallback(
    async (id: string) => {
      try {
        await api.cancelDownload(id);
      } catch (e) {
        setError(String(e));
      }
      setBusy(null);
      const list = await api.listDownloads();
      setDownloads(list);
      if (!hasActiveDownloads(list)) {
        setBusy(null);
      }
    },
    [],
  );

  const restartOneDownload = useCallback(async (id: string) => {
    dismissDownloadBanners(id);
    try {
      await api.restartDownload(id);
    } catch (e) {
      const msg = String(e);
      if (msg.includes("No restart metadata")) {
        try {
          await api.forceResetDownload(id);
        } catch (resetErr) {
          setError(String(resetErr));
        }
      } else {
        setError(msg);
      }
    }
    setBusy(null);
    downloadProgressAtRef.current.set(id, { bytes: 0, at: Date.now() });
    await refreshDownloads();
  }, [dismissDownloadBanners, refreshDownloads]);

  const retryAssistOpening = useCallback(async () => {
    const q = assistQueueRef.current;
    if (!q || q.cancelled || q.head >= q.entries.length) return;
    try {
      await openAssistAtHead(q);
    } catch (e) {
      setError(String(e));
    }
  }, [openAssistAtHead]);

  const isDownloadStuck = useCallback((id: string, status: string) => {
    if (!isActiveDownloadStatus(status)) return false;
    const entry = downloadProgressAtRef.current.get(id);
    if (!entry) return false;
    return Date.now() - entry.at > 120000;
  }, []);

  const clearRecentDownloads = useCallback(
    async (ids: string[]) => {
      if (!window.confirm("Clear recent downloads?")) return;
      try {
        await api.clearRecentDownloads(ids);
      } catch (e) {
        setError(String(e));
      }
      await refreshDownloads();
    },
    [refreshDownloads],
  );

  const removeOneDownload = useCallback(
    async (id: string) => {
      dismissDownloadBanners(id);
      try {
        await api.removeDownload(id);
      } catch (e) {
        setError(String(e));
      }
      await refreshDownloads();
    },
    [dismissDownloadBanners, refreshDownloads],
  );

  const pauseOneDownload = useCallback(
    async (id: string) => {
      try {
        await api.pauseDownload(id);
      } catch (e) {
        setError(String(e));
      }
      await refreshDownloads();
    },
    [refreshDownloads],
  );

  const resumeOneDownload = useCallback(
    async (id: string) => {
      try {
        await api.resumeDownload(id);
      } catch (e) {
        setError(String(e));
      }
      await refreshDownloads();
    },
    [refreshDownloads],
  );

  const handleAssistHost = useCallback(
    (el: HTMLDivElement | null) => {
      assistHostRef.current = el;
      if (el) {
        void syncAssistHost();
      }
    },
    [syncAssistHost],
  );

  const handleSidebarAssistHost = useCallback(
    (el: HTMLDivElement | null) => {
      sidebarAssistHostRef.current = el;
      if (el) {
        void syncAssistHost();
      }
    },
    [syncAssistHost],
  );

  const enqueueAssistEntries = useCallback(
    async (
      entries: AssistQueueEntry[],
      label: string | null = null,
      collection: AssistQueueState["collection"] = null,
      emperorShare: AssistQueueState["emperorShare"] = null,
    ) => {
      if (entries.length === 0) {
        throw new Error("Nothing to queue.");
      }

      const isDup = (existing: AssistQueueEntry[], e: AssistQueueEntry) =>
        existing.some(
          (x) =>
            (e.replaceStagedId &&
              x.replaceStagedId &&
              x.replaceStagedId === e.replaceStagedId) ||
            (x.gameId === e.gameId &&
              x.domain === e.domain &&
              x.modId === e.modId &&
              x.fileId === e.fileId),
        );

      const existing = assistQueueRef.current;
      if (existing && !existing.cancelled) {
        const toAdd = entries.filter((e) => !isDup(existing.entries, e));
        if (toAdd.length === 0) {
          setError(null);
          return;
        }
        const next: AssistQueueState = {
          ...existing,
          entries: [...existing.entries, ...toAdd],
          label: existing.label ?? label,
          collection: existing.collection ?? collection,
          emperorShare: existing.emperorShare ?? emperorShare,
        };
        setAssistQueueState(next);
        setError(null);
        return;
      }

      const q: AssistQueueState = {
        batchId: crypto.randomUUID(),
        label,
        entries,
        head: 0,
        completed: 0,
        failures: [],
        cancelled: false,
        collection,
        emperorShare,
      };
      setActiveBatch(null);
      setAssistQueueState(q);
      setError(null);
      await openAssistAtHead(q);
    },
    [openAssistAtHead, setAssistQueueState],
  );

  useEffect(() => {
    // A dependency closure emits one event per package; coalesce the bursts.
    let changedTimer: number | null = null;
    const unlistenChanged = listen("downloads-changed", () => {
      if (changedTimer != null) return;
      changedTimer = window.setTimeout(() => {
        changedTimer = null;
        void refreshDownloads();
      }, 150);
    });
    const unlistenNxm = listen<string>("nxm-url", async (event) => {
      const q = assistQueueRef.current;
      setBusy(
        q
          ? `Starting queued download ${q.head + 1}/${q.entries.length}…`
          : "Starting download…",
      );
      setError(null);
      try {
        await api.handleNxm(event.payload);
        downloadStartedAtRef.current = Date.now();
        await refreshDownloads();
      } catch (e) {
        const msg = String(e);
        if (msg.includes("DUPLICATE_NXM")) {
          await refreshDownloads();
          return;
        }
        await refreshDownloads();
        if (assistQueueRef.current) {
          try {
            await api.closeDownloadAssist();
          } catch {
            /* ignore */
          }
          await advanceAssistAfterFailure(msg);
        } else {
          setError(msg);
          setBusy(null);
        }
      }
    });
    const unlistenEmperorMod = listen<EmperorOpenMod>("emperor-open-mod", (event) => {
      void api.takePendingEmperorModLink().catch(() => {
        /* clear cold-start stash so bootstrap take does not reopen */
      });
      openEmperorModLinkRef.current(event.payload);
    });
    const unlistenEmperorFriend = listen<string>("emperor-add-friend", (event) => {
      void api.takePendingEmperorFriendLink().catch(() => {
        /* clear cold-start stash so bootstrap take does not reopen */
      });
      openEmperorFriendLinkRef.current(event.payload);
    });
    const unlistenAssist = listen<string>("assist-download-finished", async (event) => {
      const q = assistQueueRef.current;
      setBusy(
        q
          ? `Importing queued mod ${q.head + 1}/${q.entries.length}…`
          : "Importing Slow Download…",
      );
      setError(null);
      try {
        await api.importAssistDownload(event.payload);
        downloadStartedAtRef.current = Date.now();
        await refreshDownloads();
        await advanceAssistAfterStart();
      } catch (e) {
        await refreshDownloads();
        if (assistQueueRef.current) {
          try {
            await api.closeDownloadAssist();
          } catch {
            /* ignore */
          }
          await advanceAssistAfterFailure(String(e));
        } else {
          setError(String(e));
          setBusy(null);
        }
      }
    });
    const unlistenOpened = listen<{ autoclick: boolean }>("assist-opened", (event) => {
      setAssistActive(true);
      assistActiveRef.current = true;
      setAssistLoginBanner(false);
      const q = assistQueueRef.current;
      const entry = q?.entries[q.head];
      setAssistHint(
        event.payload.autoclick
          ? "Autoclick enabled — opening dialog then Slow download when ready."
          : "Click Mod Manager Download or Slow download in the panel above.",
      );
      requestAnimationFrame(() => {
        void syncAssistHost();
        requestAnimationFrame(() => {
          void syncAssistHost();
        });
      });
      if (q && entry) {
        setBusy(
          event.payload.autoclick
            ? `Assist ${q.head + 1}/${q.entries.length} — ${entry.name} (autoclick)…`
            : `Assist ${q.head + 1}/${q.entries.length} — ${entry.name}…`,
        );
        return;
      }
      setBusy(
        event.payload.autoclick
          ? "Download Assist open — autoclick will press download when ready…"
          : "Download Assist open — complete the download in the panel above…",
      );
    });
    const unlistenNeedsLogin = listen("assist-needs-login", () => {
      setAssistHint(
        "Sign in to Nexus in Download Assist once (use Stay signed in). Session is saved separately from your API key.",
      );
      setBusy("Waiting for Nexus sign-in in Download Assist…");
      setAssistLoginBanner(true);
    });
    const unlistenClosed = listen("assist-closed", async () => {
      if (Date.now() - downloadStartedAtRef.current < 2500) {
        return;
      }
      if (!assistQueueRef.current || assistQueueRef.current.cancelled) {
        setAssistActive(false);
        setBusy(null);
        return;
      }
      await advanceAssistAfterFailure("Assist closed unexpectedly");
    });
    const unlistenDlFinished = listen<{
      game_id?: string;
      id?: string;
      mod_id?: string;
      replaced?: boolean;
    }>("download-finished", async (event) => {
        const list = await api.listDownloads();
        setDownloads(list);
        const batch = activeBatchRef.current;
        if (batch) {
          const still = list.some(
            (d) =>
              d.batch_id === batch.id &&
              (d.status === "downloading" ||
              d.status === "extracting" ||
              d.status === "paused"),
          );
          if (!still) {
            if (batch.collection) {
              try {
                await api.recordNexusCollection({
                  gameId:
                    event.payload.game_id ?? activeGameRef.current?.id ?? "",
                  slug: batch.collection.slug,
                  name: batch.collection.name,
                  revision: batch.collection.revision,
                  files: batch.collection.files,
                  existingModIds: batch.collection.existingModIds,
                });
              } catch {
                /* ignore incomplete match */
              }
            }
            if (batch.emperorShare) {
              try {
                const targetGameId =
                  event.payload.game_id ?? activeGameRef.current?.id ?? "";
                if (batch.emperorShare.friendSync && batch.emperorShare.code) {
                  await api.finalizeFriendCollectionSync({
                    gameId: targetGameId,
                    peerId: batch.emperorShare.friendSync.peerId,
                    friendCollectionId:
                      batch.emperorShare.friendSync.friendCollectionId,
                    shareCode: batch.emperorShare.code,
                  });
                } else {
                  await api.recordEmperorShare({
                    gameId: targetGameId,
                    collectionId: batch.emperorShare.collectionId,
                    name: batch.emperorShare.name,
                    code: batch.emperorShare.code,
                    files: batch.emperorShare.files,
                    memberIds: batch.emperorShare.memberIds,
                    existingModIds: batch.emperorShare.existingModIds,
                  });
                }
              } catch {
                /* ignore incomplete match */
              }
            }
            setActiveBatch(null);
          }
        }
        const gameId = event.payload.game_id ?? activeGameRef.current?.id;
        if (gameId) {
          const stagedId = event.payload.mod_id;
          const parents = stagedId
            ? pendingRequirementLinksRef.current.get(stagedId)
            : undefined;
          if (stagedId && parents && parents.length > 0) {
            pendingRequirementLinksRef.current.delete(stagedId);
            try {
              await api.linkStagedRequirements(gameId, stagedId, parents);
            } catch {
              /* the requirement is staged either way */
            }
          }
          if (event.payload.replaced && stagedId) {
            pendingUpdatedFomodIdsRef.current.add(stagedId);
          }
          try {
            const refreshed = await refreshMods(gameId);
            await refreshCollections(gameId);
            await refreshModUpdates(gameId);
            await refreshMissingRequirements(gameId);
            if (
              refreshed &&
              activeGameRef.current?.id === gameId &&
              pendingUpdatedFomodIdsRef.current.size > 0 &&
              !hasActiveDownloads(list) &&
              !assistQueueRef.current
            ) {
              const ids = Array.from(pendingUpdatedFomodIdsRef.current);
              pendingUpdatedFomodIdsRef.current.clear();
              openUpdatedFomodsRef.current(refreshed.list, refreshed.needing, ids);
            }
          } catch {
            /* ignore */
          }
        }
        if (!hasActiveDownloads(list)) {
          setBusy(null);
        }
      },
    );
    const unlistenDlFailed = listen<{ id?: string; label?: string; error?: string }>(
      "download-failed",
      async (event) => {
        const list = await api.listDownloads();
        setDownloads(list);
        const batch = activeBatchRef.current;
        if (batch) {
          const still = list.some(
            (d) =>
              d.batch_id === batch.id &&
              (d.status === "downloading" ||
              d.status === "extracting" ||
              d.status === "paused"),
          );
          if (!still) setActiveBatch(null);
        }
        const label = event.payload.label ?? "Download";
        const err = event.payload.error ?? "unknown error";
        if (err !== "Reset by user") {
          const failedItem = event.payload.id
            ? list.find((d) => d.id === event.payload.id)
            : undefined;
          setError(
            `${label}: ${err}`,
            failedItem?.can_restart
              ? {
                  label: "Retry",
                  dismiss: true,
                  run: () => {
                    void restartOneDownload(failedItem.id);
                  },
                }
              : undefined,
            event.payload.id ?? null,
          );
        }
        if (!hasActiveDownloads(list)) {
          setBusy(null);
        }
      },
    );
    const unlistenDlCancelled = listen<{ id?: string; label?: string }>(
      "download-cancelled",
      async () => {
        const list = await api.listDownloads();
        setDownloads(list);
        const batch = activeBatchRef.current;
        if (batch) {
          const still = list.some(
            (d) =>
              d.batch_id === batch.id &&
              (d.status === "downloading" ||
              d.status === "extracting" ||
              d.status === "paused"),
          );
          if (!still) setActiveBatch(null);
        }
        if (!hasActiveDownloads(list)) {
          setBusy(null);
        }
      },
    );
    const progressBatcher = createThrottledBatcher<
      string,
      {
        id: string;
        bytes_downloaded: number;
        bytes_total: number | null;
        speed_bps: number;
        status: string;
      }
    >(120, (batch) => {
      setDownloads((prevList) => {
        let next: DownloadItem[] | null = null;
        for (const p of batch.values()) {
          const list = next ?? prevList;
          const idx = list.findIndex((d) => d.id === p.id);
          if (idx < 0) continue;
          // A late progress event must not pull a finished row back into the
          // active list.
          if (isFinishedDownloadStatus(list[idx].status)) continue;
          const current = list[idx];
          const nextStatus = p.status || current.status;
          if (
            current.bytes_downloaded === p.bytes_downloaded &&
            current.bytes_total === p.bytes_total &&
            current.speed_bps === p.speed_bps &&
            current.status === nextStatus
          ) {
            continue;
          }
          if (!next) next = [...prevList];
          next[idx] = {
            ...current,
            bytes_downloaded: p.bytes_downloaded,
            bytes_total: p.bytes_total,
            speed_bps: p.speed_bps,
            status: nextStatus,
          };
        }
        return next ?? prevList;
      });
    });
    const unlistenProgress = listen<{
      id: string;
      bytes_downloaded: number;
      bytes_total: number | null;
      speed_bps: number;
      status: string;
    }>("download-progress", (event) => {
      const p = event.payload;
      const now = Date.now();
      const prev = downloadProgressAtRef.current.get(p.id);
      if (!prev || prev.bytes !== p.bytes_downloaded) {
        downloadProgressAtRef.current.set(p.id, {
          bytes: p.bytes_downloaded,
          at: now,
        });
      }
      const isTerminal = Boolean(p.status && isFinishedDownloadStatus(p.status));
      progressBatcher.push(p.id, p, isTerminal);
    });
    const unlistenStarted = listen("assist-download-started", async () => {
      downloadStartedAtRef.current = Date.now();
      setAssistActive(false);
      setAssistHint(null);
      setAssistLoginBanner(false);
      await refreshDownloads();
      const q = assistQueueRef.current;
      if (q && !q.cancelled && q.head + 1 < q.entries.length) {
        const next = q.entries[q.head + 1];
        setBusy(
          `Queue ${q.head + 2}/${q.entries.length} — opening next${next ? ` (${next.name})` : ""}…`,
        );
      } else if (!q) {
        setBusy(null);
      }
      await advanceAssistAfterStart();
    });
    return () => {
      if (changedTimer != null) window.clearTimeout(changedTimer);
      progressBatcher.cancel();
      unlistenChanged.then((f) => f());
      unlistenNxm.then((f) => f());
      unlistenEmperorMod.then((f) => f());
      unlistenEmperorFriend.then((f) => f());
      unlistenAssist.then((f) => f());
      unlistenOpened.then((f) => f());
      unlistenNeedsLogin.then((f) => f());
      unlistenClosed.then((f) => f());
      unlistenDlFinished.then((f) => f());
      unlistenDlFailed.then((f) => f());
      unlistenDlCancelled.then((f) => f());
      unlistenProgress.then((f) => f());
      unlistenStarted.then((f) => f());
    };
  }, [advanceAssistAfterFailure, advanceAssistAfterStart, refreshDownloads, refreshMods, refreshCollections, refreshModUpdates, refreshMissingRequirements, restartOneDownload, syncAssistHost]);

  async function withBusy<T>(label: string, fn: () => Promise<T>): Promise<T | undefined> {
    setBusy(label);
    setError(null);
    setNotice(null);
    try {
      return await fn();
    } catch (e) {
      setError(String(e));
      return undefined;
    } finally {
      setBusy(null);
    }
  }

  async function saveApiKey() {
    await withBusy("Validating API key…", async () => {
      await api.setApiKey(apiKeyInput);
      setApiKeyInput("");
      await refreshSettings();
      setNotice({ kind: "ok", message: "Nexus API key saved." });
    });
  }

  async function finishSetup() {
    if (
      !setupCanFinish(
        Boolean(settings?.has_api_key),
        Boolean(settings?.nexus_website_signed_in),
      )
    ) {
      return;
    }
    await withBusy("Finishing setup…", async () => {
      await api.completeInitialSetup();
      const next = await refreshSettings();
      setupCompleteRef.current = next.initial_setup_complete === true;
      setTab("library");
    });
  }

  async function clearNexusWebsiteSession() {
    await withBusy("Clearing Nexus website session…", async () => {
      await api.clearAssistSession();
      await refreshSettings();
    });
  }

  async function saveModioApiKey() {
    await withBusy("Validating mod.io API key…", async () => {
      await api.setModioApiKey(modioKeyInput);
      setModioKeyInput("");
      await refreshSettings();
      setNotice({ kind: "ok", message: "mod.io API key saved." });
    });
  }

  async function manage(game: DetectedGame) {
    if (!game.supported || !game.plugin_id || !game.install_path) {
      setError("This game is not supported yet.");
      return;
    }
    if (!game.nexus_domain && game.engine_hint !== "bepinex") {
      setError("This game is not supported yet.");
      return;
    }
    await withBusy(`Managing ${game.title}…`, async () => {
      const suggestion = catalogSuggestions[game.id];
      await api.manageGame({
        id: game.id,
        title: game.title,
        nexusDomain: game.nexus_domain || suggestion?.nexus_domain || "",
        installPath: game.install_path!,
        launcher: game.launcher,
        pluginId: game.plugin_id!,
        ...managedArt(game),
        thunderstoreCommunity: suggestion?.thunderstore_community ?? null,
        modioGameId: suggestion?.modio_game_id ?? null,
      });
      const list = await refreshManaged();
      const managedGame = list.find((g) => g.id === game.id);
      if (managedGame) {
        await openGame(managedGame);
      } else {
        setActiveId(game.id);
      }
    });
  }

  function closeCatalogMatchPrompt() {
    catalogSuggestGen.current += 1;
    setCatalogMatchPrompt(null);
    setCatalogMatchSuggestion(null);
    setCatalogMatchLookingUp(false);
    setCatalogMatchProject("");
  }

  function beginCatalogMatch(game: DetectedGame, engine: CatalogMatchEngine) {
    if (!game.install_path) {
      setError(
        engine === "unreal"
          ? "Install path is required to manage this Unreal game."
          : "Install path is required to manage this Unity game.",
      );
      return;
    }
    setError(null);
    setCatalogMatchPrompt({ game, engine });
    setCatalogMatchProject("");
    const cached = catalogSuggestions[game.id] ?? null;
    setCatalogMatchSuggestion(cached);
    setCatalogMatchLookingUp(true);
    if (engine === "unreal") {
      void api.detectUeLayout(game.install_path).then((layout) => {
        if (layout?.project_name) {
          setCatalogMatchProject(layout.project_name);
        }
      });
    }
    const gen = ++catalogSuggestGen.current;
    void (async () => {
      try {
        const s = await api.suggestCatalogIds(game.title);
        if (catalogSuggestGen.current !== gen) {
          return;
        }
        setCatalogMatchSuggestion(s);
        setCatalogSuggestions((prev) => ({ ...prev, [game.id]: s }));
      } catch {
        if (catalogSuggestGen.current !== gen) {
          return;
        }
        // Keep cached suggestion / empty for manual entry.
      } finally {
        if (catalogSuggestGen.current === gen) {
          setCatalogMatchLookingUp(false);
        }
      }
    })();
  }

  function beginUnrealManage(game: DetectedGame) {
    beginCatalogMatch(game, "unreal");
  }

  function beginBepinexManage(game: DetectedGame) {
    beginCatalogMatch(game, "bepinex");
  }

  async function manageUnrealQuick(game: DetectedGame) {
    const suggestion = catalogSuggestions[game.id];
    if (!catalogReadyToQuickManage(suggestion)) {
      beginUnrealManage(game);
      return;
    }
    if (!game.install_path) {
      setError("Install path is required to manage this Unreal game.");
      return;
    }
    await withBusy(`Managing ${game.title}…`, async () => {
      let projectName: string | null = null;
      try {
        const layout = await api.detectUeLayout(game.install_path!);
        projectName = layout?.project_name ?? null;
      } catch {
        /* project name optional when layout detectable later */
      }
      await api.manageGame({
        id: game.id,
        title: game.title,
        nexusDomain: suggestion!.nexus_domain ?? "",
        installPath: game.install_path!,
        launcher: game.launcher,
        pluginId: "unreal",
        ...managedArt(game),
        projectName,
        thunderstoreCommunity: suggestion!.thunderstore_community ?? null,
        modioGameId: suggestion!.modio_game_id ?? null,
      });
      const list = await refreshManaged();
      const managedGame = list.find((g) => g.id === game.id);
      if (managedGame) {
        await openGame(managedGame);
      } else {
        setActiveId(game.id);
      }
    });
  }

  async function manageBepinexQuick(game: DetectedGame) {
    const suggestion = catalogSuggestions[game.id];
    if (!catalogReadyToQuickManage(suggestion)) {
      beginBepinexManage(game);
      return;
    }
    if (!game.install_path) {
      setError("Install path is required to manage this Unity game.");
      return;
    }
    await withBusy(`Managing ${game.title}…`, async () => {
      await api.manageGame({
        id: game.id,
        title: game.title,
        nexusDomain: suggestion!.nexus_domain ?? "",
        installPath: game.install_path!,
        launcher: game.launcher,
        pluginId: "bepinex",
        ...managedArt(game),
        thunderstoreCommunity: suggestion!.thunderstore_community ?? null,
        modioGameId: suggestion!.modio_game_id ?? null,
      });
      const list = await refreshManaged();
      const managedGame = list.find((g) => g.id === game.id);
      if (managedGame) {
        await openGame(managedGame);
      } else {
        setActiveId(game.id);
      }
    });
  }

  async function confirmCatalogMatch(values: CatalogMatchSubmit) {
    const prompt = catalogMatchPrompt;
    if (!prompt?.game.install_path) {
      return;
    }
    const { game, engine } = prompt;
    if (
      !values.nexusDomain &&
      !values.thunderstoreCommunity &&
      values.modioGameId == null
    ) {
      setError(
        "Enter a Nexus Mods domain, Thunderstore community, and/or mod.io game ID.",
      );
      return;
    }
    await withBusy(`Managing ${game.title}…`, async () => {
      await api.manageGame({
        id: game.id,
        title: game.title,
        nexusDomain: values.nexusDomain,
        installPath: game.install_path!,
        launcher: game.launcher,
        pluginId: engine === "unreal" ? "unreal" : "bepinex",
        ...managedArt(game),
        projectName: engine === "unreal" ? values.projectName : null,
        thunderstoreCommunity: values.thunderstoreCommunity || null,
        modioGameId: values.modioGameId,
      });
      closeCatalogMatchPrompt();
      const list = await refreshManaged();
      const managedGame = list.find((g) => g.id === game.id);
      if (managedGame) {
        await openGame(managedGame);
      } else {
        setActiveId(game.id);
      }
    });
  }

  async function confirmRelink() {
    if (!relinkPrompt?.health.relocate_target_id) {
      return;
    }
    const { game, health } = relinkPrompt;
    await withBusy(`Relinking ${game.title}…`, async () => {
      await api.relinkManagedGame(game.id, health.relocate_target_id!);
      setRelinkPrompt(null);
      const list = await refreshManaged();
      await scan();
      const relinked = list.find((g) => g.id === health.relocate_target_id);
      if (relinked) {
        await openGame(relinked);
      }
      setNotice({
        kind: "ok",
        message: `${game.title} relinked to the new install folder.`,
      });
    });
  }

  function openUnmanageModal(game: ManagedGame) {
    setUnmanageTarget(game);
    setUnmanageStatus(null);
    setUnmanageStatusLoading(true);
    void api
      .getUnmanageCollectionStatus(game.id)
      .then((status) => {
        setUnmanageStatus(status);
      })
      .catch(() => {
        setUnmanageStatus(null);
      })
      .finally(() => {
        setUnmanageStatusLoading(false);
      });
  }

  async function unmanageFromLibrary(
    game: ManagedGame,
    saveToPersonal = false,
  ) {
    await withBusy(
      saveToPersonal ? "Saving collection & removing…" : "Removing…",
      async () => {
        const closedDetail = libraryDetail?.game.id === game.id;
        const wasActive = activeId === game.id;
        if (saveToPersonal) {
          await api.exportShareCode(game.id);
          await refreshSavedCollections();
        }
        await api.unmanageGame(game.id);
        if (closedDetail) {
          closeLibraryDetail();
        }
        const list = await refreshManaged();
        const settings = await refreshSettings();
        const nextActive =
          list.find((g) => g.id === settings.last_active_game_id) ??
          list[0] ??
          null;
        setActiveId(nextActive?.id ?? null);
        if (closedDetail || wasActive) {
          setMods([]);
          setInstalledCollections([]);
          setFriendSyncStates({});
        }
        await scan();
      },
    );
  }


  async function saveUeGameSettings() {
    if (!libraryDetail) {
      return;
    }
    await withBusy("Saving game settings…", async () => {
      const midRaw = modioGameIdEdit.trim();
      const modioGameId = midRaw ? Number.parseInt(midRaw, 10) : 0;
      const updated = await api.updateManagedGame({
        id: libraryDetail.game.id,
        nexusDomain: ueDomainEdit.trim() || null,
        projectName: ueProjectEdit.trim(),
        thunderstoreCommunity: tsCommunityEdit.trim() || null,
        modioGameId: Number.isFinite(modioGameId) ? modioGameId : 0,
      });
      setLibraryDetail({ ...libraryDetail, game: updated });
      await refreshManaged();
      setNotice({ kind: "ok", message: "Game settings saved." });
    });
  }

  const browseOpts = useMemo((): BrowseSearchOpts => {
    const { tagsInclude, tagsExclude } = tagListsFromMap(browseTags);
    return {
      sort: browseSort,
      category: browseCategory || null,
      tagsInclude,
      tagsExclude,
      gameVersion: browseVersion || null,
      author: browseAuthor || null,
      offset: 0,
    };
  }, [browseSort, browseCategory, browseTags, browseVersion, browseAuthor]);

  browseOptsRef.current = browseOpts;

  useEffect(() => {
    if (tab !== "browse" || !browseSearched) return;
    void runSearch(browseMode, searchQuery, { ...browseOpts, offset: 0 });
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [sourceFilter]);

  useEffect(() => {
    if (tab !== "browse") return;
    const message = browsePrefetchErrorRef.current;
    if (!message) return;
    browsePrefetchErrorRef.current = null;
    setError(message);
  }, [tab]);

  useEffect(() => {
    if (!activeGame || browseSearched) return;
    const filter = clampCatalogSourceFilter(sourceFilter, activeGame, browseMode);
    if (filter !== sourceFilter) {
      setSourceFilter(filter);
      return;
    }
    void runSearch(
      browseMode,
      "",
      {
        sort: browseSort,
        category: null,
        tagsInclude: [],
        tagsExclude: [],
        gameVersion: null,
        offset: 0,
      },
      false,
      true,
    );
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [activeGame?.id, browseSearched, sourceFilter]);

  const activeTagCount = useMemo(
    () => Object.keys(browseTags).length,
    [browseTags],
  );

  const orderedTags = useMemo(() => {
    const all = (
      browseMode === "mods"
        ? (browseMeta?.mod_tags ?? [])
        : (browseMeta?.collection_tags ?? [])
    ).filter((t) => t.trim().toLowerCase() !== "nsfw");
    const q = deferredTagQuery.trim().toLowerCase();
    const filtered = q ? all.filter((t) => t.toLowerCase().includes(q)) : all;
    return [...filtered].sort((a, b) => {
      const rank = (t: string) =>
        browseTags[t] === "include" ? 0 : browseTags[t] === "exclude" ? 1 : 2;
      const ar = rank(a);
      const br = rank(b);
      if (ar !== br) return ar - br;
      return a.localeCompare(b, undefined, { sensitivity: "base" });
    });
  }, [browseMeta?.mod_tags, browseMeta?.collection_tags, browseMode, browseTags, deferredTagQuery]);

  useEffect(() => {
    if (tagsOpen) setTagsPageLimit(40);
  }, [tagsOpen, deferredTagQuery]);

  async function runSearch(
    mode: "mods" | "collections" = browseMode,
    query = searchQuery,
    opts: BrowseSearchOpts = browseOpts,
    append = false,
    silent = false,
  ) {
    const reportSearchError = (message: string) => {
      if (silent && tabRef.current !== "browse") {
        browsePrefetchErrorRef.current = message;
      } else {
        setError(message);
      }
    };
    if (!activeGame) {
      reportSearchError("Manage a game first.");
      return;
    }
    const hasNexus = Boolean(activeGame.nexus_domain);
    const hasTs = Boolean(activeGame.thunderstore_community);
    const hasModio = Boolean(activeGame.modio_game_id);
    if (mode === "collections" && !hasNexus && !hasTs) {
      reportSearchError(
        "Collections require a Nexus Mods domain or Thunderstore community for this game.",
      );
      return;
    }
    if (mode === "mods" && !hasNexus && !hasTs && !hasModio) {
      reportSearchError(
        "Set a Nexus domain, Thunderstore community, and/or mod.io game ID for this game.",
      );
      return;
    }
    const effectiveFilter = clampCatalogSourceFilter(
      sourceFilter,
      activeGame,
      mode,
    );
    if (effectiveFilter !== sourceFilter) {
      if (!silent) {
        setError(catalogFilterMismatchMessage(sourceFilter, activeGame));
      }
      setSourceFilter(effectiveFilter);
      return;
    }
    if (append) {
      if (
        browseLoadingMoreRef.current ||
        !browseHasMoreRef.current ||
        browseAppendFailedRef.current
      ) {
        return;
      }
      browseLoadingMoreRef.current = true;
    }

    const requestId = ++browseRequestIdRef.current;
    const offset = append ? browseNextOffsetRef.current : 0;
    const requestOpts: BrowseSearchOpts = {
      ...opts,
      offset,
      count: browsePageCountRef.current,
    };
    let appendOk = false;

    const applyModPage = (
      items: CatalogHit[],
      totalCount: number,
      hasMore: boolean,
      nextOffset: number,
      merge: "replace" | "append",
    ) => {
      browseNextOffsetRef.current = nextOffset;
      browseHasMoreRef.current = hasMore;
      setBrowseHasMore(hasMore);
      setBrowseTotalCount(totalCount);
      if (merge === "replace") {
        setModHits(items);
        setBrowseSearched(true);
      } else {
        setModHits((prev) => {
          const seen = new Set(prev.map((m) => m.id));
          const merged = [...prev];
          for (const item of items) {
            if (!seen.has(item.id)) {
              seen.add(item.id);
              merged.push(item);
            }
          }
          return merged.length === prev.length ? prev : merged;
        });
      }
    };

    const applyCollectionPage = (
      items: CollectionHit[],
      totalCount: number,
      merge: "replace" | "append",
    ) => {
      const nextOffset = offset + items.length;
      browseNextOffsetRef.current = nextOffset;
      const hasMore = items.length > 0 && nextOffset < totalCount;
      browseHasMoreRef.current = hasMore;
      setBrowseHasMore(hasMore);
      setBrowseTotalCount(totalCount);
      if (merge === "replace") {
        setCollectionHits(items);
        setBrowseSearched(true);
      } else {
        setCollectionHits((prev) => {
          const seen = new Set(prev.map((c) => collectionHitKey(c)));
          const merged = [...prev];
          for (const item of items) {
            const key = collectionHitKey(item);
            if (!seen.has(key)) {
              seen.add(key);
              merged.push(item);
            }
          }
          return merged.length === prev.length ? prev : merged;
        });
      }
    };

    let committed = false;
    try {
      if (!append) {
        browseNextOffsetRef.current = 0;
        browseLoadingMoreRef.current = false;
        browseAppendFailedRef.current = false;
        browseAppendStuckRef.current = false;
        browseSentinelTopRef.current = null;
        browseResultsRequestRef.current = requestId;
        setBrowseResultsPending(true);
        const perform = async () => {
          if (mode === "mods") {
            const page = await api.searchCatalog(activeGame.id, query, {
              ...requestOpts,
              sourceFilter,
            });
            if (requestId !== browseRequestIdRef.current) return;
            browsePrefetchErrorRef.current = null;
            committed = true;
            startTransition(() => {
              applyModPage(
                page.items,
                page.total_count,
                page.has_more,
                page.next_offset,
                "replace",
              );
              setBrowseResultsPending(false);
            });
          } else {
            const page = await api.searchCollectionCatalog(activeGame.id, query, {
              ...requestOpts,
              sourceFilter:
                sourceFilter === "modio" ? "all" : sourceFilter,
            });
            if (requestId !== browseRequestIdRef.current) return;
            browsePrefetchErrorRef.current = null;
            committed = true;
            startTransition(() => {
              applyCollectionPage(page.items, page.total_count, "replace");
              if (page.has_more != null) {
                browseHasMoreRef.current = page.has_more;
                setBrowseHasMore(page.has_more);
              }
              if (page.next_offset != null) {
                browseNextOffsetRef.current = page.next_offset;
              }
              setBrowseResultsPending(false);
            });
          }
        };
        if (silent) {
          setBrowseLoading(true);
          await perform();
        } else {
          setBrowseLoading(false);
          await withBusy(
            mode === "mods" ? "Searching mods…" : "Searching collections…",
            perform,
          );
        }
      } else if (mode === "mods") {
        const page = await api.searchCatalog(activeGame.id, query, {
          ...requestOpts,
          sourceFilter,
        });
        if (requestId !== browseRequestIdRef.current) return;
        startTransition(() => {
          applyModPage(
            page.items,
            page.total_count,
            page.has_more,
            page.next_offset,
            "append",
          );
        });
        appendOk = true;
        browseAppendFailedRef.current = false;
      } else {
        const page = await api.searchCollectionCatalog(activeGame.id, query, {
          ...requestOpts,
          sourceFilter: sourceFilter === "modio" ? "all" : sourceFilter,
        });
        if (requestId !== browseRequestIdRef.current) return;
        startTransition(() => {
          applyCollectionPage(page.items, page.total_count, "append");
          if (page.has_more != null) {
            browseHasMoreRef.current = page.has_more;
            setBrowseHasMore(page.has_more);
          }
          if (page.next_offset != null) {
            browseNextOffsetRef.current = page.next_offset;
          }
        });
        appendOk = true;
        browseAppendFailedRef.current = false;
      }
    } catch (e) {
      if (requestId === browseRequestIdRef.current) {
        reportSearchError(String(e));
        if (!append) {
          browseHasMoreRef.current = false;
          setBrowseHasMore(false);
        } else {
          browseAppendFailedRef.current = true;
        }
      }
    } finally {
      if (!append && !committed && browseResultsRequestRef.current === requestId) {
        setBrowseResultsPending(false);
      }
      if (silent && !append && requestId === browseRequestIdRef.current) {
        setBrowseLoading(false);
      }
      if (append) {
        const before = browseSentinelTopRef.current;
        if (requestId !== browseRequestIdRef.current) {
          /* superseded by a newer search or reset; do not clobber its loading state */
        } else if (!(appendOk && before != null)) {
          browseLoadingMoreRef.current = false;
        } else {
          const origin = before;
          requestAnimationFrame(() => {
            requestAnimationFrame(() => {
              if (requestId !== browseRequestIdRef.current) return;
              browseLoadingMoreRef.current = false;
              if (!browseResultsVisibleRef.current) return;
              const marker = browseSentinelRef.current;
              const root = browseResultsRef.current;
              if (!marker || !root) return;
              const after = sentinelContentTop(
                marker.getBoundingClientRect().top,
                root.getBoundingClientRect().top,
                root.scrollTop,
              );
              if (after <= origin + 1) {
                browseAppendStuckRef.current = true;
                return;
              }
              if (
                browseHasMoreRef.current &&
                !browseAppendFailedRef.current &&
                isBrowseSentinelNearView(0)
              ) {
                browseSentinelTopRef.current = after;
                void runSearch(
                  browseModeRef.current,
                  searchQueryRef.current,
                  browseOptsRef.current,
                  true,
                );
              }
            });
          });
        }
      }
    }
  }

  function isBrowseSentinelNearView(margin = 0): boolean {
    const sentinel = browseSentinelRef.current;
    const root = browseResultsRef.current;
    if (!sentinel || !root) return false;
    const rootRect = root.getBoundingClientRect();
    const sentRect = sentinel.getBoundingClientRect();
    return isSentinelInScrollView(
      rootRect,
      { width: root.clientWidth, height: root.clientHeight },
      sentRect,
      { width: window.innerWidth, height: window.innerHeight },
      margin,
    );
  }

  function clearAuthorSuggestions() {
    authorSuggestReqRef.current += 1;
    setAuthorSuggestions([]);
    setAuthorSuggestOpen(false);
    setAuthorSuggestIndex(0);
    setAuthorSuggestLoading(false);
  }

  function pickAuthorSuggestion(hit: AuthorSuggestion) {
    clearAuthorSuggestions();
    browseByAuthor(hit.name, hit.source);
  }

  async function search() {
    if (browseMode === "mods") {
      const prefix = parseAuthorQuery(searchQuery);
      if (prefix !== null) {
        if (authorSuggestOpen && authorSuggestions.length > 0) {
          const pick =
            authorSuggestions[authorSuggestIndex] ?? authorSuggestions[0];
          pickAuthorSuggestion(pick);
          return;
        }
        if (prefix.length > 0) {
          clearAuthorSuggestions();
          browseByAuthor(prefix);
          return;
        }
        return;
      }
    }
    await runSearch();
  }

  function cycleBrowseTag(tag: string) {
    const tags = setTagInMap(browseTags, tag);
    setBrowseTags(tags);
    const { tagsInclude, tagsExclude } = tagListsFromMap(tags);
    runSearch(browseMode, searchQuery, {
      ...browseOpts,
      tagsInclude,
      tagsExclude,
      offset: 0,
    });
  }

  const hideNsfw = settings?.adult_content === false;

  async function toggleBrowseHideNsfw() {
    const nextAdultContent = hideNsfw;
    setSettings((prev) =>
      prev ? { ...prev, adult_content: nextAdultContent } : prev,
    );
    try {
      await api.setAdultContent(nextAdultContent);
      await refreshSettings();
      await runSearch(browseMode, searchQuery, {
        ...browseOpts,
        offset: 0,
      });
    } catch (err) {
      await refreshSettings();
      setError(String(err));
    }
  }

  function clearBrowseFilters() {
    setBrowseCategory("");
    setBrowseVersion("");
    setBrowseAuthor("");
    setBrowseAuthorDraft("");
    setBrowseTags({});
    setTagQuery("");
    setTagsOpen(false);
    runSearch(browseMode, searchQuery, {
      ...browseOpts,
      category: null,
      gameVersion: null,
      author: null,
      tagsInclude: [],
      tagsExclude: [],
      offset: 0,
    });
  }

  function applyBrowseAuthor(raw: string) {
    const author = raw.trim();
    setBrowseAuthorDraft(author);
    if (author === browseAuthor) return;
    setBrowseAuthor(author);
    // Author filter skips mod.io; move off a mod.io-only source when applying.
    if (author && sourceFilter === "modio" && activeGame) {
      setSourceFilter(clampCatalogSourceFilter("all", activeGame, "mods"));
    }
    void runSearch(browseMode, searchQuery, {
      ...browseOpts,
      author: author || null,
      offset: 0,
    });
  }

  function browseByAuthor(
    name: string,
    source?: CatalogHit["source"] | null,
  ) {
    const author = name.trim();
    if (!author || !activeGame) return;
    let nextSource = sourceFilter;
    if (source === "nexus" || source === "thunderstore") {
      nextSource = source;
    } else if (source === "modio") {
      // Author filter skips mod.io; prefer merged Nexus/Thunderstore results.
      nextSource = "all";
    }
    nextSource = clampCatalogSourceFilter(nextSource, activeGame, "mods");
    setTab("browse");
    setBrowseMode("mods");
    closeBrowseDetail();
    setSearchQuery("");
    clearAuthorSuggestions();
    setBrowseAuthor(author);
    setBrowseAuthorDraft(author);
    setBrowseCategory("");
    setBrowseVersion("");
    setBrowseTags({});
    setTagQuery("");
    setTagsOpen(false);
    if (nextSource !== sourceFilter) {
      setSourceFilter(nextSource);
    }
    void runSearch("mods", "", {
      ...browseOpts,
      author,
      category: null,
      gameVersion: null,
      tagsInclude: [],
      tagsExclude: [],
      offset: 0,
    });
  }

  async function openMod(
    hit: CatalogHit,
    detailTab: DetailTab = "info",
    opts?: { skipHistoryPush?: boolean; navOverrides?: Partial<NavEntry> },
  ) {
    if (!activeGame) return;
    if (!opts?.skipHistoryPush) {
      pushNavTarget({
        ...captureNavEntry(),
        ...opts?.navOverrides,
        tab: opts?.navOverrides?.tab ?? navTab(),
        browseDetail: { kind: "mod", hit, tab: detailTab },
      });
    }
    const detailReq = ++browseDetailReqRef.current;
    rememberBrowseScroll();
    setBrowseDetail({ kind: "mod", hit, tab: detailTab });
    setModDetail(null);
    setModDetailLoading(true);
    setModFilesLoading(detailTab === "files");
    setTsDetail(null);
    setTsVersion("");
    setCollectionModFiles([]);
    setCollectionDetail(null);
    setCollectionDetailLoading(false);
    setCollectionFilesLoading(false);
    setModioDetail(null);
    setModioFiles([]);
    clearModFiles();
    const hitId = hit.id;
    const patchBrowseHit = (enrich: (current: CatalogHit) => CatalogHit) => {
      const current = browseDetailRef.current;
      if (!current || current.kind !== "mod" || current.hit.id !== hitId) return;
      const nextHit = enrich(current.hit);
      setBrowseDetail({ ...current, hit: nextHit });
    };
    if (hit.source === "thunderstore") {
      if (!hit.community || !hit.namespace || !hit.package_name) {
        setModDetailLoading(false);
        setModFilesLoading(false);
        return;
      }
      try {
        await withBusy("Loading package…", async () => {
          const detail = await api.getThunderstorePackage(
            hit.community!,
            hit.namespace!,
            hit.package_name!,
          );
          if (detailReq !== browseDetailReqRef.current) return;
          setTsDetail(detail);
          setTsVersion(detail.latest_version ?? detail.versions[0]?.version_number ?? "");
          patchBrowseHit((current) => enrichCatalogHitFromThunderstore(current, detail));
        });
      } finally {
        if (detailReq === browseDetailReqRef.current) {
          setModDetailLoading(false);
          setModFilesLoading(false);
        }
      }
      return;
    }
    if (hit.source === "modio") {
      if (hit.modio_game_id == null || hit.modio_mod_id == null) {
        setModDetailLoading(false);
        setModFilesLoading(false);
        return;
      }
      try {
        await withBusy("Loading mod.io mod…", async () => {
          const detail = await api.getModioMod(hit.modio_game_id!, hit.modio_mod_id!);
          if (detailReq !== browseDetailReqRef.current) return;
          setModioDetail(detail);
          patchBrowseHit((current) => enrichCatalogHitFromModio(current, detail));
          if (detailTab === "files") {
            const files = await api.modioFiles(hit.modio_game_id!, hit.modio_mod_id!);
            if (detailReq !== browseDetailReqRef.current) return;
            setModioFiles(files);
          }
        });
      } finally {
        if (detailReq === browseDetailReqRef.current) {
          setModDetailLoading(false);
          setModFilesLoading(false);
        }
      }
      return;
    }
    const domain = hit.domain_name || activeGame.nexus_domain;
    const modId = hit.mod_id;
    if (!modId) {
      setModDetailLoading(false);
      setModFilesLoading(false);
      return;
    }
    try {
      await withBusy(
        detailTab === "files" ? "Loading files…" : "Loading mod…",
        async () => {
          const detail = await api.getMod(domain, modId);
          if (detailReq !== browseDetailReqRef.current) return;
          setModDetail(detail);
          patchBrowseHit((current) => enrichCatalogHitFromNexus(current, detail));
          if (detailTab === "files" && !nexusModUnavailable(detail)) {
            await loadModFiles(domain, modId);
          }
        },
      );
    } finally {
      if (detailReq === browseDetailReqRef.current) {
        setModDetailLoading(false);
        setModFilesLoading(false);
      }
    }
  }

  async function openStagedMod(mod: StagedMod) {
    if (!activeGame) return;
    const hit = catalogHitFromStaged(mod, activeGame);
    if (!hit) return;
    setBrowseReturnTab("library");
    setTab("browse");
    await openMod(hit, "info", {
      navOverrides: { tab: "browse", browseReturnTab: "library" },
    });
  }

  async function openCollectionMod(
    hit: CatalogHit,
    returnTab: "collections" | "friends" | "library",
    targetGameId?: string | null,
  ) {
    const targetGame =
      (targetGameId ? managed.find((g) => g.id === targetGameId) : null) ??
      activeGame;
    if (!targetGame) {
      setNotice({
        kind: "warn",
        message: "Manage this game in Library to open in Browse.",
      });
      return;
    }
    if (activeGame?.id !== targetGame.id) {
      pendingEmperorModRef.current = { hit, gameId: targetGame.id };
      setBrowseReturnTab(returnTab);
      try {
        await api.setActiveGame(targetGame.id);
      } catch (e) {
        pendingEmperorModRef.current = null;
        setError(String(e));
        return;
      }
      setActiveId(targetGame.id);
      return;
    }
    setBrowseReturnTab(returnTab);
    setTab("browse");
    await openMod(hit, "info", {
      navOverrides: { tab: "browse", browseReturnTab: returnTab },
    });
  }

  async function openRecentDownload(item: DownloadItem) {
    const hit = catalogHitFromDownload(item);
    const gameId = item.page?.game_id;
    if (!hit || !gameId) return;
    const game = managed.find((g) => g.id === gameId);
    if (!game) {
      setNotice({
        kind: "warn",
        message: "That game is no longer managed.",
      });
      return;
    }
    if (activeGame?.id !== game.id) {
      pendingDownloadPageRef.current = { hit, gameId: game.id };
      try {
        await api.setActiveGame(game.id);
      } catch (e) {
        pendingDownloadPageRef.current = null;
        setError(String(e));
        return;
      }
      setActiveId(game.id);
      return;
    }
    setBrowseReturnTab("downloads");
    setTab("browse");
    await openMod(hit, "info", {
      navOverrides: { tab: "browse", browseReturnTab: "downloads" },
    });
  }

  async function openEmperorModLink(payload: EmperorOpenMod) {
    const key = JSON.stringify(payload);
    const now = Date.now();
    const last = lastEmperorOpenRef.current;
    if (last && last.key === key && now - last.at < 2000) return;
    lastEmperorOpenRef.current = { key, at: now };

    const game = managedGameForModDeepLink(managedRef.current, payload);
    if (!game) {
      setNotice({
        kind: "warn",
        message: "That game is not managed.",
      });
      return;
    }
    const hit = catalogHitFromModDeepLink(payload);
    if (activeGameRef.current?.id !== game.id) {
      pendingEmperorModRef.current = { hit, gameId: game.id };
      try {
        await api.setActiveGame(game.id);
      } catch (e) {
        pendingEmperorModRef.current = null;
        setError(String(e));
        return;
      }
      setActiveId(game.id);
      return;
    }
    setTab("browse");
    await openMod(hit, "info", {
      navOverrides: { tab: "browse" },
    });
  }
  openEmperorModLinkRef.current = (payload) => {
    void openEmperorModLink(payload);
  };

  async function copyModDeepLink(hit: CatalogHit) {
    const link = modShareUrlFromHit(hit);
    if (!link) {
      setNotice({
        kind: "warn",
        message: "This mod has no shareable Emperor link.",
      });
      return;
    }
    try {
      await navigator.clipboard.writeText(link);
      setNotice({ kind: "ok", message: "Emperor link copied." });
    } catch {
      setNotice({
        kind: "warn",
        message: "Could not copy the link to the clipboard.",
      });
    }
  }

  async function openCollection(
    hit: CollectionHit,
    detailTab: DetailTab = "info",
    opts?: { skipHistoryPush?: boolean },
  ) {
    if (!activeGame) return;
    if (!opts?.skipHistoryPush) {
      pushNavTarget({
        ...captureNavEntry(),
        browseDetail: { kind: "collection", hit, tab: detailTab },
      });
    }
    const detailReq = ++browseDetailReqRef.current;
    rememberBrowseScroll();
    setBrowseDetail({ kind: "collection", hit, tab: detailTab });
    setModDetail(null);
    setModDetailLoading(false);
    setModFilesLoading(false);
    clearModFiles();
    setCollectionModFiles([]);
    setCollectionDetail(null);
    setCollectionDetailLoading(true);
    setCollectionFilesLoading(true);
    setTsDetail(null);
    if (collectionHitSource(hit) === "thunderstore") {
      if (!hit.community || !hit.namespace || !hit.package_name) {
        setCollectionDetailLoading(false);
        setCollectionFilesLoading(false);
        return;
      }
      try {
        await withBusy("Loading modpack…", async () => {
          const detail = await api.getThunderstorePackage(
            hit.community!,
            hit.namespace!,
            hit.package_name!,
          );
          if (detailReq !== browseDetailReqRef.current) return;
          setTsDetail(detail);
          setTsVersion(detail.latest_version ?? detail.versions[0]?.version_number ?? "");
        });
      } finally {
        if (detailReq === browseDetailReqRef.current) {
          setCollectionDetailLoading(false);
          setCollectionFilesLoading(false);
        }
      }
      return;
    }
    const domain = hit.domain_name || activeGame.nexus_domain;
    try {
      await withBusy(
        detailTab === "files" ? "Loading collection mods…" : "Loading collection…",
        async () => {
          const detailPromise = api.getCollection({ slug: hit.slug, domain });
          const filesPromise = api.collectionFiles({
            slug: hit.slug,
            revision: hit.revision_number,
          });
          const [detail, files] = await Promise.all([detailPromise, filesPromise]);
          if (detailReq !== browseDetailReqRef.current) return;
          startTransition(() => {
            setCollectionDetail(detail);
            setCollectionModFiles(files);
          });
        },
      );
    } finally {
      if (detailReq === browseDetailReqRef.current) {
        setCollectionDetailLoading(false);
        setCollectionFilesLoading(false);
      }
    }
  }

  async function setDetailTab(detailTab: DetailTab) {
    if (!browseDetail || !activeGame) return;
    if (browseDetail.tab === detailTab) return;
    const detailReq = browseDetailReqRef.current;
    const next = { ...browseDetail, tab: detailTab };
    setBrowseDetail(next);
    if (next.kind === "mod") {
      if (next.hit.source === "thunderstore") {
        if (!tsDetail && next.hit.community && next.hit.namespace && next.hit.package_name) {
          setModDetailLoading(true);
          try {
            await withBusy("Loading package…", async () => {
              const detail = await api.getThunderstorePackage(
                next.hit.community!,
                next.hit.namespace!,
                next.hit.package_name!,
              );
              if (detailReq !== browseDetailReqRef.current) return;
              setTsDetail(detail);
              setTsVersion(detail.latest_version ?? detail.versions[0]?.version_number ?? "");
            });
          } finally {
            if (detailReq === browseDetailReqRef.current) setModDetailLoading(false);
          }
        }
      } else if (next.hit.source === "modio") {
        if (
          next.hit.modio_game_id != null &&
          next.hit.modio_mod_id != null &&
          (!modioDetail || (detailTab === "files" && modioFiles.length === 0))
        ) {
          const needDetail = !modioDetail;
          const needFiles = detailTab === "files" && modioFiles.length === 0;
          if (needDetail) setModDetailLoading(true);
          if (needFiles) setModFilesLoading(true);
          try {
            await withBusy(
              detailTab === "files" ? "Loading files…" : "Loading mod.io mod…",
              async () => {
                if (needDetail) {
                  const detail = await api.getModioMod(
                    next.hit.modio_game_id!,
                    next.hit.modio_mod_id!,
                  );
                  if (detailReq !== browseDetailReqRef.current) return;
                  setModioDetail(detail);
                }
                if (needFiles) {
                  const files = await api.modioFiles(
                    next.hit.modio_game_id!,
                    next.hit.modio_mod_id!,
                  );
                  if (detailReq !== browseDetailReqRef.current) return;
                  setModioFiles(files);
                }
              },
            );
          } finally {
            if (detailReq === browseDetailReqRef.current) {
              if (needDetail) setModDetailLoading(false);
              if (needFiles) setModFilesLoading(false);
            }
          }
        }
      } else if (
        detailTab === "files" &&
        next.hit.mod_id &&
        modFilesModIdRef.current !== next.hit.mod_id
      ) {
        if (!nexusModUnavailable(modDetail)) {
          const domain = next.hit.domain_name || activeGame.nexus_domain;
          const modId = next.hit.mod_id;
          const needDetail = !modDetail;
          if (needDetail) setModDetailLoading(true);
          setModFilesLoading(true);
          try {
            await withBusy(modDetail ? "Loading files…" : "Loading mod…", async () => {
              let detail = modDetail;
              if (!detail) {
                detail = await api.getMod(domain, modId);
                if (detailReq !== browseDetailReqRef.current) return;
                setModDetail(detail);
              }
              if (!nexusModUnavailable(detail)) {
                await loadModFiles(domain, modId);
              }
            });
          } finally {
            if (detailReq === browseDetailReqRef.current) {
              if (needDetail) setModDetailLoading(false);
              setModFilesLoading(false);
            }
          }
        }
      } else if (detailTab === "info" && !modDetail && next.hit.mod_id) {
        setModDetailLoading(true);
        try {
          await withBusy("Loading mod…", async () => {
            const detail = await api.getMod(
              next.hit.domain_name || activeGame.nexus_domain,
              next.hit.mod_id!,
            );
            if (detailReq !== browseDetailReqRef.current) return;
            setModDetail(detail);
          });
        } finally {
          if (detailReq === browseDetailReqRef.current) setModDetailLoading(false);
        }
      }
    } else if (
      collectionHitSource(next.hit) === "thunderstore"
    ) {
      if (
        !tsDetail &&
        next.hit.community &&
        next.hit.namespace &&
        next.hit.package_name
      ) {
        setCollectionDetailLoading(true);
        setCollectionFilesLoading(true);
        try {
          await withBusy("Loading modpack…", async () => {
            const detail = await api.getThunderstorePackage(
              next.hit.community!,
              next.hit.namespace!,
              next.hit.package_name!,
            );
            if (detailReq !== browseDetailReqRef.current) return;
            setTsDetail(detail);
            setTsVersion(
              detail.latest_version ?? detail.versions[0]?.version_number ?? "",
            );
          });
        } finally {
          if (detailReq === browseDetailReqRef.current) {
            setCollectionDetailLoading(false);
            setCollectionFilesLoading(false);
          }
        }
      }
    } else if (detailTab === "files" && collectionModFiles.length === 0) {
      setCollectionFilesLoading(true);
      try {
        await withBusy("Loading collection mods…", async () => {
          const files = await api.collectionFiles({
            slug: next.hit.slug,
            revision: next.hit.revision_number,
          });
          if (detailReq !== browseDetailReqRef.current) return;
          startTransition(() => setCollectionModFiles(files));
        });
      } finally {
        if (detailReq === browseDetailReqRef.current) setCollectionFilesLoading(false);
      }
    } else if (detailTab === "info" && !collectionDetail) {
      setCollectionDetailLoading(true);
      try {
        await withBusy("Loading collection…", async () => {
          const detail = await api.getCollection({
            slug: next.hit.slug,
            domain: next.hit.domain_name || activeGame.nexus_domain,
          });
          if (detailReq !== browseDetailReqRef.current) return;
          setCollectionDetail(detail);
        });
      } finally {
        if (detailReq === browseDetailReqRef.current) setCollectionDetailLoading(false);
      }
    }
  }

  async function openGame(
    game: ManagedGame,
    opts?: { skipHistoryPush?: boolean },
  ) {
    if (!opts?.skipHistoryPush) {
      pushNavTarget({
        tab: "library",
        browseReturnTab: null,
        browseDetail: null,
        libraryDetail: { gameId: game.id },
        gameSavedDetailId: null,
        savedDetailId: null,
      });
    }
    resetBrowseState();
    resetLibraryEphemeralState();
    setGameModal(null);
    setGameSettingsMenuOpen(false);
    setSourceFilter((prev) =>
      clampCatalogSourceFilter(prev, game, browseMode),
    );
    setLibraryDetail({ game });
    gameSavedOpenRef.current += 1;
    setGameSavedDetail(null);
    setGameSavedDetailPending(false);
    setUeDomainEdit(game.nexus_domain);
    setUeProjectEdit(game.project_name ?? "");
    setTsCommunityEdit(game.thunderstore_community ?? "");
    setModioGameIdEdit(
      game.modio_game_id != null && game.modio_game_id > 0
        ? String(game.modio_game_id)
        : "",
    );
    setTab("library");
    setActiveId(game.id);
    void loadGameInfo(game.nexus_domain);
    try {
      await api.setActiveGame(game.id);
    } catch (e) {
      setError(String(e));
    }
  }

  async function installThunderstorePackage() {
    if (!activeGame || browseDetail?.kind !== "mod") return;
    const hit = browseDetail.hit;
    if (hit.source !== "thunderstore" || !hit.community || !hit.namespace || !hit.package_name) {
      return;
    }
    goToDownloadsOnInstall();
    setBusy(`Installing ${hit.name}…`);
    setError(null);
    try {
      await api.downloadThunderstoreMod({
        gameId: activeGame.id,
        community: hit.community,
        namespace: hit.namespace,
        name: hit.package_name,
        version: tsVersion || null,
      });
      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      await refreshDownloads();
      setNotice({ kind: "ok", message: `Installed ${hit.name} (with dependencies).` });
    } catch (e) {
      setError(String(e));
      await refreshDownloads();
    } finally {
      setBusy(null);
    }
  }

  function closeLibraryDetail() {
    setLibraryDetail(null);
    setGameModal(null);
    setGameSettingsMenuOpen(false);
    setGameInfo(null);
    gameSavedOpenRef.current += 1;
    setGameSavedDetail(null);
    setGameSavedDetailPending(false);
  }

  async function installModioFile(fileId?: number | null, version?: string | null) {
    if (!activeGame || browseDetail?.kind !== "mod") return;
    const hit = browseDetail.hit;
    if (hit.source !== "modio" || hit.modio_game_id == null || hit.modio_mod_id == null) {
      return;
    }
    goToDownloadsOnInstall();
    setBusy(`Installing ${hit.name}…`);
    setError(null);
    try {
      await api.downloadModioMod({
        gameId: activeGame.id,
        modioGameId: hit.modio_game_id,
        modId: hit.modio_mod_id,
        fileId: fileId ?? null,
        name: hit.name,
        version: version ?? null,
        installDeps: true,
      });
      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      await refreshDownloads();
      setNotice({ kind: "ok", message: `Installed ${hit.name}.` });
    } catch (e) {
      setError(String(e));
      await refreshDownloads();
    } finally {
      setBusy(null);
    }
  }


  async function queueNexusFile(hit: CatalogHit, file: ModFileInfo) {
    if (!activeGame || hit.source !== "nexus" || hit.mod_id == null) return;
    const domain = hit.domain_name || activeGame.nexus_domain;
    const name = hit.name;
    if (isPremium) {
      goToDownloadsOnInstall();
      setBusy(`Downloading ${file.name}…`);
      setError(null);
      try {
        await api.downloadMod({
          gameId: activeGame.id,
          domain,
          modId: hit.mod_id,
          fileId: file.file_id,
          name,
          version: file.version,
          fileName: chosenFileLabel(name, file),
        });
        await refreshMods(activeGame.id);
        await refreshDownloads();
      } catch (e) {
        const msg = String(e);
        if (!msg.includes("Paused")) {
          setError(msg);
        }
        await refreshDownloads();
      } finally {
        setBusy(null);
      }
    } else {
      try {
        await enqueueAssistEntries(
          [
            createQueueEntry(
              activeGame.id,
              domain,
              hit.mod_id,
              file.file_id,
              name,
              file.version,
              null,
              chosenFileLabel(name, file),
            ),
          ],
          null,
        );
      } catch (e) {
        setError(String(e));
        setBusy(null);
      }
    }
  }

  async function confirmQuickInstall(main: ModFileInfo, extras: ModFileInfo[]) {
    const prompt = quickInstall;
    if (!prompt) return;
    setQuickInstall(null);
    await queueNexusFile(prompt.hit, main);
    for (const extra of extras) {
      await queueNexusFile(prompt.hit, extra);
    }
  }

  async function installFile(file: ModFileInfo) {
    if (browseDetail?.kind !== "mod") return;
    await queueNexusFile(browseDetail.hit, file);
  }

  async function uninstallCatalogMod(hit: CatalogHit) {
    if (!activeGame) return;
    const game = activeGame;
    const matches = stagedModsForCatalogId(mods, game, hit.id);
    if (matches.length === 0) return;
    await withBusy(`Uninstalling ${hit.name}…`, async () => {
      for (const staged of matches) {
        await api.removeMod(game.id, staged.id);
      }
      await refreshMods(game.id);
      await refreshCollections(game.id);
      setModUpdates((prev) => {
        const next = { ...prev };
        for (const staged of matches) delete next[staged.id];
        return next;
      });
    });
  }

  async function quickInstallMod(hit: CatalogHit) {
    if (!activeGame || hit.source !== "nexus" || hit.mod_id == null) return;
    const domain = hit.domain_name || activeGame.nexus_domain;
    setError(null);
    setBusy(`Finding main file for ${hit.name}…`);
    try {
      const files = await api.modFiles(domain, hit.mod_id);
      const current = files.filter((file) => !isHistoricalModFile(file));
      const mains = current.filter(isMainCategoryFile);
      if (mains.length >= 2) {
        setQuickInstall({
          hit,
          mains,
          optionals: current.filter(isOptionalCategoryFile),
        });
        setBusy(null);
        return;
      }
      const file = pickMainModFile(files);
      if (!file) {
        setError(`No main file found for ${hit.name}. Open Files to choose one.`);
        setBusy(null);
        return;
      }
      setBusy(null);
      await queueNexusFile(hit, file);
    } catch (e) {
      setError(String(e));
      setBusy(null);
    }
  }

  async function installCollection(c: CollectionHit) {
    if (!activeGame) return;
    if (collectionHitSource(c) === "thunderstore") {
      if (!c.community || !c.namespace || !c.package_name) return;
      goToDownloadsOnInstall();
      setBusy(`Installing modpack ${c.name}…`);
      setError(null);
      try {
        await api.downloadThunderstoreMod({
          gameId: activeGame.id,
          community: c.community,
          namespace: c.namespace,
          name: c.package_name,
          version: c.latest_version || tsVersion || null,
          recordAsModpack: true,
        });
        await refreshMods(activeGame.id);
        await refreshCollections(activeGame.id);
        await refreshDownloads();
        setNotice({ kind: "ok", message: `Installed ${c.name}.` });
      } catch (e) {
        setError(String(e));
        await refreshDownloads();
      } finally {
        setBusy(null);
      }
      return;
    }
    if (isPremium) {
      goToDownloadsOnInstall();
      setBusy(`Installing collection ${c.name}…`);
      setError(null);
      try {
        await api.installCollection({
          gameId: activeGame.id,
          slug: c.slug,
          revision: c.revision_number,
          includeOptional,
          name: c.name,
        });
        await refreshMods(activeGame.id);
        await refreshCollections(activeGame.id);
        await refreshDownloads();
      } catch (e) {
        setError(String(e));
      } finally {
        setBusy(null);
      }
      return;
    }

    setBusy(`Loading collection ${c.name}…`);
    setError(null);
    try {
      const existingModIds = mods.map((m) => m.id);
      const allFiles = await api.collectionFiles({
        slug: c.slug,
        revision: c.revision_number,
      });
      const filesToInstall = allFiles.filter((f) => includeOptional || !f.optional);
      if (filesToInstall.length === 0) {
        throw new Error("No mods to install in this collection (try including optional mods).");
      }
      const entries = filesToInstall.map((f) =>
        createQueueEntry(
          activeGame.id,
          f.domain_name,
          f.mod_id,
          f.file_id,
          f.mod_name,
          f.version,
          null,
          chosenFileLabel(f.mod_name, { name: f.file_name }),
        ),
      );
      await enqueueAssistEntries(entries, c.name, {
        slug: c.slug,
        name: c.name,
        revision: c.revision_number,
        existingModIds,
        files: filesToInstall.map((f) => ({ modId: f.mod_id, fileId: f.file_id })),
      });
    } catch (e) {
      setAssistQueueState(null);
      setBusy(null);
      setError(String(e));
    }
  }

  async function importThunderstoreProfileCode() {
    if (!activeGame) return;
    const code = profileCode.trim() || shareImportCode.trim();
    if (!code) {
      setError("Paste an Emperor share code or r2modman / Gale profile code.");
      return;
    }
    await beginCodeImport(activeGame.id, code);
  }

  async function handleShareImportResult(
    gameId: string,
    code: string,
    result: ShareImportResult,
    friendSync?: { peerId: string; friendCollectionId: string } | null,
  ) {
    if (result.needs_assist.length > 0) {
      const existingModIds = mods.map((m) => m.id);
      const entries = result.needs_assist.map((f) =>
        createQueueEntry(
          gameId,
          f.domain,
          f.mod_id,
          f.file_id,
          f.name,
          f.version,
        ),
      );
      await enqueueAssistEntries(entries, result.name, null, {
        collectionId: result.collection_id,
        name: result.name,
        code,
        existingModIds,
        memberIds: result.member_ids,
        files: result.needs_assist.map((f) => ({
          domain: f.domain,
          modId: f.mod_id,
          fileId: f.file_id,
        })),
        friendSync: friendSync ?? null,
      });
      if (result.warnings.length > 0) {
        setNotice({
          kind: "warn",
          message:
            result.warnings.length === 1
              ? result.warnings[0]
              : `${result.warnings.length} warnings while importing ${result.name}.`,
          details: result.warnings,
        });
      }
      return;
    }
    await refreshMods(gameId);
    await refreshCollections(gameId);
    await refreshDownloads();
    setNotice({
      kind: result.warnings.length > 0 ? "warn" : "ok",
      message: `${friendSync ? "Synced" : "Installed"} ${result.name} (${result.member_ids.length} mods).`,
      details: result.warnings.length > 0 ? result.warnings : undefined,
    });
  }

  async function syncInstalledCollection(c: InstalledCollection) {
    if (!activeGame) return;
    const gameId = activeGame.id;
    setError(null);
    setSyncingCollectionIds((prev) => ({ ...prev, [c.id]: true }));
    try {
      if (c.friend_sync) {
        const res = await api.joinOrSyncFriendCollection({
          gameId,
          peerId: c.friend_sync.friend_peer_id,
          friendCollectionId: c.friend_sync.friend_collection_id,
        });
        await handleShareImportResult(
          gameId,
          res.share_code,
          {
            collection_id: res.collection_id,
            name: res.name,
            member_ids: res.member_ids,
            needs_assist: res.needs_assist,
            warnings: res.warnings,
            collection: res.collection ?? null,
          },
          {
            peerId: c.friend_sync.friend_peer_id,
            friendCollectionId: c.friend_sync.friend_collection_id,
          },
        );
        return;
      }
      if (c.source === "nexus" && c.slug) {
        const detail = await api.getCollection({ slug: c.slug });
        await installCollection({
          source: "nexus",
          id: detail.slug,
          slug: detail.slug,
          name: detail.name || c.name,
          summary: detail.summary,
          author: null,
          category: null,
          endorsements: detail.endorsements,
          total_downloads: detail.total_downloads,
          overall_rating: null,
          overall_rating_count: null,
          tile_image_url: detail.tile_image_url ?? null,
          revision_number: detail.revision_number ?? c.revision ?? null,
          latest_version: null,
          domain_name: detail.domain_name,
          created_at: null,
          updated_at: null,
          mod_count: null,
          file_size: null,
        });
        return;
      }
      if (
        c.source === "thunderstore" &&
        c.kind === "modpack" &&
        c.community &&
        c.namespace &&
        c.package_name
      ) {
        await withBusy(`Updating modpack ${c.name}…`, async () => {
          await api.downloadThunderstoreMod({
            gameId,
            community: c.community!,
            namespace: c.namespace!,
            name: c.package_name!,
            version: null,
            recordAsModpack: true,
          });
          await refreshMods(gameId);
          await refreshCollections(gameId);
          await refreshDownloads();
          setNotice({ kind: "ok", message: `Updated ${c.name}.` });
        });
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setSyncingCollectionIds((prev) => {
        const next = { ...prev };
        delete next[c.id];
        return next;
      });
    }
  }

  const runAutoSyncPass = useCallback(async () => {
    const currentActive = activeGameRef.current;
    if (currentActive) {
      await refreshFriendSyncStates(currentActive.id);
    }
    for (const game of managedRef.current) {
      let states: FriendCollectionSyncStatus[] = [];
      try {
        states = await api.getFriendCollectionSyncStates(game.id);
      } catch {
        continue;
      }
      for (const st of states) {
        if (!st.auto_sync || !st.has_update) continue;
        const key = `${game.id}:${st.collection_id}`;
        if (autoSyncInFlightRef.current.has(key)) continue;
        autoSyncInFlightRef.current.add(key);
        try {
          const res = await api.joinOrSyncFriendCollection({
            gameId: game.id,
            peerId: st.peer_id,
            friendCollectionId: st.friend_collection_id,
          });
          if (res.needs_assist.length === 0) {
            if (activeGameRef.current?.id === game.id) {
              await refreshMods(game.id);
              await refreshCollections(game.id);
              await refreshFriendSyncStates(game.id);
              await refreshDownloads();
            }
            setNotice({
              kind: "ok",
              message: `Auto-synced ${res.name} to latest settings.`,
            });
          } else if (
            autoSyncNotifiedHashRef.current.get(key) !== st.latest_hash
          ) {
            autoSyncNotifiedHashRef.current.set(key, st.latest_hash);
            setNotice({
              kind: "warn",
              message: `${res.name} has an update that requires manual Nexus download assist. Click Sync on the game page to download.`,
            });
          }
        } catch (e) {
          if (autoSyncNotifiedHashRef.current.get(key) !== st.latest_hash) {
            autoSyncNotifiedHashRef.current.set(key, st.latest_hash);
            setNotice({
              kind: "warn",
              message: `Auto-sync failed for collection: ${String(e)}`,
            });
          }
        } finally {
          autoSyncInFlightRef.current.delete(key);
        }
      }
    }
  }, [
    refreshCollections,
    refreshDownloads,
    refreshFriendSyncStates,
    refreshMods,
  ]);

  async function toggleInstalledCollectionAutoSync(
    c: InstalledCollection,
    autoSync: boolean,
  ) {
    if (!activeGame || !c.friend_sync) return;
    setError(null);
    try {
      await api.setFriendCollectionAutoSync(activeGame.id, c.id, autoSync);
      await refreshCollections(activeGame.id);
      if (autoSync) {
        void runAutoSyncPass();
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function saveCollectionChanges(c: InstalledCollection) {
    if (!activeGame) return;
    setSavingCollectionIds((prev) => ({ ...prev, [c.id]: true }));
    setError(null);
    try {
      const updated = await api.saveCollectionChanges(activeGame.id, c.id);
      setInstalledCollections((prev) =>
        prev.map((item) => (item.id === updated.id ? updated : item)),
      );
      await refreshCollections(activeGame.id);
      await refreshSavedCollections(true);
      setNotice({ kind: "ok", message: `Saved changes to ${c.name}.` });
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingCollectionIds((prev) => {
        const next = { ...prev };
        delete next[c.id];
        return next;
      });
    }
  }

  async function toggleInstalledCollectionAutoSave(
    c: InstalledCollection,
    autoSave: boolean,
  ) {
    if (!activeGame || savingCollectionIds[c.id]) return;
    setSavingCollectionIds((prev) => ({ ...prev, [c.id]: true }));
    setError(null);
    try {
      const updated = await api.setCollectionAutoSave(
        activeGame.id,
        c.id,
        autoSave,
      );
      setInstalledCollections((prev) =>
        prev.map((item) => (item.id === updated.id ? updated : item)),
      );
      await refreshCollections(activeGame.id);
      await refreshSavedCollections(true);
    } catch (e) {
      setError(String(e));
    } finally {
      setSavingCollectionIds((prev) => {
        const next = { ...prev };
        delete next[c.id];
        return next;
      });
    }
  }

  useEffect(() => {
    const unlisten = listen("friends-changed", () => {
      void runAutoSyncPass();
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, [runAutoSyncPass]);

  useEffect(() => {
    const unlisten = listen<boolean>("friends-checking", (event) => {
      const checking = Boolean(event.payload);
      if (checkingFriendUpdatesTimerRef.current != null) {
        window.clearTimeout(checkingFriendUpdatesTimerRef.current);
        checkingFriendUpdatesTimerRef.current = null;
      }
      if (checking) {
        checkingFriendUpdatesTimerRef.current = window.setTimeout(() => {
          checkingFriendUpdatesTimerRef.current = null;
          setCheckingFriendUpdates(true);
        }, MOD_UPDATE_CHECK_TOAST_DELAY_MS);
      } else {
        setCheckingFriendUpdates(false);
      }
    });
    return () => {
      if (checkingFriendUpdatesTimerRef.current != null) {
        window.clearTimeout(checkingFriendUpdatesTimerRef.current);
        checkingFriendUpdatesTimerRef.current = null;
      }
      void unlisten.then((f) => f());
    };
  }, []);

  useEffect(() => {
    if (managed.length > 0) {
      void runAutoSyncPass();
    }
  }, [managed.length, runAutoSyncPass]);

  async function runUnifiedImport(
    gameId: string,
    code: string,
    name?: string | null,
  ) {
    goToDownloadsOnInstall();
    setBusy("Importing code…");
    setError(null);
    try {
      const imported = await api.importCode(gameId, code, name);
      if (imported.kind === "thunderstore_profile") {
        await refreshMods(gameId);
        await refreshCollections(gameId);
        await refreshDownloads();
        const warnings = imported.warnings ?? [];
        setNotice({
          kind: warnings.length > 0 ? "warn" : "ok",
          message:
            warnings.length > 0
              ? `Imported profile ${imported.collection.name} with ${warnings.length} staging warning${warnings.length === 1 ? "" : "s"}.`
              : `Imported profile ${imported.collection.name}.`,
          details: warnings.length > 0 ? warnings : undefined,
        });
        setProfileCode("");
        setShareImportCode("");
        setProfileImportOpen(false);
        setShareImportOpen(false);
      } else {
        await handleShareImportResult(gameId, code, imported.result);
        setShareImportCode("");
        setProfileCode("");
        setShareImportOpen(false);
        setProfileImportOpen(false);
      }
    } catch (e) {
      setError(String(e));
      await refreshDownloads();
    } finally {
      setBusy(null);
    }
  }

  function codeAlreadySaved(code: string): boolean {
    const trimmed = code.trim();
    return savedCollections.some((entry) => entry.code.trim() === trimmed);
  }

  async function beginCodeImport(
    gameId: string,
    code: string,
    name?: string | null,
  ) {
    const trimmed = code.trim();
    if (!trimmed) {
      setError("Paste an Emperor share code or r2modman / Gale profile code.");
      return;
    }
    if (codeAlreadySaved(trimmed)) {
      await runUnifiedImport(gameId, trimmed, name);
      return;
    }
    let detected;
    try {
      detected = await api.detectImportCode(trimmed);
    } catch (e) {
      setError(String(e));
      return;
    }
    if (detected.kind !== "emperor") {
      await runUnifiedImport(gameId, trimmed, name);
      return;
    }
    setSaveImportPrompt({
      gameId,
      code: trimmed,
      name: name ?? null,
      previewName: detected.preview?.name ?? null,
      modCount: detected.preview?.mods.length,
    });
  }

  async function confirmSaveImport(save: boolean) {
    if (!saveImportPrompt) return;
    const prompt = saveImportPrompt;
    setSaveImportPrompt(null);
    if (save) {
      try {
        await api.saveCollectionCode({
          code: prompt.code,
          name: prompt.name ?? prompt.previewName ?? null,
          sourceGameId: prompt.gameId,
        });
        await refreshSavedCollections();
      } catch (e) {
        setError(String(e));
        return;
      }
    }
    await runUnifiedImport(prompt.gameId, prompt.code, prompt.name);
  }

  async function openShareCodeFile() {
    if (!activeGame) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      multiple: false,
      filters: [
        { name: "Emperor share", extensions: ["emperor-share", "txt"] },
        { name: "Text", extensions: ["txt"] },
      ],
    });
    if (!selected || Array.isArray(selected)) return;
    setBusy("Reading share file…");
    setError(null);
    try {
      const code = (await api.readShareFile(selected)).trim();
      if (!code) {
        setError("Share file is empty.");
        return;
      }
      setShareImportCode(code);
      await beginCodeImport(activeGame.id, code);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  function openSaveLoadoutPrompt() {
    if (!activeGame) return;
    setError(null);
    setSaveLoadoutName("");
    setSaveLoadoutPrompt({
      gameId: activeGame.id,
      gameTitle: activeGame.title,
      modCount: enabledModCount,
    });
  }

  async function saveLoadout(name?: string | null) {
    if (!activeGame) return;
    const trimmed = name?.trim() || null;
    setSaveLoadoutPrompt(null);
    setBusy("Saving loadout…");
    setError(null);
    try {
      const result = await api.exportShareCode(activeGame.id, trimmed);
      setNotice({
        kind: result.warnings.length > 0 ? "warn" : "ok",
        message: `Saved loadout "${result.saved.name}" (${result.mod_count} mod(s)) to Collections.`,
        details: result.warnings.length > 0 ? result.warnings : undefined,
      });
      await refreshSavedCollections();
      await refreshCollections(activeGame.id);
      await refreshMods(activeGame.id);
      setGameModal("collections");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function renameSavedCollectionById(id: string, nextName: string) {
    try {
      const updated = await api.renameSavedCollection(id, nextName);
      setSavedDetail((prev) =>
        prev && prev.entry.id === updated.id ? { ...prev, entry: updated } : prev,
      );
      setGameSavedDetail((prev) =>
        prev && prev.entry.id === updated.id ? { ...prev, entry: updated } : prev,
      );
      await refreshSavedCollections();
      if (activeGame) {
        await refreshCollections(activeGame.id);
      }
    } catch (e) {
      setError(String(e));
    }
  }

  async function renameInstalledCollectionItem(
    col: InstalledCollection,
    nextName: string,
  ) {
    if (!activeGame) return;
    try {
      const updated = await api.renameInstalledCollection(
        activeGame.id,
        col.id,
        nextName,
      );
      setInstalledCollections((prev) =>
        prev.map((item) => (item.id === updated.id ? updated : item)),
      );
      setSavedDetail((prev) =>
        prev && prev.entry.share_id === updated.id
          ? {
              ...prev,
              entry: {
                ...prev.entry,
                name: updated.name,
                code: updated.profile_code ?? prev.entry.code,
              },
            }
          : prev,
      );
      setGameSavedDetail((prev) =>
        prev && prev.entry.share_id === updated.id
          ? {
              ...prev,
              entry: {
                ...prev.entry,
                name: updated.name,
                code: updated.profile_code ?? prev.entry.code,
              },
            }
          : prev,
      );
      await refreshSavedCollections();
      await refreshCollections(activeGame.id);
    } catch (e) {
      setError(String(e));
    }
  }

  const refreshSavedCollections = useCallback(async (force = true) => {
    if (!force && navTtlRef.current.isFresh("saved-collections", 30_000)) {
      setSavedCollectionsReady(true);
      return;
    }
    try {
      const list = await api.listSavedCollections();
      navTtlRef.current.mark("saved-collections");
      startTransition(() => {
        setSavedCollections((prev) => (sameSnapshot(prev, list) ? prev : list));
      });
    } catch (e) {
      setError(String(e));
    } finally {
      setSavedCollectionsReady(true);
    }
  }, [setError]);

  function closeCollectionImport() {
    setCollectionImportOpen(false);
    setCollectionImportCode("");
  }

  async function saveImportedCollectionCode(raw: string) {
    const code = raw.trim();
    if (!code) {
      setError("Paste an Emperor share code.");
      return;
    }
    setError(null);
    try {
      await api.saveCollectionCode({ code });
      setNotice({
        kind: "ok",
        message: "Saved to Collections.",
      });
      closeCollectionImport();
      await refreshSavedCollections();
    } catch (e) {
      setCollectionImportCode(code);
      setError(String(e));
    }
  }

  async function openCollectionImportFile() {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      multiple: false,
      filters: [
        { name: "Emperor share", extensions: ["emperor-share", "txt"] },
        { name: "Text", extensions: ["txt"] },
      ],
    });
    if (!selected || Array.isArray(selected)) return;
    setBusy("Reading share file…");
    setError(null);
    try {
      const code = (await api.readShareFile(selected)).trim();
      setCollectionImportCode(code);
      if (!code) {
        setError("Share file is empty.");
        return;
      }
      await saveImportedCollectionCode(code);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function openSavedCollection(
    id: string,
    opts?: { skipHistoryPush?: boolean },
  ) {
    if (!opts?.skipHistoryPush) {
      pushNavTarget({
        ...captureNavEntry(),
        tab: "collections",
        savedDetailId: id,
      });
    }
    setError(null);
    const request = ++savedOpenRef.current;
    const known = savedCollections.find((entry) => entry.id === id);
    const installId = (entry: SavedCollectionEntry) => {
      const matches = managed.filter((game) => savedCollectionMatchesGame(entry, game));
      if (matches.length > 0) return matches[0].id;
      if (savedCollectionGameKey(entry) !== "unknown") return "";
      return entry.source_game_id ?? activeGame?.id ?? managed[0]?.id ?? "";
    };
    if (known) {
      setSavedDetail({ entry: known, mods: [] });
      setSavedInstallGameId(installId(known));
      setSavedDetailPending(true);
    }
    try {
      const detail = await api.getSavedCollection(id);
      if (request !== savedOpenRef.current) return;
      startTransition(() => {
        if (request !== savedOpenRef.current) return;
        setSavedDetail(detail);
        setSavedInstallGameId(installId(detail.entry));
        setSavedDetailPending(false);
      });
    } catch (e) {
      if (request !== savedOpenRef.current) return;
      setSavedDetailPending(false);
      setError(String(e));
    }
  }

  async function openGameSavedCollection(
    id: string,
    opts?: { skipHistoryPush?: boolean },
  ) {
    if (!opts?.skipHistoryPush) {
      pushNavTarget({
        ...captureNavEntry(),
        gameSavedDetailId: id,
      });
    }
    setError(null);
    const request = ++gameSavedOpenRef.current;
    const known = savedCollections.find((entry) => entry.id === id);
    if (known) {
      setGameSavedDetail({ entry: known, mods: [] });
      setGameSavedDetailPending(true);
    }
    try {
      const detail = await api.getSavedCollection(id);
      if (request !== gameSavedOpenRef.current) return;
      startTransition(() => {
        if (request !== gameSavedOpenRef.current) return;
        setGameSavedDetail(detail);
        setGameSavedDetailPending(false);
      });
    } catch (e) {
      if (request !== gameSavedOpenRef.current) return;
      setGameSavedDetailPending(false);
      setError(String(e));
    }
  }

  async function installGameSavedCollection() {
    if (!gameSavedDetail || !libraryDetail) return;
    await runUnifiedImport(
      libraryDetail.game.id,
      gameSavedDetail.entry.code,
      gameSavedDetail.entry.name,
    );
  }

  const gameSavedCollections = useMemo(() => {
    if (!libraryDetail) return [];
    return savedCollections.filter((c) =>
      savedCollectionMatchesGame(c, libraryDetail.game),
    );
  }, [savedCollections, libraryDetail]);

  const savedCollectionGameGroups = useMemo(
    () =>
      groupSavedCollectionsByGame(
        savedCollections,
        managed,
        detected,
        gamePlugins,
        collectionCatalogCovers,
        collectionCatalogTitles,
      ),
    [
      savedCollections,
      managed,
      detected,
      gamePlugins,
      collectionCatalogCovers,
      collectionCatalogTitles,
    ],
  );

  useEffect(() => {
    if (tab !== "collections") return;
    const requests = collectionCatalogCoverRequests(
      savedCollectionGameGroups,
      gamePlugins,
      detected,
    ).filter(
      (req) => !collectionCoverAttemptedRef.current.has(catalogCoverRequestKey(req)),
    );
    if (requests.length === 0) return;
    for (const req of requests) {
      collectionCoverAttemptedRef.current.add(catalogCoverRequestKey(req));
    }
    let cancelled = false;
    void api
      .resolveCatalogCovers(requests)
      .then((hits) => {
        if (cancelled || hits.length === 0) return;
        startTransition(() => {
          setCollectionCatalogCovers((prev) =>
            mergeCollectionCatalogCovers(prev, hits),
          );
          setCollectionCatalogTitles((prev) =>
            mergeCollectionCatalogTitles(prev, hits),
          );
        });
      })
      .catch((e) => {
        console.warn("resolve_catalog_covers (collections) failed", e);
      });
    return () => {
      cancelled = true;
    };
  }, [tab, savedCollectionGameGroups, gamePlugins, detected]);

  const visibleCollectionGroupCount = useChunkedCount(
    savedCollectionGameGroups.length,
    { initial: 16, step: 16, active: tab === "collections" && !savedDetail },
  );

  const refreshCollectionsPageMeta = useCallback(
    async (force = true) => {
      if (!force && navTtlRef.current.isFresh("collections-meta", 30_000)) {
        return;
      }
      const gen = ++collectionsMetaGenRef.current;
      if (gamePlugins.length === 0) {
        try {
          const plugins = await api.listPlugins();
          if (gen !== collectionsMetaGenRef.current) return;
          setGamePlugins(plugins);
        } catch {
          /* plugin titles are optional */
        }
      }

      const managedMatches = managed.filter((game) =>
        savedCollections.some((entry) => savedCollectionMatchesGame(entry, game)),
      );

      const installedIds = new Set<string>();
      const adviceEntries: Record<string, CollectionVersionAdvice> = {};

      await Promise.all(
        managedMatches.map(async (game) => {
          try {
            const list = await api.listInstalledCollections(game.id);
            for (const c of list) {
              if (c.source !== "emperor") continue;
              installedIds.add(c.id);
              if (c.profile_code?.trim()) {
                installedIds.add(c.profile_code.trim());
              }
            }
          } catch {
            /* ignore per-game failures */
          }

          const entries = savedCollections.filter((entry) =>
            savedCollectionMatchesGame(entry, game),
          );
          if (entries.length === 0) return;
          try {
            const advice = await api.adviseCollectionVersions(
              game.id,
              entries.map((entry) => ({
                id: entry.id,
                gameVersions: entry.game_versions,
              })),
            );
            for (const row of advice) {
              adviceEntries[row.id] = row;
            }
          } catch {
            /* ignore advice failures */
          }
        }),
      );

      if (gen !== collectionsMetaGenRef.current) return;
      navTtlRef.current.mark("collections-meta");
      startTransition(() => {
        setInstalledEmperorShareIds((prev) =>
          stringSetEqual(prev, installedIds) ? prev : installedIds,
        );
        setSavedCollectionAdvice((prev) =>
          shallowRecordOfObjectsEqual(prev, adviceEntries) ? prev : adviceEntries,
        );
      });
    },
    [gamePlugins.length, managed, savedCollections],
  );

  const refreshFriendsSharingState = useCallback(async () => {
    try {
      const next = await api.getFriendsState();
      setFriendsState(next);
    } catch {
      /* ignore if p2p state unavailable */
    }
  }, []);

  function getSavedCollectionVisibility(entryId: string): CollectionVisibility {
    const colId = `saved:${entryId}`;
    const localCol = friendsState?.local_collections.find((c) => c.id === colId);
    return resolveCollectionVisibility(localCol, colId);
  }

  function openSavedCollectionSharing(entry: SavedCollectionEntry) {
    const colId = `saved:${entry.id}`;
    setSharingTarget({
      id: colId,
      name: entry.name,
      visibility: getSavedCollectionVisibility(entry.id),
    });
    if (!friendsState) {
      void refreshFriendsSharingState();
    }
  }

  const [togglingAppearOffline, setTogglingAppearOffline] = useState(false);

  const handleToggleAppearOffline = useCallback(async () => {
    const nextOffline = !(friendsState?.appear_offline ?? false);
    setTogglingAppearOffline(true);
    try {
      const updated = await api.setP2pAppearOffline(nextOffline);
      setFriendsState(updated);
    } catch (e) {
      setError(String(e));
    } finally {
      setTogglingAppearOffline(false);
    }
  }, [friendsState?.appear_offline]);

  useEffect(() => {
    void refreshFriendsSharingState();
  }, [refreshFriendsSharingState]);

  useEffect(() => {
    const unlisten = listen("friends-changed", () => {
      void refreshFriendsSharingState();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [refreshFriendsSharingState]);

  useNavSettledEffect(
    tab === "settings",
    0,
    () => {
      void refreshFriendsSharingState();
    },
    [refreshFriendsSharingState],
  );

  useNavSettledEffect(
    tab === "collections",
    savedCollectionsReady ? 150 : 0,
    () => {
      void refreshSavedCollections(false);
      void refreshFriendsSharingState();
    },
    [refreshSavedCollections, refreshFriendsSharingState, savedCollectionsReady],
  );

  useNavSettledEffect(
    tab === "collections",
    150,
    () => {
      void refreshCollectionsPageMeta(false);
      return () => {
        collectionsMetaGenRef.current += 1;
      };
    },
    [refreshCollectionsPageMeta],
  );

  const isSavedCollectionInstalled = useCallback(
    (entry: SavedCollectionEntry) => {
      if (installedEmperorShareIds.has(entry.code.trim())) return true;
      return Boolean(entry.share_id && installedEmperorShareIds.has(entry.share_id));
    },
    [installedEmperorShareIds],
  );

  async function installSavedCollection() {
    if (!savedDetail) return;
    const gameId = savedInstallGameId;
    if (!gameId) {
      setError("Select a managed game to install into.");
      return;
    }
    await runUnifiedImport(
      gameId,
      savedDetail.entry.code,
      savedDetail.entry.name,
    );
    if (tab === "collections") void refreshCollectionsPageMeta();
  }

  async function retryCollectionIssues(
    report: CollectionIntegrityReport,
    selected: CollectionModIssue[],
  ) {
    if (!activeGame || selected.length === 0) return;
    const col = installedCollections.find((c) => c.id === report.collection_id);
    const assistNexus = !isPremium
      ? selected.filter((i) => i.source === "nexus")
      : [];
    const directTargets = isPremium
      ? selected
      : selected.filter((i) => i.source !== "nexus");

    setIntegrityRetrying(true);
    setError(null);
    try {
      let retriedCount = 0;
      const failures: string[] = [];

      if (directTargets.length > 0) {
        const res = await api.retryCollectionMods(
          activeGame.id,
          report.collection_id,
          directTargets.map((i) => ({
            mod_id: i.mod_id,
            name: i.name,
            source: i.source,
            domain: i.domain,
            nexus_mod_id: i.nexus_mod_id,
            nexus_file_id: i.nexus_file_id,
            ts_namespace: i.ts_namespace ?? null,
            ts_name: i.ts_name ?? null,
            modio_game_id: i.modio_game_id ?? null,
            modio_mod_id: i.modio_mod_id ?? null,
            modio_file_id: i.modio_file_id ?? null,
            version: i.version ?? null,
            file_name: i.file_name ?? null,
          })),
        );
        retriedCount += res.retried;
        for (const f of res.failed) {
          failures.push(`${f.name}: ${f.error}`);
        }
      }

      if (assistNexus.length > 0) {
        const entries = assistNexus.map((i) =>
          createQueueEntry(
            activeGame.id,
            i.domain,
            i.nexus_mod_id,
            i.nexus_file_id,
            i.name,
            i.version ?? null,
            null,
            chosenFileLabel(i.name, { name: i.file_name ?? null }),
          ),
        );
        const nexusCol =
          col?.source === "nexus" && col.slug
            ? {
                slug: col.slug,
                name: col.name,
                revision: col.revision ?? null,
                existingModIds: mods.map((m) => m.id),
                files: assistNexus.map((i) => ({
                  modId: i.nexus_mod_id,
                  fileId: i.nexus_file_id,
                })),
              }
            : null;
        const emperorShare =
          col?.source === "emperor"
            ? {
                collectionId: col.id,
                name: col.name,
                code: col.profile_code ?? null,
                existingModIds: mods.map((m) => m.id),
                memberIds: col.mod_ids,
                files: assistNexus.map((i) => ({
                  domain: i.domain,
                  modId: i.nexus_mod_id,
                  fileId: i.nexus_file_id,
                })),
              }
            : null;
        await enqueueAssistEntries(
          entries,
          `${report.collection_name} (retry)`,
          nexusCol,
          emperorShare,
        );
      }

      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      await refreshDownloads();
      setIntegrityDialogReport(null);

      if (failures.length > 0) {
        setError(
          `Retried ${retriedCount} mod(s), ${failures.length} failed: ${failures.join("; ")}`,
        );
      } else if (retriedCount > 0 && assistNexus.length === 0) {
        setNotice({
          kind: "ok",
          message: `Retried ${retriedCount} mod${retriedCount === 1 ? "" : "s"} in ${report.collection_name}.`,
        });
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setIntegrityRetrying(false);
    }
  }

  async function uninstallInstalledCollection(c: InstalledCollection) {
    if (!activeGame) return;
    await withBusy(`Uninstalling ${c.name}…`, async () => {
      await api.uninstallCollection(activeGame.id, c.id);
      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      if (modsCollectionFilter === c.id) setModsCollectionFilter("");
      if (tab === "collections") await refreshCollectionsPageMeta();
    });
  }

  async function deleteSavedCollectionEntry(entry: SavedCollectionEntry) {
    let shareId: string | null = entry.share_id ?? null;
    if (!shareId) {
      try {
        shareId = await api.emperorShareId(entry.code);
      } catch {
        shareId = null;
      }
    }
    await api.deleteSavedCollection(entry.id);
    await refreshSavedCollections();
    if (activeGame) {
      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      if (shareId && modsCollectionFilter === shareId) {
        setModsCollectionFilter("");
      }
    }
    if (tab === "collections") {
      await refreshCollectionsPageMeta();
    }
  }

  async function updateOneStagedMod(stagedId: string) {
    if (!activeGame) return;
    const m = mods.find((x) => x.id === stagedId);
    const upd = modUpdates[stagedId];
    if (!m || !upd) return;

    const clearUpdateBadge = () => {
      setModUpdates((prev) => {
        const next = { ...prev };
        delete next[stagedId];
        return next;
      });
    };

    if (
      m.source === "nexus" &&
      !isPremium &&
      upd.nexus_file_id != null
    ) {
      clearUpdateBadge();
      await enqueueAssistEntries(
        [
          createQueueEntry(
            activeGame.id,
            m.domain || activeGame.nexus_domain,
            m.nexus_mod_id,
            upd.nexus_file_id,
            m.name,
            upd.available_version,
            m.id,
            upd.file_name,
          ),
        ],
        `Update ${m.name}`,
      );
      return;
    }

    await withBusy("Updating mod…", async () => {
      const updated = await api.updateStagedMod(activeGame.id, stagedId);
      const refreshed = await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      clearUpdateBadge();
      void refreshModUpdates(activeGame.id);
      if (refreshed) {
        openUpdatedFomods(refreshed.list, refreshed.needing, [updated.id]);
      }
    });
  }

  async function checkStagedModUpdatesNow() {
    if (!activeGame) return;
    const gameId = activeGame.id;
    const updates = await refreshModUpdates(gameId);
    if (updates == null) return;
    const count = updates.length;
    setNotice({
      kind: "ok",
      message:
        count === 0
          ? "No updates available."
          : `${count} update${count === 1 ? "" : "s"} available.`,
    });
  }

  async function updateAllStagedMods() {
    if (!activeGame) return;
    const pending = Object.entries(modUpdates);
    if (pending.length === 0) return;

    const assistEntries: AssistQueueEntry[] = [];
    const apiIds: string[] = [];

    for (const [stagedId, upd] of pending) {
      const m = mods.find((x) => x.id === stagedId);
      if (!m) continue;
      if (
        m.source === "nexus" &&
        !isPremium &&
        upd.nexus_file_id != null
      ) {
        assistEntries.push(
          createQueueEntry(
            activeGame.id,
            m.domain || activeGame.nexus_domain,
            m.nexus_mod_id,
            upd.nexus_file_id,
            m.name,
            upd.available_version,
            m.id,
            upd.file_name,
          ),
        );
      } else {
        apiIds.push(stagedId);
      }
    }

    if (assistEntries.length > 0) {
      setModUpdates((prev) => {
        const next = { ...prev };
        for (const e of assistEntries) {
          if (e.replaceStagedId) delete next[e.replaceStagedId];
        }
        return next;
      });
      try {
        await enqueueAssistEntries(
          assistEntries,
          `Update ${assistEntries.length} mod${assistEntries.length === 1 ? "" : "s"}`,
        );
      } catch (e) {
        setError(String(e));
      }
    }

    if (apiIds.length > 0) {
      await withBusy(
        `Updating ${apiIds.length} mod${apiIds.length === 1 ? "" : "s"}…`,
        async () => {
          const updatedIds: string[] = [];
          for (const id of apiIds) {
            const updated = await api.updateStagedMod(activeGame.id, id);
            updatedIds.push(updated.id);
          }
          const refreshed = await refreshMods(activeGame.id);
          await refreshCollections(activeGame.id);
          setModUpdates((prev) => {
            const next = { ...prev };
            for (const id of apiIds) delete next[id];
            return next;
          });
          void refreshModUpdates(activeGame.id);
          if (refreshed) {
            openUpdatedFomods(refreshed.list, refreshed.needing, updatedIds);
          }
        },
      );
    }
  }

  async function importPaths(paths: string[]) {
    if (!activeGame || paths.length === 0) return;
    setImportModalOpen(false);
    setImportDropActive(false);
    await withBusy("Importing mods…", async () => {
      const result = await api.importMods({
        gameId: activeGame.id,
        paths,
      });
      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      const count = result.mods.length;
      if (result.errors.length > 0) {
        const imported =
          count > 0
            ? `Imported ${count} mod${count === 1 ? "" : "s"}. `
            : "";
        setError(`${imported}${result.errors.join(" ")}`);
        return;
      }
      if (count > 0) {
        setNotice({
          kind: "ok",
          message: `Imported ${count} mod${count === 1 ? "" : "s"}.`,
        });
      }
    });
  }

  async function pickAndImportFiles() {
    if (!activeGame) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      multiple: true,
      title: "Import mod files or archives",
      filters: [
        {
          name: "Mods",
          extensions: ["zip", "7z", "rar", "package", "ts4script", "dbc", "sims3pack"],
        },
        { name: "Archives", extensions: ["zip", "7z", "rar"] },
      ],
    });
    if (!selected) return;
    const paths = Array.isArray(selected) ? selected : [selected];
    await importPaths(paths);
  }

  async function pickAndImportFolder() {
    if (!activeGame) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Import mods from folder",
    });
    if (!selected || Array.isArray(selected)) return;
    await importPaths([selected]);
  }

  useEffect(() => {
    if (!importModalOpen) {
      setImportDropActive(false);
      return;
    }
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void import("@tauri-apps/api/window")
      .then(({ getCurrentWindow }) =>
        getCurrentWindow().onDragDropEvent((event) => {
          if (cancelled) return;
          if (event.payload.type === "enter" || event.payload.type === "over") {
            setImportDropActive(true);
          } else if (event.payload.type === "leave") {
            setImportDropActive(false);
          } else if (event.payload.type === "drop") {
            setImportDropActive(false);
            void importPaths(event.payload.paths);
          }
        }),
      )
      .then((fn) => {
        if (cancelled) {
          fn();
        } else {
          unlisten = fn;
        }
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [importModalOpen, activeGame?.id]);

  async function toggleMod(m: StagedMod) {
    if (!activeGame) return;
    await api.setModEnabled(activeGame.id, m.id, !m.enabled);
    await refreshMods(activeGame.id);
  }

  async function enableStagedMods(ids: string[]) {
    if (!activeGame || ids.length === 0) return;
    setError(null);
    try {
      for (const id of ids) {
        await api.setModEnabled(activeGame.id, id, true);
      }
      await refreshMods(activeGame.id);
    } catch (e) {
      setError(String(e));
      if (activeGame) await refreshMods(activeGame.id);
    }
  }

  async function disableStagedMods(ids: string[]) {
    if (!activeGame || ids.length === 0) return;
    setError(null);
    try {
      for (const id of ids) {
        await api.setModEnabled(activeGame.id, id, false);
      }
      await refreshMods(activeGame.id);
    } catch (e) {
      setError(String(e));
      if (activeGame) await refreshMods(activeGame.id);
    }
  }

  async function moveMod(modId: string, dir: -1 | 1) {
    if (!activeGame || !canReorderMods) return;
    const next = reorderStagedMods(mods, modId, dir);
    if (!next) return;
    await api.setLoadOrder(
      activeGame.id,
      next.map((m) => m.id),
    );
    setMods(next);
  }

  async function queueSelectedRequirements(rows: NexusRequirementDownload[]) {
    if (!activeGame || rows.length === 0) return;
    const gameId = activeGame.id;
    const replaceIdFor = (row: NexusRequirementDownload) => {
      const domain = row.domain.toLowerCase();
      return (
        mods.find(
          (m) =>
            m.source === "nexus" &&
            m.nexus_mod_id === row.mod_id &&
            (m.domain || "").toLowerCase() === domain,
        )?.id ?? null
      );
    };
    goToDownloadsOnInstall();
    setError(null);
    if (isPremium) {
      setBusy(
        `Downloading ${rows.length} requirement${rows.length === 1 ? "" : "s"}…`,
      );
      try {
        for (const row of rows) {
          const staged = await api.downloadMod({
            gameId,
            domain: row.domain,
            modId: row.mod_id,
            fileId: row.file_id,
            name: row.name,
            version: row.version,
            replaceStagedId: replaceIdFor(row),
            fileName: row.file_name,
          });
          const parents = row.required_by.filter((id) => id && id !== staged.id);
          if (parents.length > 0) {
            await api.linkStagedRequirements(gameId, staged.id, parents);
          }
        }
        await refreshMods(gameId);
        await refreshDownloads();
        await refreshMissingRequirements(gameId);
        setRequirementsModal(null);
        setNotice({
          kind: "ok",
          message: `Downloaded ${rows.length} requirement${rows.length === 1 ? "" : "s"}.`,
        });
      } catch (e) {
        setError(String(e));
        await refreshMods(gameId);
        await refreshDownloads();
        await refreshMissingRequirements(gameId);
      } finally {
        setBusy(null);
      }
      return;
    }
    for (const row of rows) {
      const parents = row.required_by.filter(Boolean);
      if (parents.length > 0) {
        pendingRequirementLinksRef.current.set(
          `${row.mod_id}_${row.file_id}`,
          parents,
        );
      }
    }
    try {
      await enqueueAssistEntries(
        rows.map((row) =>
          createQueueEntry(
            gameId,
            row.domain,
            row.mod_id,
            row.file_id,
            row.name,
            row.version,
            replaceIdFor(row),
            row.file_name,
          ),
        ),
        "Missing requirements",
      );
      setRequirementsModal(null);
      setNotice({
        kind: "ok",
        message: `Added ${rows.length} requirement${
          rows.length === 1 ? "" : "s"
        } to Download Assist.`,
      });
    } catch (e) {
      setError(String(e));
    }
  }

  async function openModRequirements(domain: string, modId: number, name: string) {
    if (!activeGame) return;
    setBusy("Checking requirements…");
    setError(null);
    try {
      setRequirementsModal(
        await api.nexusModRequirements({
          gameId: activeGame.id,
          domain,
          modId,
          name,
        }),
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function saveIgnoredRequirements(next: IgnoredRequirement[]) {
    if (!activeGame) return;
    const gameId = activeGame.id;
    setIgnoreSaving(true);
    setError(null);
    try {
      const updated = await api.setIgnoredRequirements(gameId, next);
      setManaged((prev) => prev.map((game) => (game.id === updated.id ? updated : game)));
      setLibraryDetail((detail) =>
        detail && detail.game.id === updated.id ? { ...detail, game: updated } : detail,
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setIgnoreSaving(false);
    }
  }

  function ignoreRequirements(rows: IgnoredRequirement[]) {
    if (!activeGame || rows.length === 0) return;
    const current = activeGame.ignored_requirements ?? [];
    const existingKeys = new Set(current.map((item) => item.key));
    const additions: IgnoredRequirement[] = [];
    for (const row of rows) {
      if (existingKeys.has(row.key)) continue;
      existingKeys.add(row.key);
      additions.push(row);
    }
    if (additions.length === 0) return;
    void saveIgnoredRequirements([...current, ...additions]);
  }

  function ignoreRequirement(row: IgnoredRequirement) {
    ignoreRequirements([row]);
  }

  function unignoreRequirement(key: string) {
    if (!activeGame) return;
    void saveIgnoredRequirements(
      (activeGame.ignored_requirements ?? []).filter((item) => item.key !== key),
    );
  }

  function openRequirementsReview(payload?: MissingRequirements | null) {
    setRequirementsModal(
      payload ??
        missingRequirements ?? {
          nexus: [],
          external: [],
          dlc: [],
        },
    );
  }

  async function commitGameVersion(next: string) {
    if (!activeGame || !gameVersionReport) return;
    const trimmed = next.trim();
    const saved = gameVersionReport.override_version ?? "";
    const installed = gameVersionReport.installed ?? "";
    const nextOverride = !trimmed || trimmed === installed ? "" : trimmed;
    if (nextOverride === saved) {
      setGameVersionDraft(gameVersionReport.current ?? "");
      return;
    }
    try {
      const updated = await api.setGameVersion(activeGame.id, nextOverride || null);
      setManaged((prev) => prev.map((game) => (game.id === updated.id ? updated : game)));
      setLibraryDetail((detail) =>
        detail && detail.game.id === updated.id ? { ...detail, game: updated } : detail,
      );
      await refreshGameVersions(activeGame.id);
    } catch (e) {
      setError(String(e));
    }
  }

  function openFomodPass(modsToConfigure: StagedMod[], deployAfter: boolean) {
    const step = startFomodPass(modsToConfigure, deployAfter);
    if (step.action === "open") {
      const next = { mod: fomodPassCurrent(step.pass), pass: step.pass };
      fomodDialogRef.current = next;
      setFomodDialog(next);
      return;
    }
    if (step.action === "deploy") void runLaunchModded();
  }

  function openUpdatedFomods(
    list: StagedMod[],
    needing: string[],
    updatedIds: string[],
  ) {
    if (updatedIds.length === 0 || fomodDialogRef.current) return;
    const needingSet = new Set(needing);
    const byId = new Map(list.map((mod) => [mod.id, mod]));
    const toOpen: StagedMod[] = [];
    const seen = new Set<string>();
    for (const id of updatedIds) {
      if (seen.has(id) || !needingSet.has(id)) continue;
      const mod = byId.get(id);
      if (!mod || mod.fomod_selection) continue;
      seen.add(id);
      toOpen.push(mod);
    }
    if (toOpen.length > 0) {
      openFomodPass(toOpen, false);
    }
  }
  openUpdatedFomodsRef.current = openUpdatedFomods;

  function configureFomods() {
    openFomodPass(fomodNeeding, false);
  }

  function closeFomodDialog(reason?: "saved" | "cancel") {
    const current = fomodDialogRef.current;
    if (reason === "saved" && current?.pass) {
      const step = finishFomodPass(current.pass);
      if (step.action === "open") {
        const next = { mod: fomodPassCurrent(step.pass), pass: step.pass };
        fomodDialogRef.current = next;
        setFomodDialog(next);
        return;
      }
      fomodDialogRef.current = null;
      setFomodDialog(null);
      if (step.action === "deploy") {
        void runLaunchModded();
        return;
      }
      setNotice({
        kind: "ok",
        message:
          current.pass.mods.length > 1
            ? "Saved installer choices. Launch modded to apply them."
            : `Saved installer choices for ${current.mod.name}. Launch modded to apply them.`,
      });
      return;
    }
    if (current?.pass) cancelFomodPass(current.pass);
    if (reason === "saved" && current) {
      setNotice({
        kind: "ok",
        message: `Saved installer choices for ${current.mod.name}. Launch modded to apply them.`,
      });
    }
    fomodDialogRef.current = null;
    setFomodDialog(null);
  }

  async function runLaunchModded() {
    const game = activeGameRef.current;
    if (!game) return;
    await withBusy("Deploying mods & launching…", async () => {
      const res = await api.launchModded(game.id);
      setLaunchStates((prev) => ({ ...prev, [game.id]: res.state }));
      setActiveDeploymentByGame((prev) => ({ ...prev, [game.id]: true }));
      void refreshMissingRequirements(game.id);
      void refreshWineAdvice(game.id);
      if (res.deploy_result) {
        const full = res.deploy_result.missing_requirements;
        setNotice(
          deployStatusNotice(
            {
              ...res.deploy_result,
              missing_requirements: full
                ? withoutIgnoredRequirements(full, game.ignored_requirements ?? [])
                : full,
            },
            (requirements) => {
              setRequirementsModal(full ?? requirements);
            },
          ),
        );
      }
    });
  }

  async function launchModded() {
    if (!activeGame) return;
    const gameId = activeGame.id;
    let currentMods = mods;
    let needingIds = shownFomodNeedingIds;
    try {
      needingIds = await api.listModsNeedingFomod(gameId);
      if (activeGameRef.current?.id !== gameId) return;
      setFomodNeedingIds(needingIds);
      if (needingIds.some((id) => !currentMods.some((m) => m.id === id))) {
        currentMods = await api.listMods(gameId);
        if (activeGameRef.current?.id !== gameId) return;
        setMods(currentMods);
      }
    } catch (e) {
      console.warn("list_mods_needing_fomod before launch failed", e);
    }
    const byId = new Map(currentMods.map((mod) => [mod.id, mod]));
    const needingMods = needingIds.flatMap((id) => {
      const mod = byId.get(id);
      if (!mod || mod.fomod_selection) return [];
      return [mod];
    });
    const pending = fomodModsForDeploy(needingMods);
    if (pending.length > 0) {
      openFomodPass(pending, true);
      return;
    }
    await runLaunchModded();
  }

  async function applyWineOverrides() {
    if (!activeGame) return;
    setWineApplyBusy(true);
    setError(null);
    try {
      const next = await api.applyWineOverrides(activeGame.id);
      wineAdviceReqRef.current += 1;
      if (activeGameRef.current?.id === activeGame.id) {
        setWineAdvice(next);
      }
      if (!next) {
        setNotice({
          kind: "ok",
          message:
            "Updated Steam launch options. Restart Steam if the game was already running.",
        });
      }
    } catch (e) {
      setError(String(e));
    } finally {
      setWineApplyBusy(false);
    }
  }

  async function dismissWineOverrides() {
    if (!activeGame) return;
    const gameId = activeGame.id;
    setError(null);
    try {
      const updated = await api.setWineOverridesDismissed(gameId, true);
      setManaged((prev) =>
        prev.map((game) => (game.id === updated.id ? updated : game)),
      );
      setLibraryDetail((detail) =>
        detail && detail.game.id === updated.id
          ? { ...detail, game: updated }
          : detail,
      );
      setNotice({
        kind: "warn",
        message:
          "Proton launch options dismissed. They are still available in Info from the menu (☰).",
        action: {
          label: "Open Info",
          run: () => setGameModal("info"),
          dismiss: true,
        },
      });
    } catch (e) {
      setError(String(e));
    }
  }

  async function checkLoadLogs() {
    if (!activeGame) return;
    await withBusy("Checking loader logs…", async () => {
      const result = await api.diagnoseLoad(activeGame.id);
      setLoadDiagnosis(result);
    });
  }

  async function launchUnmodded() {
    if (!activeGame) return;
    const game = activeGame;
    await withBusy("Launching unmodded…", async () => {
      const st = await api.launchUnmodded(game.id);
      setLaunchStates((prev) => ({ ...prev, [game.id]: st }));
      setActiveDeploymentByGame((prev) => ({ ...prev, [game.id]: false }));
      void refreshWineAdvice(game.id);
      setNotice({ kind: "ok", message: `Launched ${game.title} unmodded.` });
    });
  }

  async function revertDeployment() {
    if (!activeGame) return;
    const game = activeGame;
    await withBusy("Reverting deployed files…", async () => {
      await api.revertDeployment(game.id);
      setActiveDeploymentByGame((prev) => ({ ...prev, [game.id]: false }));
      void refreshWineAdvice(game.id);
      setNotice({ kind: "ok", message: "Reverted deployed files." });
    });
  }

  async function closeRunningGame() {
    if (!activeGame) return;
    const game = activeGame;
    try {
      await api.closeGame(game.id);
      setNotice({ kind: "ok", message: `Closing ${game.title}…` });
    } catch (e) {
      setError(String(e));
    }
  }

  useEffect(() => {
    if (gameModal !== "launch" || !libraryDetail) return;
    let active = true;
    const gameId = libraryDetail.game.id;
    setNewEnvName("");
    setNewEnvValue("");
    void api
      .getLaunchPreview(gameId)
      .then((preview) => {
        if (!active) return;
        setLaunchPreview(preview);
        setWrapperDraft(preview.custom_wrapper);
        setArgsDraft(preview.custom_launch_args);
      })
      .catch((e) => {
        if (active) setError(String(e));
      });
    return () => {
      active = false;
    };
  }, [gameModal, libraryDetail?.game.id]);

  async function saveLaunchOptions(
    nextSource: LaunchCommandSource,
    nextCustomVars: LaunchEnvVar[],
    nextWrapper: string,
    nextArgs: string,
  ) {
    if (!libraryDetail) return;
    const game = libraryDetail.game;
    try {
      const updated = await api.setGameLaunchOptions(
        game.id,
        nextSource,
        nextCustomVars,
        nextWrapper.trim() || null,
        nextArgs.trim() || null,
      );
      setLaunchPreview(updated);
      setWrapperDraft(updated.custom_wrapper);
      setArgsDraft(updated.custom_launch_args);
      setManaged((prev) =>
        prev.map((g) =>
          g.id === game.id
            ? {
                ...g,
                launch_command_source: nextSource,
                custom_env_vars: updated.custom_env_vars,
                custom_wrapper: updated.custom_wrapper || null,
                custom_launch_args: updated.custom_launch_args || null,
              }
            : g,
        ),
      );
      setLibraryDetail((prev) =>
        prev && prev.game.id === game.id
          ? {
              ...prev,
              game: {
                ...prev.game,
                launch_command_source: nextSource,
                custom_env_vars: updated.custom_env_vars,
                custom_wrapper: updated.custom_wrapper || null,
                custom_launch_args: updated.custom_launch_args || null,
              },
            }
          : prev,
      );
    } catch (e) {
      setError(String(e));
    }
  }

  async function setGameLaunchCommandSource(next: LaunchCommandSource) {
    if (!activeGame) return;
    const currentVars = launchPreview?.custom_env_vars ?? activeGame.custom_env_vars ?? [];
    await saveLaunchOptions(next, currentVars, wrapperDraft, argsDraft);
    setNotice({
      kind: "ok",
      message:
        next === "emperor"
          ? "Using Emperor launch commands (WINEDLLOVERRIDES & tool overrides)."
          : "Using game launcher's own launch commands.",
    });
  }

  async function removeAllMods() {
    if (!activeGame || mods.length === 0) return;
    await withBusy("Removing all mods…", async () => {
      await api.removeAllMods(activeGame.id);
      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      setModsCollectionFilter("");
    });
  }

  function setTheme(theme: ThemePreference) {
    withBusy("Saving theme…", async () => {
      await api.setTheme(theme);
      await refreshSettings();
    });
  }

  function setThemeStyle(style: ThemeStyle) {
    withBusy("Saving theme style…", async () => {
      await api.setThemeStyle(style);
      await refreshSettings();
    });
  }

  function setAccentColor(color: string) {
    withBusy("Saving accent…", async () => {
      await api.setAccentColor(color);
      await refreshSettings();
    });
  }

  function setLinuxSystemBackend(backend: LinuxSystemBackend) {
    withBusy("Saving toolkit…", async () => {
      await api.setLinuxSystemBackend(backend);
      await refreshSettings();
    });
  }

  function setInstallClickBehavior(behavior: InstallClickBehavior) {
    withBusy("Saving…", async () => {
      await api.setInstallClickBehavior(behavior);
      await refreshSettings();
    });
  }

  function goToDownloadsOnInstall() {
    if (settings?.install_click_behavior !== "stay") {
      setTab("downloads");
    }
  }

  async function recoverLegacyMods() {
    await withBusy("Recovering legacy staged mods…", async () => {
      const result = await api.recoverLegacyModData();
      await refreshManaged();
      await refreshOrphanScan();
      if (activeGameRef.current) {
        await refreshMods(activeGameRef.current.id);
      }
      const copied = result.games_copied.length;
      setNotice({
        kind: result.warnings.length ? "warn" : "ok",
        message:
          copied > 0 || result.mods_rewritten > 0
            ? `Recovered ${copied} game data folder${copied === 1 ? "" : "s"} and updated ${result.mods_rewritten} staged mod path${result.mods_rewritten === 1 ? "" : "s"}. Deploy each recovered game before removing nexus-manager data.`
            : "No recoverable legacy staging data was found.",
      });
    });
  }

  const sortOptions = browseMode === "mods" ? MOD_SORTS : COLLECTION_SORTS;
  const downloadStatus = statusSummary(busy, assistQueue, downloads, activeBatch);
  const showSidebarAssist =
    assistActive && tab !== "downloads" && !nexusLoginOpen;

  const browseGridHandlersRef = useRef({
    openMod,
    quickInstallMod,
    uninstallCatalogMod,
    openCollection,
    installCollection,
  });
  browseGridHandlersRef.current = {
    openMod,
    quickInstallMod,
    uninstallCatalogMod,
    openCollection,
    installCollection,
  };
  const getModHitKey = useCallback((m: CatalogHit) => m.id, []);
  const handleBrowseOpenMod = useCallback(
    (m: CatalogHit, nextTab: "info" | "files") =>
      browseGridHandlersRef.current.openMod(m, nextTab),
    [],
  );
  const handleBrowseQuickInstall = useCallback(
    (m: CatalogHit) => void browseGridHandlersRef.current.quickInstallMod(m),
    [],
  );
  const handleBrowseUninstall = useCallback(
    (m: CatalogHit) => void browseGridHandlersRef.current.uninstallCatalogMod(m),
    [],
  );
  const handleBrowseOpenCollection = useCallback(
    (c: CollectionHit, nextTab: "info" | "files") =>
      browseGridHandlersRef.current.openCollection(c, nextTab),
    [],
  );
  const handleBrowseInstallCollection = useCallback(
    (c: CollectionHit) => browseGridHandlersRef.current.installCollection(c),
    [],
  );
  const renderBrowseModGridItem = useCallback(
    (m: CatalogHit) => (
      <BrowseModGridCard
        mod={m}
        sort={browseSort}
        installed={installedIds.has(m.id)}
        isPremium={isPremium}
        onOpen={handleBrowseOpenMod}
        onQuickInstall={handleBrowseQuickInstall}
        onUninstall={handleBrowseUninstall}
      />
    ),
    [
      browseSort,
      handleBrowseOpenMod,
      handleBrowseQuickInstall,
      handleBrowseUninstall,
      installedIds,
      isPremium,
    ],
  );
  const renderBrowseCollectionGridItem = useCallback(
    (c: CollectionHit) => (
      <BrowseCollectionGridCard
        collection={c}
        onOpen={handleBrowseOpenCollection}
        onInstall={handleBrowseInstallCollection}
      />
    ),
    [handleBrowseInstallCollection, handleBrowseOpenCollection],
  );
  const renderBrowseGridSkeletonCard = useCallback(
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
  const renderLibraryGridSkeletonCard = useCallback(
    () => (
      <div className="media-card" aria-hidden="true">
        <div className="media-cover-stack">
          <div className="media-cover skeleton" />
        </div>
        <span className="skeleton skeleton-line skeleton-line-title" />
      </div>
    ),
    [],
  );
  const getLibraryGameKey = useCallback(
    (g: ManagedGame | DetectedGame) => g.id,
    [],
  );
  const managedHasActions = useMemo(
    () =>
      managed.some((g) => {
        const status = gameHealthById.get(g.id)?.status;
        return status === "relocate_candidate" || status === "missing";
      }),
    [gameHealthById, managed],
  );
  const renderLibraryManagedItem = useCallback(
    (g: ManagedGame) => {
      const health = gameHealthById.get(g.id);
      const badge = managedHealthBadge(health);
      const actions =
        health?.status === "relocate_candidate" ? (
          <>
            <button
              type="button"
              onClick={() =>
                setRelinkPrompt({
                  game: g,
                  health,
                })
              }
            >
              Relink
            </button>
            <button
              type="button"
              className="danger"
              onClick={() => openUnmanageModal(g)}
            >
              Unmanage
            </button>
          </>
        ) : health?.status === "missing" ? (
          <button
            type="button"
            className="danger"
            onClick={() => openUnmanageModal(g)}
          >
            Unmanage
          </button>
        ) : undefined;
      return (
        <MediaCard
          key={g.id}
          plain
          title={g.title}
          imageSrc={gameCoverSrc(g)}
          active={g.id === activeGame?.id}
          badge={badge}
          overlay={
            health?.status === "missing"
              ? g.install_path
              : health?.new_install_path ?? undefined
          }
          actions={actions}
          onClick={() => openGame(g)}
        />
      );
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [activeGame?.id, gameHealthById],
  );
  const renderLibraryUnmanagedItem = useCallback(
    (g: DetectedGame) => {
      const suggestion = catalogSuggestions[g.id];
      const hints = catalogHints(suggestion);
      if (g.supported) {
        return (
          <MediaCard
            key={g.id}
            plain
            title={g.title}
            imageSrc={gameCoverSrc(g)}
            coverMarks={hints}
            hoverLabel="Manage"
            onClick={() => manage(g)}
          />
        );
      }
      if (g.engine_hint === "unreal" && g.install_path) {
        const kind = catalogMatchCardKind(suggestion);
        const ready = kind === "ready";
        return (
          <MediaCard
            key={g.id}
            plain
            title={g.title}
            imageSrc={gameCoverSrc(g)}
            badge={catalogMatchBadge(kind)}
            coverMarks={hints}
            hoverLabel={catalogMatchHover(kind)}
            onClick={() =>
              ready ? manageUnrealQuick(g) : beginUnrealManage(g)
            }
          />
        );
      }
      if (g.engine_hint === "bepinex" && g.install_path) {
        const kind = catalogMatchCardKind(suggestion);
        const ready = kind === "ready";
        return (
          <MediaCard
            key={g.id}
            plain
            title={g.title}
            imageSrc={gameCoverSrc(g)}
            badge={catalogMatchBadge(kind)}
            coverMarks={hints}
            hoverLabel={catalogMatchHover(kind)}
            onClick={() =>
              ready ? manageBepinexQuick(g) : beginBepinexManage(g)
            }
          />
        );
      }
      return (
        <MediaCard
          key={g.id}
          plain
          title={g.title}
          imageSrc={gameCoverSrc(g)}
          coverMarks={hints}
          coverLabel="Unsupported"
          unsupported
        />
      );
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [catalogSuggestions],
  );

  function browseResultsNote(kind: "mods" | "collections"): string {
    if (browseLoading && !browseSearched) return "Searching…";
    if (browseSearched) return "No results for this search.";
    return kind === "mods"
      ? "Search to find mods for this game."
      : "Search to find collections for this game.";
  }

  function openDownloadsTab() {
    setTab("downloads");
    void refreshDownloads();
  }

  const navItems: [Tab, string][] = !settings
    ? []
    : settings.initial_setup_complete
      ? [
          ["library", "Library"],
          ["collections", "Collections"],
          ["friends", "Friends"],
          ["browse", "Browse"],
          ["downloads", "Downloads"],
          ...(settings.platform_linux ? [["tools", "Tools"] as [Tab, string]] : []),
          ["settings", "Settings"],
        ]
      : [["setup", "Setup"]];
  const nexusSignInBlocked = Boolean(assistQueue && !assistQueue.cancelled);
  const canFinishSetup = setupCanFinish(
    Boolean(settings?.has_api_key),
    Boolean(settings?.nexus_website_signed_in),
  );
  const accountConnections = (
    <AccountConnections
      settings={settings}
      apiKeyInput={apiKeyInput}
      onApiKeyInput={setApiKeyInput}
      onSaveApiKey={() => void saveApiKey()}
      onClearApiKey={() =>
        void withBusy("Clearing key…", async () => {
          await api.clearApiKey();
          await refreshSettings();
        })
      }
      modioKeyInput={modioKeyInput}
      onModioKeyInput={setModioKeyInput}
      onSaveModioApiKey={() => void saveModioApiKey()}
      onClearModioApiKey={() =>
        void withBusy("Clearing mod.io key…", async () => {
          await api.clearModioApiKey();
          await refreshSettings();
        })
      }
      signInDisabled={nexusSignInBlocked}
      onSignIn={() => {
        setWebLoginProvider("nexus");
        setNexusLoginOpen(true);
      }}
      onModioSignIn={() => {
        setWebLoginProvider("modio");
        setNexusLoginOpen(true);
      }}
      onClearWebsiteSession={() => void clearNexusWebsiteSession()}
    />
  );

  useNavSettledEffect(
    tab === "downloads",
    downloadsReady ? 150 : 0,
    () => {
      void refreshDownloads();
    },
    [downloadsReady, refreshDownloads],
  );

  return (
    <div
      className="app"
      style={
        activeGameCoverSrc
          ? ({
              "--active-game-cover": `url("${activeGameCoverSrc.replace(/"/g, '\\"')}")`,
            } as CSSProperties)
          : undefined
      }
    >
      {activeGameCoverSrc ? (
        <div className="app-game-backdrop" aria-hidden="true" />
      ) : null}
      <aside className="sidebar">
        <div className="sidebar-scroll">
        <div className="sidebar-top">
          <div
            className="brand"
            style={{ "--brand-mask": `url("${appLogo}")` } as CSSProperties}
          >
            <img className="brand-mark" src={appLogo} alt="Emperor Mod Manager" />
          </div>
          <Collapse show={navCanBack} className="sidebar-history">
            <div className="sidebar-history-inner">
              <button
                type="button"
                className="sidebar-back"
                onClick={() => void navBack()}
              >
                ← Back
              </button>
            </div>
          </Collapse>
        </div>
        <NavGroup>
          {navItems.map(([id, label]) => (
            <NavTab
              key={id}
              layoutId="sidebar-nav"
              active={tab === id}
              onClick={() => {
                setTab(id);
                if (id === "collections") {
                  savedOpenRef.current += 1;
                  setSavedDetail(null);
                  setSavedDetailPending(false);
                }
              }}
            >
              {label}
            </NavTab>
          ))}
        </NavGroup>
        <div className="sidebar-bottom">
          {activeGame ? (
            <div className="active-game">
              <span>Active</span>
              <strong>{activeGame.title}</strong>
              {(downloadStatus || showSidebarAssist) && (
                <div className="sidebar-status-block in-active-game">
                  {downloadStatus ? (
                    <button
                      type="button"
                      className="active-game-status"
                      onClick={openDownloadsTab}
                      title="Open Downloads"
                    >
                      <Throbber />
                      <span className="status-copy">
                        <span className="status-primary">{downloadStatus.primary}</span>
                        {downloadStatus.secondary ? (
                          <span className="status-secondary">{downloadStatus.secondary}</span>
                        ) : null}
                      </span>
                    </button>
                  ) : null}
                  {showSidebarAssist ? (
                    <div
                      ref={handleSidebarAssistHost}
                      className="sidebar-assist-host active"
                      aria-hidden
                    />
                  ) : null}
                </div>
              )}
            </div>
          ) : (
            (downloadStatus || showSidebarAssist) && (
              <div className="sidebar-status-block standalone">
                {downloadStatus ? (
                  <button
                    type="button"
                    className="sidebar-status"
                    onClick={openDownloadsTab}
                    title="Open Downloads"
                  >
                    <Throbber />
                    <span className="status-copy">
                      <span className="status-primary">{downloadStatus.primary}</span>
                      {downloadStatus.secondary ? (
                        <span className="status-secondary">{downloadStatus.secondary}</span>
                      ) : null}
                    </span>
                  </button>
                ) : null}
                {showSidebarAssist ? (
                  <div
                    ref={handleSidebarAssistHost}
                    className="sidebar-assist-host active"
                    aria-hidden
                  />
                ) : null}
              </div>
            )
          )}
          <div
            className="sidebar-p2p-status"
            title="Serverless iroh P2P endpoint status"
          >
            <div className="sidebar-p2p-info">
              <span
                className={`sidebar-p2p-dot ${
                  friendsState?.appear_offline
                    ? "offline"
                    : friendsState?.endpoint_ready
                      ? "ok"
                      : "warn"
                }`}
                aria-hidden="true"
              />
              <span className="sidebar-p2p-label">
                {friendsState?.appear_offline
                  ? "Appear Offline"
                  : friendsState?.endpoint_ready
                    ? "P2P Ready"
                    : "Connecting..."}
              </span>
            </div>
            <button
              type="button"
              className="sidebar-p2p-toggle"
              title={
                friendsState?.appear_offline
                  ? "Go online on P2P"
                  : "Appear offline to friends"
              }
              aria-label="Appear offline"
              aria-pressed={Boolean(friendsState?.appear_offline)}
              disabled={togglingAppearOffline}
              onClick={() => void handleToggleAppearOffline()}
            >
              <FontAwesomeIcon
                icon={friendsState?.appear_offline ? faEyeSlash : faEye}
                aria-hidden="true"
              />
            </button>
          </div>
        </div>
        </div>
      </aside>

      <main
        className={browseResultsVisible ? "main main-browse-scroll" : "main"}
        ref={mainScrollRef}
      >
        <Reveal show={banners.length > 0 || assistLoginBanner}>
          <div className="banner-stack">
            {assistLoginBanner ? (
              <StatusBanner
                kind="warn"
                message="Nexus website sign-in is required for Download Assist. Open Downloads to sign in (use Stay signed in), then the queue continues."
                action={{ label: "Open Downloads", run: () => openDownloadsTab() }}
                timeoutSeconds={bannerTimeoutFor("warn", settings)}
                onDismiss={() => setAssistLoginBanner(false)}
              />
            ) : null}
            {banners.map((b) => (
              <StatusBanner
                key={b.id}
                kind={b.kind}
                message={b.message}
                details={b.details}
                action={b.action}
                timeoutSeconds={remainingBannerTimeoutSeconds(
                  b,
                  settings,
                  sessionStartMsRef.current,
                )}
                onDismiss={() => dismissBannerById(b.id)}
              />
            ))}
          </div>
        </Reveal>

        {libraryKeptRef.current ? (
        <div
          className={tab === "library" ? "tab-kept" : "tab-kept tab-parked"}
          inert={tab !== "library"}
          aria-hidden={tab !== "library" ? true : undefined}
        >
          <ParkedTabGate active={tab === "library"}>
          <section className={libraryDetail ? "panel page-panel" : "panel page-panel library-home"}>
            <ViewSwitch
              viewKey={libraryShellKey}
              className="page-view"
              initial={false}
              onEntered={setLibraryShellSettled}
            >
            {libraryDetail ? (
              <>
                <div className="page-chrome">
                <div className="panel-head">
                  <div className="detail-head-title">
                    <button type="button" onClick={() => closeLibraryDetail()}>
                      Home
                    </button>
                    <h1>{libraryDetail.game.title}</h1>
                    <div className="detail-head-stats">
                      <span>
                        {mods.length} mod{mods.length === 1 ? "" : "s"}
                      </span>
                      <span>{enabledModCount} enabled</span>
                    </div>
                  </div>
                  <div className="panel-head-actions">
                    <button
                      type="button"
                      disabled={enabledModCount === 0}
                      onClick={() => openSaveLoadoutPrompt()}
                    >
                      Save loadout
                    </button>
                    {(() => {
                      const launchState = launchStates[libraryDetail.game.id];
                      const phase = launchState?.phase ?? "idle";
                      const isRunningOrStarting =
                        phase === "starting" || phase === "running";
                      const launchBusy = phase !== "idle";
                      const hasDeployment =
                        activeDeploymentByGame[libraryDetail.game.id] ?? false;
                      if (isRunningOrStarting) {
                        return (
                          <button
                            type="button"
                            className="danger launch-close-btn"
                            onClick={() => void closeRunningGame()}
                          >
                            <FontAwesomeIcon icon={faStop} />
                            <span>Close game</span>
                          </button>
                        );
                      }
                      const label =
                        phase === "deploying"
                          ? "Deploying…"
                          : phase === "reverting"
                            ? "Reverting…"
                            : "Launch modded";
                      return (
                        <div className="launch-split-menu" ref={launchMenuRef}>
                          <div className="launch-split-btn">
                            <button
                              type="button"
                              className="accent launch-split-primary"
                              disabled={launchBusy}
                              onClick={() => void launchModded()}
                            >
                              <FontAwesomeIcon icon={faPlay} />
                              <span>{label}</span>
                            </button>
                            <button
                              type="button"
                              className="accent launch-split-caret"
                              aria-label="Launch options"
                              aria-haspopup="menu"
                              aria-expanded={launchMenuOpen}
                              disabled={launchBusy}
                              onClick={() => setLaunchMenuOpen((open) => !open)}
                            >
                              <FontAwesomeIcon icon={faCaretDown} />
                            </button>
                          </div>
                          <PopIn
                            show={launchMenuOpen}
                            className="mods-actions-menu-popup game-settings-menu-popup"
                            role="menu"
                          >
                            <button
                              type="button"
                              role="menuitem"
                              onClick={() => {
                                setLaunchMenuOpen(false);
                                void launchUnmodded();
                              }}
                            >
                              Launch unmodded
                            </button>
                            <button
                              type="button"
                              role="menuitem"
                              onClick={() => {
                                setLaunchMenuOpen(false);
                                setGameModal("launch");
                              }}
                            >
                              Launch settings
                            </button>
                            {hasDeployment && !launchBusy && (
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  setLaunchMenuOpen(false);
                                  void revertDeployment();
                                }}
                              >
                                Revert deployment
                              </button>
                            )}
                          </PopIn>
                        </div>
                      );
                    })()}
                    <div
                      className="mods-actions-menu game-settings-menu"
                      ref={gameSettingsMenuRef}
                    >
                      <button
                        type="button"
                        className="mods-toolbar-icon-btn"
                        aria-label="Game settings"
                        aria-haspopup="menu"
                        aria-expanded={gameSettingsMenuOpen}
                        onClick={() =>
                          setGameSettingsMenuOpen((open) => !open)
                        }
                      >
                        <FontAwesomeIcon icon={faBars} />
                      </button>
                      <PopIn
                        show={gameSettingsMenuOpen}
                        className="mods-actions-menu-popup game-settings-menu-popup"
                        role="menu"
                      >
                        <button
                          type="button"
                          role="menuitem"
                          onClick={() => {
                            setGameSettingsMenuOpen(false);
                            setGameModal("info");
                          }}
                        >
                          Info
                        </button>
                        {settings?.platform_linux && (
                          <button
                            type="button"
                            role="menuitem"
                            onClick={() => {
                              setGameSettingsMenuOpen(false);
                              setGameModal("tools");
                            }}
                          >
                            Tools
                          </button>
                        )}
                        <button
                          type="button"
                          role="menuitem"
                          onClick={() => {
                            setGameSettingsMenuOpen(false);
                            void checkLoadLogs();
                          }}
                        >
                          Check logs
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          className="danger"
                          disabled={mods.length === 0}
                          onClick={() => {
                            setGameSettingsMenuOpen(false);
                            void removeAllMods();
                          }}
                        >
                          Remove all mods
                        </button>
                        <button
                          type="button"
                          role="menuitem"
                          className="danger"
                          onClick={() => {
                            setGameSettingsMenuOpen(false);
                            openUnmanageModal(libraryDetail.game);
                          }}
                        >
                          Unmanage
                        </button>
                      </PopIn>
                    </div>
                  </div>
                </div>

                <div className="detail-hero">
                  <div className="detail-cover">
                    {gameCoverSrc(libraryDetail.game) ? (
                      <img src={gameCoverSrc(libraryDetail.game)!} alt="" />
                    ) : (
                      <PlaceholderIcon />
                    )}
                  </div>
                  <div className="detail-meta">
                    <div className="detail-external-links">
                      <button
                        type="button"
                        onClick={() =>
                          openExternal(
                            gameInfo?.nexusmods_url ||
                              `https://www.nexusmods.com/${libraryDetail.game.nexus_domain}`,
                          )
                        }
                      >
                        Open on Nexus
                      </button>
                      {gameInfo?.forum_url && (
                        <button
                          type="button"
                          onClick={() => openExternal(gameInfo.forum_url!)}
                        >
                          Forum
                        </button>
                      )}
                    </div>
                    <div
                      className="detail-collections-box"
                      tabIndex={0}
                      onClick={(e) => {
                        if ((e.target as HTMLElement).closest("button")) {
                          return;
                        }
                        void refreshSavedCollections();
                        setGameModal("collections");
                      }}
                      onKeyDown={(e) => {
                        if (
                          e.target === e.currentTarget &&
                          (e.key === "Enter" || e.key === " ")
                        ) {
                          e.preventDefault();
                          void refreshSavedCollections();
                          setGameModal("collections");
                        }
                      }}
                    >
                      <div className="detail-collections-header">
                        <span className="detail-collections-title">
                          Collections
                        </span>
                        {installedCollections.length > 0 && (
                          <span className="detail-collections-count">
                            · {installedCollections.length}
                          </span>
                        )}
                      </div>
                      {installedCollections.length > 0 ? (
                        <div
                          className="detail-installed-collections"
                          aria-label="Installed collections"
                        >
                          {installedCollections.map((c) => {
                          const installedLabel = formatInstalledAt(
                            c.installed_at,
                          );
                          const sourceLabel = installedCollectionSourceLabel(c);
                          const isFiltering = modsCollectionFilter === c.id;
                          const syncState = friendSyncStates[c.id];
                          const hasUpdate = Boolean(syncState?.has_update);
                          const isSyncing = Boolean(syncingCollectionIds[c.id]);
                          const autoSyncOn = Boolean(c.friend_sync?.auto_sync);
                          const canUpdateSource = Boolean(
                            c.friend_sync ||
                              (c.source === "nexus" && c.slug) ||
                              (c.source === "thunderstore" &&
                                c.kind === "modpack" &&
                                c.community &&
                                c.namespace &&
                                c.package_name),
                          );
                          const installedMemberCount = mods.filter((m) =>
                            (m.collection_ids ?? []).includes(c.id),
                          ).length;
                          const modCount =
                            installedMemberCount > 0
                              ? installedMemberCount
                              : c.mod_ids.length;
                          const tooltip = [
                            c.name,
                            sourceLabel,
                            `${modCount} mods`,
                            c.friend_sync
                              ? hasUpdate
                                ? "Update available"
                                : "Synced"
                              : null,
                            installedLabel,
                          ]
                            .filter(Boolean)
                            .join(" · ");
                          return (
                            <article
                              key={c.id}
                              role="button"
                              tabIndex={0}
                              className={
                                isFiltering
                                  ? "installed-collection-card is-active"
                                  : "installed-collection-card"
                              }
                              title={tooltip}
                              onClick={(e) => {
                                if (
                                  (e.target as HTMLElement).closest("button")
                                ) {
                                  return;
                                }
                                void refreshSavedCollections();
                                setGameModal("collections");
                              }}
                              onKeyDown={(e) => {
                                if (
                                  e.target === e.currentTarget &&
                                  (e.key === "Enter" || e.key === " ")
                                ) {
                                  e.preventDefault();
                                  void refreshSavedCollections();
                                  setGameModal("collections");
                                }
                              }}
                            >
                              <div className="installed-collection-card-header">
                                <div className="installed-collection-card-title">
                                  {(c.source === "nexus" ||
                                    c.source === "thunderstore") && (
                                    <SourceMark
                                      source={c.source}
                                      label={sourceLabel}
                                    />
                                  )}
                                  <strong className="installed-collection-card-name">
                                    {c.name}
                                  </strong>
                                </div>
                                <div className="installed-collection-card-actions">
                                  {canUpdateSource && (
                                    <button
                                      type="button"
                                      className={hasUpdate ? "accent" : ""}
                                      disabled={isSyncing}
                                      title={
                                        c.friend_sync
                                          ? hasUpdate
                                            ? `Sync ${c.name} to friend's latest settings`
                                            : `Re-sync ${c.name} to latest settings`
                                          : `Update ${c.name} to latest version`
                                      }
                                      onClick={() =>
                                        void syncInstalledCollection(c)
                                      }
                                    >
                                      {isSyncing
                                        ? "Syncing…"
                                        : c.friend_sync
                                          ? hasUpdate
                                            ? "Sync Update"
                                            : "Sync"
                                          : "Update"}
                                    </button>
                                  )}
                                  {c.friend_sync && (
                                    <button
                                      type="button"
                                      className={autoSyncOn ? "active" : ""}
                                      aria-pressed={autoSyncOn}
                                      title={
                                        autoSyncOn
                                          ? "Auto-sync enabled (click to disable)"
                                          : "Enable auto-sync when friend updates this collection"
                                      }
                                      onClick={() =>
                                        void toggleInstalledCollectionAutoSync(
                                          c,
                                          !autoSyncOn,
                                        )
                                      }
                                    >
                                      Auto
                                    </button>
                                  )}
                                  <button
                                    type="button"
                                    className="danger icon-btn"
                                    aria-label={`Uninstall ${c.name}`}
                                    title={`Uninstall ${c.name}`}
                                    onClick={() =>
                                      setUninstallCollectionTarget(c)
                                    }
                                  >
                                    <FontAwesomeIcon icon={faTrashCan} />
                                  </button>
                                </div>
                              </div>
                              <div className="installed-collection-card-meta">
                                <span>
                                  {modCount} mod{modCount === 1 ? "" : "s"}
                                </span>
                                {c.friend_sync ? (
                                  <>
                                    {" · "}
                                    <span
                                      className={
                                        hasUpdate
                                          ? "installed-collection-sync-pill is-update"
                                          : "installed-collection-sync-pill"
                                      }
                                    >
                                      {hasUpdate ? "Update" : "Synced"}
                                    </span>
                                  </>
                                ) : null}
                                {collectionIntegrityById[c.id]?.issues
                                  .length ? (
                                  <>
                                    {" · "}
                                    <button
                                      type="button"
                                      className="installed-collection-issue-pill"
                                      onClick={() =>
                                        setIntegrityDialogReport(
                                          collectionIntegrityById[c.id],
                                        )
                                      }
                                    >
                                      {
                                        collectionIntegrityById[c.id].issues
                                          .length
                                      }{" "}
                                      issue
                                      {collectionIntegrityById[c.id].issues
                                        .length === 1
                                        ? ""
                                        : "s"}
                                    </button>
                                  </>
                                ) : null}
                              </div>
                            </article>
                          );
                        })}
                        </div>
                      ) : (
                        <div className="detail-collections-empty">
                          No collections installed
                        </div>
                      )}
                    </div>
                  </div>
                </div>
                </div>

                <div className="detail-body page-view">
                  <div className="page-chrome">
                    <Reveal
                      show={Boolean(
                        settings?.platform_linux &&
                          wineAdvice &&
                          !libraryDetail.game.wine_overrides_dismissed,
                      )}
                    >
                      {settings?.platform_linux &&
                      wineAdvice &&
                      !libraryDetail.game.wine_overrides_dismissed ? (
                        <WineOverrideNotice
                          advice={wineAdvice}
                          busy={wineApplyBusy}
                          onApply={() => void applyWineOverrides()}
                          onDismiss={() => void dismissWineOverrides()}
                        />
                      ) : null}
                    </Reveal>
                    <div className="mods-toolbar">
                      <div className="mods-toolbar-actions">
                        <div className="mods-toolbar-actions-left">
                          <button
                            type="button"
                            className="mods-toolbar-icon-btn"
                            aria-label="Import mod"
                            title="Import mod"
                            onClick={() => setImportModalOpen(true)}
                          >
                            <FontAwesomeIcon icon={faFileImport} />
                          </button>
                          <button
                            type="button"
                            disabled={checkingModUpdates}
                            onClick={() => void checkStagedModUpdatesNow()}
                          >
                            Check for updates
                          </button>
                          {stagedUpdateCount > 0 && (
                            <button
                              type="button"
                              className="accent"
                              onClick={() => void updateAllStagedMods()}
                            >
                              Update all ({stagedUpdateCount})
                            </button>
                          )}
                        </div>
                        <div className="mods-toolbar-actions-right">
                          <button
                            type="button"
                            className={
                              modsFiltersActive
                                ? "mods-toolbar-icon-btn mods-filters-toggle active"
                                : "mods-toolbar-icon-btn mods-filters-toggle"
                            }
                            aria-label="Filters"
                            aria-expanded={modsFiltersOpen}
                            aria-pressed={modsFiltersActive}
                            onClick={() => setModsFiltersOpen((open) => !open)}
                          >
                            <FontAwesomeIcon icon={faFilter} />
                          </button>
                        </div>
                      </div>
                      <Collapse show={modsFiltersOpen}>
                        <div className="filters">
                          <div className="row">
                            <input
                              placeholder="Search staged mods…"
                              value={modsQuery}
                              onChange={(e) => setModsQuery(e.target.value)}
                              aria-label="Search staged mods"
                            />
                          </div>
                          <label>
                            Game version
                            <input
                              value={gameVersionDraft}
                              placeholder={
                                gameVersionReport ? "Unknown" : "Checking…"
                              }
                              aria-label="Game version"
                              onChange={(e) =>
                                setGameVersionDraft(e.target.value)
                              }
                              onBlur={() =>
                                void commitGameVersion(gameVersionDraft)
                              }
                              onKeyDown={(e) => {
                                if (e.key === "Enter") {
                                  e.currentTarget.blur();
                                }
                              }}
                            />
                          </label>
                          {gameVersionReport?.source === "override" && (
                            <button
                              type="button"
                              className="filters-clear"
                              onClick={() => void commitGameVersion("")}
                            >
                              Use detected
                            </button>
                          )}
                          <label>
                            Status
                            <select
                              value={modsStatusFilter}
                              onChange={(e) =>
                                setModsStatusFilter(
                                  e.target.value as ModsStatusFilter,
                                )
                              }
                            >
                              <option value="all">All</option>
                              <option value="enabled">Enabled</option>
                              <option value="disabled">Disabled</option>
                            </select>
                          </label>
                          <label>
                            Sort
                            <select
                              value={modsSort}
                              onChange={(e) =>
                                setModsSort(e.target.value as ModsSort)
                              }
                            >
                              {STAGED_MOD_SORTS.map((o) => (
                                <option key={o.value} value={o.value}>
                                  {o.label}
                                </option>
                              ))}
                            </select>
                          </label>
                          <label>
                            Collection
                            <select
                              value={modsCollectionFilter}
                              onChange={(e) =>
                                setModsCollectionFilter(e.target.value)
                              }
                            >
                              <option value="">All</option>
                              <option value={MODS_COLLECTION_FILTER_NONE}>
                                None
                              </option>
                              {installedCollections.map((c) => (
                                <option key={c.id} value={c.id}>
                                  {c.name}
                                </option>
                              ))}
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
                                layoutId="mods-source"
                                active={modsSourceFilter === id}
                                aria-label={label}
                                title={label}
                                onClick={() => setModsSourceFilter(id)}
                              >
                                {id === "all" ? (
                                  label
                                ) : (
                                  <SourceMark source={id} decorative />
                                )}
                              </SegmentTab>
                            ))}
                          </Segment>
                          {modsFiltersActive && (
                            <button
                              type="button"
                              className="filters-clear"
                              onClick={() => {
                                setModsQuery("");
                                setModsStatusFilter("all");
                                setModsSourceFilter("all");
                                setModsSort("loadOrder");
                                setModsCollectionFilter("");
                              }}
                            >
                              Clear filters
                            </button>
                          )}
                        </div>
                      </Collapse>
                      {!canReorderMods && mods.length > 0 && (
                        <p className="note mods-reorder-hint">
                          Clear filters and use Load order to reorder by dragging or ↑↓.
                        </p>
                      )}
                      {modsFiltersActive && mods.length > 0 && (
                        <p className="note">
                          Showing {visibleMods.length} of {mods.length}
                        </p>
                      )}
                      {gameVersionReport && !gameVersionReport.current && (
                        <p className="note">
                          Game version is unknown. Set it to check which mods support this install.
                        </p>
                      )}
                    </div>
                    {ignoredRequirements.length > 0 && (
                      <button
                        type="button"
                        className="linkish ignored-requirements-link"
                        onClick={() => setIgnoredRequirementsOpen(true)}
                      >
                        Ignored requirements ({ignoredRequirements.length})
                      </button>
                    )}
                    <Reveal
                      show={Boolean(
                        (visibleMissingRequirements &&
                          requirementsNeedAttention(visibleMissingRequirements)) ||
                          visibleDisabledRequirements.length > 0 ||
                          versionWarnings.length > 0,
                      )}
                    >
                      {(visibleMissingRequirements &&
                        requirementsNeedAttention(visibleMissingRequirements)) ||
                      visibleDisabledRequirements.length > 0 ||
                      versionWarnings.length > 0 ? (
                        <MissingRequirementsStrip
                          requirements={
                            visibleMissingRequirements ?? {
                              nexus: [],
                              external: [],
                              dlc: [],
                            }
                          }
                          disabled={visibleDisabledRequirements}
                          versions={versionWarnings}
                          onReview={() => openRequirementsReview()}
                        />
                      ) : null}
                    </Reveal>
                    <FomodConfigureBanner
                      mods={fomodNeeding}
                      onConfigure={configureFomods}
                    />
                  </div>
                  <div className="page-scroll" ref={libraryDetailScrollRef}>
                    {showMods ? (
                      <div className="results-fill">
                    {mods.length === 0 ? (
                      <ul className="list mods">
                        <li className="empty">No staged mods. Browse mods to install some.</li>
                      </ul>
                    ) : visibleMods.length === 0 ? (
                      <ul className="list mods">
                        <li className="empty">No mods match your search/filters.</li>
                      </ul>
                    ) : (
                    <BrowseVirtualList
                      items={modDrag.displayMods}
                      scrollRef={libraryDetailScrollRef}
                      active={tab === "library"}
                      staggerEnter={!modDrag.isDragging}
                      staggerKey={activeGame?.id}
                      listRef={modDrag.listRef}
                      listClassName={
                        modDrag.isDragging ? "mods mod-list-dragging" : "mods"
                      }
                      estimateSize={estimateStagedModRowHeight()}
                      getKey={(m) => m.id}
                      onRowPointerDown={(m, event) =>
                        modDrag.onDragPointerDown(m.id, event)
                      }
                      rowClassName={(m) =>
                        [
                          m.enabled
                            ? "list-row-poster"
                            : "mod-disabled list-row-poster",
                          canReorderMods ? "mod-drag-ready" : "",
                          modDrag.draggingId === m.id ? "mod-drag-placeholder" : "",
                        ]
                          .filter(Boolean)
                          .join(" ")
                      }
                      renderItem={(m) => {
                        const idx = modIndexById.get(m.id) ?? -1;
                        const canMoveUp =
                          canReorderMods &&
                          idx >= 0 &&
                          sameEnabledNeighborIndex(modsInLoadOrder, idx, -1) >= 0;
                        const canMoveDown =
                          canReorderMods &&
                          idx >= 0 &&
                          sameEnabledNeighborIndex(modsInLoadOrder, idx, 1) >= 0;
                        const pageHit = activeGame
                          ? catalogHitFromStaged(m, activeGame)
                          : null;
                        const versionWarning = versionWarningByModId.get(m.id);
                        const modCollections = collectionsByModId.get(m.id) ?? [];
                        const isDragged = modDrag.draggingId === m.id;
                        const identity = (
                          <>
                            <ListPoster src={mediaSrc(null, m.picture_url)} />
                            <div className="mod-identity-text">
                              <strong>{m.name}</strong>
                              <small>{stagedModSourceLine(m)}</small>
                              {modCollections.length > 0 && (
                                <div className="mod-collection-chips">
                                  {modCollections.map((c) => (
                                    <button
                                      key={c.id}
                                      type="button"
                                      className={
                                        modsCollectionFilter === c.id
                                          ? "mod-collection-chip active"
                                          : "mod-collection-chip"
                                      }
                                      title={`Filter by ${c.name}`}
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        setModsCollectionFilter((prev) =>
                                          prev === c.id ? "" : c.id,
                                        );
                                      }}
                                    >
                                      {c.name}
                                    </button>
                                  ))}
                                </div>
                              )}
                              {versionWarning && (
                                <small className="mod-version-warn">
                                  {versionWarning.message}
                                </small>
                              )}
                            </div>
                          </>
                        );
                        const rowBody = (
                          <>
                            <label className="check">
                              <input
                                type="checkbox"
                                checked={m.enabled}
                                aria-label={`Enable ${m.name}`}
                                onChange={() => toggleMod(m)}
                              />
                            </label>
                            {pageHit ? (
                              <div
                                className="mod-identity staged-mod-page"
                                role="button"
                                tabIndex={0}
                                title={
                                  canReorderMods
                                    ? "Click and hold to drag in load order"
                                    : undefined
                                }
                                onClick={() => {
                                  if (modDrag.suppressClick) return;
                                  void openStagedMod(m);
                                }}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter" || e.key === " ") {
                                    e.preventDefault();
                                    void openStagedMod(m);
                                  }
                                }}
                              >
                                {identity}
                              </div>
                            ) : (
                              <div
                                className="mod-identity"
                                title={
                                  canReorderMods
                                    ? "Click and hold to drag in load order"
                                    : undefined
                                }
                              >{identity}</div>
                            )}
                            <div className="actions">
                              {modUpdates[m.id] && (
                                <button
                                  type="button"
                                  className="accent"
                                  title={
                                    modUpdates[m.id].available_version
                                      ? `Update to v${modUpdates[m.id].available_version}`
                                      : "Update available"
                                  }
                                  onClick={() => void updateOneStagedMod(m.id)}
                                >
                                  {modUpdates[m.id].available_version
                                    ? `Update to v${modUpdates[m.id].available_version}`
                                    : "Update"}
                                </button>
                              )}
                              {modsWithOptions.has(m.id) && (
                                <button
                                  type="button"
                                  className="icon-btn"
                                  onClick={() => void openModInstaller(m)}
                                  aria-label="Mod options"
                                  title="Choose which parts of this mod install"
                                >
                                  <FontAwesomeIcon icon={faGear} />
                                </button>
                              )}
                              {installedCollections.length > 1 && (
                                <button
                                  type="button"
                                  className="icon-btn"
                                  onClick={() => setMoveCollectionMod(m)}
                                  aria-label={`Move ${m.name} between collections`}
                                  title="Move between collections"
                                >
                                  <FontAwesomeIcon icon={faRightLeft} />
                                </button>
                              )}
                              <button
                                type="button"
                                className="icon-btn"
                                onClick={() => moveMod(m.id, -1)}
                                disabled={!canMoveUp}
                                aria-label="Move up in load order"
                                title={
                                  canReorderMods
                                    ? "Move up in load order"
                                    : "Clear filters and use Load order to reorder"
                                }
                              >
                                <FontAwesomeIcon icon={faArrowUp} />
                              </button>
                              <button
                                type="button"
                                className="icon-btn"
                                onClick={() => moveMod(m.id, 1)}
                                disabled={!canMoveDown}
                                aria-label="Move down in load order"
                                title={
                                  canReorderMods
                                    ? "Move down in load order"
                                    : "Clear filters and use Load order to reorder"
                                }
                              >
                                <FontAwesomeIcon icon={faArrowDown} />
                              </button>
                              <button
                                type="button"
                                className="danger icon-btn"
                                aria-label={`Remove ${m.name}`}
                                title="Remove"
                                onClick={() =>
                                  withBusy("Removing mod…", async () => {
                                    if (!activeGame) return;
                                    await api.removeMod(activeGame.id, m.id);
                                    await refreshMods(activeGame.id);
                                    await refreshCollections(activeGame.id);
                                    setModUpdates((prev) => {
                                      const next = { ...prev };
                                      delete next[m.id];
                                      return next;
                                    });
                                  })
                                }
                              >
                                <FontAwesomeIcon icon={faTrashCan} />
                              </button>
                            </div>
                          </>
                        );
                        if (isDragged && modDrag.lift) {
                          const lift = modDrag.lift;
                          return (
                            <>
                              <div style={{ visibility: "hidden" }} aria-hidden="true">
                                {rowBody}
                              </div>
                              {createPortal(
                                <div
                                  className="mod-drag-clone"
                                  style={{
                                    position: "fixed",
                                    left: lift.x,
                                    top: lift.y,
                                    width: lift.width,
                                    height: lift.height,
                                    zIndex: 40,
                                    pointerEvents: "none",
                                    transform: reduceMotion
                                      ? undefined
                                      : "scale(1.02)",
                                  }}
                                >
                                  {rowBody}
                                </div>,
                                document.body,
                              )}
                            </>
                          );
                        }
                        return rowBody;
                      }}
                    />
                    )}
                      </div>
                    ) : (
                      <StagedModsSkeleton />
                    )}
                  </div>
                </div>
              </>
            ) : (
              <div className="page-view">
                <div className="page-chrome">
                <div className="panel-head">
                  <h1>Library</h1>
                  <div className="panel-head-actions">
                    <button onClick={scan}>Scan games</button>
                  </div>
                </div>
                {relinkPrompt && (
                  <div className="panel relink-panel">
                    <h3>Relink {relinkPrompt.game.title}</h3>
                    <p className="note">
                      The install folder moved. Confirming will update the managed
                      entry and migrate staged mod data to the new game ID.
                    </p>
                    <dl className="detail-dl">
                      <dt>Old path</dt>
                      <dd>{relinkPrompt.game.install_path}</dd>
                      <dt>New path</dt>
                      <dd>{relinkPrompt.health.new_install_path ?? "—"}</dd>
                    </dl>
                    <div className="row-actions">
                      <button type="button" onClick={() => void confirmRelink()}>
                        Relink
                      </button>
                      <button type="button" onClick={() => setRelinkPrompt(null)}>
                        Cancel
                      </button>
                    </div>
                  </div>
                )}
                </div>
                <div className="page-scroll" ref={libraryHomeScrollRef}>
                <ResultsReveal
                  show={showLibraryHome}
                  skeleton={
                    <>
                      <h2>Managed</h2>
                      <BrowseGridSkeleton count={6} minCardPx={160} />
                      <h2>Unmanaged</h2>
                      <BrowseGridSkeleton count={8} minCardPx={160} />
                    </>
                  }
                >
                {showLibraryHome ? (
                <>
                {managed.length > 0 && (
                  <>
                    <h2>Managed</h2>
                    <div className="library-posters">
                      <BrowseVirtualGrid
                        items={managed}
                        scrollRef={libraryHomeScrollRef}
                        minCardPx={160}
                        fixedRowHeight={!managedHasActions}
                        hasMeta={false}
                        active={tab === "library"}
                        staggerEnter
                        trackScrollMargin
                        getKey={getLibraryGameKey}
                        renderItem={renderLibraryManagedItem}
                        renderSkeletonItem={renderLibraryGridSkeletonCard}
                      />
                    </div>
                  </>
                )}
                <h2>Unmanaged</h2>
                {catalogLookupBusy && unmanaged.length > 0 && (
                  <p className="note">Looking up Nexus / Thunderstore / mod.io…</p>
                )}
                <div className="library-posters">
                    {unmanaged.length === 0 ? (
                      <p className="note">
                        No unmanaged games found. Scan again if you installed something
                        while the app was open.
                      </p>
                    ) : (
                    <BrowseVirtualGrid
                      items={unmanaged}
                      scrollRef={libraryHomeScrollRef}
                      minCardPx={160}
                      fixedRowHeight
                      hasMeta={false}
                      active={tab === "library"}
                      staggerEnter
                      trackScrollMargin
                      getKey={getLibraryGameKey}
                      renderItem={renderLibraryUnmanagedItem}
                      renderSkeletonItem={renderLibraryGridSkeletonCard}
                    />
                    )}
                </div>
                </>
                ) : null}
                </ResultsReveal>
                </div>
              </div>
            )}
            </ViewSwitch>
          </section>
          </ParkedTabGate>
        </div>
        ) : null}

        {collectionsKeptRef.current ? (
        <div
          className={tab === "collections" ? "tab-kept" : "tab-kept tab-parked"}
          inert={tab !== "collections"}
          aria-hidden={tab !== "collections" ? true : undefined}
        >
          <ParkedTabGate active={tab === "collections"}>
          <section className="panel page-panel">
            <ViewSwitch
              viewKey={collectionsViewKey}
              className="page-view"
              initial={false}
              onEntered={setCollectionsViewSettled}
            >
            {savedDetail ? (() => {
              const matchedGroup = savedCollectionGameGroups.find((g) =>
                g.collections.some((c) => c.id === savedDetail.entry.id),
              );
              const coverSrc = posterSrc(matchedGroup?.coverPath ?? null);
              const gameTitle =
                matchedGroup?.title ??
                savedDetail.entry.game.title ??
                "Saved collection";
              return (
                <CollectionDetailView
                  title={savedDetail.entry.name}
                  onRename={(nextName) =>
                    renameSavedCollectionById(savedDetail.entry.id, nextName)
                  }
                  backLabel="Home"
                  onBack={() => {
                    savedOpenRef.current += 1;
                    setSavedDetail(null);
                    setSavedDetailPending(false);
                  }}
                  headerActions={
                    <button
                      type="button"
                      className="danger"
                      aria-label={`Delete ${savedDetail.entry.name}`}
                      onClick={() =>
                        void (async () => {
                          if (!window.confirm(`Delete ${savedDetail.entry.name}?`)) {
                            return;
                          }
                          try {
                            await deleteSavedCollectionEntry(savedDetail.entry);
                            savedOpenRef.current += 1;
                            setSavedDetail(null);
                            setSavedDetailPending(false);
                          } catch (e) {
                            setError(String(e));
                          }
                        })()
                      }
                    >
                      Delete
                    </button>
                  }
                  coverSrc={coverSrc}
                  gameTitle={gameTitle}
                  metaPills={
                    isSavedCollectionInstalled(savedDetail.entry) ? (
                      <>
                        {" "}
                        <span className="pill ok">Installed</span>
                      </>
                    ) : null
                  }
                  stats={savedDetail.entry}
                  versionWarning={
                    showGameVersionWarnings &&
                    savedCollectionAdvice[savedDetail.entry.id]?.status === "mismatch"
                      ? savedCollectionAdvice[savedDetail.entry.id]?.message
                      : null
                  }
                  managed={managed}
                  selectedGameId={savedInstallGameId}
                  onSelectGameId={setSavedInstallGameId}
                  gameMatches={(g) =>
                    savedCollectionMatchesGame(savedDetail.entry, g)
                  }
                  gameKnown={savedCollectionGameKey(savedDetail.entry) !== "unknown"}
                  actions={
                    <>
                      <button
                        type="button"
                        onClick={() => void installSavedCollection()}
                      >
                        Install
                      </button>
                      {(() => {
                        const vis = getSavedCollectionVisibility(
                          savedDetail.entry.id,
                        );
                        const label = formatVisibilityLabel(vis);
                        return (
                          <button
                            type="button"
                            className="collection-sharing-btn"
                            aria-label={`Sharing: ${label}`}
                            title={`Sharing: ${label}`}
                            onClick={() =>
                              openSavedCollectionSharing(savedDetail.entry)
                            }
                          >
                            <FontAwesomeIcon icon={visibilityIcon(vis)} />{" "}
                            {label}
                          </button>
                        );
                      })()}
                    </>
                  }
                  code={savedDetail.entry.code}
                  mods={savedDetail.mods}
                  showMods={showSavedMods}
                  skeletonCount={savedDetail.entry.mod_count}
                  scrollRef={mainScrollRef}
                  active={tab === "collections"}
                  onOpenMod={(hit) =>
                    void openCollectionMod(
                      hit,
                      "collections",
                      savedInstallGameId ||
                        managed.find((g) =>
                          savedCollectionMatchesGame(savedDetail.entry, g),
                        )?.id ||
                        null,
                    )
                  }
                />
              );
            })() : (
              <div className="page-view">
                <div className="page-chrome">
                <div className="panel-head">
                  <h1>Saved Collections</h1>
                  <div className="panel-head-actions">
                    <button
                      type="button"
                      className="mods-toolbar-icon-btn"
                      aria-label="Import collection"
                      title="Import collection"
                      onClick={() => {
                        setError(null);
                        setCollectionImportOpen(true);
                      }}
                    >
                      <FontAwesomeIcon icon={faPlus} />
                    </button>
                  </div>
                </div>
                <p className="note">
                  Local Emperor share codes grouped by game. Create one from Library →
                  game → Save loadout, or import a code or file here.
                </p>
                </div>
                <div className="page-scroll">
                {showCollectionsList ? (
                  <CollectionsGameList
                    groups={savedCollectionGameGroups}
                    expandedGameKey={expandedCollectionGameKey}
                    onToggleGameKey={(key) =>
                      setExpandedCollectionGameKey(
                        expandedCollectionGameKey === key ? null : key,
                      )
                    }
                    groupLimit={visibleCollectionGroupCount}
                    expandedGroupLimits={expandedGroupLimits}
                    onShowMoreChildren={(groupKey) =>
                      setExpandedGroupLimits((prev) => ({
                        ...prev,
                        [groupKey]: (prev[groupKey] ?? 25) + 25,
                      }))
                    }
                    renderPoster={(coverPath) => (
                      <ListPoster src={posterSrc(coverPath)} />
                    )}
                    emptyMessage="No saved share codes yet. Share a loadout from a managed game."
                    renderChild={(group, c, childIdx) => {
                      const builtFor = formatCollectionGameVersions(
                        c.game_versions,
                      );
                      const advice = savedCollectionAdvice[c.id];
                      const installed = isSavedCollectionInstalled(c);
                      return (
                        <li
                          key={c.id}
                          className="list-row-clickable stagger-item"
                          style={staggerStyle(childIdx)}
                          onClick={() => void openSavedCollection(c.id)}
                        >
                          <div className="list-row-main">
                            <strong>
                              {c.name}
                              {installed ? (
                                <span className="pill ok">Installed</span>
                              ) : null}
                            </strong>
                            <small>
                              {c.mod_count} mods
                              {builtFor ? ` · Built for ${builtFor}` : ""}
                              {formatIsoDate(c.created_at)
                                ? ` · ${formatIsoDate(c.created_at)}`
                                : ""}
                            </small>
                            {showGameVersionWarnings &&
                              advice?.status === "mismatch" && (
                                <small className="mod-version-warn">
                                  {advice.message}
                                </small>
                              )}
                          </div>
                          <div
                            className="actions"
                            onClick={(e) => e.stopPropagation()}
                          >
                            <button
                              type="button"
                              onClick={() =>
                                void (async () => {
                                  if (group.managed) {
                                    await runUnifiedImport(
                                      group.managed.id,
                                      c.code,
                                      c.name,
                                    );
                                    await refreshCollectionsPageMeta();
                                    return;
                                  }
                                  await openSavedCollection(c.id);
                                })()
                              }
                            >
                              Install
                            </button>
                            <button
                              type="button"
                              className="icon-btn"
                              aria-label={`Copy ${c.name}`}
                              title="Copy"
                              onClick={() =>
                                void navigator.clipboard.writeText(c.code)
                              }
                            >
                              <FontAwesomeIcon icon={faCopy} />
                            </button>
                            {(() => {
                              const vis = getSavedCollectionVisibility(c.id);
                              const label = formatVisibilityLabel(vis);
                              return (
                                <button
                                  type="button"
                                  className="collection-sharing-btn"
                                  aria-label={`Sharing for ${c.name}: ${label}`}
                                  title={`Sharing: ${label}`}
                                  onClick={() => openSavedCollectionSharing(c)}
                                >
                                  <FontAwesomeIcon icon={visibilityIcon(vis)} />{" "}
                                  {label}
                                </button>
                              );
                            })()}
                            <button
                              type="button"
                              className="danger icon-btn"
                              aria-label={`Delete ${c.name}`}
                              title="Delete"
                              onClick={() =>
                                void (async () => {
                                  if (!window.confirm(`Delete ${c.name}?`)) {
                                    return;
                                  }
                                  try {
                                    await deleteSavedCollectionEntry(c);
                                  } catch (err) {
                                    setError(String(err));
                                  }
                                })()
                              }
                            >
                              <FontAwesomeIcon icon={faTrashCan} />
                            </button>
                          </div>
                        </li>
                      );
                    }}
                  />
                ) : (
                  <BrowseListSkeleton count={6} posters />
                )}
                </div>
              </div>
            )}
            </ViewSwitch>
          </section>
          </ParkedTabGate>
        </div>
        ) : null}

        {tab === "setup" ? (
        <ViewSwitch viewKey={tab} className="view-slot">
        {tab === "setup" && (
          <section className="panel page-panel">
            <div className="page-chrome">
            <div className="panel-head">
              <h1>Setup</h1>
            </div>
            </div>
            <div className="page-scroll">
            <p>
              Sign in with your Nexus Mods account (use <strong>Stay signed in</strong>)
              to enable downloads. Your API key is retrieved automatically in the
              background after signing in. mod.io is optional.
            </p>
            <div className="status-card">
              <span
                className={settings?.nexus_website_signed_in ? "pill ok" : "pill warn"}
              >
                {settings?.nexus_website_signed_in
                  ? "Website sign-in saved"
                  : "Website sign-in required"}
              </span>
              <span className={settings?.has_api_key ? "pill ok" : "pill warn"}>
                {settings?.has_api_key ? "Nexus API key saved" : "API key pending"}
              </span>
            </div>
            {accountConnections}
            <div className="actions">
              <button
                type="button"
                disabled={!canFinishSetup}
                title={
                  canFinishSetup
                    ? undefined
                    : "Sign in to Nexus (or save a personal API key) first."
                }
                onClick={() => void finishSetup()}
              >
                Finish setup
              </button>
            </div>
            </div>
          </section>
        )}
        </ViewSwitch>
        ) : null}

        {friendsKeptRef.current ? (
        <div
          className={tab === "friends" ? "tab-kept" : "tab-kept tab-parked"}
          inert={tab !== "friends"}
          aria-hidden={tab !== "friends" ? true : undefined}
        >
          <ParkedTabGate active={tab === "friends"}>
            <FriendsPage
              active={tab === "friends"}
              pendingJoinCode={pendingFriendInviteCode}
              onConsumePendingJoinCode={() => setPendingFriendInviteCode(null)}
              managed={managed}
              detected={detected}
              plugins={gamePlugins}
              catalogCovers={collectionCatalogCovers}
              catalogTitles={collectionCatalogTitles}
              activeGameId={activeGame?.id ?? null}
              scrollRef={mainScrollRef}
              installedShareIds={installedEmperorShareIds}
              resolveGameCover={(game) => gameCoverSrc(game)}
              renderPoster={(coverPath) => (
                <ListPoster src={posterSrc(coverPath)} />
              )}
              onError={setError}
              onNotice={(msg) =>
                setNotice(msg ? { kind: "ok", message: msg } : null)
              }
              onInstallCollection={async (gameId, code, name) => {
                await runUnifiedImport(gameId, code, name);
              }}
              onSaveCollection={async (code, name, sourceGameId) => {
                await api.saveCollectionCode({ code, name, sourceGameId });
                await refreshSavedCollections();
                setNotice({
                  kind: "ok",
                  message: `Saved ${name} to Collections.`,
                });
              }}
              onCollectionSynced={async (gameId, res, friendSync) => {
                await handleShareImportResult(
                  gameId,
                  res.share_code,
                  {
                    collection_id: res.collection_id,
                    name: res.name,
                    member_ids: res.member_ids,
                    needs_assist: res.needs_assist,
                    warnings: res.warnings,
                    collection: res.collection ?? null,
                  },
                  friendSync ?? null,
                );
              }}
              onOpenMod={(hit, targetGameId) =>
                void openCollectionMod(hit, "friends", targetGameId)
              }
            />
          </ParkedTabGate>
        </div>
        ) : null}

        {browseKeptRef.current ? (
        <div
          className={
            tab === "browse"
              ? browseDetail == null
                ? "tab-kept tab-kept-browse"
                : "tab-kept"
              : "tab-kept tab-parked"
          }
          inert={tab !== "browse"}
          aria-hidden={tab !== "browse" ? true : undefined}
        >
          <ParkedTabGate active={tab === "browse"}>
          <ViewSwitch
            onEntered={setBrowseViewSettled}
            viewKey={
              browseDetail
                ? browseDetail.kind === "mod"
                  ? `mod:${browseDetail.hit.id}`
                  : `collection:${browseDetail.hit.slug}`
                : "browse-results"
            }
            className={browseDetail ? "panel page-panel" : "panel browse-workspace"}
            initial={false}
          >
            {browseDetail ? (
              <>
                <div className="page-chrome">
                <div className="panel-head">
                  <div className="detail-head-title">
                    <button type="button" onClick={() => closeBrowseDetail()}>
                      Home
                    </button>
                    <h1>
                      {browseDetail.kind === "mod"
                        ? (presentName(modDetail?.name) ??
                          presentName(tsDetail?.name) ??
                          presentName(modioDetail?.name) ??
                          browseDetail.hit.name)
                        : browseDetail.hit.name}
                    </h1>
                  </div>
                  <Segment>
                    <SegmentTab
                      layoutId="browse-detail-tabs"
                      active={browseDetail.tab === "info"}
                      onClick={() => setDetailTab("info")}
                    >
                      Info
                    </SegmentTab>
                    {!(
                      browseDetail.kind === "mod" &&
                      browseDetail.hit.source === "thunderstore"
                    ) && (
                      <SegmentTab
                        layoutId="browse-detail-tabs"
                        active={browseDetail.tab === "files"}
                        onClick={() => setDetailTab("files")}
                      >
                        Files
                      </SegmentTab>
                    )}
                  </Segment>
                </div>
                </div>

                <ViewSwitch
                  viewKey={`${browseDetail.kind}:${browseDetail.tab}:${
                    browseDetail.kind === "mod" ? browseDetail.hit.id : browseDetail.hit.slug
                  }`}
                  className="page-view"
                  initial={false}
                >
                {browseDetail.kind === "mod" ? (
                  <>
                    <div className="page-chrome">
                    <div className="detail-hero">
                      <div className="detail-cover">
                        {mediaSrc(
                          null,
                          (browseDetail.hit.source === "modio"
                            ? modioDetail?.picture_url
                            : modDetail?.picture_url) ?? browseDetail.hit.picture_url,
                        ) ? (
                          <img
                            src={
                              mediaSrc(
                                null,
                                (browseDetail.hit.source === "modio"
                                  ? modioDetail?.picture_url
                                  : modDetail?.picture_url) ??
                                  browseDetail.hit.picture_url,
                              )!
                            }
                            alt=""
                          />
                        ) : (
                          <PlaceholderIcon />
                        )}
                      </div>
                      <div className="detail-meta">
                        <p className="detail-author">
                          {(() => {
                            const primaryAuthor =
                              (browseDetail.hit.source === "modio"
                                ? modioDetail?.author
                                : browseDetail.hit.source === "thunderstore"
                                  ? (tsDetail?.namespace ??
                                    browseDetail.hit.author)
                                  : (modDetail?.author ??
                                    browseDetail.hit.author)) ?? null;
                            const uploader =
                              browseDetail.hit.source === "nexus"
                                ? (modDetail?.uploaded_by ?? null)
                                : null;
                            const showUploader =
                              Boolean(uploader) && uploader !== primaryAuthor;
                            // Nexus browse filters by uploader account name.
                            const primaryBrowseName =
                              browseDetail.hit.source === "nexus"
                                ? (uploader ?? primaryAuthor)
                                : primaryAuthor;
                            return (
                              <>
                                {primaryAuthor && primaryBrowseName ? (
                                  <button
                                    type="button"
                                    className="detail-author-link"
                                    title={`Browse mods by ${primaryBrowseName}`}
                                    onClick={() =>
                                      browseByAuthor(
                                        primaryBrowseName,
                                        browseDetail.hit.source,
                                      )
                                    }
                                  >
                                    {primaryAuthor}
                                  </button>
                                ) : (
                                  "Unknown author"
                                )}
                                {showUploader && uploader ? (
                                  <span className="detail-author-secondary">
                                    {" "}
                                    · uploaded by{" "}
                                    <button
                                      type="button"
                                      className="detail-author-link"
                                      title={`Browse mods by ${uploader}`}
                                      onClick={() =>
                                        browseByAuthor(
                                          uploader,
                                          browseDetail.hit.source,
                                        )
                                      }
                                    >
                                      {uploader}
                                    </button>
                                  </span>
                                ) : null}
                              </>
                            );
                          })()}
                        </p>
                        <div className="detail-stats">
                          {browseDetail.hit.source !== "modio" &&
                            (modDetail?.endorsements ?? browseDetail.hit.endorsements) !=
                              null && (
                            <span>
                              {(
                                modDetail?.endorsements ?? browseDetail.hit.endorsements
                              )!.toLocaleString()}{" "}
                              endorsements
                            </span>
                          )}
                          {(
                            (browseDetail.hit.source === "modio"
                              ? modioDetail?.downloads
                              : modDetail?.downloads) ?? browseDetail.hit.downloads
                          ) != null && (
                            <span>
                              {(
                                (browseDetail.hit.source === "modio"
                                  ? modioDetail?.downloads
                                  : modDetail?.downloads) ?? browseDetail.hit.downloads
                              )!.toLocaleString()}{" "}
                              downloads
                            </span>
                          )}
                          {modDetail?.version && browseDetail.hit.source !== "modio" && (
                            <span>v{modDetail.version}</span>
                          )}
                          {(modDetail?.category ?? browseDetail.hit.category) &&
                            browseDetail.hit.source !== "modio" && (
                            <span>{modDetail?.category ?? browseDetail.hit.category}</span>
                          )}
                          {modDetail?.contains_adult_content && (
                            <span className="detail-badge-adult">Adult</span>
                          )}
                          {modDetail?.status &&
                            modDetail.status.toLowerCase() !== "published" &&
                            modDetail.status.toLowerCase() !== "normal" && (
                              <span className="detail-badge-status">{modDetail.status}</span>
                            )}
                        </div>
                        {(installedIds.has(browseDetail.hit.id) ||
                          (browseDetail.hit.tags && browseDetail.hit.tags.length > 0)) && (
                          <div className="media-card-tags">
                            {installedIds.has(browseDetail.hit.id) ? (
                              <span className="pill ok">Installed</span>
                            ) : null}
                            {browseDetail.hit.tags?.slice(0, 8).map((t) => (
                              <span key={t} className="pill">
                                {t}
                              </span>
                            ))}
                          </div>
                        )}
                        <dl className="meta detail-dates">
                          {formatTimestamp(modDetail?.created_timestamp) && (
                            <>
                              <dt>Created</dt>
                              <dd>{formatTimestamp(modDetail?.created_timestamp)}</dd>
                            </>
                          )}
                          {formatTimestamp(modDetail?.updated_timestamp) && (
                            <>
                              <dt>Updated</dt>
                              <dd>{formatTimestamp(modDetail?.updated_timestamp)}</dd>
                            </>
                          )}
                        </dl>
                        <div className="detail-mod-actions">
                          {installedIds.has(browseDetail.hit.id) ? (
                            <button
                              type="button"
                              className="danger"
                              onClick={() => void uninstallCatalogMod(browseDetail.hit)}
                            >
                              Uninstall
                            </button>
                          ) : null}
                          {modShareUrlFromHit(browseDetail.hit) ? (
                            <button
                              type="button"
                              className="mods-toolbar-icon-btn"
                              aria-label="Copy link"
                              title="Copy link"
                              onClick={() => void copyModDeepLink(browseDetail.hit)}
                            >
                              <FontAwesomeIcon icon={faLink} />
                            </button>
                          ) : null}
                          {browseDetail.hit.source === "thunderstore" ? (
                            <>
                              <button
                                type="button"
                                onClick={() =>
                                  openExternal(
                                    browseDetail.hit.package_url ||
                                      `https://thunderstore.io/c/${browseDetail.hit.community}/p/${browseDetail.hit.namespace}/${browseDetail.hit.package_name}/`,
                                  )
                                }
                              >
                                Open on Thunderstore
                              </button>
                              <select
                                className="detail-version"
                                aria-label="Version"
                                value={tsVersion}
                                onChange={(e) => setTsVersion(e.target.value)}
                              >
                                {(tsDetail?.versions ?? []).map((v) => (
                                  <option key={v.uuid4} value={v.version_number}>
                                    {v.version_number}
                                    {v.downloads
                                      ? ` · ${v.downloads.toLocaleString()} dl`
                                      : ""}
                                  </option>
                                ))}
                              </select>
                              <button
                                type="button"
                                onClick={() => void installThunderstorePackage()}
                              >
                                Install
                              </button>
                            </>
                          ) : browseDetail.hit.source === "modio" ? (
                            <>
                              <button
                                type="button"
                                onClick={() =>
                                  openExternal(
                                    browseDetail.hit.profile_url ||
                                      modioDetail?.profile_url ||
                                      `https://mod.io/g/${browseDetail.hit.modio_game_id}/m/${browseDetail.hit.modio_mod_id}`,
                                  )
                                }
                              >
                                Open on mod.io
                              </button>
                              <button
                                type="button"
                                onClick={() =>
                                  void installModioFile(
                                    modioDetail?.primary_file_id ?? null,
                                    null,
                                  )
                                }
                              >
                                Install primary
                              </button>
                            </>
                          ) : (
                            <button
                              type="button"
                              onClick={() =>
                                openExternal(
                                  `https://www.nexusmods.com/${
                                    modDetail?.domain_name ||
                                    browseDetail.hit.domain_name ||
                                    activeGame?.nexus_domain
                                  }/mods/${browseDetail.hit.mod_id}`,
                                )
                              }
                            >
                              Open on Nexus
                            </button>
                          )}
                          {browseDetail.hit.source !== "thunderstore" &&
                            browseDetail.hit.source !== "modio" &&
                            browseDetail.hit.mod_id != null && (
                              <button
                                type="button"
                                onClick={() =>
                                  void openModRequirements(
                                    modDetail?.domain_name ||
                                      browseDetail.hit.domain_name ||
                                      activeGame?.nexus_domain ||
                                      "",
                                    browseDetail.hit.mod_id!,
                                    browseDetail.hit.name,
                                  )
                                }
                              >
                                Requirements
                              </button>
                            )}
                          {modDetail?.uploaded_users_profile_url && (
                            <button
                              type="button"
                              onClick={() =>
                                openExternal(modDetail.uploaded_users_profile_url!)
                              }
                            >
                              Author profile
                            </button>
                          )}
                        </div>
                      </div>
                    </div>
                    </div>

                    {browseDetail.tab === "info" ? (
                      modDetailLoading && !modDetail && !tsDetail && !modioDetail ? (
                        <div className="page-scroll" ref={browseDetailScrollRef}>
                        <ModDetailSkeleton tab="info" />
                        </div>
                      ) : (
                      <div className="detail-body page-scroll" ref={browseDetailScrollRef}>
                        {browseDetail.hit.source !== "thunderstore" &&
                          browseDetail.hit.source !== "modio" && (
                            <NexusUnavailableCallout detail={modDetail} />
                          )}
                        {browseDetail.hit.source !== "thunderstore" &&
                          browseDetail.hit.source !== "modio" && (
                            <ImageCarousel slides={modDetail?.image_urls ?? []} />
                          )}
                        {(browseDetail.hit.source === "thunderstore" ||
                          browseDetail.hit.source === "modio" ||
                          !nexusModUnavailable(modDetail) ||
                          Boolean(
                            (
                              modDetail?.description ??
                              modDetail?.summary ??
                              browseDetail.hit.summary
                            )?.trim(),
                          )) && (
                          <>
                            <h2>About</h2>
                            <RichText
                              className="detail-description detail-description-html"
                              text={
                                browseDetail.hit.source === "thunderstore"
                                  ? (tsDetail?.description ??
                                    browseDetail.hit.summary)
                                  : browseDetail.hit.source === "modio"
                                    ? (modioDetail?.description ??
                                      modioDetail?.summary ??
                                      browseDetail.hit.summary)
                                    : (modDetail?.description ??
                                      modDetail?.summary ??
                                      browseDetail.hit.summary)
                              }
                            />
                          </>
                        )}
                        {browseDetail.hit.source === "modio" &&
                          modioDetail?.has_dependencies && (
                          <p className="note">
                            This mod lists dependencies on mod.io; missing ones are
                            installed automatically before the primary file.
                          </p>
                        )}
                        {browseDetail.hit.source === "thunderstore" &&
                          tsDetail?.versions?.[0]?.dependencies?.length ? (
                          <>
                            <h2>Dependencies</h2>
                            <ul className="list">
                              {(
                                tsDetail.versions.find(
                                  (v) => v.version_number === tsVersion,
                                ) ?? tsDetail.versions[0]
                              ).dependencies.map((d) => (
                                <li key={d}>
                                  <strong>{d}</strong>
                                </li>
                              ))}
                            </ul>
                            <p className="note">
                              Missing dependencies are installed automatically with the
                              package.
                            </p>
                          </>
                        ) : null}
                      </div>
                      )
                    ) : modFilesLoading &&
                      modFiles.length === 0 &&
                      modioFiles.length === 0 ? (
                      <div className="page-scroll" ref={browseDetailScrollRef}>
                      <ModDetailSkeleton tab="files" />
                      </div>
                    ) : (
                      <div className="detail-body page-scroll" ref={browseDetailScrollRef}>
                        <h2>Files</h2>
                        {browseDetail.hit.source === "modio" ? (
                          <ul className="list">
                            {modioFiles.map((f) => (
                              <li key={f.file_id}>
                                <div>
                                  <strong>
                                    {f.filename}
                                    {detailInstalledFileIds.has(f.file_id) ? (
                                      <span className="pill ok">Installed</span>
                                    ) : null}
                                  </strong>
                                  <small>
                                    {f.version ? `v${f.version}` : "file"}
                                    {f.is_primary ? " · primary" : ""}
                                    {f.filesize
                                      ? ` · ${(f.filesize / (1024 * 1024)).toFixed(1)} MB`
                                      : ""}
                                  </small>
                                </div>
                                <button
                                  onClick={() =>
                                    void installModioFile(f.file_id, f.version)
                                  }
                                >
                                  Install
                                </button>
                              </li>
                            ))}
                            {modioFiles.length === 0 && (
                              <li className="empty">No files found for this mod.</li>
                            )}
                          </ul>
                        ) : nexusModUnavailable(modDetail) ? (
                          <NexusUnavailableCallout detail={modDetail} files />
                        ) : (
                          <NexusModFileList
                            files={modFiles}
                            installLabel={isPremium ? "Install" : "Download Assist"}
                            installedFileIds={detailInstalledFileIds}
                            onInstall={(file) => void installFile(file)}
                          />
                        )}
                      </div>
                    )}
                  </>
                ) : (
                  <>
                    <div className="page-chrome">
                    <div className="detail-hero">
                      <div className="detail-cover">
                        {mediaSrc(null, browseDetail.hit.tile_image_url) ? (
                          <img
                            src={mediaSrc(null, browseDetail.hit.tile_image_url)!}
                            alt=""
                          />
                        ) : (
                          <PlaceholderIcon />
                        )}
                      </div>
                      <div className="detail-meta">
                        <p className="detail-author">
                          {browseDetail.hit.author ?? "Collection"}
                        </p>
                        <div className="detail-stats">
                          {browseDetail.hit.endorsements != null && (
                            <span>
                              {browseDetail.hit.endorsements.toLocaleString()} endorsements
                            </span>
                          )}
                          {browseDetail.hit.total_downloads != null && (
                            <span>
                              {browseDetail.hit.total_downloads.toLocaleString()} downloads
                            </span>
                          )}
                          {browseDetail.hit.mod_count != null && (
                            <span>
                              {browseDetail.hit.mod_count.toLocaleString()} mods
                            </span>
                          )}
                          {collectionModFiles.length > 0 && (
                            <span>
                              {collectionModFiles.length.toLocaleString()} files
                            </span>
                          )}
                          {browseDetail.hit.file_size != null &&
                            browseDetail.hit.file_size > 0 && (
                              <span>{formatBytes(browseDetail.hit.file_size)}</span>
                            )}
                          {browseDetail.hit.overall_rating != null && (
                            <span>
                              {browseDetail.hit.overall_rating.toFixed(1)}
                              {browseDetail.hit.overall_rating_count != null
                                ? ` · ${browseDetail.hit.overall_rating_count.toLocaleString()} ratings`
                                : ""}
                            </span>
                          )}
                          {browseDetail.hit.revision_number != null && (
                            <span>rev {browseDetail.hit.revision_number}</span>
                          )}
                          {browseDetail.hit.category && (
                            <span>{browseDetail.hit.category}</span>
                          )}
                          {browseDetail.hit.domain_name && (
                            <span>{browseDetail.hit.domain_name}</span>
                          )}
                        </div>
                        <dl className="meta detail-dates">
                          {formatIsoDate(browseDetail.hit.created_at) && (
                            <>
                              <dt>Created</dt>
                              <dd>{formatIsoDate(browseDetail.hit.created_at)}</dd>
                            </>
                          )}
                          {formatIsoDate(browseDetail.hit.updated_at) && (
                            <>
                              <dt>Updated</dt>
                              <dd>{formatIsoDate(browseDetail.hit.updated_at)}</dd>
                            </>
                          )}
                        </dl>
                        <div className="actions">
                          <button onClick={() => installCollection(browseDetail.hit)}>
                            {collectionHitSource(browseDetail.hit) === "thunderstore" ||
                            isPremium
                              ? "Install"
                              : "Download Assist"}
                          </button>
                        </div>
                        {collectionHitSource(browseDetail.hit) !== "thunderstore" && (
                          <label className="inline">
                            <input
                              type="checkbox"
                              checked={includeOptional}
                              onChange={(e) => setIncludeOptional(e.target.checked)}
                            />
                            Include optional collection mods
                          </label>
                        )}
                      </div>
                    </div>
                    </div>

                    {browseDetail.tab === "info" ? (
                      collectionDetailLoading && !collectionDetail && !tsDetail ? (
                        <div className="page-scroll" ref={browseDetailScrollRef}>
                        <CollectionDetailSkeleton tab="info" />
                        </div>
                      ) : (
                        <div className="detail-body page-scroll" ref={browseDetailScrollRef}>
                          <h2>About</h2>
                          <RichText
                            className="detail-description detail-description-html"
                            text={
                              collectionHitSource(browseDetail.hit) === "thunderstore"
                                ? (tsDetail?.description ?? browseDetail.hit.summary)
                                : (collectionDetail?.description ??
                                  collectionDetail?.summary ??
                                  browseDetail.hit.summary)
                            }
                          />
                        </div>
                      )
                    ) : collectionHitSource(browseDetail.hit) === "thunderstore" ? (
                      collectionFilesLoading && !tsDetail ? (
                        <div className="page-scroll" ref={browseDetailScrollRef}>
                        <CollectionDetailSkeleton tab="files" />
                        </div>
                      ) : (
                        <div className="detail-body page-view">
                          <div className="page-chrome">
                          <h2>
                            Included mods (
                            {(tsDetail?.versions[0]?.dependencies.length ?? 0) + 1})
                          </h2>
                          </div>
                          <div className="page-scroll" ref={browseDetailScrollRef}>
                          <ul className="list">
                            <li>
                              <div>
                                <strong>{browseDetail.hit.name}</strong>
                                <small>modpack</small>
                              </div>
                            </li>
                            {(tsDetail?.versions[0]?.dependencies ?? []).map((dep) => (
                              <li key={dep}>
                                <div>
                                  <strong>{dep}</strong>
                                  <small>dependency</small>
                                </div>
                              </li>
                            ))}
                          </ul>
                          </div>
                        </div>
                      )
                    ) : collectionFilesLoading && groupedCollectionMods.length === 0 ? (
                      <div className="page-scroll" ref={browseDetailScrollRef}>
                      <CollectionDetailSkeleton tab="files" />
                      </div>
                    ) : (
                      <div className="detail-body page-view">
                        <div className="page-chrome">
                        <h2>
                          Included mods ({groupedCollectionMods.length})
                          {collectionModFiles.length > 0 &&
                          collectionModFiles.length !== groupedCollectionMods.length
                            ? ` · ${collectionModFiles.length} files`
                            : ""}
                        </h2>
                        </div>
                        <div className="page-scroll" ref={browseDetailScrollRef}>
                        {groupedCollectionMods.length === 0 ? (
                          <ul className="list">
                            <li className="empty">No mods found in this collection.</li>
                          </ul>
                        ) : (
                          <BrowseVirtualList
                            items={groupedCollectionMods}
                            scrollRef={browseDetailScrollRef}
                            active={tab === "browse"}
                            staggerEnter
                            estimateSize={estimateListRowHeight(false)}
                            getKey={(group) => `${group.domain_name}:${group.mod_id}`}
                            rowClassName="list-row-clickable"
                            renderItem={(group) => {
                              const primary = group.files[0];
                              const extras =
                                group.files.length > 1
                                  ? ` · ${group.files.length} files`
                                  : "";
                              const openGroupMod = () =>
                                openMod(
                                  {
                                    source: "nexus",
                                    id: `nexus:${group.domain_name}:${group.mod_id}`,
                                    name: group.mod_name,
                                    summary: null,
                                    picture_url: null,
                                    downloads: null,
                                    endorsements: null,
                                    author: null,
                                    category: null,
                                    tags: [],
                                    mod_id: group.mod_id,
                                    domain_name: group.domain_name,
                                    community: null,
                                    namespace: null,
                                    package_name: null,
                                    full_name: null,
                                    package_url: null,
                                    rating_score: null,
                                    latest_version: null,
                                    modio_game_id: null,
                                    modio_mod_id: null,
                                    profile_url: null,
                                  },
                                  "info",
                                );
                              return (
                                <div
                                  className="list-row-main"
                                  role="button"
                                  tabIndex={0}
                                  onClick={openGroupMod}
                                  onKeyDown={(e) => {
                                    if (e.key === "Enter" || e.key === " ") {
                                      e.preventDefault();
                                      openGroupMod();
                                    }
                                  }}
                                >
                                  <strong>{group.mod_name}</strong>
                                  <small>
                                    {primary.file_name}
                                    {primary.version ? ` · v${primary.version}` : ""}
                                    {primary.optional ? " · optional" : ""}
                                    {extras}
                                  </small>
                                </div>
                              );
                            }}
                          />
                        )}
                        </div>
                      </div>
                    )}
                  </>
                )}
                </ViewSwitch>
              </>
            ) : (
              <>
                <div className="browse-chrome">
                <div className="panel-head">
                  <div className="detail-head-title">
                    <button type="button" onClick={scrollBrowseHome}>
                      Home
                    </button>
                    <h1>Browse Mods</h1>
                  </div>
                  <div className="panel-head-actions">
                    <Segment>
                      <SegmentTab
                        layoutId="browse-mode"
                        active={browseMode === "mods"}
                        onClick={() => {
                          setBrowseMode("mods");
                          setBrowseSort("endorsements");
                          setBrowseCategory("");
                          setBrowseVersion("");
                          setBrowseAuthor("");
                          setBrowseAuthorDraft("");
                          setBrowseTags({});
                          setBrowseDetail(null);
                          runSearch("mods", searchQuery, {
                            ...browseOpts,
                            sort: "endorsements",
                            category: null,
                            gameVersion: null,
                            author: null,
                            tagsInclude: [],
                            tagsExclude: [],
                            offset: 0,
                          });
                        }}
                      >
                        Mods
                      </SegmentTab>
                      <SegmentTab
                        layoutId="browse-mode"
                        active={browseMode === "collections"}
                        onClick={() => {
                          setBrowseMode("collections");
                          setBrowseSort("endorsements");
                          setBrowseCategory("");
                          setBrowseVersion("");
                          setBrowseAuthor("");
                          setBrowseAuthorDraft("");
                          setBrowseTags({});
                          setBrowseDetail(null);
                          if (sourceFilter === "modio") setSourceFilter("all");
                          runSearch("collections", searchQuery, {
                            ...browseOpts,
                            sort: "endorsements",
                            category: null,
                            gameVersion: null,
                            author: null,
                            tagsInclude: [],
                            tagsExclude: [],
                            offset: 0,
                          });
                        }}
                      >
                        Collections
                      </SegmentTab>
                    </Segment>
                    {(browseMode === "mods"
                      ? [
                          Boolean(activeGame?.nexus_domain),
                          Boolean(activeGame?.thunderstore_community),
                          Boolean(activeGame?.modio_game_id),
                        ]
                      : [
                          Boolean(activeGame?.nexus_domain),
                          Boolean(activeGame?.thunderstore_community),
                        ]
                    ).filter(Boolean).length > 1 && (
                        <Segment className="source-filter">
                          {(
                            [
                              ["all", "All"] as const,
                              ...(activeGame?.nexus_domain
                                ? ([["nexus", "Nexus"]] as const)
                                : []),
                              ...(activeGame?.thunderstore_community
                                ? ([["thunderstore", "Thunderstore"]] as const)
                                : []),
                              ...(browseMode === "mods" && activeGame?.modio_game_id
                                ? ([["modio", "mod.io"]] as const)
                                : []),
                            ]
                          ).map(([id, label]) => (
                            <SegmentTab
                              key={id}
                              layoutId="browse-source"
                              active={sourceFilter === id}
                              aria-label={label}
                              title={label}
                              onClick={() => setSourceFilter(id)}
                            >
                              {id === "all" ? (
                                label
                              ) : (
                                <SourceMark source={id} decorative />
                              )}
                            </SegmentTab>
                          ))}
                        </Segment>
                      )}
                    {browseMode === "collections" && activeGame && (
                        <button
                          type="button"
                          onClick={() => setProfileImportOpen((v) => !v)}
                        >
                          Import code
                        </button>
                      )}
                    {browseView === "grid" && (
                      <label className="grid-scale">
                        Size
                        <input
                          type="range"
                          min={BROWSE_GRID_MIN_PX}
                          max={BROWSE_GRID_MAX_PX}
                          step={10}
                          value={browseGridMin}
                          aria-label="Grid size"
                          onChange={(e) => handleBrowseGridMinChange(Number(e.target.value))}
                        />
                      </label>
                    )}
                    {browseView === "list" && (
                      <Segment>
                        <SegmentTab
                          layoutId="browse-density"
                          active={browseListDensity === "posters"}
                          onClick={() => setBrowseListDensity("posters")}
                        >
                          Posters
                        </SegmentTab>
                        <SegmentTab
                          layoutId="browse-density"
                          active={browseListDensity === "compact"}
                          onClick={() => setBrowseListDensity("compact")}
                        >
                          Compact
                        </SegmentTab>
                      </Segment>
                    )}
                    <Segment>
                      <SegmentTab
                        layoutId="browse-view"
                        active={browseView === "list"}
                        onClick={() => setBrowseView("list")}
                      >
                        List
                      </SegmentTab>
                      <SegmentTab
                        layoutId="browse-view"
                        active={browseView === "grid"}
                        onClick={() => setBrowseView("grid")}
                      >
                        Grid
                      </SegmentTab>
                    </Segment>
                  </div>
                </div>
                {!activeGame ? (
                  <p>Select a managed game first.</p>
                ) : (
                  <>
                    <div className="row browse-search-row" ref={authorSuggestRef}>
                      <div className="browse-search-field">
                        <input
                          placeholder={
                            browseMode === "collections"
                              ? "Search collections and modpacks…"
                              : "Search mods or @creator…"
                          }
                          value={searchQuery}
                          onChange={(e) => setSearchQuery(e.target.value)}
                          onKeyDown={(e) => {
                            if (
                              browseMode === "mods" &&
                              authorSuggestOpen &&
                              authorSuggestions.length > 0
                            ) {
                              if (e.key === "ArrowDown") {
                                e.preventDefault();
                                setAuthorSuggestIndex(
                                  (i) => (i + 1) % authorSuggestions.length,
                                );
                                return;
                              }
                              if (e.key === "ArrowUp") {
                                e.preventDefault();
                                setAuthorSuggestIndex(
                                  (i) =>
                                    (i - 1 + authorSuggestions.length) %
                                    authorSuggestions.length,
                                );
                                return;
                              }
                              if (e.key === "Escape") {
                                e.preventDefault();
                                setAuthorSuggestOpen(false);
                                return;
                              }
                            }
                            if (e.key === "Enter") {
                              e.preventDefault();
                              void search();
                            }
                          }}
                          aria-autocomplete={
                            browseMode === "mods" ? "list" : undefined
                          }
                          aria-expanded={
                            browseMode === "mods" ? authorSuggestOpen : undefined
                          }
                          aria-controls={
                            browseMode === "mods"
                              ? "browse-author-suggest"
                              : undefined
                          }
                        />
                        <PopIn
                          show={
                            browseMode === "mods" &&
                            authorSuggestOpen &&
                            (authorSuggestions.length > 0 || authorSuggestLoading)
                          }
                          className="browse-author-suggest"
                          id="browse-author-suggest"
                          role="listbox"
                          aria-label="Creator suggestions"
                        >
                          {authorSuggestLoading &&
                          authorSuggestions.length === 0 ? (
                            <div className="browse-author-suggest-empty">
                              Searching creators…
                            </div>
                          ) : (
                            authorSuggestions.map((hit, i) => (
                              <button
                                key={`${hit.source}:${hit.name}`}
                                type="button"
                                role="option"
                                aria-selected={i === authorSuggestIndex}
                                className={`browse-author-suggest-item${
                                  i === authorSuggestIndex ? " active" : ""
                                }`}
                                onMouseEnter={() => setAuthorSuggestIndex(i)}
                                onClick={() => pickAuthorSuggestion(hit)}
                              >
                                <span className="browse-author-suggest-name">
                                  @{hit.name}
                                </span>
                                <span className="browse-author-suggest-meta">
                                  <SourceMark source={hit.source} />
                                  {hit.mod_count != null
                                    ? ` · ${hit.mod_count.toLocaleString()} mods`
                                    : ""}
                                </span>
                              </button>
                            ))
                          )}
                        </PopIn>
                      </div>
                      <button
                        type="button"
                        className="mods-toolbar-icon-btn"
                        onClick={() => void search()}
                        aria-label="Search"
                        title="Search"
                      >
                        <FontAwesomeIcon icon={faMagnifyingGlass} />
                      </button>
                    </div>
                    <Reveal
                      show={profileImportOpen && browseMode === "collections"}
                      className="profile-import"
                    >
                        <label>
                          Share or profile code
                          <textarea
                            rows={3}
                            value={profileCode}
                            onChange={(e) => setProfileCode(e.target.value)}
                            placeholder="Paste Emperor share code or r2modman / Gale profile code"
                            onKeyDown={(e) => {
                              if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
                                void importThunderstoreProfileCode();
                              }
                            }}
                          />
                        </label>
                        <div className="actions">
                          <button
                            type="button"
                            onClick={() => void importThunderstoreProfileCode()}
                          >
                            Import
                          </button>
                          <button
                            type="button"
                            className="linkish"
                            onClick={() => {
                              setProfileImportOpen(false);
                              setProfileCode("");
                            }}
                          >
                            Cancel
                          </button>
                        </div>
                    </Reveal>
                    <div className="filters">
                      <label>
                        Sort
                        <select
                          value={browseSort}
                          onChange={(e) => {
                            const sort = e.target.value;
                            setBrowseSort(sort);
                            runSearch(browseMode, searchQuery, {
                              ...browseOpts,
                              sort,
                            });
                          }}
                        >
                          {sortOptions.map((o) => (
                            <option key={o.value} value={o.value}>
                              {o.label}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label>
                        Category
                        <select
                          value={browseCategory}
                          onChange={(e) => {
                            const category = e.target.value;
                            setBrowseCategory(category);
                            runSearch(browseMode, searchQuery, {
                              ...browseOpts,
                              category: category || null,
                            });
                          }}
                        >
                          <option value="">Any</option>
                          {(browseMeta?.categories ?? []).map((c) => (
                            <option key={c} value={c}>
                              {c}
                            </option>
                          ))}
                        </select>
                      </label>
                      <label>
                        Game version
                        <select
                          value={browseVersion}
                          title={
                            browseMode === "mods"
                              ? "Nexus mods tagged with this game version"
                              : undefined
                          }
                          onChange={(e) => {
                            const gameVersion = e.target.value;
                            setBrowseVersion(gameVersion);
                            runSearch(browseMode, searchQuery, {
                              ...browseOpts,
                              gameVersion: gameVersion || null,
                            });
                          }}
                        >
                          <option value="">Any</option>
                          {browseGameVersionOptions(
                            browseMeta,
                            browseMode,
                            activeGame?.nexus_domain,
                          ).map((v) => (
                            <option key={v} value={v}>
                              {v}
                            </option>
                          ))}
                        </select>
                      </label>
                      {browseMode === "mods" && (
                        <label>
                          Author
                          <input
                            type="text"
                            value={browseAuthorDraft}
                            placeholder="e.g. CyanideX"
                            title="Nexus uploader or Thunderstore owner (exact)"
                            onChange={(e) => setBrowseAuthorDraft(e.target.value)}
                            onBlur={() => applyBrowseAuthor(browseAuthorDraft)}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") {
                                e.preventDefault();
                                applyBrowseAuthor(browseAuthorDraft);
                              }
                            }}
                          />
                        </label>
                      )}
                      <div className="tag-picker" ref={tagPickerRef}>
                        <span className="tag-picker-label" id="browse-tags-label">
                          Tags
                        </span>
                        <button
                          type="button"
                          className="tag-picker-trigger"
                          aria-expanded={tagsOpen}
                          aria-haspopup="listbox"
                          aria-labelledby="browse-tags-label"
                          onClick={() => setTagsOpen((open) => !open)}
                        >
                          <span>
                            {activeTagCount === 0
                              ? "Any"
                              : activeTagCount === 1
                                ? Object.keys(browseTags)[0]
                                : `${activeTagCount} tags`}
                          </span>
                        </button>
                        <PopIn
                          show={tagsOpen}
                          className="tag-picker-popup"
                          role="listbox"
                          aria-multiselectable={true}
                          aria-labelledby="browse-tags-label"
                        >
                            <input
                              ref={tagSearchRef}
                              type="text"
                              className="tag-picker-search"
                              placeholder="Filter tags…"
                              value={tagQuery}
                              onChange={(e) => setTagQuery(e.target.value)}
                              onKeyDown={(e) => {
                                if (e.key === "Escape") {
                                  e.stopPropagation();
                                  setTagsOpen(false);
                                }
                              }}
                            />
                            <p className="tag-picker-hint">
                              Click: include → exclude → clear
                            </p>
                            <div className="tag-picker-list">
                              {orderedTags.length === 0 ? (
                                <div className="tag-picker-empty">No tags</div>
                              ) : (
                                <>
                                {orderedTags.slice(0, tagsPageLimit).map((t) => {
                                  const state = browseTags[t];
                                  return (
                                    <button
                                      key={t}
                                      type="button"
                                      role="option"
                                      aria-selected={Boolean(state)}
                                      className={`tag-picker-item${
                                        state === "include"
                                          ? " include"
                                          : state === "exclude"
                                            ? " exclude"
                                            : ""
                                      }`}
                                      onClick={() => cycleBrowseTag(t)}
                                    >
                                      <span className="tag-picker-state" aria-hidden="true">
                                        {state === "include"
                                          ? "+"
                                          : state === "exclude"
                                            ? "−"
                                            : ""}
                                      </span>
                                      <span>{t}</span>
                                    </button>
                                  );
                                })}
                                {orderedTags.length > tagsPageLimit && (
                                  <button
                                    type="button"
                                    className="tag-picker-item tag-picker-more"
                                    onClick={() => setTagsPageLimit((n) => n + 60)}
                                  >
                                    Show more ({orderedTags.length - tagsPageLimit} remaining)
                                  </button>
                                )}
                                </>
                              )}
                            </div>
                        </PopIn>
                      </div>
                      <button
                        type="button"
                        role="switch"
                        aria-checked={hideNsfw}
                        className="browse-nsfw-toggle"
                        onClick={() => void toggleBrowseHideNsfw()}
                        title={
                          hideNsfw
                            ? "NSFW content is hidden (click to show)"
                            : "Showing NSFW content (click to hide)"
                        }
                      >
                        <span className="switch-track" aria-hidden="true">
                          <span className="switch-thumb" />
                        </span>
                        <span>Hide NSFW</span>
                      </button>
                      <button
                        type="button"
                        className="filters-clear"
                        onClick={clearBrowseFilters}
                      >
                        Clear filters
                      </button>
                    </div>
                    {browseMode === "collections" &&
                      Boolean(activeGame.nexus_domain) &&
                      sourceFilter !== "thunderstore" && (
                      <label className="inline">
                        <input
                          type="checkbox"
                          checked={includeOptional}
                          onChange={(e) => setIncludeOptional(e.target.checked)}
                        />
                        Include optional collection mods
                      </label>
                    )}
                  </>
                )}
                </div>
                {activeGame ? (
                  <div className="browse-results" ref={browseResultsRef}>
                    {browseMode === "mods" && browseAuthor ? (
                      <div className="browse-author-chip" role="status">
                        <span>
                          Creator: <strong>{browseAuthor}</strong>
                        </span>
                        <button
                          type="button"
                          className="browse-author-chip-clear icon-btn"
                          aria-label={`Clear creator filter ${browseAuthor}`}
                          title="Clear"
                          onClick={() => applyBrowseAuthor("")}
                        >
                          <FontAwesomeIcon icon={faXmark} />
                        </button>
                      </div>
                    ) : null}
                    {showBrowseGrid ? (
                    <div className="results-fill">
                    {browseMode === "mods" ? (
                      browseView === "grid" ? (
                        <>
                          {modHits.length === 0 ? (
                            <p className="note">{browseResultsNote("mods")}</p>
                          ) : (
                            <BrowseVirtualGrid
                              items={modHits}
                              scrollRef={browseResultsRef}
                              active={tab === "browse"}
                              staggerEnter
                              minCardPx={deferredBrowseGridMin}
                              fixedRowHeight
                              fillTrailingSkeletons={browseHasMore}
                              initialScrollTop={browseScrollRef.current}
                              onPageSize={onBrowsePageSize}
                              getKey={getModHitKey}
                              renderItem={renderBrowseModGridItem}
                              renderSkeletonItem={renderBrowseGridSkeletonCard}
                            />
                          )}
                        </>
                      ) : modHits.length === 0 ? (
                        <ul className="list">
                          <li className="empty">{browseResultsNote("mods")}</li>
                        </ul>
                      ) : (
                        <BrowseVirtualList
                          items={modHits}
                          active={tab === "browse"}
                          staggerEnter
                          scrollRef={browseResultsRef}
                          onPageSize={onBrowsePageSize}
                          getKey={(m) => m.id}
                          rowClassName={
                            browseListDensity === "posters"
                              ? "list-row-clickable list-row-poster"
                              : "list-row-clickable"
                          }
                          estimateSize={estimateListRowHeight(browseListDensity === "posters")}
                          renderItem={(m) => {
                            const posterDetail =
                              browseListDensity === "posters" ? posterModDetail(m) : null;
                            const posterTags =
                              browseListDensity === "posters"
                                ? m.tags.filter((tag) => tag.trim()).slice(0, 4)
                                : [];
                            return (
                              <>
                                {browseListDensity === "posters" ? (
                                  <ListPoster
                                    src={mediaSrc(null, m.picture_url)}
                                    decoding="async"
                                  />
                                ) : null}
                                <div
                                  className="list-row-main"
                                  onClick={() => openMod(m, "info")}
                                  onKeyDown={(e) => {
                                    if (e.key === "Enter" || e.key === " ") {
                                      e.preventDefault();
                                      openMod(m, "info");
                                    }
                                  }}
                                  role="button"
                                  tabIndex={0}
                                >
                                  <strong>
                                    {m.name}
                                    {installedIds.has(m.id) ? (
                                      <span className="pill ok">Installed</span>
                                    ) : null}
                                  </strong>
                                  <small>
                                    <SourceMark source={m.source} />
                                    {" · "}
                                    {m.author ? (
                                      <button
                                        type="button"
                                        className="detail-author-link"
                                        title={`Browse mods by ${m.author}`}
                                        onClick={(e) => {
                                          e.stopPropagation();
                                          browseByAuthor(m.author!, m.source);
                                        }}
                                      >
                                        {m.author}
                                      </button>
                                    ) : (
                                      "unknown"
                                    )}
                                    {m.source === "nexus" && m.endorsements != null
                                      ? ` · ${m.endorsements} endorsements`
                                      : m.downloads != null
                                        ? ` · ${m.downloads.toLocaleString()} downloads`
                                        : ""}
                                  </small>
                                  {posterDetail ? (
                                    <small className="list-row-detail">{posterDetail}</small>
                                  ) : null}
                                  {posterTags.length > 0 ? (
                                    <div className="list-row-tags">
                                      {posterTags.map((tag) => (
                                        <span key={tag} className="pill">
                                          {tag}
                                        </span>
                                      ))}
                                    </div>
                                  ) : null}
                                  {m.summary && <p className="summary">{m.summary}</p>}
                                </div>
                                {m.source === "nexus" ? (
                                  <div className="list-row-actions">
                                    {installedIds.has(m.id) ? (
                                      <button
                                        type="button"
                                        className="danger"
                                        onClick={() => void uninstallCatalogMod(m)}
                                      >
                                        Uninstall
                                      </button>
                                    ) : (
                                      <button
                                        type="button"
                                        onClick={() => void quickInstallMod(m)}
                                      >
                                        {isPremium ? "Install" : "Download Assist"}
                                      </button>
                                    )}
                                    <button
                                      type="button"
                                      onClick={() => openMod(m, "files")}
                                    >
                                      Files
                                    </button>
                                  </div>
                                ) : m.source === "thunderstore" ? (
                                  installedIds.has(m.id) ? (
                                    <button
                                      type="button"
                                      className="danger"
                                      onClick={() => void uninstallCatalogMod(m)}
                                    >
                                      Uninstall
                                    </button>
                                  ) : (
                                    <button onClick={() => openMod(m, "info")}>Install</button>
                                  )
                                ) : installedIds.has(m.id) ? (
                                  <div className="list-row-actions">
                                    <button
                                      type="button"
                                      className="danger"
                                      onClick={() => void uninstallCatalogMod(m)}
                                    >
                                      Uninstall
                                    </button>
                                    <button onClick={() => openMod(m, "files")}>Files</button>
                                  </div>
                                ) : (
                                  <button onClick={() => openMod(m, "files")}>Files</button>
                                )}
                              </>
                            );
                          }}
                        />
                      )
                    ) : browseView === "grid" ? (
                      <>
                        {collectionHits.length === 0 ? (
                          <p className="note">{browseResultsNote("collections")}</p>
                        ) : (
                          <BrowseVirtualGrid
                            items={collectionHits}
                            scrollRef={browseResultsRef}
                            active={tab === "browse"}
                            staggerEnter
                            minCardPx={deferredBrowseGridMin}
                            fixedRowHeight
                            fillTrailingSkeletons={browseHasMore}
                            initialScrollTop={browseScrollRef.current}
                            onPageSize={onBrowsePageSize}
                            getKey={collectionHitKey}
                            renderItem={renderBrowseCollectionGridItem}
                            renderSkeletonItem={renderBrowseGridSkeletonCard}
                          />
                        )}
                      </>
                    ) : collectionHits.length === 0 ? (
                      <ul className="list">
                        <li className="empty">{browseResultsNote("collections")}</li>
                      </ul>
                    ) : (
                      <BrowseVirtualList
                        items={collectionHits}
                        scrollRef={browseResultsRef}
                        active={tab === "browse"}
                        staggerEnter
                        onPageSize={onBrowsePageSize}
                        getKey={collectionHitKey}
                        rowClassName={
                          browseListDensity === "posters"
                            ? "list-row-clickable list-row-poster"
                            : "list-row-clickable"
                        }
                        estimateSize={estimateListRowHeight(browseListDensity === "posters")}
                        renderItem={(c) => {
                          const posterDetail =
                            browseListDensity === "posters" ? posterCollectionDetail(c) : null;
                          return (
                            <>
                              {browseListDensity === "posters" ? (
                                <ListPoster
                                  src={mediaSrc(null, c.tile_image_url)}
                                  decoding="async"
                                />
                              ) : null}
                              <div
                                className="list-row-main"
                                onClick={() => openCollection(c, "info")}
                                onKeyDown={(e) => {
                                  if (e.key === "Enter" || e.key === " ") {
                                    e.preventDefault();
                                    openCollection(c, "info");
                                  }
                                }}
                                role="button"
                                tabIndex={0}
                              >
                                <strong>{c.name}</strong>
                                <small>
                                  <SourceMark source={collectionHitSource(c)} />
                                  {" · "}
                                  {c.author ?? c.slug}
                                  {c.mod_count != null
                                    ? ` · ${c.mod_count.toLocaleString()} mods`
                                    : ""}
                                  {c.file_size != null && c.file_size > 0
                                    ? ` · ${formatBytes(c.file_size)}`
                                    : ""}
                                </small>
                                {posterDetail ? (
                                  <small className="list-row-detail">{posterDetail}</small>
                                ) : null}
                                {c.summary && <p className="summary">{c.summary}</p>}
                              </div>
                              <button onClick={() => installCollection(c)}>Install</button>
                            </>
                          );
                        }}
                      />
                    )}
                    {browseHasMore ? (
                      <div
                        ref={setBrowseSentinel}
                        className="browse-scroll-sentinel"
                      >
                        {tab === "browse" ? (
                          <BrowseResultsSkeleton
                            view={browseView}
                            posters={browseListDensity === "posters"}
                            rows={2}
                            minCardPx={deferredBrowseGridMin}
                          />
                        ) : null}
                      </div>
                    ) : null}
                    {(modHits.length > 0 || collectionHits.length > 0) && (
                      <p className="browse-page-status note">
                        {browseMode === "mods"
                          ? `Showing ${modHits.length}${
                              browseTotalCount > 0 ? ` of ${browseTotalCount}` : ""
                            } mods`
                          : `Showing ${collectionHits.length}${
                              browseTotalCount > 0
                                ? ` of ${browseTotalCount}`
                                : ""
                            } collections`}
                        {browseHasMore ? " · scroll for more" : ""}
                      </p>
                    )}
                    </div>
                    ) : browseResultsVisible ? (
                      <BrowseResultsSkeleton
                        view={browseView}
                        posters={browseListDensity === "posters"}
                        count={browseSkeletonCount}
                        minCardPx={browseGridMin}
                      />
                    ) : null}
                  </div>
                ) : null}
              </>
            )}
          </ViewSwitch>
          </ParkedTabGate>
        </div>
        ) : null}

        {downloadsKeptRef.current ? (
        <div
          className={tab === "downloads" ? "tab-kept" : "tab-kept tab-parked"}
          inert={tab !== "downloads"}
          aria-hidden={tab !== "downloads" ? true : undefined}
        >
          <ParkedTabGate active={tab === "downloads"}>
          <DownloadsWorkspace
            downloads={downloads}
            queue={assistQueue}
            activeBatch={activeBatch}
            assistActive={assistActive}
            assistHint={assistHint}
            active={tab === "downloads"}
            loading={!downloadsReady}
            onRefresh={refreshDownloads}
            onSkip={() => skipAssistCurrent()}
            onCancel={() => cancelAssistQueue()}
            onCancelRemaining={() => cancelRemainingBatch()}
            onCancelDownload={(id) => cancelOneDownload(id)}
            onRestartDownload={(id) => restartOneDownload(id)}
            onRetryAssistOpening={() => retryAssistOpening()}
            isDownloadStuck={isDownloadStuck}
            onClearRecent={(ids) => clearRecentDownloads(ids)}
            onOpenRecent={(item) => void openRecentDownload(item)}
            onRemoveDownload={(id) => removeOneDownload(id)}
            onPauseDownload={(id) => pauseOneDownload(id)}
            onResumeDownload={(id) => resumeOneDownload(id)}
            onAssistHost={handleAssistHost}
          />
          </ParkedTabGate>
        </div>
        ) : null}

        {toolsKeptRef.current ? (
        <div
          className={tab === "tools" ? "tab-kept" : "tab-kept tab-parked"}
          inert={tab !== "tools"}
          aria-hidden={tab !== "tools" ? true : undefined}
        >
          <ParkedTabGate active={tab === "tools"}>
          <ToolsErrorBoundary>
            <ToolsWorkspace active={tab === "tools"} />
          </ToolsErrorBoundary>
          </ParkedTabGate>
        </div>
        ) : null}

        {settingsKeptRef.current ? (
        <div
          className={tab === "settings" ? "tab-kept" : "tab-kept tab-parked"}
          inert={tab !== "settings"}
          aria-hidden={tab !== "settings" ? true : undefined}
        >
          <ParkedTabGate active={tab === "settings"}>
          <section className="panel page-panel">
            <div className="page-chrome">
            <div className="panel-head">
              <h1>Settings</h1>
            </div>
            </div>
            <div className="page-scroll">
            <SettingsSection title="App updates" defaultOpen>
              <p className="note">
                Current version:{" "}
                <strong>v{appInfo?.version ?? appUpdate?.current_version ?? "…"}</strong>
                {appUpdate?.latest_version
                  ? ` · Latest on GitHub: v${appUpdate.latest_version}`
                  : ""}
                {appUpdate?.install_via
                  ? ` · Updates with ${installViaLabel(appUpdate.install_via)}`
                  : ""}
              </p>
              {appUpdateMessage && <p className="note">{appUpdateMessage}</p>}
              {appUpdate?.update_available && !appUpdate.asset_name && (
                <p className="note">
                  {appUpdate.install_via
                    ? "A newer release exists, but the matching package is not on the GitHub release yet."
                    : "A newer release exists, but this copy was not installed in a way the app can replace. Use the release page to update it."}
                </p>
              )}
              <div className="actions">
                <button
                  type="button"
                  disabled={checkingAppUpdate || installingAppUpdate}
                  onClick={() => void checkForAppUpdate(false)}
                >
                  {checkingAppUpdate ? "Checking…" : "Check for updates"}
                </button>
                {appUpdate?.update_available && appUpdate.asset_name && (
                  <button
                    type="button"
                    disabled={checkingAppUpdate || installingAppUpdate}
                    onClick={() => void installAppUpdate()}
                  >
                    {installingAppUpdate
                      ? "Updating…"
                      : `Update to v${appUpdate.latest_version}`}
                  </button>
                )}
                {appUpdate?.release_url && (
                  <button
                    type="button"
                    className="linkish"
                    onClick={() => void openUrl(appUpdate.release_url!)}
                  >
                    Open release page
                  </button>
                )}
              </div>
            </SettingsSection>
            <SettingsSection title="Appearance">
              <div className="setting-block">
                <span>Theme</span>
                <Segment>
                  {(["light", "dark", "system"] as const).map((t) => (
                    <SegmentTab
                      key={t}
                      layoutId="settings-theme"
                      active={themePref === t}
                      onClick={() => setTheme(t)}
                    >
                      {t === "light" ? "Light" : t === "dark" ? "Dark" : "System"}
                    </SegmentTab>
                  ))}
                </Segment>
              </div>
              <div className="setting-block">
                <span>Style</span>
                <Segment>
                  {(
                    [
                      ["app", "App"],
                      ["brand", "Brand"],
                      ["system", "System"],
                    ] as const
                  ).map(([value, label]) => (
                    <SegmentTab
                      key={value}
                      layoutId="settings-theme-style"
                      active={themeStyle === value}
                      onClick={() => setThemeStyle(value)}
                    >
                      {label}
                    </SegmentTab>
                  ))}
                </Segment>
                <p className="note">
                  App is a monochrome light/dark look with a chosen accent. Brand keeps
                  the warm palette. System uses{" "}
                  {settings?.platform_windows
                    ? "Windows window, border, and accent colors"
                    : settings?.platform_linux
                      ? "GTK or Qt theme colors (pick the toolkit below)"
                      : "operating system colors"}
                  .
                </p>
              </div>
              {themeStyle === "system" && settings?.platform_linux && (
                <div className="setting-block">
                  <span>Toolkit</span>
                  <Segment>
                    {(
                      [
                        ["gtk", "Gtk"],
                        ["qt", "Qt"],
                      ] as const
                    ).map(([value, label]) => (
                      <SegmentTab
                        key={value}
                        layoutId="settings-linux-toolkit"
                        active={linuxSystemBackend === value}
                        onClick={() => setLinuxSystemBackend(value)}
                      >
                        {label}
                      </SegmentTab>
                    ))}
                  </Segment>
                  <p className="note">
                    Gtk reads the GTK theme; Qt reads qt6ct/qt5ct or Plasma
                    color schemes (same source as apps like qBittorrent).
                  </p>
                </div>
              )}
              {themeStyle === "app" && (
                <div className="setting-block">
                  <span>Accent</span>
                  <div className="accent-swatches">
                    {ACCENT_PRESETS.map((swatch) => (
                      <button
                        key={swatch}
                        type="button"
                        className={
                          accentColor === swatch
                            ? "accent-swatch active"
                            : "accent-swatch"
                        }
                        style={{ ["--swatch" as string]: swatch }}
                        aria-label={`Accent ${swatch}`}
                        title={swatch}
                        onClick={() => setAccentColor(swatch)}
                      />
                    ))}
                    <label className="inline">
                      <span className="note">Hex</span>
                      <input
                        className="accent-hex"
                        type="text"
                        spellCheck={false}
                        key={accentColor}
                        defaultValue={accentColor}
                        aria-label="Accent color hex"
                        onBlur={(e) => {
                          const next = normalizeAccent(e.target.value);
                          e.target.value = next;
                          if (next === accentColor) return;
                          setAccentColor(next);
                        }}
                      />
                    </label>
                  </div>
                </div>
              )}
            </SettingsSection>
            <SettingsSection title="Banners">
              <div className="setting-block">
                <span>Remember across restarts</span>
                <Segment>
                  {(
                    [
                      ["off", "Off"],
                      ["errors", "Errors only"],
                      ["errors_and_warnings", "Errors & warnings"],
                    ] as const
                  ).map(([val, optLabel]) => (
                    <SegmentTab
                      key={val}
                      layoutId="settings-banner-persistence"
                      active={bannerPersistenceFor(settings) === val}
                      onClick={() => {
                        if (bannerPersistenceFor(settings) === val) return;
                        void withBusy("Saving…", async () => {
                          await api.setBannerPersistence(val);
                          await refreshSettings();
                        });
                      }}
                    >
                      {optLabel}
                    </SegmentTab>
                  ))}
                </Segment>
              </div>
              <label className="inline">
                <input
                  type="checkbox"
                  checked={settings?.dismiss_banner_on_retry !== false}
                  onChange={(e) =>
                    withBusy("Saving…", async () => {
                      await api.setDismissBannerOnRetry(e.target.checked);
                      await refreshSettings();
                    })
                  }
                />
                Dismiss download error banners when retrying or removing the download
              </label>
              <p className="note">
                Choose how long status banners stay visible before automatically
                dismissing. Saved banners remain until dismissed, retried, or timed out.
              </p>
              {(
                [
                  ["info", "Green (status)"],
                  ["warn", "Amber (warning)"],
                  ["error", "Red (error)"],
                ] as const
              ).map(([kind, label]) => {
                const current = bannerTimeoutFor(kind, settings);
                const presetValues = new Set([5, 10, 30, 60, 300]);
                const isPreset = current !== null && presetValues.has(current);
                const isCustom =
                  Boolean(customBannerTimeouts[kind]) ||
                  (current !== null && !isPreset);
                const activeValue = isCustom
                  ? "custom"
                  : current === null
                    ? "never"
                    : String(current);
                const saveTimeout = (seconds: number | null) => {
                  const next = {
                    info: bannerTimeoutFor("info", settings),
                    warn: bannerTimeoutFor("warn", settings),
                    error: bannerTimeoutFor("error", settings),
                    [kind]: seconds,
                  };
                  if (current === seconds) return;
                  void withBusy("Saving…", async () => {
                    await api.setBannerTimeouts(next);
                    await refreshSettings();
                  });
                };
                return (
                  <div className="setting-block banner-timeout-row" key={kind}>
                    <span className="banner-timeout-label">
                      <span
                        className={`banner-timeout-dot ${kind}`}
                        aria-hidden="true"
                      />
                      {label}
                    </span>
                    <div className="banner-timeout-controls">
                      <Segment>
                        {(
                          [
                            ["never", "Never"],
                            ["5", "5s"],
                            ["10", "10s"],
                            ["30", "30s"],
                            ["60", "1m"],
                            ["300", "5m"],
                            ["custom", "Custom"],
                          ] as const
                        ).map(([val, optLabel]) => (
                          <SegmentTab
                            key={val}
                            layoutId={`settings-banner-timeout-${kind}`}
                            active={activeValue === val}
                            onClick={() => {
                              if (val === "custom") {
                                setCustomBannerTimeouts((prev) => ({
                                  ...prev,
                                  [kind]: true,
                                }));
                                return;
                              }
                              setCustomBannerTimeouts((prev) => ({
                                ...prev,
                                [kind]: false,
                              }));
                              saveTimeout(val === "never" ? null : Number(val));
                            }}
                          >
                            {optLabel}
                          </SegmentTab>
                        ))}
                      </Segment>
                      {isCustom && (
                        <label className="inline">
                          <input
                            type="number"
                            min={1}
                            max={86400}
                            key={`${kind}:${current ?? "none"}`}
                            defaultValue={current ?? 10}
                            aria-label={`${label} custom seconds`}
                            onBlur={(e) => {
                              const n = Math.round(Number(e.target.value));
                              if (!Number.isFinite(n)) return;
                              const clamped = Math.min(86400, Math.max(1, n));
                              e.target.value = String(clamped);
                              saveTimeout(clamped);
                            }}
                          />
                          <span className="note">sec</span>
                        </label>
                      )}
                    </div>
                  </div>
                );
              })}
            </SettingsSection>
            <SettingsSection title="Downloads & install">
              <div className="setting-block">
                <span>After Install</span>
                <Segment>
                  {(
                    [
                      ["stay", "Stay here"],
                      ["downloads", "Go to Downloads"],
                    ] as const
                  ).map(([value, label]) => (
                    <SegmentTab
                      key={value}
                      layoutId="settings-after-install"
                      active={(settings?.install_click_behavior ?? "downloads") === value}
                      onClick={() => setInstallClickBehavior(value)}
                    >
                      {label}
                    </SegmentTab>
                  ))}
                </Segment>
              </div>
              <div className="setting-block">
                <span>Recent downloads</span>
                <p className="note">
                  Finished downloads stay in the Recent list until you clear them,
                  or until the list reaches this length. The oldest entries drop
                  off first.
                </p>
                <label className="inline">
                  Keep
                  <input
                    type="number"
                    min={1}
                    max={500}
                    key={settings?.recent_downloads_limit ?? 50}
                    defaultValue={settings?.recent_downloads_limit ?? 50}
                    aria-label="Recent downloads limit"
                    onBlur={(e) => {
                      const n = Math.round(Number(e.target.value));
                      if (!Number.isFinite(n)) return;
                      const limit = Math.min(500, Math.max(1, n));
                      e.target.value = String(limit);
                      if (limit === (settings?.recent_downloads_limit ?? 50)) return;
                      void withBusy("Saving…", async () => {
                        await api.setRecentDownloadsLimit(limit);
                        await refreshSettings();
                        await refreshDownloads();
                      });
                    }}
                  />
                </label>
              </div>
              <label className="inline">
                <input
                  type="checkbox"
                  checked={settings?.autoclick_free_download ?? true}
                  onChange={(e) =>
                    withBusy("Saving…", async () => {
                      await api.setAutoclickFreeDownload(e.target.checked);
                      await refreshSettings();
                    })
                  }
                />
                Autoclick free download button in Download Assist (Nexus Fast
                Download–style; waits for countdown)
              </label>
              <p className="note">
                Free accounts: Install opens the Downloads workspace with an embedded
                Nexus Assist panel. Sign in once with Stay signed in, using Log in to
                Nexus in Accounts or that panel. The website session is saved separately
                from your API key. Autoclick can press Slow download
                for you. Premium: one-click Install and Collections. Register{" "}
                <code>nxm://</code> via the desktop entry for browser “Download with
                manager” links.
              </p>
            </SettingsSection>
            <SettingsSection title="Content">
              <label className="inline">
                <input
                  type="checkbox"
                  checked={settings?.adult_content ?? true}
                  onChange={(e) =>
                    withBusy("Saving…", async () => {
                      await api.setAdultContent(e.target.checked);
                      await refreshSettings();
                    })
                  }
                />
                Show adult content in search
              </label>
              <label className="inline">
                <input
                  type="checkbox"
                  checked={settings?.game_version_warnings !== false}
                  onChange={(e) =>
                    withBusy("Saving…", async () => {
                      await api.setGameVersionWarnings(e.target.checked);
                      await refreshSettings();
                    })
                  }
                />
                Warn when a mod does not match the game version
              </label>
            </SettingsSection>
            <SettingsSection title="Data & maintenance">
              {settings && (
                <dl className="meta">
                  <dt>Config</dt>
                  <dd>{settings.config_dir}</dd>
                  <dt>Data</dt>
                  <dd>{settings.data_dir}</dd>
                  <dt>Cache</dt>
                  <dd>{settings.cache_dir}</dd>
                </dl>
              )}
              <div className="setting-block">
                <span>Download cache</span>
                <p className="note">
                  Removes leftover archives under the cache downloads folder. Active
                  downloads are left alone. Staging and installed mods are not touched.
                </p>
                <div className="actions">
                  <button
                    type="button"
                    onClick={() => {
                      if (
                        !window.confirm(
                          "Clear cached download archives? Active downloads will be kept."
                        )
                      ) {
                        return;
                      }
                      void withBusy("Clearing download cache…", async () => {
                        const result = await api.clearDownloadCache();
                        setNotice({
                          kind: "ok",
                          message:
                            result.files_removed === 0
                              ? "Download cache is already empty."
                              : `Cleared ${result.files_removed} cached file${
                                  result.files_removed === 1 ? "" : "s"
                                } (${formatBytes(result.bytes_freed)}).`,
                        });
                      });
                    }}
                  >
                    Clear download cache
                  </button>
                </div>
              </div>
              <div className="setting-block">
                <span>Mod recovery</span>
                <p className="note">
                  Scan staging data left behind by older versions or recover data from
                  nexus-manager. Recovery copies data first, so deployed symlinks remain
                  safe until you deploy again.
                </p>
                {orphanScan &&
                  (orphanScan.legacy_game_ids.length > 0 ||
                    orphanScan.missing_staging.length > 0 ||
                    orphanScan.untracked_staging.length > 0 ||
                    orphanScan.deploy_without_loadorder.length > 0) && (
                    <p className="note">
                      Found {orphanScan.legacy_game_ids.length} legacy game data folder
                      {orphanScan.legacy_game_ids.length === 1 ? "" : "s"}, {" "}
                      {orphanScan.missing_staging.length} missing staging path
                      {orphanScan.missing_staging.length === 1 ? "" : "s"}, and {" "}
                      {orphanScan.untracked_staging.length} untracked staging folder
                      {orphanScan.untracked_staging.length === 1 ? "" : "s"}.
                    </p>
                  )}
                <div className="actions">
                  <button type="button" onClick={() => void refreshOrphanScan()}>
                    Scan staged mods
                  </button>
                  <button type="button" onClick={() => void recoverLegacyMods()}>
                    Recover from nexus-manager
                  </button>
                </div>
              </div>
            </SettingsSection>
            <SettingsSection title="Social">
              <SocialSettingsSection
                friendsState={friendsState}
                onFriendsStateChange={setFriendsState}
                onNotice={(msg) => setNotice({ kind: "ok", message: msg })}
                onError={(msg) => setError(msg)}
              />
            </SettingsSection>
            <SettingsSection title="Accounts">
              {accountConnections}
            </SettingsSection>
            </div>
          </section>
          </ParkedTabGate>
        </div>
        ) : null}
      </main>
      {loadDiagnosis && (
        <LoadLogDialog
          diagnosis={loadDiagnosis}
          gameTitle={activeGame?.title}
          onClose={() => setLoadDiagnosis(null)}
        />
      )}
      {importModalOpen && activeGame && (
        <Modal
          title="Import mod"
          subtitle={`Stage archives, mod files, or folders for ${activeGame.title}.`}
          onClose={() => setImportModalOpen(false)}
          footer={
            <div className="modal-foot-actions">
              <button
                type="button"
                className="linkish"
                onClick={() => setImportModalOpen(false)}
              >
                Cancel
              </button>
            </div>
          }
        >
          <div
            className={
              importDropActive
                ? "import-dropzone import-dropzone-active"
                : "import-dropzone"
            }
          >
            <p className="import-dropzone-title">
              Drop archives, mod files, or folders here
            </p>
            <p className="note">
              Archives (.zip, .7z, .rar), loose mod files, and extracted folders
              are detected automatically.
            </p>
            <div className="import-dropzone-actions">
              <button type="button" onClick={() => void pickAndImportFiles()}>
                Choose file(s)…
              </button>
              <button type="button" onClick={() => void pickAndImportFolder()}>
                Choose folder…
              </button>
            </div>
          </div>
        </Modal>
      )}
      {saveLoadoutPrompt && (
        <Modal
          title="Save loadout"
          onClose={() => setSaveLoadoutPrompt(null)}
          footer={
            <div className="modal-foot-actions">
              <button
                type="button"
                className="linkish"
                onClick={() => setSaveLoadoutPrompt(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                className="accent"
                onClick={() => void saveLoadout(saveLoadoutName)}
              >
                Save loadout
              </button>
            </div>
          }
        >
          <p className="note">
            Save {saveLoadoutPrompt.modCount} enabled mod
            {saveLoadoutPrompt.modCount === 1 ? "" : "s"} for{" "}
            {saveLoadoutPrompt.gameTitle} as a shareable collection.
          </p>
          <form
            onSubmit={(e) => {
              e.preventDefault();
              void saveLoadout(saveLoadoutName);
            }}
          >
            <label>
              Collection name
              <input
                type="text"
                autoFocus
                value={saveLoadoutName}
                onChange={(e) => setSaveLoadoutName(e.target.value)}
                placeholder="Shared loadout"
              />
            </label>
          </form>
        </Modal>
      )}
      {saveImportPrompt && (
        <Modal
          title="Add to your collections?"
          onClose={() => setSaveImportPrompt(null)}
          footer={
            <div className="modal-foot-actions">
              <button
                type="button"
                className="linkish"
                onClick={() => setSaveImportPrompt(null)}
              >
                Cancel
              </button>
              <button type="button" onClick={() => void confirmSaveImport(false)}>
                Don't add
              </button>
              <button type="button" onClick={() => void confirmSaveImport(true)}>
                Add to collections
              </button>
            </div>
          }
        >
          <p className="note">
            {(saveImportPrompt.previewName?.trim() || "This loadout") +
              (saveImportPrompt.modCount != null
                ? ` (${saveImportPrompt.modCount} mod${saveImportPrompt.modCount === 1 ? "" : "s"})`
                : "")}
            . Saving keeps the code in Collections so you can install it again later.
          </p>
        </Modal>
      )}
      {collectionImportOpen && (
        <Modal
          className="modal-collection-import"
          title="Import collection"
          onClose={closeCollectionImport}
          footer={
            <div className="modal-foot-actions">
              <button
                type="button"
                className="linkish"
                onClick={closeCollectionImport}
              >
                Cancel
              </button>
              <button type="button" onClick={() => void openCollectionImportFile()}>
                Open file
              </button>
              <button
                type="button"
                onClick={() => void saveImportedCollectionCode(collectionImportCode)}
              >
                Save
              </button>
            </div>
          }
        >
          <label>
            Code
            <textarea
              rows={4}
              value={collectionImportCode}
              onChange={(e) => setCollectionImportCode(e.target.value)}
              placeholder="Paste an Emperor share code"
            />
          </label>
          {error ? <p className="error-banner">{error}</p> : null}
        </Modal>
      )}
      {sharingTarget && (
        <CollectionSharingModal
          collectionId={sharingTarget.id}
          collectionName={sharingTarget.name}
          initialVisibility={sharingTarget.visibility}
          friends={friendsState?.friends ?? []}
          onSave={async (visibility) => {
            try {
              const next = await api.setCollectionSharing(
                sharingTarget.id,
                visibility,
              );
              setFriendsState(next);
            } catch (e) {
              setError(String(e));
            }
          }}
          onClose={() => setSharingTarget(null)}
        />
      )}
      {nexusLoginOpen && (
        <Modal
          className="modal-nexus-login"
          title={
            webLoginProvider === "modio"
              ? "Log in to mod.io"
              : webLoginProvider === "nexus-key"
                ? "Complete Nexus API key"
                : "Log in to Nexus"
          }
          subtitle={
            webLoginProvider === "modio"
              ? "Sign in to your mod.io account. This window closes automatically once your API key is retrieved."
              : webLoginProvider === "nexus-key"
                ? "Complete any prompt below (e.g. cookie accept or key confirmation) — this window will close automatically once your key appears."
                : "Use Stay signed in. This window closes after Nexus accepts the login."
          }
          onClose={closeNexusLogin}
          footer={
            <div className="modal-foot-actions">
              <button
                type="button"
                className="linkish"
                onClick={closeNexusLogin}
                disabled={nexusLoginConfirming}
              >
                Cancel
              </button>
              {webLoginProvider === "nexus" && (
                <button
                  type="button"
                  onClick={() => void confirmNexusLogin()}
                  disabled={nexusLoginConfirming}
                >
                  {nexusLoginConfirming ? "Checking…" : "I've signed in"}
                </button>
              )}
            </div>
          }
        >
          <div ref={nexusLoginHostRef} className="nexus-login-host">
            <p className="note">
              {webLoginProvider === "modio" ? "Opening mod.io…" : "Opening Nexus…"}
            </p>
          </div>
        </Modal>
      )}
      {catalogMatchPrompt && (
        <CatalogMatchDialog
          game={catalogMatchPrompt.game}
          engine={catalogMatchPrompt.engine}
          suggestion={catalogMatchSuggestion}
          lookingUp={catalogMatchLookingUp}
          projectName={catalogMatchProject}
          busy={Boolean(busy)}
          onClose={closeCatalogMatchPrompt}
          onSubmit={(values) => void confirmCatalogMatch(values)}
        />
      )}
      {quickInstall && (
        <QuickInstallDialog
          modName={quickInstall.hit.name}
          mains={quickInstall.mains}
          optionals={quickInstall.optionals}
          installLabel={isPremium ? "Install" : "Download Assist"}
          busy={Boolean(busy)}
          onClose={() => setQuickInstall(null)}
          onInstall={(main, extras) => void confirmQuickInstall(main, extras)}
        />
      )}
      {requirementsModal && (
        <RequirementsDialog
          requirements={requirementsModal}
          disabled={modalDisabledRequirements}
          ignored={ignoredRequirements}
          versions={versionWarnings}
          isPremium={isPremium}
          busy={Boolean(busy) || ignoreSaving}
          onClose={() => setRequirementsModal(null)}
          onAddSelected={(rows) => void queueSelectedRequirements(rows)}
          onEnable={(ids) => void enableStagedMods(ids)}
          onDisable={(ids) => void disableStagedMods(ids)}
          onIgnore={ignoreRequirement}
          onIgnoreMany={ignoreRequirements}
        />
      )}
      {ignoredRequirementsOpen && ignoredRequirements.length > 0 && (
        <IgnoredRequirementsDialog
          ignored={ignoredRequirements}
          busy={Boolean(busy) || ignoreSaving}
          onClose={() => setIgnoredRequirementsOpen(false)}
          onUnignore={unignoreRequirement}
        />
      )}
      {libraryDetail && gameModal === "info" && (
        <Modal
          title={`${libraryDetail.game.title} — Info`}
          className="modal-game-detail"
          onClose={() => setGameModal(null)}
        >
          <h2>About</h2>
          <dl className="meta">
            <dt>Launcher</dt>
            <dd>{libraryDetail.game.launcher}</dd>
            <dt>Nexus domain</dt>
            <dd>{libraryDetail.game.nexus_domain || "—"}</dd>
            {libraryDetail.game.thunderstore_community && (
              <>
                <dt>Thunderstore</dt>
                <dd>{libraryDetail.game.thunderstore_community}</dd>
              </>
            )}
            {libraryDetail.game.modio_game_id != null &&
              libraryDetail.game.modio_game_id > 0 && (
              <>
                <dt>mod.io game ID</dt>
                <dd>{libraryDetail.game.modio_game_id}</dd>
              </>
            )}
            <dt>Install</dt>
            <dd>
              <button
                type="button"
                className="linkish path-link"
                title="Open in file manager"
                onClick={() =>
                  openPathInExplorer(libraryDetail.game.install_path)
                }
              >
                {libraryDetail.game.install_path}
              </button>
            </dd>
            <dt>Plugin</dt>
            <dd>{libraryDetail.game.plugin_id}</dd>
            {libraryDetail.game.project_name && (
              <>
                <dt>UE project</dt>
                <dd>{libraryDetail.game.project_name}</dd>
              </>
            )}
            {gameInfo?.genre && (
              <>
                <dt>Genre</dt>
                <dd>{gameInfo.genre}</dd>
              </>
            )}
            {gameInfo?.mods != null && (
              <>
                <dt>Nexus mods</dt>
                <dd>{gameInfo.mods.toLocaleString()}</dd>
              </>
            )}
            {gameInfo?.file_count != null && (
              <>
                <dt>Nexus files</dt>
                <dd>{gameInfo.file_count.toLocaleString()}</dd>
              </>
            )}
            {gameInfo?.downloads != null && (
              <>
                <dt>Downloads</dt>
                <dd>{gameInfo.downloads.toLocaleString()}</dd>
              </>
            )}
          </dl>
          {settings?.platform_linux && wineAdvice && (
            <div style={{ marginTop: "1rem" }}>
              <WineOverrideNotice
                advice={wineAdvice}
                busy={wineApplyBusy}
                onApply={() => void applyWineOverrides()}
              />
            </div>
          )}
          <div className="panel" style={{ marginTop: "1rem" }}>
            <h3>Catalog & deploy settings</h3>
            <p className="note">
              Override Nexus domain, Thunderstore community, mod.io game ID,
              or Unreal project folder when auto-detect is wrong. At least one
              catalog source is required to browse.
            </p>
            <label>
              Nexus domain
              <input
                value={ueDomainEdit}
                onChange={(e) => setUeDomainEdit(e.target.value)}
                placeholder="optional if Thunderstore/mod.io only"
              />
            </label>
            <label>
              Thunderstore community
              <input
                value={tsCommunityEdit}
                onChange={(e) => setTsCommunityEdit(e.target.value)}
                placeholder="e.g. lethal-company, valheim"
              />
            </label>
            <label>
              mod.io game ID
              <input
                value={modioGameIdEdit}
                onChange={(e) => setModioGameIdEdit(e.target.value)}
                placeholder="numeric id from mod.io"
                inputMode="numeric"
              />
            </label>
            {isUnrealPlugin(libraryDetail.game.plugin_id) && (
              <label>
                Project folder
                <input
                  value={ueProjectEdit}
                  onChange={(e) => setUeProjectEdit(e.target.value)}
                  placeholder="e.g. Pal, Phoenix, Stalker2"
                />
              </label>
            )}
            <button type="button" onClick={() => void saveUeGameSettings()}>
              Save
            </button>
          </div>
        </Modal>
      )}
      {libraryDetail && gameModal === "collections" && (
        <Modal
          title={
            gameSavedDetail
              ? gameSavedDetail.entry.name
              : `${libraryDetail.game.title} — Collections`
          }
          className="modal-game-detail"
          onClose={() => {
            gameSavedOpenRef.current += 1;
            setGameSavedDetail(null);
            setGameSavedDetailPending(false);
            setShareImportOpen(false);
            setGameModal(null);
          }}
        >
          <div className="game-saved-collections">
            {gameSavedDetail ? (
              <>
                <div className="panel-head-inline">
                  <div className="detail-head-title">
                    <button
                      type="button"
                      onClick={() => {
                        gameSavedOpenRef.current += 1;
                        setGameSavedDetail(null);
                        setGameSavedDetailPending(false);
                      }}
                    >
                      Back
                    </button>
                    <EditableCollectionTitle
                      title={gameSavedDetail.entry.name}
                      onRename={(nextName) =>
                        renameSavedCollectionById(
                          gameSavedDetail.entry.id,
                          nextName,
                        )
                      }
                    />
                    {isSavedCollectionInstalled(gameSavedDetail.entry) ? (
                      <span className="pill ok">Installed</span>
                    ) : null}
                  </div>
                  <div className="panel-head-actions">
                    <button
                      type="button"
                      className="danger"
                      aria-label={`Delete ${gameSavedDetail.entry.name}`}
                      onClick={() =>
                        void (async () => {
                          if (
                            !window.confirm(
                              `Delete ${gameSavedDetail.entry.name}?`,
                            )
                          ) {
                            return;
                          }
                          try {
                            await deleteSavedCollectionEntry(
                              gameSavedDetail.entry,
                            );
                            gameSavedOpenRef.current += 1;
                            setGameSavedDetail(null);
                            setGameSavedDetailPending(false);
                          } catch (e) {
                            setError(String(e));
                          }
                        })()
                      }
                    >
                      Delete
                    </button>
                  </div>
                </div>
                <CollectionStats entry={gameSavedDetail.entry} />
                {showGameVersionWarnings &&
                  savedCollectionAdvice[gameSavedDetail.entry.id]?.status ===
                    "mismatch" && (
                    <p className="mod-version-warn">
                      {savedCollectionAdvice[gameSavedDetail.entry.id]?.message}
                    </p>
                  )}
                <div className="saved-collection-actions">
                  <button
                    type="button"
                    onClick={() => void installGameSavedCollection()}
                  >
                    Install into {libraryDetail.game.title}
                  </button>
                  <button
                    type="button"
                    className="icon-btn"
                    aria-label="Copy code"
                    title="Copy code"
                    onClick={() =>
                      void navigator.clipboard.writeText(
                        gameSavedDetail.entry.code,
                      )
                    }
                  >
                    <FontAwesomeIcon icon={faCopy} />
                  </button>
                </div>
                <SavedCollectionDetailBody
                  entry={gameSavedDetail.entry}
                  mods={gameSavedDetail.mods}
                  showMods={showGameSavedMods}
                  scrollRef={mainScrollRef}
                  active={tab === "library"}
                  onOpenMod={(hit) =>
                    void openCollectionMod(hit, "library", libraryDetail.game.id)
                  }
                />
              </>
            ) : savedCollectionsReady ? (
              <>
                {installedCollections.length > 0 && (
                  <div className="installed-collections">
                    <h2>Installed collections</h2>
                    <ul className="list">
                      {installedCollections.map((c, idx) => {
                        const installedLabel = formatInstalledAt(
                          c.installed_at,
                        );
                        const syncState = friendSyncStates[c.id];
                        const hasUpdate = Boolean(syncState?.has_update);
                        const isSyncing = Boolean(syncingCollectionIds[c.id]);
                        const isSaving = Boolean(savingCollectionIds[c.id]);
                        const autoSyncOn = Boolean(c.friend_sync?.auto_sync);
                        const autoSaveOn = Boolean(c.auto_save);
                        const hasUnsaved = Boolean(c.has_unsaved_changes);
                        const installedMemberCount = mods.filter((m) =>
                          (m.collection_ids ?? []).includes(c.id),
                        ).length;
                        const modCount =
                          installedMemberCount > 0
                            ? installedMemberCount
                            : c.mod_ids.length;
                        const canUpdateSource = Boolean(
                          c.friend_sync ||
                            (c.source === "nexus" && c.slug) ||
                            (c.source === "thunderstore" &&
                              c.kind === "modpack" &&
                              c.community &&
                              c.namespace &&
                              c.package_name),
                        );
                        return (
                          <li
                            key={c.id}
                            className="stagger-item"
                            style={staggerStyle(idx)}
                          >
                            <div>
                              <EditableCollectionTitle
                                as="strong"
                                title={c.name}
                                onRename={(nextName) =>
                                  renameInstalledCollectionItem(c, nextName)
                                }
                              />
                              <small>
                                {installedCollectionSourceLabel(c)}
                                {` · ${modCount} mod${modCount === 1 ? "" : "s"}`}
                                {installedLabel
                                  ? ` · ${installedLabel}`
                                  : ""}
                                {hasUnsaved && !autoSaveOn ? (
                                  <>
                                    {" · "}
                                    <span className="installed-collection-sync-pill is-update">
                                      Unsaved changes
                                    </span>
                                  </>
                                ) : null}
                                {c.friend_sync ? (
                                  <>
                                    {" · "}
                                    <span
                                      className={
                                        hasUpdate
                                          ? "installed-collection-sync-pill is-update"
                                          : "installed-collection-sync-pill"
                                      }
                                    >
                                      {hasUpdate ? "Update available" : "Synced"}
                                    </span>
                                  </>
                                ) : null}
                                {collectionIntegrityById[c.id]?.issues.length ? (
                                  <>
                                    {" · "}
                                    <button
                                      type="button"
                                      className="installed-collection-issue-pill"
                                      onClick={() =>
                                        setIntegrityDialogReport(
                                          collectionIntegrityById[c.id],
                                        )
                                      }
                                    >
                                      {collectionIntegrityById[c.id].issues.length}{" "}
                                      issue
                                      {collectionIntegrityById[c.id].issues.length === 1
                                        ? ""
                                        : "s"}
                                    </button>
                                  </>
                                ) : null}
                              </small>
                            </div>
                            <div className="actions">
                              {!autoSaveOn && (
                                <button
                                  type="button"
                                  className={hasUnsaved ? "accent" : ""}
                                  disabled={isSaving}
                                  title={
                                    hasUnsaved
                                      ? `Save load order, member mods, and FOMOD options for ${c.name}`
                                      : `Save current snapshot of ${c.name}`
                                  }
                                  onClick={() => void saveCollectionChanges(c)}
                                >
                                  {isSaving ? "Saving…" : "Save"}
                                </button>
                              )}
                              <button
                                type="button"
                                role="switch"
                                aria-checked={autoSaveOn}
                                disabled={isSaving}
                                className="collection-autosave-toggle"
                                title="Automatically save changes to load order, member mods, and FOMOD options"
                                onClick={() =>
                                  void toggleInstalledCollectionAutoSave(
                                    c,
                                    !autoSaveOn,
                                  )
                                }
                              >
                                <span
                                  className="switch-track"
                                  aria-hidden="true"
                                >
                                  <span className="switch-thumb" />
                                </span>
                                <span>Auto-save</span>
                              </button>
                              {canUpdateSource && (
                                <button
                                  type="button"
                                  className={hasUpdate ? "accent" : ""}
                                  disabled={isSyncing}
                                  onClick={() =>
                                    void syncInstalledCollection(c)
                                  }
                                >
                                  {isSyncing
                                    ? "Syncing…"
                                    : c.friend_sync
                                      ? hasUpdate
                                        ? "Sync Update"
                                        : "Sync"
                                      : "Update"}
                                </button>
                              )}
                              {c.friend_sync && (
                                <button
                                  type="button"
                                  className={autoSyncOn ? "active" : ""}
                                  aria-pressed={autoSyncOn}
                                  title={
                                    autoSyncOn
                                      ? "Auto-sync enabled (click to disable)"
                                      : "Enable auto-sync when friend updates this collection"
                                  }
                                  onClick={() =>
                                    void toggleInstalledCollectionAutoSync(
                                      c,
                                      !autoSyncOn,
                                    )
                                  }
                                >
                                  Auto
                                </button>
                              )}
                              <button
                                type="button"
                                className="danger"
                                onClick={() =>
                                  setUninstallCollectionTarget(c)
                                }
                              >
                                Uninstall
                              </button>
                            </div>
                          </li>
                        );
                      })}
                    </ul>
                  </div>
                )}
                <div className="panel-head-inline">
                  <h2>Saved share codes</h2>
                  <button
                    type="button"
                    className={shareImportOpen ? "active" : ""}
                    onClick={() => setShareImportOpen((v) => !v)}
                  >
                    Import code
                  </button>
                </div>
                <Reveal
                  show={shareImportOpen}
                  className="profile-import share-panel"
                >
                  <h2>Import code</h2>
                  <label>
                    Code
                    <textarea
                      rows={4}
                      value={shareImportCode}
                      onChange={(e) => setShareImportCode(e.target.value)}
                      placeholder="Paste Emperor share code or r2modman / Gale profile code"
                    />
                  </label>
                  <div className="actions">
                    <button
                      type="button"
                      onClick={() => {
                        const code = shareImportCode.trim();
                        if (!code || !activeGame) return;
                        void beginCodeImport(activeGame.id, code);
                      }}
                    >
                      Import
                    </button>
                    <button
                      type="button"
                      onClick={() => void openShareCodeFile()}
                    >
                      Open file
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        const code = shareImportCode.trim();
                        if (!code) return;
                        void (async () => {
                          try {
                            await api.saveCollectionCode({
                              code,
                              sourceGameId: activeGame?.id,
                            });
                            setNotice({
                              kind: "ok",
                              message: "Saved to Collections.",
                            });
                            setShareImportCode("");
                            setShareImportOpen(false);
                            await refreshSavedCollections();
                          } catch (e) {
                            setError(String(e));
                          }
                        })();
                      }}
                    >
                      Save only
                    </button>
                    <button
                      type="button"
                      className="linkish"
                      onClick={() => setShareImportOpen(false)}
                    >
                      Cancel
                    </button>
                  </div>
                </Reveal>
                <p className="note">
                  Emperor share codes that match this game (Nexus domain,
                  Thunderstore community, mod.io ID, or source game). Create
                  one using Save loadout, or paste a code using Import
                  code → Save only.
                </p>
                <ul className="list">
                  {gameSavedCollections.map((c, idx) => (
                    <li
                      key={c.id}
                      className="list-row-clickable stagger-item"
                      style={staggerStyle(idx)}
                      onClick={() => void openGameSavedCollection(c.id)}
                    >
                      <div className="list-row-main">
                        <EditableCollectionTitle
                          as="strong"
                          stopRowClick
                          title={c.name}
                          onRename={(nextName) =>
                            renameSavedCollectionById(c.id, nextName)
                          }
                        />
                        <small>
                          {c.mod_count} mods
                          {c.game.nexus_domain
                            ? ` · ${c.game.nexus_domain}`
                            : ""}
                          {c.game.thunderstore_community
                            ? ` · ${c.game.thunderstore_community}`
                            : ""}
                          {c.game.modio_game_id
                            ? ` · mod.io ${c.game.modio_game_id}`
                            : ""}
                          {formatIsoDate(c.created_at)
                            ? ` · ${formatIsoDate(c.created_at)}`
                            : ""}
                        </small>
                      </div>
                      <div
                        className="actions"
                        onClick={(e) => e.stopPropagation()}
                      >
                        <button
                          type="button"
                          className="icon-btn"
                          aria-label={`Copy ${c.name}`}
                          title="Copy"
                          onClick={() =>
                            void navigator.clipboard.writeText(c.code)
                          }
                        >
                          <FontAwesomeIcon icon={faCopy} />
                        </button>
                        <button
                          type="button"
                          className="danger icon-btn"
                          aria-label={`Delete ${c.name}`}
                          title="Delete"
                          onClick={() =>
                            void (async () => {
                              if (!window.confirm(`Delete ${c.name}?`)) {
                                return;
                              }
                              try {
                                await deleteSavedCollectionEntry(c);
                              } catch (err) {
                                setError(String(err));
                              }
                            })()
                          }
                        >
                          <FontAwesomeIcon icon={faTrashCan} />
                        </button>
                      </div>
                    </li>
                  ))}
                  {gameSavedCollections.length === 0 && (
                    <li className="empty">
                      No saved share codes for this game yet. Share a loadout
                      from Mods or save a code with Import code.
                    </li>
                  )}
                </ul>
              </>
            ) : (
              <BrowseListSkeleton count={6} />
            )}
          </div>
        </Modal>
      )}
      {libraryDetail && gameModal === "tools" && settings?.platform_linux && (
        <Modal
          title={`${libraryDetail.game.title} — Tools`}
          className="modal-game-detail"
          onClose={() => setGameModal(null)}
        >
          <GameToolsPanel game={libraryDetail.game} />
        </Modal>
      )}
      {libraryDetail && gameModal === "launch" && (
        <Modal
          title={`${libraryDetail.game.title} — Launch settings`}
          className="modal-game-detail"
          onClose={() => setGameModal(null)}
          footer={
            <div className="modal-foot-actions">
              <button type="button" onClick={() => setGameModal(null)}>
                Done
              </button>
            </div>
          }
        >
          <div className="launch-settings-modal">
            <div className="setting-block">
              <span>Launch commands</span>
              <p className="note">
                Choose whether Emperor applies required mod overrides (such as{" "}
                <code>WINEDLLOVERRIDES</code> for Proton/Wine proxy DLLs) and
                custom options when launching modded, or defers to{" "}
                {shortLauncher(libraryDetail.game.launcher)}
                {"'"}s own launch options.
              </p>
              <div className="actions">
                <button
                  type="button"
                  className={
                    (libraryDetail.game.launch_command_source ?? "emperor") === "emperor"
                      ? "accent"
                      : ""
                  }
                  onClick={() => void setGameLaunchCommandSource("emperor")}
                >
                  Emperor launch commands
                </button>
                <button
                  type="button"
                  className={
                    libraryDetail.game.launch_command_source === "launcher"
                      ? "accent"
                      : ""
                  }
                  onClick={() => void setGameLaunchCommandSource("launcher")}
                >
                  {shortLauncher(libraryDetail.game.launcher)} launch commands
                </button>
              </div>
            </div>

            {launchPreview && (
              <>
                <div className="setting-block">
                  <span>Wrapper &amp; Arguments</span>
                  <div className="launch-wrapper-row">
                    <input
                      type="text"
                      placeholder="Wrapper (e.g. gamemoderun)"
                      value={wrapperDraft}
                      onChange={(e) => setWrapperDraft(e.target.value)}
                    />
                    <input
                      type="text"
                      placeholder="Arguments (e.g. --launcher-skip)"
                      value={argsDraft}
                      onChange={(e) => setArgsDraft(e.target.value)}
                    />
                    <button
                      type="button"
                      onClick={() =>
                        void saveLaunchOptions(
                          libraryDetail.game.launch_command_source ?? "emperor",
                          launchPreview.custom_env_vars,
                          wrapperDraft,
                          argsDraft,
                        )
                      }
                    >
                      Save
                    </button>
                  </div>
                </div>

                <div className="setting-block">
                  <span>Environment Variables</span>
                  <div className="launch-env-list">
                    {launchPreview.auto_env_vars.map((v) => (
                      <div className="launch-env-row" key={`auto-${v.name}`}>
                        <code className="launch-env-name">{v.name}</code>
                        <span className="launch-env-eq">=</span>
                        <code className="launch-env-val">{v.value}</code>
                        <span className="launch-env-badge">Auto</span>
                        <button
                          type="button"
                          className="icon-btn"
                          title="Copy"
                          onClick={() =>
                            void navigator.clipboard?.writeText(`${v.name}=${v.value}`)
                          }
                        >
                          <FontAwesomeIcon icon={faCopy} />
                        </button>
                      </div>
                    ))}
                    {launchPreview.custom_env_vars.map((v, idx) => (
                      <div className="launch-env-row" key={`custom-${v.name}-${idx}`}>
                        <code className="launch-env-name">{v.name}</code>
                        <span className="launch-env-eq">=</span>
                        <code className="launch-env-val">{v.value}</code>
                        <button
                          type="button"
                          className="icon-btn"
                          title="Copy"
                          onClick={() =>
                            void navigator.clipboard?.writeText(`${v.name}=${v.value}`)
                          }
                        >
                          <FontAwesomeIcon icon={faCopy} />
                        </button>
                        <button
                          type="button"
                          className="icon-btn danger"
                          title="Remove variable"
                          onClick={() => {
                            const next = launchPreview.custom_env_vars.filter(
                              (_, i) => i !== idx,
                            );
                            void saveLaunchOptions(
                              libraryDetail.game.launch_command_source ?? "emperor",
                              next,
                              wrapperDraft,
                              argsDraft,
                            );
                          }}
                        >
                          <FontAwesomeIcon icon={faTrashCan} />
                        </button>
                      </div>
                    ))}
                    {launchPreview.auto_env_vars.length === 0 &&
                      launchPreview.custom_env_vars.length === 0 && (
                        <p className="note">No environment variables configured.</p>
                      )}
                  </div>

                  <div className="launch-env-add">
                    <input
                      type="text"
                      placeholder="NAME"
                      value={newEnvName}
                      onChange={(e) => setNewEnvName(e.target.value)}
                    />
                    <span className="launch-env-eq">=</span>
                    <input
                      type="text"
                      placeholder="VALUE"
                      value={newEnvValue}
                      onChange={(e) => setNewEnvValue(e.target.value)}
                    />
                    <button
                      type="button"
                      className="accent"
                      aria-label="Add environment variable"
                      disabled={!newEnvName.trim()}
                      onClick={() => {
                        const name = newEnvName.trim();
                        if (!name) return;
                        const next = launchPreview.custom_env_vars.filter(
                          (v) => v.name !== name,
                        );
                        next.push({ name, value: newEnvValue.trim() });
                        setNewEnvName("");
                        setNewEnvValue("");
                        void saveLaunchOptions(
                          libraryDetail.game.launch_command_source ?? "emperor",
                          next,
                          wrapperDraft,
                          argsDraft,
                        );
                      }}
                    >
                      <FontAwesomeIcon icon={faPlus} />
                    </button>
                  </div>
                </div>

                <div className="setting-block">
                  <span>Applied launch options preview</span>
                  <pre className="launch-preview-code">
                    <code>{launchPreview.effective_command}</code>
                  </pre>
                </div>
              </>
            )}
          </div>
        </Modal>
      )}
      {unmanageTarget && (
        <Modal
          title={`Unmanage ${unmanageTarget.title}?`}
          onClose={() => setUnmanageTarget(null)}
          footer={
            <div className="modal-foot-actions">
              <button type="button" onClick={() => setUnmanageTarget(null)}>
                Cancel
              </button>
              {unmanageStatus?.has_portable_mods &&
                !unmanageStatus.already_saved && (
                  <button
                    type="button"
                    onClick={() => {
                      const target = unmanageTarget;
                      setUnmanageTarget(null);
                      void unmanageFromLibrary(target, true);
                    }}
                  >
                    Save and delete
                  </button>
                )}
              <button
                type="button"
                className="danger"
                onClick={() => {
                  const target = unmanageTarget;
                  setUnmanageTarget(null);
                  void unmanageFromLibrary(target, false);
                }}
              >
                Delete
              </button>
            </div>
          }
        >
          <p className="note">
            Are you sure you want to unmanage {unmanageTarget.title}? Emperor
            will purge deployed mods from the game folder and permanently delete
            its staged mods and mod data from disk.
          </p>
          {unmanageStatusLoading ? (
            <p className="note">Checking mod collection…</p>
          ) : (
            unmanageStatus?.has_portable_mods &&
            !unmanageStatus.already_saved && (
              <p className="note">
                Do you wish to save this collection (
                {unmanageStatus.mod_count} mod
                {unmanageStatus.mod_count === 1 ? "" : "s"}) to your Personal
                Collections before deleting?
              </p>
            )
          )}
        </Modal>
      )}
      {uninstallCollectionTarget && (
        <Modal
          title={`Uninstall ${uninstallCollectionTarget.name}?`}
          onClose={() => setUninstallCollectionTarget(null)}
          footer={
            <div className="modal-foot-actions">
              <button
                type="button"
                onClick={() => setUninstallCollectionTarget(null)}
              >
                Cancel
              </button>
              <button
                type="button"
                className="danger"
                onClick={() => {
                  const target = uninstallCollectionTarget;
                  setUninstallCollectionTarget(null);
                  void uninstallInstalledCollection(target);
                }}
              >
                Uninstall
              </button>
            </div>
          }
        >
          <p className="note">
            Are you sure you want to uninstall {uninstallCollectionTarget.name}?
            Mods only used by this collection will be removed. Shared
            requirements stay.
          </p>
        </Modal>
      )}
      <div className="toast-stack">
        <UpdateToast
          show={
            Boolean(appUpdate?.update_available && appUpdate.latest_version) &&
            dismissedUpdateVersion !== appUpdate?.latest_version &&
            tab != null &&
            tab !== "settings" &&
            tab !== "setup"
          }
          version={appUpdate?.latest_version ?? ""}
          progress={installingAppUpdate ? appUpdateMessage : null}
          canInstall={Boolean(appUpdate?.asset_name)}
          checking={checkingAppUpdate}
          installing={installingAppUpdate}
          onUpdate={() => void installAppUpdate()}
          onOpenRelease={() => {
            if (appUpdate?.release_url) void openUrl(appUpdate.release_url);
          }}
          onLater={() => {
            if (appUpdate?.latest_version) setDismissedUpdateVersion(appUpdate.latest_version);
          }}
        />
        <CheckingUpdatesToast
          show={checkingModUpdates}
          stagedCount={checkingModUpdates && mods.length > 0 ? mods.length : null}
        />
        <CheckingFriendUpdatesToast show={checkingFriendUpdates} />
      </div>
      {openFomod && activeGame && (
        <FomodDialog
          key={openFomod.mod.id}
          gameId={activeGame.id}
          mod={openFomod.mod}
          progress={openFomod.pass ? fomodPassProgress(openFomod.pass) : null}
          onClose={closeFomodDialog}
          onSaved={(selection) => {
            const savedId = fomodDialogRef.current?.mod.id;
            if (!savedId) return;
            setMods((prev) =>
              prev.map((m) =>
                m.id === savedId ? { ...m, fomod_selection: selection } : m,
              ),
            );
            setFomodNeedingIds((ids) => ids.filter((id) => id !== savedId));
            void refreshCollections(activeGame.id);
          }}
        />
      )}
      {modOptionsFor && activeGame && (
        <ModOptionsDialog
          gameId={activeGame.id}
          mod={modOptionsFor}
          onClose={() => setModOptionsFor(null)}
          onSaved={(selection) => {
            setMods((prev) =>
              prev.map((m) =>
                m.id === modOptionsFor.id ? { ...m, option_selection: selection } : m,
              ),
            );
            setNotice({
              kind: "ok",
              message: `Saved options for ${modOptionsFor.name}. Launch modded to apply them.`,
            });
            void refreshCollections(activeGame.id);
          }}
        />
      )}
      {moveCollectionMod && activeGame && installedCollections.length > 1 && (
        <MoveModCollectionDialog
          mod={moveCollectionMod}
          collections={installedCollections}
          busy={moveCollectionBusy}
          onClose={() => {
            if (!moveCollectionBusy) setMoveCollectionMod(null);
          }}
          onSave={async (collectionIds) => {
            setMoveCollectionBusy(true);
            try {
              await api.setModCollections(
                activeGame.id,
                moveCollectionMod.id,
                collectionIds,
              );
              await refreshMods(activeGame.id);
              await refreshCollections(activeGame.id);
              setMoveCollectionMod(null);
            } catch (e) {
              setError(String(e));
            } finally {
              setMoveCollectionBusy(false);
            }
          }}
        />
      )}
      {integrityDialogReport && activeGame && (
        <CollectionIntegrityDialog
          report={integrityDialogReport}
          retrying={integrityRetrying}
          onRetry={(selected) =>
            void retryCollectionIssues(integrityDialogReport, selected)
          }
          onClose={() => {
            if (!integrityRetrying) setIntegrityDialogReport(null);
          }}
        />
      )}
    </div>
  );
}
