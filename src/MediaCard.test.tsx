import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MediaCard } from "./MediaCard";

async function render(ui: ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return {
    host,
    async unmount() {
      await act(async () => root.unmount());
      host.remove();
    },
  };
}

function makeRect(left: number, top: number, width: number, height: number): DOMRect {
  return {
    x: left,
    y: top,
    left,
    top,
    width,
    height,
    right: left + width,
    bottom: top + height,
    toJSON: () => ({}),
  };
}

afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});

describe("MediaCard glow portal", () => {
  it("portals .media-cover-glow into the enclosing .panel outside .page-scroll and tracks scroll", async () => {
    vi.useFakeTimers();
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((cb) => {
      return window.setTimeout(() => cb(performance.now()), 0);
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id) => {
      window.clearTimeout(id);
    });

    const view = await render(
      <section className="panel page-panel library-home">
        <div className="page-chrome">
          <h1>Library</h1>
        </div>
        <div className="page-scroll">
          <MediaCard
            plain
            title="CONTROL Resonant"
            imageSrc="https://example.test/control.jpg"
            onClick={() => {}}
          />
        </div>
      </section>,
    );

    const panel = view.host.querySelector<HTMLElement>(".panel")!;
    const scrollEl = view.host.querySelector<HTMLElement>(".page-scroll")!;
    const stack = view.host.querySelector<HTMLElement>(".media-cover-stack")!;
    const coverImg = view.host.querySelector<HTMLImageElement>(".media-cover img")!;

    let stackTop = 100;
    panel.getBoundingClientRect = () => makeRect(20, 10, 1000, 700);
    stack.getBoundingClientRect = () => makeRect(40, stackTop, 200, 300);

    // Mark the cover image as loaded so canGlow becomes true.
    await act(async () => {
      coverImg.dispatchEvent(new Event("load"));
    });

    // Hover the card cover stack.
    await act(async () => {
      stack.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    });
    await act(async () => {
      vi.advanceTimersByTime(32);
    });

    const glow = panel.querySelector<HTMLImageElement>(":scope > img.media-cover-glow");
    expect(glow).not.toBeNull();
    expect(glow!.classList.contains("media-cover-glow-portal")).toBe(true);
    expect(glow!.classList.contains("is-visible")).toBe(true);
    expect(scrollEl.querySelector("img.media-cover-glow")).toBeNull();

    // Geometry: left = (40 - 20) - 200 * 0.18 = -16, top = (100 - 10) - 300 * 0.18 = 36
    // width = 200 * 1.36 = 272, height = 300 * 1.36 = 408
    expect(Number.parseFloat(glow!.style.left)).toBeCloseTo(-16, 4);
    expect(Number.parseFloat(glow!.style.top)).toBeCloseTo(36, 4);
    expect(Number.parseFloat(glow!.style.width)).toBeCloseTo(272, 4);
    expect(Number.parseFloat(glow!.style.height)).toBeCloseTo(408, 4);

    // Simulate scrolling .page-scroll by 50px upwards.
    stackTop = 50;
    await act(async () => {
      scrollEl.dispatchEvent(new Event("scroll"));
      vi.advanceTimersByTime(16);
    });
    expect(Number.parseFloat(glow!.style.top)).toBeCloseTo(-14, 4);

    // Mouse leave fades out and unmounts after the 550ms fallback timer.
    await act(async () => {
      stack.dispatchEvent(new MouseEvent("mouseout", { bubbles: true }));
    });
    expect(glow!.classList.contains("is-visible")).toBe(false);
    await act(async () => {
      vi.advanceTimersByTime(560);
    });
    expect(panel.querySelector("img.media-cover-glow")).toBeNull();

    await view.unmount();
  });

  it("falls back to inline .media-cover-glow when rendered without a .panel ancestor", async () => {
    vi.useFakeTimers();
    const view = await render(
      <MediaCard
        plain
        title="Standalone Game"
        imageSrc="https://example.test/standalone.jpg"
        onClick={() => {}}
      />,
    );

    const stack = view.host.querySelector<HTMLElement>(".media-cover-stack")!;
    const coverImg = view.host.querySelector<HTMLImageElement>(".media-cover img")!;

    await act(async () => {
      coverImg.dispatchEvent(new Event("load"));
    });
    await act(async () => {
      stack.dispatchEvent(new MouseEvent("mouseover", { bubbles: true }));
    });

    const inlineGlow = stack.querySelector<HTMLImageElement>(":scope > img.media-cover-glow");
    expect(inlineGlow).not.toBeNull();
    expect(inlineGlow!.classList.contains("media-cover-glow-portal")).toBe(false);

    await view.unmount();
  });
});
