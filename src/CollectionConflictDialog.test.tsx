// @vitest-environment jsdom
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import { CollectionConflictDialog } from "./CollectionConflictDialog";
import type { CollectionConflictPlan } from "./types";

let root: Root | null = null;
let host: HTMLDivElement | null = null;

function render(element: ReactElement) {
  host = document.createElement("div");
  document.body.appendChild(host);
  root = createRoot(host);
  act(() => {
    root!.render(element);
  });
}

afterEach(() => {
  if (root) {
    act(() => {
      root!.unmount();
    });
    root = null;
  }
  host?.remove();
  host = null;
});

const samplePlan: CollectionConflictPlan = {
  targetCollectionId: "friend:peer1:coop",
  targetCollectionName: "Friends Co-op",
  isSyncedTarget: true,
  exactMatchCount: 4,
  repairCount: 0,
  installCount: 2,
  autoUpdateCount: 1,
  conflicts: [
    {
      conflictKey: "nexus:skyrimspecialedition:100:2",
      source: "nexus",
      incomingName: "SkyUI",
      incomingVersion: "5.2",
      incomingFileId: 2,
      incomingFileName: "SkyUI_5_2.7z",
      existingModId: "100_1",
      existingName: "SkyUI",
      existingVersion: "5.1",
      existingFileId: 1,
      existingFileName: "SkyUI_5_1.7z",
      existingEnabled: true,
      existingIndependent: false,
      conflictingCollections: [
        {
          id: "emperor-share:solo",
          name: "Solo Loadout",
          isSynced: false,
          enabledModCount: 12,
        },
      ],
      recommendedAction: "disable_other_collections",
    },
    {
      conflictKey: "ts:owner:pkg:2.0.0",
      source: "thunderstore",
      incomingName: "Pkg",
      incomingVersion: "2.0.0",
      incomingFileId: null,
      incomingFileName: null,
      existingModId: "ts_Owner_Pkg",
      existingName: "Pkg",
      existingVersion: "1.0.0",
      existingFileId: null,
      existingFileName: null,
      existingEnabled: true,
      existingIndependent: true,
      conflictingCollections: [],
      recommendedAction: "replace_existing",
    },
  ],
  conflictingCollections: [
    {
      id: "emperor-share:solo",
      name: "Solo Loadout",
      isSynced: false,
      enabledModCount: 12,
      conflictCount: 1,
      conflictKeys: ["nexus:skyrimspecialedition:100:2"],
    },
  ],
};

describe("CollectionConflictDialog", () => {
  it("renders summary counts, conflicting collections, and submits recommended defaults", () => {
    const onConfirm = vi.fn();
    const onCancel = vi.fn();
    render(
      <CollectionConflictDialog
        plan={samplePlan}
        onConfirm={onConfirm}
        onCancel={onCancel}
      />,
    );

    const text = document.body.textContent ?? "";
    expect(text).toContain("4 already installed (will reuse)");
    expect(text).toContain("2 version conflicts");
    expect(text).toContain("Solo Loadout");
    expect(text).toContain("Non-synced");

    const confirmBtn = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.includes("Continue"),
    )!;
    expect(confirmBtn.textContent).toContain("5 to download");

    act(() => {
      confirmBtn.click();
    });

    expect(onConfirm).toHaveBeenCalledWith({
      disableCollectionIds: ["emperor-share:solo"],
      modActions: {
        "nexus:skyrimspecialedition:100:2": "disable_other_collections",
        "ts:owner:pkg:2.0.0": "replace_existing",
      },
    });
  });

  it("unchecking conflicting collection switches its conflict action to disable_old_and_install_new", () => {
    const onConfirm = vi.fn();
    render(
      <CollectionConflictDialog
        plan={samplePlan}
        onConfirm={onConfirm}
        onCancel={() => {}}
      />,
    );

    const checkbox = document.querySelector<HTMLInputElement>(
      'input[type="checkbox"]',
    )!;
    expect(checkbox.checked).toBe(true);
    act(() => {
      checkbox.click();
    });
    expect(checkbox.checked).toBe(false);

    const confirmBtn = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.includes("Continue"),
    )!;
    act(() => {
      confirmBtn.click();
    });

    expect(onConfirm).toHaveBeenCalledWith({
      disableCollectionIds: [],
      modActions: {
        "nexus:skyrimspecialedition:100:2": "disable_old_and_install_new",
        "ts:owner:pkg:2.0.0": "replace_existing",
      },
    });
  });

  it("bulk keep all existing skips conflict downloads and clears disableCollectionIds", () => {
    const onConfirm = vi.fn();
    render(
      <CollectionConflictDialog
        plan={samplePlan}
        onConfirm={onConfirm}
        onCancel={() => {}}
      />,
    );

    const keepAllBtn = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.includes("Keep all existing"),
    )!;
    act(() => {
      keepAllBtn.click();
    });

    const confirmBtn = Array.from(document.querySelectorAll("button")).find(
      (b) => b.textContent?.includes("Continue"),
    )!;
    // 2 install + 1 autoUpdate + 0 from conflicts = 3
    expect(confirmBtn.textContent).toContain("3 to download");

    act(() => {
      confirmBtn.click();
    });
    expect(onConfirm).toHaveBeenCalledWith({
      disableCollectionIds: [],
      modActions: {
        "nexus:skyrimspecialedition:100:2": "keep_existing",
        "ts:owner:pkg:2.0.0": "keep_existing",
      },
    });
  });
});
