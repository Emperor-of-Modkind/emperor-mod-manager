import type {
  FriendMatchedLocalCollection,
  FriendRecord,
  FriendShareRef,
  InstalledCollection,
  LocalShareableCollection,
  SavedCollectionEntry,
} from "./types";

/** Returns connected friends who are currently syncing the given local collection ID (`active:<gameId>` or `saved:<id>`). */
export function getFriendsSyncingCollection(
  friends: readonly FriendRecord[] | undefined | null,
  collectionId: string,
): FriendRecord[] {
  if (!friends || !collectionId) return [];
  return friends.filter(
    (f) =>
      f.status === "connected" &&
      Array.isArray(f.syncing_my_collections) &&
      f.syncing_my_collections.includes(collectionId),
  );
}

/** Matches an installed collection to its corresponding `saved:<id>` P2P collection ID, if any. */
export function findMatchingSavedCollectionId(
  col: Pick<InstalledCollection, "id" | "profile_code">,
  savedCollections: readonly SavedCollectionEntry[],
): string | null {
  const trimmedCode = col.profile_code?.trim();
  const match = savedCollections.find(
    (entry) =>
      (entry.share_id && entry.share_id === col.id) ||
      (trimmedCode && entry.code.trim() === trimmedCode),
  );
  return match ? `saved:${match.id}` : null;
}

/** Resolves the list of local collection names/entries a friend is currently syncing. */
export function getCollectionsSyncedByFriend(
  friend: Pick<FriendRecord, "syncing_my_collections">,
  localCollections: readonly LocalShareableCollection[] | undefined | null,
): Array<{ id: string; name: string }> {
  const ids = friend.syncing_my_collections ?? [];
  if (ids.length === 0) return [];
  const byId = new Map<string, LocalShareableCollection>();
  for (const col of localCollections ?? []) {
    byId.set(col.id, col);
  }
  return ids.map((id) => ({
    id,
    name: byId.get(id)?.name ?? id,
  }));
}

/** Formats a concise summary string for who is syncing a collection (used for tooltips and compact labels). */
export function formatSyncersSummary(
  syncers: readonly FriendRecord[],
  prefix = "Synced by",
): string {
  if (syncers.length === 0) return "";
  const names = syncers.map((f) => f.display_name.trim() || "Friend");
  if (names.length <= 2) {
    return `${prefix} ${names.join(", ")}`;
  }
  return `${prefix} ${names[0]}, ${names[1]} +${names.length - 2}`;
}

/** Returns a friend's active loadouts that resolve to one of your local collections. */
export function getFriendActiveLoadoutMatches(
  friend: Pick<FriendRecord, "cached_profile"> | undefined | null,
): Array<{ col: FriendShareRef; match: FriendMatchedLocalCollection }> {
  const cols = friend?.cached_profile?.collections ?? [];
  const out: Array<{ col: FriendShareRef; match: FriendMatchedLocalCollection }> = [];
  for (const col of cols) {
    if (col.kind === "active_loadout" && col.matched_local_collection) {
      out.push({ col, match: col.matched_local_collection });
    }
  }
  return out;
}

/** Formats a concise badge/pill label describing how a friend's active loadout relates to your collection. */
export function formatActiveLoadoutMatchLabel(
  match: FriendMatchedLocalCollection,
): string {
  switch (match.kind) {
    case "synced_to_yours":
      return match.has_local_changes
        ? `Synced to your "${match.name}" · Update pending`
        : `Synced to your "${match.name}"`;
    case "matches_saved":
    case "matches_installed":
      return `Matches your "${match.name}"`;
    case "matches_active_loadout":
      return "Matches your active loadout";
  }
}

/** Returns connected friends whose active loadout currently resolves to the given local collection ID (`saved:<id>` or `active:<gameId>`). */
export function getFriendsWithActiveLoadoutMatching(
  friends: readonly FriendRecord[] | undefined | null,
  localCollectionId: string,
): FriendRecord[] {
  if (!friends || !localCollectionId) return [];
  return friends.filter(
    (f) =>
      f.status === "connected" &&
      (f.cached_profile?.collections ?? []).some(
        (col) =>
          col.kind === "active_loadout" &&
          col.matched_local_collection?.id === localCollectionId,
      ),
  );
}
