// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";

import { ModConfigDialog } from "./ModConfigDialog";
import type { StagedMod } from "./types";

const invokeMock = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...args: unknown[]) => invokeMock(...args),
}));

const sampleMod: StagedMod = {
  id: "mod-sc",
  name: "Seamless Co-op",
  version: "1.8.9",
  enabled: true,
  order: 0,
  nexus_mod_id: 510,
  nexus_file_id: 1,
  domain: "eldenring",
  staging_path: "/staging/sc",
  source: "nexus",
};

describe("ModConfigDialog", () => {
  let container: HTMLDivElement;
  let root: Root;

  beforeEach(() => {
    (globalThis as { IS_REACT_ACT_ENVIRONMENT?: boolean }).IS_REACT_ACT_ENVIRONMENT = true;
    container = document.createElement("div");
    document.body.appendChild(container);
    root = createRoot(container);
    invokeMock.mockReset();
  });

  afterEach(async () => {
    await act(async () => {
      root.unmount();
    });
    container.remove();
  });

  it("loads ersc_settings.ini in Form mode, highlights empty cooppassword, and saves edits", async () => {
    invokeMock.mockImplementation((cmd: string) => {
      if (cmd === "list_mod_config_files") {
        return Promise.resolve([
          {
            rel_path: "SeamlessCoop/ersc_settings.ini",
            name: "ersc_settings.ini",
            content: "[PASSWORD]\n; Set a session password\ncooppassword =\n",
          },
        ]);
      }
      if (cmd === "save_mod_config_file") {
        return Promise.resolve(undefined);
      }
      return Promise.reject(new Error(`unexpected ${cmd}`));
    });

    const onClose = vi.fn();
    const onSaved = vi.fn();

    await act(async () => {
      root.render(
        <ModConfigDialog
          gameId="er-1"
          mod={sampleMod}
          onClose={onClose}
          onSaved={onSaved}
        />,
      );
    });

    expect(document.body.textContent).toContain("Required");
    expect(document.body.textContent).toContain("Set a session password");

    const input = document.body.querySelector("input[type='text']") as HTMLInputElement;
    expect(input).not.toBeNull();

    await act(async () => {
      const setter = Object.getOwnPropertyDescriptor(
        window.HTMLInputElement.prototype,
        "value",
      )?.set;
      setter?.call(input, "tarnished");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });

    const saveBtn = Array.from(document.body.querySelectorAll("button")).find((b) =>
      b.textContent?.includes("Save config"),
    );
    expect(saveBtn).toBeDefined();

    await act(async () => {
      saveBtn?.dispatchEvent(new MouseEvent("click", { bubbles: true }));
    });

    expect(invokeMock).toHaveBeenCalledWith("save_mod_config_file", {
      gameId: "er-1",
      modId: "mod-sc",
      relPath: "SeamlessCoop/ersc_settings.ini",
      content: "[PASSWORD]\n; Set a session password\ncooppassword = tarnished\n",
    });
    expect(onSaved).toHaveBeenCalledWith("ersc_settings.ini");
    expect(onClose).toHaveBeenCalled();
  });
});
