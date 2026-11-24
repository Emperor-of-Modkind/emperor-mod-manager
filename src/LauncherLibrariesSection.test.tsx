import { createRoot, type Root } from "react-dom/client";
import { act, createRef } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { LauncherLibrariesSection } from "./LauncherLibrariesSection";
import type { LibraryGame, LibrarySection } from "./types";

function makeGame(partial: Partial<LibraryGame> & Pick<LibraryGame, "id" | "title">): LibraryGame {
  return {
    source: "steam",
    launcher: "Steam",
    external_id: partial.id,
    supported: false,
    plugin_id: null,
    nexus_domain: null,
    cover_path: null,
    cover_url: null,
    install_size_bytes: null,
    platforms: ["windows"],
    can_cli_install: false,
    metadata: { dlcs: [], languages: [] },
    ...partial,
  };
}

const sections: LibrarySection[] = [
  {
    source: "steam",
    label: "Steam",
    total_owned: 2,
    warning: "Showing locally cached Steam titles.",
    games: [
      makeGame({ id: "489830", title: "Skyrim Special Edition", supported: true }),
      makeGame({ id: "999", title: "Random Indie", supported: false }),
    ],
  },
  {
    source: "heroic_epic",
    label: "Heroic — Epic",
    total_owned: 1,
    warning: null,
    games: [
      makeGame({
        id: "Crab",
        title: "Control",
        source: "heroic_epic",
        launcher: "Heroic (Epic)",
        supported: true,
        can_cli_install: true,
      }),
    ],
  },
];

describe("LauncherLibrariesSection", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    try {
      globalThis.localStorage?.removeItem?.("emm.librarySections.expanded");
    } catch {
      // ignore in test env without localStorage
    }
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("renders collapsed per-launcher sections with counts and expands on click", async () => {
    const scrollRef = createRef<HTMLDivElement>();
    await act(async () => {
      root.render(
        <LauncherLibrariesSection
          sections={sections}
          loading={false}
          active
          scrollRef={scrollRef}
          installsByGameId={{}}
          resolveCoverSrc={() => null}
          onSelectGame={() => {}}
          onRefresh={() => {}}
          onOpenSettings={() => {}}
        />,
      );
    });

    const steamToggle = host.querySelector<HTMLButtonElement>(
      '[data-testid="library-section-steam"] .launcher-library-toggle',
    );
    expect(steamToggle).not.toBeNull();
    expect(steamToggle?.getAttribute("aria-expanded")).toBe("false");
    expect(steamToggle?.textContent).toContain("Steam");
    expect(steamToggle?.textContent).toContain("2 uninstalled");

    await act(async () => {
      steamToggle?.click();
    });
    expect(steamToggle?.getAttribute("aria-expanded")).toBe("true");
    expect(host.textContent).toContain("Showing locally cached Steam titles.");
  });

  it("opens collapsible filter bar with search, Supported only switch, and clear filters", async () => {
    const scrollRef = createRef<HTMLDivElement>();
    await act(async () => {
      root.render(
        <LauncherLibrariesSection
          sections={sections}
          loading={false}
          active
          scrollRef={scrollRef}
          installsByGameId={{}}
          resolveCoverSrc={() => null}
          onSelectGame={() => {}}
          onRefresh={() => {}}
          onOpenSettings={() => {}}
        />,
      );
    });

    const filtersToggle = host.querySelector<HTMLButtonElement>('button[aria-label="Filters"]');
    expect(filtersToggle).not.toBeNull();
    expect(filtersToggle?.getAttribute("aria-expanded")).toBe("false");
    expect(host.querySelector('input[aria-label="Filter uninstalled games"]')).toBeNull();

    await act(async () => {
      filtersToggle?.click();
    });

    expect(filtersToggle?.getAttribute("aria-expanded")).toBe("true");
    const searchInput = host.querySelector<HTMLInputElement>(
      'input[aria-label="Filter uninstalled games"]',
    );
    const supportedSwitch = host.querySelector<HTMLButtonElement>('button[role="switch"]');
    expect(searchInput).not.toBeNull();
    expect(supportedSwitch).not.toBeNull();
    expect(supportedSwitch?.getAttribute("aria-checked")).toBe("false");

    await act(async () => {
      supportedSwitch?.click();
    });

    expect(supportedSwitch?.getAttribute("aria-checked")).toBe("true");
    expect(filtersToggle?.getAttribute("aria-pressed")).toBe("true");
    const steamToggle = host.querySelector<HTMLButtonElement>(
      '[data-testid="library-section-steam"] .launcher-library-toggle',
    );
    expect(steamToggle?.textContent).toContain("1 / 2 uninstalled");

    const clearBtn = host.querySelector<HTMLButtonElement>(".filters-clear");
    expect(clearBtn).not.toBeNull();
    await act(async () => {
      clearBtn?.click();
    });

    expect(supportedSwitch?.getAttribute("aria-checked")).toBe("false");
    expect(filtersToggle?.getAttribute("aria-pressed")).toBe("false");
    expect(steamToggle?.textContent).toContain("2 uninstalled");

    const syncBtn = Array.from(host.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Sync libraries"),
    );
    const onRefresh = vi.fn();
    await act(async () => {
      root.render(
        <LauncherLibrariesSection
          sections={sections}
          loading={false}
          active
          scrollRef={scrollRef}
          installsByGameId={{}}
          resolveCoverSrc={() => null}
          onSelectGame={() => {}}
          onRefresh={onRefresh}
          onOpenSettings={() => {}}
        />,
      );
    });
    await act(async () => {
      syncBtn?.click();
    });
    expect(onRefresh).toHaveBeenCalledWith(true);
  });
});
