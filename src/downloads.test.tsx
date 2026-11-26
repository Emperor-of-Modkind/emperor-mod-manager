import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import { DownloadsWorkspace } from "./downloads";
import type { DownloadItem } from "./types";

const mounted: Array<{ host: HTMLElement; root: Root }> = [];

async function render(element: ReactElement) {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const root = createRoot(host);
  await act(async () => {
    root.render(element);
  });
  mounted.push({ host, root });
  return host;
}

afterEach(async () => {
  while (mounted.length > 0) {
    const entry = mounted.pop()!;
    await act(async () => {
      entry.root.unmount();
    });
    entry.host.remove();
  }
});

function makeItem(overrides: Partial<DownloadItem>): DownloadItem {
  return {
    id: "dl-1",
    label: "Test Mod",
    status: "failed",
    error: "Network error",
    bytes_downloaded: 0,
    bytes_total: null,
    speed_bps: 0,
    batch_id: null,
    can_restart: false,
    file_name: null,
    page: {
      source: "nexus",
      game_id: "skyrim",
      domain: "skyrimspecialedition",
      mod_id: 42,
    },
    ...overrides,
  };
}

function renderWorkspace(
  downloads: DownloadItem[],
  callbacks?: {
    onRestartDownload?: (id: string) => void;
    onOpenRecent?: (item: DownloadItem) => void;
    onRemoveDownload?: (id: string) => void;
    onCancelDownload?: (id: string) => void;
    onCancelBatch?: (batchId: string) => void;
  },
) {
  return render(
    <DownloadsWorkspace
      downloads={downloads}
      queue={null}
      activeBatch={null}
      assistActive={false}
      assistHint={null}
      onRefresh={() => {}}
      onSkip={() => {}}
      onCancel={() => {}}
      onCancelRemaining={() => {}}
      onCancelBatch={callbacks?.onCancelBatch}
      onCancelDownload={callbacks?.onCancelDownload ?? (() => {})}
      onRestartDownload={callbacks?.onRestartDownload ?? (() => {})}
      onRetryAssistOpening={() => {}}
      isDownloadStuck={() => false}
      onClearRecent={() => {}}
      onOpenRecent={callbacks?.onOpenRecent ?? (() => {})}
      onRemoveDownload={callbacks?.onRemoveDownload ?? (() => {})}
      onPauseDownload={() => {}}
      onResumeDownload={() => {}}
      onAssistHost={() => {}}
    />,
  );
}

describe("DownloadsWorkspace recent failed/cancelled rows", () => {
  it("shows Retry (and never Dismiss) when failed row can_restart is true", async () => {
    const onRestartDownload = vi.fn();
    const host = await renderWorkspace(
      [makeItem({ id: "r1", status: "failed", can_restart: true })],
      { onRestartDownload },
    );

    const buttons = Array.from(host.querySelectorAll(".downloads-recent-actions button"));
    const labels = buttons.map((b) => b.textContent?.trim()).filter(Boolean);
    expect(labels).toEqual(["Retry"]);
    expect(host.textContent).not.toContain("Dismiss");
    expect(host.querySelector(".downloads-recent-error")?.textContent).toContain(
      "Failed · Network error",
    );

    await act(async () => {
      (buttons[0] as HTMLButtonElement).click();
    });
    expect(onRestartDownload).toHaveBeenCalledWith("r1");
  });

  it("shows Open mod when failed or cancelled row cannot restart but has a page", async () => {
    const onOpenRecent = vi.fn();
    const failedNoRestart = makeItem({
      id: "f1",
      status: "failed",
      can_restart: false,
      error: "link expired",
    });
    const cancelledNoRestart = makeItem({
      id: "c1",
      status: "cancelled",
      can_restart: false,
      error: null,
    });

    const host = await renderWorkspace([failedNoRestart, cancelledNoRestart], {
      onOpenRecent,
    });

    const rows = Array.from(host.querySelectorAll(".downloads-recent li"));
    expect(rows).toHaveLength(2);
    for (const row of rows) {
      const actionLabels = Array.from(
        row.querySelectorAll(".downloads-recent-actions button"),
      )
        .map((b) => b.textContent?.trim())
        .filter(Boolean);
      expect(actionLabels).toEqual(["Open mod"]);
    }
    expect(host.textContent).not.toContain("Dismiss");

    await act(async () => {
      (rows[0].querySelector(".downloads-control-one") as HTMLButtonElement).click();
    });
    expect(onOpenRecent).toHaveBeenCalledWith(failedNoRestart);
  });

  it("shows only Remove when row cannot restart and has no page", async () => {
    const onRemoveDownload = vi.fn();
    const host = await renderWorkspace(
      [makeItem({ id: "l1", status: "failed", can_restart: false, page: null })],
      { onRemoveDownload },
    );

    const actions = Array.from(host.querySelectorAll(".downloads-recent-actions button"));
    expect(actions).toHaveLength(1);
    expect(actions[0].getAttribute("aria-label")).toBe("Remove Test Mod");

    await act(async () => {
      (actions[0] as HTMLButtonElement).click();
    });
    expect(onRemoveDownload).toHaveBeenCalledWith("l1");
  });
});

