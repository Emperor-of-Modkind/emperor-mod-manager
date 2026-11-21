import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

import {
  DEFAULT_BANNER_TIMEOUTS,
  StatusBanner,
  bannerPersistenceFor,
  bannerTimeoutFor,
  deployStatusNotice,
  dismissBanner,
  dismissBannersForDownload,
  filterPersistableBanners,
  hiddenBannerDetails,
  pruneExpiredBanners,
  pushBanner,
  remainingBannerTimeoutSeconds,
  type StackedBanner,
} from "./StatusBanner";
import type { DeployResult, MissingRequirements, Settings } from "./types";

const requirements: MissingRequirements = {
  nexus: [
    {
      domain: "skyrimspecialedition",
      mod_id: 42,
      file_id: 7,
      name: "Address Library",
      version: "11",
      notes: null,
      url: "https://www.nexusmods.com/skyrimspecialedition/mods/42",
      required_by: ["1_2"],
      required_by_names: ["My Mod"],
    },
  ],
  external: [],
  dlc: [],
  error: null,
};

function deployResult(overrides: Partial<DeployResult> = {}): DeployResult {
  return {
    file_count: 4,
    enabled_mods: 2,
    warnings: [],
    ...overrides,
  };
}

describe("hiddenBannerDetails", () => {
  it("hides a detail that is already the whole message", () => {
    expect(hiddenBannerDetails("Only this", ["Only this"])).toEqual([]);
  });

  it("keeps every warning when the message is a summary", () => {
    expect(hiddenBannerDetails("Deployed 2 mods.", ["Skipped a", "Skipped b"])).toEqual([
      "Skipped a",
      "Skipped b",
    ]);
  });
});

describe("deployStatusNotice", () => {
  it("keeps every warning and does not open the requirements modal until asked", () => {
    const opened: MissingRequirements[] = [];
    const notice = deployStatusNotice(
      deployResult({
        warnings: ["Skipped A: missing master B.", "Skipped C: missing master D."],
        plugin_skips: [
          { plugin: "A.esp", missing_master: "B.esm" },
          { plugin: "C.esp", missing_master: "D.esm" },
        ],
        missing_requirements: requirements,
      }),
      (payload) => opened.push(payload),
    );
    expect(notice.kind).toBe("warn");
    expect(notice.message).toBe("Launched with 4 files from 2 mods.");
    expect(notice.message).not.toContain("Skipped");
    expect(notice.details).toEqual([
      "Skipped A: missing master B.",
      "Skipped C: missing master D.",
    ]);
    expect(opened).toEqual([]);
    notice.action?.run();
    expect(opened).toEqual([requirements]);
    expect(notice.action?.label).toBe("Review requirements");
  });

  it("leaves the warning readable when there is nothing to queue or link", () => {
    const notice = deployStatusNotice(
      deployResult({
        warnings: ["Skipped A: missing master B."],
        plugin_skips: [{ plugin: "A.esp", missing_master: "B.esm" }],
        missing_requirements: { nexus: [], external: [], dlc: [], error: null },
      }),
      () => {
        throw new Error("modal should stay closed");
      },
    );
    expect(notice.action).toBeUndefined();
    expect(notice.details).toEqual(["Skipped A: missing master B."]);
  });

  it("does not offer review when deploy did not skip a master", () => {
    const notice = deployStatusNotice(
      deployResult({
        warnings: ["Something else."],
        missing_requirements: requirements,
      }),
      () => {
        throw new Error("modal should stay closed");
      },
    );
    expect(notice.action).toBeUndefined();
  });

  it("uses the enabled-mod summary when there are no warnings", () => {
    const notice = deployStatusNotice(deployResult(), () => {});
    expect(notice).toEqual({
      kind: "ok",
      message: "Deployed 4 files from 2 enabled mods and launched game.",
    });
  });
});

