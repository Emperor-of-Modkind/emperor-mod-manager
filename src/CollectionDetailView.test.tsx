import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  CollectionDetailView,
  EditableCollectionTitle,
} from "./CollectionDetailView";
import type { ManagedGame, ShareModEntry } from "./types";

const cyberpunk = {
  id: "cp2077",
  plugin_id: "cyberpunk2077",
  title: "Cyberpunk 2077",
  install_dir: "/games/cp2077",
  launcher: "Steam",
  nexus_domain: "cyberpunk2077",
} as unknown as ManagedGame;

const cyberpunkGog = {
  id: "cp2077_gog",
  plugin_id: "cyberpunk2077",
  title: "Cyberpunk 2077 (GOG)",
  install_dir: "/games/cp2077-gog",
  launcher: "GOG",
  nexus_domain: "cyberpunk2077",
} as unknown as ManagedGame;

const skyrim = {
  id: "skyrim",
  plugin_id: "skyrimse",
  title: "Skyrim SE",
  install_dir: "/games/skyrim",
  launcher: "Steam",
  nexus_domain: "skyrimspecialedition",
} as unknown as ManagedGame;

function renderDetail(props: Partial<Parameters<typeof CollectionDetailView>[0]>) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root: Root = createRoot(host);
  act(() => {
    root.render(
      <CollectionDetailView
        title="Test Pack"
        backLabel="Back"
        onBack={vi.fn()}
        gameTitle="Cyberpunk 2077"
        stats={{
          mod_count: 3,
          nexus_count: 3,
          thunderstore_count: 0,
          modio_count: 0,
          updated_at: "2026-01-01T00:00:00Z",
        }}
        managed={[cyberpunk, skyrim]}
        selectedGameId="cp2077"
        onSelectGameId={vi.fn()}
        gameMatches={(g) => g.nexus_domain === "cyberpunk2077"}
        gameKnown={true}
        actions={<button type="button">Install</button>}
        code="#emperor1test"
        mods={[]}
        showMods={false}
        active={true}
        {...props}
      />,
    );
  });
  return {
    host,
    cleanup: () => {
      act(() => root.unmount());
      host.remove();
    },
  };
}

