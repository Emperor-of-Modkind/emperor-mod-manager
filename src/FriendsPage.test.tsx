import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { describe, expect, it, vi, beforeEach } from "vitest";
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
  },
}));

const sampleState: FriendsState = {
  endpoint_ready: true,
  peer_id: "peer_self",
  join_code: "emperor-friend1:abc",
  display_name: "PlayerOne",
  pending_requests: [],
  local_collections: [],
  friends: [
    {
      peer_id: "peer_alice",
      display_name: "Alice",
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

    const aliceBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Alice"),
    );
    expect(aliceBtn).toBeTruthy();
    await act(async () => {
      aliceBtn!.click();
    });
    await flush();

    expect(host.textContent).toContain("Skyrim Loadout");
    const colBtn = [...host.querySelectorAll("button")].find((b) =>
      b.textContent?.includes("Skyrim Loadout"),
    );
    expect(colBtn).toBeTruthy();
    await act(async () => {
      colBtn!.click();
    });
    await flush();

    expect(host.textContent).toContain("SkyUI");
    expect(host.textContent).toContain("FOMOD Config");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });
});
