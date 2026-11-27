import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  APP_UPDATE_MIN_FOCUS_RETRY_GAP_MS,
  APP_UPDATE_POLL_INTERVAL_MS,
  APP_UPDATE_RETRY_DELAYS_MS,
  APP_UPDATE_STALE_ON_FOCUS_MS,
  createAppUpdateScheduler,
} from "./appUpdateScheduler";

describe("createAppUpdateScheduler", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-11-27T00:00:00Z"));
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("checks immediately on start and polls periodically after success", async () => {
    const check = vi.fn<() => Promise<boolean>>().mockResolvedValue(true);
    const scheduler = createAppUpdateScheduler({ check });

    scheduler.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(APP_UPDATE_POLL_INTERVAL_MS - 1);
    expect(check).toHaveBeenCalledTimes(1);

    await vi.advanceTimersByTimeAsync(1);
    expect(check).toHaveBeenCalledTimes(2);

    scheduler.stop();
  });

  it("retries with exponential backoff after transient failures and resets on success", async () => {
    const check = vi
      .fn<() => Promise<boolean>>()
      .mockResolvedValueOnce(false)
      .mockRejectedValueOnce(new Error("offline"))
      .mockResolvedValueOnce(true)
      .mockResolvedValue(true);

    const scheduler = createAppUpdateScheduler({ check });
    scheduler.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(1);

    // First retry after 15s
    await vi.advanceTimersByTimeAsync(APP_UPDATE_RETRY_DELAYS_MS[0] - 1);
    expect(check).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(1);
    expect(check).toHaveBeenCalledTimes(2);

    // Second retry after 30s (this one succeeds)
    await vi.advanceTimersByTimeAsync(APP_UPDATE_RETRY_DELAYS_MS[1] - 1);
    expect(check).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1);
    expect(check).toHaveBeenCalledTimes(3);

    // Back to 30m periodic cadence after success
    await vi.advanceTimersByTimeAsync(APP_UPDATE_RETRY_DELAYS_MS[2]);
    expect(check).toHaveBeenCalledTimes(3);
    await vi.advanceTimersByTimeAsync(
      APP_UPDATE_POLL_INTERVAL_MS - APP_UPDATE_RETRY_DELAYS_MS[2],
    );
    expect(check).toHaveBeenCalledTimes(4);

    scheduler.stop();
  });

  it("retries immediately on window online event after a failed startup check", async () => {
    const check = vi
      .fn<() => Promise<boolean>>()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);

    const scheduler = createAppUpdateScheduler({ check });
    scheduler.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(1);

    // Online event fires 2s later — should not wait for the 15s backoff timer
    await vi.advanceTimersByTimeAsync(2_000);
    window.dispatchEvent(new Event("online"));
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(2);

    scheduler.stop();
  });

  it("re-checks on window focus and visibilitychange when stale, and skips when fresh", async () => {
    const check = vi.fn<() => Promise<boolean>>().mockResolvedValue(true);
    const scheduler = createAppUpdateScheduler({ check });

    scheduler.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(1);

    // Focus while fresh (< 10 min) should skip
    await vi.advanceTimersByTimeAsync(APP_UPDATE_STALE_ON_FOCUS_MS - 1_000);
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(1);

    // Focus once stale (>= 10 min) should re-check
    await vi.advanceTimersByTimeAsync(1_000);
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(2);

    // Visibility restore (e.g. tray restore) once stale should also re-check
    await vi.advanceTimersByTimeAsync(APP_UPDATE_STALE_ON_FOCUS_MS);
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "visible",
    });
    document.dispatchEvent(new Event("visibilitychange"));
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(3);

    scheduler.stop();
  });

  it("respects the focus retry cooldown after a failure", async () => {
    const check = vi
      .fn<() => Promise<boolean>>()
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(false)
      .mockResolvedValueOnce(true);

    const scheduler = createAppUpdateScheduler({
      check,
      // Use longer backoff so focus cooldown is exercised independently
      retryDelaysMs: [120_000],
    });
    scheduler.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(1);

    // Focus too soon (< 30s) should not spam
    await vi.advanceTimersByTimeAsync(APP_UPDATE_MIN_FOCUS_RETRY_GAP_MS - 1_000);
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(1);

    // Focus after 30s should retry
    await vi.advanceTimersByTimeAsync(1_000);
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(2);

    scheduler.stop();
  });

  it("deduplicates overlapping checks", async () => {
    let resolveFirst!: (ok: boolean) => void;
    const check = vi.fn<() => Promise<boolean>>(
      () =>
        new Promise<boolean>((resolve) => {
          resolveFirst = resolve;
        }),
    );

    const scheduler = createAppUpdateScheduler({ check });
    scheduler.start();
    const p1 = scheduler.triggerNow();
    const p2 = scheduler.triggerNow();
    expect(check).toHaveBeenCalledTimes(1);

    resolveFirst(true);
    await expect(Promise.all([p1, p2])).resolves.toEqual([true, true]);

    scheduler.stop();
  });

  it("resets backoff when recordExternalSuccess is called", async () => {
    const check = vi
      .fn<() => Promise<boolean>>()
      .mockResolvedValueOnce(false)
      .mockResolvedValue(true);

    const scheduler = createAppUpdateScheduler({ check });
    scheduler.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(1);

    // Manual check in Settings succeeded
    scheduler.recordExternalSuccess();

    // Old 15s retry timer must not fire
    await vi.advanceTimersByTimeAsync(APP_UPDATE_RETRY_DELAYS_MS[0] + 1_000);
    expect(check).toHaveBeenCalledTimes(1);

    // Full 30m poll timer fires instead
    await vi.advanceTimersByTimeAsync(
      APP_UPDATE_POLL_INTERVAL_MS - (APP_UPDATE_RETRY_DELAYS_MS[0] + 1_000),
    );
    expect(check).toHaveBeenCalledTimes(2);

    scheduler.stop();
  });

  it("cleans up timers and listeners on stop", async () => {
    const check = vi.fn<() => Promise<boolean>>().mockResolvedValue(false);
    const scheduler = createAppUpdateScheduler({ check });

    scheduler.start();
    await vi.advanceTimersByTimeAsync(0);
    expect(check).toHaveBeenCalledTimes(1);

    scheduler.stop();

    window.dispatchEvent(new Event("online"));
    window.dispatchEvent(new Event("focus"));
    await vi.advanceTimersByTimeAsync(APP_UPDATE_POLL_INTERVAL_MS * 2);
    expect(check).toHaveBeenCalledTimes(1);
  });
});
