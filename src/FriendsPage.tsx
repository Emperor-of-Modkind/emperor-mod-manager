import {
  startTransition,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { listen } from "@tauri-apps/api/event";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faCheck,
  faChevronRight,
  faCopy,
  faLock,
  faRotate,
  faRss,
  faShareNodes,
  faTrash,
  faUserPlus,
  faUsers,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import { api } from "./api";
import {
  formatActiveLoadoutMatchLabel,
  getCollectionsSyncedByFriend,
  getFriendActiveLoadoutMatches,
} from "./collectionSyncers";
import {
  formatMutualSummary,
  getMutualFriends,
  getMutualFriendsForPeerId,
} from "./friendConnections";
import {
  catalogCoverRequestKey,
  collectionCatalogCoverRequests,
  mergeCollectionCatalogCovers,
  mergeCollectionCatalogTitles,
} from "./catalogCovers";
import {
  CollectionDetailView,
  CollectionPlaceholderIcon,
  formatCollectionDate,
} from "./CollectionDetailView";
import { CollectionsGameList } from "./CollectionsGameList";
import { extractFriendJoinCode, friendShareUrlFromCode } from "./modDeepLink";
import { FriendAvatar } from "./FriendAvatar";
import { Modal } from "./Modal";
import { Segment, SegmentTab, staggerStyle } from "./motion";
import { groupSavedCollectionsByGame } from "./savedCollectionGroups";
import { BrowseListSkeleton } from "./Skeleton";
import type {
  CatalogHit,
  CollectionPlanSource,
  CollectionResolutionChoices,
  DetectedGame,
  FriendFeedDiff,
  FriendFeedEvent,
  FriendRecord,
  FriendShareRef,
  FriendsState,
  FriendSyncResult,
  GamePluginInfo,
  InstalledCollection,
  ManagedGame,
  PlayingGameStatus,
  SavedCollectionEntry,
  ShareDecodePreview,
} from "./types";

function getFriendPlayingGame(friend: FriendRecord): PlayingGameStatus | null {
  if (!friend.online) return null;
  return friend.cached_profile?.playing_game ?? null;
}

function formatPlayingGameLabel(playing: PlayingGameStatus): string {
  const modeSuffix =
    playing.mode === "modded"
      ? " (Modded)"
      : playing.mode === "unmodded"
        ? " (Unmodded)"
        : "";
  return `Playing ${playing.game_title}${modeSuffix}`;
}

function formatOnlineStatusLabel(friend: FriendRecord): string {
  const playing = getFriendPlayingGame(friend);
  return playing ? `Online · ${formatPlayingGameLabel(playing)}` : "Online";
}

export type FriendsPageProps = {
  active: boolean;
  managed: ManagedGame[];
  detected?: DetectedGame[];
  plugins?: GamePluginInfo[];
  catalogCovers?: Record<string, string | null>;
  catalogTitles?: Record<string, string | null>;
  activeGameId: string | null;
  scrollRef?: RefObject<HTMLElement | null>;
  installedShareIds?: Set<string>;
  resolveGameCover?: (game: ManagedGame) => string | null;
  renderPoster?: (coverPath: string | null) => ReactNode;
  onError: (msg: string | null) => void;
  onNotice: (msg: string | null) => void;
  onInstallCollection?: (
    gameId: string,
    code: string,
    name: string,
  ) => Promise<void> | void;
  onSaveCollection?: (
    code: string,
    name: string,
    sourceGameId: string | null,
  ) => Promise<void> | void;
  onCollectionSynced: (
    gameId: string,
    result: FriendSyncResult,
    friendSync?: { peerId: string; friendCollectionId: string },
  ) => Promise<void> | void;
  onFriendsRefreshed?: () => Promise<void> | void;
  resolveCollectionConflicts?: (
    gameId: string,
    source: CollectionPlanSource,
  ) => Promise<CollectionResolutionChoices | null>;
  onOpenMod?: (hit: CatalogHit, targetGameId?: string | null) => void;
  pendingJoinCode?: string | null;
  onConsumePendingJoinCode?: () => void;
};

function gameMatchesHint(
  game: ManagedGame,
  hint: ShareDecodePreview["game"],
): boolean {
  const h = hint;
  if (h.id && game.id === h.id) return true;
  if (h.plugin_id && game.plugin_id === h.plugin_id) return true;
  if (h.nexus_domain && game.nexus_domain === h.nexus_domain) return true;
  if (
    h.thunderstore_community &&
    game.thunderstore_community === h.thunderstore_community
  ) {
    return true;
  }
  if (h.modio_game_id != null && game.modio_game_id === h.modio_game_id) {
    return true;
  }
  return false;
}

function hasKnownGameHint(hint: ShareDecodePreview["game"]): boolean {
  return Boolean(
    hint.id?.trim() ||
      hint.plugin_id?.trim() ||
      hint.nexus_domain?.trim() ||
      hint.thunderstore_community?.trim() ||
      (hint.modio_game_id != null && hint.modio_game_id > 0) ||
      hint.title?.trim(),
  );
}

function formatLastSeen(iso?: string | null): string {
  if (!iso) return "Never";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

function formatRelativeTime(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const diffSec = Math.floor((Date.now() - d.getTime()) / 1000);
  if (diffSec < 60) return "Just now";
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  const diffDays = Math.floor(diffHr / 24);
  if (diffDays < 7) return `${diffDays}d ago`;
  return d.toLocaleDateString();
}

function formatTimelineMilestone(iso: string): { key: string; label: string } {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) {
    return { key: "unknown", label: "Recent" };
  }
  const key = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
  const now = new Date();
  const todayKey = `${now.getFullYear()}-${String(now.getMonth() + 1).padStart(2, "0")}-${String(now.getDate()).padStart(2, "0")}`;
  const yesterday = new Date(now);
  yesterday.setDate(now.getDate() - 1);
  const yesterdayKey = `${yesterday.getFullYear()}-${String(yesterday.getMonth() + 1).padStart(2, "0")}-${String(yesterday.getDate()).padStart(2, "0")}`;
  if (key === todayKey) return { key, label: "Today" };
  if (key === yesterdayKey) return { key, label: "Yesterday" };
  const label = d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
  return { key, label };
}

function formatTimelineDate(iso: string): string {
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  const diffMs = Date.now() - d.getTime();
  if (diffMs >= 0 && diffMs < 24 * 60 * 60 * 1000) {
    return formatRelativeTime(iso);
  }
  return d.toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
    year: "numeric",
  });
}

function feedDiffsEqual(
  a?: FriendFeedDiff | null,
  b?: FriendFeedDiff | null,
): boolean {
  if (!a && !b) return true;
  if (!a || !b) return false;
  return (
    a.mods_added_count === b.mods_added_count &&
    a.mods_removed_count === b.mods_removed_count &&
    a.mods_updated_count === b.mods_updated_count &&
    a.mods_added.length === b.mods_added.length &&
    a.mods_removed.length === b.mods_removed.length &&
    a.mods_updated.length === b.mods_updated.length &&
    a.mods_added.every((v, i) => v === b.mods_added[i]) &&
    a.mods_removed.every((v, i) => v === b.mods_removed[i]) &&
    a.mods_updated.every((v, i) => v === b.mods_updated[i])
  );
}

const FEED_DEDUPE_WINDOW_MS = 5 * 60 * 1000;

function dedupeActiveLoadoutEvents(
  events: FriendFeedEvent[],
): FriendFeedEvent[] {
  const saved = events.filter((ev) => ev.collection_kind === "saved_collection");
  if (saved.length === 0) return events;
  return events.filter((ev) => {
    if (ev.collection_kind !== "active_loadout") return true;
    const evTime = Date.parse(ev.occurred_at);
    return !saved.some((s) => {
      if (
        Boolean(s.is_self) !== Boolean(ev.is_self) ||
        s.peer_id !== ev.peer_id ||
        s.kind !== ev.kind ||
        (s.game_title ?? null) !== (ev.game_title ?? null) ||
        (s.mod_count ?? null) !== (ev.mod_count ?? null) ||
        !feedDiffsEqual(s.diff, ev.diff)
      ) {
        return false;
      }
      const sTime = Date.parse(s.occurred_at);
      if (Number.isNaN(evTime) || Number.isNaN(sTime)) {
        return s.occurred_at === ev.occurred_at;
      }
      return Math.abs(sTime - evTime) <= FEED_DEDUPE_WINDOW_MS;
    });
  });
}

function formatDiffNames(names: string[], total: number): string {
  if (names.length === 0) return "";
  const extra = total - names.length;
  return extra > 0 ? `${names.join(", ")} +${extra} more` : names.join(", ");
}

function hasFeedDiffDetails(diff?: FriendFeedDiff | null): diff is FriendFeedDiff {
  if (!diff) return false;
  return (
    diff.mods_added_count > 0 ||
    diff.mods_removed_count > 0 ||
    diff.mods_updated_count > 0
  );
}

const EMPTY_DETECTED: DetectedGame[] = [];
const EMPTY_PLUGINS: GamePluginInfo[] = [];

