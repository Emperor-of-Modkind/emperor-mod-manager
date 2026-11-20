import { act, useState } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  ParkedTabGate,
  TtlTracker,
  createThrottledBatcher,
  shallowObjectEqual,
  shallowRecordOfObjectsEqual,
  stringSetEqual,
  useChunkedCount,
  useNavSettledEffect,
} from "./perf";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

describe("TtlTracker", () => {
  it("tracks per-key freshness windows and supports targeted or full invalidation", () => {
    const ttl = new TtlTracker();
    expect(ttl.isFresh("scan", 30_000, 1000)).toBe(false);

    ttl.mark("scan", 1000);
    expect(ttl.isFresh("scan", 30_000, 1000)).toBe(true);
    expect(ttl.isFresh("scan", 30_000, 30_999)).toBe(true);
    expect(ttl.isFresh("scan", 30_000, 31_000)).toBe(false);

    ttl.mark("meta", 5000);
    ttl.invalidate("scan");
    expect(ttl.isFresh("scan", 30_000, 5000)).toBe(false);
    expect(ttl.isFresh("meta", 30_000, 5000)).toBe(true);

    ttl.invalidate();
    expect(ttl.isFresh("meta", 30_000, 5000)).toBe(false);
  });
});

describe("createThrottledBatcher", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("coalesces rapid updates for the same key into one periodic flush", () => {
    const flushed: Array<Array<[string, number]>> = [];
    const batcher = createThrottledBatcher<string, number>(120, (batch) => {
      flushed.push(Array.from(batch.entries()));
    });

    batcher.push("dl-1", 10);
    batcher.push("dl-1", 25);
    batcher.push("dl-2", 5);
    expect(flushed).toHaveLength(0);

    vi.advanceTimersByTime(119);
    expect(flushed).toHaveLength(0);

    vi.advanceTimersByTime(1);
    expect(flushed).toEqual([
      [
        ["dl-1", 25],
        ["dl-2", 5],
      ],
    ]);
  });

  it("flushes immediately on terminal transitions and clears pending timers", () => {
    const flushed: Array<Array<[string, string]>> = [];
    const batcher = createThrottledBatcher<string, string>(120, (batch) => {
      flushed.push(Array.from(batch.entries()));
    });

    batcher.push("dl-1", "downloading");
    batcher.push("dl-1", "staged", true);
    expect(flushed).toEqual([[["dl-1", "staged"]]]);

    vi.advanceTimersByTime(500);
    expect(flushed).toHaveLength(1);
  });

  it("drops pending items when cancelled", () => {
    const onFlush = vi.fn();
    const batcher = createThrottledBatcher<string, number>(120, onFlush);

    batcher.push("dl-1", 50);
    batcher.cancel();
    vi.advanceTimersByTime(500);
    expect(onFlush).not.toHaveBeenCalled();
  });
});

describe("equality helpers", () => {
  it("compares string sets by membership", () => {
    expect(stringSetEqual(new Set(["a", "b"]), new Set(["b", "a"]))).toBe(true);
    expect(stringSetEqual(new Set(["a"]), new Set(["a", "b"]))).toBe(false);
    expect(stringSetEqual(new Set(["a", "c"]), new Set(["a", "b"]))).toBe(false);
  });

  it("compares shallow objects and records of shallow objects", () => {
    expect(shallowObjectEqual({ a: 1, b: "x" }, { a: 1, b: "x" })).toBe(true);
    expect(shallowObjectEqual({ a: 1, b: "x" }, { a: 1, b: "y" })).toBe(false);
    expect(
      shallowRecordOfObjectsEqual(
        { k1: { installed: 2, total: 5 }, k2: { installed: 1, total: 1 } },
        { k1: { installed: 2, total: 5 }, k2: { installed: 1, total: 1 } },
      ),
    ).toBe(true);
    expect(
      shallowRecordOfObjectsEqual(
        { k1: { installed: 2, total: 5 } },
        { k1: { installed: 3, total: 5 } },
      ),
    ).toBe(false);
  });
});

