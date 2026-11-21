import { describe, expect, it } from "vitest";
import {
  formatMutualSummary,
  getDiscoverablePlayers,
  getMutualFriends,
  getMutualFriendsForPeerId,
  isPeerIdFormat,
} from "./friendConnections";
import type { FriendRecord, FriendsState } from "./types";

function makeFriend(
  peer_id: string,
  display_name: string,
  sharedFriends?: Array<{ peer_id: string; display_name: string }> | null,
  status: FriendRecord["status"] = "connected",
): FriendRecord {
  return {
    peer_id,
    display_name,
    status,
    online: true,
    cached_profile:
      sharedFriends === undefined
        ? null
        : {
            peer_id,
            display_name,
            updated_at: "2026-11-21T00:00:00Z",
            collections: [],
            friends: sharedFriends,
          },
  };
}

function makeState(
  friends: FriendRecord[],
  pending_requests: FriendsState["pending_requests"] = [],
): FriendsState {
  return {
    endpoint_ready: true,
    peer_id: "peer_self",
    display_name: "Self",
    display_name_set: true,
    friends,
    pending_requests,
    local_collections: [],
  };
}

describe("friendConnections", () => {
  it("computes mutual friends between local user and a connected friend", () => {
    const alice = makeFriend("peer_alice", "Alice", [
      { peer_id: "peer_bob", display_name: "Bob" },
      { peer_id: "peer_carol", display_name: "Carol" },
      { peer_id: "peer_self", display_name: "Self" },
    ]);
    const bob = makeFriend("peer_bob", "Bob", []);
    const pendingDan = makeFriend("peer_dan", "Dan", [], "pending_outgoing");
    const state = makeState([alice, bob, pendingDan]);

    const mutuals = getMutualFriends(state, alice);
    expect(mutuals.map((m) => m.peer_id)).toEqual(["peer_bob"]);
    expect(formatMutualSummary(mutuals)).toBe("1 mutual friend · Bob");
  });

  it("computes mutual friends for an arbitrary peer ID (e.g. incoming request)", () => {
    const alice = makeFriend("peer_alice", "Alice", [
      { peer_id: "peer_carol", display_name: "Carol" },
    ]);
    const bob = makeFriend("peer_bob", "Bob", [
      { peer_id: "peer_carol", display_name: "Carol" },
    ]);
    const state = makeState([bob, alice]);

    const mutuals = getMutualFriendsForPeerId(state, "peer_carol");
    expect(mutuals.map((m) => m.display_name)).toEqual(["Alice", "Bob"]);
    expect(formatMutualSummary(mutuals)).toBe("2 mutual friends · Alice, Bob");
  });

  it("aggregates, deduplicates, ranks discoverable players, and tracks relationship state", () => {
    const alice = makeFriend("peer_alice", "Alice", [
      { peer_id: "peer_carol", display_name: "Carol" },
      { peer_id: "peer_dave", display_name: "Dave" },
      { peer_id: "peer_eve", display_name: "Eve" },
      { peer_id: "peer_self", display_name: "Self" },
      { peer_id: "peer_bob", display_name: "Bob" },
    ]);
    const bob = makeFriend("peer_bob", "Bob", [
      { peer_id: "peer_carol", display_name: "Carol" },
      { peer_id: "peer_frank", display_name: "Frank" },
    ]);
    const outgoingDave = makeFriend("peer_dave", "Dave", null, "pending_outgoing");
    const state = makeState(
      [alice, bob, outgoingDave],
      [
        {
          peer_id: "peer_eve",
          display_name: "Eve",
          received_at: "2026-11-21T00:00:00Z",
        },
      ],
    );

    const players = getDiscoverablePlayers(state);
    // Self and connected Bob are excluded; Carol has 2 mutuals so ranks first.
    expect(players.map((p) => p.peer_id)).toEqual([
      "peer_carol",
      "peer_dave",
      "peer_eve",
      "peer_frank",
    ]);
    expect(players[0].mutual_friends.map((m) => m.display_name)).toEqual([
      "Alice",
      "Bob",
    ]);
    expect(players[0].relationship).toBe("none");
    expect(players[1].relationship).toBe("pending_outgoing");
    expect(players[2].relationship).toBe("pending_incoming");
    expect(players[3].relationship).toBe("none");
  });

  it("validates 64-char hex peer ID format", () => {
    expect(isPeerIdFormat("a".repeat(64))).toBe(true);
    expect(isPeerIdFormat(`  ${"0123456789abcdef".repeat(4)}  `)).toBe(true);
    expect(isPeerIdFormat("a".repeat(63))).toBe(false);
    expect(isPeerIdFormat("g".repeat(64))).toBe(false);
    expect(isPeerIdFormat("Alice")).toBe(false);
  });
});