describe("StatusBanner", () => {
  async function render(ui: ReactElement) {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);
    await act(async () => {
      root.render(ui);
    });
    return {
      host,
      async unmount() {
        await act(async () => root.unmount());
        host.remove();
      },
    };
  }

  it("shows the action and the full details without expanding first", async () => {
    let reviewed = false;
    let dismissed = false;
    const view = await render(
      <StatusBanner
        kind="warn"
        message="Deployed 4 files from 2 mods."
        details={["Skipped A: missing master B.", "Skipped C: missing master D."]}
        action={{
          label: "Review requirements",
          run: () => {
            reviewed = true;
          },
        }}
        onDismiss={() => {
          dismissed = true;
        }}
      />,
    );
    expect(view.host.textContent).toContain("Review requirements");
    expect(view.host.textContent).not.toContain("Skipped A");
    const review = [...view.host.querySelectorAll("button")].find(
      (button) => button.textContent === "Review requirements",
    );
    await act(async () => {
      review?.click();
    });
    expect(reviewed).toBe(true);

    const more = [...view.host.querySelectorAll("button")].find(
      (button) => button.textContent === "Show more",
    );
    await act(async () => {
      more?.click();
    });
    expect(view.host.textContent).toContain("Skipped A: missing master B.");
    expect(view.host.textContent).toContain("Skipped C: missing master D.");
    expect(view.host.textContent).toContain("Review requirements");

    const dismiss = view.host.querySelector(
      'button[aria-label="Dismiss"]',
    ) as HTMLButtonElement | null;
    await act(async () => {
      dismiss?.click();
    });
    expect(dismissed).toBe(true);
    await view.unmount();
  });

  it("omits show more when the message is the whole notice", async () => {
    const view = await render(
      <StatusBanner kind="error" message="Install path is required." onDismiss={() => {}} />,
    );
    expect(view.host.textContent).not.toContain("Show more");
    expect(view.host.querySelector('button[aria-label="Dismiss"]')).not.toBeNull();
    await view.unmount();
  });

  it("dismisses the banner after running an action with dismiss: true", async () => {
    const onDismiss = vi.fn();
    const run = vi.fn();
    const view = await render(
      <StatusBanner
        kind="error"
        message="Download failed"
        action={{ label: "Retry", dismiss: true, run }}
        onDismiss={onDismiss}
      />,
    );

    const button = Array.from(view.host.querySelectorAll("button")).find(
      (b) => b.textContent === "Retry",
    );
    expect(button).toBeDefined();
    await act(async () => {
      button!.click();
    });
    expect(run).toHaveBeenCalledTimes(1);
    expect(onDismiss).toHaveBeenCalledTimes(1);
    await view.unmount();
  });

  it("keeps the banner open when dismiss is not set on the action", async () => {
    const onDismiss = vi.fn();
    const run = vi.fn();
    const view = await render(
      <StatusBanner
        kind="warn"
        message="Launched with warnings"
        action={{ label: "Review requirements", run }}
        onDismiss={onDismiss}
      />,
    );

    const button = Array.from(view.host.querySelectorAll("button")).find(
      (b) => b.textContent === "Review requirements",
    );
    expect(button).toBeDefined();
    await act(async () => {
      button!.click();
    });
    expect(run).toHaveBeenCalledTimes(1);
    expect(onDismiss).not.toHaveBeenCalled();
    await view.unmount();
  });
});

describe("banner timeouts", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  async function renderMount(ui: ReactElement) {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);
    await act(async () => {
      root.render(ui);
    });
    return {
      async rerender(next: ReactElement) {
        await act(async () => {
          root.render(next);
        });
      },
      async unmount() {
        await act(async () => root.unmount());
        host.remove();
      },
    };
  }

  it("uses sensible defaults and respects explicit null or custom values", () => {
    expect(DEFAULT_BANNER_TIMEOUTS).toEqual({ info: 10, warn: 300, error: null });
    expect(bannerTimeoutFor("info", null)).toBe(10);
    expect(bannerTimeoutFor("warn", null)).toBe(300);
    expect(bannerTimeoutFor("error", null)).toBeNull();

    const custom = {
      banner_timeouts: { info: null, warn: 45, error: 15 },
    } as Settings;
    expect(bannerTimeoutFor("info", custom)).toBeNull();
    expect(bannerTimeoutFor("warn", custom)).toBe(45);
    expect(bannerTimeoutFor("error", custom)).toBe(15);
  });

  it("auto-dismisses after timeoutSeconds and does not reset on onDismiss identity change", async () => {
    vi.useFakeTimers();
    const onDismiss1 = vi.fn();
    const onDismiss2 = vi.fn();
    const view = await renderMount(
      <StatusBanner
        kind="info"
        message="Closing Cyberpunk 2077…"
        timeoutSeconds={10}
        onDismiss={onDismiss1}
      />,
    );

    await act(async () => {
      vi.advanceTimersByTime(6000);
    });
    expect(onDismiss1).not.toHaveBeenCalled();

    await view.rerender(
      <StatusBanner
        kind="info"
        message="Closing Cyberpunk 2077…"
        timeoutSeconds={10}
        onDismiss={onDismiss2}
      />,
    );

    await act(async () => {
      vi.advanceTimersByTime(4000);
    });
    expect(onDismiss1).not.toHaveBeenCalled();
    expect(onDismiss2).toHaveBeenCalledTimes(1);
    await view.unmount();
  });

  it("resets timer when message changes and never auto-dismisses when null", async () => {
    vi.useFakeTimers();
    const onDismiss = vi.fn();
    const view = await renderMount(
      <StatusBanner
        kind="info"
        message="First"
        timeoutSeconds={10}
        onDismiss={onDismiss}
      />,
    );

    await act(async () => {
      vi.advanceTimersByTime(7000);
    });
    await view.rerender(
      <StatusBanner
        kind="info"
        message="Second"
        timeoutSeconds={10}
        onDismiss={onDismiss}
      />,
    );

    await act(async () => {
      vi.advanceTimersByTime(5000);
    });
    expect(onDismiss).not.toHaveBeenCalled();

    await act(async () => {
      vi.advanceTimersByTime(5000);
    });
    expect(onDismiss).toHaveBeenCalledTimes(1);

    await view.rerender(
      <StatusBanner
        kind="error"
        message="Persistent error"
        timeoutSeconds={null}
        onDismiss={onDismiss}
      />,
    );
    await act(async () => {
      vi.advanceTimersByTime(600_000);
    });
    expect(onDismiss).toHaveBeenCalledTimes(1);
    await view.unmount();
  });
});

