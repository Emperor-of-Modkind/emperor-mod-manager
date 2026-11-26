import { createRoot, type Root } from "react-dom/client";
import { act, type ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("./api", () => ({
  api: {
    setBackgroundSettings: vi.fn(),
  },
}));

import { api } from "./api";
import {
  BackgroundTraySettings,
  DEFAULT_BACKGROUND_SETTINGS,
} from "./BackgroundTraySettings";

const setBackgroundSettingsMock = vi.mocked(api.setBackgroundSettings);

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

function switches(host: HTMLElement): HTMLButtonElement[] {
  return [...host.querySelectorAll<HTMLButtonElement>('button[role="switch"]')];
}

describe("BackgroundTraySettings", () => {
  beforeEach(() => {
    setBackgroundSettingsMock.mockReset();
    setBackgroundSettingsMock.mockResolvedValue(undefined);
  });

  it("disables sub-options when run_in_background is off and enables them when on", async () => {
    const onUpdated = vi.fn();
    const view = await render(
      <BackgroundTraySettings
        value={DEFAULT_BACKGROUND_SETTINGS}
        trayAvailable={true}
        onUpdated={onUpdated}
      />,
    );

    const buttons = switches(view.host);
    expect(buttons).toHaveLength(5);
    expect(buttons[0].disabled).toBe(false);
    expect(buttons[0].getAttribute("aria-checked")).toBe("false");
    for (const sub of buttons.slice(1)) {
      expect(sub.disabled).toBe(true);
    }

    await act(async () => {
      buttons[0].click();
    });

    expect(setBackgroundSettingsMock).toHaveBeenCalledWith({
      ...DEFAULT_BACKGROUND_SETTINGS,
      run_in_background: true,
    });
    expect(onUpdated).toHaveBeenCalledTimes(1);

    await view.unmount();
  });

  it("toggles close_to_tray, minimize_to_tray, start_on_login, and start_minimized when active", async () => {
    const onUpdated = vi.fn();
    const active = {
      ...DEFAULT_BACKGROUND_SETTINGS,
      run_in_background: true,
    };
    const view = await render(
      <BackgroundTraySettings
        value={active}
        trayAvailable={true}
        onUpdated={onUpdated}
      />,
    );

    const buttons = switches(view.host);
    for (const btn of buttons) {
      expect(btn.disabled).toBe(false);
    }

    // Toggle minimize_to_tray (index 2)
    await act(async () => {
      buttons[2].click();
    });
    expect(setBackgroundSettingsMock).toHaveBeenCalledWith({
      ...active,
      minimize_to_tray: true,
    });

    // Toggle start_on_login (index 3)
    await act(async () => {
      buttons[3].click();
    });
    expect(setBackgroundSettingsMock).toHaveBeenCalledWith({
      ...active,
      start_on_login: true,
    });

    await view.unmount();
  });

  it("shows a notice and disables all switches when tray support is unavailable", async () => {
    const view = await render(
      <BackgroundTraySettings
        value={DEFAULT_BACKGROUND_SETTINGS}
        trayAvailable={false}
        onUpdated={vi.fn()}
      />,
    );

    expect(view.host.querySelector('[role="status"]')?.textContent).toContain(
      "System tray support was not detected",
    );
    for (const btn of switches(view.host)) {
      expect(btn.disabled).toBe(true);
    }

    await view.unmount();
  });
});
