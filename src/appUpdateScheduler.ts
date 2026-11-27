export const APP_UPDATE_POLL_INTERVAL_MS = 30 * 60_000;
export const APP_UPDATE_STALE_ON_FOCUS_MS = 10 * 60_000;
export const APP_UPDATE_RETRY_DELAYS_MS = [
  15_000,
  30_000,
  60_000,
  120_000,
  300_000,
] as const;
export const APP_UPDATE_MIN_FOCUS_RETRY_GAP_MS = 30_000;

export type AppUpdateSchedulerOptions = {
  check: () => Promise<boolean>;
  now?: () => number;
  pollIntervalMs?: number;
  staleOnFocusMs?: number;
  retryDelaysMs?: readonly number[];
  minFocusRetryGapMs?: number;
};

export type AppUpdateScheduler = {
  start: () => void;
  stop: () => void;
  triggerNow: () => Promise<boolean>;
  recordExternalSuccess: () => void;
};

/**
 * Schedules quiet app-update checks across startup, transient offline/API failures,
 * long-running/tray sessions, network reconnects, and window focus/restore.
 */
export function createAppUpdateScheduler(
  options: AppUpdateSchedulerOptions,
): AppUpdateScheduler {
  const now = options.now ?? (() => Date.now());
  const pollIntervalMs = options.pollIntervalMs ?? APP_UPDATE_POLL_INTERVAL_MS;
  const staleOnFocusMs = options.staleOnFocusMs ?? APP_UPDATE_STALE_ON_FOCUS_MS;
  const retryDelaysMs = options.retryDelaysMs ?? APP_UPDATE_RETRY_DELAYS_MS;
  const minFocusRetryGapMs =
    options.minFocusRetryGapMs ?? APP_UPDATE_MIN_FOCUS_RETRY_GAP_MS;

  let started = false;
  let disposed = false;
  let timerId: number | null = null;
  let inFlight: Promise<boolean> | null = null;
  let lastAttemptAt: number | null = null;
  let lastSuccessAt: number | null = null;
  let consecutiveFailures = 0;

  function clearTimer() {
    if (timerId != null) {
      window.clearTimeout(timerId);
      timerId = null;
    }
  }

  function scheduleNext(delayMs: number) {
    if (disposed || !started) return;
    clearTimer();
    timerId = window.setTimeout(() => {
      timerId = null;
      void triggerNow();
    }, delayMs);
  }

  function nextRetryDelayMs(failureCount: number): number {
    if (retryDelaysMs.length === 0) return pollIntervalMs;
    const idx = Math.min(Math.max(0, failureCount - 1), retryDelaysMs.length - 1);
    return retryDelaysMs[idx];
  }

  function isStale(): boolean {
    if (lastSuccessAt == null) return true;
    return now() - lastSuccessAt >= staleOnFocusMs;
  }

  function triggerNow(): Promise<boolean> {
    if (disposed) return Promise.resolve(false);
    if (inFlight) return inFlight;

    clearTimer();
    lastAttemptAt = now();

    let checkPromise: Promise<boolean>;
    try {
      checkPromise = Promise.resolve(options.check());
    } catch {
      checkPromise = Promise.resolve(false);
    }

    const run = checkPromise
      .catch(() => false)
      .then((ok) => {
        if (disposed) return ok;
        if (ok) {
          consecutiveFailures = 0;
          lastSuccessAt = now();
          scheduleNext(pollIntervalMs);
        } else {
          consecutiveFailures += 1;
          scheduleNext(nextRetryDelayMs(consecutiveFailures));
        }
        return ok;
      })
      .finally(() => {
        if (inFlight === run) {
          inFlight = null;
        }
      });

    inFlight = run;
    return run;
  }

  function recordExternalSuccess() {
    if (disposed) return;
    consecutiveFailures = 0;
    lastSuccessAt = now();
    if (started && !inFlight) {
      scheduleNext(pollIntervalMs);
    }
  }

  function onOnline() {
    if (disposed || !started) return;
    if (consecutiveFailures > 0 || isStale()) {
      void triggerNow();
    }
  }

  function onResume() {
    if (disposed || !started) return;
    if (consecutiveFailures > 0) {
      if (lastAttemptAt == null || now() - lastAttemptAt >= minFocusRetryGapMs) {
        void triggerNow();
      }
      return;
    }
    if (isStale()) {
      void triggerNow();
    }
  }

  function onVisibilityChange() {
    if (document.visibilityState === "visible") {
      onResume();
    }
  }

  function start() {
    if (disposed || started) return;
    started = true;
    window.addEventListener("online", onOnline);
    window.addEventListener("focus", onResume);
    document.addEventListener("visibilitychange", onVisibilityChange);
    void triggerNow();
  }

  function stop() {
    if (disposed) return;
    disposed = true;
    started = false;
    clearTimer();
    window.removeEventListener("online", onOnline);
    window.removeEventListener("focus", onResume);
    document.removeEventListener("visibilitychange", onVisibilityChange);
  }

  return {
    start,
    stop,
    triggerNow,
    recordExternalSuccess,
  };
}
