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
});
