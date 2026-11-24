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
        <FomodDialog
          gameId="cyberpunk2077"
          mod={mod}
          progress={{ current: 2, total: 5 }}
          onClose={onClose}
          onSaved={onSaved}
        />,
      );
    });
    await flush();

    expect(document.body.textContent).toContain("2 of 5");
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

  it("refreshes conditional step visibility when an earlier option changes", async () => {
    const conditionalView = (showBranchB: boolean): FomodInstallerView => ({
      module_name: "Branching Mod",
      unsupported_scripted: false,
      selection: {
        selected: {
          "0-branch.0-pick": [
            showBranchB ? "0-branch.0-pick.1-b" : "0-branch.0-pick.0-a",
          ],
          "1-only-b.0-sub": ["1-only-b.0-sub.0-patch"],
        },
      },
      steps: [
        {
          id: "0-branch",
          name: "Branch",
          visible: true,
          groups: [
            {
              id: "0-branch.0-pick",
              name: "Pick",
              type: "SelectExactlyOne",
              plugins: [
                {
                  id: "0-branch.0-pick.0-a",
                  name: "Option A",
                  description: null,
                  image: null,
                  plugin_type: "Recommended",
                },
                {
                  id: "0-branch.0-pick.1-b",
                  name: "Option B",
                  description: null,
                  image: null,
                  plugin_type: "Optional",
                },
              ],
            },
          ],
        },
        {
          id: "1-only-b",
          name: "Only B Step",
          visible: showBranchB,
          groups: [
            {
              id: "1-only-b.0-sub",
              name: "Sub",
              type: "SelectExactlyOne",
              plugins: [
                {
                  id: "1-only-b.0-sub.0-patch",
                  name: "B Patch",
                  description: null,
                  image: null,
                  plugin_type: "Recommended",
                },
              ],
            },
          ],
        },
      ],
    });

    getFomodInstaller.mockImplementation(
      (_g: string, _m: string, draft?: { selected: Record<string, string[]> }) => {
        const picked = draft?.selected["0-branch.0-pick"] ?? [];
        return Promise.resolve(conditionalView(picked.includes("0-branch.0-pick.1-b")));
      },
    );

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <FomodDialog
          gameId="cyberpunk2077"
          mod={mod}
          onClose={vi.fn()}
          onSaved={vi.fn()}
        />,
      );
    });
    await flush();

    // With Option A selected, Step 2 is hidden so the primary button is Finish.
    expect(
      Array.from(document.body.querySelectorAll("button")).some((b) => b.textContent === "Finish"),
    ).toBe(true);

    // Selecting Option B refreshes the installer view and reveals Step 2 ("Next").
    const optionB = Array.from(document.body.querySelectorAll("label")).find((el) =>
      el.textContent?.includes("Option B"),
    );
    expect(optionB).toBeTruthy();
    await act(async () => {
      optionB!.querySelector("input")!.click();
    });
    await flush();

    const next = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Next",
    );
    expect(next).toBeTruthy();
    await act(async () => {
      next!.click();
    });

    expect(document.body.textContent).toContain("Only B Step");
    expect(document.body.textContent).toContain("B Patch");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("filters to only new settings on update, supports showing all settings, and preserves hidden picks on finish", async () => {
    const updateView: FomodInstallerView = {
      ...view,
      selection: {
        selected: {
          "0-voice-language.0-language": ["0-voice-language.0-language.1-russian"],
          "1-voice-set.0-voices": ["1-voice-set.0-voices.0-all-voices"],
        },
      },
      pending_review: {
        new_group_ids: ["1-voice-set.0-voices"],
        new_plugin_ids: [],
        invalidated_group_ids: [],
      },
    };
    getFomodInstaller.mockResolvedValue(updateView);

    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);
    const onSaved = vi.fn();
    const onClose = vi.fn();

    await act(async () => {
      root.render(
        <FomodDialog
          gameId="cyberpunk2077"
          mod={mod}
          onClose={onClose}
          onSaved={onSaved}
        />,
      );
    });
    await flush();

    // Unchanged Step 1 ("Voice Language") is skipped; Step 2 ("Voice Set") is shown directly with Finish.
    expect(document.body.textContent).toContain("Voice Set");
    expect(document.body.textContent).not.toContain("English");
    expect(document.body.textContent).toContain("New");
    expect(document.body.textContent).toContain("Show all settings");

    // Toggling "Show all settings" reveals Step 1 and Next/Back navigation.
    const showAll = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Show all settings",
    )!;
    await act(async () => {
      showAll.click();
    });
    const back = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Back",
    );
    expect(back).toBeTruthy();

    // Switch back to only new settings, pick "Jackie only", and click Finish.
    const showOnlyNew = Array.from(document.body.querySelectorAll("button")).find(
      (b) => b.textContent === "Show only new settings",
    )!;
    await act(async () => {
      showOnlyNew.click();
    });

    const jackie = Array.from(document.body.querySelectorAll("label")).find((el) =>
      el.textContent?.includes("Jackie only"),
    )!;
    await act(async () => {
      jackie.querySelector("input")!.click();
    });
    await flush();

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
    // Hidden Step 1 choice ("Russian") is preserved alongside the newly chosen Step 2 pick.
    expect(saved.selected["0-voice-language.0-language"]).toEqual([
      "0-voice-language.0-language.1-russian",
    ]);
    expect(saved.selected["1-voice-set.0-voices"]).toEqual([
      "1-voice-set.0-voices.1-jackie-only",
    ]);
    expect(onSaved).toHaveBeenCalled();
    expect(onClose).toHaveBeenCalledWith("saved");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });
});
