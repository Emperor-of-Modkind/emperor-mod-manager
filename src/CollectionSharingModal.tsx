import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faLock,
  faUserCheck,
  faUsers,
} from "@fortawesome/free-solid-svg-icons";
import { Modal } from "./Modal";
import type {
  CollectionVisibility,
  FriendRecord,
  LocalShareableCollection,
} from "./types";

export function resolveCollectionVisibility(
  col: LocalShareableCollection | undefined,
  collectionId: string,
): CollectionVisibility {
  if (col?.visibility) return col.visibility;
  if (col?.hidden) return { mode: "private" };
  if (collectionId.startsWith("saved:")) return { mode: "private" };
  return { mode: "all_friends" };
}

export function formatVisibilityLabel(visibility: CollectionVisibility): string {
  switch (visibility.mode) {
    case "all_friends":
      return "All friends";
    case "specific_friends": {
      const count = visibility.peer_ids.length;
      return count === 1 ? "1 friend" : `${count} friends`;
    }
    case "private":
      return "Private";
  }
}

export function visibilityIcon(visibility: CollectionVisibility) {
  switch (visibility.mode) {
    case "all_friends":
      return faUsers;
    case "specific_friends":
      return faUserCheck;
    case "private":
      return faLock;
  }
}

export type CollectionSharingModalProps = {
  collectionId: string;
  collectionName: string;
  initialVisibility: CollectionVisibility;
  friends: FriendRecord[];
  onSave: (visibility: CollectionVisibility) => Promise<void>;
  onClose: () => void;
};

export function CollectionSharingModal({
  collectionName,
  initialVisibility,
  friends,
  onSave,
  onClose,
}: CollectionSharingModalProps) {
  const [mode, setMode] = useState<CollectionVisibility["mode"]>(
    initialVisibility.mode,
  );
  const [selectedPeers, setSelectedPeers] = useState<string[]>(
    initialVisibility.mode === "specific_friends"
      ? initialVisibility.peer_ids
      : [],
  );
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    setMode(initialVisibility.mode);
    setSelectedPeers(
      initialVisibility.mode === "specific_friends"
        ? initialVisibility.peer_ids
        : [],
    );
  }, [initialVisibility]);

  const connectedFriends = friends.filter((f) => f.status === "connected");

  function togglePeer(peerId: string) {
    setSelectedPeers((prev) =>
      prev.includes(peerId)
        ? prev.filter((id) => id !== peerId)
        : [...prev, peerId],
    );
  }

  async function handleSave() {
    setSaving(true);
    try {
      const next: CollectionVisibility =
        mode === "all_friends"
          ? { mode: "all_friends" }
          : mode === "specific_friends"
            ? { mode: "specific_friends", peer_ids: selectedPeers }
            : { mode: "private" };
      await onSave(next);
      onClose();
    } finally {
      setSaving(false);
    }
  }

  return (
    <Modal
      className="modal-collection-sharing"
      title={`Sharing — ${collectionName}`}
      onClose={onClose}
      footer={
        <div className="modal-foot-actions">
          <button
            type="button"
            className="linkish"
            onClick={onClose}
            disabled={saving}
          >
            Cancel
          </button>
          <button
            type="button"
            onClick={() => void handleSave()}
            disabled={saving}
          >
            {saving ? "Saving..." : "Save"}
          </button>
        </div>
      }
    >
      <fieldset className="collection-sharing-options">
        <legend className="sr-only">Collection visibility</legend>
        <label className="collection-sharing-option">
          <input
            type="radio"
            name="collection-visibility-mode"
            value="all_friends"
            checked={mode === "all_friends"}
            onChange={() => setMode("all_friends")}
          />
          <FontAwesomeIcon icon={faUsers} />
          <div>
            <strong>All friends</strong>
            <small>Visible to all connected P2P friends.</small>
          </div>
        </label>

        <label className="collection-sharing-option">
          <input
            type="radio"
            name="collection-visibility-mode"
            value="specific_friends"
            checked={mode === "specific_friends"}
            onChange={() => setMode("specific_friends")}
          />
          <FontAwesomeIcon icon={faUserCheck} />
          <div>
            <strong>Specific friends</strong>
            <small>Only selected friends can see and sync this collection.</small>
          </div>
        </label>

        <label className="collection-sharing-option">
          <input
            type="radio"
            name="collection-visibility-mode"
            value="private"
            checked={mode === "private"}
            onChange={() => setMode("private")}
          />
          <FontAwesomeIcon icon={faLock} />
          <div>
            <strong>Private</strong>
            <small>Hidden from all friends.</small>
          </div>
        </label>
      </fieldset>

      {mode === "specific_friends" && (
        <div className="collection-sharing-friends">
          <div className="collection-sharing-friends-head">
            <span>Select friends ({selectedPeers.length})</span>
            {connectedFriends.length > 0 && (
              <div className="collection-sharing-friends-bulk">
                <button
                  type="button"
                  className="linkish"
                  onClick={() =>
                    setSelectedPeers(connectedFriends.map((f) => f.peer_id))
                  }
                >
                  Select all
                </button>
                <button
                  type="button"
                  className="linkish"
                  onClick={() => setSelectedPeers([])}
                >
                  Clear
                </button>
              </div>
            )}
          </div>
          {connectedFriends.length === 0 ? (
            <p className="muted">
              No connected friends yet. Add friends in the Friends tab to share
              with specific people.
            </p>
          ) : (
            <div className="collection-sharing-friends-list">
              {connectedFriends.map((f) => (
                <label
                  key={f.peer_id}
                  className="collection-sharing-friend-row"
                >
                  <input
                    type="checkbox"
                    checked={selectedPeers.includes(f.peer_id)}
                    onChange={() => togglePeer(f.peer_id)}
                  />
                  <span>{f.display_name}</span>
                  <span className="muted">
                    {f.online ? "Online" : "Offline"}
                  </span>
                </label>
              ))}
            </div>
          )}
        </div>
      )}
    </Modal>
  );
}