describe("React perf primitives", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    host.remove();
    vi.useRealTimers();
  });

  it("ParkedTabGate skips subtree reconciliation while inactive and wakes on activation", () => {
    let childRenders = 0;
    function Child({ value }: { value: number }) {
      childRenders += 1;
      return <span data-testid="child">{value}</span>;
    }

    let setProps!: (next: { active: boolean; value: number }) => void;
    function Harness() {
      const [state, setState] = useState({ active: false, value: 1 });
      setProps = setState;
      return (
        <ParkedTabGate active={state.active}>
          <Child value={state.value} />
        </ParkedTabGate>
      );
    }

    act(() => {
      root.render(<Harness />);
    });
    expect(childRenders).toBe(1);
    expect(host.textContent).toBe("1");

    act(() => {
      setProps({ active: false, value: 2 });
    });
    act(() => {
      setProps({ active: false, value: 3 });
    });
    expect(childRenders).toBe(1);
    expect(host.textContent).toBe("1");

    act(() => {
      setProps({ active: true, value: 3 });
    });
    expect(childRenders).toBe(2);
    expect(host.textContent).toBe("3");

    act(() => {
      setProps({ active: true, value: 4 });
    });
    expect(childRenders).toBe(3);
    expect(host.textContent).toBe("4");
  });

  it("useNavSettledEffect cancels rapid switches, runs synchronously when delayMs <= 0, and ignores delayMs changes", () => {
    vi.useFakeTimers();
    const calls: string[] = [];
    const cleanups: string[] = [];

    let setProps!: (next: { active: boolean; delay: number; tag: string }) => void;
    function Harness() {
      const [state, setState] = useState({ active: true, delay: 150, tag: "a" });
      setProps = setState;
      useNavSettledEffect(
        state.active,
        state.delay,
        () => {
          calls.push(state.tag);
          return () => cleanups.push(state.tag);
        },
        [state.tag],
      );
      return null;
    }

    act(() => {
      root.render(<Harness />);
    });
    vi.advanceTimersByTime(100);
    expect(calls).toEqual([]);

    // Switching away before 150ms cancels the scheduled work.
    act(() => {
      setProps({ active: false, delay: 150, tag: "a" });
    });
    vi.advanceTimersByTime(500);
    expect(calls).toEqual([]);

    // Activating with delay 0 fires synchronously, and changing delay to 150 does not re-fire.
    act(() => {
      setProps({ active: true, delay: 0, tag: "b" });
    });
    expect(calls).toEqual(["b"]);

    act(() => {
      setProps({ active: true, delay: 150, tag: "b" });
    });
    vi.advanceTimersByTime(500);
    expect(calls).toEqual(["b"]);
    expect(cleanups).toEqual([]);

    // Deactivating runs cleanup for the settled effect.
    act(() => {
      setProps({ active: false, delay: 150, tag: "b" });
    });
    expect(cleanups).toEqual(["b"]);
  });

  it("useChunkedCount progressively ramps rendered counts across animation frames", () => {
    const frames: FrameRequestCallback[] = [];
    const origRaf = window.requestAnimationFrame;
    const origCancel = window.cancelAnimationFrame;
    window.requestAnimationFrame = ((cb: FrameRequestCallback) => {
      frames.push(cb);
      return frames.length;
    }) as typeof window.requestAnimationFrame;
    window.cancelAnimationFrame = (() => {}) as typeof window.cancelAnimationFrame;

    try {
      let setState!: (next: { total: number; active: boolean }) => void;
      function Harness() {
        const [state, update] = useState({ total: 50, active: true });
        setState = update;
        const visible = useChunkedCount(state.total, {
          initial: 20,
          step: 20,
          active: state.active,
        });
        return <span>{visible}</span>;
      }

      act(() => {
        root.render(<Harness />);
      });
      expect(host.textContent).toBe("20");
      expect(frames.length).toBe(1);

      act(() => {
        frames.shift()!(16);
      });
      expect(host.textContent).toBe("40");

      act(() => {
        frames.shift()!(32);
      });
      expect(host.textContent).toBe("50");

      // Shrinking total clamps immediately.
      act(() => {
        setState({ total: 12, active: true });
      });
      expect(host.textContent).toBe("12");
    } finally {
      window.requestAnimationFrame = origRaf;
      window.cancelAnimationFrame = origCancel;
    }
  });
});
