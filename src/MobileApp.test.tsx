// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { DesktopSnapshot, MobileConnectionState } from "./types";

const invoked: string[] = [];
let mockConnState: MobileConnectionState = {
  paired: false,
  desktop_name: null,
  desktop_peer_id: null,
  mode: "disconnected",
  last_synced_at: null,
  last_error: null,
  allow_remote_launch: false,
};
let mockSnapshot: DesktopSnapshot | null = null;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd: string, args?: Record<string, unknown>) => {
    invoked.push(cmd);
    if (cmd === "get_mobile_connection_state") {
      return Promise.resolve(mockConnState);
    }
    if (cmd === "get_mobile_snapshot" || cmd === "sync_mobile_now") {
      return Promise.resolve(mockSnapshot);
    }
    if (cmd === "get_mobile_system_palette") {
      return Promise.resolve(null);
    }
    if (cmd === "mobile_get_game_cover") {
      return Promise.resolve("data:image/png;base64,iVBORw0KGgo=");
    }
    if (cmd === "mobile_get_saved_collection") {
      return Promise.resolve({
        entry: {
          id: "sc1",
          name: "Saved Pack",
          code: "EMP-SAVED",
          created_at: "2026-11-23T20:00:00Z",
          updated_at: "2026-11-23T20:00:00Z",
          game: { title: "Skyrim Special Edition", plugin_id: "skyrimse" },
          mod_count: 1,
          nexus_count: 1,
          thunderstore_count: 0,
          modio_count: 0,
        },
        mods: [
          {
            s: "nexus",
            display_name: "Saved Mod One",
            version: "1.0",
          },
        ],
      });
    }
    if (cmd === "mobile_check_mod_updates") {
      return Promise.resolve([
        { staged_id: "m1", available_version: "2.2.7", source: "nexus", nexus_file_id: 99 },
      ]);
    }
    if (cmd === "mobile_update_all_mods") {
      return Promise.resolve({ updated: 0, queued_assist: 1, errors: [] });
    }
    if (cmd === "mobile_catalog_rpc" && args?.method === "decode_share_code") {
      return Promise.resolve({
        name: "Alex's Co-op Pack",
        game: { title: "Skyrim Special Edition", plugin_id: "skyrimse" },
        mods: [
          {
            s: "nexus",
            domain: "skyrimspecialedition",
            mod_id: 123,
            display_name: "Co-op Mod Alpha",
            version: "1.2",
          },
        ],
        nexus_count: 1,
        thunderstore_count: 0,
        modio_count: 0,
        code_len: 10,
        size_warning: false,
      });
    }
    if (cmd === "mobile_catalog_rpc" && args?.method === "resolve_share_mod_metadata") {
      return Promise.resolve([
        {
          key: "nexus:skyrimspecialedition:123",
          picture_url: "https://example.com/coop-mod.jpg",
          author: "AlexAuthor",
        },
      ]);
    }
    if (cmd === "mobile_catalog_rpc" && args?.method === "search_catalog") {
      return Promise.resolve({
        items: [
          {
            source: "nexus",
            id: "123",
            name: "SkyUI Mobile Browse Test",
            summary: "Modern UI overhaul",
            picture_url: "https://example.com/skyui.jpg",
            author: "schlangster",
            downloads: 120000,
            endorsements: 50000,
            category: "UI",
            tags: [],
            mod_id: 123,
            domain_name: "skyrimspecialedition",
            community: null,
            namespace: null,
            package_name: null,
            full_name: null,
            package_url: null,
            rating_score: null,
            latest_version: "5.2",
            modio_game_id: null,
            modio_mod_id: null,
            profile_url: null,
          },
        ],
        total_count: 1,
        next_offset: 1,
        has_more: false,
        nexus_available: true,
        thunderstore_available: false,
        modio_available: false,
      });
    }
    if (cmd === "mobile_auto_sort_mods") {
      return Promise.resolve({ ordered_ids: ["m1"], changes: 1 });
    }
    if (cmd === "discover_lan_desktops") {
      return Promise.resolve([
        {
          host_name: "Gaming-PC",
          desktop_endpoint_id: "peer-abc",
          lan_addrs: ["192.168.1.50:12345"],
          pairing_active: true,
          app_version: "0.9.2",
        },
      ]);
    }
    return Promise.resolve({});
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import MobileApp from "./MobileApp";

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  invoked.length = 0;
  mockConnState = {
    paired: false,
    desktop_name: null,
    desktop_peer_id: null,
    mode: "disconnected",
    last_synced_at: null,
    last_error: null,
    allow_remote_launch: false,
  };
  mockSnapshot = null;
  if (root) {
    act(() => root!.unmount());
    root = null;
  }
  if (container) {
    container.remove();
    container = null;
  }
});

