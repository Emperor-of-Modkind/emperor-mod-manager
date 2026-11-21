import type { FriendRecord, FriendsState } from "./types";

export type DiscoverableRelationship =
  | "none"
  | "pending_outgoing"
  | "pending_incoming";

export type DiscoverablePlayer = {
  peer_id: string;
  display_name: string;
  mutual_friends: FriendRecord[];
  relationship: DiscoverableRelationship;
};

const PEER_ID_HEX_RE = /^[0-9a-fA-F]{64}$/;

/**
 * Returns true if the input looks like a raw 64-char hex iroh EndpointId.
 */
export function isPeerIdFormat(raw: string): boolean {
  return PEER_ID_HEX_RE.test(raw.trim());
}

/**
 * Returns connected friends shared between the local user and `friend`,
 * based on `friend.cached_profile.friends`.
 */
export function getMutualFriends(
  state: FriendsState | null | undefined,
  friend: FriendRecord | null | undefined,
): FriendRecord[] {
  if (!state || !friend) return [];
  const theirFriends = friend.cached_profile?.friends;
  if (!theirFriends || theirFriends.length === 0) return [];
  const myConnectedById = new Map<string, FriendRecord>();
  for (const f of state.friends) {
    if (f.status === "connected" && f.peer_id !== friend.peer_id) {
      myConnectedById.set(f.peer_id, f);
    }
  }
  const mutuals: FriendRecord[] = [];
  const seen = new Set<string>();
  for (const sf of theirFriends) {
    if (seen.has(sf.peer_id)) continue;
    const match = myConnectedById.get(sf.peer_id);
    if (match) {
      seen.add(sf.peer_id);
      mutuals.push(match);
    }
  }
  mutuals.sort((a, b) => a.display_name.localeCompare(b.display_name));
  return mutuals;
}

/**
 * Returns all connected friends of the local user whose shared friends list
 * includes `targetPeerId`.
 */
export function getMutualFriendsForPeerId(
  state: FriendsState | null | undefined,
  targetPeerId: string,
): FriendRecord[] {
  if (!state || !targetPeerId) return [];
  const mutuals: FriendRecord[] = [];
  for (const f of state.friends) {
    if (f.status !== "connected" || f.peer_id === targetPeerId) continue;
    const theirFriends = f.cached_profile?.friends;
    if (theirFriends?.some((sf) => sf.peer_id === targetPeerId)) {
      mutuals.push(f);
    }
  }
  mutuals.sort((a, b) => a.display_name.localeCompare(b.display_name));
  return mutuals;
}

/**
 * Aggregates friends-of-friends across all connected friends who share their
 * friends list, excluding the local user and already-connected friends.
 */
export function getDiscoverablePlayers(
  state: FriendsState | null | undefined,
): DiscoverablePlayer[] {
  if (!state) return [];
  const selfId = state.peer_id ?? null;
  const connectedIds = new Set<string>();
  const pendingOutgoingIds = new Set<string>();
  for (const f of state.friends) {
    if (f.status === "connected") {
      connectedIds.add(f.peer_id);
    } else if (f.status === "pending_outgoing") {
      pendingOutgoingIds.add(f.peer_id);
    }
  }
  const pendingIncomingIds = new Set(
    state.pending_requests.map((r) => r.peer_id),
  );

  const byPeer = new Map<
    string,
    {
      peer_id: string;
      display_name: string;
      mutual_friends: FriendRecord[];
    }
  >();

  for (const friend of state.friends) {
    if (friend.status !== "connected") continue;
    const shared = friend.cached_profile?.friends;
    if (!shared) continue;
    for (const sf of shared) {
      if (!sf.peer_id) continue;
      if (sf.peer_id === selfId || connectedIds.has(sf.peer_id)) continue;
      const existing = byPeer.get(sf.peer_id);
      if (existing) {
        if (!existing.mutual_friends.some((m) => m.peer_id === friend.peer_id)) {
          existing.mutual_friends.push(friend);
        }
        if (
          (!existing.display_name || existing.display_name === "Friend") &&
          sf.display_name
        ) {
          existing.display_name = sf.display_name;
        }
      } else {
        byPeer.set(sf.peer_id, {
          peer_id: sf.peer_id,
          display_name: sf.display_name || "Friend",
          mutual_friends: [friend],
        });
      }
    }
  }

  const players: DiscoverablePlayer[] = [];
  for (const entry of byPeer.values()) {
    entry.mutual_friends.sort((a, b) =>
      a.display_name.localeCompare(b.display_name),
    );
    const relationship: DiscoverableRelationship = pendingIncomingIds.has(
      entry.peer_id,
    )
      ? "pending_incoming"
      : pendingOutgoingIds.has(entry.peer_id)
        ? "pending_outgoing"
        : "none";
    players.push({
      peer_id: entry.peer_id,
      display_name: entry.display_name,
      mutual_friends: entry.mutual_friends,
      relationship,
    });
  }

  players.sort((a, b) => {
    if (b.mutual_friends.length !== a.mutual_friends.length) {
      return b.mutual_friends.length - a.mutual_friends.length;
    }
    return a.display_name.localeCompare(b.display_name);
  });

  return players;
}

export function formatMutualSummary(mutuals: FriendRecord[]): string {
  if (mutuals.length === 0) return "";
  const countLabel =
    mutuals.length === 1 ? "1 mutual friend" : `${mutuals.length} mutual friends`;
  const names = mutuals
    .slice(0, 3)
    .map((m) => m.display_name)
    .join(", ");
  const extra = mutuals.length > 3 ? ` +${mutuals.length - 3}` : "";
  return `${countLabel} · ${names}${extra}`;
}
