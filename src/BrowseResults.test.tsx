import { createRoot, type Root } from "react-dom/client";
import { act, createRef, type ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { BrowseVirtualGrid, BrowseVirtualList } from "./BrowseResults";

(globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;

class FakeResizeObserver {
  static instances: FakeResizeObserver[] = [];
  readonly targets = new Set<Element>();
  private readonly cb: ResizeObserverCallback;

  constructor(cb: ResizeObserverCallback) {
    this.cb = cb;
    FakeResizeObserver.instances.push(this);
  }

  observe(target: Element) {
    this.targets.add(target);
  }

  unobserve(target: Element) {
    this.targets.delete(target);
  }

  disconnect() {
    this.targets.clear();
  }

  trigger() {
    this.cb([], this as unknown as ResizeObserver);
  }
}

function setClientSize(el: HTMLElement, width: number, height: number) {
  Object.defineProperty(el, "clientWidth", { configurable: true, value: width });
  Object.defineProperty(el, "clientHeight", { configurable: true, value: height });
}

async function mount(ui: ReactElement, container?: HTMLElement) {
  const host = container ?? document.createElement("div");
  if (!host.isConnected) document.body.appendChild(host);
  const root: Root = createRoot(host);
  await act(async () => {
    root.render(ui);
  });
  return {
    host,
    async rerender(next: ReactElement) {
      await act(async () => {
        root.render(next);
      });
    },
    async unmount() {
      await act(async () => root.unmount());
      if (!container) host.remove();
    },
  };
}

describe("BrowseResults virtualizers", () => {
  const originalResizeObserver = globalThis.ResizeObserver;

  beforeEach(() => {
    FakeResizeObserver.instances = [];
    globalThis.ResizeObserver = FakeResizeObserver as unknown as typeof ResizeObserver;
  });

  afterEach(() => {
    globalThis.ResizeObserver = originalResizeObserver;
    vi.restoreAllMocks();
  });

  it("virtualizes large grids instead of mounting every item", async () => {
    const scrollEl = document.createElement("div");
    setClientSize(scrollEl, 800, 600);
    document.body.appendChild(scrollEl);
    const scrollRef = createRef<HTMLElement | null>();
    scrollRef.current = scrollEl;

    const items = Array.from({ length: 200 }, (_, i) => ({ id: `mod-${i}` }));
    const view = await mount(
      <BrowseVirtualGrid
        items={items}
        scrollRef={scrollRef}
        minCardPx={160}
        getKey={(item) => item.id}
        renderItem={(item) => <div className="test-card">{item.id}</div>}
      />,
      scrollEl,
    );

    const renderedCards = scrollEl.querySelectorAll(".test-card");
    expect(renderedCards.length).toBeGreaterThan(0);
    expect(renderedCards.length).toBeLessThan(40);

    await view.unmount();
    scrollEl.remove();
  });

  it("does not report page size while BrowseVirtualList is parked (active=false)", async () => {
    const scrollEl = document.createElement("div");
    setClientSize(scrollEl, 800, 600);
    document.body.appendChild(scrollEl);
    const scrollRef = createRef<HTMLElement | null>();
    scrollRef.current = scrollEl;

    const onPageSize = vi.fn();
    const items = Array.from({ length: 40 }, (_, i) => ({ id: `item-${i}` }));
    const view = await mount(
      <BrowseVirtualList
        items={items}
        scrollRef={scrollRef}
        active={false}
        estimateSize={112}
        rowClassName="row"
        onPageSize={onPageSize}
        getKey={(item) => item.id}
        renderItem={(item) => <span>{item.id}</span>}
      />,
      scrollEl,
    );

    expect(onPageSize).not.toHaveBeenCalled();

    await view.rerender(
      <BrowseVirtualList
        items={items}
        scrollRef={scrollRef}
        active={true}
        estimateSize={112}
        rowClassName="row"
        onPageSize={onPageSize}
        getKey={(item) => item.id}
        renderItem={(item) => <span>{item.id}</span>}
      />,
    );
    expect(onPageSize).toHaveBeenCalledTimes(1);

    // Parking again and collapsing to 0x0 must not overwrite page size.
    onPageSize.mockClear();
    await view.rerender(
      <BrowseVirtualList
        items={items}
        scrollRef={scrollRef}
        active={false}
        estimateSize={112}
        rowClassName="row"
        onPageSize={onPageSize}
        getKey={(item) => item.id}
        renderItem={(item) => <span>{item.id}</span>}
      />,
    );
    setClientSize(scrollEl, 0, 0);
    await act(async () => {
      for (const observer of FakeResizeObserver.instances) observer.trigger();
    });
    expect(onPageSize).not.toHaveBeenCalled();

    await view.unmount();
    scrollEl.remove();
  });

  it("does not spin requestAnimationFrame when mounted scroll element starts at width 0", async () => {
    const rafSpy = vi.spyOn(window, "requestAnimationFrame").mockReturnValue(1);
    const scrollEl = document.createElement("div");
    setClientSize(scrollEl, 0, 0);
    document.body.appendChild(scrollEl);
    const scrollRef = createRef<HTMLElement | null>();
    scrollRef.current = scrollEl;

    const view = await mount(
      <BrowseVirtualGrid
        items={[{ id: "mod-0" }]}
        scrollRef={scrollRef}
        minCardPx={160}
        getKey={(item) => item.id}
        renderItem={(item) => <div>{item.id}</div>}
      />,
      scrollEl,
    );

    expect(rafSpy).not.toHaveBeenCalled();

    await view.unmount();
    scrollEl.remove();
  });

  it("preserves rendered card DOM nodes when width changes within the same column count", async () => {
    const scrollEl = document.createElement("div");
    setClientSize(scrollEl, 800, 600);
    document.body.appendChild(scrollEl);
    const scrollRef = createRef<HTMLElement | null>();
    scrollRef.current = scrollEl;

    const items = Array.from({ length: 24 }, (_, i) => ({ id: `mod-${i}` }));
    const view = await mount(
      <BrowseVirtualGrid
        items={items}
        scrollRef={scrollRef}
        minCardPx={160}
        getKey={(item) => item.id}
        renderItem={(item) => <div data-id={item.id}>{item.id}</div>}
      />,
      scrollEl,
    );

    const firstCardBefore = scrollEl.querySelector("[data-id='mod-0']");
    expect(firstCardBefore).not.toBeNull();

    // Change width by 20px (still 4 columns for minCardPx=160).
    setClientSize(scrollEl, 820, 600);
    await act(async () => {
      for (const observer of FakeResizeObserver.instances) observer.trigger();
    });

    const firstCardAfter = scrollEl.querySelector("[data-id='mod-0']");
    expect(firstCardAfter).toBe(firstCardBefore);

    await view.unmount();
    scrollEl.remove();
  });

  it("staggers initial virtual list rows when staggerEnter is true without clobbering transform", async () => {
    const scrollEl = document.createElement("div");
    setClientSize(scrollEl, 800, 600);
    document.body.appendChild(scrollEl);
    const scrollRef = createRef<HTMLElement | null>();
    scrollRef.current = scrollEl;

    const items = Array.from({ length: 10 }, (_, i) => ({ id: `row-${i}` }));
    const view = await mount(
      <BrowseVirtualList
        items={items}
        scrollRef={scrollRef}
        active={true}
        staggerEnter={true}
        staggerKey="game-1"
        estimateSize={60}
        rowClassName="row"
        getKey={(item) => item.id}
        renderItem={(item) => <span>{item.id}</span>}
      />,
      scrollEl,
    );

    const rows = Array.from(scrollEl.querySelectorAll("li.browse-virtual-row"));
    expect(rows.length).toBeGreaterThan(1);
    expect(rows[0].classList.contains("is-stagger")).toBe(true);
    expect((rows[0] as HTMLElement).style.getPropertyValue("--stagger-index")).toBe("0");
    expect((rows[1] as HTMLElement).style.getPropertyValue("--stagger-index")).toBe("1");
    expect((rows[1] as HTMLElement).style.transform).toContain("translateY(");

    await view.rerender(
      <BrowseVirtualList
        items={items}
        scrollRef={scrollRef}
        active={true}
        staggerEnter={false}
        staggerKey="game-1"
        estimateSize={60}
        rowClassName="row"
        getKey={(item) => item.id}
        renderItem={(item) => <span>{item.id}</span>}
      />,
    );

    const rowsAfter = Array.from(scrollEl.querySelectorAll("li.browse-virtual-row"));
    expect(rowsAfter[0].classList.contains("is-stagger")).toBe(false);

    await view.unmount();
    scrollEl.remove();
  });

  it("fills remaining columns of a partial final row with skeletons when fillTrailingSkeletons is true", async () => {
    const scrollEl = document.createElement("div");
    setClientSize(scrollEl, 800, 600);
    document.body.appendChild(scrollEl);
    const scrollRef = createRef<HTMLElement | null>();
    scrollRef.current = scrollEl;

    // Width 800, minCardPx 160 -> 4 columns; 6 items leaves 2 trailing slots in row 2
    const items = Array.from({ length: 6 }, (_, i) => ({ id: `mod-${i}` }));
    const view = await mount(
      <BrowseVirtualGrid
        items={items}
        scrollRef={scrollRef}
        minCardPx={160}
        fixedRowHeight
        fillTrailingSkeletons
        getKey={(item) => item.id}
        renderItem={(item) => <div className="real-card">{item.id}</div>}
        renderSkeletonItem={(index) => <div className="skeleton-card">{index}</div>}
      />,
      scrollEl,
    );

    expect(scrollEl.querySelectorAll(".real-card")).toHaveLength(6);
    expect(scrollEl.querySelectorAll(".skeleton-card")).toHaveLength(2);

    await view.unmount();
    scrollEl.remove();
  });

  it("updates scrollMargin when content above BrowseVirtualList in the scroll wrapper resizes", async () => {
    const scrollEl = document.createElement("div");
    setClientSize(scrollEl, 800, 600);
    document.body.appendChild(scrollEl);
    const scrollRef = createRef<HTMLElement | null>();
    scrollRef.current = scrollEl;

    let listOffsetTop = 280;
    vi.spyOn(scrollEl, "getBoundingClientRect").mockImplementation(
      () => new DOMRect(0, 100, 800, 600),
    );

    const items = Array.from({ length: 40 }, (_, i) => ({ id: `mod-${i}` }));
    const view = await mount(
      <div className="detail-scroll-content">
        <div className="detail-hero" />
        <div className="detail-body">
          <div className="mods-toolbar" />
          <div className="results-fill">
            <BrowseVirtualList
              items={items}
              scrollRef={scrollRef}
              active={true}
              estimateSize={60}
              rowClassName="row"
              getKey={(item) => item.id}
              renderItem={(item) => <span>{item.id}</span>}
            />
          </div>
        </div>
      </div>,
      scrollEl,
    );

    const contentRoot = scrollEl.querySelector(".detail-scroll-content")!;
    const listEl = scrollEl.querySelector("ul.browse-virtual") as HTMLUListElement;
    vi.spyOn(listEl, "getBoundingClientRect").mockImplementation(
      () => new DOMRect(0, 100 + listOffsetTop - scrollEl.scrollTop, 800, 2400),
    );

    const marginObserver = FakeResizeObserver.instances.find((ro) =>
      ro.targets.has(contentRoot),
    );
    expect(marginObserver).toBeDefined();

    // Simulate filters expanding above the list (+140px) and scrolling past the hero/toolbar.
    listOffsetTop = 420;
    scrollEl.scrollTop = 420;
    await act(async () => {
      marginObserver!.trigger();
      scrollEl.dispatchEvent(new Event("scroll"));
    });

    const firstRow = scrollEl.querySelector('li[data-index="0"]') as HTMLElement | null;
    expect(firstRow).not.toBeNull();
    expect(firstRow!.style.transform).toBe("translateY(0px)");

    await view.unmount();
    scrollEl.remove();
  });
});
