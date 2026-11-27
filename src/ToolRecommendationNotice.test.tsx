import { describe, expect, it, vi } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { ToolRecommendationNotice } from "./ToolRecommendationNotice";
import type { PluginToolRecommendation } from "./types";

const sampleRecs: PluginToolRecommendation[] = [
  {
    tool_id: "darktide_mod_loader",
    tool_name: "Darktide Mod Loader",
    reason: "Required to patch Darktide's bundle database and load enabled mods.",
  },
  {
    tool_id: "darktide_mod_framework",
    tool_name: "Darktide Mod Framework (DMF)",
    reason: "Required by Darktide mods for hooks, options menus, and keybinds.",
  },
];

describe("ToolRecommendationNotice", () => {
  it("renders recommended tools, Open Tools button, and dismiss button", () => {
    const html = renderToStaticMarkup(
      <ToolRecommendationNotice
        recommendations={sampleRecs}
        onOpenTools={vi.fn()}
        onDismiss={vi.fn()}
      />,
    );
    expect(html).toContain("Recommended tools");
    expect(html).toContain("Darktide Mod Loader");
    expect(html).toContain("Darktide Mod Framework (DMF)");
    expect(html).toContain("Open Tools");
    expect(html).toContain('aria-label="Dismiss"');
  });

  it("returns empty markup when there are no recommendations", () => {
    const html = renderToStaticMarkup(
      <ToolRecommendationNotice recommendations={[]} onOpenTools={vi.fn()} />,
    );
    expect(html).toBe("");
  });
});