const APP_INFO = {
  name: "emperor-mod-manager",
  version: "0.9.2",
  platform: "android",
  is_mobile: true,
  linux_only: false,
};

function makePairedSnapshot(): DesktopSnapshot {
  return {
    rev: 5,
    synced_at: "2026-11-23T22:00:00Z",
    desktop_name: "Gaming-PC",
    app_version: "0.9.2",
    allow_remote_launch: true,
    account: {
      nexus_connected: true,
      nexus_user: null,
      modio_connected: false,
      p2p_display_name: "Luu",
      p2p_profile_picture: null,
      theme: "dark",
      theme_style: "app",
      accent_color: "#0078d4",
      adult_content: false,
      game_version_warnings: true,
      auto_sort_before_deploy: false,
    },
    active_game_id: "skyrimse",
    managed_games: [
      {
        id: "skyrimse",
        plugin_id: "skyrimse",
        title: "Skyrim Special Edition",
        install_path: "/games/skyrimse",
        cover_url: null,
      } as DesktopSnapshot["managed_games"][number],
    ],
    mods_by_game: {
      skyrimse: [
        {
          id: "m1",
          name: "SKSE64",
          version: "2.2.6",
          enabled: true,
          order: 0,
          staging_path: "/staging/m1",
          picture_url: "https://example.com/skse.png",
          collection_ids: ["col1"],
        } as DesktopSnapshot["mods_by_game"][string][number],
        {
          id: "m2",
          name: "SMIM FOMOD",
          version: "2.08",
          enabled: true,
          order: 1,
          staging_path: "/staging/m2",
          collection_ids: ["col1"],
        } as DesktopSnapshot["mods_by_game"][string][number],
      ],
    },
    installed_collections_by_game: {
      skyrimse: [
        {
          id: "col1",
          name: "Essential Skyrim",
          revision: 4,
          auto_save: false,
        } as DesktopSnapshot["installed_collections_by_game"][string][number],
      ],
    },
    saved_collections: [
      {
        id: "sc1",
        name: "Saved Pack",
        code: "EMP-SAVED",
        created_at: "2026-11-23T20:00:00Z",
        updated_at: "2026-11-23T20:00:00Z",
        game: { title: "Skyrim Special Edition", plugin_id: "skyrimse" },
        mod_count: 1,
        nexus_count: 1,
        thunderstore_count: 0,
        modio_count: 0,
      },
    ],
    bethesda_plugins_by_game: {},
    downloads: [],
    friends: {
      endpoint_ready: true,
      display_name: "Luu",
      display_name_set: true,
      friends: [
        {
          peer_id: "peer-friend-1",
          display_name: "Alex",
          status: "connected",
          online: true,
          cached_profile: {
            peer_id: "peer-friend-1",
            display_name: "Alex",
            updated_at: "2026-11-23T21:00:00Z",
            collections: [
              {
                id: "fc1",
                kind: "active_loadout",
                name: "Alex's Co-op Pack",
                share_code: "EMP-123",
                share_hash: "abc",
                updated_at: "2026-11-23T21:00:00Z",
              },
            ],
            friends: [
              {
                peer_id: "peer-mutual-1",
                display_name: "Sam",
                online: true,
              },
            ],
          },
        },
      ],
      pending_requests: [],
      local_collections: [],
      feed_events: [
        {
          id: "evt1",
          peer_id: "peer-friend-1",
          friend_display_name: "Alex",
          kind: "collection_updated",
          collection_id: "fc1",
          collection_kind: "active_loadout",
          collection_name: "Alex's Co-op Pack",
          game_title: "Skyrim Special Edition",
          mod_count: 14,
          share_hash: "abc",
          occurred_at: "2026-11-23T21:55:00Z",
        },
      ],
    },
    launch_states: {},
    banners: [
      { id: 1, kind: "info", message: "Deployed 2 mods cleanly" },
      { id: 2, kind: "warn", message: "Load order has 1 warning" },
      { id: 3, kind: "error", message: "Archive checksum mismatch" },
    ],
    mods_needing_fomod_by_game: {
      skyrimse: ["m2"],
    },
  };
}

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("MobileApp", () => {
  it("renders MobilePairingView when unpaired and calls zero desktop-only commands", async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    await act(async () => {
      root!.render(<MobileApp appInfo={APP_INFO} />);
    });
    await flush();

    expect(container.textContent).toContain("Pair with Desktop");
    expect(container.textContent).toContain("Gaming-PC");
    expect(container.textContent).toContain("6-Digit Pairing Code");
    expect(invoked).not.toContain("get_settings");
    expect(invoked).not.toContain("set_p2p_appear_offline");
    expect(invoked).not.toContain("open_nexus_website_login");
    expect(invoked).not.toContain("scan_games");
  });

  it("renders sticky desktop banners, Library actions, Browse, clickable Collections, and Friends Feed + Profile when paired", async () => {
    mockConnState = {
      paired: true,
      desktop_name: "Gaming-PC",
      desktop_peer_id: "peer-abc",
      mode: "lan_direct",
      last_synced_at: "2026-11-23T22:00:00Z",
      last_error: null,
      allow_remote_launch: true,
    };
    mockSnapshot = makePairedSnapshot();

    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    await act(async () => {
      root!.render(<MobileApp appInfo={APP_INFO} />);
    });
    await flush();

    // Replicated green/amber/red desktop banners in sticky region
    const bannerRegion = container.querySelector(".mobile-banner-region");
    expect(bannerRegion).not.toBeNull();
    expect(bannerRegion?.querySelector(".banner.info")?.textContent).toContain(
      "Deployed 2 mods cleanly",
    );
    expect(bannerRegion?.querySelector(".banner.warn")?.textContent).toContain(
      "Load order has 1 warning",
    );
    expect(bannerRegion?.querySelector(".banner.error")?.textContent).toContain(
      "Archive checksum mismatch",
    );

    // Library starts on the Games Grid; clicking a game opens its dedicated Game Detail page
    expect(container.querySelector(".mobile-library-grid")).not.toBeNull();
    expect(container.querySelector(".app-game-backdrop")).not.toBeNull();
    const openGameBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Open Skyrim Special Edition"),
    );
    expect(openGameBtn).toBeDefined();
    await act(async () => {
      openGameBtn!.click();
    });
    await flush();

    // Game Detail page shows Back to Library, FOMOD banner, Auto-Sort, Check Updates, and mod thumbnail
    expect(container.textContent).toContain("Back to Library");
    expect(container.textContent).toContain("1 mod needs installer choices");
    expect(container.textContent).toContain("Auto-Sort");
    expect(container.textContent).toContain("Check Updates");
    expect(container.querySelector('img[src="https://example.com/skse.png"]')).not.toBeNull();

    // System back (popstate) returns from Game Detail to Library Games Grid
    await act(async () => {
      window.dispatchEvent(new PopStateEvent("popstate"));
    });
    await flush();
    expect(container.querySelector(".mobile-library-grid")).not.toBeNull();

    // Titlebar has no app icon, no user avatar icon, and no active-game chip
    expect(container.querySelector(".mobile-topbar-logo")).toBeNull();
    expect(container.querySelector(".mobile-topbar-avatar-btn")).toBeNull();
    expect(container.querySelector(".mobile-active-game-chip")).toBeNull();
    expect(container.querySelector(".mobile-topbar-title")?.textContent).toBe("Library");

    // Browse tab loads catalog results from desktop with NO game selector dropdown
    const browseBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Browse"),
    );
    expect(browseBtn).toBeDefined();
    await act(async () => {
      browseBtn!.click();
    });
    await flush();
    expect(container.querySelector('select[aria-label="Browse game"]')).toBeNull();
    expect(container.textContent).toContain("Browse · Skyrim Special Edition");

    // Collections tab: saved collection loads ONCE (no repeating reload loop)
    const colsBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.classList.contains("mobile-nav-btn") && b.textContent?.includes("Collections"),
    );
    await act(async () => {
      colsBtn!.click();
    });
    await flush();
    const savedColCard = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Saved Pack"),
    );
    expect(savedColCard).toBeDefined();
    await act(async () => {
      savedColCard!.click();
    });
    await flush();
    expect(container.textContent).toContain("Saved Mod One");
    expect(invoked.filter((c) => c === "mobile_get_saved_collection").length).toBe(1);

    // Friends tab defaults to Feed and clicking an event opens Friend Profile with Feed, Collections, and Friends tabs
    const friendsBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.classList.contains("mobile-nav-btn") && b.textContent?.includes("Friends"),
    );
    await act(async () => {
      friendsBtn!.click();
    });
    await flush();
    expect(container.textContent).toContain('updated "Alex\'s Co-op Pack"');
    const feedCard = container.querySelector<HTMLButtonElement>(".mobile-feed-card");
    expect(feedCard).not.toBeNull();
    await act(async () => {
      feedCard!.click();
    });
    await flush();
    expect(container.textContent).toContain("Back to Friends");
    // Personal feed tab is active by default on profile
    expect(container.textContent).toContain('updated "Alex\'s Co-op Pack"');
    // Switch to Collections tab on Friend Profile and click into Friend Collection Detail
    const profColsTab = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Collections (1)"),
    );
    expect(profColsTab).toBeDefined();
    await act(async () => {
      profColsTab!.click();
    });
    await flush();
    expect(container.textContent).toContain("Active Loadouts (1)");
    expect(container.textContent).toContain("Sync to PC");
    const friendColBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Alex's Co-op Pack"),
    );
    expect(friendColBtn).toBeDefined();
    await act(async () => {
      friendColBtn!.click();
    });
    await flush();
    expect(container.textContent).toContain("Co-op Mod Alpha");
    expect(
      container.querySelector('img[src="https://example.com/coop-mod.jpg"]'),
    ).not.toBeNull();
    // Back to Alex's profile, then switch to Friends tab on Friend Profile
    const backToFriendBtn = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Back to Alex"),
    );
    await act(async () => {
      backToFriendBtn!.click();
    });
    await flush();
    const profFriendsTab = Array.from(container.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Friends (1)"),
    );
    expect(profFriendsTab).toBeDefined();
    await act(async () => {
      profFriendsTab!.click();
    });
    await flush();
    expect(container.textContent).toContain("Sam");
  });

  it("renders folded-mode titlebar + bottom nav with active game and supports offline native browsing", async () => {
    const prevWidth = window.innerWidth;
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      writable: true,
      value: 380,
    });
    window.dispatchEvent(new Event("resize"));

    try {
      mockConnState = {
        paired: true,
        desktop_name: "GamingPC",
        desktop_peer_id: "peer-desktop",
        mode: "disconnected",
        last_synced_at: "2026-11-24T00:00:00Z",
        last_error: null,
        allow_remote_launch: true,
      };
      mockSnapshot = {
        rev: 5,
        synced_at: "2026-11-24T00:00:00Z",
        desktop_name: "GamingPC",
        app_version: "0.9.2",
        allow_remote_launch: true,
        account: {
          nexus_connected: true,
          nexus_user: null,
          modio_connected: false,
          p2p_display_name: "Dragonborn",
          p2p_profile_picture: null,
          theme: "dark",
          theme_style: "app",
          accent_color: "#6a7cff",
          adult_content: false,
          game_version_warnings: true,
          auto_sort_before_deploy: false,
        },
        active_game_id: "g1",
        managed_games: [
          {
            id: "g1",
            plugin_id: "skyrimse",
            title: "Skyrim Special Edition",
            launcher: "steam",
            install_path: "/games/skyrimse",
            nexus_domain: "skyrimspecialedition",
            thunderstore_community: null,
            modio_game_id: null,
            cover_url: null,
          },
        ],
        mods_by_game: {
          g1: [
            {
              id: "m1",
              name: "SkyUI",
              version: "5.2",
              enabled: true,
              order: 0,
              source: "nexus",
              staging_path: "/staging/m1",
              nexus_mod_id: 12604,
              nexus_file_id: 1,
              domain: "skyrimspecialedition",
            },
          ],
        },
        installed_collections_by_game: { g1: [] },
        saved_collections: [],
        bethesda_plugins_by_game: {},
        downloads: [],
        friends: null,
        launch_states: {},
        banners: [],
        mods_needing_fomod_by_game: {},
      };

      const { default: MobileApp } = await import("./MobileApp");
      container = document.createElement("div");
      document.body.appendChild(container);
      root = createRoot(container);

      await act(async () => {
        root!.render(
          <MobileApp
            appInfo={{
              name: "Emperor Mod Manager",
              version: "0.9.2",
              platform: "android",
              is_mobile: true,
              linux_only: false,
            }}
          />,
        );
      });
      await flush();

      // Folded layout active, no duplicate boot sync when cached snapshot exists
      expect(container.querySelector(".mobile-layout-compact")).not.toBeNull();
      expect(invoked).not.toContain("sync_mobile_now");

      // Titlebar has no app icon, no user avatar, no active game chip, and uses compact status pill
      expect(container.querySelector(".mobile-topbar-logo")).toBeNull();
      expect(container.querySelector(".mobile-topbar-avatar-btn")).toBeNull();
      expect(container.querySelector(".mobile-active-game-chip")).toBeNull();
      expect(container.querySelector(".mobile-topbar-title")?.textContent).toBe("Library");
      expect(container.querySelector(".mobile-status-pill.compact")?.textContent).toContain(
        "Offline",
      );

      // Active game is docked in the bottom navigation bar
      const activeGameBar = container.querySelector<HTMLButtonElement>(
        ".mobile-bottom-active-game",
      );
      expect(activeGameBar).not.toBeNull();
      expect(activeGameBar!.textContent).toContain("Skyrim Special Edition");
      expect(activeGameBar!.textContent).toContain("1/1");

      // Tapping active game bar opens its loadout page
      await act(async () => {
        activeGameBar!.click();
      });
      await flush();
      expect(container.querySelector(".mobile-topbar-title")?.textContent).toBe(
        "Skyrim Special Edition",
      );

      // Browse tab works natively even when paired desktop is offline
      const browseBtn = Array.from(container.querySelectorAll(".mobile-nav-btn")).find((b) =>
        b.textContent?.includes("Browse"),
      ) as HTMLButtonElement | undefined;
      expect(browseBtn).toBeDefined();
      await act(async () => {
        browseBtn!.click();
      });
      await flush();
      expect(invoked).toContain("mobile_catalog_rpc");
      expect(container.textContent).not.toContain(
        "Connect to your paired desktop to browse mods",
      );
    } finally {
      Object.defineProperty(window, "innerWidth", {
        configurable: true,
        writable: true,
        value: prevWidth,
      });
      window.dispatchEvent(new Event("resize"));
    }
  });
});
