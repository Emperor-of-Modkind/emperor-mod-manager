import {
  Fragment,
  createElement,
  memo,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

type ParkedTabGateProps = {
  active: boolean;
  children: ReactNode;
};

/**
 * Freezes reconciliation for parked tabs. While both the previous and next
 * renders have `active === false`, React skips reconciling the subtree so
 * unrelated state changes (progress ticks, typing, switching other tabs)
 * perform zero work inside inactive pages.
 */
export const ParkedTabGate = memo(
  function ParkedTabGate({ children }: ParkedTabGateProps) {
    return createElement(Fragment, null, children);
  },
  (prev, next) => !prev.active && !next.active,
);

type ChunkedOptions = {
  initial?: number;
  step?: number;
  active?: boolean;
};

/**
 * Progressively ramps a rendered item count from `initial` to `total` across
 * animation frames so large non-virtualized lists paint frame 1 immediately
 * without blocking the main thread.
 */
export function useChunkedCount(
  total: number,
  { initial = 24, step = 24, active = true }: ChunkedOptions = {},
): number {
  const safeInitial = Math.max(1, initial);
  const safeStep = Math.max(1, step);
  const [count, setCount] = useState(() => Math.min(total, safeInitial));

  useEffect(() => {
    if (total <= safeInitial) {
      setCount(total);
      return;
    }
    setCount((prev) => {
      if (prev > total) return total;
      if (prev < safeInitial) return safeInitial;
      return prev;
    });
  }, [total, safeInitial]);

  useEffect(() => {
    if (!active || count >= total) return;
    let cancelled = false;
    const id = window.requestAnimationFrame(() => {
      if (cancelled) return;
      setCount((prev) => Math.min(total, prev + safeStep));
    });
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(id);
    };
  }, [active, count, safeStep, total]);

  return Math.min(total, count);
}

/**
 * Runs a background refresh callback only after `active` has remained true for
 * `delayMs`. Rapidly clicking across sidebar tabs cancels the timer before any
 * expensive IPC or disk scan starts.
 */
export function useNavSettledEffect(
  active: boolean,
  delayMs: number,
  effect: () => void | (() => void),
  deps: readonly unknown[] = [],
) {
  const effectRef = useRef(effect);
  effectRef.current = effect;
  const delayRef = useRef(delayMs);
  delayRef.current = delayMs;

  useEffect(() => {
    if (!active) return;
    const wait = delayRef.current;
    if (wait <= 0) {
      const cleanup = effectRef.current();
      return () => {
        if (typeof cleanup === "function") cleanup();
      };
    }
    let cleanup: void | (() => void);
    let cancelled = false;
    const id = window.setTimeout(() => {
      if (cancelled) return;
      cleanup = effectRef.current();
    }, wait);
    return () => {
      cancelled = true;
      window.clearTimeout(id);
      if (typeof cleanup === "function") cleanup();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [active, ...deps]);
}

/** Tracks per-key freshness timestamps so rapid tab revisits skip redundant IPCs. */
export class TtlTracker {
  private readonly timestamps = new Map<string, number>();

  isFresh(key: string, ttlMs: number, now = Date.now()): boolean {
    const last = this.timestamps.get(key);
    return last != null && now - last < ttlMs;
  }

  mark(key: string, now = Date.now()): void {
    this.timestamps.set(key, now);
  }

  invalidate(key?: string): void {
    if (key == null) {
      this.timestamps.clear();
    } else {
      this.timestamps.delete(key);
    }
  }
}

export type ThrottledBatcher<K, V> = {
  push: (key: K, value: V, immediate?: boolean) => void;
  flushNow: () => void;
  cancel: () => void;
};

/**
 * Coalesces high-frequency keyed events (such as download progress ticks) into
 * periodic batched flushes. Pass `immediate: true` for terminal transitions.
 */
export function createThrottledBatcher<K, V>(
  intervalMs: number,
  onFlush: (batch: Map<K, V>) => void,
): ThrottledBatcher<K, V> {
  let pending = new Map<K, V>();
  let timer: number | null = null;

  const flushNow = () => {
    if (timer != null) {
      window.clearTimeout(timer);
      timer = null;
    }
    if (pending.size === 0) return;
    const batch = pending;
    pending = new Map<K, V>();
    onFlush(batch);
  };

  const push = (key: K, value: V, immediate = false) => {
    pending.set(key, value);
    if (immediate) {
      flushNow();
      return;
    }
    if (timer == null) {
      timer = window.setTimeout(flushNow, intervalMs);
    }
  };

  const cancel = () => {
    if (timer != null) {
      window.clearTimeout(timer);
      timer = null;
    }
    pending.clear();
  };

  return { push, flushNow, cancel };
}

export function stringSetEqual(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
  if (a === b) return true;
  if (a.size !== b.size) return false;
  for (const item of a) {
    if (!b.has(item)) return false;
  }
  return true;
}

export function shallowObjectEqual<T extends Record<string, unknown>>(
  a: T,
  b: T,
): boolean {
  if (a === b) return true;
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) return false;
  for (const key of aKeys) {
    if (!Object.prototype.hasOwnProperty.call(b, key)) return false;
    if (!Object.is(a[key], b[key])) return false;
  }
  return true;
}

export function shallowRecordOfObjectsEqual<T extends Record<string, unknown>>(
  a: Record<string, T>,
  b: Record<string, T>,
): boolean {
  if (a === b) return true;
  const aKeys = Object.keys(a);
  const bKeys = Object.keys(b);
  if (aKeys.length !== bKeys.length) return false;
  for (const key of aKeys) {
    if (!Object.prototype.hasOwnProperty.call(b, key)) return false;
    if (!shallowObjectEqual(a[key], b[key])) return false;
  }
  return true;
}
