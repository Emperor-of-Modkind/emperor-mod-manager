import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", () => ({
  api: {
    getLauncherInstallOptions: vi.fn(),
    requestLauncherInstall: vi.fn(),
  },
}));

import { api } from "./api";
import { LauncherInstallModal } from "./LauncherInstallModal";
import type { LibraryGame } from "./types";

const steamGame: LibraryGame = {
  id: "s1",
  title: "Skyrim Special Edition",
  source: "steam",
  launcher: "Steam",
  external_id: "489830",
  supported: true,
  plugin_id: "skyrimse",
  nexus_domain: "skyrimspecialedition",
  cover_path: null,
  cover_url: null,
  install_size_bytes: null,
  platforms: ["windows"],
  can_cli_install: false,
  metadata: { dlcs: [], languages: [] },
};

const heroicGame: LibraryGame = {
  id: "h1",
  title: "Control",
  source: "heroic_epic",
  launcher: "Heroic (Epic)",
  external_id: "Crab",
  supported: true,
  plugin_id: "control",
  nexus_domain: "control",
  cover_path: null,
  cover_url: null,
  install_size_bytes: 42_000_000_000,
  platforms: ["windows"],
  can_cli_install: true,
  metadata: {
    heroic_runner: "legendary",
    dlcs: [{ app_name: "CrabDLC1", title: "The Foundation" }],
    languages: [],
  },
};

describe("LauncherInstallModal", () => {
  let host: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    vi.clearAllMocks();
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(async () => {
    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("shows Steam library folders and triggers steam:// handoff", async () => {
    vi.mocked(api.getLauncherInstallOptions).mockResolvedValue({
      source: "steam",
      external_id: "489830",
      title: "Skyrim Special Edition",
      default_install_dir: null,
      default_dir_free_bytes: null,
      steam_libraries: [{ path: "/home/user/.steam/steam", free_bytes: 120_000_000_000 }],
      platforms: ["windows"],
      dlcs: [],
      languages: [],
      cli_available: false,
      cli_name: null,
      handoff_url: "steam://install/489830",
      handoff_note: "Steam's native install dialog will open.",
    });
    vi.mocked(api.requestLauncherInstall).mockResolvedValue({
      job_id: null,
      method: "handoff",
      handoff_url: "steam://install/489830",
      message: "Requested Steam to install Skyrim Special Edition.",
    });

    const onInstallStarted = vi.fn();
    await act(async () => {
      root.render(
        <LauncherInstallModal
          game={steamGame}
          coverSrc={null}
          progress={null}
          onClose={() => {}}
          onRescan={() => {}}
          onInstallStarted={onInstallStarted}
          onCancelInstall={() => {}}
        />,
      );
    });

    expect(document.body.textContent).toContain("/home/user/.steam/steam");
    const installBtn = Array.from(document.body.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Install in Steam"),
    );
    expect(installBtn).toBeDefined();

    await act(async () => {
      installBtn?.click();
    });

    expect(api.requestLauncherInstall).toHaveBeenCalledWith(
      expect.objectContaining({
        source: "steam",
        external_id: "489830",
        method: "handoff",
      }),
    );
    expect(onInstallStarted).toHaveBeenCalled();
    expect(document.body.querySelector('[data-testid="install-handoff-confirmation"]')).not.toBeNull();
  });

  it("shows Heroic CLI install folder & DLC options and submits heroic_cli request", async () => {
    vi.mocked(api.getLauncherInstallOptions).mockResolvedValue({
      source: "heroic_epic",
      external_id: "Crab",
      title: "Control",
      default_install_dir: "/games/Heroic",
      default_dir_free_bytes: 500_000_000_000,
      steam_libraries: [],
      platforms: ["windows"],
      dlcs: [{ app_name: "CrabDLC1", title: "The Foundation" }],
      languages: [],
      cli_available: true,
      cli_name: "legendary",
      handoff_url: "heroic://launch?appName=Crab&runner=legendary",
      handoff_note: "Direct CLI or Heroic GUI.",
    });
    vi.mocked(api.requestLauncherInstall).mockResolvedValue({
      job_id: "job-1",
      method: "heroic_cli",
      handoff_url: null,
      message: "Started installing Control via legendary.",
    });

    await act(async () => {
      root.render(
        <LauncherInstallModal
          game={heroicGame}
          coverSrc={null}
          progress={null}
          onClose={() => {}}
          onRescan={() => {}}
          onInstallStarted={() => {}}
          onCancelInstall={() => {}}
        />,
      );
    });

    const dlcCheckbox = document.body.querySelector<HTMLInputElement>(
      '.install-dlc-list input[type="checkbox"]',
    );
    expect(dlcCheckbox).not.toBeNull();
    await act(async () => {
      dlcCheckbox?.click();
    });

    const installBtn = Array.from(document.body.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Install via legendary"),
    );
    expect(installBtn).toBeDefined();

    await act(async () => {
      installBtn?.click();
    });

    expect(api.requestLauncherInstall).toHaveBeenCalledWith(
      expect.objectContaining({
        source: "heroic_epic",
        external_id: "Crab",
        method: "heroic_cli",
        install_dir: "/games/Heroic",
        dlc_ids: ["CrabDLC1"],
      }),
    );
  });
});
