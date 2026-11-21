import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  CollectionDetailView,
  EditableCollectionTitle,
} from "./CollectionDetailView";
import type { ManagedGame } from "./types";

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
