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
    addFriendByPeer: vi.fn(),
    cancelQueuedFriendInvite: vi.fn(),
    respondFriendRequest: vi.fn(),
    removeFriend: vi.fn(),
    refreshFriends: vi.fn(),
    markFriendFeedRead: vi.fn(),
    clearFriendFeed: vi.fn(),
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

async function clickFriendsTab(host: HTMLElement, label: string) {
  const btn = [
    ...host.querySelectorAll<HTMLButtonElement>(".friends-tabs-row button"),
  ].find((b) => b.textContent?.trim().startsWith(label));
  expect(btn, `expected tab button "${label}"`).toBeTruthy();
  await act(async () => {
    btn!.click();
  });
  await flush();
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

    // Friend Feed is the default tab; switch to Friends to inspect friend cards.
    expect(host.textContent).toContain("Friend Feed");
    expect(host.textContent).not.toContain("Find Players");
    await clickFriendsTab(host, "Friends");

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

    // Friend profile defaults to Feed tab; switch to Collections.
    expect(host.textContent).toContain("Alice's Feed");
    await clickFriendsTab(host, "Collections");

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

    await clickFriendsTab(host, "Friends");
    const aliceBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Alice"),
    );
    await act(async () => {
      aliceBtn!.click();
    });
    await flush();
    await clickFriendsTab(host, "Collections");

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
    expect(host.textContent).toContain("Friend Feed");
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

    // Switch to Friends tab to inspect friend cards.
    await clickFriendsTab(host, "Friends");

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

    await clickFriendsTab(host, "Friends");
    const friendCard = Array.from(host.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Alice"),
    );
    expect(friendCard).toBeTruthy();

    await act(async () => {
      friendCard!.click();
    });
    await flush();
    await clickFriendsTab(host, "Collections");

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

  it("switches to Friend Feed tab, shows unread badge & mod diffs, marks read, and opens collection detail", async () => {
    const feedState: FriendsState = {
      ...sampleState,
      feed_last_read_at: "2026-11-20T12:00:00Z",
      feed_events: [
        {
          id: "ev_1",
          peer_id: "peer_alice",
          friend_display_name: "Alice",
          kind: "collection_updated",
          collection_id: "active:skyrim",
          collection_kind: "active_loadout",
          collection_name: "Skyrim Loadout",
          game_title: "Skyrim SE",
          mod_count: 2,
          share_hash: "hash1",
          diff: {
            mods_added: ["USSEP"],
            mods_added_count: 1,
            mods_removed: ["OldMod"],
            mods_removed_count: 1,
            mods_updated: ["SkyUI"],
            mods_updated_count: 1,
          },
          occurred_at: "2026-11-21T12:00:00Z",
        },
        {
          id: "ev_hidden",
          peer_id: "peer_alice",
          friend_display_name: "Alice",
          kind: "collection_added",
          collection_id: "saved:revoked",
          collection_kind: "saved_collection",
          collection_name: "Secret Pack",
          game_title: "Skyrim SE",
          mod_count: 5,
          share_hash: "hash_secret",
          occurred_at: "2026-11-21T12:05:00Z",
        },
      ],
    };
    const markedState: FriendsState = {
      ...feedState,
      feed_last_read_at: "2026-11-21T12:10:00Z",
    };
    const clearedState: FriendsState = {
      ...markedState,
      feed_events: [],
    };
    vi.mocked(api.getFriendsState).mockResolvedValue(feedState);
    vi.mocked(api.markFriendFeedRead).mockResolvedValue(markedState);
    vi.mocked(api.clearFriendFeed).mockResolvedValue(clearedState);

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

    // Friend Feed is active by default, immediately marks read, and filters out revoked/hidden items.
    expect(api.markFriendFeedRead).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("Skyrim Loadout");
    expect(host.textContent).toContain("+1 added: USSEP");
    expect(host.textContent).toContain("-1 removed: OldMod");
    expect(host.textContent).toContain("~1 updated: SkyUI");
    expect(host.textContent).not.toContain("Secret Pack");

    // Clicking the feed item opens CollectionDetailView.
    const itemBtn = host.querySelector<HTMLButtonElement>(".friends-feed-item-btn");
    expect(itemBtn).toBeTruthy();
    await act(async () => {
      itemBtn!.click();
    });
    await flush();

    expect(host.textContent).toContain("Contained mods");
    expect(host.textContent).toContain("SkyUI");

    // Clicking Back returns straight to the Friend Feed tab.
    const backBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Back",
    );
    expect(backBtn).toBeTruthy();
    await act(async () => {
      backBtn!.click();
    });
    await flush();

    expect(host.querySelector(".friends-feed-list")).toBeTruthy();

    // Clear Feed clears the list and renders the empty state.
    const clearBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Clear Feed",
    );
    expect(clearBtn).toBeTruthy();
    await act(async () => {
      clearBtn!.click();
    });
    await flush();

    expect(api.clearFriendFeed).toHaveBeenCalledTimes(1);
    expect(host.textContent).toContain("No recent activity yet.");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("omits Find Players tab, shows mutual friends and friend detail Feed (default) and Friends tabs", async () => {
    const carolId = "c".repeat(64);
    const mutualState: FriendsState = {
      ...sampleState,
      pending_requests: [
        {
          peer_id: "peer_eve",
          display_name: "Eve",
          received_at: "2026-11-21T00:00:00Z",
        },
      ],
      feed_last_read_at: "2026-11-21T12:10:00Z",
      feed_events: [
        {
          id: "ev_alice",
          peer_id: "peer_alice",
          friend_display_name: "Alice",
          kind: "collection_added",
          collection_id: "active:skyrim",
          collection_kind: "active_loadout",
          collection_name: "Skyrim Loadout",
          game_title: "Skyrim SE",
          mod_count: 1,
          share_hash: "hash1",
          occurred_at: "2026-11-21T12:00:00Z",
        },
        {
          id: "ev_bob",
          peer_id: "peer_bob",
          friend_display_name: "Bob",
          kind: "collection_added",
          collection_id: "saved:bobpack",
          collection_kind: "saved_collection",
          collection_name: "Bob Pack",
          game_title: "Baldur's Gate 3",
          mod_count: 3,
          share_hash: "hash_bob",
          occurred_at: "2026-11-21T12:01:00Z",
        },
      ],
      friends: [
        {
          ...sampleState.friends[0],
          cached_profile: {
            ...sampleState.friends[0].cached_profile!,
            friends: [
              { peer_id: "peer_bob", display_name: "Bob" },
              { peer_id: carolId, display_name: "Carol" },
            ],
          },
        },
        {
          peer_id: "peer_bob",
          display_name: "Bob",
          status: "connected",
          online: true,
          cached_profile: {
            peer_id: "peer_bob",
            display_name: "Bob",
            updated_at: "2026-11-21T00:00:00Z",
            collections: [
              {
                id: "saved:bobpack",
                kind: "saved_collection",
                name: "Bob Pack",
                share_code: "#emperor1bob",
                share_hash: "hash_bob",
                updated_at: "2026-11-21T12:01:00Z",
              },
            ],
            friends: null,
          },
        },
      ],
    };
    vi.mocked(api.getFriendsState).mockResolvedValue(mutualState);
    vi.mocked(api.addFriendByPeer).mockResolvedValue({
      ...mutualState,
      friends: [
        ...mutualState.friends,
        {
          peer_id: carolId,
          display_name: "Carol",
          status: "pending_outgoing",
          online: false,
        },
      ],
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

    // Find Players tab is removed; Friends tab shows pending request badge.
    expect(host.textContent).not.toContain("Find Players");
    const pendingBadge = host.querySelector(
      '.friends-tabs-row [aria-label="1 pending"]',
    );
    expect(pendingBadge?.textContent?.trim()).toBe("1");

    // Switch to Friends tab.
    await clickFriendsTab(host, "Friends");

    // Alice's card shows 1 mutual friend (Bob).
    expect(host.textContent).toContain("1 mutual");

    const aliceCard = [
      ...host.querySelectorAll<HTMLButtonElement>(".friends-friend-card-main"),
    ].find((b) => b.textContent?.includes("Alice"));
    await act(async () => {
      aliceCard!.click();
    });
    await flush();

    // Alice's profile opens on Feed tab by default, showing only Alice's events (not Bob's).
    expect(host.textContent).toContain("Alice's Feed");
    expect(host.textContent).toContain("Skyrim Loadout");
    expect(host.textContent).not.toContain("Bob Pack");

    // Clicking a feed item on Alice's profile opens CollectionDetailView and Back returns to Alice's Feed.
    const aliceFeedItem = host.querySelector<HTMLButtonElement>(
      ".friends-feed-item-btn",
    );
    expect(aliceFeedItem).toBeTruthy();
    await act(async () => {
      aliceFeedItem!.click();
    });
    await flush();
    expect(host.textContent).toContain("Contained mods");

    const backBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Back",
    );
    await act(async () => {
      backBtn!.click();
    });
    await flush();
    expect(host.textContent).toContain("Alice's Feed");

    // Switch to Alice's Friends sub-tab -> shows Bob (Mutual) and Carol (Add Friend).
    await clickFriendsTab(host, "Friends");
    expect(host.textContent).toContain("Bob");
    expect(host.textContent).toContain("Mutual");
    expect(host.textContent).toContain("Carol");

    // Add Carol directly from Alice's Friends sub-tab.
    const addCarolBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Add Friend"),
    );
    expect(addCarolBtn).toBeTruthy();
    await act(async () => {
      addCarolBtn!.click();
    });
    await flush();
    expect(api.addFriendByPeer).toHaveBeenCalledWith(carolId, "Carol");

    // Click View Profile on mutual friend Bob -> opens on Bob's Feed tab by default.
    const viewBobBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "View Profile",
    );
    expect(viewBobBtn).toBeTruthy();
    await act(async () => {
      viewBobBtn!.click();
    });
    await flush();

    expect(host.textContent).toContain("Bob's Feed");
    expect(host.textContent).toContain("Bob Pack");
    expect(host.textContent).not.toContain("Skyrim Loadout");

    // Switch to Bob's Friends sub-tab -> shows private notice.
    await clickFriendsTab(host, "Friends");
    expect(host.textContent).toContain("Bob's friends list is private.");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("shows which of your collections a friend is syncing on their card and detail view", async () => {
    vi.mocked(api.getFriendsState).mockResolvedValue({
      ...sampleState,
      local_collections: [
        {
          id: "saved:pack1",
          kind: "saved_collection",
          name: "Co-op Essentials",
          game_title: "Skyrim",
          mod_count: 12,
          hidden: false,
        },
      ],
      friends: [
        {
          peer_id: "alice-peer",
          display_name: "Alice",
          status: "connected",
          online: true,
          syncing_my_collections: ["saved:pack1"],
        },
      ],
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

    await clickFriendsTab(host, "Friends");
    expect(host.textContent).toContain("Syncing your: Co-op Essentials");

    const aliceCard = host.querySelector<HTMLButtonElement>(
      ".friends-friend-card-main",
    );
    await act(async () => {
      aliceCard!.click();
    });
    await flush();

    expect(host.textContent).toContain(
      "Syncing your collection: Co-op Essentials",
    );

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("shows when a friend's active loadout matches or is synced to your collection across feed, card, and collections", async () => {
    const activeMatchState: FriendsState = {
      ...sampleState,
      feed_last_read_at: "2026-11-21T02:00:00Z",
      local_collections: [
        {
          id: "saved:pack1",
          kind: "saved_collection",
          name: "Co-op Essentials",
          game_title: "Skyrim",
          mod_count: 1,
          hidden: false,
        },
      ],
      feed_events: [
        {
          id: "ev1",
          peer_id: "alice-peer",
          friend_display_name: "Alice",
          kind: "collection_updated",
          collection_id: "active:skyrim",
          collection_kind: "active_loadout",
          collection_name: "Skyrim Loadout",
          game_title: "Skyrim",
          mod_count: 1,
          share_hash: "hash-active",
          occurred_at: "2026-11-21T01:00:00Z",
        },
      ],
      friends: [
        {
          peer_id: "alice-peer",
          display_name: "Alice",
          status: "connected",
          online: true,
          syncing_my_collections: ["saved:pack1"],
          cached_profile: {
            peer_id: "alice-peer",
            display_name: "Alice",
            updated_at: "2026-11-21T01:00:00Z",
            collections: [
              {
                id: "active:skyrim",
                kind: "active_loadout",
                name: "Skyrim Loadout",
                share_code: "#emperor1test",
                share_hash: "hash-active",
                updated_at: "2026-11-21T01:00:00Z",
                matched_local_collection: {
                  id: "saved:pack1",
                  name: "Co-op Essentials",
                  kind: "synced_to_yours",
                  game_id: "skyrim",
                  has_local_changes: false,
                },
              },
            ],
          },
        },
      ],
    };
    vi.mocked(api.getFriendsState).mockResolvedValue(activeMatchState);
    vi.mocked(api.markFriendFeedRead).mockResolvedValue(activeMatchState);

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

    // Feed tab shows the active loadout match pill.
    expect(host.textContent).toContain('Synced to your "Co-op Essentials"');

    // Friends tab card shows Active loadout: Co-op Essentials.
    await clickFriendsTab(host, "Friends");
    expect(host.textContent).toContain("Active loadout: Co-op Essentials");

    // Friend detail banner shows the active loadout match.
    const aliceCard = host.querySelector<HTMLButtonElement>(
      ".friends-friend-card-main",
    );
    await act(async () => {
      aliceCard!.click();
    });
    await flush();
    expect(host.textContent).toContain(
      'Active loadout: Synced to your "Co-op Essentials"',
    );

    // Collections tab shows the match pill on the Active Loadout row.
    await clickFriendsTab(host, "Collections");
    expect(host.textContent).toContain('Synced to your "Co-op Essentials"');

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("queues friend-of-friend invites when target is offline, shows Queued pill & card, and supports Cancel", async () => {
    const carolId = "c".repeat(64);
    const initialState: FriendsState = {
      ...sampleState,
      peer_id: "0".repeat(64),
      queued_invites: [],
      friends: [
        {
          peer_id: "a".repeat(64),
          display_name: "Alice",
          status: "connected",
          online: true,
          cached_profile: {
            peer_id: "a".repeat(64),
            display_name: "Alice",
            updated_at: "2026-11-22T12:00:00Z",
            collections: [],
            friends: [{ peer_id: carolId, display_name: "Carol", online: false }],
          },
        },
      ],
    };
    const queuedState: FriendsState = {
      ...initialState,
      queued_invites: [
        {
          peer_id: carolId,
          display_name: "Carol",
          queued_at: "2026-11-22T12:05:00Z",
          relayed_via_peer_ids: ["a".repeat(64)],
        },
      ],
    };

    vi.mocked(api.getFriendsState).mockResolvedValue(initialState);
    vi.mocked(api.addFriendByPeer).mockResolvedValue(queuedState);
    vi.mocked(api.cancelQueuedFriendInvite).mockResolvedValue(initialState);

    const onNotice = vi.fn();
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
          onNotice={onNotice}
          onCollectionSynced={() => {}}
        />,
      );
    });
    await flush();

    // Open Friends tab -> Alice's profile -> Alice's Friends sub-tab.
    await clickFriendsTab(host, "Friends");
    const aliceCard = host.querySelector<HTMLButtonElement>(
      ".friends-friend-card-main",
    );
    await act(async () => {
      aliceCard!.click();
    });
    await flush();
    await clickFriendsTab(host, "Friends");

    // Click Add Friend on offline Carol -> should show Queued pill and queued notice.
    const addBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Add Friend"),
    );
    expect(addBtn).toBeTruthy();
    await act(async () => {
      addBtn!.click();
    });
    await flush();

    expect(api.addFriendByPeer).toHaveBeenCalledWith(carolId, "Carol");
    expect(onNotice).toHaveBeenCalledWith(
      "Friend request queued — will be sent when they're online.",
    );
    expect(host.textContent).toContain("Queued");

    // Go back to Friends list -> verify "Queued Outgoing Requests" card with relay info & Cancel.
    const backBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Back",
    );
    await act(async () => {
      backBtn!.click();
    });
    await flush();

    expect(host.textContent).toContain("Queued Outgoing Requests");
    expect(host.textContent).toContain("Waiting for them to come online");
    expect(host.textContent).toContain("Relayed via Alice");

    const cancelBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Cancel"),
    );
    expect(cancelBtn).toBeTruthy();
    await act(async () => {
      cancelBtn!.click();
    });
    await flush();

    expect(api.cancelQueuedFriendInvite).toHaveBeenCalledWith(carolId);
    expect(host.textContent).not.toContain("Queued Outgoing Requests");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("shows the user's own feed events in the global feed without unread badge, even with no friends", async () => {
    const selfOnlyState: FriendsState = {
      ...sampleState,
      friends: [],
      feed_last_read_at: null,
      feed_events: [
        {
          id: "self:local-1",
          peer_id: "peer_self",
          friend_display_name: "PlayerOne",
          is_self: true,
          kind: "collection_updated",
          collection_id: "active:skyrim",
          collection_kind: "active_loadout",
          collection_name: "My Skyrim Loadout",
          game_title: "Skyrim SE",
          mod_count: 4,
          share_hash: "self_hash_1",
          diff: {
            mods_added: ["SKSE"],
            mods_added_count: 1,
            mods_removed: [],
            mods_removed_count: 0,
            mods_updated: [],
            mods_updated_count: 0,
          },
          occurred_at: "2026-11-22T15:00:00Z",
        },
      ],
    };
    vi.mocked(api.getFriendsState).mockResolvedValue(selfOnlyState);

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

    // Renders own feed item even with 0 friends, with You pill and static container.
    expect(host.textContent).toContain("PlayerOne");
    expect(host.textContent).toContain("My Skyrim Loadout");
    expect(host.textContent).toContain("+1 added: SKSE");
    expect(host.querySelector(".friends-feed-item-static")).toBeTruthy();
    expect(host.querySelector(".friends-feed-item-btn")).toBeNull();
    expect(host.querySelector(".friends-feed-item.unread")).toBeNull();
    expect(host.querySelector(".friends-feed-badge")).toBeNull();
    expect(api.markFriendFeedRead).not.toHaveBeenCalled();

    const pills = [...host.querySelectorAll(".friends-feed-meta .pill")].map(
      (el) => el.textContent?.trim(),
    );
    expect(pills).toContain("You");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("filters global feed by You vs friend and excludes own events from per-friend feed", async () => {
    const mixedState: FriendsState = {
      ...sampleState,
      feed_last_read_at: "2026-11-22T20:00:00Z",
      feed_events: [
        {
          id: "self:local-1",
          peer_id: "peer_self",
          friend_display_name: "PlayerOne",
          is_self: true,
          kind: "collection_added",
          collection_id: "saved:my-pack",
          collection_kind: "saved_collection",
          collection_name: "My Own Pack",
          game_title: "Skyrim SE",
          mod_count: 2,
          share_hash: "self_hash",
          occurred_at: "2026-11-22T16:00:00Z",
        },
        {
          id: "alice:ev-1",
          peer_id: "peer_alice",
          friend_display_name: "Alice",
          is_self: false,
          kind: "collection_updated",
          collection_id: "active:skyrim",
          collection_kind: "active_loadout",
          collection_name: "Skyrim Loadout",
          game_title: "Skyrim SE",
          mod_count: 1,
          share_hash: "hash1",
          occurred_at: "2026-11-22T15:00:00Z",
        },
      ],
    };
    vi.mocked(api.getFriendsState).mockResolvedValue(mixedState);

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

    // Both own and friend events appear in the global feed.
    expect(host.textContent).toContain("My Own Pack");
    expect(host.textContent).toContain("Skyrim Loadout");

    // Filter dropdown lets the user filter to You (PlayerOne) or Alice.
    const filterSelect = host.querySelector<HTMLSelectElement>(
      'select[aria-label="Filter feed by friend"]',
    );
    expect(filterSelect).toBeTruthy();
    const optionTexts = [...filterSelect!.options].map((o) => o.textContent);
    expect(optionTexts).toContain("Everyone");
    expect(optionTexts).toContain("You (PlayerOne)");
    expect(optionTexts).toContain("Alice");

    await act(async () => {
      filterSelect!.value = "self";
      filterSelect!.dispatchEvent(new Event("change", { bubbles: true }));
    });
    await flush();

    expect(host.textContent).toContain("My Own Pack");
    expect(host.textContent).not.toContain("Skyrim Loadout");

    // Reset filter and open Alice's per-friend Feed tab: own event must not appear.
    await clickFriendsTab(host, "Friends");
    const aliceBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Alice"),
    );
    expect(aliceBtn).toBeTruthy();
    await act(async () => {
      aliceBtn!.click();
    });
    await flush();

    expect(host.textContent).toContain("Alice's Feed");
    expect(host.textContent).toContain("Skyrim Loadout");
    expect(host.textContent).not.toContain("My Own Pack");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("renders alternating timeline with date milestones and deduplicates matching active loadout against saved collection", async () => {
    const dedupeState: FriendsState = {
      ...sampleState,
      feed_last_read_at: "2026-11-22T20:00:00Z",
      feed_events: [
        {
          id: "self:saved-inc",
          peer_id: "peer_self",
          friend_display_name: "PlayerOne",
          is_self: true,
          kind: "collection_updated",
          collection_id: "saved:inc",
          collection_kind: "saved_collection",
          collection_name: "INC",
          game_title: "Cyberpunk 2077",
          mod_count: 70,
          share_hash: "h_inc",
          diff: {
            mods_added: [],
            mods_added_count: 0,
            mods_removed: [],
            mods_removed_count: 0,
            mods_updated: ["ArchiveXL"],
            mods_updated_count: 1,
          },
          occurred_at: "2026-11-22T16:00:00Z",
        },
        {
          id: "self:active-cp",
          peer_id: "peer_self",
          friend_display_name: "PlayerOne",
          is_self: true,
          kind: "collection_updated",
          collection_id: "active:cyberpunk2077",
          collection_kind: "active_loadout",
          collection_name: "Cyberpunk 2077 Loadout",
          game_title: "Cyberpunk 2077",
          mod_count: 70,
          share_hash: "h_active",
          diff: {
            mods_added: [],
            mods_added_count: 0,
            mods_removed: [],
            mods_removed_count: 0,
            mods_updated: ["ArchiveXL"],
            mods_updated_count: 1,
          },
          occurred_at: "2026-11-22T16:00:05Z",
        },
        {
          id: "alice:ev-older",
          peer_id: "peer_alice",
          friend_display_name: "Alice",
          is_self: false,
          kind: "collection_updated",
          collection_id: "active:skyrim",
          collection_kind: "active_loadout",
          collection_name: "Skyrim Loadout",
          game_title: "Skyrim SE",
          mod_count: 1,
          share_hash: "hash1",
          occurred_at: "2026-11-18T12:00:00Z",
        },
      ],
    };
    vi.mocked(api.getFriendsState).mockResolvedValue(dedupeState);

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

    // Duplicate ActiveLoadout ("Cyberpunk 2077 Loadout") is collapsed into SavedCollection ("INC").
    expect(host.textContent).toContain("INC");
    expect(host.textContent).not.toContain("Cyberpunk 2077 Loadout");
    expect(host.textContent).toContain("Skyrim Loadout");

    // Timeline renders date milestones and alternating left/right rows.
    expect(host.querySelector(".friends-timeline")).toBeTruthy();
    const milestones = host.querySelectorAll(".friends-timeline-milestone-badge");
    expect(milestones.length).toBe(2);
    const rows = host.querySelectorAll(".friends-timeline-row");
    expect(rows.length).toBe(2);
    expect(rows[0].classList.contains("left")).toBe(true);
    expect(rows[1].classList.contains("right")).toBe(true);

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("shows currently playing game on online friend cards and hides it when offline", async () => {
    vi.mocked(api.getFriendsState).mockResolvedValue({
      endpoint_ready: true,
      peer_id: "peer_self",
      join_code: "emperor-friend1:self",
      display_name: "Me",
      display_name_set: true,
      friends: [
        {
          peer_id: "peer_alice",
          display_name: "Alice",
          status: "connected",
          online: true,
          cached_profile: {
            peer_id: "peer_alice",
            display_name: "Alice",
            updated_at: "2026-11-23T18:00:00Z",
            collections: [],
            playing_game: {
              game_id: "cyberpunk2077",
              game_title: "Cyberpunk 2077",
              mode: "modded",
            },
          },
        },
        {
          peer_id: "peer_bob",
          display_name: "Bob",
          status: "connected",
          online: false,
          last_seen_at: "2026-11-23T17:00:00Z",
          cached_profile: {
            peer_id: "peer_bob",
            display_name: "Bob",
            updated_at: "2026-11-23T17:00:00Z",
            collections: [],
            playing_game: {
              game_id: "skyrimse",
              game_title: "Skyrim Special Edition",
              mode: "unmodded",
            },
          },
        },
      ],
      pending_requests: [],
      local_collections: [],
      feed_events: [],
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

    // Switch to the Friends tab.
    const friendsTabBtn = Array.from(
      host.querySelectorAll<HTMLButtonElement>("button"),
    ).find((b) => b.textContent?.includes("Friends"));
    expect(friendsTabBtn).toBeTruthy();
    await act(async () => {
      friendsTabBtn!.click();
    });
    await flush();

    expect(host.textContent).toContain("Playing Cyberpunk 2077 (Modded)");
    expect(host.textContent).not.toContain("Playing Skyrim Special Edition");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });
});
