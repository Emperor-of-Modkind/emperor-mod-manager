// @vitest-environment jsdom
import { act, type ReactElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  CollectionIntegrityDialog,
  describeCollectionModIssue,
} from "./CollectionIntegrityDialog";
import type { CollectionIntegrityReport } from "./types";

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

const sampleReport: CollectionIntegrityReport = {
  collection_id: "nexus:test-pack",
  collection_name: "Test Pack",
  checked_count: 5,
  issues: [
    {
      mod_id: "10_1",
      name: "SkyUI",
      source: "nexus",
      issue: { kind: "missing_staging_dir" },
      can_retry: true,
      domain: "skyrimspecialedition",
      nexus_mod_id: 10,
      nexus_file_id: 1,
      version: "5.2",
    },
    {
      mod_id: "ts_Owner_ModB",
      name: "Mod B",
      source: "thunderstore",
      issue: {
        kind: "corrupted_staging",
        actual_files: 1,
        expected_files: 4,
      },
      can_retry: true,
      domain: "riskofrain2",
      nexus_mod_id: 0,
      nexus_file_id: 0,
      ts_namespace: "Owner",
      ts_name: "ModB",
      version: "1.0.0",
    },
    {
      mod_id: "999_9",
      name: "Unknown Mod",
      source: "nexus",
      issue: { kind: "missing_from_load_order" },
      can_retry: false,
      domain: "",
      nexus_mod_id: 0,
      nexus_file_id: 0,
    },
  ],
};

describe("CollectionIntegrityDialog", () => {
  it("describes all issue kinds clearly", () => {
    expect(describeCollectionModIssue({ kind: "missing_from_load_order" })).toBe(
      "Missing from load order",
    );
    expect(describeCollectionModIssue({ kind: "missing_staging_dir" })).toBe(
      "Missing staging folder",
    );
    expect(
      describeCollectionModIssue({
        kind: "corrupted_staging",
        actual_files: 2,
        expected_files: 5,
      }),
    ).toBe("Corrupted (2/5 files)");
    expect(
      describeCollectionModIssue({
        kind: "corrupted_staging",
        actual_files: 0,
        expected_files: null,
      }),
    ).toBe("Empty staging folder");
    expect(describeCollectionModIssue({ kind: "missing_from_manifest" })).toBe(
      "Missing from collection",
    );
  });

  it("preselects retryable mods and retries only the checked ones", () => {
    const onRetry = vi.fn();
    const onClose = vi.fn();
    render(
      <CollectionIntegrityDialog
        report={sampleReport}
        retrying={false}
        onRetry={onRetry}
        onClose={onClose}
      />,
    );

    const checkboxes = Array.from(
      document.body.querySelectorAll<HTMLInputElement>('input[type="checkbox"]'),
    );
    expect(checkboxes).toHaveLength(3);
    expect(checkboxes[0].checked).toBe(true);
    expect(checkboxes[1].checked).toBe(true);
    expect(checkboxes[2].checked).toBe(false);
    expect(checkboxes[2].disabled).toBe(true);

    // Uncheck the first mod so only Mod B is retried
    act(() => {
      checkboxes[0].click();
    });

    const retryBtn = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent?.includes("Retry selected"),
    );
    expect(retryBtn?.textContent).toContain("Retry selected (1)");

    act(() => {
      retryBtn?.click();
    });

    expect(onRetry).toHaveBeenCalledTimes(1);
    expect(onRetry).toHaveBeenCalledWith([sampleReport.issues[1]]);
  });
});