describe("CollectionDetailView", () => {
  it("hides the game selector when the game is known and exactly one managed game matches", () => {
    const { host, cleanup } = renderDetail({});
    expect(host.querySelector("select.saved-collection-game-select")).toBeNull();
    const installBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Install",
    );
    expect(installBtn).toBeTruthy();
    cleanup();
  });

  it("scopes the game selector to matching instances when multiple managed games match", () => {
    const { host, cleanup } = renderDetail({
      managed: [cyberpunk, cyberpunkGog, skyrim],
    });
    const select = host.querySelector(
      "select.saved-collection-game-select",
    ) as HTMLSelectElement | null;
    expect(select).toBeTruthy();
    const optionTexts = [...select!.options].map((o) => o.textContent);
    expect(optionTexts).toEqual([
      "Select managed game…",
      "Cyberpunk 2077",
      "Cyberpunk 2077 (GOG)",
    ]);
    cleanup();
  });

  it("shows noGamesFallback instead of unrelated games when the game is known but unmanaged", () => {
    const { host, cleanup } = renderDetail({
      managed: [skyrim],
      selectedGameId: "",
    });
    expect(host.querySelector("select.saved-collection-game-select")).toBeNull();
    expect(host.textContent).toContain("Manage this game in Library to install.");
    cleanup();
  });

  it("shows all managed games in the selector when the collection game is unknown", () => {
    const { host, cleanup } = renderDetail({
      managed: [cyberpunk, skyrim],
      selectedGameId: "",
      gameKnown: false,
      gameMatches: () => false,
    });
    const select = host.querySelector(
      "select.saved-collection-game-select",
    ) as HTMLSelectElement | null;
    expect(select).toBeTruthy();
    const optionTexts = [...select!.options].map((o) => o.textContent);
    expect(optionTexts).toEqual([
      "Select managed game…",
      "Cyberpunk 2077",
      "Skyrim SE",
    ]);
    cleanup();
  });

  it("renders contained mods as media cards and opens a mod on click", () => {
    const onOpenMod = vi.fn();
    const mods: ShareModEntry[] = [
      {
        s: "nexus",
        domain: "cyberpunk2077",
        mod_id: 2380,
        file_id: 168077,
        display_name: "RED4ext",
        version: "1.31.0",
        fomod: { selected: {} },
      },
      {
        s: "thunderstore",
        community: "riskofrain2",
        namespace: "bbepis",
        name: "BepInExPack",
        display_name: "BepInExPack",
        version: "5.4.2100",
      },
    ];
    const { host, cleanup } = renderDetail({
      mods,
      showMods: true,
      onOpenMod,
    });

    expect(host.querySelector(".media-grid.browse-grid")).toBeTruthy();
    const cards = [...host.querySelectorAll(".media-card")];
    expect(cards).toHaveLength(2);
    expect(cards[0].textContent).toContain("RED4ext");
    expect(cards[0].textContent).toContain("FOMOD Config");
    expect(cards[0].querySelector(".media-badge")?.textContent).toBe(
      "v1.31.0 · cyberpunk2077 2380/168077",
    );
    expect(cards[0].querySelector(".media-overlay")).toBeNull();
    expect(cards[0].querySelector(".media-card-meta")).toBeNull();
    expect(cards[1].textContent).toContain("BepInExPack");

    act(() => {
      (cards[0] as HTMLElement).click();
    });
    expect(onOpenMod).toHaveBeenCalledTimes(1);
    expect(onOpenMod.mock.calls[0][0]).toMatchObject({
      source: "nexus",
      domain_name: "cyberpunk2077",
      mod_id: 2380,
      name: "RED4ext",
    });
    cleanup();
  });

  it("toggles the filter row and filters contained mods by query and source", async () => {
    const mods: ShareModEntry[] = [
      {
        s: "nexus",
        domain: "cyberpunk2077",
        mod_id: 2380,
        file_id: 168077,
        display_name: "RED4ext",
        version: "1.31.0",
      },
      {
        s: "thunderstore",
        community: "riskofrain2",
        namespace: "bbepis",
        name: "BepInExPack",
        display_name: "BepInExPack",
        version: "5.4.2100",
      },
    ];
    const { host, cleanup } = renderDetail({
      mods,
      showMods: true,
    });

    const filterBtn = host.querySelector(
      'button[aria-label="Filters"]',
    ) as HTMLButtonElement | null;
    expect(filterBtn).toBeTruthy();
    expect(filterBtn!.getAttribute("aria-expanded")).toBe("false");

    act(() => {
      filterBtn!.click();
    });
    expect(filterBtn!.getAttribute("aria-expanded")).toBe("true");

    const searchInput = host.querySelector(
      'input[aria-label="Search contained mods"]',
    ) as HTMLInputElement | null;
    expect(searchInput).toBeTruthy();

    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!;
      setter.call(searchInput, "RED4");
      searchInput!.dispatchEvent(new Event("input", { bubbles: true }));
    });

    expect(host.querySelectorAll(".media-card")).toHaveLength(1);
    expect(host.textContent).toContain("Showing 1 of 2");

    const clearBtn = host.querySelector(
      "button.filters-clear",
    ) as HTMLButtonElement | null;
    expect(clearBtn).toBeTruthy();
    act(() => {
      clearBtn!.click();
    });
    expect(host.querySelectorAll(".media-card")).toHaveLength(2);

    const tsBtn = host.querySelector(
      '.source-filter button[aria-label="Thunderstore"]',
    ) as HTMLButtonElement | null;
    expect(tsBtn).toBeTruthy();
    act(() => {
      tsBtn!.click();
    });
    expect(host.querySelectorAll(".media-card")).toHaveLength(1);
    expect(host.querySelector(".media-card")?.textContent).toContain("BepInExPack");

    cleanup();
  });
});

describe("EditableCollectionTitle", () => {
  it("supports double-click and pencil rename in compact strong mode", async () => {
    const onRename = vi.fn(async () => {});
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    act(() => {
      root.render(
        <EditableCollectionTitle
          as="strong"
          stopRowClick
          title="Old Pack"
          onRename={onRename}
        />,
      );
    });

    const titleEl = host.querySelector("strong");
    expect(titleEl?.textContent).toBe("Old Pack");

    // Pencil button enters edit mode
    const pencilBtn = host.querySelector(
      "button.collection-rename-btn.is-compact",
    ) as HTMLButtonElement | null;
    expect(pencilBtn).toBeTruthy();
    act(() => {
      pencilBtn!.click();
    });

    const input = host.querySelector(
      "input.collection-title-input.is-compact",
    ) as HTMLInputElement | null;
    expect(input).toBeTruthy();
    expect(input!.value).toBe("Old Pack");

    // Escape cancels without calling onRename
    act(() => {
      input!.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Escape", bubbles: true }),
      );
    });
    expect(onRename).not.toHaveBeenCalled();

    // Double-click enters edit mode and Enter commits
    const titleAfter = host.querySelector("strong")!;
    act(() => {
      titleAfter.dispatchEvent(new MouseEvent("dblclick", { bubbles: true }));
    });
    const input2 = host.querySelector(
      "input.collection-title-input.is-compact",
    ) as HTMLInputElement;
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )!.set!;
      setter.call(input2, "New Pack");
      input2.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await act(async () => {
      input2.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    });
    expect(onRename).toHaveBeenCalledWith("New Pack");

    act(() => {
      root.unmount();
    });
    host.remove();
  });
});