describe("DownloadsWorkspace queued items and backend batches", () => {
  it("renders queued downloads in FIFO order inside the Queue pane and excludes them from Recent", async () => {
    const onCancelDownload = vi.fn();
    const onCancelBatch = vi.fn();
    // Backend returns newest-first: q2 was enqueued after q1.
    const host = await renderWorkspace(
      [
        makeItem({
          id: "q2",
          label: "Owner-ModB-1.0.0",
          status: "queued",
          error: null,
          batch_id: "batch-ts",
          batch_label: "My Modpack",
        }),
        makeItem({
          id: "q1",
          label: "Owner-ModA-1.0.0",
          status: "queued",
          error: null,
          batch_id: "batch-ts",
          batch_label: "My Modpack",
        }),
        makeItem({
          id: "active-1",
          label: "BepInEx-BepInExPack-5.4.21",
          status: "downloading",
          error: null,
          bytes_downloaded: 500,
          bytes_total: 1000,
          speed_bps: 250,
          batch_id: "batch-ts",
          batch_label: "My Modpack",
        }),
        makeItem({
          id: "done-1",
          label: "Owner-Core-1.0.0",
          status: "done",
          error: null,
          batch_id: "batch-ts",
          batch_label: "My Modpack",
        }),
      ],
      { onCancelDownload, onCancelBatch },
    );

    expect(host.querySelector(".downloads-subtitle")?.textContent).toBe(
      "My Modpack — 1/4 complete",
    );

    const queueItems = Array.from(host.querySelectorAll(".downloads-queue-item"));
    expect(queueItems).toHaveLength(2);
    expect(queueItems[0].textContent).toContain("Owner-ModA-1.0.0");
    expect(queueItems[0].textContent).toContain("Queued · #1");
    expect(queueItems[1].textContent).toContain("Owner-ModB-1.0.0");
    expect(queueItems[1].textContent).toContain("Queued · #2");

    // Recent should only contain the finished item, not queued ones.
    const recentItems = Array.from(host.querySelectorAll(".downloads-recent li"));
    expect(recentItems).toHaveLength(1);
    expect(recentItems[0].textContent).toContain("Owner-Core-1.0.0");

    // Cancel individual queued item.
    await act(async () => {
      (queueItems[0].querySelector("button") as HTMLButtonElement).click();
    });
    expect(onCancelDownload).toHaveBeenCalledWith("q1");

    // Cancel remaining batch button in header.
    const cancelBatchBtn = Array.from(
      host.querySelectorAll(".panel-head-actions button"),
    ).find((b) => b.textContent?.includes("Cancel remaining downloads")) as
      | HTMLButtonElement
      | undefined;
    expect(cancelBatchBtn).toBeDefined();
    await act(async () => {
      cancelBatchBtn!.click();
    });
    expect(onCancelBatch).toHaveBeenCalledWith("batch-ts");
  });
});