describe("banner stack", () => {
  it("stacks amber and red banners newest-first while replacing green banners", () => {
    let stack: StackedBanner[] = [];
    stack = pushBanner(stack, { kind: "info", message: "Deployed." }, 1);
    stack = pushBanner(stack, { kind: "warn", message: "Warning 1" }, 2);
    stack = pushBanner(stack, { kind: "error", message: "Error 1" }, 3);
    stack = pushBanner(stack, { kind: "warn", message: "Warning 2" }, 4);
    expect(stack.map((b) => [b.id, b.kind, b.message])).toEqual([
      [4, "warn", "Warning 2"],
      [3, "error", "Error 1"],
      [2, "warn", "Warning 1"],
      [1, "info", "Deployed."],
    ]);

    // A new green banner replaces the old green banner, keeping amber/red stacked.
    stack = pushBanner(stack, { kind: "info", message: "Saved." }, 5);
    expect(stack.map((b) => [b.id, b.kind, b.message])).toEqual([
      [5, "info", "Saved."],
      [4, "warn", "Warning 2"],
      [3, "error", "Error 1"],
      [2, "warn", "Warning 1"],
    ]);
  });

  it("deduplicates identical amber/red banners, caps stack size, and dismisses by id", () => {
    let stack: StackedBanner[] = [];
    stack = pushBanner(stack, { kind: "error", message: "Same error", details: ["A"] }, 1, 2);
    stack = pushBanner(stack, { kind: "warn", message: "Warn 1" }, 2, 2);
    // Identical error refreshes to top without duplicating
    stack = pushBanner(stack, { kind: "error", message: "Same error", details: ["A"] }, 3, 2);
    expect(stack.map((b) => b.id)).toEqual([3, 2]);

    // Adding a 3rd warn/error drops the oldest stacked item (maxStacked = 2)
    stack = pushBanner(stack, { kind: "error", message: "Error 2" }, 4, 2);
    expect(stack.map((b) => b.id)).toEqual([4, 3]);

    stack = dismissBanner(stack, 4);
    expect(stack.map((b) => b.id)).toEqual([3]);
  });

  it("filters persistable banners by persistence mode and dismisses by downloadId", () => {
    const stack: StackedBanner[] = [
      { id: 1, kind: "info", message: "Done", createdAt: 1000 },
      { id: 2, kind: "warn", message: "Warn", createdAt: 2000 },
      {
        id: 3,
        kind: "error",
        message: "Download failed",
        downloadId: "dl-1",
        createdAt: 3000,
      },
    ];

    expect(bannerPersistenceFor(undefined)).toBe("errors");
    expect(filterPersistableBanners(stack, "off")).toEqual([]);
    expect(filterPersistableBanners(stack, "errors")).toEqual([
      {
        id: 3,
        kind: "error",
        message: "Download failed",
        details: undefined,
        download_id: "dl-1",
        created_at: 3000,
      },
    ]);
    expect(filterPersistableBanners(stack, "errors_and_warnings").map((b) => b.id)).toEqual([
      2, 3,
    ]);

    expect(dismissBannersForDownload(stack, "dl-1").map((b) => b.id)).toEqual([1, 2]);
  });

  it("computes remaining timeout across restarts and prunes expired banners", () => {
    const settings = {
      banner_timeouts: { info: 10, warn: 60, error: null },
    } as Settings;

    const warnBanner: StackedBanner = {
      id: 1,
      kind: "warn",
      message: "Warning",
      createdAt: 10_000,
    };
    const errorBanner: StackedBanner = {
      id: 2,
      kind: "error",
      message: "Error",
      createdAt: 10_000,
    };

    expect(remainingBannerTimeoutSeconds(warnBanner, settings, 25_000)).toBe(45);
    expect(remainingBannerTimeoutSeconds(errorBanner, settings, 999_000)).toBeNull();
    expect(remainingBannerTimeoutSeconds(warnBanner, settings, 5_000)).toBe(60);

    expect(
      pruneExpiredBanners([warnBanner, errorBanner], settings, 70_000).map((b) => b.id),
    ).toEqual([2]);
    expect(
      pruneExpiredBanners([warnBanner, errorBanner], settings, 69_000).map((b) => b.id),
    ).toEqual([1, 2]);
  });
});