export function FriendsPage({
  active,
  managed,
  detected = EMPTY_DETECTED,
  plugins = EMPTY_PLUGINS,
  catalogCovers,
  catalogTitles,
  activeGameId,
  scrollRef,
  installedShareIds,
  resolveGameCover,
  renderPoster,
  onError,
  onNotice,
  onInstallCollection,
  onSaveCollection,
  onCollectionSynced,
  onFriendsRefreshed,
  resolveCollectionConflicts,
  onOpenMod,
  pendingJoinCode,
  onConsumePendingJoinCode,
}: FriendsPageProps) {
  const [state, setState] = useState<FriendsState | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [autoChecking, setAutoChecking] = useState(false);
  const [displayNameInput, setDisplayNameInput] = useState("");
  const [addFriendOpen, setAddFriendOpen] = useState(false);
  const [addingFriend, setAddingFriend] = useState(false);
  const [joinCodeInput, setJoinCodeInput] = useState("");
  const [copiedCode, setCopiedCode] = useState(false);
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null);
  const [selectedCol, setSelectedCol] = useState<FriendShareRef | null>(null);
  const [friendsTab, setFriendsTab] = useState<"feed" | "friends">("feed");
  const [friendDetailTab, setFriendDetailTab] = useState<
    "feed" | "collections" | "friends"
  >("feed");
  const [addingPeerId, setAddingPeerId] = useState<string | null>(null);
  const [openedColFromFeed, setOpenedColFromFeed] = useState(false);
  const [feedFriendFilter, setFeedFriendFilter] = useState<string>("all");
  const [clearingFeed, setClearingFeed] = useState(false);
  const lastMarkedFeedTsRef = useRef<string | null>(null);
  const [decodedPreview, setDecodedPreview] =
    useState<ShareDecodePreview | null>(null);
  const [decoding, setDecoding] = useState(false);
  const [decodedByHash, setDecodedByHash] = useState<
    Record<string, ShareDecodePreview>
  >({});
  const [decodingLibrary, setDecodingLibrary] = useState(false);
  const [resolvedCatalogCovers, setResolvedCatalogCovers] = useState<
    Record<string, string>
  >({});
  const [resolvedCatalogTitles, setResolvedCatalogTitles] = useState<
    Record<string, string>
  >({});
  const coverAttemptedRef = useRef<Set<string>>(new Set());
  const [expandedFriendGameKey, setExpandedFriendGameKey] = useState<
    string | null
  >(null);
  const [installedByGame, setInstalledByGame] = useState<
    Record<string, InstalledCollection[]>
  >({});
  const [syncStatesByGame, setSyncStatesByGame] = useState<
    Record<string, Record<string, boolean>>
  >({});
  const [targetSyncStates, setTargetSyncStates] = useState<
    Record<string, boolean>
  >({});
  const [rowBusyKey, setRowBusyKey] = useState<string | null>(null);
  const [targetGameId, setTargetGameId] = useState<string>("");
  const [installedForTarget, setInstalledForTarget] = useState<
    InstalledCollection[]
  >([]);
  const [syncing, setSyncing] = useState(false);
  const [syncRefreshTick, setSyncRefreshTick] = useState(0);
  const [installing, setInstalling] = useState(false);
  const [saving, setSaving] = useState(false);

  const loadState = useCallback(async () => {
    try {
      const next = await api.getFriendsState();
      setState(next);
      setDisplayNameInput((prev) => (prev ? prev : next.display_name));
    } catch (e) {
      onError(String(e));
    } finally {
      setLoading(false);
    }
  }, [onError]);

  useEffect(() => {
    void loadState();
  }, [loadState]);

  useEffect(() => {
    const unlisten = listen("friends-changed", () => {
      void loadState();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [loadState]);

  useEffect(() => {
    const unlisten = listen<boolean>("friends-checking", (event) => {
      setAutoChecking(Boolean(event.payload));
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    if (active) {
      void loadState();
    }
  }, [active, loadState]);

  const selectedFriend: FriendRecord | null = useMemo(() => {
    if (!state || !selectedPeerId) return null;
    return state.friends.find((f) => f.peer_id === selectedPeerId) ?? null;
  }, [state, selectedPeerId]);

  useEffect(() => {
    setFriendDetailTab("feed");
  }, [selectedPeerId]);

  const friendsById = useMemo(() => {
    const map = new Map<string, FriendRecord>();
    for (const f of state?.friends ?? []) {
      map.set(f.peer_id, f);
    }
    return map;
  }, [state]);

  const feedEvents: FriendFeedEvent[] = useMemo(() => {
    const raw = state?.feed_events ?? [];
    if (raw.length === 0) return [];
    const visible = raw.filter((ev) => {
      if (ev.is_self) return true;
      const friend = friendsById.get(ev.peer_id);
      if (!friend) return false;
      // Ensure collection is still currently visible in the friend's cached profile.
      const cols = friend.cached_profile?.collections;
      if (cols && !cols.some((c) => c.id === ev.collection_id)) {
        return false;
      }
      return true;
    });
    return dedupeActiveLoadoutEvents(visible);
  }, [state, friendsById]);

  const feedLastReadAt = state?.feed_last_read_at ?? null;

  const unreadFeedCount = useMemo(() => {
    const friendEvents = feedEvents.filter((ev) => !ev.is_self);
    if (friendEvents.length === 0) return 0;
    if (!feedLastReadAt) return friendEvents.length;
    return friendEvents.filter((ev) => ev.occurred_at > feedLastReadAt).length;
  }, [feedEvents, feedLastReadAt]);

  const feedFilterFriends = useMemo(() => {
    const seen = new Map<string, string>();
    for (const ev of feedEvents) {
      if (ev.is_self) {
        if (!seen.has("self")) {
          const selfName = state?.display_name || ev.friend_display_name;
          seen.set("self", selfName ? `You (${selfName})` : "You");
        }
        continue;
      }
      if (!seen.has(ev.peer_id)) {
        const live = friendsById.get(ev.peer_id);
        seen.set(ev.peer_id, live?.display_name ?? ev.friend_display_name);
      }
    }
    return Array.from(seen.entries()).map(([peer_id, display_name]) => ({
      peer_id,
      display_name,
    }));
  }, [feedEvents, friendsById, state?.display_name]);

  const filteredFeedEvents = useMemo(() => {
    if (feedFriendFilter === "all") return feedEvents;
    if (feedFriendFilter === "self") {
      return feedEvents.filter((ev) => Boolean(ev.is_self));
    }
    return feedEvents.filter(
      (ev) => !ev.is_self && ev.peer_id === feedFriendFilter,
    );
  }, [feedEvents, feedFriendFilter]);

  const selectedFriendFeedEvents = useMemo(() => {
    if (!selectedFriend) return [];
    return feedEvents.filter(
      (ev) => !ev.is_self && ev.peer_id === selectedFriend.peer_id,
    );
  }, [feedEvents, selectedFriend]);

  useEffect(() => {
    if (
      feedFriendFilter !== "all" &&
      !feedFilterFriends.some((f) => f.peer_id === feedFriendFilter)
    ) {
      setFeedFriendFilter("all");
    }
  }, [feedFriendFilter, feedFilterFriends]);

  useEffect(() => {
    if (
      !active ||
      friendsTab !== "feed" ||
      selectedFriend ||
      selectedCol ||
      unreadFeedCount === 0
    ) {
      return;
    }
    const latestTs =
      feedEvents.find((ev) => !ev.is_self)?.occurred_at ?? null;
    if (!latestTs || lastMarkedFeedTsRef.current === latestTs) return;
    lastMarkedFeedTsRef.current = latestTs;
    void api
      .markFriendFeedRead()
      .then((next) => setState(next))
      .catch(() => {
        lastMarkedFeedTsRef.current = null;
      });
  }, [active, friendsTab, selectedFriend, selectedCol, unreadFeedCount, feedEvents]);

  useEffect(() => {
    const cols = selectedFriend?.cached_profile?.collections ?? [];
    if (!selectedFriend || cols.length === 0) {
      setDecodingLibrary(false);
      return;
    }
    const missing = cols.filter(
      (c) => c.share_code && !decodedByHash[c.share_hash],
    );
    if (missing.length === 0) {
      setDecodingLibrary(false);
      return;
    }
    let cancelled = false;
    setDecodingLibrary(true);
    void Promise.allSettled(
      missing.map(async (c) => ({
        hash: c.share_hash,
        preview: await api.decodeFriendShareCode(c.share_code),
      })),
    )
      .then((results) => {
        if (cancelled) return;
        setDecodedByHash((prev) => {
          const next = { ...prev };
          for (const res of results) {
            if (res.status === "fulfilled") {
              next[res.value.hash] = res.value.preview;
            }
          }
          return next;
        });
      })
      .finally(() => {
        if (!cancelled) setDecodingLibrary(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedFriend, decodedByHash]);

  const friendColById = useMemo(() => {
    const map = new Map<string, FriendShareRef>();
    for (const c of selectedFriend?.cached_profile?.collections ?? []) {
      map.set(c.id, c);
    }
    return map;
  }, [selectedFriend]);

  const friendSavedEntries: SavedCollectionEntry[] = useMemo(() => {
    const cols = selectedFriend?.cached_profile?.collections ?? [];
    return cols.map((col) => {
      const preview = decodedByHash[col.share_hash];
      return {
        id: col.id,
        name: col.name,
        code: col.share_code,
        created_at: col.updated_at,
        updated_at: col.updated_at,
        source_game_id: null,
        game: preview?.game ?? {},
        game_versions: null,
        mod_count: preview?.mods.length ?? 0,
        nexus_count: preview?.nexus_count ?? 0,
        thunderstore_count: preview?.thunderstore_count ?? 0,
        modio_count: preview?.modio_count ?? 0,
        share_id: col.share_hash,
      };
    });
  }, [selectedFriend, decodedByHash]);

  const mergedCatalogCovers = useMemo(
    () =>
      catalogCovers
        ? { ...catalogCovers, ...resolvedCatalogCovers }
        : resolvedCatalogCovers,
    [catalogCovers, resolvedCatalogCovers],
  );

  const mergedCatalogTitles = useMemo(
    () =>
      catalogTitles
        ? { ...catalogTitles, ...resolvedCatalogTitles }
        : resolvedCatalogTitles,
    [catalogTitles, resolvedCatalogTitles],
  );

  const friendGameGroups = useMemo(
    () =>
      groupSavedCollectionsByGame(
        friendSavedEntries,
        managed,
        detected,
        plugins,
        mergedCatalogCovers,
        mergedCatalogTitles,
      ),
    [
      friendSavedEntries,
      managed,
      detected,
      plugins,
      mergedCatalogCovers,
      mergedCatalogTitles,
    ],
  );

  useEffect(() => {
    if (!active || !selectedFriend || friendGameGroups.length === 0) return;
    const requests = collectionCatalogCoverRequests(
      friendGameGroups,
      plugins,
      detected,
    ).filter((req) => !coverAttemptedRef.current.has(catalogCoverRequestKey(req)));
    if (requests.length === 0) return;
    for (const req of requests) {
      coverAttemptedRef.current.add(catalogCoverRequestKey(req));
    }
    let cancelled = false;
    void api
      .resolveCatalogCovers(requests)
      .then((hits) => {
        if (cancelled || hits.length === 0) return;
        startTransition(() => {
          setResolvedCatalogCovers((prev) =>
            mergeCollectionCatalogCovers(prev, hits),
          );
          setResolvedCatalogTitles((prev) =>
            mergeCollectionCatalogTitles(prev, hits),
          );
        });
      })
      .catch((e) => {
        console.warn("resolve_catalog_covers (friends) failed", e);
      });
    return () => {
      cancelled = true;
    };
  }, [active, selectedFriend, friendGameGroups, plugins, detected]);

  const managedGroupIdsKey = useMemo(
    () =>
      Array.from(
        new Set(
          friendGameGroups
            .map((g) => g.managed?.id)
            .filter((id): id is string => Boolean(id)),
        ),
      )
        .sort()
        .join(","),
    [friendGameGroups],
  );

  useEffect(() => {
    if (!selectedFriend || !managedGroupIdsKey) {
      setInstalledByGame({});
      setSyncStatesByGame({});
      return;
    }
    const managedIds = managedGroupIdsKey.split(",");
    let cancelled = false;
    void Promise.allSettled(
      managedIds.map(async (gid) => {
        const [cols, syncStates] = await Promise.all([
          api.listInstalledCollections(gid),
          api.getFriendCollectionSyncStates
            ? api.getFriendCollectionSyncStates(gid).catch(() => [])
            : Promise.resolve([]),
        ]);
        const hasUpdateMap: Record<string, boolean> = {};
        for (const st of syncStates ?? []) {
          hasUpdateMap[st.collection_id] = st.has_update;
        }
        return { gid, cols, hasUpdateMap };
      }),
    ).then((results) => {
      if (cancelled) return;
      const next: Record<string, InstalledCollection[]> = {};
      const nextSync: Record<string, Record<string, boolean>> = {};
      for (const r of results) {
        if (r.status === "fulfilled") {
          next[r.value.gid] = r.value.cols;
          nextSync[r.value.gid] = r.value.hasUpdateMap;
        }
      }
      setInstalledByGame(next);
      setSyncStatesByGame(nextSync);
    });
    return () => {
      cancelled = true;
    };
  }, [selectedFriend, managedGroupIdsKey, state, syncRefreshTick]);

  useEffect(() => {
    if (!selectedCol || !selectedCol.share_code) {
      setDecodedPreview(null);
      return;
    }
    let cancelled = false;
    setDecoding(true);
    void api
      .decodeFriendShareCode(selectedCol.share_code)
      .then((preview) => {
        if (cancelled) return;
        setDecodedPreview(preview);
        const match = managed.find((g) => gameMatchesHint(g, preview.game));
        const fallback = hasKnownGameHint(preview.game)
          ? undefined
          : (managed.find((g) => g.id === activeGameId) ?? managed[0]);
        setTargetGameId((match ?? fallback)?.id ?? "");
      })
      .catch((e) => {
        if (!cancelled) onError(String(e));
      })
      .finally(() => {
        if (!cancelled) setDecoding(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedCol, managed, activeGameId, onError]);

  useEffect(() => {
    if (!targetGameId) {
      setInstalledForTarget([]);
      setTargetSyncStates({});
      return;
    }
    let cancelled = false;
    void Promise.all([
      api.listInstalledCollections(targetGameId).catch(() => []),
      api.getFriendCollectionSyncStates
        ? api.getFriendCollectionSyncStates(targetGameId).catch(() => [])
        : Promise.resolve([]),
    ]).then(([cols, syncStates]) => {
      if (cancelled) return;
      setInstalledForTarget(cols);
      const map: Record<string, boolean> = {};
      for (const st of syncStates ?? []) {
        map[st.collection_id] = st.has_update;
      }
      setTargetSyncStates(map);
    });
    return () => {
      cancelled = true;
    };
  }, [targetGameId, state, syncRefreshTick]);

  const linkedInstalled: InstalledCollection | null = useMemo(() => {
    if (!selectedFriend || !selectedCol) return null;
    return (
      installedForTarget.find(
        (c) =>
          c.friend_sync?.friend_peer_id === selectedFriend.peer_id &&
          c.friend_sync?.friend_collection_id === selectedCol.id,
      ) ?? null
    );
  }, [installedForTarget, selectedFriend, selectedCol]);

  const hasUpdate = Boolean(
    linkedInstalled &&
      selectedCol &&
      (targetSyncStates[linkedInstalled.id] ??
        linkedInstalled.friend_sync?.last_synced_hash !== selectedCol.share_hash),
  );

  async function handleCopyJoinCode() {
    if (!state?.join_code) return;
    try {
      const shareText = friendShareUrlFromCode(state.join_code) ?? state.join_code;
      await navigator.clipboard.writeText(shareText);
      setCopiedCode(true);
      onNotice("Friend invite link copied to clipboard.");
      setTimeout(() => setCopiedCode(false), 2000);
    } catch (e) {
      onError(String(e));
    }
  }

  useEffect(() => {
    const code = pendingJoinCode?.trim();
    if (!code) return;
    onError(null);
    setSelectedCol(null);
    setSelectedPeerId(null);
    setJoinCodeInput(code);
    setAddFriendOpen(true);
    onConsumePendingJoinCode?.();
  }, [pendingJoinCode, onConsumePendingJoinCode, onError]);

  async function handleSaveDisplayName(e: React.FormEvent) {
    e.preventDefault();
    onError(null);
    try {
      const next = await api.setP2pDisplayName(displayNameInput);
      setState(next);
      onNotice("Display name saved.");
    } catch (err) {
      onError(String(err));
    }
  }

  async function handleAddFriend(e: React.FormEvent) {
    e.preventDefault();
    onError(null);
    const trimmed = joinCodeInput.trim();
    if (!trimmed) return;
    const normalized = extractFriendJoinCode(trimmed) ?? trimmed;
    setAddingFriend(true);
    try {
      const next = await api.addFriendByCode(normalized);
      setState(next);
      setJoinCodeInput("");
      setAddFriendOpen(false);
      onNotice("Friend added! Connecting via P2P...");
    } catch (err) {
      onError(String(err));
    } finally {
      setAddingFriend(false);
    }
  }

  async function handleRespondRequest(peerId: string, accept: boolean) {
    onError(null);
    try {
      const next = await api.respondFriendRequest(peerId, accept);
      setState(next);
    } catch (err) {
      onError(String(err));
    }
  }

  async function handleAddFriendByPeer(peerId: string, displayName?: string) {
    onError(null);
    setAddingPeerId(peerId);
    try {
      const wasIncoming = Boolean(
        state?.pending_requests.some((r) => r.peer_id === peerId),
      );
      const next = await api.addFriendByPeer(peerId, displayName);
      setState(next);
      const isQueued = Boolean(
        next.queued_invites?.some((q) => q.peer_id === peerId),
      );
      onNotice(
        wasIncoming
          ? "Friend request accepted! Connecting via P2P..."
          : isQueued
            ? "Friend request queued — will be sent when they're online."
            : "Friend request sent! Connecting via P2P...",
      );
    } catch (err) {
      onError(String(err));
    } finally {
      setAddingPeerId(null);
    }
  }

  async function handleCancelQueuedInvite(peerId: string) {
    onError(null);
    try {
      const next = await api.cancelQueuedFriendInvite(peerId);
      setState(next);
      onNotice("Queued friend request cancelled.");
    } catch (err) {
      onError(String(err));
    }
  }

  async function handleRemoveFriend(peerId: string) {
    onError(null);
    try {
      const next = await api.removeFriend(peerId);
      setState(next);
      if (selectedPeerId === peerId) {
        setSelectedPeerId(null);
        setSelectedCol(null);
      }
    } catch (err) {
      onError(String(err));
    }
  }

  async function handleRefreshAll() {
    onError(null);
    setRefreshing(true);
    try {
      const next = await api.refreshFriends();
      setState(next);
      await onFriendsRefreshed?.();
      setSyncRefreshTick((t) => t + 1);
    } catch (err) {
      onError(String(err));
    } finally {
      setRefreshing(false);
    }
  }

  async function handleClearFeed() {
    onError(null);
    setClearingFeed(true);
    try {
      const next = await api.clearFriendFeed();
      setState(next);
      setFeedFriendFilter("all");
    } catch (err) {
      onError(String(err));
    } finally {
      setClearingFeed(false);
    }
  }

  function handleOpenFeedEvent(ev: FriendFeedEvent, fromGlobalFeed: boolean) {
    const friend = friendsById.get(ev.peer_id);
    if (!friend) return;
    const liveCol =
      friend.cached_profile?.collections.find((c) => c.id === ev.collection_id) ??
      null;
    setSelectedPeerId(friend.peer_id);
    if (liveCol) {
      setOpenedColFromFeed(fromGlobalFeed);
      setSelectedCol(liveCol);
    } else {
      setOpenedColFromFeed(false);
      setSelectedCol(null);
      setExpandedFriendGameKey(null);
    }
  }

  function renderFeedList(events: FriendFeedEvent[], fromGlobalFeed: boolean) {
    const rows: ReactNode[] = [];
    let lastMilestoneKey: string | null = null;

    events.forEach((ev, idx) => {
      const milestone = formatTimelineMilestone(ev.occurred_at);
      if (milestone.key !== lastMilestoneKey) {
        lastMilestoneKey = milestone.key;
        rows.push(
          <li
            key={`milestone:${milestone.key}`}
            className="friends-timeline-milestone"
          >
            <span className="friends-timeline-milestone-badge">
              {milestone.label}
            </span>
          </li>,
        );
      }

      const side = idx % 2 === 0 ? "left" : "right";
      const isSelf = Boolean(ev.is_self);
      const liveFriend = isSelf ? undefined : friendsById.get(ev.peer_id);
      const friendName = isSelf
        ? state?.display_name || ev.friend_display_name || "You"
        : (liveFriend?.display_name ?? ev.friend_display_name);
      const avatarSrc = isSelf
        ? state?.profile_picture
        : (liveFriend?.profile_picture ??
          liveFriend?.cached_profile?.profile_picture);
      const isUnread =
        !isSelf && (!feedLastReadAt || ev.occurred_at > feedLastReadAt);
      const diff = hasFeedDiffDetails(ev.diff) ? ev.diff : null;
      const dateLabel = formatTimelineDate(ev.occurred_at);
      const fullDateTitle = formatLastSeen(ev.occurred_at);

      const itemContent = (
        <>
          <FriendAvatar
            name={friendName}
            src={avatarSrc}
            online={liveFriend?.online ?? false}
            size="md"
            showStatus={Boolean(liveFriend)}
          />
          <div className="friends-feed-item-body">
            <div className="friends-feed-item-headline">
              <span className="friends-feed-item-text">
                <strong>{friendName}</strong>{" "}
                {ev.kind === "collection_added" ? (
                  <>
                    shared a new collection{" "}
                    <strong>{ev.collection_name}</strong>
                  </>
                ) : ev.kind === "collection_renamed" ? (
                  <>
                    renamed{" "}
                    <strong>{ev.previous_name ?? "a collection"}</strong> to{" "}
                    <strong>{ev.collection_name}</strong>
                  </>
                ) : (
                  <>
                    updated <strong>{ev.collection_name}</strong>
                    {ev.previous_name ? (
                      <>
                        {" "}
                        <span className="muted">
                          (renamed from {ev.previous_name})
                        </span>
                      </>
                    ) : null}
                  </>
                )}
              </span>
              <time
                className="muted friends-feed-time friends-feed-time-inline"
                dateTime={ev.occurred_at}
                title={fullDateTitle}
              >
                {dateLabel}
              </time>
            </div>

            <div className="friends-feed-meta">
              {isSelf ? <span className="pill">You</span> : null}
              <span className="pill">
                {ev.collection_kind === "active_loadout"
                  ? "Active Loadout"
                  : "Saved Collection"}
              </span>
              {(() => {
                if (isSelf || ev.collection_kind !== "active_loadout") {
                  return null;
                }
                const activeCol = liveFriend?.cached_profile?.collections.find(
                  (c) => c.id === ev.collection_id,
                );
                const match = activeCol?.matched_local_collection;
                if (!match) return null;
                return (
                  <span
                    className={`pill ${match.has_local_changes ? "warn" : "ok"}`}
                  >
                    {formatActiveLoadoutMatchLabel(match)}
                  </span>
                );
              })()}
              {ev.game_title ? (
                <span className="pill">{ev.game_title}</span>
              ) : null}
              {ev.mod_count != null ? (
                <span className="muted">
                  {ev.mod_count} {ev.mod_count === 1 ? "mod" : "mods"}
                </span>
              ) : null}
            </div>

            {diff && (
              <div className="friends-feed-diff">
                {diff.mods_added_count > 0 && (
                  <div className="friends-feed-diff-line added">
                    +{diff.mods_added_count} added
                    {diff.mods_added.length > 0
                      ? `: ${formatDiffNames(
                          diff.mods_added,
                          diff.mods_added_count,
                        )}`
                      : ""}
                  </div>
                )}
                {diff.mods_removed_count > 0 && (
                  <div className="friends-feed-diff-line removed">
                    -{diff.mods_removed_count} removed
                    {diff.mods_removed.length > 0
                      ? `: ${formatDiffNames(
                          diff.mods_removed,
                          diff.mods_removed_count,
                        )}`
                      : ""}
                  </div>
                )}
                {diff.mods_updated_count > 0 && (
                  <div className="friends-feed-diff-line updated">
                    ~{diff.mods_updated_count} updated
                    {diff.mods_updated.length > 0
                      ? `: ${formatDiffNames(
                          diff.mods_updated,
                          diff.mods_updated_count,
                        )}`
                      : ""}
                  </div>
                )}
              </div>
            )}
          </div>
          {!isSelf ? (
            <FontAwesomeIcon
              icon={faChevronRight}
              className="friends-timeline-chevron"
              aria-hidden="true"
            />
          ) : null}
        </>
      );

      const cardEl = (
        <div
          className={`friends-timeline-card${isUnread ? " unread" : ""}`}
        >
          {isSelf ? (
            <div className="friends-feed-item-static">{itemContent}</div>
          ) : (
            <button
              type="button"
              className="friends-feed-item-btn"
              onClick={() => handleOpenFeedEvent(ev, fromGlobalFeed)}
            >
              {itemContent}
            </button>
          )}
        </div>
      );

      const dateEl = (
        <div className="friends-timeline-date-col">
          <time
            className="muted friends-timeline-date"
            dateTime={ev.occurred_at}
            title={fullDateTitle}
          >
            {dateLabel}
          </time>
        </div>
      );

      rows.push(
        <li
          key={ev.id}
          className={`friends-feed-item friends-timeline-row ${side} stagger-item${
            isUnread ? " unread" : ""
          }`}
          style={staggerStyle(idx)}
        >
          <div className="friends-timeline-slot-left">
            {side === "left" ? cardEl : dateEl}
          </div>
          <div className="friends-timeline-axis" aria-hidden="true">
            <span
              className={`friends-timeline-dot${isUnread ? " unread" : ""}`}
            />
          </div>
          <div className="friends-timeline-slot-right">
            {side === "right" ? cardEl : dateEl}
          </div>
        </li>,
      );
    });

    return <ul className="friends-feed-list friends-timeline">{rows}</ul>;
  }

  async function handleJoinOrSync() {
    if (!selectedFriend || !selectedCol || !targetGameId) return;
    onError(null);
    setSyncing(true);
    try {
      let resolutions: CollectionResolutionChoices | null = null;
      if (resolveCollectionConflicts) {
        resolutions = await resolveCollectionConflicts(targetGameId, {
          kind: "friend_sync",
          peerId: selectedFriend.peer_id,
          friendCollectionId: selectedCol.id,
        });
        if (!resolutions) return;
      }
      const res = await api.joinOrSyncFriendCollection({
        gameId: targetGameId,
        peerId: selectedFriend.peer_id,
        friendCollectionId: selectedCol.id,
        ...(resolutions ? { resolutions } : {}),
      });
      await onCollectionSynced(targetGameId, res, {
        peerId: selectedFriend.peer_id,
        friendCollectionId: selectedCol.id,
      });
      const updatedCols = await api.listInstalledCollections(targetGameId);
      setInstalledForTarget(updatedCols);
    } catch (err) {
      onError(String(err));
    } finally {
      setSyncing(false);
    }
  }

  async function handleToggleAutoSync(autoSync: boolean) {
    if (!linkedInstalled || !targetGameId) return;
    try {
      await api.setFriendCollectionAutoSync(
        targetGameId,
        linkedInstalled.id,
        autoSync,
      );
      const updatedCols = await api.listInstalledCollections(targetGameId);
      setInstalledForTarget(updatedCols);
      if (autoSync && hasUpdate) {
        await handleJoinOrSync();
      }
    } catch (err) {
      onError(String(err));
    }
  }

  async function handleInstall() {
    if (!selectedCol || !onInstallCollection) return;
    if (!targetGameId) {
      onError("Select a managed game to install into.");
      return;
    }
    onError(null);
    setInstalling(true);
    try {
      await onInstallCollection(
        targetGameId,
        selectedCol.share_code,
        selectedCol.name,
      );
      const updatedCols = await api.listInstalledCollections(targetGameId);
      setInstalledForTarget(updatedCols);
    } catch (err) {
      onError(String(err));
    } finally {
      setInstalling(false);
    }
  }

  async function handleSave() {
    if (!selectedCol || !onSaveCollection) return;
    onError(null);
    setSaving(true);
    try {
      await onSaveCollection(
        selectedCol.share_code,
        selectedCol.name,
        targetGameId || null,
      );
    } catch (err) {
      onError(String(err));
    } finally {
      setSaving(false);
    }
  }

  if (loading && !state) {
    return (
      <section className="panel page-panel">
        <div className="page-chrome">
        <div className="panel-head">
          <h1>Friends</h1>
        </div>
        </div>
        <div className="page-scroll">
        <p className="muted">Starting P2P node...</p>
        </div>
      </section>
    );
  }

  if (state && !state.display_name_set) {
    return (
      <section className="panel page-panel friends-panel">
        <div className="page-scroll">
        <div className="card friends-card friends-onboarding-card">
          <h3>Set Your Display Name</h3>
          <p className="muted">
            Choose a display name so friends can recognize you. You can change
            this later in Settings &rarr; Social.
          </p>
          <form className="friends-inline-form" onSubmit={handleSaveDisplayName}>
            <input
              type="text"
              value={displayNameInput}
              onChange={(e) => setDisplayNameInput(e.target.value)}
              placeholder="Your display name"
              aria-label="Your display name"
              autoFocus
            />
            <button type="submit" disabled={!displayNameInput.trim()}>
              Save Name
            </button>
          </form>
        </div>
        </div>
      </section>
    );
  }

  if (selectedFriend && selectedCol) {
    const matchedManaged = decodedPreview
      ? (managed.find((g) => g.id === targetGameId) ??
        managed.find((g) => gameMatchesHint(g, decodedPreview.game)) ??
        null)
      : null;
    const coverSrc =
      matchedManaged && resolveGameCover ? resolveGameCover(matchedManaged) : null;
    const gameTitle =
      matchedManaged?.title ??
      decodedPreview?.game.title ??
      "Shared Collection";
    const activeMatch =
      selectedCol.kind === "active_loadout"
        ? selectedCol.matched_local_collection ?? null
        : null;
    const isInstalled = Boolean(
      linkedInstalled ||
        activeMatch?.kind === "matches_installed" ||
        activeMatch?.kind === "matches_active_loadout" ||
        (selectedCol.share_code &&
          installedShareIds?.has(selectedCol.share_code.trim())),
    );
    const syncLabel = syncing
      ? "Syncing..."
      : hasUpdate
        ? "Sync Update"
        : linkedInstalled
          ? "Re-sync"
          : "Install & Join Synced";

    return (
      <section className="panel page-panel friends-panel">
        <CollectionDetailView
          title={selectedCol.name}
          backLabel="Back"
          onBack={() => {
            setSelectedCol(null);
            if (openedColFromFeed) {
              setSelectedPeerId(null);
              setOpenedColFromFeed(false);
            }
          }}
          headerPills={
            <span className="pill">
              {selectedCol.kind === "active_loadout"
                ? "Active Loadout"
                : "Saved Collection"}
            </span>
          }
          headerActions={
            <span
              className={`pill ${selectedFriend.online ? "ok" : ""}`}
              title={`Shared by ${selectedFriend.display_name}`}
            >
              {selectedFriend.online
                ? `${selectedFriend.display_name} · ${formatOnlineStatusLabel(selectedFriend)}`
                : `${selectedFriend.display_name} · Offline (Cached)`}
            </span>
          }
          coverSrc={coverSrc}
          gameTitle={gameTitle}
          metaPills={
            <>
              {isInstalled ? (
                <>
                  {" "}
                  <span className="pill ok">Installed</span>
                </>
              ) : null}
              {linkedInstalled ? (
                <>
                  {" "}
                  <span className={`pill ${hasUpdate ? "warn" : "ok"}`}>
                    {hasUpdate ? "Update available" : "Synced"}
                  </span>
                </>
              ) : null}
              {activeMatch ? (
                <>
                  {" "}
                  <span
                    className={`pill ${activeMatch.has_local_changes ? "warn" : "ok"}`}
                  >
                    {formatActiveLoadoutMatchLabel(activeMatch)}
                  </span>
                </>
              ) : null}
            </>
          }
          stats={{
            mod_count: decodedPreview?.mods.length ?? 0,
            nexus_count: decodedPreview?.nexus_count ?? 0,
            thunderstore_count: decodedPreview?.thunderstore_count ?? 0,
            modio_count: decodedPreview?.modio_count ?? 0,
            updated_at: selectedCol.updated_at,
          }}
          managed={managed}
          selectedGameId={targetGameId}
          onSelectGameId={setTargetGameId}
          gameMatches={(g) =>
            decodedPreview ? gameMatchesHint(g, decodedPreview.game) : false
          }
          gameKnown={Boolean(decodedPreview && hasKnownGameHint(decodedPreview.game))}
          actions={
            <>
              {onInstallCollection ? (
                <button
                  type="button"
                  disabled={installing || syncing || !targetGameId}
                  onClick={() => void handleInstall()}
                >
                  {installing ? "Installing..." : "Install"}
                </button>
              ) : null}
              <button
                type="button"
                disabled={syncing || installing || !targetGameId}
                onClick={() => void handleJoinOrSync()}
              >
                {syncLabel}
              </button>
              {onSaveCollection ? (
                <button
                  type="button"
                  disabled={saving}
                  onClick={() => void handleSave()}
                >
                  {saving ? "Saving..." : "Save"}
                </button>
              ) : null}
            </>
          }
          noGamesFallback={
            <>
              <span className="muted">
                Manage this game in Library to install or join &amp; sync.
              </span>
              {onSaveCollection ? (
                <button
                  type="button"
                  disabled={saving}
                  onClick={() => void handleSave()}
                >
                  {saving ? "Saving..." : "Save"}
                </button>
              ) : null}
            </>
          }
          metaExtra={
            linkedInstalled ? (
              <label className="friends-autosync-toggle">
                <input
                  type="checkbox"
                  checked={Boolean(linkedInstalled.friend_sync?.auto_sync)}
                  onChange={(e) => void handleToggleAutoSync(e.target.checked)}
                />
                Auto-sync when {selectedFriend.display_name} updates this
                collection
              </label>
            ) : null
          }
          code={selectedCol.share_code}
          mods={decodedPreview?.mods ?? []}
          showMods={!decoding && Boolean(decodedPreview)}
          scrollRef={scrollRef}
          active={active}
          onOpenMod={onOpenMod ? (hit) => onOpenMod(hit, targetGameId || null) : undefined}
          loadingBody={
            !decoding && !decodedPreview ? (
              <div className="card friends-card">
                <p className="muted">
                  Could not decode share code for this collection.
                </p>
              </div>
            ) : null
          }
        />
      </section>
    );
  }

  return (
    <section className="panel page-panel friends-panel">
      <div className="page-chrome">
      <div className="panel-head">
        <div className="detail-head-title">
          {selectedFriend ? (
            <>
              <button
                type="button"
                onClick={() => {
                  setSelectedPeerId(null);
                }}
              >
                Back
              </button>
              <h1>Friends Collections</h1>
            </>
          ) : (
            <h1>
              <FontAwesomeIcon icon={faUsers} /> Friends
            </h1>
          )}
        </div>
        <div
          className={
            selectedFriend
              ? "panel-head-actions friends-library-head-actions"
              : "panel-head-actions"
          }
        >
          {!selectedFriend && (
            <>
              <button
                type="button"
                disabled={!state?.join_code}
                onClick={() => void handleCopyJoinCode()}
              >
                <FontAwesomeIcon icon={copiedCode ? faCheck : faShareNodes} />{" "}
                {copiedCode ? "Code Copied" : "Share Friend Code"}
              </button>
              <button
                type="button"
                onClick={() => {
                  onError(null);
                  setAddFriendOpen(true);
                }}
              >
                <FontAwesomeIcon icon={faUserPlus} /> Add Friend
              </button>
            </>
          )}
          <button
            type="button"
            className="friends-refresh-btn"
            aria-label="Refresh"
            title="Refresh"
            onClick={() => void handleRefreshAll()}
            disabled={refreshing || autoChecking}
          >
            <FontAwesomeIcon icon={faRotate} spin={refreshing || autoChecking} />
          </button>
          {selectedFriend && (
            <>
              <h1 className="friends-library-name">
                {selectedFriend.display_name}
              </h1>
              <FriendAvatar
                name={selectedFriend.display_name}
                src={
                  selectedFriend.profile_picture ??
                  selectedFriend.cached_profile?.profile_picture
                }
                online={selectedFriend.online}
                size="md"
                showStatus
              />
            </>
          )}
        </div>
      </div>
      {!selectedFriend && (
        <div className="friends-tabs-row">
          <Segment>
            <SegmentTab
              layoutId="friends-page-tabs"
              active={friendsTab === "feed"}
              onClick={() => setFriendsTab("feed")}
            >
              <FontAwesomeIcon icon={faRss} /> Friend Feed
              {unreadFeedCount > 0 ? (
                <span
                  className="pill friends-feed-badge"
                  aria-label={`${unreadFeedCount} unread`}
                >
                  {unreadFeedCount}
                </span>
              ) : null}
            </SegmentTab>
            <SegmentTab
              layoutId="friends-page-tabs"
              active={friendsTab === "friends"}
              onClick={() => setFriendsTab("friends")}
            >
              <FontAwesomeIcon icon={faUsers} /> Friends
              {(state?.pending_requests.length ?? 0) > 0 ? (
                <span
                  className="pill friends-feed-badge"
                  aria-label={`${state!.pending_requests.length} pending`}
                >
                  {state!.pending_requests.length}
                </span>
              ) : null}
            </SegmentTab>
          </Segment>
        </div>
      )}
      {selectedFriend && !selectedCol && (
        <>
          {(() => {
            const synced = getCollectionsSyncedByFriend(
              selectedFriend,
              state?.local_collections,
            );
            const activeMatches = getFriendActiveLoadoutMatches(selectedFriend);
            if (synced.length === 0 && activeMatches.length === 0) return null;
            return (
              <div className="friends-detail-syncing-banner">
                {synced.length > 0 && (
                  <span className="pill ok">
                    Syncing your{" "}
                    {synced.length === 1 ? "collection" : "collections"}:{" "}
                    {synced.map((c) => c.name).join(", ")}
                  </span>
                )}
                {activeMatches.map(({ col, match }) => (
                  <span
                    key={col.id}
                    className={`pill ${match.has_local_changes ? "warn" : "ok"}`}
                  >
                    Active loadout: {formatActiveLoadoutMatchLabel(match)}
                  </span>
                ))}
              </div>
            );
          })()}
          <div className="friends-tabs-row">
          <Segment>
            <SegmentTab
              layoutId="friend-detail-tabs"
              active={friendDetailTab === "feed"}
              onClick={() => setFriendDetailTab("feed")}
            >
              <FontAwesomeIcon icon={faRss} /> Feed
            </SegmentTab>
            <SegmentTab
              layoutId="friend-detail-tabs"
              active={friendDetailTab === "collections"}
              onClick={() => setFriendDetailTab("collections")}
            >
              Collections
            </SegmentTab>
            <SegmentTab
              layoutId="friend-detail-tabs"
              active={friendDetailTab === "friends"}
              onClick={() => setFriendDetailTab("friends")}
            >
              <FontAwesomeIcon icon={faUsers} /> Friends
              {Array.isArray(selectedFriend.cached_profile?.friends) ? (
                <span className="pill friends-feed-badge">
                  {
                    selectedFriend.cached_profile.friends.filter(
                      (sf) => sf.peer_id !== state?.peer_id,
                    ).length
                  }
                </span>
              ) : null}
            </SegmentTab>
          </Segment>
        </div>
        </>
      )}
      </div>

      <div className="page-scroll">
      {!selectedFriend && friendsTab === "friends" && (
        <>
          {state && state.pending_requests.length > 0 && (
            <div className="card friends-card">
              <h3>Pending Friend Requests</h3>
              <div className="friends-list">
                {state.pending_requests.map((req) => {
                  const reqMutuals = getMutualFriendsForPeerId(state, req.peer_id);
                  return (
                  <div key={req.peer_id} className="friends-row">
                    <div className="friends-row-identity">
                      <FriendAvatar
                        name={req.display_name}
                        src={req.profile_picture}
                        size="sm"
                      />
                      <div>
                        <strong>{req.display_name}</strong>
                        {reqMutuals.length > 0 && (
                          <div className="muted friends-mutual-label">
                            {formatMutualSummary(reqMutuals)}
                          </div>
                        )}
                        <div className="muted friends-peer-id">{req.peer_id}</div>
                      </div>
                    </div>
                    <div className="friends-row-actions">
                      <button
                        type="button"
                        onClick={() => void handleRespondRequest(req.peer_id, true)}
                      >
                        <FontAwesomeIcon icon={faCheck} /> Accept
                      </button>
                      <button
                        type="button"
                        className="danger"
                        onClick={() => void handleRespondRequest(req.peer_id, false)}
                      >
                        <FontAwesomeIcon icon={faXmark} /> Decline
                      </button>
                    </div>
                  </div>
                  );
                })}
              </div>
            </div>
          )}

          {state && (state.queued_invites?.length ?? 0) > 0 && (
            <div className="card friends-card">
              <h3>Queued Outgoing Requests</h3>
              <div className="friends-list">
                {state.queued_invites!.map((q) => {
                  const qMutuals = getMutualFriendsForPeerId(state, q.peer_id);
                  const relayNames = (q.relayed_via_peer_ids ?? [])
                    .map(
                      (mid) =>
                        state.friends.find((f) => f.peer_id === mid)
                          ?.display_name,
                    )
                    .filter((n): n is string => Boolean(n));
                  return (
                    <div key={q.peer_id} className="friends-row">
                      <div className="friends-row-identity">
                        <FriendAvatar name={q.display_name} size="sm" />
                        <div>
                          <strong>
                            {q.display_name}{" "}
                            <span className="pill warn">Queued</span>
                          </strong>
                          <div className="muted friends-mutual-label">
                            Waiting for them to come online
                            {relayNames.length > 0
                              ? ` · Relayed via ${relayNames.join(", ")}`
                              : ""}
                          </div>
                          {qMutuals.length > 0 && (
                            <div className="muted friends-mutual-label">
                              {formatMutualSummary(qMutuals)}
                            </div>
                          )}
                        </div>
                      </div>
                      <div className="friends-row-actions">
                        <button
                          type="button"
                          className="danger"
                          onClick={() =>
                            void handleCancelQueuedInvite(q.peer_id)
                          }
                        >
                          <FontAwesomeIcon icon={faXmark} /> Cancel
                        </button>
                      </div>
                    </div>
                  );
                })}
              </div>
            </div>
          )}

          <div className="card friends-card">
            <h3>Connected Friends</h3>
            {!state || state.friends.length === 0 ? (
              <p className="muted">
                No friends connected yet. Use Share Friend Code or Add Friend
                above to connect.
              </p>
            ) : (
              <div className="friends-cards-grid">
                {state.friends.map((f) => {
                  const colCount = f.cached_profile?.collections.length ?? 0;
                  const mutualCount = getMutualFriends(state, f).length;
                  const avatarSrc =
                    f.profile_picture ?? f.cached_profile?.profile_picture;
                  const mutualSuffix =
                    mutualCount > 0 ? ` · ${mutualCount} mutual` : "";
                  const syncingMine = getCollectionsSyncedByFriend(
                    f,
                    state.local_collections,
                  );
                  const activeMatches = getFriendActiveLoadoutMatches(f);
                  const playingGame = getFriendPlayingGame(f);
                  return (
                    <div key={f.peer_id} className="friends-friend-card">
                      <button
                        type="button"
                        className="friends-friend-card-main"
                        onClick={() => {
                          setSelectedPeerId(f.peer_id);
                          setSelectedCol(null);
                          setExpandedFriendGameKey(null);
                        }}
                      >
                        <FriendAvatar
                          name={f.display_name}
                          src={avatarSrc}
                          online={f.online}
                          size="lg"
                          showStatus={f.status !== "pending_outgoing"}
                        />
                        <div className="friends-friend-card-info">
                          <strong className="friends-friend-card-name">
                            {f.display_name}
                          </strong>
                          <div className="muted friends-friend-card-meta">
                            {f.status === "pending_outgoing"
                              ? "Waiting for friend to accept"
                              : f.online
                                ? `Online · ${colCount} collection(s)${mutualSuffix}`
                                : `Offline · ${colCount} collection(s)${mutualSuffix}`}
                          </div>
                          {f.status !== "pending_outgoing" && !f.online && (
                            <div className="muted friends-friend-card-seen">
                              Last seen {formatLastSeen(f.last_seen_at)}
                            </div>
                          )}
                          {playingGame && (
                            <div
                              className="friends-friend-card-syncing"
                              title={formatPlayingGameLabel(playingGame)}
                            >
                              {formatPlayingGameLabel(playingGame)}
                            </div>
                          )}
                          {syncingMine.length > 0 && (
                            <div
                              className="friends-friend-card-syncing"
                              title={`Syncing your ${syncingMine.map((c) => c.name).join(", ")}`}
                            >
                              Syncing your:{" "}
                              {syncingMine.map((c) => c.name).join(", ")}
                            </div>
                          )}
                          {activeMatches.length > 0 && (
                            <div
                              className="friends-friend-card-syncing"
                              title={activeMatches
                                .map(({ match }) =>
                                  formatActiveLoadoutMatchLabel(match),
                                )
                                .join(" · ")}
                            >
                              Active loadout:{" "}
                              {activeMatches
                                .map(({ match }) =>
                                  match.kind === "matches_active_loadout"
                                    ? "Your active loadout"
                                    : match.name,
                                )
                                .join(", ")}
                            </div>
                          )}
                        </div>
                      </button>
                      <button
                        type="button"
                        className="danger friends-friend-card-remove"
                        aria-label={`Remove ${f.display_name}`}
                        title={`Remove ${f.display_name}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          void handleRemoveFriend(f.peer_id);
                        }}
                      >
                        <FontAwesomeIcon icon={faTrash} />
                      </button>
                    </div>
                  );
                })}
              </div>
            )}
          </div>
        </>
      )}

      {!selectedFriend && friendsTab === "feed" && (
        <div className="card friends-card friends-feed-panel">
          <div className="friends-feed-toolbar">
            <div>
              <h3>Friend Feed</h3>
              <p className="muted friends-feed-subtitle">
                Recent collection updates and new shares from you and your
                friends.
              </p>
            </div>
            {feedEvents.length > 0 && (
              <div className="friends-feed-controls">
                {feedFilterFriends.length > 1 && (
                  <select
                    aria-label="Filter feed by friend"
                    value={feedFriendFilter}
                    onChange={(e) => setFeedFriendFilter(e.target.value)}
                  >
                    <option value="all">Everyone</option>
                    {feedFilterFriends.map((f) => (
                      <option key={f.peer_id} value={f.peer_id}>
                        {f.display_name}
                      </option>
                    ))}
                  </select>
                )}
                <button
                  type="button"
                  disabled={clearingFeed}
                  onClick={() => void handleClearFeed()}
                >
                  {clearingFeed ? "Clearing..." : "Clear Feed"}
                </button>
              </div>
            )}
          </div>

          {filteredFeedEvents.length > 0 ? (
            renderFeedList(filteredFeedEvents, true)
          ) : !state || state.friends.length === 0 ? (
            <p className="muted">
              No friends connected yet. Add friends to see their collection
              updates here, or update your shared collections to see your own
              activity.
            </p>
          ) : (
            <p className="muted">
              No recent activity yet. New collections, updates, and renames from
              your shared collections and friends will appear here automatically.
            </p>
          )}
        </div>
      )}

      {selectedFriend && !selectedCol && friendDetailTab === "feed" && (
        <div className="card friends-card friends-feed-panel">
          <div className="friends-feed-toolbar">
            <div>
              <h3>{selectedFriend.display_name}&apos;s Feed</h3>
              <p className="muted friends-feed-subtitle">
                Recent collection updates and new shares from{" "}
                {selectedFriend.display_name}.{" "}
                <span className={`pill ${selectedFriend.online ? "ok" : ""}`}>
                  {selectedFriend.online
                    ? formatOnlineStatusLabel(selectedFriend)
                    : selectedFriend.cached_via_peer_name
                      ? `Offline · Cached via ${selectedFriend.cached_via_peer_name} (${formatLastSeen(selectedFriend.last_seen_at)})`
                      : `Offline · Cached (${formatLastSeen(selectedFriend.last_seen_at)})`}
                </span>
              </p>
            </div>
          </div>

          {selectedFriendFeedEvents.length === 0 ? (
            <p className="muted">
              No recent activity from {selectedFriend.display_name} yet. New
              collections, updates, and renames will appear here automatically.
            </p>
          ) : (
            renderFeedList(selectedFriendFeedEvents, false)
          )}
        </div>
      )}

      {selectedFriend && !selectedCol && friendDetailTab === "friends" && (
        <div className="card friends-card">
          <h3>{selectedFriend.display_name}&apos;s Friends</h3>
          {!selectedFriend.cached_profile ? (
            <p className="muted">
              Waiting to sync {selectedFriend.display_name}&apos;s profile.
            </p>
          ) : selectedFriend.cached_profile.friends == null ? (
            <p className="muted friends-private-notice">
              <FontAwesomeIcon icon={faLock} />{" "}
              {selectedFriend.display_name}&apos;s friends list is private.
            </p>
          ) : (() => {
              const visibleFriends = selectedFriend.cached_profile.friends.filter(
                (sf) => sf.peer_id !== state?.peer_id,
              );
              const mutualFriends = getMutualFriends(state, selectedFriend);
              const mutualIds = new Set(mutualFriends.map((m) => m.peer_id));
              if (visibleFriends.length === 0) {
                return (
                  <p className="muted">
                    {selectedFriend.display_name} has no other connected friends
                    yet.
                  </p>
                );
              }
              return (
                <>
                  <p className="muted friends-feed-subtitle">
                    {visibleFriends.length}{" "}
                    {visibleFriends.length === 1 ? "friend" : "friends"}
                    {mutualFriends.length > 0
                      ? ` · ${mutualFriends.length} mutual`
                      : ""}
                  </p>
                  <div className="friends-list">
                    {visibleFriends.map((sf) => {
                      const localFriend = state?.friends.find(
                        (f) => f.peer_id === sf.peer_id,
                      );
                      const isMutual = mutualIds.has(sf.peer_id);
                      const isQueuedOut = Boolean(
                        state?.queued_invites?.some(
                          (q) => q.peer_id === sf.peer_id,
                        ),
                      );
                      const isPendingOut =
                        localFriend?.status === "pending_outgoing";
                      const isPendingIn = Boolean(
                        state?.pending_requests.some(
                          (r) => r.peer_id === sf.peer_id,
                        ),
                      );
                      const busy = addingPeerId === sf.peer_id;
                      const avatarSrc =
                        localFriend?.profile_picture ??
                        localFriend?.cached_profile?.profile_picture;
                      const showOnlineDot =
                        isMutual || typeof sf.online === "boolean";
                      const isOnline =
                        localFriend?.online ?? sf.online ?? false;
                      return (
                        <div key={sf.peer_id} className="friends-row">
                          <div className="friends-row-identity">
                            <FriendAvatar
                              name={localFriend?.display_name ?? sf.display_name}
                              src={avatarSrc}
                              online={isOnline}
                              showStatus={showOnlineDot}
                              size="sm"
                            />
                            <div>
                              <strong>
                                {localFriend?.display_name ?? sf.display_name}
                                {isMutual && (
                                  <>
                                    {" "}
                                    <span className="pill ok">Mutual</span>
                                  </>
                                )}
                              </strong>
                            </div>
                          </div>
                          <div className="friends-row-actions">
                            {isMutual && localFriend ? (
                              <button
                                type="button"
                                className="linkish"
                                onClick={() => {
                                  setSelectedPeerId(localFriend.peer_id);
                                  setSelectedCol(null);
                                  setExpandedFriendGameKey(null);
                                }}
                              >
                                View Profile
                              </button>
                            ) : isQueuedOut ? (
                              <span className="pill warn">Queued</span>
                            ) : isPendingOut ? (
                              <span className="pill">Request Sent</span>
                            ) : isPendingIn ? (
                              <button
                                type="button"
                                disabled={busy}
                                onClick={() =>
                                  void handleRespondRequest(sf.peer_id, true)
                                }
                              >
                                <FontAwesomeIcon icon={faCheck} /> Accept
                                Request
                              </button>
                            ) : (
                              <button
                                type="button"
                                disabled={busy}
                                onClick={() =>
                                  void handleAddFriendByPeer(
                                    sf.peer_id,
                                    sf.display_name,
                                  )
                                }
                              >
                                <FontAwesomeIcon icon={faUserPlus} />{" "}
                                {busy ? "Sending..." : "Add Friend"}
                              </button>
                            )}
                          </div>
                        </div>
                      );
                    })}
                  </div>
                </>
              );
            })()}
        </div>
      )}

      {selectedFriend && !selectedCol && friendDetailTab === "collections" && (
        <div>
          <p className="note">
            Shared collections and active loadouts from {selectedFriend.display_name},
            grouped by game.{" "}
            <span className={`pill ${selectedFriend.online ? "ok" : ""}`}>
              {selectedFriend.online
                ? formatOnlineStatusLabel(selectedFriend)
                : selectedFriend.cached_via_peer_name
                  ? `Offline · Cached via ${selectedFriend.cached_via_peer_name} (${formatLastSeen(selectedFriend.last_seen_at)})`
                  : `Offline · Cached (${formatLastSeen(selectedFriend.last_seen_at)})`}
            </span>
          </p>
          {decodingLibrary && friendGameGroups.length === 0 ? (
            <BrowseListSkeleton count={4} posters />
          ) : (
            <CollectionsGameList
              groups={friendGameGroups}
              expandedGameKey={expandedFriendGameKey}
              onToggleGameKey={(key) =>
                setExpandedFriendGameKey(
                  expandedFriendGameKey === key ? null : key,
                )
              }
              renderPoster={(coverPath) =>
                renderPoster ? (
                  renderPoster(coverPath)
                ) : (
                  <div className="list-poster" aria-hidden="true">
                    {coverPath ? (
                      <img src={coverPath} alt="" />
                    ) : (
                      <CollectionPlaceholderIcon />
                    )}
                  </div>
                )
              }
              emptyMessage={
                !selectedFriend.online
                  ? `No shared collections cached for ${selectedFriend.display_name} yet. They will appear once your friend comes online.`
                  : `No shared collections cached for ${selectedFriend.display_name} yet.`
              }
              renderChild={(group, entry, childIdx) => {
                const col = friendColById.get(entry.id);
                if (!col) return null;
                const installedForGroup = group.managed
                  ? installedByGame[group.managed.id] ?? []
                  : [];
                const linked =
                  installedForGroup.find(
                    (ic) =>
                      ic.friend_sync?.friend_peer_id === selectedFriend.peer_id &&
                      ic.friend_sync?.friend_collection_id === col.id,
                  ) ?? null;
                const rowHasUpdate = Boolean(
                  linked &&
                    ((group.managed
                      ? syncStatesByGame[group.managed.id]?.[linked.id]
                      : undefined) ??
                      linked.friend_sync?.last_synced_hash !== col.share_hash),
                );
                const rowActiveMatch =
                  col.kind === "active_loadout"
                    ? col.matched_local_collection ?? null
                    : null;
                const installed = Boolean(
                  linked ||
                    rowActiveMatch?.kind === "matches_installed" ||
                    rowActiveMatch?.kind === "matches_active_loadout" ||
                    installedShareIds?.has(col.share_code.trim()),
                );
                const updatedDate = formatCollectionDate(col.updated_at);
                const busyHere = rowBusyKey === col.id;
                const syncButtonLabel = busyHere
                  ? "Syncing..."
                  : rowHasUpdate
                    ? "Sync Update"
                    : linked
                      ? "Re-sync"
                      : "Install & Join Synced";

                return (
                  <li
                    key={col.id}
                    className="list-row-clickable stagger-item"
                    style={staggerStyle(childIdx)}
                    onClick={() => {
                      setOpenedColFromFeed(false);
                      setSelectedCol(col);
                    }}
                  >
                    <div className="list-row-main">
                      <strong>
                        {col.name}
                        <span className="pill">
                          {col.kind === "active_loadout"
                            ? "Active Loadout"
                            : "Saved Collection"}
                        </span>
                        {installed ? (
                          <span className="pill ok">Installed</span>
                        ) : null}
                        {linked ? (
                          <span
                            className={`pill ${rowHasUpdate ? "warn" : "ok"}`}
                          >
                            {rowHasUpdate ? "Update available" : "Synced"}
                          </span>
                        ) : null}
                        {rowActiveMatch ? (
                          <span
                            className={`pill ${rowActiveMatch.has_local_changes ? "warn" : "ok"}`}
                          >
                            {formatActiveLoadoutMatchLabel(rowActiveMatch)}
                          </span>
                        ) : null}
                      </strong>
                      <small>
                        {entry.mod_count} mods
                        {updatedDate ? ` · Updated ${updatedDate}` : ""}
                      </small>
                    </div>
                    <div
                      className="actions"
                      onClick={(e) => e.stopPropagation()}
                    >
                      {onInstallCollection ? (
                        <button
                          type="button"
                          disabled={busyHere}
                          onClick={() =>
                            void (async () => {
                              if (!group.managed) {
                                setSelectedCol(col);
                                return;
                              }
                              onError(null);
                              setRowBusyKey(col.id);
                              try {
                                await onInstallCollection(
                                  group.managed.id,
                                  col.share_code,
                                  col.name,
                                );
                                const updated =
                                  await api.listInstalledCollections(
                                    group.managed.id,
                                  );
                                setInstalledByGame((prev) => ({
                                  ...prev,
                                  [group.managed!.id]: updated,
                                }));
                              } catch (err) {
                                onError(String(err));
                              } finally {
                                setRowBusyKey(null);
                              }
                            })()
                          }
                        >
                          Install
                        </button>
                      ) : null}
                      <button
                        type="button"
                        disabled={busyHere}
                        onClick={() =>
                          void (async () => {
                            if (!group.managed) {
                              setSelectedCol(col);
                              return;
                            }
                            onError(null);
                            setRowBusyKey(col.id);
                            try {
                              let resolutions: CollectionResolutionChoices | null =
                                null;
                              if (resolveCollectionConflicts) {
                                resolutions = await resolveCollectionConflicts(
                                  group.managed.id,
                                  {
                                    kind: "friend_sync",
                                    peerId: selectedFriend.peer_id,
                                    friendCollectionId: col.id,
                                  },
                                );
                                if (!resolutions) return;
                              }
                              const res = await api.joinOrSyncFriendCollection({
                                gameId: group.managed.id,
                                peerId: selectedFriend.peer_id,
                                friendCollectionId: col.id,
                                ...(resolutions ? { resolutions } : {}),
                              });
                              await onCollectionSynced(group.managed.id, res, {
                                peerId: selectedFriend.peer_id,
                                friendCollectionId: col.id,
                              });
                              const [updated, syncStates] = await Promise.all([
                                api.listInstalledCollections(group.managed.id),
                                api.getFriendCollectionSyncStates
                                  ? api
                                      .getFriendCollectionSyncStates(
                                        group.managed.id,
                                      )
                                      .catch(() => [])
                                  : Promise.resolve([]),
                              ]);
                              setInstalledByGame((prev) => ({
                                ...prev,
                                [group.managed!.id]: updated,
                              }));
                              const syncMap: Record<string, boolean> = {};
                              for (const st of syncStates ?? []) {
                                syncMap[st.collection_id] = st.has_update;
                              }
                              setSyncStatesByGame((prev) => ({
                                ...prev,
                                [group.managed!.id]: syncMap,
                              }));
                            } catch (err) {
                              onError(String(err));
                            } finally {
                              setRowBusyKey(null);
                            }
                          })()
                        }
                      >
                        {syncButtonLabel}
                      </button>
                      <button
                        type="button"
                        className="icon-btn"
                        aria-label={`Copy ${col.name}`}
                        title="Copy"
                        onClick={() =>
                          void navigator.clipboard?.writeText(col.share_code)
                        }
                      >
                        <FontAwesomeIcon icon={faCopy} />
                      </button>
                    </div>
                  </li>
                );
              }}
            />
          )}
        </div>
      )}
      </div>
      {addFriendOpen && (
        <Modal
          title="Add a Friend"
          subtitle="Paste a friend's invite link or join code (emperor-friend1:...) to send a mutual connection request."
          onClose={() => setAddFriendOpen(false)}
          footer={
            <div className="modal-foot-actions">
              <button
                type="button"
                onClick={() => setAddFriendOpen(false)}
                disabled={addingFriend}
              >
                Cancel
              </button>
              <button
                type="submit"
                form="add-friend-modal-form"
                disabled={!joinCodeInput.trim() || addingFriend}
              >
                <FontAwesomeIcon icon={faUserPlus} />{" "}
                {addingFriend ? "Connecting..." : "Connect"}
              </button>
            </div>
          }
        >
          <form id="add-friend-modal-form" onSubmit={handleAddFriend}>
            <input
              type="text"
              value={joinCodeInput}
              onChange={(e) => setJoinCodeInput(e.target.value)}
              placeholder="https://emperor-of-modkind.github.io/open/?to=friend/... or emperor-friend1:..."
              aria-label="Friend join code"
              autoFocus
            />
          </form>
        </Modal>
      )}
    </section>
  );
}
