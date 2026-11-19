import {
  Fragment,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
} from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowDown,
  faArrowUp,
  faBars,
  faCopy,
  faFilter,
  faGear,
  faLink,
  faMagnifyingGlass,
  faPlus,
  faTrashCan,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import { motion, useReducedMotion } from "motion/react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { openUrl, revealItemInDir } from "@tauri-apps/plugin-opener";
import appLogo from "./assets/app-logo.png";
import { AccountConnections } from "./AccountConnections";
import { SettingsSection } from "./SettingsSection";
import { UpdateToast } from "./UpdateToast";
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
import { ImageCarousel } from "./ImageCarousel";
import {
  FadeIn,
  MOTION_EASE,
  MOTION_FAST,
  MotionCard,
  NavTab,
  Collapse,
  PopIn,
  Reveal,
  Segment,
  SegmentTab,
  ViewSwitch,
} from "./motion";
import { RichText } from "./RichText";
import { BrowseVirtualGrid, BrowseVirtualList } from "./BrowseResults";
import { estimateListRowHeight } from "./browseVirtual";
import { AppNavHistory, type NavEntry } from "./appNavHistory";
import { attachMouseNavigationListeners } from "./mouseNavigation";
import { browseGameVersionOptions } from "./gameVersion";
import {
  catalogHitFromModDeepLink,
  managedGameForModDeepLink,
  modDeepLinkFromHit,
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
import { FomodDialog } from "./FomodDialog";
import { ModOptionsDialog } from "./ModOptionsDialog";
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
  deployStatusNotice,
  type BannerAction,
  type StatusNotice,
} from "./StatusBanner";
import { WineOverrideNotice } from "./WineOverrideNotice";
import type {
  AuthorSuggestion,
  BrowseMeta,
  BrowseSearchOpts,
  CatalogHit,
  CatalogSuggestion,
  CollectionDetail,
  CollectionHit,
  CollectionModFile,
  DetectedGame,
  DownloadItem,
  GameHealth,
  GameInfo,
  GameVersionReport,
  IgnoredRequirement,
  InstalledCollection,
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
  SavedCollectionEntry,
  SavedCollectionDetail,
  ShareImportResult,
  ShareModEntry,
  AppInfo,
  AppInstallVia,
  AppUpdateStatus,
} from "./types";
import {
  formatCollectionGameVersions,
  groupSavedCollectionsByGame,
  savedCollectionMatchesGame,
} from "./savedCollectionGroups";
import "./App.css";

type Tab = "setup" | "library" | "collections" | "browse" | "downloads" | "tools" | "settings";
type ViewMode = "list" | "grid";
type BrowseListDensity = "compact" | "posters";
type DetailTab = "info" | "files";
type GameDetailTab = "info" | "mods" | "collections" | "tools";
type BrowseTagMap = Record<string, TagFilterState>;

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

function catalogMatchActionLabel(kind: CatalogMatchCardKind): string {
  if (kind === "ready") return "Manage";
  if (kind === "review") return "Choose catalog";
  return "Find catalog";
}

function catalogHintTags(s: CatalogSuggestion | null | undefined): string[] {
  if (!s) {
    return [];
  }
  const tags: string[] = [];
  if (s.nexus_name || s.nexus_domain) {
    tags.push(`Nexus: ${s.nexus_name ?? s.nexus_domain}`);
  }
  if (s.thunderstore_name || s.thunderstore_community) {
    tags.push(`TS: ${s.thunderstore_name ?? s.thunderstore_community}`);
  }
  if (s.modio_name || (s.modio_game_id != null && s.modio_game_id > 0)) {
    tags.push(`mod.io: ${s.modio_name ?? String(s.modio_game_id)}`);
  }
  return tags;
}

function catalogHintLine(s: CatalogSuggestion | null | undefined): string {
  return catalogHintTags(s).join(" · ");
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

function shortLauncher(launcher: string): string {
  const l = launcher.toLowerCase();
  if (l.includes("steam")) return "Steam";
  if (l.includes("heroic")) return "Heroic";
  if (l.includes("lutris")) return "Lutris";
  if (l.includes("bottles")) return "Bottles";
  return launcher.split("(")[0]?.trim() || launcher;
}

function PlaceholderIcon() {
  return (
    <div className="media-placeholder" aria-hidden>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="3" y="6" width="18" height="12" rx="2" />
        <path d="M8 12h.01M12 10v4M16 12h.01" strokeLinecap="round" />
      </svg>
    </div>
  );
}

function ListPoster({
  src,
  decoding,
}: {
  src: string | null;
  decoding?: "async";
}) {
  return (
    <div className="list-poster" aria-hidden="true">
      {src ? (
        <img
          src={src}
          alt=""
          loading="lazy"
          decoding={decoding}
          onError={(e) => {
            (e.currentTarget as HTMLImageElement).style.display = "none";
          }}
        />
      ) : (
        <PlaceholderIcon />
      )}
    </div>
  );
}

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

export function NexusModFileList(props: {
  files: ModFileInfo[];
  installLabel: string;
  installedFileIds?: ReadonlySet<number>;
  onInstall: (file: ModFileInfo) => void;
}) {
  const { files, installLabel, installedFileIds, onInstall } = props;
  const current = files.filter((file) => !isHistoricalModFile(file));
  const historical = files.filter((file) => isHistoricalModFile(file));
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
            {historical.map((file) => (
              <NexusFileRow
                key={file.file_id}
                file={file}
                installLabel={installLabel}
                installed={installedFileIds?.has(file.file_id)}
                onInstall={onInstall}
              />
            ))}
          </ul>
        </details>
      )}
    </>
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

function MediaCard(props: {
  title: string;
  imageSrc: string | null;
  badge?: string | null;
  overlay?: string | null;
  tags?: string[];
  coverTags?: string[];
  meta?: string | null;
  installed?: boolean;
  actions?: ReactNode;
  coverActions?: ReactNode;
  coverLabel?: string | null;
  hoverLabel?: string | null;
  unsupported?: boolean;
  onClick?: () => void;
  enterIndex?: number;
  /** Skip the motion wrapper. Browse virtualization mounts cards while scrolling. */
  plain?: boolean;
}) {
  const {
    title,
    imageSrc,
    badge,
    overlay,
    tags,
    coverTags,
    meta,
    installed,
    actions,
    coverActions,
    coverLabel,
    hoverLabel,
    unsupported,
    onClick,
    enterIndex,
    plain,
  } = props;
  const classes = [
    "media-card",
    onClick ? "media-card-clickable" : "",
    unsupported ? "media-card-unsupported" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const onKeyDown = onClick
    ? (event: ReactKeyboardEvent<HTMLElement>) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onClick();
        }
      }
    : undefined;
  const body = (
    <>
      <div className="media-cover">
        {badge ? <span className="media-badge">{badge}</span> : null}
        {installed ? <span className="media-status">Installed</span> : null}
        {imageSrc ? (
          <img
            src={imageSrc}
            alt=""
            loading="lazy"
            decoding={plain ? "async" : undefined}
            onError={(e) => {
              (e.currentTarget as HTMLImageElement).style.display = "none";
            }}
          />
        ) : (
          <PlaceholderIcon />
        )}
        {overlay ? <span className="media-overlay">{overlay}</span> : null}
        {coverTags && coverTags.length > 0 ? (
          <div className="media-cover-tags">
            {coverTags.map((t) => (
              <span key={t} className="media-cover-tag">
                {t}
              </span>
            ))}
          </div>
        ) : null}
        {coverLabel ? (
          <span className="media-cover-label">{coverLabel}</span>
        ) : null}
        {hoverLabel ? (
          <span className="media-cover-label media-cover-hover">{hoverLabel}</span>
        ) : null}
        {coverActions ? (
          <div
            className="media-cover-actions"
            onClick={(e) => e.stopPropagation()}
            onKeyDown={(e) => e.stopPropagation()}
          >
            {coverActions}
          </div>
        ) : null}
      </div>
      <div className="media-card-title">{title}</div>
      {meta ? <div className="media-card-meta">{meta}</div> : null}
      {tags && tags.length > 0 ? (
        <div className="media-card-tags">
          {tags.map((t) => (
            <span key={t} className="pill">
              {t}
            </span>
          ))}
        </div>
      ) : null}
      {actions ? (
        <div className="actions" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
          {actions}
        </div>
      ) : null}
    </>
  );
  if (plain) {
    return (
      <article
        className={classes}
        onClick={onClick}
        onKeyDown={onKeyDown}
        role={onClick ? "button" : undefined}
        tabIndex={onClick ? 0 : undefined}
      >
        {body}
      </article>
    );
  }
  return (
    <MotionCard
      className={classes}
      enterIndex={enterIndex}
      onClick={onClick}
      onKeyDown={onKeyDown}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
    >
      {body}
    </MotionCard>
  );
}

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

