import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { api } from "./api";
import { GameToolsPanel } from "./tools";
import type { GameToolsResponse, ManagedGame, PluginToolStatus } from "./types";

const sampleGame: ManagedGame = {
  id: "game-pz-test",
  title: "Project Zomboid",
  launcher: "steam",
  install_path: "/games/ProjectZomboid",
  plugin_id: "projectzomboid",
  nexus_domain: "projectzomboid",
  cover_path: null,
};

const sampleTool: PluginToolStatus = {
  id: "pz3d",
  name: "PZ3D (3D Mod)",
  description: "Standalone 3D mod + ZombieBuddy setup.",
  homepage_url: "https://pz3d.dev/install/guide.html",
  installed: false,
  installed_version: null,
  latest_version: null,
  update_available: false,
  status_detail: null,
  notes: [],
  can_uninstall: true,
};

let mountedRoot: Root | null = null;
let mountedContainer: HTMLDivElement | null = null;

function renderPanel(element: ReactElement) {
  mountedContainer = document.createElement("div");
  document.body.appendChild(mountedContainer);
  mountedRoot = createRoot(mountedContainer);
  act(() => {
    mountedRoot!.render(element);
  });
  return mountedContainer;
}

beforeEach(() => {
  vi.restoreAllMocks();
});

afterEach(() => {
  if (mountedRoot) {
    act(() => {
      mountedRoot!.unmount();
    });
    mountedRoot = null;
  }
  if (mountedContainer) {
    mountedContainer.remove();
    mountedContainer = null;
  }
});

describe("GameToolsPanel plugin tools", () => {
  it("renders plugin tool cards, runs install, and hides Linux Vulkan panels on non-Linux", async () => {
    const initial: GameToolsResponse = {
      platform_linux: false,
      overrides: {
        lsfg_vk: { enabled: false, use_global_settings: true },
        autohdr_vk: { enabled: false, use_global_settings: true },
        executables: [],
      },
      executables: [],
      steam_app_id: null,
      launch_options: null,
      plugin_tools: [sampleTool],
    };
    const installedTool: PluginToolStatus = {
      ...sampleTool,
      installed: true,
      installed_version: "0.10.4",
      status_detail: "ZombieBuddy v2.3.4 · JVM heap: -Xmx6144m",
    };

    const getSpy = vi
      .spyOn(api, "getGameTools")
      .mockResolvedValueOnce(initial)
      .mockResolvedValueOnce({ ...initial, plugin_tools: [installedTool] });
    const installSpy = vi.spyOn(api, "installGamePluginTool").mockResolvedValue({
      tool: installedTool,
      message: "Installed PZ3D 0.10.4 with ZombieBuddy.",
      warnings: [],
    });

    const onModsChanged = vi.fn();
    const container = renderPanel(
      <GameToolsPanel game={sampleGame} onModsChanged={onModsChanged} />,
    );
    await act(async () => {
      await Promise.resolve();
    });

    expect(getSpy).toHaveBeenCalledWith(sampleGame.id);
    expect(container.textContent).toContain("PZ3D (3D Mod)");
    expect(container.textContent).toContain("Not installed");
    expect(container.textContent).not.toContain("lsfg-vk");
    expect(container.textContent).not.toContain("AutoHDR-VK");

    const installBtn = Array.from(container.querySelectorAll("button")).find(
      (b) => b.textContent === "Install",
    );
    expect(installBtn).toBeTruthy();

    await act(async () => {
      installBtn!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(installSpy).toHaveBeenCalledWith(sampleGame.id, "pz3d");
    expect(onModsChanged).toHaveBeenCalledTimes(1);
    expect(container.textContent).toContain("Installed PZ3D 0.10.4 with ZombieBuddy.");
    expect(container.textContent).toContain("0.10.4");
    expect(container.textContent).toContain("Uninstall");
  });
});
