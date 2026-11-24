import { useEffect, useMemo, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowLeft,
  faCloudArrowDown,
  faCopy,
  faLayerGroup,
  faRss,
  faUserGroup,
  faUserPlus,
} from "@fortawesome/free-solid-svg-icons";

import { api } from "./api";
import { FriendAvatar } from "./FriendAvatar";
import { Segment, SegmentTab } from "./motion";
import { SourceMark } from "./SourceMark";
import type {
  FriendFeedEvent,
  FriendRecord,
  FriendShareRef,
  FriendsState,
  ManagedGame,
  ShareDecodePreview,
  ShareModEntry,
  ShareModMetadataHit,
} from "./types";

type FriendsSubTab = "feed" | "friends" | "requests";
type FriendProfileTab = "feed" | "collections" | "friends";

type Props = {
  friendsState: FriendsState | null;
  activeGame: ManagedGame | null;
  online: boolean;
  onRefresh: () => Promise<void>;
  onNotify: (kind: "info" | "warn" | "error", msg: string) => void;
  onPushBack?: (handler: () => void) => void;
};

function shareModKey(m: ShareModEntry): string | null {
  if (m.s === "nexus") {
    const domain = m.domain?.trim().toLowerCase();
    if (!domain || !m.mod_id) return null;
    return `nexus:${domain}:${m.mod_id}`;
  }
  if (m.s === "thunderstore") {
    const c = m.community?.trim().toLowerCase();
    const ns = m.namespace?.trim().toLowerCase();
    const n = m.name?.trim().toLowerCase();
    if (!c || !ns || !n) return null;
    return `thunderstore:${c}:${ns}:${n}`;
  }
  if (m.s === "modio") {
    if (!m.game_id || !m.mod_id) return null;
    return `modio:${m.game_id}:${m.mod_id}`;
  }
  return null;
}

function feedSummary(evt: FriendFeedEvent): string {
  switch (evt.kind) {
    case "collection_added":
      return `shared "${evt.collection_name}"`;
    case "collection_updated":
      return `updated "${evt.collection_name}"`;
    case "collection_renamed":
      return `renamed "${evt.previous_name || "a collection"}" to "${evt.collection_name}"`;
    default:
      return `updated "${evt.collection_name}"`;
  }
}

function formatRelativeTime(iso: string): string {
  const ts = Date.parse(iso);
  if (Number.isNaN(ts)) return iso;
  const diffSec = Math.max(0, Math.floor((Date.now() - ts) / 1000));
  if (diffSec < 60) return "just now";
  if (diffSec < 3600) return `${Math.floor(diffSec / 60)}m ago`;
  if (diffSec < 86400) return `${Math.floor(diffSec / 3600)}h ago`;
  return `${Math.floor(diffSec / 86400)}d ago`;
}