export default function App() {
  const [tab, setTabState] = useState<Tab | null>(null);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [appInfo, setAppInfo] = useState<AppInfo | null>(null);
  const [appUpdate, setAppUpdate] = useState<AppUpdateStatus | null>(null);
  const [checkingAppUpdate, setCheckingAppUpdate] = useState(false);
  const [installingAppUpdate, setInstallingAppUpdate] = useState(false);
  const [appUpdateMessage, setAppUpdateMessage] = useState<string | null>(null);
  const [dismissedUpdateVersion, setDismissedUpdateVersion] = useState<string | null>(null);
  const [error, setErrorMessage] = useState<string | null>(null);
  const [errorAction, setErrorAction] = useState<BannerAction | null>(null);
  const setError = useCallback((message: string | null, action?: BannerAction | null) => {
    setErrorMessage(message);
    setErrorAction(action ?? null);
  }, []);
  const [notice, setNotice] = useState<StatusNotice | null>(null);
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
  const [fomodFor, setFomodFor] = useState<StagedMod | null>(null);
  const fomodPromptedRef = useRef<Set<string>>(new Set());
  const [loadDiagnosis, setLoadDiagnosis] = useState<LoadDiagnosis | null>(null);
  const [modsQuery, setModsQuery] = useState("");
  const [modsStatusFilter, setModsStatusFilter] =
    useState<ModsStatusFilter>("all");
  const [modsSourceFilter, setModsSourceFilter] =
    useState<CatalogSourceFilter>("all");
  const [modsSort, setModsSort] = useState<ModsSort>("loadOrder");
  const [modsCollectionFilter, setModsCollectionFilter] = useState("");
  const [modsFiltersOpen, setModsFiltersOpen] = useState(false);
  const [modsActionsMenuOpen, setModsActionsMenuOpen] = useState(false);
  const [installedCollections, setInstalledCollections] = useState<
    InstalledCollection[]
  >([]);
  const [profileImportOpen, setProfileImportOpen] = useState(false);
  const [profileCode, setProfileCode] = useState("");
  const [shareExportOpen, setShareExportOpen] = useState(false);
  const [shareExportName, setShareExportName] = useState("");
  const [shareImportOpen, setShareImportOpen] = useState(false);
  const [shareImportCode, setShareImportCode] = useState("");
  const [collectionImportOpen, setCollectionImportOpen] = useState(false);
  const [collectionImportCode, setCollectionImportCode] = useState("");
  const [saveImportPrompt, setSaveImportPrompt] = useState<SaveImportPrompt | null>(
    null,
  );
  const [lastShareCode, setLastShareCode] = useState<string | null>(null);
  const [savedCollections, setSavedCollections] = useState<SavedCollectionEntry[]>(
    [],
  );
  const [savedDetail, setSavedDetail] = useState<SavedCollectionDetail | null>(
    null,
  );
  const [savedRename, setSavedRename] = useState("");
  const [savedInstallGameId, setSavedInstallGameId] = useState("");
  const [expandedCollectionGameKey, setExpandedCollectionGameKey] = useState<
    string | null
  >(null);
  const [installedEmperorShareIds, setInstalledEmperorShareIds] = useState(
    () => new Set<string>(),
  );
  const [savedEntryShareIds, setSavedEntryShareIds] = useState<
    Record<string, string>
  >({});
  const [savedCollectionAdvice, setSavedCollectionAdvice] = useState<
    Record<string, CollectionVersionAdvice>
  >({});
  const [gamePlugins, setGamePlugins] = useState<GamePluginInfo[]>([]);
  const [gameSavedDetail, setGameSavedDetail] = useState<SavedCollectionDetail | null>(
    null,
  );
  const [gameSavedRename, setGameSavedRename] = useState("");
  const [orphanScan, setOrphanScan] = useState<OrphanScan | null>(null);
  const [downloads, setDownloads] = useState<DownloadItem[]>([]);
  const [searchQuery, setSearchQuery] = useState("");
  const [modHits, setModHits] = useState<CatalogHit[]>([]);
  const [collectionHits, setCollectionHits] = useState<CollectionHit[]>([]);
  const [browseMode, setBrowseMode] = useState<"mods" | "collections">("mods");
  const [sourceFilter, setSourceFilter] = useState<CatalogSourceFilter>("all");
  const [libraryView, setLibraryView] = useState<ViewMode>("grid");
  const [browseView, setBrowseView] = useState<ViewMode>("grid");
  const [browseGridMin, setBrowseGridMin] = useState(160);
  const [browseListDensity, setBrowseListDensity] =
    useState<BrowseListDensity>("compact");
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
  const [tagsOpen, setTagsOpen] = useState(false);
  const [browseTotalCount, setBrowseTotalCount] = useState(0);
  const [browseHasMore, setBrowseHasMore] = useState(false);
  const [browsePageCount, setBrowsePageCount] = useState(24);
  const [browseSearched, setBrowseSearched] = useState(false);
  const [browseLoading, setBrowseLoading] = useState(false);
  const [browseMeta, setBrowseMeta] = useState<BrowseMeta | null>(null);
  const [browseDetail, setBrowseDetail] = useState<BrowseDetail | null>(null);
  const [browseReturnTab, setBrowseReturnTab] = useState<Tab | null>(null);
  const [libraryDetail, setLibraryDetail] = useState<{
    game: ManagedGame;
    tab: GameDetailTab;
  } | null>(null);
  const [gameInfo, setGameInfo] = useState<GameInfo | null>(null);
  const [modDetail, setModDetail] = useState<ModDetail | null>(null);
  const [tsDetail, setTsDetail] = useState<TsPackageDetail | null>(null);
  const [tsVersion, setTsVersion] = useState("");
  const [modioDetail, setModioDetail] = useState<ModioModDetail | null>(null);
  const [modioFiles, setModioFiles] = useState<ModioFileInfo[]>([]);
  const [modFiles, setModFiles] = useState<ModFileInfo[]>([]);
  const [modioKeyInput, setModioKeyInput] = useState("");
  const [modioGameIdEdit, setModioGameIdEdit] = useState("");
  const [collectionModFiles, setCollectionModFiles] = useState<CollectionModFile[]>([]);
  const [collectionDetail, setCollectionDetail] = useState<CollectionDetail | null>(null);
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
  const nexusLoginGenRef = useRef(0);
  const [nexusLoginOpen, setNexusLoginOpen] = useState(false);
  const [nexusLoginConfirming, setNexusLoginConfirming] = useState(false);
  const tabRef = useRef(tab);
  tabRef.current = tab;
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
  const modsActionsMenuRef = useRef<HTMLDivElement | null>(null);
  const mainScrollRef = useRef<HTMLElement | null>(null);
  const browseResultsRef = useRef<HTMLDivElement | null>(null);
  const browseScrollRef = useRef(0);
  const browseScrollEpochRef = useRef(0);
  const browseScrollArmedEpochRef = useRef(0);
  const browseResultsVisibleRef = useRef(false);
  const browsePrefetchErrorRef = useRef<string | null>(null);
  const browseSentinelRef = useRef<HTMLDivElement | null>(null);
  const browseLoadingMoreRef = useRef(false);
  const browseNextOffsetRef = useRef(0);
  const browseAppendFailedRef = useRef(false);
  const browseRequestIdRef = useRef(0);
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
  const activeGameCoverSrc = mediaSrc(activeGame?.cover_path ?? null);
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
  const lastEmperorOpenRef = useRef<{ key: string; at: number } | null>(null);
  const modUpdatesReqRef = useRef(0);
  const modFilesReqRef = useRef(0);
  const modFilesModIdRef = useRef<number | null>(null);
  const wineAdviceReqRef = useRef(0);
  const missingReqRef = useRef(0);
  const gameVersionReqRef = useRef(0);
  const pendingRequirementLinksRef = useRef<Map<string, string[]>>(new Map());
  const pictureFillInFlightRef = useRef(false);
  const pictureFillSkippedRef = useRef(new Set<string>());
  const [pictureFillTick, setPictureFillTick] = useState(0);

  const clearModFiles = useCallback(() => {
    modFilesReqRef.current += 1;
    modFilesModIdRef.current = null;
    setModFiles([]);
  }, []);

  const loadModFiles = useCallback(async (domain: string, modId: number) => {
    const reqId = ++modFilesReqRef.current;
    modFilesModIdRef.current = null;
    setModFiles([]);
    const files = await api.modFiles(domain, modId);
    if (reqId !== modFilesReqRef.current) return;
    modFilesModIdRef.current = modId;
    setModFiles(files);
  }, []);

  const closeBrowseDetail = useCallback(() => {
    setBrowseDetail(null);
    setModDetail(null);
    setTsDetail(null);
    setTsVersion("");
    setModioDetail(null);
    setModioFiles([]);
    clearModFiles();
    setCollectionModFiles([]);
    setCollectionDetail(null);
  }, [clearModFiles]);

  const resetBrowseState = useCallback(() => {
    browseRequestIdRef.current += 1;
    browseNextOffsetRef.current = 0;
    browseLoadingMoreRef.current = false;
    browseAppendFailedRef.current = false;
    browseScrollEpochRef.current += 1;
    browseScrollRef.current = 0;
    browseResultsVisibleRef.current = false;
    browsePrefetchErrorRef.current = null;
    setBrowseLoading(false);
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
      libraryDetail: library
        ? { gameId: library.game.id, tab: library.tab }
        : null,
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
      setGameSavedDetail(null);
      setSavedDetail(null);
      setSavedRename("");
      setBrowseReturnTab(entry.browseReturnTab);

      if (entry.tab !== tabRef.current) {
        setTabState(entry.tab);
      }

      if (entry.libraryDetail) {
        const game = managedRef.current.find((g) => g.id === entry.libraryDetail!.gameId);
        if (game) {
          setLibraryDetail({ game, tab: entry.libraryDetail.tab });
          if (entry.libraryDetail.tab === "collections") {
            void refreshSavedCollections();
          }
          if (game.nexus_domain) {
            try {
              setGameInfo(await api.getGame(game.nexus_domain));
            } catch {
              /* Nexus metadata is optional */
            }
          }
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
    if (fomodFor) {
      setFomodFor(null);
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
    if (modsActionsMenuOpen) {
      setModsActionsMenuOpen(false);
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
    if (shareExportOpen) {
      setShareExportOpen(false);
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
    setShareExportOpen(false);
    setShareExportName("");
    setShareImportOpen(false);
    setShareImportCode("");
    setLastShareCode(null);
    setModsFiltersOpen(false);
    setModsActionsMenuOpen(false);
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

  const collectionNamesByModId = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const m of mods) {
      const names = collectionsForMod(m, installedCollections).map((c) => c.name);
      if (names.length) map.set(m.id, names);
    }
    return map;
  }, [mods, installedCollections]);

  const visibleMods = useMemo(
    () =>
      filterAndSortStagedMods(mods, {
        query: modsQuery,
        status: modsStatusFilter,
        source: modsSourceFilter,
        collectionId: modsCollectionFilter,
        sort: modsSort,
        collectionNamesByModId,
      }),
    [
      mods,
      modsQuery,
      modsStatusFilter,
      modsSourceFilter,
      modsCollectionFilter,
      modsSort,
      collectionNamesByModId,
    ],
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

  useEffect(() => {
    if (unmanaged.length === 0) {
      setCatalogSuggestions({});
      setCatalogLookupBusy(false);
      return;
    }
    const requests = unmanaged.map((g) => ({ id: g.id, title: g.title }));
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
  }, [unmanaged]);

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
    await applyAppearance(
      s.theme ?? "system",
      s.theme_style ?? "app",
      s.accent_color,
    );
    return s;
  }, []);

  const loadAppInfo = useCallback(async () => {
    try {
      setAppInfo(await api.getAppInfo());
    } catch {
      /* ignore */
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
      setGameHealth(health);
      return health;
    } catch {
      setGameHealth([]);
      return [];
    }
  }, []);

  const refreshManaged = useCallback(async () => {
    const list = await api.listManaged();
    setManaged(list);
    void refreshGameHealth();
    return list;
  }, [refreshGameHealth]);

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
    const list = await api.listMods(gameId);
    if (activeGameRef.current?.id !== gameId) return;
    setMods(list);
    // Reading a manifest per mod is a disk walk, so ask once per refresh
    // instead of on every row render.
    try {
      const withOptions = await api.listModsWithOptions(gameId);
      if (activeGameRef.current?.id !== gameId) return;
      setModsWithOptions(new Set(withOptions));
    } catch (e) {
      console.warn("list_mods_with_options failed", e);
    }
    try {
      const needing = await api.listModsNeedingFomod(gameId);
      if (activeGameRef.current?.id !== gameId) return;
      setFomodFor((open) => {
        if (open) return open;
        const nextId = needing.find((id) => !fomodPromptedRef.current.has(id));
        if (!nextId) return null;
        const staged = list.find((m) => m.id === nextId);
        if (!staged) return null;
        fomodPromptedRef.current.add(nextId);
        return staged;
      });
    } catch (e) {
      console.warn("list_mods_needing_fomod failed", e);
    }
    void refreshWineAdvice(gameId);
  }, [refreshWineAdvice]);

  const openModInstaller = useCallback(async (m: StagedMod) => {
    if (!activeGameRef.current) return;
    const gameId = activeGameRef.current.id;
    try {
      const fomod = await api.getFomodInstaller(gameId, m.id);
      if (fomod) {
        setFomodFor(m);
        return;
      }
    } catch (e) {
      console.warn("get_fomod_installer failed", e);
    }
    setModOptionsFor(m);
  }, []);

  const refreshModUpdates = useCallback(async (gameId: string) => {
    const req = ++modUpdatesReqRef.current;
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

  const refreshCollections = useCallback(async (gameId: string) => {
    const list = await api.listInstalledCollections(gameId);
    if (activeGameRef.current?.id !== gameId) return;
    setInstalledCollections(list);
  }, []);

  const refreshOrphanScan = useCallback(async () => {
    const scan = await api.scanModOrphans();
    setOrphanScan(scan);
    return scan;
  }, []);

  const refreshDownloads = useCallback(async () => {
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
  }, []);

  const scan = useCallback(async () => {
    setBusy("Scanning installed games…");
    setError(null);
    setNotice(null);
    try {
      setDetected(await api.scanGames());
      await refreshGameHealth();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }, [refreshGameHealth]);

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
        await refreshManaged();
        await refreshOrphanScan();
        await scan();
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

  useEffect(() => {
    if (tab !== "settings") return;
    void loadAppInfo();
    void checkForAppUpdate(true);
  }, [tab, loadAppInfo, checkForAppUpdate]);

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
      wineAdviceReqRef.current += 1;
      setWineAdvice(null);
      if (prevId !== null) {
        resetBrowseState();
        resetLibraryEphemeralState();
      }
      setSavedDetail(null);
      setSavedRename("");
      setSavedInstallGameId(nextId ?? "");
      setGameSavedDetail(null);
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
    setGameInfo(null);
    if (activeGame.nexus_domain) {
      void api.getGame(activeGame.nexus_domain).then(setGameInfo).catch(() => {
        /* Nexus metadata is optional for local game info */
      });
    }
  }, [activeGame, libraryDetail]);

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

  useEffect(() => {
    const gameId = activeGame?.id;
    if (!gameId || libraryDetail?.tab !== "mods" || pictureFillInFlightRef.current) return;
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
  }, [activeGame?.id, libraryDetail?.tab, mods, pictureFillTick]);

  useEffect(() => {
    if (!activeGame || libraryDetail?.tab !== "mods") return;
    const gameId = activeGame.id;
    let cancelled = false;
    const poll = () => {
      if (cancelled) return;
      void refreshModUpdates(gameId);
    };
    poll();
    const id = window.setInterval(poll, 15 * 60 * 1000);
    return () => {
      cancelled = true;
      window.clearInterval(id);
    };
  }, [activeGame, libraryDetail?.tab, refreshModUpdates]);

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
      return;
    }
    if (libraryDetail?.tab !== "mods") return;
    void refreshMissingRequirements(activeGame.id);
  }, [activeGame, libraryDetail?.tab, nexusModsKey, refreshMissingRequirements]);

  const stagedVersionKey = useMemo(
    () => mods.map((mod) => `${mod.id}:${mod.nexus_mod_id}:${mod.collection_ids?.join(",") ?? ""}`).join("|"),
    [mods],
  );

  useEffect(() => {
    setGameVersionDraft(gameVersionReport?.current ?? "");
  }, [gameVersionReport]);

  useEffect(() => {
    if (!activeGame || libraryDetail?.tab !== "mods") return;
    void refreshGameVersions(activeGame.id);
  }, [activeGame, libraryDetail?.tab, stagedVersionKey, refreshGameVersions]);

  useEffect(() => {
    setModsQuery("");
    setModsStatusFilter("all");
    setModsSourceFilter("all");
    setModsSort("loadOrder");
    setModsCollectionFilter("");
    setModsFiltersOpen(false);
    setModsActionsMenuOpen(false);
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
    if (!activeGame.nexus_domain) {
      setBrowseMeta(null);
      return;
    }
    let cancelled = false;
    (async () => {
      try {
        const meta = await api.browseMeta(activeGame.nexus_domain);
        if (!cancelled) setBrowseMeta(meta);
      } catch (e) {
        if (!cancelled) setBrowseMeta(null);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [activeGame, tab]);

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
    if (!modsActionsMenuOpen) return;
    const onPointerDown = (e: MouseEvent) => {
      if (
        modsActionsMenuRef.current &&
        !modsActionsMenuRef.current.contains(e.target as Node)
      ) {
        setModsActionsMenuOpen(false);
      }
    };
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") setModsActionsMenuOpen(false);
    };
    document.addEventListener("mousedown", onPointerDown);
    document.addEventListener("keydown", onKeyDown);
    return () => {
      document.removeEventListener("mousedown", onPointerDown);
      document.removeEventListener("keydown", onKeyDown);
    };
  }, [modsActionsMenuOpen]);

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
    // List (and sentinel) unmount while browseDetail is open — re-bind on Back.
    if (tab !== "browse" || browseDetail || !browseHasMore) return;
    const sentinel = browseSentinelRef.current;
    const root = browseResultsRef.current;
    if (!sentinel || !root) return;

    const observer = new IntersectionObserver(
      (entries) => {
        const visible = entries.some((e) => e.isIntersecting);
        if (!visible) {
          browseAppendFailedRef.current = false;
          return;
        }
        if (
          browseLoadingMoreRef.current ||
          !browseHasMoreRef.current ||
          browseAppendFailedRef.current
        ) {
          return;
        }
        void runSearch(
          browseModeRef.current,
          searchQueryRef.current,
          browseOptsRef.current,
          true,
        );
      },
      { root, rootMargin: "240px", threshold: 0 },
    );
    observer.observe(sentinel);
    return () => observer.disconnect();
    // Re-bind when list remounts / size / mode / view changes so the sentinel stays observed.
    // eslint-disable-next-line react-hooks/exhaustive-deps -- runSearch reads latest via refs
  }, [
    tab,
    browseDetail,
    browseMode,
    browseView,
    browseHasMore,
    modHits.length,
    collectionHits.length,
  ]);

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
        try {
          await api.setAssistVisible(false);
        } catch {
          /* ignore */
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
      api.setAssistVisible(false).catch(() => {});
      return;
    }
    const syncSoon = () => {
      requestAnimationFrame(() => {
        if (nexusLoginOpenRef.current) return;
        void syncAssistHost();
        requestAnimationFrame(() => {
          if (nexusLoginOpenRef.current) return;
          void syncAssistHost();
        });
      });
    };
    syncSoon();
    const onResize = () => syncSoon();
    window.addEventListener("resize", onResize);
    window.addEventListener("scroll", onResize, true);
    window.visualViewport?.addEventListener("resize", onResize);
    window.visualViewport?.addEventListener("scroll", onResize);
    const observer = new ResizeObserver(() => syncSoon());
    const fullHost = assistHostRef.current;
    const miniHost = sidebarAssistHostRef.current;
    if (fullHost) observer.observe(fullHost);
    if (miniHost) observer.observe(miniHost);
    return () => {
      assistSyncGenRef.current += 1;
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
      try {
        await api.openNexusWebsiteLogin();
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
  }, [nexusLoginOpen, syncNexusLoginHost]);

  useEffect(() => {
    const unlisten = listen("nexus-login-complete", () => {
      if (!nexusLoginOpenRef.current) return;
      nexusLoginGenRef.current += 1;
      nexusLoginOpenRef.current = false;
      setNexusLoginConfirming(false);
      setNexusLoginOpen(false);
      api.closeDownloadAssist().catch(() => {});
      void refreshSettings();
      setNotice({
        kind: "ok",
        message: "Nexus website session saved.",
      });
    });
    return () => {
      unlisten.then((drop) => drop());
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
        await api.recordEmperorShare({
          gameId: q.entries[0]?.gameId ?? game?.id ?? "",
          collectionId: q.emperorShare.collectionId,
          name: q.emperorShare.name,
          code: q.emperorShare.code,
          files: q.emperorShare.files,
          memberIds: q.emperorShare.memberIds,
          existingModIds: q.emperorShare.existingModIds,
        });
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
  }, [refreshDownloads]);

  const forceResetOneDownload = useCallback(async (id: string) => {
    try {
      await api.forceResetDownload(id);
    } catch (e) {
      setError(String(e));
    }
    setBusy(null);
    downloadProgressAtRef.current.delete(id);
    const list = await api.listDownloads();
    setDownloads(list);
    if (!hasActiveDownloads(list)) {
      setBusy(null);
    }
  }, []);

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
      try {
        await api.removeDownload(id);
      } catch (e) {
        setError(String(e));
      }
      await refreshDownloads();
    },
    [refreshDownloads],
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
                await api.recordEmperorShare({
                  gameId:
                    event.payload.game_id ?? activeGameRef.current?.id ?? "",
                  collectionId: batch.emperorShare.collectionId,
                  name: batch.emperorShare.name,
                  code: batch.emperorShare.code,
                  files: batch.emperorShare.files,
                  memberIds: batch.emperorShare.memberIds,
                  existingModIds: batch.emperorShare.existingModIds,
                });
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
          try {
            await refreshMods(gameId);
            await refreshCollections(gameId);
            await refreshModUpdates(gameId);
            await refreshMissingRequirements(gameId);
          } catch {
            /* ignore */
          }
        }
        if (!hasActiveDownloads(list)) {
          setBusy(null);
        }
      },
    );
    const unlistenDlFailed = listen<{ label?: string; error?: string }>(
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
          setError(`${label}: ${err}`);
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
      setDownloads((prevList) => {
        const idx = prevList.findIndex((d) => d.id === p.id);
        if (idx < 0) return prevList;
        // A late progress event must not pull a finished row back into the
        // active list.
        if (isFinishedDownloadStatus(prevList[idx].status)) return prevList;
        const next = [...prevList];
        next[idx] = {
          ...next[idx],
          bytes_downloaded: p.bytes_downloaded,
          bytes_total: p.bytes_total,
          speed_bps: p.speed_bps,
          status: p.status || next[idx].status,
        };
        return next;
      });
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
      unlistenChanged.then((f) => f());
      unlistenNxm.then((f) => f());
      unlistenEmperorMod.then((f) => f());
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
  }, [advanceAssistAfterFailure, advanceAssistAfterStart, refreshDownloads, refreshMods, refreshCollections, refreshModUpdates, refreshMissingRequirements, syncAssistHost]);

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
        coverPath: game.cover_path,
        thunderstoreCommunity: suggestion?.thunderstore_community ?? null,
        modioGameId: suggestion?.modio_game_id ?? null,
      });
      const list = await refreshManaged();
      const managedGame = list.find((g) => g.id === game.id);
      if (managedGame) {
        await openGame(managedGame, "mods");
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
        coverPath: game.cover_path,
        projectName,
        thunderstoreCommunity: suggestion!.thunderstore_community ?? null,
        modioGameId: suggestion!.modio_game_id ?? null,
      });
      const list = await refreshManaged();
      const managedGame = list.find((g) => g.id === game.id);
      if (managedGame) {
        await openGame(managedGame, "mods");
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
        coverPath: game.cover_path,
        thunderstoreCommunity: suggestion!.thunderstore_community ?? null,
        modioGameId: suggestion!.modio_game_id ?? null,
      });
      const list = await refreshManaged();
      const managedGame = list.find((g) => g.id === game.id);
      if (managedGame) {
        await openGame(managedGame, "mods");
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
        coverPath: game.cover_path,
        projectName: engine === "unreal" ? values.projectName : null,
        thunderstoreCommunity: values.thunderstoreCommunity || null,
        modioGameId: values.modioGameId,
      });
      closeCatalogMatchPrompt();
      const list = await refreshManaged();
      const managedGame = list.find((g) => g.id === game.id);
      if (managedGame) {
        await openGame(managedGame, "mods");
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
        await openGame(relinked, "mods");
      }
      setNotice({
        kind: "ok",
        message: `${game.title} relinked to the new install folder.`,
      });
    });
  }

  async function unmanageFromLibrary(game: ManagedGame) {
    await withBusy("Removing…", async () => {
      const closedDetail = libraryDetail?.game.id === game.id;
      const wasActive = activeId === game.id;
      await api.unmanageGame(game.id);
      if (closedDetail) {
        closeLibraryDetail();
      }
      const list = await refreshManaged();
      const settings = await refreshSettings();
      const nextActive =
        list.find((g) => g.id === settings.last_active_game_id) ?? list[0] ?? null;
      setActiveId(nextActive?.id ?? null);
      if (closedDetail || wasActive) {
        setMods([]);
        setInstalledCollections([]);
      }
      await scan();
    });
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
    const all =
      browseMode === "mods"
        ? (browseMeta?.mod_tags ?? [])
        : (browseMeta?.collection_tags ?? []);
    const q = tagQuery.trim().toLowerCase();
    const filtered = q ? all.filter((t) => t.toLowerCase().includes(q)) : all;
    return [...filtered].sort((a, b) => {
      const rank = (t: string) =>
        browseTags[t] === "include" ? 0 : browseTags[t] === "exclude" ? 1 : 2;
      const ar = rank(a);
      const br = rank(b);
      if (ar !== br) return ar - br;
      return a.localeCompare(b, undefined, { sensitivity: "base" });
    });
  }, [browseMeta?.mod_tags, browseMeta?.collection_tags, browseMode, browseTags, tagQuery]);

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
          return merged;
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
          return merged;
        });
      }
    };

    try {
      if (!append) {
        browseNextOffsetRef.current = 0;
        browseAppendFailedRef.current = false;
        const perform = async () => {
          if (mode === "mods") {
            const page = await api.searchCatalog(activeGame.id, query, {
              ...requestOpts,
              sourceFilter,
            });
            if (requestId !== browseRequestIdRef.current) return;
            browsePrefetchErrorRef.current = null;
            applyModPage(
              page.items,
              page.total_count,
              page.has_more,
              page.next_offset,
              "replace",
            );
          } else {
            const page = await api.searchCollectionCatalog(activeGame.id, query, {
              ...requestOpts,
              sourceFilter:
                sourceFilter === "modio" ? "all" : sourceFilter,
            });
            if (requestId !== browseRequestIdRef.current) return;
            browsePrefetchErrorRef.current = null;
            applyCollectionPage(page.items, page.total_count, "replace");
            if (page.has_more != null) {
              browseHasMoreRef.current = page.has_more;
              setBrowseHasMore(page.has_more);
            }
            if (page.next_offset != null) {
              browseNextOffsetRef.current = page.next_offset;
            }
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
        applyModPage(
          page.items,
          page.total_count,
          page.has_more,
          page.next_offset,
          "append",
        );
        appendOk = true;
        browseAppendFailedRef.current = false;
      } else {
        const page = await api.searchCollectionCatalog(activeGame.id, query, {
          ...requestOpts,
          sourceFilter: sourceFilter === "modio" ? "all" : sourceFilter,
        });
        if (requestId !== browseRequestIdRef.current) return;
        applyCollectionPage(page.items, page.total_count, "append");
        if (page.has_more != null) {
          browseHasMoreRef.current = page.has_more;
          setBrowseHasMore(page.has_more);
        }
        if (page.next_offset != null) {
          browseNextOffsetRef.current = page.next_offset;
        }
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
      if (silent && !append && requestId === browseRequestIdRef.current) {
        setBrowseLoading(false);
      }
      if (append) {
        browseLoadingMoreRef.current = false;
        if (
          appendOk &&
          requestId === browseRequestIdRef.current &&
          browseHasMoreRef.current &&
          isBrowseSentinelNearView()
        ) {
          queueMicrotask(() => {
            void runSearch(
              browseModeRef.current,
              searchQueryRef.current,
              browseOptsRef.current,
              true,
            );
          });
        }
      }
    }
  }

  function isBrowseSentinelNearView(margin = 240): boolean {
    const sentinel = browseSentinelRef.current;
    const root = browseResultsRef.current;
    if (!sentinel || !root) return false;
    const rootRect = root.getBoundingClientRect();
    const sentRect = sentinel.getBoundingClientRect();
    return (
      sentRect.top < rootRect.bottom + margin &&
      sentRect.bottom > rootRect.top - margin
    );
  }

  useEffect(() => {
    if (tab !== "browse" || browseDetail || !browseHasMore) return;
    const loaded = browseMode === "mods" ? modHits.length : collectionHits.length;
    if (loaded === 0) return;
    const id = requestAnimationFrame(() => {
      if (!isBrowseSentinelNearView(0)) return;
      void runSearch(
        browseModeRef.current,
        searchQueryRef.current,
        browseOptsRef.current,
        true,
      );
    });
    return () => cancelAnimationFrame(id);
    // runSearch and the sentinel check read the latest filters through refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [
    tab,
    browseDetail,
    browseHasMore,
    browseMode,
    browseView,
    browsePageCount,
    modHits.length,
    collectionHits.length,
  ]);

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
    rememberBrowseScroll();
    setBrowseDetail({ kind: "mod", hit, tab: detailTab });
    setModDetail(null);
    setTsDetail(null);
    setTsVersion("");
    setCollectionModFiles([]);
    setCollectionDetail(null);
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
      if (!hit.community || !hit.namespace || !hit.package_name) return;
      await withBusy("Loading package…", async () => {
        const detail = await api.getThunderstorePackage(
          hit.community!,
          hit.namespace!,
          hit.package_name!,
        );
        setTsDetail(detail);
        setTsVersion(detail.latest_version ?? detail.versions[0]?.version_number ?? "");
        patchBrowseHit((current) => enrichCatalogHitFromThunderstore(current, detail));
      });
      return;
    }
    if (hit.source === "modio") {
      if (hit.modio_game_id == null || hit.modio_mod_id == null) return;
      await withBusy("Loading mod.io mod…", async () => {
        const detail = await api.getModioMod(hit.modio_game_id!, hit.modio_mod_id!);
        setModioDetail(detail);
        patchBrowseHit((current) => enrichCatalogHitFromModio(current, detail));
        if (detailTab === "files") {
          setModioFiles(
            await api.modioFiles(hit.modio_game_id!, hit.modio_mod_id!),
          );
        }
      });
      return;
    }
    const domain = hit.domain_name || activeGame.nexus_domain;
    const modId = hit.mod_id;
    if (!modId) return;
    await withBusy(
      detailTab === "files" ? "Loading files…" : "Loading mod…",
      async () => {
        const detail = await api.getMod(domain, modId);
        setModDetail(detail);
        patchBrowseHit((current) => enrichCatalogHitFromNexus(current, detail));
        if (detailTab === "files") {
          await loadModFiles(domain, modId);
        }
      },
    );
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
    const link = modDeepLinkFromHit(hit);
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
    rememberBrowseScroll();
    setBrowseDetail({ kind: "collection", hit, tab: detailTab });
    setModDetail(null);
    clearModFiles();
    setCollectionModFiles([]);
    setCollectionDetail(null);
    setTsDetail(null);
    if (collectionHitSource(hit) === "thunderstore") {
      if (!hit.community || !hit.namespace || !hit.package_name) return;
      await withBusy("Loading modpack…", async () => {
        const detail = await api.getThunderstorePackage(
          hit.community!,
          hit.namespace!,
          hit.package_name!,
        );
        setTsDetail(detail);
        setTsVersion(detail.latest_version ?? detail.versions[0]?.version_number ?? "");
      });
      return;
    }
    const domain = hit.domain_name || activeGame.nexus_domain;
    await withBusy(
      detailTab === "files" ? "Loading collection mods…" : "Loading collection…",
      async () => {
        const detailPromise = api.getCollection({ slug: hit.slug, domain });
        const filesPromise = api.collectionFiles({
          slug: hit.slug,
          revision: hit.revision_number,
        });
        const [detail, files] = await Promise.all([detailPromise, filesPromise]);
        setCollectionDetail(detail);
        setCollectionModFiles(files);
      },
    );
  }

  async function setDetailTab(detailTab: DetailTab) {
    if (!browseDetail || !activeGame) return;
    if (browseDetail.tab === detailTab) return;
    const next = { ...browseDetail, tab: detailTab };
    setBrowseDetail(next);
    if (next.kind === "mod") {
      if (next.hit.source === "thunderstore") {
        if (!tsDetail && next.hit.community && next.hit.namespace && next.hit.package_name) {
          await withBusy("Loading package…", async () => {
            const detail = await api.getThunderstorePackage(
              next.hit.community!,
              next.hit.namespace!,
              next.hit.package_name!,
            );
            setTsDetail(detail);
            setTsVersion(detail.latest_version ?? detail.versions[0]?.version_number ?? "");
          });
        }
      } else if (next.hit.source === "modio") {
        if (
          next.hit.modio_game_id != null &&
          next.hit.modio_mod_id != null &&
          (!modioDetail || (detailTab === "files" && modioFiles.length === 0))
        ) {
          await withBusy(
            detailTab === "files" ? "Loading files…" : "Loading mod.io mod…",
            async () => {
              if (!modioDetail) {
                setModioDetail(
                  await api.getModioMod(
                    next.hit.modio_game_id!,
                    next.hit.modio_mod_id!,
                  ),
                );
              }
              if (detailTab === "files" && modioFiles.length === 0) {
                setModioFiles(
                  await api.modioFiles(
                    next.hit.modio_game_id!,
                    next.hit.modio_mod_id!,
                  ),
                );
              }
            },
          );
        }
      } else if (
        detailTab === "files" &&
        next.hit.mod_id &&
        modFilesModIdRef.current !== next.hit.mod_id
      ) {
        await withBusy("Loading files…", async () => {
          await loadModFiles(
            next.hit.domain_name || activeGame.nexus_domain,
            next.hit.mod_id!,
          );
        });
      } else if (detailTab === "info" && !modDetail && next.hit.mod_id) {
        await withBusy("Loading mod…", async () => {
          setModDetail(
            await api.getMod(
              next.hit.domain_name || activeGame.nexus_domain,
              next.hit.mod_id!,
            ),
          );
        });
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
        await withBusy("Loading modpack…", async () => {
          const detail = await api.getThunderstorePackage(
            next.hit.community!,
            next.hit.namespace!,
            next.hit.package_name!,
          );
          setTsDetail(detail);
          setTsVersion(
            detail.latest_version ?? detail.versions[0]?.version_number ?? "",
          );
        });
      }
    } else if (detailTab === "files" && collectionModFiles.length === 0) {
      await withBusy("Loading collection mods…", async () => {
        setCollectionModFiles(
          await api.collectionFiles({
            slug: next.hit.slug,
            revision: next.hit.revision_number,
          }),
        );
      });
    } else if (detailTab === "info" && !collectionDetail) {
      await withBusy("Loading collection…", async () => {
        setCollectionDetail(
          await api.getCollection({
            slug: next.hit.slug,
            domain: next.hit.domain_name || activeGame.nexus_domain,
          }),
        );
      });
    }
  }

  async function openGame(
    game: ManagedGame,
    detailTab: GameDetailTab = "info",
    opts?: { skipHistoryPush?: boolean },
  ) {
    if (!opts?.skipHistoryPush) {
      pushNavTarget({
        tab: "library",
        browseReturnTab: null,
        browseDetail: null,
        libraryDetail: { gameId: game.id, tab: detailTab },
        gameSavedDetailId: null,
        savedDetailId: null,
      });
    }
    await api.setActiveGame(game.id);
    setActiveId(game.id);
    resetBrowseState();
    resetLibraryEphemeralState();
    setSourceFilter((prev) =>
      clampCatalogSourceFilter(prev, game, browseMode),
    );
    setLibraryDetail({ game, tab: detailTab });
    setGameSavedDetail(null);
    setUeDomainEdit(game.nexus_domain);
    setUeProjectEdit(game.project_name ?? "");
    setTsCommunityEdit(game.thunderstore_community ?? "");
    setModioGameIdEdit(
      game.modio_game_id != null && game.modio_game_id > 0
        ? String(game.modio_game_id)
        : "",
    );
    setTab("library");
    setGameInfo(null);
    if (detailTab === "collections") {
      void refreshSavedCollections();
    }
    if (game.nexus_domain) {
      try {
        setGameInfo(await api.getGame(game.nexus_domain));
      } catch {
        /* Nexus metadata is optional for local game info */
      }
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

  function setGameDetailTab(detailTab: GameDetailTab) {
    if (detailTab !== "collections") {
      setGameSavedDetail(null);
    }
    setLibraryDetail((prev) => (prev ? { ...prev, tab: detailTab } : prev));
    if (detailTab === "collections") {
      void refreshSavedCollections();
    }
  }

  function closeLibraryDetail() {
    setLibraryDetail(null);
    setGameInfo(null);
    setGameSavedDetail(null);
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
      message: `Installed ${result.name} (${result.member_ids.length} mods).`,
      details: result.warnings.length > 0 ? result.warnings : undefined,
    });
  }

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

  async function saveShareCodeToFile(code: string, suggestedName?: string) {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({
      defaultPath: `${(suggestedName || "loadout").replace(/[^\w.-]+/g, "_") || "loadout"}.emperor-share`,
      filters: [
        { name: "Emperor share", extensions: ["emperor-share"] },
        { name: "Text", extensions: ["txt"] },
      ],
    });
    if (!path) return false;
    await api.writeShareFile(path, code);
    return true;
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

  async function exportShareLoadout() {
    if (!activeGame) return;
    setBusy("Creating share code…");
    setError(null);
    try {
      const result = await api.exportShareCode(
        activeGame.id,
        shareExportName.trim() || null,
      );
      setLastShareCode(result.code);
      let message = `Copied share code for ${result.mod_count} mod(s). Saved to Collections. Prefer Save as file for Discord.`;
      try {
        await navigator.clipboard.writeText(result.code);
      } catch {
        message = `Share code ready for ${result.mod_count} mod(s). Saved to Collections (copy from the box below, or Save as file for Discord).`;
      }
      setNotice({
        kind: "warn",
        message,
        details: result.warnings.length > 0 ? result.warnings : undefined,
      });
      setShareExportOpen(false);
      setShareExportName("");
      await refreshSavedCollections();
      await refreshCollections(activeGame.id);
      await refreshMods(activeGame.id);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  }

  async function refreshSavedCollections() {
    try {
      const list = await api.listSavedCollections();
      setSavedCollections(list);
    } catch (e) {
      setError(String(e));
    }
  }

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
    setBusy("Loading collection…");
    setError(null);
    try {
      const detail = await api.getSavedCollection(id);
      setSavedDetail(detail);
      setSavedRename(detail.entry.name);
      const matches = managed.filter((g) =>
        savedCollectionMatchesGame(detail.entry, g),
      );
      setSavedInstallGameId(
        matches[0]?.id ??
          detail.entry.source_game_id ??
          activeGame?.id ??
          managed[0]?.id ??
          "",
      );
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
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
    setBusy("Loading collection…");
    setError(null);
    try {
      const detail = await api.getSavedCollection(id);
      setGameSavedDetail(detail);
      setGameSavedRename(detail.entry.name);
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
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
      ),
    [savedCollections, managed, detected, gamePlugins],
  );

  const refreshCollectionsPageMeta = useCallback(async () => {
    if (gamePlugins.length === 0) {
      try {
        setGamePlugins(await api.listPlugins());
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

    const shareIdByEntry: Record<string, string> = {};
    await Promise.all(
      savedCollections.map(async (entry) => {
        try {
          shareIdByEntry[entry.id] = await api.emperorShareId(entry.code);
        } catch {
          /* ignore */
        }
      }),
    );

    setInstalledEmperorShareIds(installedIds);
    setSavedEntryShareIds(shareIdByEntry);
    setSavedCollectionAdvice(adviceEntries);
  }, [gamePlugins.length, managed, savedCollections]);

  useEffect(() => {
    if (tab !== "collections") return;
    void refreshCollectionsPageMeta();
  }, [tab, refreshCollectionsPageMeta]);

  const isSavedCollectionInstalled = useCallback(
    (entry: SavedCollectionEntry) => {
      if (installedEmperorShareIds.has(entry.code.trim())) return true;
      const shareId = savedEntryShareIds[entry.id];
      return Boolean(shareId && installedEmperorShareIds.has(shareId));
    },
    [installedEmperorShareIds, savedEntryShareIds],
  );

  function shareModLabel(m: ShareModEntry): string {
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

  async function uninstallInstalledCollection(c: InstalledCollection) {
    if (!activeGame) return;
    if (
      !window.confirm(
        `Uninstall ${c.name}? Mods only used by this collection will be removed. Shared requirements stay.`,
      )
    ) {
      return;
    }
    await withBusy(`Uninstalling ${c.name}…`, async () => {
      await api.uninstallCollection(activeGame.id, c.id);
      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      if (modsCollectionFilter === c.id) setModsCollectionFilter("");
      if (tab === "collections") await refreshCollectionsPageMeta();
    });
  }

  async function deleteSavedCollectionEntry(entry: SavedCollectionEntry) {
    let shareId: string | null = savedEntryShareIds[entry.id] ?? null;
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
      await api.updateStagedMod(activeGame.id, stagedId);
      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
      clearUpdateBadge();
      void refreshModUpdates(activeGame.id);
    });
  }

  async function checkStagedModUpdatesNow() {
    if (!activeGame) return;
    const gameId = activeGame.id;
    await withBusy("Checking for updates…", async () => {
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
          for (const id of apiIds) {
            await api.updateStagedMod(activeGame.id, id);
          }
          await refreshMods(activeGame.id);
          await refreshCollections(activeGame.id);
          setModUpdates((prev) => {
            const next = { ...prev };
            for (const id of apiIds) delete next[id];
            return next;
          });
          void refreshModUpdates(activeGame.id);
        },
      );
    }
  }

  async function importArchive() {
    if (!activeGame) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      multiple: false,
      filters: [
        {
          name: "Mods",
          extensions: ["zip", "7z", "rar", "package", "ts4script", "dbc", "sims3pack"],
        },
        { name: "Archives", extensions: ["zip", "7z", "rar"] },
      ],
    });
    if (!selected || Array.isArray(selected)) return;
    await withBusy("Importing archive…", async () => {
      await api.importModArchive({ gameId: activeGame.id, path: selected });
      await refreshMods(activeGame.id);
      await refreshCollections(activeGame.id);
    });
  }

  async function importFolder() {
    if (!activeGame) return;
    const { open } = await import("@tauri-apps/plugin-dialog");
    const selected = await open({
      directory: true,
      multiple: false,
      title: "Import mods from folder",
    });
    if (!selected || Array.isArray(selected)) return;
    await withBusy("Importing folder…", async () => {
      const result = await api.importModFolder({
        gameId: activeGame.id,
        path: selected,
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

  function ignoreRequirement(row: IgnoredRequirement) {
    if (!activeGame) return;
    const current = activeGame.ignored_requirements ?? [];
    if (current.some((item) => item.key === row.key)) return;
    void saveIgnoredRequirements([...current, row]);
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

  async function deploy() {
    if (!activeGame) return;
    await withBusy("Deploying mods…", async () => {
      const result = await api.deployMods(activeGame.id);
      void refreshMissingRequirements(activeGame.id);
      void refreshWineAdvice(activeGame.id);
      const full = result.missing_requirements;
      setNotice(
        deployStatusNotice(
          {
            ...result,
            missing_requirements: full
              ? withoutIgnoredRequirements(full, activeGame.ignored_requirements ?? [])
              : full,
          },
          (requirements) => {
            setRequirementsModal(full ?? requirements);
          },
        ),
      );
    });
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

  async function checkLoadLogs() {
    if (!activeGame) return;
    await withBusy("Checking loader logs…", async () => {
      const result = await api.diagnoseLoad(activeGame.id);
      setLoadDiagnosis(result);
    });
  }

  async function purge() {
    if (!activeGame) return;
    await withBusy("Purging deployed files…", async () => {
      await api.purgeMods(activeGame.id);
      void refreshWineAdvice(activeGame.id);
      setNotice({ kind: "ok", message: "Purged deployed files." });
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
      onSignIn={() => setNexusLoginOpen(true)}
      onClearWebsiteSession={() => void clearNexusWebsiteSession()}
    />
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
        <div className="sidebar-top">
          <div className="brand">
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
        <nav>
          {navItems.map(([id, label]) => (
            <NavTab
              key={id}
              layoutId="sidebar-nav"
              active={tab === id}
              onClick={() => {
                setTab(id);
                if (id === "downloads") refreshDownloads();
                if (id === "library" && detected.length === 0) scan();
                if (id === "collections") {
                  void refreshSavedCollections();
                  setSavedDetail(null);
                }
              }}
            >
              {label}
            </NavTab>
          ))}
        </nav>
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
                    <span className="status-primary">{downloadStatus.primary}</span>
                    {downloadStatus.secondary ? (
                      <span className="status-secondary">{downloadStatus.secondary}</span>
                    ) : null}
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
                  <span className="status-primary">{downloadStatus.primary}</span>
                  {downloadStatus.secondary ? (
                    <span className="status-secondary">{downloadStatus.secondary}</span>
                  ) : null}
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
      </aside>

      <main className="main" ref={mainScrollRef}>
        <Reveal show={Boolean(error || assistLoginBanner || notice)}>
          {error ? (
            <StatusBanner
              kind="error"
              message={error}
              action={errorAction}
              onDismiss={() => setError(null)}
            />
          ) : assistLoginBanner ? (
            <StatusBanner
              kind="warn"
              message="Nexus website sign-in is required for Download Assist. Open Downloads to sign in (use Stay signed in), then the queue continues."
              action={{ label: "Open Downloads", run: () => openDownloadsTab() }}
              onDismiss={() => setAssistLoginBanner(false)}
            />
          ) : notice ? (
            <StatusBanner
              kind={notice.kind === "ok" ? "info" : "warn"}
              message={notice.message}
              details={notice.details}
              action={notice.action}
              onDismiss={() => setNotice(null)}
            />
          ) : null}
        </Reveal>

        {tab ? (
        <ViewSwitch viewKey={tab} className="view-slot">
        {tab === "setup" && (
          <section className="panel">
            <h1>Setup</h1>
            <p>
              A Nexus API key and a Nexus website sign-in are both required before
              the rest of the app opens. Use Stay signed in. The website session
              is saved separately from the API key. Premium and free accounts
              both need the sign-in. mod.io is optional.
            </p>
            <div className="status-card">
              <span className={settings?.has_api_key ? "pill ok" : "pill warn"}>
                {settings?.has_api_key ? "Nexus API key saved" : "Nexus API key required"}
              </span>
              <span
                className={settings?.nexus_website_signed_in ? "pill ok" : "pill warn"}
              >
                {settings?.nexus_website_signed_in
                  ? "Website sign-in saved"
                  : "Website sign-in required"}
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
                    : "Save a Nexus API key and finish website sign-in first."
                }
                onClick={() => void finishSetup()}
              >
                Finish setup
              </button>
            </div>
          </section>
        )}

        {tab === "library" && (
          <section className="panel">
            <ViewSwitch
              viewKey={libraryDetail ? `game:${libraryDetail.game.id}` : "library-home"}
              initial={false}
            >
            {libraryDetail ? (
              <>
                <div className="panel-head">
                  <div className="detail-head-title">
                    <button type="button" onClick={() => closeLibraryDetail()}>
                      Home
                    </button>
                    <h1>{libraryDetail.game.title}</h1>
                  </div>
                  <div className="panel-head-actions">
                    <Segment>
                      <SegmentTab
                        layoutId="library-game-tabs"
                        active={libraryDetail.tab === "info"}
                        onClick={() => setGameDetailTab("info")}
                      >
                        Info
                      </SegmentTab>
                      <SegmentTab
                        layoutId="library-game-tabs"
                        active={libraryDetail.tab === "mods"}
                        onClick={() => setGameDetailTab("mods")}
                      >
                        Mods
                      </SegmentTab>
                      <SegmentTab
                        layoutId="library-game-tabs"
                        active={libraryDetail.tab === "collections"}
                        onClick={() => setGameDetailTab("collections")}
                      >
                        Collections
                      </SegmentTab>
                      {settings?.platform_linux && (
                        <SegmentTab
                          layoutId="library-game-tabs"
                          active={libraryDetail.tab === "tools"}
                          onClick={() => setGameDetailTab("tools")}
                        >
                          Tools
                        </SegmentTab>
                      )}
                    </Segment>
                    <button
                      className="danger"
                      onClick={() => void unmanageFromLibrary(libraryDetail.game)}
                    >
                      Unmanage
                    </button>
                  </div>
                </div>

                <div className="detail-hero">
                  <div className="detail-cover">
                    {mediaSrc(libraryDetail.game.cover_path) ? (
                      <img src={mediaSrc(libraryDetail.game.cover_path)!} alt="" />
                    ) : (
                      <PlaceholderIcon />
                    )}
                  </div>
                  <div className="detail-meta">
                    <p className="detail-author">
                      {shortLauncher(libraryDetail.game.launcher)}
                      {gameInfo?.genre ? ` · ${gameInfo.genre}` : ""}
                    </p>
                    <div className="detail-stats">
                      <span>
                        {mods.length} mod{mods.length === 1 ? "" : "s"}
                      </span>
                      <span>
                        {mods.filter((m) => m.enabled).length} enabled
                      </span>
                      {gameInfo?.mods != null && (
                        <span>{gameInfo.mods.toLocaleString()} on Nexus</span>
                      )}
                      {gameInfo?.downloads != null && (
                        <span>{gameInfo.downloads.toLocaleString()} downloads</span>
                      )}
                      <span>{libraryDetail.game.nexus_domain}</span>
                    </div>
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
                  </div>
                </div>

                <ViewSwitch
                  viewKey={
                    libraryDetail.tab === "collections" && gameSavedDetail
                      ? `collections:${gameSavedDetail.entry.id}`
                      : libraryDetail.tab
                  }
                  initial={false}
                >
                {libraryDetail.tab === "info" && (
                  <div className="detail-body">
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
                  </div>
                )}
                {libraryDetail.tab === "mods" && (
                  <div className="detail-body">
                    <Reveal
                      show={Boolean(shareExportOpen || shareImportOpen || lastShareCode)}
                      className="profile-import share-panel"
                    >
                        {shareExportOpen && (
                          <>
                            <h2>Share loadout</h2>
                            <p className="note">
                              Encodes enabled portable mods (Nexus, Thunderstore, mod.io) into a
                              pasteable code. Saved under Collections. For Discord, use Save as
                              file — chat paste often breaks the code.
                            </p>
                            <label>
                              Name (optional)
                              <input
                                value={shareExportName}
                                onChange={(e) => setShareExportName(e.target.value)}
                                placeholder="My loadout"
                              />
                            </label>
                            <div className="actions">
                              <button
                                type="button"
                                onClick={() => void exportShareLoadout()}
                              >
                                Create &amp; copy code
                              </button>
                              <button
                                type="button"
                                className="linkish"
                                onClick={() => setShareExportOpen(false)}
                              >
                                Cancel
                              </button>
                            </div>
                          </>
                        )}
                        {shareImportOpen && (
                          <>
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
                          </>
                        )}
                        {lastShareCode && !shareExportOpen && (
                          <>
                            <h2>Last share code</h2>
                            <textarea
                              rows={3}
                              readOnly
                              value={lastShareCode}
                              onFocus={(e) => e.target.select()}
                            />
                            <div className="actions">
                              <button
                                type="button"
                                className="icon-btn"
                                aria-label="Copy again"
                                title="Copy again"
                                onClick={() =>
                                  void navigator.clipboard.writeText(lastShareCode)
                                }
                              >
                                <FontAwesomeIcon icon={faCopy} />
                              </button>
                              <button
                                type="button"
                                onClick={() =>
                                  void (async () => {
                                    try {
                                      const saved = await saveShareCodeToFile(
                                        lastShareCode,
                                        shareExportName.trim() || "loadout",
                                      );
                                      if (saved) {
                                        setNotice({
                                          kind: "ok",
                                          message:
                                            "Saved .emperor-share file. Send that instead of pasting into Discord.",
                                        });
                                      }
                                    } catch (e) {
                                      setError(String(e));
                                    }
                                  })()
                                }
                              >
                                Save as file
                              </button>
                              <button
                                type="button"
                                className="linkish"
                                onClick={() => setLastShareCode(null)}
                              >
                                Dismiss
                              </button>
                            </div>
                          </>
                        )}
                    </Reveal>
                    {installedCollections.length > 0 && (
                      <div className="installed-collections">
                        <h2>Installed collections</h2>
                        <ul className="list">
                          {installedCollections.map((c) => {
                            const installedLabel = formatInstalledAt(c.installed_at);
                            return (
                              <li key={c.id}>
                                <div>
                                  <strong>{c.name}</strong>
                                  <small>
                                    {installedCollectionSourceLabel(c)}
                                    {` · ${c.mod_ids.length} mods`}
                                    {installedLabel ? ` · ${installedLabel}` : ""}
                                  </small>
                                </div>
                                <div className="actions">
                                  <button
                                    type="button"
                                    className={
                                      modsCollectionFilter === c.id ? "active" : ""
                                    }
                                    onClick={() =>
                                      setModsCollectionFilter((prev) =>
                                        prev === c.id ? "" : c.id,
                                      )
                                    }
                                  >
                                    {modsCollectionFilter === c.id
                                      ? "Showing"
                                      : "Filter"}
                                  </button>
                                  <button
                                    type="button"
                                    className="danger"
                                    onClick={() =>
                                      void uninstallInstalledCollection(c)
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
                    <div className="mods-toolbar">
                      <div className="mods-toolbar-actions">
                        <div className="mods-toolbar-actions-left">
                          {stagedUpdateCount > 0 && (
                            <button
                              type="button"
                              className="accent"
                              onClick={() => void updateAllStagedMods()}
                            >
                              Update all ({stagedUpdateCount})
                            </button>
                          )}
                          <button type="button" onClick={deploy}>
                            Deploy
                          </button>
                          <button type="button" onClick={purge}>
                            Purge
                          </button>
                          <div className="mods-actions-menu" ref={modsActionsMenuRef}>
                            <button
                              type="button"
                              className="mods-toolbar-icon-btn"
                              aria-label="More"
                              aria-haspopup="menu"
                              aria-expanded={modsActionsMenuOpen}
                              onClick={() =>
                                setModsActionsMenuOpen((open) => !open)
                              }
                            >
                              <FontAwesomeIcon icon={faBars} />
                            </button>
                            <PopIn
                              show={modsActionsMenuOpen}
                              className="mods-actions-menu-popup"
                              role="menu"
                            >
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  setModsActionsMenuOpen(false);
                                  setShareExportOpen((v) => !v);
                                  setShareImportOpen(false);
                                }}
                              >
                                Share loadout
                              </button>
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  setModsActionsMenuOpen(false);
                                  setShareImportOpen((v) => !v);
                                  setShareExportOpen(false);
                                }}
                              >
                                Import code
                              </button>
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  setModsActionsMenuOpen(false);
                                  void importArchive();
                                }}
                              >
                                Import archive
                              </button>
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  setModsActionsMenuOpen(false);
                                  void importFolder();
                                }}
                              >
                                Import folder
                              </button>
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  setModsActionsMenuOpen(false);
                                  void checkLoadLogs();
                                }}
                              >
                                Check logs
                              </button>
                              <button
                                type="button"
                                role="menuitem"
                                onClick={() => {
                                  setModsActionsMenuOpen(false);
                                  void checkStagedModUpdatesNow();
                                }}
                              >
                                Check for updates
                              </button>
                              <button
                                type="button"
                                role="menuitem"
                                className="danger"
                                disabled={mods.length === 0}
                                onClick={() => {
                                  setModsActionsMenuOpen(false);
                                  void removeAllMods();
                                }}
                              >
                                Remove all mods
                              </button>
                            </PopIn>
                          </div>
                        </div>
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
                                onClick={() => setModsSourceFilter(id)}
                              >
                                {label}
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
                    <Reveal show={Boolean(settings?.platform_linux && wineAdvice)}>
                      {settings?.platform_linux && wineAdvice ? (
                        <WineOverrideNotice
                          advice={wineAdvice}
                          busy={wineApplyBusy}
                          onApply={() => void applyWineOverrides()}
                        />
                      ) : null}
                    </Reveal>
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
                    <ul
                      className={
                        modDrag.isDragging ? "list mods mod-list-dragging" : "list mods"
                      }
                      ref={modDrag.listRef}
                    >
                      {modDrag.displayMods.map((m) => {
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
                        const versionWarning = versionWarnings.find(
                          (row) => row.stagedId === m.id,
                        );
                        const modCollections = collectionsForMod(
                          m,
                          installedCollections,
                        );
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
                        const rowClass = [
                          m.enabled
                            ? "list-row-poster"
                            : "mod-disabled list-row-poster",
                          canReorderMods ? "mod-drag-ready" : "",
                          isDragged ? "mod-dragging" : "",
                        ]
                          .filter(Boolean)
                          .join(" ");
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
                              <div className="mod-identity">{identity}</div>
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
                          return (
                            <Fragment key={m.id}>
                              <li
                                className="mod-drag-placeholder"
                                style={{ height: modDrag.lift.height }}
                                aria-hidden="true"
                              />
                              <li
                                className={rowClass}
                                style={{
                                  position: "fixed",
                                  left: modDrag.lift.x,
                                  top: modDrag.lift.y,
                                  width: modDrag.lift.width,
                                  height: modDrag.lift.height,
                                  zIndex: 40,
                                  margin: 0,
                                  pointerEvents: "none",
                                  transform: reduceMotion
                                    ? undefined
                                    : "scale(1.02)",
                                }}
                              >
                                {rowBody}
                              </li>
                            </Fragment>
                          );
                        }
                        return (
                          <motion.li
                            key={m.id}
                            layout={
                              !reduceMotion &&
                              modDrag.isDragging &&
                              !isDragged
                            }
                            transition={{
                              layout: {
                                duration:
                                  reduceMotion || !modDrag.isDragging
                                    ? 0
                                    : MOTION_FAST,
                                ease: MOTION_EASE,
                              },
                            }}
                            className={rowClass}
                            ref={(el) => modDrag.setRowRef(m.id, el)}
                            onPointerDown={(e) =>
                              modDrag.onDragPointerDown(m.id, e)
                            }
                            title={
                              canReorderMods
                                ? "Click and hold to drag in load order"
                                : undefined
                            }
                          >
                            {rowBody}
                          </motion.li>
                        );
                      })}
                      {mods.length === 0 && (
                        <li className="empty">No staged mods. Browse mods to install some.</li>
                      )}
                      {mods.length > 0 && visibleMods.length === 0 && (
                        <li className="empty">No mods match your search/filters.</li>
                      )}
                    </ul>
                  </div>
                )}
                {libraryDetail.tab === "collections" && (
                  <div className="detail-body game-saved-collections">
                    {gameSavedDetail ? (
                      <>
                        <div className="panel-head-inline">
                          <button
                            type="button"
                            className="linkish"
                            onClick={() => closeLibraryDetail()}
                          >
                            Home
                          </button>
                          <h2>{gameSavedDetail.entry.name}</h2>
                        </div>
                        <p className="note">
                          {gameSavedDetail.entry.mod_count} mods
                          {gameSavedDetail.entry.nexus_count
                            ? ` · ${gameSavedDetail.entry.nexus_count} Nexus`
                            : ""}
                          {gameSavedDetail.entry.thunderstore_count
                            ? ` · ${gameSavedDetail.entry.thunderstore_count} Thunderstore`
                            : ""}
                          {gameSavedDetail.entry.modio_count
                            ? ` · ${gameSavedDetail.entry.modio_count} mod.io`
                            : ""}
                        </p>
                        <label>
                          Name
                          <div className="row">
                            <input
                              value={gameSavedRename}
                              onChange={(e) => setGameSavedRename(e.target.value)}
                            />
                            <button
                              type="button"
                              onClick={() =>
                                void (async () => {
                                  try {
                                    const updated = await api.renameSavedCollection(
                                      gameSavedDetail.entry.id,
                                      gameSavedRename,
                                    );
                                    setGameSavedDetail({
                                      ...gameSavedDetail,
                                      entry: updated,
                                    });
                                    await refreshSavedCollections();
                                  } catch (e) {
                                    setError(String(e));
                                  }
                                })()
                              }
                            >
                              Rename
                            </button>
                          </div>
                        </label>
                        <p className="note">
                          Install into <strong>{libraryDetail.game.title}</strong>
                        </p>
                        <div className="actions" style={{ marginBottom: "1rem" }}>
                          <button
                            type="button"
                            onClick={() => void installGameSavedCollection()}
                          >
                            Install
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
                          <button
                            type="button"
                            className="danger icon-btn"
                            aria-label={`Delete ${gameSavedDetail.entry.name}`}
                            title="Delete"
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
                                  setGameSavedDetail(null);
                                } catch (e) {
                                  setError(String(e));
                                }
                              })()
                            }
                          >
                            <FontAwesomeIcon icon={faTrashCan} />
                          </button>
                        </div>
                        <h2>Contained mods</h2>
                        <ul className="list">
                          {gameSavedDetail.mods.map((m, i) => (
                            <li key={`${m.s}-${i}`}>
                              <div>
                                <strong>{shareModLabel(m)}</strong>
                                <small>
                                  {m.s}
                                  {m.version ? ` · v${m.version}` : ""}
                                  {m.s === "nexus" && m.mod_id != null
                                    ? ` · ${m.domain} ${m.mod_id}/${m.file_id}`
                                    : ""}
                                  {m.s === "thunderstore"
                                    ? ` · ${m.community}/${m.namespace}-${m.name}`
                                    : ""}
                                  {m.s === "modio"
                                    ? ` · game ${m.game_id} mod ${m.mod_id}`
                                    : ""}
                                </small>
                              </div>
                            </li>
                          ))}
                          {gameSavedDetail.mods.length === 0 && (
                            <li className="empty">No mods in this share.</li>
                          )}
                        </ul>
                        <h2>Code</h2>
                        <textarea
                          rows={4}
                          readOnly
                          value={gameSavedDetail.entry.code}
                        />
                      </>
                    ) : (
                      <>
                        {installedCollections.length > 0 && (
                          <div className="installed-collections">
                            <h2>Installed collections</h2>
                            <ul className="list">
                              {installedCollections.map((c) => {
                                const installedLabel = formatInstalledAt(
                                  c.installed_at,
                                );
                                return (
                                  <li key={c.id}>
                                    <div>
                                      <strong>{c.name}</strong>
                                      <small>
                                        {installedCollectionSourceLabel(c)}
                                        {` · ${c.mod_ids.length} mods`}
                                        {installedLabel
                                          ? ` · ${installedLabel}`
                                          : ""}
                                      </small>
                                    </div>
                                    <div className="actions">
                                      <button
                                        type="button"
                                        className={
                                          modsCollectionFilter === c.id
                                            ? "active"
                                            : ""
                                        }
                                        onClick={() => {
                                          setModsCollectionFilter(c.id);
                                          setGameDetailTab("mods");
                                        }}
                                      >
                                        {modsCollectionFilter === c.id
                                          ? "Showing"
                                          : "Filter"}
                                      </button>
                                      <button
                                        type="button"
                                        className="danger"
                                        onClick={() =>
                                          void uninstallInstalledCollection(c)
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
                        <h2>Saved share codes</h2>
                        <p className="note">
                          Emperor share codes that match this game (Nexus domain,
                          Thunderstore community, mod.io ID, or source game). Create
                          one from Mods → Share loadout, or paste a code under Mods →
                          Import code → Save only.
                        </p>
                        <ul className="list">
                          {gameSavedCollections.map((c) => (
                            <li
                              key={c.id}
                              className="list-row-clickable"
                              onClick={() => void openGameSavedCollection(c.id)}
                            >
                              <div className="list-row-main">
                                <strong>{c.name}</strong>
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
                                  {` · ${new Date(c.created_at).toLocaleString()}`}
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
                              from Mods or save a code from Import code.
                            </li>
                          )}
                        </ul>
                      </>
                    )}
                  </div>
                )}
                {libraryDetail.tab === "tools" && settings?.platform_linux && (
                  <GameToolsPanel game={libraryDetail.game} />
                )}
                </ViewSwitch>
              </>
            ) : (
              <div>
                <div className="panel-head">
                  <h1>Library</h1>
                  <div className="panel-head-actions">
                    <Segment>
                      <SegmentTab
                        layoutId="library-view"
                        active={libraryView === "list"}
                        onClick={() => setLibraryView("list")}
                      >
                        List
                      </SegmentTab>
                      <SegmentTab
                        layoutId="library-view"
                        active={libraryView === "grid"}
                        onClick={() => setLibraryView("grid")}
                      >
                        Grid
                      </SegmentTab>
                    </Segment>
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
                {managed.length > 0 && (
                  <>
                    <h2>Managed</h2>
                    {libraryView === "grid" ? (
                      <FadeIn className="media-grid">
                        {managed.map((g, index) => {
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
                                  onClick={() => void unmanageFromLibrary(g)}
                                >
                                  Unmanage
                                </button>
                              </>
                            ) : health?.status === "missing" ? (
                              <button
                                type="button"
                                className="danger"
                                onClick={() => void unmanageFromLibrary(g)}
                              >
                                Unmanage
                              </button>
                            ) : undefined;
                          return (
                            <MediaCard
                              key={g.id}
                              enterIndex={index}
                              title={g.title}
                              imageSrc={mediaSrc(g.cover_path)}
                              badge={badge}
                              overlay={
                                health?.status === "missing"
                                  ? g.install_path
                                  : health?.new_install_path ?? undefined
                              }
                              actions={actions}
                              onClick={() => openGame(g, "mods")}
                            />
                          );
                        })}
                      </FadeIn>
                    ) : (
                      <ul className="list">
                        {managed.map((g) => {
                          const health = gameHealthById.get(g.id);
                          const badge = managedHealthBadge(health);
                          return (
                            <li
                              key={g.id}
                              className="list-row-clickable"
                              onClick={() => openGame(g, "mods")}
                              onKeyDown={(e) => {
                                if (e.key === "Enter" || e.key === " ") {
                                  e.preventDefault();
                                  openGame(g, "mods");
                                }
                              }}
                              role="button"
                              tabIndex={0}
                            >
                              <div>
                                <strong>{g.title}</strong>
                                {badge ? (
                                  <span className="pill warn">{badge}</span>
                                ) : null}
                                <small>
                                  {g.launcher} · {g.nexus_domain || g.thunderstore_community || g.plugin_id}
                                  {health?.status === "missing"
                                    ? ` · ${g.install_path}`
                                    : health?.new_install_path
                                      ? ` → ${health.new_install_path}`
                                      : ""}
                                </small>
                              </div>
                              {(health?.status === "relocate_candidate" ||
                                health?.status === "missing") && (
                                <div
                                  className="actions"
                                  onClick={(e) => e.stopPropagation()}
                                >
                                  {health.status === "relocate_candidate" ? (
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
                                  ) : null}
                                  <button
                                    type="button"
                                    className="danger"
                                    onClick={() => void unmanageFromLibrary(g)}
                                  >
                                    Unmanage
                                  </button>
                                </div>
                              )}
                            </li>
                          );
                        })}
                      </ul>
                    )}
                  </>
                )}
                <h2>Unmanaged</h2>
                {catalogLookupBusy && unmanaged.length > 0 && (
                  <p className="note">Looking up Nexus / Thunderstore / mod.io…</p>
                )}
                {libraryView === "grid" ? (
                  <FadeIn className="media-grid">
                    {unmanaged.map((g, index) => {
                      const suggestion = catalogSuggestions[g.id];
                      const hints = catalogHintTags(suggestion);
                      if (g.supported) {
                        return (
                          <MediaCard
                            key={g.id}
                            enterIndex={index}
                            title={g.title}
                            imageSrc={mediaSrc(g.cover_path)}
                            coverTags={hints}
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
                            enterIndex={index}
                            title={g.title}
                            imageSrc={mediaSrc(g.cover_path)}
                            badge={catalogMatchBadge(kind)}
                            coverTags={hints}
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
                            enterIndex={index}
                            title={g.title}
                            imageSrc={mediaSrc(g.cover_path)}
                            badge={catalogMatchBadge(kind)}
                            coverTags={hints}
                            hoverLabel={catalogMatchHover(kind)}
                            onClick={() =>
                              ready
                                ? manageBepinexQuick(g)
                                : beginBepinexManage(g)
                            }
                          />
                        );
                      }
                      return (
                        <MediaCard
                          key={g.id}
                          enterIndex={index}
                          title={g.title}
                          imageSrc={mediaSrc(g.cover_path)}
                          coverTags={hints}
                          coverLabel="Unsupported"
                          unsupported
                        />
                      );
                    })}
                    {unmanaged.length === 0 && (
                      <p className="note">
                        No unmanaged games found. Scan again if you installed something
                        while the app was open.
                      </p>
                    )}
                  </FadeIn>
                ) : (
                  <ul className="list">
                    {unmanaged.map((g) => {
                      const suggestion = catalogSuggestions[g.id];
                      const hintLine = catalogHintLine(suggestion);
                      const kind =
                        !g.supported &&
                        (g.engine_hint === "unreal" || g.engine_hint === "bepinex") &&
                        g.install_path
                          ? catalogMatchCardKind(suggestion)
                          : null;
                      return (
                        <li key={g.id}>
                          <div>
                            <strong>{g.title}</strong>
                            <small>
                              {g.launcher}
                              {g.install_path ? ` · ${g.install_path}` : ""}
                              {!g.supported &&
                              g.engine_hint !== "unreal" &&
                              g.engine_hint !== "bepinex"
                                ? " · unsupported"
                                : ""}
                              {g.engine_hint === "unreal" && !g.supported
                                ? " · Unreal Engine"
                                : ""}
                              {g.engine_hint === "bepinex" && !g.supported
                                ? " · Unity / BepInEx"
                                : ""}
                              {kind === "needs" ? " · needs catalog" : ""}
                              {kind === "review" ? " · review match" : ""}
                              {hintLine ? ` · ${hintLine}` : ""}
                            </small>
                          </div>
                          <div className="actions">
                            {g.supported ? (
                              <button type="button" onClick={() => manage(g)}>
                                Manage
                              </button>
                            ) : g.engine_hint === "unreal" && g.install_path ? (
                              <button
                                type="button"
                                onClick={() =>
                                  kind === "ready"
                                    ? manageUnrealQuick(g)
                                    : beginUnrealManage(g)
                                }
                              >
                                {catalogMatchActionLabel(kind ?? "needs")}
                              </button>
                            ) : g.engine_hint === "bepinex" && g.install_path ? (
                              <button
                                type="button"
                                onClick={() =>
                                  kind === "ready"
                                    ? manageBepinexQuick(g)
                                    : beginBepinexManage(g)
                                }
                              >
                                {catalogMatchActionLabel(kind ?? "needs")}
                              </button>
                            ) : null}
                          </div>
                        </li>
                      );
                    })}
                    {unmanaged.length === 0 && (
                      <li className="empty">
                        No unmanaged games found. Scan again if you installed something
                        while the app was open.
                      </li>
                    )}
                  </ul>
                )}
              </div>
            )}
            </ViewSwitch>
          </section>
        )}

        {tab === "browse" && (
          <ViewSwitch
            viewKey={
              browseDetail
                ? browseDetail.kind === "mod"
                  ? `mod:${browseDetail.hit.id}`
                  : `collection:${browseDetail.hit.slug}`
                : "browse-results"
            }
            className={browseDetail ? "panel" : "panel browse-workspace"}
            initial={false}
          >
            {browseDetail ? (
              <>
                <div className="panel-head">
                  <div className="detail-head-title">
                    <button type="button" onClick={() => closeBrowseDetail()}>
                      Home
                    </button>
                    <h1>
                      {browseDetail.kind === "mod"
                        ? (modDetail?.name ??
                          tsDetail?.name ??
                          modioDetail?.name ??
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

                <ViewSwitch
                  viewKey={`${browseDetail.kind}:${browseDetail.tab}:${
                    browseDetail.kind === "mod" ? browseDetail.hit.id : browseDetail.hit.slug
                  }`}
                  initial={false}
                >
                {browseDetail.kind === "mod" ? (
                  <>
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
                          {modDeepLinkFromHit(browseDetail.hit) ? (
                            <button
                              type="button"
                              className="icon-btn"
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

                    {browseDetail.tab === "info" ? (
                      <div className="detail-body">
                        {browseDetail.hit.source !== "thunderstore" &&
                          browseDetail.hit.source !== "modio" && (
                            <ImageCarousel slides={modDetail?.image_urls ?? []} />
                          )}
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
                    ) : (
                      <div className="detail-body">
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

                    {browseDetail.tab === "info" ? (
                      <div className="detail-body">
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
                    ) : collectionHitSource(browseDetail.hit) === "thunderstore" ? (
                      <div className="detail-body">
                        <h2>
                          Included mods (
                          {(tsDetail?.versions[0]?.dependencies.length ?? 0) + 1})
                        </h2>
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
                    ) : (
                      <div className="detail-body">
                        {(() => {
                          const groups = groupCollectionMods(collectionModFiles);
                          return (
                            <>
                              <h2>
                                Included mods ({groups.length})
                                {collectionModFiles.length > 0 &&
                                collectionModFiles.length !== groups.length
                                  ? ` · ${collectionModFiles.length} files`
                                  : ""}
                              </h2>
                              <ul className="list">
                                {groups.map((group) => {
                                  const primary = group.files[0];
                                  const extras =
                                    group.files.length > 1
                                      ? ` · ${group.files.length} files`
                                      : "";
                                  return (
                                    <li
                                      key={group.mod_id}
                                      className="list-row-clickable"
                                      onClick={() =>
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
                                        )
                                      }
                                    >
                                      <div>
                                        <strong>{group.mod_name}</strong>
                                        <small>
                                          {primary.file_name}
                                          {primary.version ? ` · v${primary.version}` : ""}
                                          {primary.optional ? " · optional" : ""}
                                          {extras}
                                        </small>
                                      </div>
                                    </li>
                                  );
                                })}
                                {collectionModFiles.length === 0 && (
                                  <li className="empty">No mods found in this collection.</li>
                                )}
                              </ul>
                            </>
                          );
                        })()}
                      </div>
                    )}
                  </>
                )}
                </ViewSwitch>
              </>
            ) : (
              <div className="view-enter">
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
                              onClick={() => setSourceFilter(id)}
                            >
                              {label}
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
                          min={120}
                          max={280}
                          step={10}
                          value={browseGridMin}
                          aria-label="Grid size"
                          onChange={(e) => setBrowseGridMin(Number(e.target.value))}
                        />
                      </label>
                    )}
                    {browseView === "list" && (
                      <Segment>
                        <SegmentTab
                          layoutId="browse-density"
                          active={browseListDensity === "compact"}
                          onClick={() => setBrowseListDensity("compact")}
                        >
                          Compact
                        </SegmentTab>
                        <SegmentTab
                          layoutId="browse-density"
                          active={browseListDensity === "posters"}
                          onClick={() => setBrowseListDensity("posters")}
                        >
                          Posters
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
                                  {hit.source === "thunderstore"
                                    ? "Thunderstore"
                                    : "Nexus"}
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
                        className="icon-btn"
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
                                orderedTags.map((t) => {
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
                                })
                              )}
                            </div>
                        </PopIn>
                      </div>
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
                    {browseMode === "mods" ? (
                      browseView === "grid" ? (
                        <FadeIn key={modHits[0]?.id ?? `mods:${searchQuery}`}>
                          {modHits.length === 0 ? (
                            <p className="note">{browseResultsNote("mods")}</p>
                          ) : (
                            <BrowseVirtualGrid
                              items={modHits}
                              scrollRef={browseResultsRef}
                              minCardPx={browseGridMin}
                              onPageSize={onBrowsePageSize}
                              getKey={(m) => m.id}
                              renderItem={(m) => {
                                const sourceLabel =
                                  m.source === "thunderstore"
                                    ? "Thunderstore"
                                    : m.source === "modio"
                                      ? "mod.io"
                                      : "Nexus";
                                const meta = [sourceLabel, m.category]
                                  .filter(Boolean)
                                  .join(" · ");
                                return (
                                  <MediaCard
                                    plain
                                    title={m.name}
                                    imageSrc={mediaSrc(null, m.picture_url)}
                                    badge={m.author ?? "Mod"}
                                    overlay={
                                      m.source === "nexus" && m.endorsements != null
                                        ? `${m.endorsements.toLocaleString()} endorsements`
                                        : m.downloads != null
                                          ? `${m.downloads.toLocaleString()} downloads`
                                          : null
                                    }
                                    meta={meta}
                                    installed={installedIds.has(m.id)}
                                    onClick={() => openMod(m, "info")}
                                    coverActions={
                                      m.source === "thunderstore" ? (
                                        installedIds.has(m.id) ? (
                                          <button
                                            type="button"
                                            className="danger"
                                            onClick={(e) => {
                                              e.stopPropagation();
                                              void uninstallCatalogMod(m);
                                            }}
                                          >
                                            Uninstall
                                          </button>
                                        ) : (
                                          <button
                                            onClick={(e) => {
                                              e.stopPropagation();
                                              openMod(m, "info");
                                            }}
                                          >
                                            Install
                                          </button>
                                        )
                                      ) : m.source === "nexus" ? (
                                        <>
                                          {installedIds.has(m.id) ? (
                                            <button
                                              type="button"
                                              className="danger"
                                              onClick={(e) => {
                                                e.stopPropagation();
                                                void uninstallCatalogMod(m);
                                              }}
                                            >
                                              Uninstall
                                            </button>
                                          ) : (
                                            <button
                                              type="button"
                                              onClick={(e) => {
                                                e.stopPropagation();
                                                void quickInstallMod(m);
                                              }}
                                            >
                                              {isPremium ? "Install" : "Download Assist"}
                                            </button>
                                          )}
                                          <button
                                            type="button"
                                            onClick={(e) => {
                                              e.stopPropagation();
                                              openMod(m, "files");
                                            }}
                                          >
                                            Files
                                          </button>
                                        </>
                                      ) : (
                                        <>
                                          {installedIds.has(m.id) ? (
                                            <button
                                              type="button"
                                              className="danger"
                                              onClick={(e) => {
                                                e.stopPropagation();
                                                void uninstallCatalogMod(m);
                                              }}
                                            >
                                              Uninstall
                                            </button>
                                          ) : null}
                                          <button
                                            onClick={(e) => {
                                              e.stopPropagation();
                                              openMod(m, "files");
                                            }}
                                          >
                                            Files
                                          </button>
                                        </>
                                      )
                                    }
                                  />
                                );
                              }}
                            />
                          )}
                        </FadeIn>
                      ) : modHits.length === 0 ? (
                        <ul className="list">
                          <li className="empty">{browseResultsNote("mods")}</li>
                        </ul>
                      ) : (
                        <BrowseVirtualList
                          items={modHits}
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
                                    {m.source === "thunderstore"
                                      ? "Thunderstore"
                                      : m.source === "modio"
                                        ? "mod.io"
                                        : "Nexus"}
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
                      <FadeIn
                        key={
                          collectionHits[0]
                            ? collectionHitKey(collectionHits[0])
                            : `collections:${searchQuery}`
                        }
                      >
                        {collectionHits.length === 0 ? (
                          <p className="note">{browseResultsNote("collections")}</p>
                        ) : (
                          <BrowseVirtualGrid
                            items={collectionHits}
                            scrollRef={browseResultsRef}
                            minCardPx={browseGridMin}
                            onPageSize={onBrowsePageSize}
                            getKey={collectionHitKey}
                            renderItem={(c) => {
                              const meta = [
                                collectionHitSource(c) === "thunderstore"
                                  ? "Thunderstore"
                                  : "Nexus",
                                c.mod_count != null
                                  ? `${c.mod_count.toLocaleString()} mods`
                                  : null,
                                c.file_size != null && c.file_size > 0
                                  ? formatBytes(c.file_size)
                                  : null,
                              ]
                                .filter(Boolean)
                                .join(" · ");
                              return (
                                <MediaCard
                                  plain
                                  title={c.name}
                                  imageSrc={mediaSrc(null, c.tile_image_url)}
                                  badge={
                                    collectionHitSource(c) === "thunderstore"
                                      ? "Modpack"
                                      : "Collection"
                                  }
                                  overlay={
                                    c.endorsements != null
                                      ? `${c.endorsements.toLocaleString()} endorsements`
                                      : c.total_downloads != null
                                        ? `${c.total_downloads.toLocaleString()} downloads`
                                        : null
                                  }
                                  meta={meta}
                                  onClick={() => openCollection(c, "info")}
                                  coverActions={
                                    <button
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        installCollection(c);
                                      }}
                                    >
                                      Install
                                    </button>
                                  }
                                />
                              );
                            }}
                          />
                        )}
                      </FadeIn>
                    ) : collectionHits.length === 0 ? (
                      <ul className="list">
                        <li className="empty">{browseResultsNote("collections")}</li>
                      </ul>
                    ) : (
                      <BrowseVirtualList
                        items={collectionHits}
                        scrollRef={browseResultsRef}
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
                                  {collectionHitSource(c) === "thunderstore"
                                    ? "Thunderstore"
                                    : "Nexus"}
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
                    <div
                      ref={browseSentinelRef}
                      className="browse-scroll-sentinel"
                      aria-hidden="true"
                    />
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
                ) : null}
              </div>
            )}
          </ViewSwitch>
        )}

        {tab === "collections" && (
          <section className="panel">
            <ViewSwitch
              viewKey={savedDetail ? `saved:${savedDetail.entry.id}` : "saved-list"}
              initial={false}
            >
            {savedDetail ? (
              <div>
                <div className="panel-head">
                  <button
                    type="button"
                    className="linkish"
                    onClick={() => {
                      setSavedDetail(null);
                      setSavedRename("");
                    }}
                  >
                    Home
                  </button>
                  <h1>{savedDetail.entry.name}</h1>
                </div>
                <div className="detail-body">
                  <p className="note">
                    {savedDetail.entry.mod_count} mods
                    {savedDetail.entry.nexus_count
                      ? ` · ${savedDetail.entry.nexus_count} Nexus`
                      : ""}
                    {savedDetail.entry.thunderstore_count
                      ? ` · ${savedDetail.entry.thunderstore_count} Thunderstore`
                      : ""}
                    {savedDetail.entry.modio_count
                      ? ` · ${savedDetail.entry.modio_count} mod.io`
                      : ""}
                    {formatCollectionGameVersions(savedDetail.entry.game_versions)
                      ? ` · Built for ${formatCollectionGameVersions(savedDetail.entry.game_versions)}`
                      : ""}
                    {isSavedCollectionInstalled(savedDetail.entry) ? (
                      <>
                        {" "}
                        <span className="pill ok">Installed</span>
                      </>
                    ) : null}
                  </p>
                  {showGameVersionWarnings &&
                    savedCollectionAdvice[savedDetail.entry.id]?.status ===
                      "mismatch" && (
                      <p className="mod-version-warn">
                        {savedCollectionAdvice[savedDetail.entry.id]?.message}
                      </p>
                    )}
                  <label>
                    Name
                    <div className="row">
                      <input
                        value={savedRename}
                        onChange={(e) => setSavedRename(e.target.value)}
                      />
                      <button
                        type="button"
                        onClick={() =>
                          void (async () => {
                            try {
                              const updated = await api.renameSavedCollection(
                                savedDetail.entry.id,
                                savedRename,
                              );
                              setSavedDetail({ ...savedDetail, entry: updated });
                              await refreshSavedCollections();
                            } catch (e) {
                              setError(String(e));
                            }
                          })()
                        }
                      >
                        Rename
                      </button>
                    </div>
                  </label>
                  <label>
                    Install into
                    <select
                      value={savedInstallGameId}
                      onChange={(e) => setSavedInstallGameId(e.target.value)}
                    >
                      <option value="">Select managed game…</option>
                      {managed.map((g) => (
                        <option key={g.id} value={g.id}>
                          {g.title}
                          {savedCollectionMatchesGame(savedDetail.entry, g)
                            ? " (match)"
                            : ""}
                        </option>
                      ))}
                    </select>
                  </label>
                  <div className="actions" style={{ marginBottom: "1rem" }}>
                    <button type="button" onClick={() => void installSavedCollection()}>
                      Install
                    </button>
                    <button
                      type="button"
                      className="icon-btn"
                      aria-label="Copy code"
                      title="Copy code"
                      onClick={() =>
                        void navigator.clipboard.writeText(savedDetail.entry.code)
                      }
                    >
                      <FontAwesomeIcon icon={faCopy} />
                    </button>
                    <button
                      type="button"
                      className="danger icon-btn"
                      aria-label={`Delete ${savedDetail.entry.name}`}
                      title="Delete"
                      onClick={() =>
                        void (async () => {
                          if (!window.confirm(`Delete ${savedDetail.entry.name}?`)) {
                            return;
                          }
                          try {
                            await deleteSavedCollectionEntry(savedDetail.entry);
                            setSavedDetail(null);
                          } catch (e) {
                            setError(String(e));
                          }
                        })()
                      }
                    >
                      <FontAwesomeIcon icon={faTrashCan} />
                    </button>
                  </div>
                  <h2>Contained mods</h2>
                  <ul className="list">
                    {savedDetail.mods.map((m, i) => (
                      <li key={`${m.s}-${i}`}>
                        <div>
                          <strong>{shareModLabel(m)}</strong>
                          <small>
                            {m.s}
                            {m.version ? ` · v${m.version}` : ""}
                            {m.s === "nexus" && m.mod_id != null
                              ? ` · ${m.domain} ${m.mod_id}/${m.file_id}`
                              : ""}
                            {m.s === "thunderstore"
                              ? ` · ${m.community}/${m.namespace}-${m.name}`
                              : ""}
                            {m.s === "modio"
                              ? ` · game ${m.game_id} mod ${m.mod_id}`
                              : ""}
                          </small>
                        </div>
                      </li>
                    ))}
                    {savedDetail.mods.length === 0 && (
                      <li className="empty">No mods in this share.</li>
                    )}
                  </ul>
                  <h2>Code</h2>
                  <textarea rows={4} readOnly value={savedDetail.entry.code} />
                </div>
              </div>
            ) : (
              <div>
                <div className="panel-head">
                  <h1>Saved Collections</h1>
                  <div className="panel-head-actions">
                    <button
                      type="button"
                      className="icon-btn"
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
                  game → Mods → Share loadout, or import a code or file here.
                </p>
                <ul className="list collections-by-game">
                  {savedCollectionGameGroups.map((group) => {
                    const expanded = expandedCollectionGameKey === group.key;
                    return (
                      <li
                        key={group.key}
                        className="collections-game-row list-row-poster"
                      >
                        <div
                          className="collections-game-header list-row-clickable"
                          aria-expanded={expanded}
                          onClick={() =>
                            setExpandedCollectionGameKey(
                              expanded ? null : group.key,
                            )
                          }
                          onKeyDown={(e: ReactKeyboardEvent<HTMLDivElement>) => {
                            if (e.key === "Enter" || e.key === " ") {
                              e.preventDefault();
                              setExpandedCollectionGameKey(
                                expanded ? null : group.key,
                              );
                            }
                          }}
                          role="button"
                          tabIndex={0}
                        >
                          <ListPoster src={mediaSrc(group.coverPath)} />
                          <div className="list-row-main">
                            <strong>{group.title}</strong>
                            <small>
                              {group.collections.length} collection
                              {group.collections.length === 1 ? "" : "s"}
                              {group.managed ? " · managed" : " · not managed"}
                            </small>
                          </div>
                        </div>
                        <Collapse
                          show={expanded}
                          className="collections-game-expand"
                        >
                          <ul className="list collections-game-children">
                            {group.collections.map((c) => {
                              const builtFor = formatCollectionGameVersions(
                                c.game_versions,
                              );
                              const advice = savedCollectionAdvice[c.id];
                              const installed = isSavedCollectionInstalled(c);
                              return (
                                <li
                                  key={c.id}
                                  className="list-row-clickable"
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
                                      {` · ${new Date(c.created_at).toLocaleString()}`}
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
                                    <button
                                      type="button"
                                      className="danger icon-btn"
                                      aria-label={`Delete ${c.name}`}
                                      title="Delete"
                                      onClick={() =>
                                        void (async () => {
                                          if (
                                            !window.confirm(`Delete ${c.name}?`)
                                          ) {
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
                            })}
                          </ul>
                        </Collapse>
                      </li>
                    );
                  })}
                  {savedCollectionGameGroups.length === 0 && (
                    <li className="empty">
                      No saved share codes yet. Share a loadout from a managed game.
                    </li>
                  )}
                </ul>
              </div>
            )}
            </ViewSwitch>
          </section>
        )}

        {tab === "downloads" && (
          <DownloadsWorkspace
            downloads={downloads}
            queue={assistQueue}
            activeBatch={activeBatch}
            assistActive={assistActive}
            assistHint={assistHint}
            onRefresh={refreshDownloads}
            onSkip={() => skipAssistCurrent()}
            onCancel={() => cancelAssistQueue()}
            onCancelRemaining={() => cancelRemainingBatch()}
            onCancelDownload={(id) => cancelOneDownload(id)}
            onRestartDownload={(id) => restartOneDownload(id)}
            onForceResetDownload={(id) => forceResetOneDownload(id)}
            onRetryAssistOpening={() => retryAssistOpening()}
            isDownloadStuck={isDownloadStuck}
            onClearRecent={(ids) => clearRecentDownloads(ids)}
            onOpenRecent={(item) => void openRecentDownload(item)}
            onRemoveDownload={(id) => removeOneDownload(id)}
            onPauseDownload={(id) => pauseOneDownload(id)}
            onResumeDownload={(id) => resumeOneDownload(id)}
            onAssistHost={handleAssistHost}
          />
        )}

        {tab === "tools" && (
          <ToolsErrorBoundary>
            <ToolsWorkspace />
          </ToolsErrorBoundary>
        )}

        {tab === "settings" && (
          <section className="panel">
            <h1>Settings</h1>
            <SettingsSection title="App updates">
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
            <SettingsSection title="Accounts">
              {accountConnections}
            </SettingsSection>
          </section>
        )}
        </ViewSwitch>
        ) : null}
      </main>
      {loadDiagnosis && (
        <LoadLogDialog
          diagnosis={loadDiagnosis}
          gameTitle={activeGame?.title}
          onClose={() => setLoadDiagnosis(null)}
        />
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
      {nexusLoginOpen && (
        <Modal
          className="modal-nexus-login"
          title="Log in to Nexus"
          subtitle="Use Stay signed in. This window closes after Nexus accepts the login."
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
              <button
                type="button"
                onClick={() => void confirmNexusLogin()}
                disabled={nexusLoginConfirming}
              >
                {nexusLoginConfirming ? "Checking…" : "I've signed in"}
              </button>
            </div>
          }
        >
          <div ref={nexusLoginHostRef} className="nexus-login-host">
            <p className="note">Opening Nexus…</p>
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
      {fomodFor && activeGame && (
        <FomodDialog
          gameId={activeGame.id}
          mod={fomodFor}
          onClose={() => setFomodFor(null)}
          onSaved={(selection) => {
            setMods((prev) =>
              prev.map((m) =>
                m.id === fomodFor.id ? { ...m, fomod_selection: selection } : m,
              ),
            );
            setNotice({
              kind: "ok",
              message: `Saved installer choices for ${fomodFor.name}. Deploy to apply them.`,
            });
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
              message: `Saved options for ${modOptionsFor.name}. Deploy to apply them.`,
            });
          }}
        />
      )}
    </div>
  );
}
