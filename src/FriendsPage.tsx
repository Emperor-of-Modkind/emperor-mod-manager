import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faCheck,
  faCopy,
  faEye,
  faEyeSlash,
  faRotate,
  faTrash,
  faUserPlus,
  faUsers,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import { api } from "./api";
import type {
  FriendRecord,
  FriendShareRef,
  FriendsState,
  FriendSyncResult,
  InstalledCollection,
  ManagedGame,
  ShareDecodePreview,
} from "./types";

export type FriendsPageProps = {
  active: boolean;
  managed: ManagedGame[];
  activeGameId: string | null;
  onError: (msg: string | null) => void;
  onNotice: (msg: string | null) => void;
  onCollectionSynced: (
    gameId: string,
    result: FriendSyncResult,
  ) => Promise<void> | void;
};

function gameMatchesHint(game: ManagedGame, preview: ShareDecodePreview): boolean {
  const h = preview.game;
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

function formatLastSeen(iso?: string | null): string {
  if (!iso) return "Never";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString();
}

export function FriendsPage({
  active,
  managed,
  activeGameId,
  onError,
  onNotice,
  onCollectionSynced,
}: FriendsPageProps) {
  const [state, setState] = useState<FriendsState | null>(null);
  const [loading, setLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);
  const [displayNameInput, setDisplayNameInput] = useState("");
  const [joinCodeInput, setJoinCodeInput] = useState("");
  const [copiedCode, setCopiedCode] = useState(false);
  const [showPrivacy, setShowPrivacy] = useState(false);
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null);
  const [selectedCol, setSelectedCol] = useState<FriendShareRef | null>(null);
  const [decodedPreview, setDecodedPreview] =
    useState<ShareDecodePreview | null>(null);
  const [decoding, setDecoding] = useState(false);
  const [targetGameId, setTargetGameId] = useState<string>("");
  const [installedForTarget, setInstalledForTarget] = useState<
    InstalledCollection[]
  >([]);
  const [syncing, setSyncing] = useState(false);

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
    if (active) {
      void loadState();
    }
  }, [active, loadState]);

  const selectedFriend: FriendRecord | null = useMemo(() => {
    if (!state || !selectedPeerId) return null;
    return state.friends.find((f) => f.peer_id === selectedPeerId) ?? null;
  }, [state, selectedPeerId]);

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
        const match =
          managed.find((g) => gameMatchesHint(g, preview)) ??
          managed.find((g) => g.id === activeGameId) ??
          managed[0];
        setTargetGameId(match?.id ?? "");
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
      onNotice("Display name updated.");
    } catch (err) {
      onError(String(err));
    }
  }

  async function handleAddFriend(e: React.FormEvent) {
    e.preventDefault();
    onError(null);
    if (!joinCodeInput.trim()) return;
    try {
      const next = await api.addFriendByCode(joinCodeInput.trim());
      setState(next);
      setJoinCodeInput("");
      onNotice("Friend added! Connecting via P2P...");
    } catch (err) {
      onError(String(err));
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

  async function handleToggleVisibility(collectionId: string, hidden: boolean) {
    onError(null);
    try {
      const next = await api.setCollectionVisibility(collectionId, hidden);
      setState(next);
    } catch (err) {
      onError(String(err));
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
      await onCollectionSynced(targetGameId, res);
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
    } catch (err) {
      onError(String(err));
    }
  }

  if (loading && !state) {
    return (
      <section className="panel">
        <div className="panel-head">
          <h1>Friends</h1>
        </div>
        <p className="muted">Starting P2P node...</p>
      </section>
    );
  }

  return (
    <section className="panel friends-panel">
      <div className="panel-head">
        <div className="detail-head-title">
          {selectedFriend ? (
            <button
              type="button"
              onClick={() => {
                if (selectedCol) {
                  setSelectedCol(null);
                } else {
                  setSelectedPeerId(null);
                }
              }}
            >
              Back
            </button>
          ) : null}
          <h1>
            <FontAwesomeIcon icon={faUsers} />{" "}
            {selectedCol
              ? selectedCol.name
              : selectedFriend
                ? selectedFriend.display_name
                : "Friends"}
          </h1>
          <span
            className={`pill ${state?.endpoint_ready ? "ok" : "warn"}`}
            title="Serverless iroh P2P endpoint status"
          >
            {state?.endpoint_ready ? "P2P Ready" : "Connecting..."}
          </span>
        </div>
        <div className="panel-head-actions">
          <button
            type="button"
            onClick={() => setShowPrivacy((v) => !v)}
            title="Choose which collections friends can see"
          >
            <FontAwesomeIcon icon={showPrivacy ? faEyeSlash : faEye} />{" "}
            Sharing Privacy
          </button>
          <button
            type="button"
            onClick={() => void handleRefreshAll()}
            disabled={refreshing}
          >
            <FontAwesomeIcon icon={faRotate} spin={refreshing} /> Refresh
          </button>
        </div>
      </div>

      {!selectedFriend && (
        <>
          <div className="friends-top-grid">
            <div className="card friends-card">
              <h3>Your Join Code</h3>
              <p className="muted">
                Share this code with friends to connect directly peer-to-peer.
              </p>
              <form className="friends-inline-form" onSubmit={handleSaveDisplayName}>
                <input
                  type="text"
                  value={displayNameInput}
                  onChange={(e) => setDisplayNameInput(e.target.value)}
                  placeholder="Your display name"
                  aria-label="Your display name"
                />
                <button type="submit">Save Name</button>
              </form>
              <div className="friends-code-row">
                <code className="friends-join-code">
                  {state?.join_code ?? "Generating P2P join code..."}
                </code>
                <button
                  type="button"
                  disabled={!state?.join_code}
                  onClick={() => void handleCopyJoinCode()}
                >
                  <FontAwesomeIcon icon={copiedCode ? faCheck : faCopy} />{" "}
                  {copiedCode ? "Copied" : "Copy"}
                </button>
              </div>
            </div>

            <div className="card friends-card">
              <h3>Add a Friend</h3>
              <p className="muted">
                Paste a friend&apos;s join code (<code>emperor-friend1:...</code>) to
                send a mutual connection request.
              </p>
              <form className="friends-inline-form" onSubmit={handleAddFriend}>
                <input
                  type="text"
                  value={joinCodeInput}
                  onChange={(e) => setJoinCodeInput(e.target.value)}
                  placeholder="emperor-friend1:..."
                  aria-label="Friend join code"
                />
                <button type="submit" disabled={!joinCodeInput.trim()}>
                  <FontAwesomeIcon icon={faUserPlus} /> Connect
                </button>
              </form>
            </div>
          </div>

          {showPrivacy && state && (
            <div className="card friends-card">
              <h3>Shared with Friends</h3>
              <p className="muted">
                Your active loadouts and saved collections are shared as compact
                live share codes. Hide any you want to keep private.
              </p>
              {state.local_collections.length === 0 ? (
                <p className="muted">No active loadouts or saved collections yet.</p>
              ) : (
                <div className="friends-list">
                  {state.local_collections.map((c) => (
                    <div key={c.id} className="friends-row">
                      <div>
                        <strong>{c.name}</strong>{" "}
                        <span className="pill">{c.game_title}</span>{" "}
                        <span className="muted">{c.mod_count} mod(s)</span>
                      </div>
                      <button
                        type="button"
                        onClick={() => void handleToggleVisibility(c.id, !c.hidden)}
                      >
                        <FontAwesomeIcon icon={c.hidden ? faEyeSlash : faEye} />{" "}
                        {c.hidden ? "Hidden" : "Visible"}
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {state && state.pending_requests.length > 0 && (
            <div className="card friends-card">
              <h3>Pending Friend Requests</h3>
              <div className="friends-list">
                {state.pending_requests.map((req) => (
                  <div key={req.peer_id} className="friends-row">
                    <div>
                      <strong>{req.display_name}</strong>
                      <div className="muted friends-peer-id">{req.peer_id}</div>
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
                No friends connected yet. Share your join code or paste a
                friend&apos;s code above.
              </p>
            ) : (
              <div className="friends-list">
                {state.friends.map((f) => {
                  const colCount = f.cached_profile?.collections.length ?? 0;
                  return (
                    <div key={f.peer_id} className="friends-row">
                      <button
                        type="button"
                        className="friends-row-main"
                        onClick={() => {
                          setSelectedPeerId(f.peer_id);
                          setSelectedCol(null);
                        }}
                      >
                        <span
                          className={`friends-status-dot ${f.online ? "online" : "offline"}`}
                        />
                        <div>
                          <strong>{f.display_name}</strong>
                          <div className="muted">
                            {f.status === "pending_outgoing"
                              ? "Waiting for friend to accept"
                              : f.online
                                ? `Online · ${colCount} collection(s)`
                                : `Offline (Cached) · Last seen ${formatLastSeen(f.last_seen_at)} · ${colCount} collection(s)`}
                          </div>
                        </div>
                      </button>
                      <button
                        type="button"
                        className="danger"
                        aria-label={`Remove ${f.display_name}`}
                        onClick={() => void handleRemoveFriend(f.peer_id)}
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
        <div className="card friends-card">
          <div className="friends-row">
            <div>
              <span
                className={`pill ${selectedFriend.online ? "ok" : ""}`}
              >
                {selectedFriend.online
                  ? "Online"
                  : `Offline · Cached (${formatLastSeen(selectedFriend.last_seen_at)})`}
              </span>
            </div>
          </div>
          {!selectedFriend.cached_profile ||
          selectedFriend.cached_profile.collections.length === 0 ? (
            <p className="muted">
              No shared collections cached for {selectedFriend.display_name} yet.
              {!selectedFriend.online
                ? " They will appear once your friend comes online."
                : ""}
            </p>
          ) : (
            <div className="friends-list">
              {selectedFriend.cached_profile.collections.map((col) => (
                <button
                  key={col.id}
                  type="button"
                  className="friends-row friends-row-main"
                  onClick={() => setSelectedCol(col)}
                >
                  <div>
                    <strong>{col.name}</strong>{" "}
                    <span className="pill">
                      {col.kind === "active_loadout"
                        ? "Active Loadout"
                        : "Saved Collection"}
                    </span>
                    <div className="muted">
                      Updated {formatLastSeen(col.updated_at)}
                    </div>
                  </div>
                  <span className="muted">View &rarr;</span>
                </button>
              ))}
            </div>
          )}
        </div>
      )}

      {selectedFriend && selectedCol && (
        <div className="card friends-card">
          {decoding ? (
            <p className="muted">Decoding collection...</p>
          ) : decodedPreview ? (
            <>
              <div className="friends-sync-bar">
                <div>
                  <strong>
                    {decodedPreview.game.title ?? "Shared Collection"}
                  </strong>{" "}
                  <span className="muted">
                    {decodedPreview.mods.length} mod(s) in load order
                  </span>
                  {linkedInstalled ? (
                    <span className={`pill ${hasUpdate ? "warn" : "ok"}`}>
                      {hasUpdate ? "Update available" : "Synced"}
                    </span>
                  ) : null}
                </div>
                <div className="friends-row-actions">
                  {managed.length > 0 ? (
                    <>
                      <select
                        value={targetGameId}
                        onChange={(e) => setTargetGameId(e.target.value)}
                        aria-label="Target managed game"
                      >
                        {managed.map((g) => (
                          <option key={g.id} value={g.id}>
                            {g.title}
                          </option>
                        ))}
                      </select>
                      <button
                        type="button"
                        disabled={syncing || !targetGameId}
                        onClick={() => void handleJoinOrSync()}
                      >
                        {syncing
                          ? "Syncing..."
                          : hasUpdate
                            ? "Sync Update"
                            : linkedInstalled
                              ? "Re-sync Collection"
                              : "Join & Sync Collection"}
                      </button>
                    </>
                  ) : (
                    <span className="muted">
                      Manage this game in Library to join &amp; sync.
                    </span>
                  )}
                </div>
              </div>

              {linkedInstalled && (
                <label className="friends-autosync-toggle">
                  <input
                    type="checkbox"
                    checked={Boolean(linkedInstalled.friend_sync?.auto_sync)}
                    onChange={(e) => void handleToggleAutoSync(e.target.checked)}
                  />
                  Auto-sync when {selectedFriend.display_name} updates this
                  collection
                </label>
              )}

              <div className="friends-mod-list">
                {decodedPreview.mods.map((m, idx) => (
                  <div key={`${m.s}-${idx}`} className="friends-mod-row">
                    <span className="friends-mod-order">#{idx + 1}</span>
                    <div className="friends-mod-meta">
                      <strong>
                        {m.display_name ?? m.name ?? `Mod ${m.mod_id ?? idx + 1}`}
                      </strong>
                      <span className="pill">{m.s}</span>
                      {m.version ? (
                        <span className="muted">v{m.version}</span>
                      ) : null}
                      {m.fomod ? (
                        <span className="pill ok">FOMOD Config</span>
                      ) : null}
                    </div>
                  </div>
                ))}
              </div>
            </>
          ) : (
            <p className="muted">Could not decode share code for this collection.</p>
          )}
        </div>
      )}
    </section>
  );
}
