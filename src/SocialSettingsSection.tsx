import { useEffect, useRef, useState, type ChangeEvent, type FormEvent } from "react";
import { api } from "./api";
import { FriendAvatar, resizeProfilePicture } from "./FriendAvatar";
import type { FriendsState } from "./types";

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
  const fileInputRef = useRef<HTMLInputElement>(null);

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
    </>
  );
}
