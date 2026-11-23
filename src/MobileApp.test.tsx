// @vitest-environment jsdom
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, describe, expect, it, vi } from "vitest";

const invoked: string[] = [];

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn((cmd: string) => {
    invoked.push(cmd);
    if (cmd === "get_mobile_connection_state") {
      return Promise.resolve({
        paired: false,
        desktop_name: null,
        desktop_peer_id: null,
        mode: "disconnected",
        last_synced_at: null,
        last_error: null,
        allow_remote_launch: false,
      });
    }
    if (cmd === "get_mobile_snapshot") {
      return Promise.resolve(null);
    }
    if (cmd === "discover_lan_desktops") {
      return Promise.resolve([
        {
          host_name: "Gaming-PC",
          desktop_endpoint_id: "peer-abc",
          lan_addrs: ["192.168.1.50:12345"],
          pairing_active: true,
          app_version: "0.9.2",
        },
      ]);
    }
    return Promise.reject(new Error(`unexpected command ${cmd}`));
  }),
}));

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

import MobileApp from "./MobileApp";

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  invoked.length = 0;
  if (root) {
    act(() => root!.unmount());
    root = null;
  }
  if (container) {
    container.remove();
    container = null;
  }
});

describe("MobileApp", () => {
  it("renders MobilePairingView when unpaired and calls zero desktop-only commands", async () => {
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);

    await act(async () => {
      root!.render(
        <MobileApp
          appInfo={{
            name: "emperor-mod-manager",
            version: "0.9.2",
            platform: "android",
            is_mobile: true,
            linux_only: false,
          }}
        />,
      );
    });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(container.textContent).toContain("Pair with Desktop");
    expect(container.textContent).toContain("Gaming-PC");
    expect(container.textContent).toContain("6-Digit Pairing Code");
    expect(invoked).not.toContain("get_settings");
    expect(invoked).not.toContain("set_p2p_appear_offline");
    expect(invoked).not.toContain("open_nexus_website_login");
    expect(invoked).not.toContain("scan_games");
  });
});
