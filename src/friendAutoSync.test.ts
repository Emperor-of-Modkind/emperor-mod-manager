import { describe, expect, it } from "vitest";
import {
  AutoSyncTracker,
  classifyOutdatedEnabledCollections,
  createEmptyAutoSyncSummary,
  isCollectionEnabledForLaunch,
  isSyncResultComplete,
  raceWithTimeout,
} from "./friendAutoSync";
import type {
  FriendCollectionSyncStatus,
  InstalledCollection,
} from "./types";

function makeCollection(
  overrides: Partial<InstalledCollection> = {},
): InstalledCollection {
  return {
    id: "friend:peer1:saved:c1",
    source: "emperor",
    kind: "share",
    name: "Squad Pack",
    mod_ids: ["m1"],
    installed_at: "2026-11-26T00:00:00Z",
    friend_sync: {
      friend_peer_id: "peer1",
      friend_collection_id: "saved:c1",
      last_synced_hash: "h1",
      last_synced_at: "2026-11-26T00:00:00Z",
      auto_sync: true,
    },
    ...overrides,
  };
}

function makeStatus(
  overrides: Partial<FriendCollectionSyncStatus> = {},
): FriendCollectionSyncStatus {
  return {
    collection_id: "friend:peer1:saved:c1",
    peer_id: "peer1",
    friend_collection_id: "saved:c1",
    latest_hash: "h2",
    has_update: true,
    auto_sync: true,
    friend_online: true,
    ...overrides,
  };
}

describe("AutoSyncTracker", () => {
  it("backs off transient failures and retries after backoff or forceRetry", () => {
    const tracker = new AutoSyncTracker(60_000);
    const key = "game1:col1";

    expect(tracker.shouldSkip(key, "h2", { now: 1_000 })).toBe(false);
    expect(tracker.shouldNotify(key, "h2")).toBe(true);

    tracker.recordTransientFailure(key, "h2", 1_000);

    // Within backoff window -> skip and do not re-notify
    expect(tracker.shouldSkip(key, "h2", { now: 30_000 })).toBe(true);
    expect(tracker.shouldNotify(key, "h2")).toBe(false);

    // forceRetry overrides backoff immediately
    expect(tracker.shouldSkip(key, "h2", { forceRetry: true, now: 30_000 })).toBe(false);
    expect(tracker.shouldNotify(key, "h2", { forceRetry: true })).toBe(true);

    // After backoff window expires -> retries automatically
    expect(tracker.shouldSkip(key, "h2", { now: 61_000 })).toBe(false);

    // Newer hash retries immediately even within backoff
    expect(tracker.shouldSkip(key, "h3", { now: 2_000 })).toBe(false);
  });

  it("keeps blocked hashes (conflicts / assist) suppressed until hash changes or forceRetry", () => {
    const tracker = new AutoSyncTracker(60_000);
    const key = "game1:col1";

    tracker.recordBlocked(key, "h2", "conflict");

    // Still skipped even long after backoff
    expect(tracker.shouldSkip(key, "h2", { now: 1_000_000 })).toBe(true);
    expect(tracker.shouldNotify(key, "h2")).toBe(false);

    // forceRetry or new hash unblocks
    expect(tracker.shouldSkip(key, "h2", { forceRetry: true })).toBe(false);
    expect(tracker.shouldSkip(key, "h3", { now: 1_000_000 })).toBe(false);

    tracker.recordSuccess(key);
    expect(tracker.shouldSkip(key, "h2", { now: 1_000_000 })).toBe(false);
  });

  it("tracks in-flight syncs, coalesces reruns, and waits for a game's active syncs", async () => {
    const tracker = new AutoSyncTracker();
    const key = "game1:col1";
    let resolveSync!: () => void;
    const p = new Promise<void>((resolve) => {
      resolveSync = resolve;
    });

    tracker.registerInFlight(key, p);
    expect(tracker.isInFlight(key)).toBe(true);

    // A second trigger arrives mid-sync
    tracker.requestRerun(key);

    let waited = false;
    const waiter = tracker.waitForGame("game1").then(() => {
      waited = true;
    });
    expect(waited).toBe(false);

    resolveSync();
    await waiter;
    expect(waited).toBe(true);

    tracker.clearInFlight(key);
    expect(tracker.isInFlight(key)).toBe(false);
    expect(tracker.consumeRerun(key)).toBe(true);
    expect(tracker.consumeRerun(key)).toBe(false);
  });
});