export function MobileFriendsView({
  friendsState,
  activeGame,
  online,
  onRefresh,
  onNotify,
  onPushBack,
}: Props) {
  // Default to Friend Feed to match desktop FriendsPage
  const [subTab, setSubTab] = useState<FriendsSubTab>("feed");
  const [selectedPeerId, setSelectedPeerId] = useState<string | null>(null);
  const [selectedColId, setSelectedColId] = useState<string | null>(null);
  const [profileTab, setProfileTab] = useState<FriendProfileTab>("feed");
  const [syncingColId, setSyncingColId] = useState<string | null>(null);

  // Decoded friend collection preview + mod artwork
  const [colPreview, setColPreview] = useState<ShareDecodePreview | null>(null);
  const [colModMeta, setColModMeta] = useState<Record<string, ShareModMetadataHit>>({});
  const [colLoading, setColLoading] = useState(false);

  const onNotifyRef = useRef(onNotify);
  onNotifyRef.current = onNotify;

  const friends = friendsState?.friends ?? [];
  const feedEvents = friendsState?.feed_events ?? [];
  const requests = friendsState?.pending_requests ?? [];

  const selectedFriend: FriendRecord | null = selectedPeerId
    ? friends.find((f) => f.peer_id === selectedPeerId) ?? null
    : null;

  const selectedFriendCol: FriendShareRef | null = useMemo(() => {
    if (!selectedFriend || !selectedColId) return null;
    return (
      selectedFriend.cached_profile?.collections?.find((c) => c.id === selectedColId) ??
      null
    );
  }, [selectedFriend, selectedColId]);

  const selectedColShareCode = selectedFriendCol?.share_code ?? null;

  useEffect(() => {
    if (!selectedColShareCode) {
      setColPreview(null);
      setColModMeta({});
      setColLoading(false);
      return;
    }
    let cancelled = false;
    setColLoading(true);
    setColPreview(null);
    setColModMeta({});
    api
      .mobileCatalogRpc<ShareDecodePreview>("decode_share_code", {
        code: selectedColShareCode,
      })
      .then((preview) => {
        if (cancelled) return;
        setColPreview(preview);
        if (preview.mods.length > 0) {
          void api
            .mobileCatalogRpc<ShareModMetadataHit[]>("resolve_share_mod_metadata", {
              mods: preview.mods,
            })
            .then((hits) => {
              if (cancelled || !Array.isArray(hits)) return;
              const map: Record<string, ShareModMetadataHit> = {};
              for (const h of hits) map[h.key] = h;
              setColModMeta(map);
            })
            .catch(() => {});
        }
      })
      .catch((e) => {
        if (!cancelled) {
          onNotifyRef.current("error", `Failed to decode friend collection: ${String(e)}`);
        }
      })
      .finally(() => {
        if (!cancelled) setColLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [selectedColShareCode]);

  const selectedFriendFeedEvents = useMemo(() => {
    if (!selectedFriend) return [];
    return feedEvents.filter(
      (ev) => !ev.is_self && ev.peer_id === selectedFriend.peer_id,
    );
  }, [feedEvents, selectedFriend]);

  const selectedFriendFriendsList = useMemo(() => {
    if (!selectedFriend) return [];
    const list = selectedFriend.cached_profile?.friends ?? [];
    return list.filter((sf) => sf.peer_id !== friendsState?.peer_id);
  }, [selectedFriend, friendsState?.peer_id]);

  if (!friendsState) {
    return (
      <div className="mobile-empty-pane">
        <p className="muted">Sync with your paired PC to view your Friend Feed and friends.</p>
      </div>
    );
  }

  const openFriendProfile = (peerId: string) => {
    setSelectedColId(null);
    setSelectedPeerId(peerId);
    setProfileTab("feed");
    onPushBack?.(() => {
      setSelectedColId(null);
      setSelectedPeerId(null);
    });
  };

  const openFriendCollection = (col: FriendShareRef) => {
    setSelectedColId(col.id);
    onPushBack?.(() => setSelectedColId(null));
  };

  const syncFriendCollection = async (peerId: string, col: FriendShareRef) => {
    if (!activeGame) {
      onNotify("warn", "Select a managed game in Library first to sync a friend's collection.");
      return;
    }
    setSyncingColId(col.id);
    try {
      await api.mobileSyncFriendCollection(activeGame.id, peerId, col.id);
      await onRefresh();
      onNotify("info", `Started syncing "${col.name}" to your PC.`);
    } catch (e) {
      onNotify("error", `Failed to sync collection: ${String(e)}`);
    } finally {
      setSyncingColId(null);
    }
  };

  // Friend Collection Detail View
  if (selectedFriend && selectedFriendCol) {
    return (
      <div className="mobile-detail-view">
        <button
          type="button"
          className="ghost mobile-back-btn"
          onClick={() => setSelectedColId(null)}
        >
          <FontAwesomeIcon icon={faArrowLeft} /> Back to {selectedFriend.display_name}
        </button>

        <div className="mobile-surface-card mobile-detail-header-card">
          <h2>{selectedFriendCol.name}</h2>
          <p className="muted">
            Shared by {selectedFriend.display_name}
            {colPreview?.game?.title ? ` · ${colPreview.game.title}` : ""}
            {colPreview ? ` · ${colPreview.mods.length} mods` : ""}
            {` · Updated ${formatRelativeTime(selectedFriendCol.updated_at)}`}
          </p>
          <div className="mobile-action-bar">
            <button
              type="button"
              className="primary"
              disabled={!online || syncingColId !== null}
              onClick={() => void syncFriendCollection(selectedFriend.peer_id, selectedFriendCol)}
            >
              <FontAwesomeIcon icon={faCloudArrowDown} />{" "}
              {syncingColId === selectedFriendCol.id ? "Syncing…" : "Sync to PC"}
            </button>
            <button
              type="button"
              className="secondary"
              onClick={() => {
                void navigator.clipboard?.writeText(selectedFriendCol.share_code);
                onNotify("info", "Share code copied to clipboard");
              }}
            >
              <FontAwesomeIcon icon={faCopy} /> Copy Share Code
            </button>
          </div>
        </div>

        <section className="mobile-detail-section">
          <h3>Mods ({colPreview?.mods.length ?? 0})</h3>
          {colLoading ? (
            <p className="muted">Loading collection mods from desktop…</p>
          ) : colPreview && colPreview.mods.length > 0 ? (
            <div className="mobile-mod-list">
              {colPreview.mods.map((m, idx) => {
                const key = shareModKey(m);
                const meta = key ? colModMeta[key] : undefined;
                const art = meta?.picture_url ?? null;
                return (
                  <div key={`${m.display_name || m.name || idx}:${idx}`} className="mobile-mod-row">
                    {art ? (
                      <img
                        className="mobile-mod-thumb"
                        src={art}
                        alt={m.display_name || m.name || "Mod"}
                        loading="lazy"
                      />
                    ) : (
                      <div className="mobile-mod-thumb mobile-mod-thumb-placeholder">
                        <SourceMark source={m.s} />
                      </div>
                    )}
                    <div className="mobile-mod-info">
                      <strong>{m.display_name || m.name || `Mod #${idx + 1}`}</strong>
                      <span className="muted">
                        {meta?.author ? `${meta.author} · ` : ""}
                        {m.s}
                        {m.version ? ` · v${m.version}` : ""}
                      </span>
                    </div>
                    <SourceMark source={m.s} />
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="muted">Could not decode mods for this collection.</p>
          )}
        </section>
      </div>
    );
  }

  // Friend Profile View
  if (selectedFriend) {
    const profile = selectedFriend.cached_profile;
    const collections = profile?.collections ?? [];
    const activeLoadouts = collections.filter((c) => c.kind === "active_loadout");
    const savedCols = collections.filter((c) => c.kind === "saved_collection");

    return (
      <div className="mobile-detail-view">
        <button
          type="button"
          className="ghost mobile-back-btn"
          onClick={() => setSelectedPeerId(null)}
        >
          <FontAwesomeIcon icon={faArrowLeft} /> Back to Friends
        </button>

        <div className="mobile-surface-card mobile-friend-profile-hero">
          <FriendAvatar
            name={selectedFriend.display_name}
            src={selectedFriend.profile_picture ?? profile?.profile_picture}
            online={selectedFriend.online}
            size="lg"
            showStatus
          />
          <div className="mobile-friend-profile-meta">
            <h2>{selectedFriend.display_name}</h2>
            <p className="muted">
              {selectedFriend.online
                ? "Online"
                : selectedFriend.last_seen_at
                  ? `Last seen ${formatRelativeTime(selectedFriend.last_seen_at)}`
                  : "Offline"}
              {selectedFriend.cached_via_peer_name
                ? ` · Cached via ${selectedFriend.cached_via_peer_name}`
                : ""}
            </p>
          </div>
        </div>

        <Segment>
          <SegmentTab
            layoutId="mobile-friend-profile-tabs"
            active={profileTab === "feed"}
            onClick={() => setProfileTab("feed")}
          >
            <FontAwesomeIcon icon={faRss} /> Feed
          </SegmentTab>
          <SegmentTab
            layoutId="mobile-friend-profile-tabs"
            active={profileTab === "collections"}
            onClick={() => setProfileTab("collections")}
          >
            <FontAwesomeIcon icon={faLayerGroup} /> Collections ({collections.length})
          </SegmentTab>
          <SegmentTab
            layoutId="mobile-friend-profile-tabs"
            active={profileTab === "friends"}
            onClick={() => setProfileTab("friends")}
          >
            <FontAwesomeIcon icon={faUserGroup} /> Friends ({selectedFriendFriendsList.length})
          </SegmentTab>
        </Segment>

        {profileTab === "feed" &&
          (selectedFriendFeedEvents.length === 0 ? (
            <div className="mobile-surface-card">
              <p className="muted">
                No recent activity from {selectedFriend.display_name} yet.
              </p>
            </div>
          ) : (
            <div className="mobile-card-list">
              {selectedFriendFeedEvents.map((evt) => {
                const linkedCol = collections.find((c) => c.id === evt.collection_id);
                return (
                  <button
                    key={evt.id}
                    type="button"
                    className="mobile-feed-card"
                    disabled={!linkedCol}
                    onClick={() => {
                      if (linkedCol) openFriendCollection(linkedCol);
                    }}
                  >
                    <FriendAvatar
                      name={selectedFriend.display_name}
                      src={selectedFriend.profile_picture ?? profile?.profile_picture}
                      online={selectedFriend.online}
                      size="sm"
                      showStatus
                    />
                    <div className="mobile-feed-card-body">
                      <div className="mobile-feed-card-top">
                        <strong>{selectedFriend.display_name}</strong>
                        <span className="muted">{formatRelativeTime(evt.occurred_at)}</span>
                      </div>
                      <p>
                        {feedSummary(evt)}
                        {evt.game_title ? ` for ${evt.game_title}` : ""}
                      </p>
                      {evt.diff ? (
                        <div className="mobile-feed-diff muted">
                          {evt.diff.mods_added_count > 0 ? `+${evt.diff.mods_added_count} added` : null}
                          {evt.diff.mods_removed_count > 0 ? ` · -${evt.diff.mods_removed_count} removed` : null}
                          {evt.diff.mods_updated_count > 0 ? ` · ${evt.diff.mods_updated_count} updated` : null}
                        </div>
                      ) : evt.mod_count != null ? (
                        <span className="muted">{evt.mod_count} mods</span>
                      ) : null}
                    </div>
                  </button>
                );
              })}
            </div>
          ))}

        {profileTab === "collections" && (
          <>
            <section className="mobile-detail-section">
              <h3>Active Loadouts ({activeLoadouts.length})</h3>
              {activeLoadouts.length === 0 ? (
                <p className="muted">No shared active loadouts yet.</p>
              ) : (
                <div className="mobile-card-list">
                  {activeLoadouts.map((col) => (
                    <div key={col.id} className="mobile-collection-card">
                      <button
                        type="button"
                        className="mobile-collection-card-main"
                        onClick={() => openFriendCollection(col)}
                      >
                        <strong>{col.name}</strong>
                        <span className="muted">
                          Updated {formatRelativeTime(col.updated_at)}
                          {col.matched_local_collection
                            ? ` · Matches ${col.matched_local_collection.name}`
                            : ""}
                        </span>
                      </button>
                      <button
                        type="button"
                        className="primary"
                        disabled={!online || syncingColId !== null}
                        onClick={() => void syncFriendCollection(selectedFriend.peer_id, col)}
                      >
                        <FontAwesomeIcon icon={faCloudArrowDown} />{" "}
                        {syncingColId === col.id ? "Syncing…" : "Sync to PC"}
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </section>

            <section className="mobile-detail-section">
              <h3>Shared Collections ({savedCols.length})</h3>
              {savedCols.length === 0 ? (
                <p className="muted">No shared saved collections yet.</p>
              ) : (
                <div className="mobile-card-list">
                  {savedCols.map((col) => (
                    <div key={col.id} className="mobile-collection-card">
                      <button
                        type="button"
                        className="mobile-collection-card-main"
                        onClick={() => openFriendCollection(col)}
                      >
                        <strong>{col.name}</strong>
                        <span className="muted">Updated {formatRelativeTime(col.updated_at)}</span>
                      </button>
                      <button
                        type="button"
                        className="primary"
                        disabled={!online || syncingColId !== null}
                        onClick={() => void syncFriendCollection(selectedFriend.peer_id, col)}
                      >
                        <FontAwesomeIcon icon={faCloudArrowDown} />{" "}
                        {syncingColId === col.id ? "Syncing…" : "Sync to PC"}
                      </button>
                    </div>
                  ))}
                </div>
              )}
            </section>
          </>
        )}

        {profileTab === "friends" &&
          (selectedFriendFriendsList.length === 0 ? (
            <div className="mobile-surface-card">
              <p className="muted">
                {selectedFriend.display_name} has not shared their friends list.
              </p>
            </div>
          ) : (
            <div className="mobile-card-list">
              {selectedFriendFriendsList.map((sf) => {
                const knownFriend = friends.find((f) => f.peer_id === sf.peer_id);
                return (
                  <button
                    key={sf.peer_id}
                    type="button"
                    className="mobile-friend-card"
                    disabled={!knownFriend}
                    onClick={() => {
                      if (knownFriend) openFriendProfile(knownFriend.peer_id);
                    }}
                  >
                    <FriendAvatar
                      name={sf.display_name}
                      src={knownFriend?.profile_picture}
                      online={Boolean(sf.online ?? knownFriend?.online)}
                      size="sm"
                      showStatus={sf.online != null || Boolean(knownFriend)}
                    />
                    <div className="mobile-friend-card-body">
                      <strong>{sf.display_name}</strong>
                      <span className="muted">
                        {knownFriend
                          ? knownFriend.online
                            ? "Mutual friend · Online"
                            : "Mutual friend"
                          : sf.online
                            ? "Online"
                            : "Friend"}
                      </span>
                    </div>
                    {knownFriend ? <span className="pill">Profile</span> : null}
                  </button>
                );
              })}
            </div>
          ))}
      </div>
    );
  }

  return (
    <div className="mobile-friends-pane">
      <div className="mobile-toolbar-row">
        <h2>Friends</h2>
        <Segment>
          <SegmentTab
            layoutId="mobile-friends-tabs"
            active={subTab === "feed"}
            onClick={() => setSubTab("feed")}
          >
            <FontAwesomeIcon icon={faRss} /> Feed
          </SegmentTab>
          <SegmentTab
            layoutId="mobile-friends-tabs"
            active={subTab === "friends"}
            onClick={() => setSubTab("friends")}
          >
            <FontAwesomeIcon icon={faUserGroup} /> Friends ({friends.length})
          </SegmentTab>
          <SegmentTab
            layoutId="mobile-friends-tabs"
            active={subTab === "requests"}
            onClick={() => setSubTab("requests")}
          >
            <FontAwesomeIcon icon={faUserPlus} /> Requests
            {requests.length > 0 ? ` (${requests.length})` : ""}
          </SegmentTab>
        </Segment>
      </div>

      {subTab === "feed" ? (
        feedEvents.length === 0 ? (
          <div className="mobile-surface-card">
            <p className="muted">
              No recent friend activity yet. Updates to friends&apos; shared loadouts and collections appear here.
            </p>
          </div>
        ) : (
          <div className="mobile-card-list">
            {feedEvents.map((evt) => {
              const friend = friends.find((f) => f.peer_id === evt.peer_id);
              return (
                <button
                  key={evt.id}
                  type="button"
                  className="mobile-feed-card"
                  onClick={() => {
                    if (friend) openFriendProfile(friend.peer_id);
                  }}
                  disabled={!friend}
                >
                  <FriendAvatar
                    name={evt.friend_display_name}
                    src={friend?.profile_picture}
                    online={friend?.online}
                    size="sm"
                    showStatus={Boolean(friend)}
                  />
                  <div className="mobile-feed-card-body">
                    <div className="mobile-feed-card-top">
                      <strong>{evt.friend_display_name}</strong>
                      <span className="muted">{formatRelativeTime(evt.occurred_at)}</span>
                    </div>
                    <p>
                      {feedSummary(evt)}
                      {evt.game_title ? ` for ${evt.game_title}` : ""}
                    </p>
                    {evt.diff ? (
                      <div className="mobile-feed-diff muted">
                        {evt.diff.mods_added_count > 0 ? `+${evt.diff.mods_added_count} added` : null}
                        {evt.diff.mods_removed_count > 0 ? ` · -${evt.diff.mods_removed_count} removed` : null}
                        {evt.diff.mods_updated_count > 0 ? ` · ${evt.diff.mods_updated_count} updated` : null}
                      </div>
                    ) : evt.mod_count != null ? (
                      <span className="muted">{evt.mod_count} mods</span>
                    ) : null}
                  </div>
                </button>
              );
            })}
          </div>
        )
      ) : subTab === "friends" ? (
        friends.length === 0 ? (
          <div className="mobile-surface-card">
            <p className="muted">No friends added on your paired PC yet.</p>
          </div>
        ) : (
          <div className="mobile-card-list">
            {friends.map((friend) => (
              <button
                key={friend.peer_id}
                type="button"
                className="mobile-friend-card"
                onClick={() => openFriendProfile(friend.peer_id)}
              >
                <FriendAvatar
                  name={friend.display_name}
                  src={friend.profile_picture ?? friend.cached_profile?.profile_picture}
                  online={friend.online}
                  size="md"
                  showStatus
                />
                <div className="mobile-friend-card-body">
                  <strong>{friend.display_name}</strong>
                  <span className="muted">
                    {friend.online ? "Online" : "Offline"}
                    {friend.cached_profile?.collections?.length
                      ? ` · ${friend.cached_profile.collections.length} shared`
                      : ""}
                  </span>
                </div>
                <span className="pill">Profile</span>
              </button>
            ))}
          </div>
        )
      ) : requests.length === 0 ? (
        <div className="mobile-surface-card">
          <p className="muted">No pending friend requests.</p>
        </div>
      ) : (
        <div className="mobile-card-list">
          {requests.map((req) => (
            <div key={req.peer_id} className="mobile-friend-card">
              <FriendAvatar
                name={req.display_name}
                src={req.profile_picture}
                size="sm"
              />
              <div className="mobile-friend-card-body">
                <strong>{req.display_name}</strong>
                <span className="muted">
                  Requested {formatRelativeTime(req.received_at)}
                </span>
              </div>
              <span className="pill">Pending on PC</span>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
