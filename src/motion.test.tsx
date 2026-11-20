import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const stopSpy = vi.fn();
const animateSpy = vi.fn((..._args: unknown[]) => ({ stop: stopSpy }));

vi.mock("motion/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("motion/react")>();
  return {
    ...actual,
    animate: (...args: unknown[]) => animateSpy(...args),
    useReducedMotion: () => false,
  };
});

import { NavGroup, NavTab, Segment, SegmentTab } from "./motion";

function mockButtonRect(
  btn: HTMLButtonElement,
  rect: { left: number; top: number; width: number; height: number },
) {
  Object.defineProperty(btn, "offsetLeft", { configurable: true, value: rect.left });
  Object.defineProperty(btn, "offsetTop", { configurable: true, value: rect.top });
  Object.defineProperty(btn, "offsetWidth", { configurable: true, value: rect.width });
  Object.defineProperty(btn, "offsetHeight", { configurable: true, value: rect.height });
}

describe("HighlightGroup indicators (SegmentTab and NavTab)", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    animateSpy.mockClear();
    stopSpy.mockClear();
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => root.unmount());
    host.remove();
  });

  async function render(ui: ReactElement) {
    await act(async () => {
      root.render(ui);
    });
  }

  it("does not animate on initial mount or when re-rendering with the same active tab", async () => {
    function Harness({ parked, active }: { parked: boolean; active: "a" | "b" }) {
      return (
        <div className={parked ? "tab-kept tab-parked" : "tab-kept"}>
          <Segment>
            <SegmentTab active={active === "a"}>A</SegmentTab>
            <SegmentTab active={active === "b"}>B</SegmentTab>
          </Segment>
        </div>
      );
    }

    await render(<Harness parked={true} active="b" />);
    const buttons = host.querySelectorAll<HTMLButtonElement>("button");
    mockButtonRect(buttons[0], { left: 4, top: 4, width: 50, height: 30 });
    mockButtonRect(buttons[1], { left: 58, top: 4, width: 60, height: 30 });

    expect(animateSpy).not.toHaveBeenCalled();
    expect(buttons[0].querySelector(".segment-indicator")).toBeNull();
    expect(buttons[1].querySelector(".segment-indicator")).not.toBeNull();

    // Unparking the tab keeps "b" active and must not trigger an indicator animation.
    await render(<Harness parked={false} active="b" />);
    expect(animateSpy).not.toHaveBeenCalled();
    expect(buttons[1].querySelector(".segment-indicator")).not.toBeNull();
  });

  it("animates in container-local coordinates only when switching to another SegmentTab", async () => {
    function Harness({ active }: { active: "light" | "dark" | "system" }) {
      return (
        <Segment>
          <SegmentTab active={active === "light"}>Light</SegmentTab>
          <SegmentTab active={active === "dark"}>Dark</SegmentTab>
          <SegmentTab active={active === "system"}>System</SegmentTab>
        </Segment>
      );
    }

    await render(<Harness active="system" />);
    const buttons = host.querySelectorAll<HTMLButtonElement>("button");
    mockButtonRect(buttons[0], { left: 4, top: 4, width: 50, height: 30 });
    mockButtonRect(buttons[1], { left: 58, top: 4, width: 50, height: 30 });
    mockButtonRect(buttons[2], { left: 112, top: 4, width: 70, height: 30 });

    // Re-render once with the same active tab so the group records "system"'s local rect.
    await render(<Harness active="system" />);
    expect(animateSpy).not.toHaveBeenCalled();

    // Switch active tab from "system" (112, 4, 70x30) to "light" (4, 4, 50x30).
    await render(<Harness active="light" />);
    expect(animateSpy).toHaveBeenCalledTimes(1);
    const [el, keyframes] = animateSpy.mock.calls[0] as unknown as [
      HTMLElement,
      { x: number[]; y: number[]; scaleX: number[]; scaleY: number[] },
    ];
    expect(el.className).toBe("segment-indicator");
    expect(keyframes.x).toEqual([108, 0]);
    expect(keyframes.y).toEqual([0, 0]);
    expect(keyframes.scaleX).toEqual([70 / 50, 1]);
    expect(keyframes.scaleY).toEqual([1, 1]);
  });

  it("animates NavTab vertically inside NavGroup without reacting to unrelated re-renders", async () => {
    function Harness({ active, showBack }: { active: "mods" | "settings"; showBack: boolean }) {
      return (
        <div>
          {showBack ? <button type="button">Back</button> : null}
          <NavGroup>
            <NavTab active={active === "mods"}>Mods</NavTab>
            <NavTab active={active === "settings"}>Settings</NavTab>
          </NavGroup>
        </div>
      );
    }

    await render(<Harness active="mods" showBack={false} />);
    const navButtons = host.querySelectorAll<HTMLButtonElement>("nav button");
    mockButtonRect(navButtons[0], { left: 0, top: 0, width: 180, height: 36 });
    mockButtonRect(navButtons[1], { left: 0, top: 42, width: 180, height: 36 });

    await render(<Harness active="mods" showBack={true} />);
    expect(animateSpy).not.toHaveBeenCalled();

    await render(<Harness active="settings" showBack={true} />);
    expect(animateSpy).toHaveBeenCalledTimes(1);
    const [el, keyframes] = animateSpy.mock.calls[0] as unknown as [
      HTMLElement,
      { x: number[]; y: number[]; scaleX: number[]; scaleY: number[] },
    ];
    expect(el.className).toBe("nav-indicator");
    expect(keyframes.x).toEqual([0, 0]);
    expect(keyframes.y).toEqual([-42, 0]);
  });

  it("does not stop an in-flight NavTab or SegmentTab animation on unrelated re-renders", async () => {
    function Harness({
      tab,
      timeout,
      tick,
    }: {
      tab: "library" | "collections" | "browse";
      timeout: "never" | "10s" | "30s";
      tick: number;
    }) {
      return (
        <div data-tick={tick}>
          <NavGroup>
            <NavTab active={tab === "library"}>Library</NavTab>
            <NavTab active={tab === "collections"}>Collections</NavTab>
            <NavTab active={tab === "browse"}>Browse</NavTab>
          </NavGroup>
          <Segment>
            <SegmentTab active={timeout === "never"}>Never</SegmentTab>
            <SegmentTab active={timeout === "10s"}>10s</SegmentTab>
            <SegmentTab active={timeout === "30s"}>30s</SegmentTab>
          </Segment>
        </div>
      );
    }

    await render(<Harness tab="library" timeout="never" tick={0} />);
    const navButtons = host.querySelectorAll<HTMLButtonElement>("nav button");
    mockButtonRect(navButtons[0], { left: 0, top: 0, width: 180, height: 36 });
    mockButtonRect(navButtons[1], { left: 0, top: 42, width: 180, height: 36 });
    mockButtonRect(navButtons[2], { left: 0, top: 84, width: 180, height: 36 });

    const segButtons = host.querySelectorAll<HTMLButtonElement>(".segment button");
    mockButtonRect(segButtons[0], { left: 4, top: 4, width: 64, height: 30 });
    mockButtonRect(segButtons[1], { left: 72, top: 4, width: 44, height: 30 });
    mockButtonRect(segButtons[2], { left: 120, top: 4, width: 44, height: 30 });

    // Switch both NavTab and SegmentTab.
    await render(<Harness tab="browse" timeout="10s" tick={1} />);
    expect(animateSpy).toHaveBeenCalledTimes(2);
    expect(stopSpy).not.toHaveBeenCalled();

    const navIndicator = navButtons[2].querySelector<HTMLSpanElement>(".nav-indicator")!;
    const segIndicator = segButtons[1].querySelector<HTMLSpanElement>(".segment-indicator")!;
    // Simulate mid-flight transforms applied by motion's animate().
    navIndicator.style.transform = "translateY(-42px)";
    segIndicator.style.transform = "translateX(-68px)";

    // Unrelated re-renders while animating must not stop or freeze the indicators.
    await render(<Harness tab="browse" timeout="10s" tick={2} />);
    await render(<Harness tab="browse" timeout="10s" tick={3} />);
    expect(animateSpy).toHaveBeenCalledTimes(2);
    expect(stopSpy).not.toHaveBeenCalled();

    // Switching again stops the previous indicators and clears their inline transforms.
    await render(<Harness tab="collections" timeout="30s" tick={4} />);
    expect(stopSpy).toHaveBeenCalledTimes(2);
    expect(navIndicator.style.transform).toBe("");
    expect(segIndicator.style.transform).toBe("");
    expect(animateSpy).toHaveBeenCalledTimes(4);
  });

  it("animates cleanly after unparking a parked Segment without an extra re-render", async () => {
    function Harness({ parked, active }: { parked: boolean; active: "a" | "b" }) {
      return (
        <div className={parked ? "tab-kept tab-parked" : "tab-kept"}>
          <Segment>
            <SegmentTab active={active === "a"}>A</SegmentTab>
            <SegmentTab active={active === "b"}>B</SegmentTab>
          </Segment>
        </div>
      );
    }

    // Mounted while parked (offsetWidth/Height default to 0 in jsdom).
    await render(<Harness parked={true} active="a" />);
    const buttons = host.querySelectorAll<HTMLButtonElement>("button");
    expect(animateSpy).not.toHaveBeenCalled();

    // Unparked: buttons now have layout dimensions.
    mockButtonRect(buttons[0], { left: 4, top: 4, width: 50, height: 30 });
    mockButtonRect(buttons[1], { left: 58, top: 4, width: 60, height: 30 });

    await render(<Harness parked={false} active="b" />);
    expect(animateSpy).toHaveBeenCalledTimes(1);
    const [, keyframes] = animateSpy.mock.calls[0] as unknown as [
      HTMLElement,
      { x: number[]; y: number[]; scaleX: number[]; scaleY: number[] },
    ];
    expect(keyframes.x).toEqual([-54, 0]);
    expect(keyframes.scaleX).toEqual([50 / 60, 1]);
  });
});
