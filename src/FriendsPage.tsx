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
  faCopy,
  faRotate,
  faShareNodes,
  faTrash,
  faUserPlus,
  faUsers,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import { api } from "./api";
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
import { FriendAvatar } from "./FriendAvatar";
import { Modal } from "./Modal";
import { staggerStyle } from "./motion";
import { groupSavedCollectionsByGame } from "./savedCollectionGroups";
import { BrowseListSkeleton } from "./Skeleton";
import type {
  CatalogHit,
  DetectedGame,
  FriendRecord,
  FriendShareRef,
  FriendsState,
  FriendSyncResult,
  GamePluginInfo,
  InstalledCollection,
  ManagedGame,
  SavedCollectionEntry,
  ShareDecodePreview,
} from "./types";

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
  onOpenMod?: (hit: CatalogHit, targetGameId?: string | null) => void;
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
  onOpenMod,
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
  const [rowBusyKey, setRowBusyKey] = useState<string | null>(null);
  const [targetGameId, setTargetGameId] = useState<string>("");
  const [installedForTarget, setInstalledForTarget] = useState<
    InstalledCollection[]
  >([]);
  const [syncing, setSyncing] = useState(false);
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
      return;
    }
    const managedIds = managedGroupIdsKey.split(",");
    let cancelled = false;
    void Promise.allSettled(
      managedIds.map(async (gid) => ({
        gid,
        cols: await api.listInstalledCollections(gid),
      })),
    ).then((results) => {
      if (cancelled) return;
      const next: Record<string, InstalledCollection[]> = {};
      for (const r of results) {
        if (r.status === "fulfilled") {
          next[r.value.gid] = r.value.cols;
        }
      }
      setInstalledByGame(next);
    });
    return () => {
      cancelled = true;
    };
  }, [selectedFriend, managedGroupIdsKey, state]);

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
      return;
    }
    let cancelled = false;
    void api
      .listInstalledCollections(targetGameId)
      .then((cols) => {
        if (!cancelled) setInstalledForTarget(cols);
      })
      .catch(() => {
        if (!cancelled) setInstalledForTarget([]);
      });
    return () => {
      cancelled = true;
    };
  }, [targetGameId, state]);

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
      linkedInstalled.friend_sync?.last_synced_hash !== selectedCol.share_hash,
  );

  async function handleCopyJoinCode() {
    if (!state?.join_code) return;
    try {
      await navigator.clipboard.writeText(state.join_code);
      setCopiedCode(true);
      onNotice("Friend code copied to clipboard.");
      setTimeout(() => setCopiedCode(false), 2000);
    } catch (e) {
      onError(String(e));
    }
  }

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
    if (!joinCodeInput.trim()) return;
    setAddingFriend(true);
    try {
      const next = await api.addFriendByCode(joinCodeInput.trim());
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
    } catch (err) {
      onError(String(err));
    } finally {
      setRefreshing(false);
    }
  }

  async function handleJoinOrSync() {
    if (!selectedFriend || !selectedCol || !targetGameId) return;
    onError(null);
    setSyncing(true);
    try {
      const res = await api.joinOrSyncFriendCollection({
        gameId: targetGameId,
        peerId: selectedFriend.peer_id,
        friendCollectionId: selectedCol.id,
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
    const isInstalled = Boolean(
      linkedInstalled ||
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
          onBack={() => setSelectedCol(null)}
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
                ? `${selectedFriend.display_name} · Online`
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
      </div>

      <div className="page-scroll">
      {!selectedFriend && (
        <>
          {state && state.pending_requests.length > 0 && (
            <div className="card friends-card">
              <h3>Pending Friend Requests</h3>
              <div className="friends-list">
                {state.pending_requests.map((req) => (
                  <div key={req.peer_id} className="friends-row">
                    <div className="friends-row-identity">
                      <FriendAvatar
                        name={req.display_name}
                        src={req.profile_picture}
                        size="sm"
                      />
                      <div>
                        <strong>{req.display_name}</strong>
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
                ))}
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
                  const avatarSrc =
                    f.profile_picture ?? f.cached_profile?.profile_picture;
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
                                ? `Online · ${colCount} collection(s)`
                                : `Offline · ${colCount} collection(s)`}
                          </div>
                          {f.status !== "pending_outgoing" && !f.online && (
                            <div className="muted friends-friend-card-seen">
                              Last seen {formatLastSeen(f.last_seen_at)}
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

      {selectedFriend && !selectedCol && (
        <div>
          <p className="note">
            Shared collections and active loadouts from {selectedFriend.display_name},
            grouped by game.{" "}
            <span className={`pill ${selectedFriend.online ? "ok" : ""}`}>
              {selectedFriend.online
                ? "Online"
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
                    linked.friend_sync?.last_synced_hash !== col.share_hash,
                );
                const installed = Boolean(
                  linked || installedShareIds?.has(col.share_code.trim()),
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
                    onClick={() => setSelectedCol(col)}
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
                              const res = await api.joinOrSyncFriendCollection({
                                gameId: group.managed.id,
                                peerId: selectedFriend.peer_id,
                                friendCollectionId: col.id,
                              });
                              await onCollectionSynced(group.managed.id, res);
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
          subtitle="Paste a friend's join code (emperor-friend1:...) to send a mutual connection request."
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
              placeholder="emperor-friend1:..."
              aria-label="Friend join code"
              autoFocus
            />
          </form>
        </Modal>
      )}
    </section>
  );
}
