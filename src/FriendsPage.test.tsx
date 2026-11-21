import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { describe, expect, it, vi, beforeEach } from "vitest";
import { listen } from "@tauri-apps/api/event";
import { FriendsPage } from "./FriendsPage";
import { api } from "./api";
import type { FriendsState } from "./types";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn().mockResolvedValue(() => {}),
}));

vi.mock("./api", () => ({
  api: {
    getFriendsState: vi.fn(),
    setP2pDisplayName: vi.fn(),
    setCollectionVisibility: vi.fn(),
    addFriendByCode: vi.fn(),
    respondFriendRequest: vi.fn(),
    removeFriend: vi.fn(),
    refreshFriends: vi.fn(),
    decodeFriendShareCode: vi.fn(),
    joinOrSyncFriendCollection: vi.fn(),
    finalizeFriendCollectionSync: vi.fn(),
    setFriendCollectionAutoSync: vi.fn(),
    listInstalledCollections: vi.fn().mockResolvedValue([]),
    resolveCatalogCovers: vi.fn().mockResolvedValue([]),
  },
}));

const sampleState: FriendsState = {
  endpoint_ready: true,
  appear_offline: false,
  peer_id: "peer_self",
  join_code: "emperor-friend1:abc",
  display_name: "PlayerOne",
  display_name_set: true,
  profile_picture: null,
  pending_requests: [],
  local_collections: [],
  friends: [
    {
      peer_id: "peer_alice",
      display_name: "Alice",
      profile_picture: "data:image/png;base64,iVBORw0KGgo=",
      status: "connected",
      online: false,
      last_seen_at: "2026-11-20T12:00:00Z",
      cached_profile: {
        peer_id: "peer_alice",
        display_name: "Alice",
        updated_at: "2026-11-20T12:00:00Z",
        collections: [
          {
            id: "active:skyrim",
            kind: "active_loadout",
            name: "Skyrim Loadout",
            share_code: "#emperor1test",
            share_hash: "hash1",
            updated_at: "2026-11-20T12:00:00Z",
          },
        ],
      },
    },
  ],
};

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

