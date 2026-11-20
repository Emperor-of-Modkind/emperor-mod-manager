import { useCallback, useEffect, useRef, useState } from "react";

export const MAX_CONCURRENT_IMAGE_LOADS = 6;
export const MAX_CONCURRENT_CACHED_DECODES = 8;
export const IMAGE_LOAD_TIMEOUT_MS = 15_000;
const MAX_CACHED_IMAGE_URLS = 1000;

const loadedUrls = new Set<string>();
const failedUrls = new Set<string>();

type QueueEntry = {
  url: string;
  onGrant: () => void;
  timeoutMs: number;
  cancelled: boolean;
  active: boolean;
  released: boolean;
  timer: ReturnType<typeof setTimeout> | null;
};

type QueuePool = {
  maxConcurrent: number;
  activeCount: number;
  pending: QueueEntry[];
};

const networkPool: QueuePool = {
  maxConcurrent: MAX_CONCURRENT_IMAGE_LOADS,
  activeCount: 0,
  pending: [],
};

const cachedDecodePool: QueuePool = {
  maxConcurrent: MAX_CONCURRENT_CACHED_DECODES,
  activeCount: 0,
  pending: [],
};

function addBounded(cache: Set<string>, url: string) {
  if (cache.has(url)) {
    cache.delete(url);
  } else if (cache.size >= MAX_CACHED_IMAGE_URLS) {
    const oldest = cache.values().next().value;
    if (oldest !== undefined) cache.delete(oldest);
  }
  cache.add(url);
}

export function isImageLoaded(url: string): boolean {
  return loadedUrls.has(url);
}

export function isImageFailed(url: string): boolean {
  return failedUrls.has(url);
}

export function markImageLoaded(url: string): void {
  failedUrls.delete(url);
  addBounded(loadedUrls, url);
}

export function markImageFailed(url: string): void {
  loadedUrls.delete(url);
  addBounded(failedUrls, url);
}

function clearEntryTimer(entry: QueueEntry) {
  if (entry.timer !== null) {
    clearTimeout(entry.timer);
    entry.timer = null;
  }
}

function releaseFromPool(pool: QueuePool, entry: QueueEntry) {
  if (entry.released) return;
  entry.released = true;
  entry.cancelled = true;
  clearEntryTimer(entry);
  if (entry.active) {
    entry.active = false;
    pool.activeCount = Math.max(0, pool.activeCount - 1);
    pumpPool(pool);
  }
}

function activateInPool(pool: QueuePool, entry: QueueEntry) {
  entry.active = true;
  pool.activeCount += 1;
  if (entry.timeoutMs > 0) {
    entry.timer = setTimeout(() => {
      releaseFromPool(pool, entry);
    }, entry.timeoutMs);
  }
  entry.onGrant();
}

function pumpPool(pool: QueuePool) {
  while (pool.activeCount < pool.maxConcurrent && pool.pending.length > 0) {
    const next = pool.pending.shift();
    if (!next || next.cancelled || next.released) continue;
    activateInPool(pool, next);
  }
}

function enqueueInPool(
  pool: QueuePool,
  url: string,
  onGrant: () => void,
  options?: { timeoutMs?: number },
): () => void {
  const entry: QueueEntry = {
    url,
    onGrant,
    timeoutMs: options?.timeoutMs ?? IMAGE_LOAD_TIMEOUT_MS,
    cancelled: false,
    active: false,
    released: false,
    timer: null,
  };
  if (pool.activeCount < pool.maxConcurrent) {
    activateInPool(pool, entry);
  } else {
    pool.pending.push(entry);
  }
  return () => releaseFromPool(pool, entry);
}

/**
 * Acquires a concurrency slot for loading an image URL (network or cached decode).
 * Returns a release/cancel function that frees the slot (or removes the entry
 * from the pending queue if it has not started yet).
 */
export function enqueueImageLoad(
  url: string,
  onGrant: () => void,
  options?: { timeoutMs?: number; cached?: boolean },
): () => void {
  const pool = options?.cached ? cachedDecodePool : networkPool;
  return enqueueInPool(pool, url, onGrant, options);
}

function countPending(pool: QueuePool): number {
  let pending = 0;
  for (const entry of pool.pending) {
    if (!entry.cancelled && !entry.released) pending += 1;
  }
  return pending;
}

export function getImageQueueStats(): {
  active: number;
  pending: number;
  cachedActive: number;
  cachedPending: number;
} {
  return {
    active: networkPool.activeCount,
    pending: countPending(networkPool),
    cachedActive: cachedDecodePool.activeCount,
    cachedPending: countPending(cachedDecodePool),
  };
}

function resetPool(pool: QueuePool) {
  for (const entry of pool.pending) {
    clearEntryTimer(entry);
    entry.cancelled = true;
    entry.released = true;
  }
  pool.pending.length = 0;
  pool.activeCount = 0;
}

export function resetImageQueueForTests(): void {
  resetPool(networkPool);
  resetPool(cachedDecodePool);
  loadedUrls.clear();
  failedUrls.clear();
}

export type QueuedImageStatus = "idle" | "loading" | "ready" | "error";

export function useQueuedImageSrc(src: string | null): {
  activeSrc: string | null;
  status: QueuedImageStatus;
  markLoaded: () => void;
  markFailed: () => void;
} {
  const [grantedSrc, setGrantedSrc] = useState<string | null>(null);
  const [readySrc, setReadySrc] = useState<string | null>(null);
  const [errorSrc, setErrorSrc] = useState<string | null>(() =>
    src && isImageFailed(src) ? src : null,
  );
  const releaseRef = useRef<(() => void) | null>(null);

  useEffect(() => {
    releaseRef.current?.();
    releaseRef.current = null;

    if (!src) {
      setGrantedSrc(null);
      setReadySrc(null);
      setErrorSrc(null);
      return;
    }

    if (isImageFailed(src)) {
      setGrantedSrc(null);
      setReadySrc(null);
      setErrorSrc(src);
      return;
    }

    setGrantedSrc(null);
    setReadySrc(null);
    setErrorSrc(null);

    let cancelled = false;
    const release = enqueueImageLoad(
      src,
      () => {
        if (!cancelled) {
          setGrantedSrc(src);
        }
      },
      { cached: isImageLoaded(src) },
    );
    releaseRef.current = release;

    return () => {
      cancelled = true;
      release();
      if (releaseRef.current === release) {
        releaseRef.current = null;
      }
    };
  }, [src]);

  const markLoaded = useCallback(() => {
    if (!src) return;
    markImageLoaded(src);
    releaseRef.current?.();
    releaseRef.current = null;
    setReadySrc(src);
  }, [src]);

  const markFailed = useCallback(() => {
    if (!src) return;
    markImageFailed(src);
    releaseRef.current?.();
    releaseRef.current = null;
    setErrorSrc(src);
  }, [src]);

  if (!src) {
    return { activeSrc: null, status: "idle", markLoaded, markFailed };
  }
  if (errorSrc === src || isImageFailed(src)) {
    return { activeSrc: null, status: "error", markLoaded, markFailed };
  }
  if (readySrc === src) {
    return { activeSrc: src, status: "ready", markLoaded, markFailed };
  }
  return {
    activeSrc: grantedSrc === src ? src : null,
    status: "loading",
    markLoaded,
    markFailed,
  };
}
