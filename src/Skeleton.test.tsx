import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { describe, expect, it } from "vitest";

import {
  BrowseGridSkeleton,
  BrowseListSkeleton,
  BrowseResultsSkeleton,
  STAGED_MODS_SKELETON_ROWS,
  StagedModsSkeleton,
} from "./Skeleton";

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

describe("loading skeletons", () => {
  it("shows staged-mod rows with a loading label", async () => {
    const view = await render(<StagedModsSkeleton />);
    expect(view.host.textContent).toContain("Loading staged mods");
    expect(view.host.querySelector("[aria-busy='true']")).not.toBeNull();
    expect(view.host.querySelectorAll("ul.list.mods > li")).toHaveLength(
      STAGED_MODS_SKELETON_ROWS,
    );
    expect(view.host.querySelectorAll(".list-poster")).toHaveLength(
      STAGED_MODS_SKELETON_ROWS,
    );
    await view.unmount();
  });

  it("shows a browse grid with the requested card count", async () => {
    const view = await render(<BrowseGridSkeleton count={6} minCardPx={180} />);
    expect(view.host.textContent).toContain("Searching");
    expect(view.host.querySelector("[aria-busy='true']")).not.toBeNull();
    expect(view.host.querySelectorAll(".media-card")).toHaveLength(6);
    expect(view.host.querySelectorAll(".media-cover")).toHaveLength(6);
    const grid = view.host.querySelector(".browse-grid") as HTMLElement | null;
    expect(grid?.style.getPropertyValue("--browse-card-min")).toBe("180px");
    await view.unmount();
  });

  it("shows compact browse rows or poster rows", async () => {
    const compact = await render(<BrowseListSkeleton count={4} />);
    expect(compact.host.textContent).toContain("Searching");
    expect(compact.host.querySelectorAll("ul.list > li")).toHaveLength(4);
    expect(compact.host.querySelectorAll(".list-poster")).toHaveLength(0);
    await compact.unmount();

    const posters = await render(<BrowseListSkeleton count={3} posters />);
    expect(posters.host.querySelectorAll("ul.list > li")).toHaveLength(3);
    expect(posters.host.querySelectorAll(".list-row-poster")).toHaveLength(3);
    expect(posters.host.querySelectorAll(".list-poster")).toHaveLength(3);
    await posters.unmount();
  });

  it("uses two rows when browse is loading another page", async () => {
    const list = await render(<BrowseResultsSkeleton view="list" rows={2} />);
    expect(list.host.querySelectorAll("ul.list > li")).toHaveLength(2);
    await list.unmount();

    const grid = await render(
      <BrowseResultsSkeleton view="grid" count={2} minCardPx={160} />,
    );
    expect(grid.host.querySelectorAll(".media-card")).toHaveLength(2);
    await grid.unmount();
  });
});
