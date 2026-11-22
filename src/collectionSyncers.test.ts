import { describe, expect, it } from "vitest";
import {
  findMatchingSavedCollectionId,
  formatActiveLoadoutMatchLabel,
  formatSyncersSummary,
  getCollectionsSyncedByFriend,
  getFriendActiveLoadoutMatches,
  getFriendsSyncingCollection,
  getFriendsWithActiveLoadoutMatching,
} from "./collectionSyncers";
import type {
  FriendRecord,
  LocalShareableCollection,
  SavedCollectionEntry,
} from "./types";

function makeFriend(
  peer_id: string,
  display_name: string,
  syncing_my_collections: string[] = [],
  status: FriendRecord["status"] = "connected",
): FriendRecord {
  return {
    peer_id,
    display_name,
    status,
    online: true,
    syncing_my_collections,
  };
}

describe("collectionSyncers helpers", () => {
  it("filters connected friends syncing a given collection ID", () => {
    const friends = [
      makeFriend("p1", "Alice", ["saved:c1", "active:skyrim"]),
      makeFriend("p2", "Bob", ["saved:c2"]),
      makeFriend("p3", "Carol", ["saved:c1"], "pending_outgoing"),
    ];
    expect(getFriendsSyncingCollection(friends, "saved:c1").map((f) => f.peer_id)).toEqual([
      "p1",
    ]);
    expect(getFriendsSyncingCollection(friends, "active:skyrim").map((f) => f.peer_id)).toEqual([
      "p1",
    ]);
    expect(getFriendsSyncingCollection(friends, "saved:missing")).toEqual([]);
  });

  it("matches installed collection to saved collection by share_id or profile_code", () => {
    const saved: SavedCollectionEntry[] = [
      {
        id: "s1",
        name: "Pack One",
        code: "#emperor1abc",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        game: {},
        mod_count: 2,
        nexus_count: 2,
        thunderstore_count: 0,
        modio_count: 0,
        share_id: "emperor-share:hash1",
      },
      {
        id: "s2",
        name: "Pack Two",
        code: "#emperor1xyz",
        created_at: "2026-01-01T00:00:00Z",
        updated_at: "2026-01-01T00:00:00Z",
        game: {},
        mod_count: 1,
        nexus_count: 1,
        thunderstore_count: 0,
        modio_count: 0,
      },
    ];

    expect(
      findMatchingSavedCollectionId({ id: "emperor-share:hash1" }, saved),
    ).toBe("saved:s1");
    expect(
      findMatchingSavedCollectionId(
        { id: "other", profile_code: " #emperor1xyz " },
        saved,
      ),
    ).toBe("saved:s2");
    expect(
      findMatchingSavedCollectionId({ id: "unmatched" }, saved),
    ).toBeNull();
  });

  it("resolves local collection names for a friend's synced IDs", () => {
    const localCols: LocalShareableCollection[] = [
      {
        id: "active:skyrim",
        kind: "active_loadout",
        name: "Skyrim Loadout",
        game_title: "Skyrim",
        mod_count: 10,
        hidden: false,
      },
    ];
    const friend = makeFriend("p1", "Alice", ["active:skyrim", "saved:unknown"]);
    expect(getCollectionsSyncedByFriend(friend, localCols)).toEqual([
      { id: "active:skyrim", name: "Skyrim Loadout" },
      { id: "saved:unknown", name: "saved:unknown" },
    ]);
  });

  it("formats syncers summary cleanly", () => {
    expect(formatSyncersSummary([])).toBe("");
    expect(formatSyncersSummary([makeFriend("p1", "Alice")])).toBe("Synced by Alice");
    expect(
      formatSyncersSummary([makeFriend("p1", "Alice"), makeFriend("p2", "Bob")]),
    ).toBe("Synced by Alice, Bob");
    expect(
      formatSyncersSummary([
        makeFriend("p1", "Alice"),
        makeFriend("p2", "Bob"),
        makeFriend("p3", "Carol"),
        makeFriend("p4", "Dave"),
      ]),
    ).toBe("Synced by Alice, Bob +2");
  });

  it("extracts and formats active loadout matches and filters matching friends", () => {
    const f1: FriendRecord = {
      ...makeFriend("p1", "Alice"),
      cached_profile: {
        peer_id: "p1",
        display_name: "Alice",
        updated_at: "2026-11-21T00:00:00Z",
        collections: [
          {
            id: "active:skyrim",
            kind: "active_loadout",
            name: "Skyrim Loadout",
            share_code: "#emperor1a",
            share_hash: "ha",
            updated_at: "2026-11-21T00:00:00Z",
            matched_local_collection: {
              id: "saved:coop",
              name: "Co-op Pack",
              kind: "synced_to_yours",
              has_local_changes: true,
            },
          },
        ],
      },
    };
    const f2: FriendRecord = {
      ...makeFriend("p2", "Bob"),
      cached_profile: {
        peer_id: "p2",
        display_name: "Bob",
        updated_at: "2026-11-21T00:00:00Z",
        collections: [
          {
            id: "active:skyrim",
            kind: "active_loadout",
            name: "Skyrim Loadout",
            share_code: "#emperor1b",
            share_hash: "hb",
            updated_at: "2026-11-21T00:00:00Z",
            matched_local_collection: {
              id: "active:skyrim",
              name: "Skyrim Loadout",
              kind: "matches_active_loadout",
            },
          },
        ],
      },
    };

    const matches = getFriendActiveLoadoutMatches(f1);
    expect(matches).toHaveLength(1);
    expect(formatActiveLoadoutMatchLabel(matches[0].match)).toBe(
      'Synced to your "Co-op Pack" · Update pending',
    );
    expect(
      formatActiveLoadoutMatchLabel(
        getFriendActiveLoadoutMatches(f2)[0].match,
      ),
    ).toBe("Matches your active loadout");

    expect(
      getFriendsWithActiveLoadoutMatching([f1, f2], "saved:coop").map(
        (f) => f.peer_id,
      ),
    ).toEqual(["p1"]);
    expect(
      getFriendsWithActiveLoadoutMatching([f1, f2], "active:skyrim").map(
        (f) => f.peer_id,
      ),
    ).toEqual(["p2"]);
  });
});
