import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { describe, expect, it, vi } from "vitest";
import {
  CollectionSharingModal,
  formatVisibilityLabel,
  resolveCollectionVisibility,
} from "./CollectionSharingModal";
import type { FriendRecord } from "./types";

const friends: FriendRecord[] = [
  {
    peer_id: "peer_alice",
    display_name: "Alice",
    status: "connected",
    online: true,
    last_seen_at: null,
    cached_profile: null,
  },
  {
    peer_id: "peer_bob",
    display_name: "Bob",
    status: "connected",
    online: false,
    last_seen_at: null,
    cached_profile: null,
  },
  {
    peer_id: "peer_pending",
    display_name: "Carol",
    status: "pending_outgoing",
    online: false,
    last_seen_at: null,
    cached_profile: null,
  },
];

describe("CollectionSharingModal", () => {
  it("defaults saved collections to private and formats labels", () => {
    expect(resolveCollectionVisibility(undefined, "saved:123")).toEqual({
      mode: "private",
    });
    expect(resolveCollectionVisibility(undefined, "active:skyrim")).toEqual({
      mode: "all_friends",
    });
    expect(formatVisibilityLabel({ mode: "all_friends" })).toBe("All friends");
    expect(
      formatVisibilityLabel({ mode: "specific_friends", peer_ids: ["a"] }),
    ).toBe("1 friend");
    expect(
      formatVisibilityLabel({
        mode: "specific_friends",
        peer_ids: ["a", "b"],
      }),
    ).toBe("2 friends");
    expect(formatVisibilityLabel({ mode: "private" })).toBe("Private");
  });

  it("allows selecting specific connected friends and saving", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <CollectionSharingModal
          collectionId="saved:c1"
          collectionName="My Pack"
          initialVisibility={{ mode: "private" }}
          friends={friends}
          onSave={onSave}
          onClose={onClose}
        />,
      );
    });

    const specificRadio = document.body.querySelector(
      'input[type="radio"][value="specific_friends"]',
    ) as HTMLInputElement;
    expect(specificRadio).toBeTruthy();

    await act(async () => {
      specificRadio.click();
    });

    // Only connected friends (Alice, Bob) should appear, not pending Carol.
    const checkboxes = [...document.body.querySelectorAll('input[type="checkbox"]')];
    expect(checkboxes).toHaveLength(2);
    expect(document.body.textContent).toContain("Alice");
    expect(document.body.textContent).toContain("Bob");
    expect(document.body.textContent).not.toContain("Carol");

    await act(async () => {
      (checkboxes[0] as HTMLInputElement).click();
    });

    const saveBtn = [...document.body.querySelectorAll("button")].find(
      (b) => b.textContent?.trim() === "Save",
    );
    expect(saveBtn).toBeTruthy();

    await act(async () => {
      saveBtn!.click();
    });

    expect(onSave).toHaveBeenCalledWith({
      mode: "specific_friends",
      peer_ids: ["peer_alice"],
    });
    expect(onClose).toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });
});