describe("FriendsPage", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(api.getFriendsState).mockResolvedValue(sampleState);
    vi.mocked(api.decodeFriendShareCode).mockResolvedValue({
      name: "Skyrim Loadout",
      game: { id: "skyrim", title: "Skyrim SE" },
      mods: [
        {
          s: "nexus",
          mod_id: 10,
          file_id: 20,
          display_name: "SkyUI",
          version: "5.2",
          fomod: { selected: { g1: ["p1"] } },
        },
      ],
      nexus_count: 1,
      thunderstore_count: 0,
      modio_count: 0,
      code_len: 20,
      size_warning: false,
    });
  });

  it("renders offline cached friend collections and decodes FOMOD badge", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <FriendsPage
          active={true}
          managed={[]}
          activeGameId={null}
          onError={() => {}}
          onNotice={() => {}}
          onCollectionSynced={() => {}}
        />,
      );
    });
    await flush();

    expect(host.textContent).toContain("Alice");
    expect(host.textContent).not.toContain("Sharing Privacy");
    expect(host.textContent).not.toContain("Shared with Friends");
    expect(host.textContent).not.toContain("P2P Ready");
    const mainRefreshBtn = host.querySelector(
      'button.friends-refresh-btn[aria-label="Refresh"]',
    );
    expect(mainRefreshBtn).toBeTruthy();
    expect(mainRefreshBtn?.textContent?.trim()).toBe("");

    const aliceBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Alice"),
    );
    expect(aliceBtn).toBeTruthy();
    await act(async () => {
      aliceBtn!.click();
    });
    await flush();

    const libraryActions = host.querySelector(".friends-library-head-actions");
    expect(libraryActions).toBeTruthy();
    const titleHeading = host.querySelector(".detail-head-title h1");
    expect(titleHeading?.textContent).toBe("Friends Collections");
    const actionChildren = [...(libraryActions?.children ?? [])];
    expect(actionChildren).toHaveLength(3);
    expect(actionChildren[0].classList.contains("friends-refresh-btn")).toBe(true);
    expect(actionChildren[1].tagName).toBe("H1");
    expect(actionChildren[1].textContent).toContain("Alice");
    expect(actionChildren[2].classList.contains("friends-avatar-md")).toBe(true);

    expect(host.textContent).toContain("Skyrim SE");
    expect(host.textContent).toContain("1 collection");
    const gameHeader = host.querySelector(".collections-game-header") as HTMLElement;
    expect(gameHeader).toBeTruthy();
    await act(async () => {
      gameHeader.click();
    });
    await flush();

    expect(host.textContent).toContain("Skyrim Loadout");
    const colRow = [...host.querySelectorAll(".collections-game-children li")].find(
      (li) => li.textContent?.includes("Skyrim Loadout"),
    ) as HTMLElement;
    expect(colRow).toBeTruthy();
    await act(async () => {
      colRow.click();
    });
    await flush();

    expect(host.textContent).toContain("SkyUI");
    expect(host.textContent).toContain("FOMOD Config");
    expect(host.textContent).toContain("Contained mods");
    expect(host.textContent).toContain("Code");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("reuses CollectionDetailView with Install, Install & Join Synced, and Save buttons", async () => {
    const onInstallCollection = vi.fn().mockResolvedValue(undefined);
    const onSaveCollection = vi.fn().mockResolvedValue(undefined);
    const onCollectionSynced = vi.fn().mockResolvedValue(undefined);
    vi.mocked(api.joinOrSyncFriendCollection).mockResolvedValue({
      collection_id: "col_1",
      name: "Skyrim Loadout",
      share_code: "#emperor1test",
      share_hash: "hash1",
      member_ids: ["m1"],
      needs_assist: [],
      warnings: [],
      collection: null,
    });

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <FriendsPage
          active={true}
          managed={[
            {
              id: "skyrim",
              plugin_id: "skyrimse",
              title: "Skyrim SE",
              install_dir: "/games/skyrim",
              launcher: "Steam",
              nexus_domain: "skyrimspecialedition",
            } as never,
          ]}
          activeGameId="skyrim"
          onError={() => {}}
          onNotice={() => {}}
          onInstallCollection={onInstallCollection}
          onSaveCollection={onSaveCollection}
          onCollectionSynced={onCollectionSynced}
        />,
      );
    });
    await flush();

    const aliceBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Alice"),
    );
    await act(async () => {
      aliceBtn!.click();
    });
    await flush();

    const gameHeader = host.querySelector(".collections-game-header") as HTMLElement;
    expect(gameHeader).toBeTruthy();
    expect(gameHeader.textContent).toContain("Skyrim SE");
    expect(gameHeader.textContent).toContain("managed");
    await act(async () => {
      gameHeader.click();
    });
    await flush();

    const colRow = [...host.querySelectorAll(".collections-game-children li")].find(
      (li) => li.textContent?.includes("Skyrim Loadout"),
    ) as HTMLElement;
    expect(colRow).toBeTruthy();
    await act(async () => {
      colRow.click();
    });
    await flush();

    const installBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Install",
    );
    const joinSyncBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Install & Join Synced",
    );
    const saveBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Save",
    );

    expect(installBtn).toBeTruthy();
    expect(joinSyncBtn).toBeTruthy();
    expect(saveBtn).toBeTruthy();

    await act(async () => {
      installBtn!.click();
    });
    await flush();
    expect(onInstallCollection).toHaveBeenCalledWith(
      "skyrim",
      "#emperor1test",
      "Skyrim Loadout",
    );

    await act(async () => {
      joinSyncBtn!.click();
    });
    await flush();
    expect(api.joinOrSyncFriendCollection).toHaveBeenCalledWith({
      gameId: "skyrim",
      peerId: "peer_alice",
      friendCollectionId: "active:skyrim",
    });
    expect(onCollectionSynced).toHaveBeenCalledTimes(1);

    await act(async () => {
      saveBtn!.click();
    });
    await flush();
    expect(onSaveCollection).toHaveBeenCalledWith(
      "#emperor1test",
      "Skyrim Loadout",
      "skyrim",
    );

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("shows only display name onboarding when display_name_set is false", async () => {
    vi.mocked(api.getFriendsState).mockResolvedValue({
      ...sampleState,
      display_name: "",
      display_name_set: false,
    });
    vi.mocked(api.setP2pDisplayName).mockResolvedValueOnce({
      ...sampleState,
      display_name: "ChosenName",
      display_name_set: true,
    });

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <FriendsPage
          active={true}
          managed={[]}
          activeGameId={null}
          onError={() => {}}
          onNotice={() => {}}
          onCollectionSynced={() => {}}
        />,
      );
    });
    await flush();

    expect(host.textContent).toContain("Set Your Display Name");
    expect(host.textContent).not.toContain("Connected Friends");
    expect(host.textContent).not.toContain("Share Friend Code");
    expect(host.textContent).not.toContain("Add Friend");

    const input = host.querySelector<HTMLInputElement>(
      'input[aria-label="Your display name"]',
    );
    expect(input).toBeTruthy();

    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(input, "ChosenName");
      input!.dispatchEvent(new Event("input", { bubbles: true }));
    });

    const form = host.querySelector("form");
    await act(async () => {
      form!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    await flush();

    expect(api.setP2pDisplayName).toHaveBeenCalledWith("ChosenName");
    expect(host.textContent).toContain("Connected Friends");
    expect(host.textContent).toContain("Share Friend Code");
    expect(host.querySelector('input[aria-label="Your display name"]')).toBeNull();

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("uses Share Friend Code button, Add Friend modal, and friend cards with avatars", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    vi.mocked(api.addFriendByCode).mockResolvedValueOnce(sampleState);

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <FriendsPage
          active={true}
          managed={[]}
          activeGameId={null}
          onError={() => {}}
          onNotice={() => {}}
          onCollectionSynced={() => {}}
        />,
      );
    });
    await flush();

    // No raw join code box on page.
    expect(host.querySelector(".friends-join-code")).toBeNull();

    // Friend card renders with avatar image.
    const card = host.querySelector(".friends-friend-card");
    expect(card).toBeTruthy();
    const avatarImg = card!.querySelector<HTMLImageElement>("img.friends-avatar-img");
    expect(avatarImg?.src).toBe("data:image/png;base64,iVBORw0KGgo=");

    // Share Friend Code copies clickable HTTPS invite link.
    const shareBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Share Friend Code"),
    );
    expect(shareBtn).toBeTruthy();
    await act(async () => {
      shareBtn!.click();
    });
    await flush();
    expect(writeText).toHaveBeenCalledWith(
      "https://emperor-of-modkind.github.io/open/?to=friend/emperor-friend1:abc",
    );

    // Add Friend opens modal and submits join code (or pasted invite link).
    const addBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Add Friend"),
    );
    expect(addBtn).toBeTruthy();
    await act(async () => {
      addBtn!.click();
    });
    await flush();

    const modalInput = document.body.querySelector<HTMLInputElement>(
      'input[aria-label="Friend join code"]',
    );
    expect(modalInput).toBeTruthy();
    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(
        modalInput,
        "https://emperor-of-modkind.github.io/open/?to=friend/emperor-friend1:xyz",
      );
      modalInput!.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const modalForm = document.body.querySelector("#add-friend-modal-form");
    await act(async () => {
      modalForm!.dispatchEvent(
        new Event("submit", { bubbles: true, cancelable: true }),
      );
    });
    await flush();
    expect(api.addFriendByCode).toHaveBeenCalledWith("emperor-friend1:xyz");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("opens the Add Friend modal prefilled when a friend deep link arrives", async () => {
    vi.mocked(api.getFriendsState).mockResolvedValue(sampleState);
    const onConsume = vi.fn();
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);

    await act(async () => {
      root.render(
        <FriendsPage
          active
          managed={[]}
          activeGameId={null}
          pendingJoinCode="emperor-friend1:deepLink123"
          onConsumePendingJoinCode={onConsume}
          onError={vi.fn()}
          onNotice={vi.fn()}
          onCollectionSynced={vi.fn()}
        />,
      );
    });
    await flush();

    expect(onConsume).toHaveBeenCalledTimes(1);
    const modalInput = document.body.querySelector<HTMLInputElement>(
      'input[aria-label="Friend join code"]',
    );
    expect(modalInput?.value).toBe("emperor-friend1:deepLink123");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("spins and disables the refresh button while friends-checking is active", async () => {
    const handlers = new Map<string, (e: { payload: unknown }) => void>();
    vi.mocked(listen).mockImplementation(async (event, cb) => {
      handlers.set(event, cb as (e: { payload: unknown }) => void);
      return () => {
        handlers.delete(event);
      };
    });

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <FriendsPage
          active
          managed={[]}
          activeGameId={null}
          onError={() => {}}
          onNotice={() => {}}
          onCollectionSynced={async () => {}}
        />,
      );
    });
    await flush();

    const refreshBtn = host.querySelector<HTMLButtonElement>(
      "button.friends-refresh-btn",
    );
    expect(refreshBtn).toBeTruthy();
    expect(refreshBtn!.disabled).toBe(false);
    expect(refreshBtn!.querySelector("svg")?.classList.contains("fa-spin")).toBe(
      false,
    );

    await act(async () => {
      handlers.get("friends-checking")?.({ payload: true });
    });
    expect(refreshBtn!.disabled).toBe(true);
    expect(refreshBtn!.querySelector("svg")?.classList.contains("fa-spin")).toBe(
      true,
    );

    await act(async () => {
      handlers.get("friends-checking")?.({ payload: false });
    });
    expect(refreshBtn!.disabled).toBe(false);
    expect(refreshBtn!.querySelector("svg")?.classList.contains("fa-spin")).toBe(
      false,
    );

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("resolves catalog posters for unmanaged friend collection games", async () => {
    vi.mocked(api.decodeFriendShareCode).mockResolvedValueOnce({
      name: "Cozy Farm",
      game: {
        nexus_domain: "stardewvalley",
        title: "Stardew Valley",
      },
      mods: [],
      nexus_count: 1,
      thunderstore_count: 0,
      modio_count: 0,
      code_len: 16,
      size_warning: false,
    });
    vi.mocked(api.resolveCatalogCovers).mockResolvedValueOnce([
      {
        id: "collection:nexus:stardewvalley",
        cover_url: "https://images.nexusmods.com/images/games/v2/1303/tile.jpg",
        title: "Stardew Valley",
      },
    ]);

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <FriendsPage
          active
          managed={[]}
          activeGameId={null}
          onError={() => {}}
          onNotice={() => {}}
          onCollectionSynced={async () => {}}
        />,
      );
    });
    await flush();

    const friendCard = Array.from(host.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Alice"),
    );
    expect(friendCard).toBeTruthy();

    await act(async () => {
      friendCard!.click();
    });
    await flush();

    expect(api.resolveCatalogCovers).toHaveBeenCalledWith([
      {
        id: "collection:nexus:stardewvalley",
        nexus_domain: "stardewvalley",
        modio_game_id: null,
        icon_path: null,
      },
    ]);

    const posterImg = host.querySelector<HTMLImageElement>(".list-poster img");
    expect(posterImg).toBeTruthy();
    expect(posterImg!.getAttribute("src")).toBe(
      "https://images.nexusmods.com/images/games/v2/1303/tile.jpg",
    );

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });
});
