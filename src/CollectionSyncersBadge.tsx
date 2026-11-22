import { FriendAvatar } from "./FriendAvatar";
import { formatSyncersSummary } from "./collectionSyncers";
import type { FriendRecord } from "./types";

export type CollectionSyncersBadgeProps = {
  syncers: readonly FriendRecord[];
  labelPrefix?: string;
  compact?: boolean;
};

const MAX_AVATARS = 3;

export function CollectionSyncersBadge({
  syncers,
  labelPrefix = "Synced by",
  compact = false,
}: CollectionSyncersBadgeProps) {
  if (syncers.length === 0) return null;

  const fullTooltip = `${labelPrefix} ${syncers
    .map((f) => f.display_name.trim() || "Friend")
    .join(", ")}`;

  const visibleAvatars = syncers.slice(0, MAX_AVATARS);
  const label = compact
    ? `${syncers.length} syncing`
    : formatSyncersSummary(syncers, labelPrefix);

  return (
    <span
      className="collection-syncers-badge"
      title={fullTooltip}
      aria-label={fullTooltip}
    >
      <span className="collection-syncers-avatars" aria-hidden="true">
        {visibleAvatars.map((f) => (
          <FriendAvatar
            key={f.peer_id}
            name={f.display_name}
            src={f.profile_picture ?? f.cached_profile?.profile_picture}
            online={f.online}
            size="xs"
          />
        ))}
      </span>
      <span className="collection-syncers-label">{label}</span>
    </span>
  );
}
