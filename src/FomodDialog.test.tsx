import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { FomodDialog } from "./FomodDialog";
import type { FomodInstallerView, StagedMod } from "./types";

const getFomodInstaller = vi.fn();
const setFomodSelection = vi.fn();

vi.mock("./api", () => ({
  api: {
    getFomodInstaller: (...args: unknown[]) => getFomodInstaller(...args),
    setFomodSelection: (...args: unknown[]) => setFomodSelection(...args),
  },
}));

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: (p: string) => `asset://${p}`,
}));

const mod: StagedMod = {
  id: "1_1",
  name: "True Cyberpsychosis",
  nexus_mod_id: 1,
  nexus_file_id: 1,
  version: "1.0",
  domain: "cyberpunk2077",
  staging_path: "/tmp/mod",
  enabled: true,
  order: 1,
};

const view: FomodInstallerView = {
  module_name: "True Cyberpsychosis",
  unsupported_scripted: false,
  selection: {
    selected: {
      "0-voice-language.0-language": ["0-voice-language.0-language.0-english"],
      "1-voice-set.0-voices": ["1-voice-set.0-voices.0-all-voices"],
    },
  },
  steps: [
    {
      id: "0-voice-language",
      name: "Voice Language",
      visible: true,
      groups: [
        {
          id: "0-voice-language.0-language",
          name: "Language",
          type: "SelectExactlyOne",
          plugins: [
            {
              id: "0-voice-language.0-language.0-english",
              name: "English",
              description: "English voices",
              image: null,
              plugin_type: "Recommended",
            },
            {
              id: "0-voice-language.0-language.1-russian",
              name: "Russian",
              description: "Russian voices",
              image: null,
              plugin_type: "Optional",
            },
          ],
        },
      ],
    },
    {
      id: "1-voice-set",
      name: "Voice Set",
      visible: true,
      groups: [
        {
          id: "1-voice-set.0-voices",
          name: "Voices",
          type: "SelectExactlyOne",
          plugins: [
            {
              id: "1-voice-set.0-voices.0-all-voices",
              name: "All voices",
              description: null,
              image: null,
              plugin_type: "Recommended",
            },
            {
              id: "1-voice-set.0-voices.1-jackie-only",
              name: "Jackie only",
              description: null,
              image: null,
              plugin_type: "Optional",
            },
          ],
        },
      ],
    },
  ],
};

async function flush() {
  await act(async () => {
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("FomodDialog", () => {
  beforeEach(() => {
    getFomodInstaller.mockReset();
    setFomodSelection.mockReset();
    getFomodInstaller.mockResolvedValue(view);
    setFomodSelection.mockImplementation((_g: string, _m: string, sel: unknown) =>
      Promise.resolve(sel),
    );
  });

  it("walks steps and saves the selection", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);
    const onSaved = vi.fn();
    const onClose = vi.fn();

    await act(async () => {
      root.render(
        <FomodDialog gameId="cyberpunk2077" mod={mod} onClose={onClose} onSaved={onSaved} />,
      );
    });
    await flush();

    expect(document.body.textContent).toContain("English");
    expect(document.body.textContent).toContain("Voice Language");

    const russian = Array.from(document.body.querySelectorAll("label")).find((el) =>
      el.textContent?.includes("Russian"),
    );
    expect(russian).toBeTruthy();
    await act(async () => {
      russian!.querySelector("input")!.click();
    });

    const next = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Next",
    )!;
    await act(async () => {
      next.click();
    });

    expect(document.body.textContent).toContain("Jackie only");
    const jackie = Array.from(document.body.querySelectorAll("label")).find((el) =>
      el.textContent?.includes("Jackie only"),
    )!;
    await act(async () => {
      jackie.querySelector("input")!.click();
    });

    const finish = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Finish",
    )!;
    await act(async () => {
      finish.click();
    });
    await flush();

    expect(setFomodSelection).toHaveBeenCalled();
    const saved = setFomodSelection.mock.calls[0][2] as {
      selected: Record<string, string[]>;
    };
    expect(saved.selected["0-voice-language.0-language"]).toEqual([
      "0-voice-language.0-language.1-russian",
    ]);
    expect(saved.selected["1-voice-set.0-voices"]).toEqual([
      "1-voice-set.0-voices.1-jackie-only",
    ]);
    expect(onSaved).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalled();

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });
});
