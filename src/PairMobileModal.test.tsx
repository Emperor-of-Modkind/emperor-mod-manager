import { afterEach, describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";
import { MobileStatusBar } from "./MobileStatusBar";

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

afterEach(() => {
  vi.clearAllMocks();
});

describe("MobileStatusBar", () => {
  it("renders LAN direct connection pill", () => {
    const html = renderToStaticMarkup(
      <MobileStatusBar
        state={{
          paired: true,
          desktop_name: "Gaming-PC",
          desktop_peer_id: "peer-1",
          mode: "lan_direct",
          last_synced_at: "2026-11-23T19:00:00Z",
          last_error: null,
          allow_remote_launch: false,
        }}
      />,
    );
    expect(html).toContain("Connected · LAN (Gaming-PC)");
    expect(html).toContain("mode-lan_direct");
  });

  it("renders iroh relay and offline cached states", () => {
    const relayHtml = renderToStaticMarkup(
      <MobileStatusBar
        state={{
          paired: true,
          desktop_name: "Gaming-PC",
          desktop_peer_id: "peer-1",
          mode: "iroh_relay",
          last_synced_at: null,
          last_error: null,
          allow_remote_launch: true,
        }}
      />,
    );
    expect(relayHtml).toContain("Connected · Relay (Gaming-PC)");

    const offlineHtml = renderToStaticMarkup(
      <MobileStatusBar
        state={{
          paired: true,
          desktop_name: "Gaming-PC",
          desktop_peer_id: "peer-1",
          mode: "disconnected",
          last_synced_at: null,
          last_error: "unreachable",
          allow_remote_launch: false,
        }}
      />,
    );
    expect(offlineHtml).toContain("Offline · Cached");
  });

  it("renders nothing when unpaired", () => {
    const html = renderToStaticMarkup(
      <MobileStatusBar
        state={{
          paired: false,
          desktop_name: null,
          desktop_peer_id: null,
          mode: "disconnected",
          last_synced_at: null,
          last_error: null,
          allow_remote_launch: false,
        }}
      />,
    );
    expect(html).toBe("");
  });
});
