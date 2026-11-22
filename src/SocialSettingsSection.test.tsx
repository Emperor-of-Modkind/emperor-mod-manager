import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { SocialSettingsSection } from "./SocialSettingsSection";
import { api } from "./api";
import type { FriendsState } from "./types";

vi.mock("./api", () => ({
  api: {
    setP2pDisplayName: vi.fn(),
    setP2pProfilePicture: vi.fn(),
    setFriendsListVisibility: vi.fn(),
    setAllowMutualConnections: vi.fn(),
    setAllowCacheRelay: vi.fn(),
  },
}));

const baseState: FriendsState = {
  endpoint_ready: true,
  peer_id: "peer_self",
  join_code: "emperor-friend1:abc",
  display_name: "PlayerOne",
  display_name_set: true,
  profile_picture: "data:image/png;base64,iVBORw0KGgo=",
  friends: [],
  pending_requests: [],
  local_collections: [],
};

async function flush() {
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
}

describe("SocialSettingsSection", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("updates display name and removes profile picture", async () => {
    const onFriendsStateChange = vi.fn();
    const onNotice = vi.fn();
    const onError = vi.fn();
    vi.mocked(api.setP2pDisplayName).mockResolvedValue({
      ...baseState,
      display_name: "RenamedPlayer",
    });
    vi.mocked(api.setP2pProfilePicture).mockResolvedValue({
      ...baseState,
      profile_picture: null,
    });

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <SocialSettingsSection
          friendsState={baseState}
          onFriendsStateChange={onFriendsStateChange}
          onNotice={onNotice}
          onError={onError}
        />,
      );
    });

    const input = host.querySelector<HTMLInputElement>(
      'input[aria-label="Social display name"]',
    );
    expect(input?.value).toBe("PlayerOne");

    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(input, "RenamedPlayer");
      input!.dispatchEvent(new Event("input", { bubbles: true }));
    });

    const form = host.querySelector("form");
    await act(async () => {
      form!.dispatchEvent(new Event("submit", { bubbles: true, cancelable: true }));
    });
    await flush();

    expect(api.setP2pDisplayName).toHaveBeenCalledWith("RenamedPlayer");
    expect(onNotice).toHaveBeenCalledWith("Display name updated.");

    const removeBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Remove",
    );
    expect(removeBtn).toBeTruthy();
    await act(async () => {
      removeBtn!.click();
    });
    await flush();

    expect(api.setP2pProfilePicture).toHaveBeenCalledWith(null);
    expect(onNotice).toHaveBeenCalledWith("Profile picture removed.");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("updates friends list privacy via the visibility modal", async () => {
    const onFriendsStateChange = vi.fn();
    const onNotice = vi.fn();
    const onError = vi.fn();
    const stateWithFriend: FriendsState = {
      ...baseState,
      friends_list_visibility: { mode: "all_friends" },
      friends: [
        {
          peer_id: "peer_alice",
          display_name: "Alice",
          status: "connected",
          online: true,
        },
      ],
    };
    vi.mocked(api.setFriendsListVisibility).mockResolvedValue({
      ...stateWithFriend,
      friends_list_visibility: {
        mode: "specific_friends",
        peer_ids: ["peer_alice"],
      },
    });

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <SocialSettingsSection
          friendsState={stateWithFriend}
          onFriendsStateChange={onFriendsStateChange}
          onNotice={onNotice}
          onError={onError}
        />,
      );
    });

    expect(host.textContent).toContain("Friends list privacy");
    expect(host.textContent).toContain("All friends");

    const changeBtn = [...host.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Change Privacy",
    );
    expect(changeBtn).toBeTruthy();
    await act(async () => {
      changeBtn!.click();
    });

    const specificRadio = document.querySelector<HTMLInputElement>(
      'input[type="radio"][value="specific_friends"]',
    );
    expect(specificRadio).toBeTruthy();
    await act(async () => {
      specificRadio!.click();
    });

    const aliceCheckbox = document.querySelector<HTMLInputElement>(
      '.collection-sharing-friend-row input[type="checkbox"]',
    );
    expect(aliceCheckbox).toBeTruthy();
    await act(async () => {
      aliceCheckbox!.click();
    });

    const modalSaveBtn = [
      ...document.querySelectorAll<HTMLButtonElement>(".modal-foot-actions button"),
    ].find((b) => b.textContent?.trim() === "Save");
    expect(modalSaveBtn).toBeTruthy();
    await act(async () => {
      modalSaveBtn!.click();
    });
    await flush();

    expect(api.setFriendsListVisibility).toHaveBeenCalledWith({
      mode: "specific_friends",
      peer_ids: ["peer_alice"],
    });
    expect(onNotice).toHaveBeenCalledWith("Friends list privacy updated.");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("toggles allow mutual connections setting", async () => {
    const onFriendsStateChange = vi.fn();
    const onNotice = vi.fn();
    const onError = vi.fn();
    vi.mocked(api.setAllowMutualConnections).mockResolvedValue({
      ...baseState,
      allow_mutual_connections: false,
    });

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <SocialSettingsSection
          friendsState={baseState}
          onFriendsStateChange={onFriendsStateChange}
          onNotice={onNotice}
          onError={onError}
        />,
      );
    });

    const toggle = host.querySelector<HTMLButtonElement>(
      'button[role="switch"]',
    );
    expect(toggle).toBeTruthy();
    expect(toggle?.getAttribute("aria-checked")).toBe("true");

    await act(async () => {
      toggle!.click();
    });
    await flush();

    expect(api.setAllowMutualConnections).toHaveBeenCalledWith(false);
    expect(onFriendsStateChange).toHaveBeenCalledWith({
      ...baseState,
      allow_mutual_connections: false,
    });
    expect(onNotice).toHaveBeenCalledWith("Mutual connections disabled.");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("toggles share cached friend profiles setting", async () => {
    const onFriendsStateChange = vi.fn();
    const onNotice = vi.fn();
    const onError = vi.fn();
    vi.mocked(api.setAllowCacheRelay).mockResolvedValue({
      ...baseState,
      allow_cache_relay: false,
    });

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <SocialSettingsSection
          friendsState={baseState}
          onFriendsStateChange={onFriendsStateChange}
          onNotice={onNotice}
          onError={onError}
        />,
      );
    });

    const toggles = host.querySelectorAll<HTMLButtonElement>(
      'button[role="switch"]',
    );
    expect(toggles.length).toBe(2);
    const relayToggle = toggles[1];
    expect(relayToggle.getAttribute("aria-checked")).toBe("true");

    await act(async () => {
      relayToggle.click();
    });
    await flush();

    expect(api.setAllowCacheRelay).toHaveBeenCalledWith(false);
    expect(onFriendsStateChange).toHaveBeenCalledWith({
      ...baseState,
      allow_cache_relay: false,
    });
    expect(onNotice).toHaveBeenCalledWith(
      "Offline friend cache sharing disabled.",
    );

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });
});
