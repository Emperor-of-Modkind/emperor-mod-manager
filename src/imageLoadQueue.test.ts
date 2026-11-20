import { afterEach, describe, expect, it, vi } from "vitest";
import {
  MAX_CONCURRENT_CACHED_DECODES,
  MAX_CONCURRENT_IMAGE_LOADS,
  enqueueImageLoad,
  getImageQueueStats,
  isImageFailed,
  isImageLoaded,
  markImageFailed,
  markImageLoaded,
  resetImageQueueForTests,
} from "./imageLoadQueue";

describe("imageLoadQueue", () => {
  afterEach(() => {
    vi.useRealTimers();
    resetImageQueueForTests();
  });

  it("caps active concurrent image loads and pumps queued entries on release", () => {
    const granted: string[] = [];
    const releases: Array<() => void> = [];

    for (let i = 0; i < MAX_CONCURRENT_IMAGE_LOADS + 4; i++) {
      const url = `https://example.com/img-${i}.jpg`;
      releases.push(enqueueImageLoad(url, () => granted.push(url)));
    }

    expect(granted).toHaveLength(MAX_CONCURRENT_IMAGE_LOADS);
    expect(getImageQueueStats()).toEqual({
      active: MAX_CONCURRENT_IMAGE_LOADS,
      pending: 4,
      cachedActive: 0,
      cachedPending: 0,
    });

    releases[0]();
    expect(granted).toHaveLength(MAX_CONCURRENT_IMAGE_LOADS + 1);
    expect(granted[MAX_CONCURRENT_IMAGE_LOADS]).toBe(
      `https://example.com/img-${MAX_CONCURRENT_IMAGE_LOADS}.jpg`,
    );

    // Calling release twice is a no-op
    releases[0]();
    expect(granted).toHaveLength(MAX_CONCURRENT_IMAGE_LOADS + 1);
  });

  it("cancels queued entries before they ever acquire a slot", () => {
    const granted: string[] = [];
    const releases: Array<() => void> = [];

    for (let i = 0; i < MAX_CONCURRENT_IMAGE_LOADS + 2; i++) {
      const url = `https://example.com/img-${i}.jpg`;
      releases.push(enqueueImageLoad(url, () => granted.push(url)));
    }

    // Cancel the first queued entry (index MAX_CONCURRENT_IMAGE_LOADS)
    releases[MAX_CONCURRENT_IMAGE_LOADS]();
    expect(getImageQueueStats().pending).toBe(1);

    // Release an active slot; the cancelled one is skipped
    releases[0]();
    expect(granted).toContain(
      `https://example.com/img-${MAX_CONCURRENT_IMAGE_LOADS + 1}.jpg`,
    );
    expect(granted).not.toContain(
      `https://example.com/img-${MAX_CONCURRENT_IMAGE_LOADS}.jpg`,
    );
  });

  it("releases stalled active slots after timeout", () => {
    vi.useFakeTimers();
    const granted: string[] = [];

    for (let i = 0; i < MAX_CONCURRENT_IMAGE_LOADS + 1; i++) {
      const url = `https://example.com/stall-${i}.jpg`;
      enqueueImageLoad(url, () => granted.push(url), { timeoutMs: 500 });
    }

    expect(granted).toHaveLength(MAX_CONCURRENT_IMAGE_LOADS);
    vi.advanceTimersByTime(500);
    expect(granted).toHaveLength(MAX_CONCURRENT_IMAGE_LOADS + 1);
  });

  it("tracks loaded and failed URLs cleanly", () => {
    const url = "https://example.com/poster.jpg";
    expect(isImageLoaded(url)).toBe(false);
    expect(isImageFailed(url)).toBe(false);

    markImageLoaded(url);
    expect(isImageLoaded(url)).toBe(true);
    expect(isImageFailed(url)).toBe(false);

    markImageFailed(url);
    expect(isImageLoaded(url)).toBe(false);
    expect(isImageFailed(url)).toBe(true);
  });

  it("throttles cached image decodes separately with release-on-complete backpressure", () => {
    const granted: string[] = [];
    const releases: Array<() => void> = [];

    for (let i = 0; i < MAX_CONCURRENT_CACHED_DECODES + 3; i++) {
      const url = `https://example.com/cached-${i}.jpg`;
      releases.push(
        enqueueImageLoad(url, () => granted.push(url), { cached: true }),
      );
    }

    expect(granted).toHaveLength(MAX_CONCURRENT_CACHED_DECODES);
    expect(getImageQueueStats()).toEqual({
      active: 0,
      pending: 0,
      cachedActive: MAX_CONCURRENT_CACHED_DECODES,
      cachedPending: 3,
    });

    // Cancel a queued cached entry before it starts
    releases[MAX_CONCURRENT_CACHED_DECODES + 1]();
    expect(getImageQueueStats().cachedPending).toBe(2);

    // Releasing an active slot pumps the next non-cancelled cached entry
    releases[0]();
    expect(granted).toHaveLength(MAX_CONCURRENT_CACHED_DECODES + 1);
    expect(granted).toContain(
      `https://example.com/cached-${MAX_CONCURRENT_CACHED_DECODES}.jpg`,
    );

    releases[1]();
    expect(granted).toContain(
      `https://example.com/cached-${MAX_CONCURRENT_CACHED_DECODES + 2}.jpg`,
    );
    expect(granted).not.toContain(
      `https://example.com/cached-${MAX_CONCURRENT_CACHED_DECODES + 1}.jpg`,
    );
  });
});