describe("isSyncResultComplete", () => {
  it("returns false when needs_assist is non-empty", () => {
    expect(
      isSyncResultComplete({
        share_hash: "h2",
        needs_assist: [
          { domain: "skyrim", mod_id: 1, file_id: 2, name: "Mod" },
        ],
        collection: null,
      }),
    ).toBe(false);
  });

  it("returns false when backend reverted last_synced_hash due to partial sync warnings", () => {
    expect(
      isSyncResultComplete({
        share_hash: "h2",
        needs_assist: [],
        collection: makeCollection({
          friend_sync: {
            friend_peer_id: "peer1",
            friend_collection_id: "saved:c1",
            last_synced_hash: "h1",
            last_synced_at: "2026-11-26T00:00:00Z",
            auto_sync: true,
          },
        }),
      }),
    ).toBe(false);
  });

  it("returns true when needs_assist is empty and last_synced_hash matches share_hash", () => {
    expect(
      isSyncResultComplete({
        share_hash: "h2",
        needs_assist: [],
        collection: makeCollection({
          friend_sync: {
            friend_peer_id: "peer1",
            friend_collection_id: "saved:c1",
            last_synced_hash: "h2",
            last_synced_at: "2026-11-26T00:00:00Z",
            auto_sync: true,
          },
        }),
      }),
    ).toBe(true);
  });
});

describe("classifyOutdatedEnabledCollections", () => {
  it("ignores disabled collections and collections with all member mods disabled", () => {
    const colDisabled = makeCollection({ id: "c_disabled", disabled: true });
    const colAllModsOff = makeCollection({
      id: "c_off",
      mod_ids: ["m_off"],
    });
    const colActive = makeCollection({ id: "c_active", mod_ids: ["m_on"] });

    const mods = [
      { id: "m_off", enabled: false, collection_ids: ["c_off"] },
      { id: "m_on", enabled: true, collection_ids: ["c_active"] },
    ];

    expect(isCollectionEnabledForLaunch(colDisabled, mods)).toBe(false);
    expect(isCollectionEnabledForLaunch(colAllModsOff, mods)).toBe(false);
    expect(isCollectionEnabledForLaunch(colActive, mods)).toBe(true);

    const states = [
      makeStatus({ collection_id: "c_disabled", has_update: true }),
      makeStatus({ collection_id: "c_off", has_update: true }),
      makeStatus({ collection_id: "c_active", has_update: true, auto_sync: false }),
    ];

    const res = classifyOutdatedEnabledCollections(
      [colDisabled, colAllModsOff, colActive],
      states,
      mods,
    );
    expect(res).toHaveLength(1);
    expect(res[0].collection.id).toBe("c_active");
    expect(res[0].reason).toBe("manual_only");
  });

  it("classifies conflict, assist, failed, and pending reasons from AutoSyncSummary", () => {
    const c1 = makeCollection({ id: "c1", mod_ids: ["m1"] });
    const c2 = makeCollection({ id: "c2", mod_ids: ["m2"] });
    const c3 = makeCollection({ id: "c3", mod_ids: ["m3"] });
    const c4 = makeCollection({ id: "c4", mod_ids: ["m4"] });
    const mods = [
      { id: "m1", enabled: true, collection_ids: ["c1"] },
      { id: "m2", enabled: true, collection_ids: ["c2"] },
      { id: "m3", enabled: true, collection_ids: ["c3"] },
      { id: "m4", enabled: true, collection_ids: ["c4"] },
    ];
    const states = [
      makeStatus({ collection_id: "c1" }),
      makeStatus({ collection_id: "c2" }),
      makeStatus({ collection_id: "c3" }),
      makeStatus({ collection_id: "c4" }),
    ];
    const summary = createEmptyAutoSyncSummary();
    summary.conflictIds.push("c1");
    summary.assistIds.push("c2");
    summary.failedIds.push("c3");

    const res = classifyOutdatedEnabledCollections(
      [c1, c2, c3, c4],
      states,
      mods,
      summary,
    );
    expect(res.map((r) => [r.collection.id, r.reason])).toEqual([
      ["c1", "conflict"],
      ["c2", "needs_assist"],
      ["c3", "failed"],
      ["c4", "pending"],
    ]);
  });
});

describe("raceWithTimeout", () => {
  it("resolves with value before timeout and returns null on timeout", async () => {
    const fast = await raceWithTimeout(Promise.resolve("ok"), 100);
    expect(fast).toBe("ok");

    const slow = await raceWithTimeout(
      new Promise<string>((resolve) => setTimeout(() => resolve("late"), 200)),
      15,
    );
    expect(slow).toBeNull();
  });
});
