import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { MoveModCollectionDialog } from "./MoveModCollectionDialog";
import type { InstalledCollection, StagedMod } from "./types";

function makeMod(collectionIds: string[]): StagedMod {
  return {
    id: "1_2",
    name: "SkyUI",
    version: "5.2",
    domain: "skyrimspecialedition",
    nexus_mod_id: 1,
    nexus_file_id: 2,
    staging_path: "/tmp/skyui",
    enabled: true,
    order: 0,
    independent: collectionIds.length === 0,
    depends_on: [],
    collection_ids: collectionIds,
  };
}

function makeCollection(
  id: string,
  name: string,
  modIds: string[],
  synced = false,
): InstalledCollection {
  return {
    id,
    source: synced ? "emperor" : "nexus",
    kind: synced ? "share" : "collection",
    name,
    mod_ids: modIds,
    installed_at: "2026-01-01T00:00:00Z",
    friend_sync: synced
      ? {
          friend_peer_id: "peer-1",
          friend_collection_id: "saved:1",
          last_synced_hash: "hash-1",
          last_synced_at: "2026-01-01T00:00:00Z",
          auto_sync: false,
        }
      : null,
  };
}

let activeRoot: Root | null = null;
let activeHost: HTMLElement | null = null;

function mount(element: ReactElement) {
  activeHost = document.createElement("div");
  document.body.appendChild(activeHost);
  activeRoot = createRoot(activeHost);
  act(() => {
    activeRoot!.render(element);
  });
  return activeHost;
}

afterEach(() => {
  if (activeRoot) {
    act(() => {
      activeRoot!.unmount();
    });
    activeRoot = null;
  }
  if (activeHost) {
    activeHost.remove();
    activeHost = null;
  }
});

describe("MoveModCollectionDialog", () => {
  it("renders checkboxes for collections and moves mod when selection changes", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const onClose = vi.fn();
    const collections = [
      makeCollection("col-a", "Essentials", ["1_2"]),
      makeCollection("col-b", "Visuals", ["3_4", "5_6"]),
    ];

    mount(
      <MoveModCollectionDialog
        mod={makeMod(["col-a"])}
        collections={collections}
        busy={false}
        onSave={onSave}
        onClose={onClose}
      />,
    );

    const checkboxes = Array.from(
      document.body.querySelectorAll<HTMLInputElement>('input[type="checkbox"]'),
    );
    expect(checkboxes).toHaveLength(2);
    expect(checkboxes[0].checked).toBe(true);
    expect(checkboxes[1].checked).toBe(false);

    const saveBtn = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Save",
    )!;
    expect(saveBtn.disabled).toBe(true);

    // Move from Essentials -> Visuals
    act(() => {
      checkboxes[0].click();
      checkboxes[1].click();
    });
    expect(saveBtn.disabled).toBe(false);

    await act(async () => {
      saveBtn.click();
    });

    expect(onSave).toHaveBeenCalledTimes(1);
    expect(onSave).toHaveBeenCalledWith(["col-b"]);
  });

  it("allows unchecking all collections to make mod standalone", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const collections = [
      makeCollection("col-a", "Essentials", ["1_2"]),
      makeCollection("col-b", "Visuals", []),
    ];

    mount(
      <MoveModCollectionDialog
        mod={makeMod(["col-a"])}
        collections={collections}
        busy={false}
        onSave={onSave}
        onClose={vi.fn()}
      />,
    );

    const checkboxes = Array.from(
      document.body.querySelectorAll<HTMLInputElement>('input[type="checkbox"]'),
    );
    const saveBtn = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Save",
    )!;

    act(() => {
      checkboxes[0].click();
    });

    await act(async () => {
      saveBtn.click();
    });

    expect(onSave).toHaveBeenCalledWith([]);
  });

  it("disables adding a mod to a synced collection it is not already in", () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const collections = [
      makeCollection("col-local", "Local Pack", []),
      makeCollection("col-synced", "Friend Pack", ["9_9"], true),
    ];

    mount(
      <MoveModCollectionDialog
        mod={makeMod([])}
        collections={collections}
        busy={false}
        onSave={onSave}
        onClose={vi.fn()}
      />,
    );

    const checkboxes = Array.from(
      document.body.querySelectorAll<HTMLInputElement>('input[type="checkbox"]'),
    );
    const labels = Array.from(
      document.body.querySelectorAll<HTMLLabelElement>(".move-collection-option"),
    );
    const saveBtn = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Save",
    )!;

    expect(checkboxes[0].disabled).toBe(false);
    expect(checkboxes[1].disabled).toBe(true);
    expect(checkboxes[1].checked).toBe(false);
    expect(labels[1].classList.contains("is-disabled")).toBe(true);
    expect(labels[1].title).toBe("Mods cannot be added to a synced collection");
    expect(labels[1].textContent).toContain("Synced");

    act(() => {
      checkboxes[1].click();
    });

    expect(checkboxes[1].checked).toBe(false);
    expect(saveBtn.disabled).toBe(true);
    expect(onSave).not.toHaveBeenCalled();
  });

  it("locks existing synced collection membership while still allowing adding a non-synced collection", async () => {
    const onSave = vi.fn().mockResolvedValue(undefined);
    const collections = [
      makeCollection("col-local", "Local Pack", []),
      makeCollection("col-synced", "Friend Pack", ["1_2"], true),
    ];

    mount(
      <MoveModCollectionDialog
        mod={makeMod(["col-synced"])}
        collections={collections}
        busy={false}
        onSave={onSave}
        onClose={vi.fn()}
      />,
    );

    const checkboxes = Array.from(
      document.body.querySelectorAll<HTMLInputElement>('input[type="checkbox"]'),
    );
    const labels = Array.from(
      document.body.querySelectorAll<HTMLLabelElement>(".move-collection-option"),
    );
    const saveBtn = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Save",
    )!;

    expect(checkboxes[0].disabled).toBe(false);
    expect(checkboxes[1].disabled).toBe(true);
    expect(checkboxes[1].checked).toBe(true);
    expect(labels[1].classList.contains("is-disabled")).toBe(true);
    expect(labels[1].title).toBe(
      "Membership in this synced collection is managed by its author",
    );

    // Clicking the synced checkbox does not uncheck it
    act(() => {
      checkboxes[1].click();
    });
    expect(checkboxes[1].checked).toBe(true);
    expect(saveBtn.disabled).toBe(true);

    // Checking the non-synced collection works and preserves the synced collection
    act(() => {
      checkboxes[0].click();
    });

    await act(async () => {
      saveBtn.click();
    });

    expect(onSave).toHaveBeenCalledWith(["col-synced", "col-local"]);
  });
});
