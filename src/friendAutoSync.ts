import type {
  FriendCollectionSyncStatus,
  FriendSyncResult,
  InstalledCollection,
  StagedMod,
} from "./types";

export const TRANSIENT_RETRY_BACKOFF_MS = 60_000;
export const PRE_LAUNCH_FRIEND_REFRESH_TIMEOUT_MS = 4_000;
export const GAME_VIEW_FRIEND_REFRESH_TTL_MS = 30_000;

export type AutoSyncSummary = {
  syncedNames: string[];
  pendingManualNames: string[];
  pendingManualIds: string[];
  conflictNames: string[];
  conflictIds: string[];
  assistNames: string[];
  assistIds: string[];
  failedNames: string[];
  failedIds: string[];
};

export function createEmptyAutoSyncSummary(): AutoSyncSummary {
  return {
    syncedNames: [],
    pendingManualNames: [],
    pendingManualIds: [],
    conflictNames: [],
    conflictIds: [],
    assistNames: [],
    assistIds: [],
    failedNames: [],
    failedIds: [],
  };
}

export type AutoSyncBlockReason = "conflict" | "needs_assist";

export class AutoSyncTracker {
  private readonly backoffMs: number;
  private readonly inFlight = new Map<string, Promise<void>>();
  private readonly rerunRequested = new Set<string>();
  private readonly blockedHashes = new Map<
    string,
    { hash: string; reason: AutoSyncBlockReason }
  >();
  private readonly transientFailures = new Map<
    string,
    { hash: string; failedAt: number }
  >();

  constructor(backoffMs = TRANSIENT_RETRY_BACKOFF_MS) {
    this.backoffMs = backoffMs;
  }

  isInFlight(key: string): boolean {
    return this.inFlight.has(key);
  }

  requestRerun(key: string): void {
    this.rerunRequested.add(key);
  }

  consumeRerun(key: string): boolean {
    return this.rerunRequested.delete(key);
  }

  registerInFlight(key: string, promise: Promise<void>): void {
    this.inFlight.set(key, promise);
  }

  clearInFlight(key: string): void {
    this.inFlight.delete(key);
  }

  async waitForGame(gameId: string): Promise<void> {
    const prefix = `${gameId}:`;
    const pending: Promise<void>[] = [];
    for (const [key, promise] of this.inFlight.entries()) {
      if (key.startsWith(prefix)) {
        pending.push(promise);
      }
    }
    if (pending.length > 0) {
      await Promise.allSettled(pending);
    }
  }

  shouldSkip(
    key: string,
    latestHash: string,
    opts?: { forceRetry?: boolean; now?: number },
  ): boolean {
    if (opts?.forceRetry) return false;
    const blocked = this.blockedHashes.get(key);
    if (blocked && blocked.hash === latestHash) {
      return true;
    }
    const failed = this.transientFailures.get(key);
    if (failed && failed.hash === latestHash) {
      const now = opts?.now ?? Date.now();
      if (now - failed.failedAt < this.backoffMs) {
        return true;
      }
    }
    return false;
  }

  shouldNotify(
    key: string,
    latestHash: string,
    opts?: { forceRetry?: boolean },
  ): boolean {
    if (opts?.forceRetry) return true;
    const blocked = this.blockedHashes.get(key);
    if (blocked && blocked.hash === latestHash) return false;
    const failed = this.transientFailures.get(key);
    if (failed && failed.hash === latestHash) return false;
    return true;
  }

  recordSuccess(key: string): void {
    this.blockedHashes.delete(key);
    this.transientFailures.delete(key);
  }

  recordBlocked(
    key: string,
    latestHash: string,
    reason: AutoSyncBlockReason,
  ): void {
    this.transientFailures.delete(key);
    this.blockedHashes.set(key, { hash: latestHash, reason });
  }

  recordTransientFailure(
    key: string,
    latestHash: string,
    now = Date.now(),
  ): void {
    this.blockedHashes.delete(key);
    this.transientFailures.set(key, { hash: latestHash, failedAt: now });
  }

  clearKey(key: string): void {
    this.blockedHashes.delete(key);
    this.transientFailures.delete(key);
    this.rerunRequested.delete(key);
  }
}

export function isSyncResultComplete(
  res: Pick<FriendSyncResult, "share_hash" | "needs_assist" | "collection">,
): boolean {
  if (res.needs_assist.length > 0) return false;
  const recordedHash = res.collection?.friend_sync?.last_synced_hash;
  if (recordedHash != null) {
    return recordedHash === res.share_hash;
  }
  return true;
}

export type OutdatedSyncedCollectionReason =
  | "manual_only"
  | "conflict"
  | "needs_assist"
  | "failed"
  | "pending";

export type OutdatedSyncedCollection = {
  collection: InstalledCollection;
  status: FriendCollectionSyncStatus;
  reason: OutdatedSyncedCollectionReason;
};

export function isCollectionEnabledForLaunch(
  col: InstalledCollection,
  mods: ReadonlyArray<Pick<StagedMod, "id" | "enabled" | "collection_ids">>,
): boolean {
  if (col.disabled) return false;
  const members = mods.filter(
    (m) =>
      (m.collection_ids ?? []).includes(col.id) || col.mod_ids.includes(m.id),
  );
  if (members.length > 0 && members.every((m) => !m.enabled)) {
    return false;
  }
  return true;
}

export function classifyOutdatedEnabledCollections(
  collections: ReadonlyArray<InstalledCollection>,
  syncStates: ReadonlyArray<FriendCollectionSyncStatus>,
  mods: ReadonlyArray<Pick<StagedMod, "id" | "enabled" | "collection_ids">>,
  summary?: AutoSyncSummary | null,
): OutdatedSyncedCollection[] {
  const statusById = new Map<string, FriendCollectionSyncStatus>();
  for (const st of syncStates) {
    statusById.set(st.collection_id, st);
  }
  const outdated: OutdatedSyncedCollection[] = [];
  for (const col of collections) {
    if (!col.friend_sync) continue;
    if (!isCollectionEnabledForLaunch(col, mods)) continue;
    const status = statusById.get(col.id);
    if (!status || !status.has_update) continue;

    let reason: OutdatedSyncedCollectionReason = "pending";
    if (!status.auto_sync) {
      reason = "manual_only";
    } else if (summary?.conflictIds.includes(col.id)) {
      reason = "conflict";
    } else if (summary?.assistIds.includes(col.id)) {
      reason = "needs_assist";
    } else if (summary?.failedIds.includes(col.id)) {
      reason = "failed";
    }
    outdated.push({ collection: col, status, reason });
  }
  return outdated;
}

export function outdatedReasonLabel(
  reason: OutdatedSyncedCollectionReason,
): string {
  switch (reason) {
    case "manual_only":
      return "Auto-sync is turned off for this pack.";
    case "conflict":
      return "Update has version conflicts that need your choice.";
    case "needs_assist":
      return "Update requires Nexus download assist.";
    case "failed":
      return "Automatic sync failed and needs a retry.";
    case "pending":
      return "An update from your friend is waiting to be synced.";
  }
}

export async function raceWithTimeout<T>(
  promise: Promise<T>,
  timeoutMs: number,
): Promise<T | null> {
  let timer: ReturnType<typeof setTimeout> | null = null;
  try {
    return await Promise.race([
      promise,
      new Promise<null>((resolve) => {
        timer = setTimeout(() => resolve(null), timeoutMs);
      }),
    ]);
  } finally {
    if (timer != null) clearTimeout(timer);
  }
}
