import { useEffect, useRef, useState, type ChangeEvent, type FormEvent } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { api } from "./api";
import {
  CollectionSharingModal,
  formatVisibilityLabel,
  visibilityIcon,
} from "./CollectionSharingModal";
import { FriendAvatar, resizeProfilePicture } from "./FriendAvatar";
import { SettingsToggle } from "./SettingsSection";
import type { CollectionVisibility, FriendsState } from "./types";

export type SocialSettingsSectionProps = {
  friendsState: FriendsState | null;
  onFriendsStateChange: (next: FriendsState) => void;
  onNotice: (msg: string) => void;
  onError: (msg: string | null) => void;
};

export function SocialSettingsSection({
  friendsState,
  onFriendsStateChange,
  onNotice,
  onError,
}: SocialSettingsSectionProps) {
  const [nameInput, setNameInput] = useState(friendsState?.display_name ?? "");
  const [savingName, setSavingName] = useState(false);
  const [savingPicture, setSavingPicture] = useState(false);
  const [savingMutuals, setSavingMutuals] = useState(false);
  const [savingCacheRelay, setSavingCacheRelay] = useState(false);
  const [privacyModalOpen, setPrivacyModalOpen] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);

  const friendsListVisibility: CollectionVisibility =
    friendsState?.friends_list_visibility ?? { mode: "all_friends" };
  const allowMutualConnections = friendsState?.allow_mutual_connections !== false;
  const allowCacheRelay = friendsState?.allow_cache_relay !== false;

  useEffect(() => {
    setNameInput(friendsState?.display_name ?? "");
  }, [friendsState?.display_name]);

  async function handleSaveName(e: FormEvent) {
    e.preventDefault();
    if (!nameInput.trim()) return;
    onError(null);
    setSavingName(true);
    try {
      const next = await api.setP2pDisplayName(nameInput);
      onFriendsStateChange(next);
      onNotice("Display name updated.");
    } catch (err) {
      onError(String(err));
    } finally {
      setSavingName(false);
    }
  }

  async function handlePickPicture(e: ChangeEvent<HTMLInputElement>) {
    const file = e.target.files?.[0];
    e.target.value = "";
    if (!file) return;
    onError(null);
    setSavingPicture(true);
    try {
      const dataUrl = await resizeProfilePicture(file);
      const next = await api.setP2pProfilePicture(dataUrl);
      onFriendsStateChange(next);
      onNotice("Profile picture updated.");
    } catch (err) {
      onError(String(err));
    } finally {
      setSavingPicture(false);
    }
  }

  async function handleRemovePicture() {
    onError(null);
    setSavingPicture(true);
    try {
      const next = await api.setP2pProfilePicture(null);
      onFriendsStateChange(next);
      onNotice("Profile picture removed.");
    } catch (err) {
      onError(String(err));
    } finally {
      setSavingPicture(false);
    }
  }

  async function handleSaveFriendsListVisibility(nextVis: CollectionVisibility) {
    onError(null);
    try {
      const next = await api.setFriendsListVisibility(nextVis);
      onFriendsStateChange(next);
      onNotice("Friends list privacy updated.");
    } catch (err) {
      onError(String(err));
      throw err;
    }
  }

  async function handleToggleMutualConnections(allow: boolean) {
    onError(null);
    setSavingMutuals(true);
    try {
      const next = await api.setAllowMutualConnections(allow);
      onFriendsStateChange(next);
      onNotice(
        allow
          ? "Mutual connections enabled."
          : "Mutual connections disabled.",
      );
    } catch (err) {
      onError(String(err));
    } finally {
      setSavingMutuals(false);
    }
  }

  async function handleToggleCacheRelay(allow: boolean) {
    onError(null);
    setSavingCacheRelay(true);
    try {
      const next = await api.setAllowCacheRelay(allow);
      onFriendsStateChange(next);
      onNotice(
        allow
          ? "Offline friend cache sharing enabled."
          : "Offline friend cache sharing disabled.",
      );
    } catch (err) {
      onError(String(err));
    } finally {
      setSavingCacheRelay(false);
    }
  }

  return (
    <>
      <div className="setting-block">
        <span>Display name</span>
        <form className="social-settings-name-form" onSubmit={handleSaveName}>
          <input
            type="text"
            value={nameInput}
            onChange={(e) => setNameInput(e.target.value)}
            placeholder="Your display name"
            aria-label="Social display name"
            disabled={savingName}
          />
          <button type="submit" disabled={!nameInput.trim() || savingName}>
            {savingName ? "Saving..." : "Save"}
          </button>
        </form>
        <p className="note">Shown to friends when sharing collections over P2P.</p>
      </div>

      <div className="setting-block">
        <span>Profile picture</span>
        <div className="social-settings-profile">
          <FriendAvatar
            name={friendsState?.display_name || nameInput || "You"}
            src={friendsState?.profile_picture}
            size="lg"
          />
          <div className="social-settings-profile-actions">
            <input
              ref={fileInputRef}
              type="file"
              accept="image/png,image/jpeg,image/webp,image/gif"
              aria-label="Upload profile picture"
              style={{ display: "none" }}
              onChange={(e) => void handlePickPicture(e)}
            />
            <button
              type="button"
              disabled={savingPicture}
              onClick={() => fileInputRef.current?.click()}
            >
              {savingPicture
                ? "Updating..."
                : friendsState?.profile_picture
                  ? "Change Picture"
                  : "Choose Picture"}
            </button>
            {friendsState?.profile_picture ? (
              <button
                type="button"
                className="danger"
                disabled={savingPicture}
                onClick={() => void handleRemovePicture()}
              >
                Remove
              </button>
            ) : null}
          </div>
        </div>
        <p className="note">
          Optional. Resized automatically and shared with connected friends.
        </p>
      </div>

      <div className="setting-block">
        <span>Friends list privacy</span>
        <div className="social-settings-privacy-row">
          <span className="pill">
            <FontAwesomeIcon icon={visibilityIcon(friendsListVisibility)} />{" "}
            {formatVisibilityLabel(friendsListVisibility)}
          </span>
          <button
            type="button"
            onClick={() => setPrivacyModalOpen(true)}
          >
            Change Privacy
          </button>
        </div>
        <p className="note">
          Controls who can see your connected friends list and discover mutual
          connections.
        </p>
      </div>

      <div className="setting-block">
        <span>Allow mutual connections</span>
        <SettingsToggle
          checked={allowMutualConnections}
          disabled={savingMutuals}
          onChange={(checked) => void handleToggleMutualConnections(checked)}
        >
          Let friends of friends see me in mutual friends lists
        </SettingsToggle>
        <p className="note">
          When turned off, your friends will not include you in their shared
          friends lists.
        </p>
      </div>

      <div className="setting-block">
        <span>Share cached friend profiles</span>
        <SettingsToggle
          checked={allowCacheRelay}
          disabled={savingCacheRelay}
          onChange={(checked) => void handleToggleCacheRelay(checked)}
        >
          Allow mutual friends to pull newer cached profiles of offline friends from me
        </SettingsToggle>
        <p className="note">
          When turned off, your client will not share cached profiles of offline
          friends with mutual friends.
        </p>
      </div>

      {privacyModalOpen && (
        <CollectionSharingModal
          collectionId="friends_list"
          collectionName="Friends List"
          modalTitle="Friends list privacy"
          legend="Friends list visibility"
          allFriendsDescription="All connected friends can view your friends list and see mutual connections."
          specificFriendsDescription="Only selected friends can view your friends list and discover mutual connections."
          privateDescription="Your friends list is hidden from all friends."
          initialVisibility={friendsListVisibility}
          friends={friendsState?.friends ?? []}
          onSave={handleSaveFriendsListVisibility}
          onClose={() => setPrivacyModalOpen(false)}
        />
      )}
    </>
  );
}
