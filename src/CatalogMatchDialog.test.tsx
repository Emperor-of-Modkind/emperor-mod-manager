import { createRoot, type Root } from "react-dom/client";
import { act } from "react";
import { describe, expect, it } from "vitest";

import { CatalogMatchDialog } from "./CatalogMatchDialog";
import type { CatalogSuggestion, DetectedGame } from "./types";

const game: DetectedGame = {
  id: "steam:123",
  title: "Palworld Deluxe Edition",
  install_path: "/games/Palworld",
  launcher: "steam",
  supported: false,
  plugin_id: null,
  nexus_domain: null,
  cover_path: null,
  engine_hint: "unreal",
};

const suggestion: CatalogSuggestion = {
  nexus_domain: "palworld",
  nexus_name: "Palworld",
  thunderstore_community: "palworld",
  thunderstore_name: "Palworld",
  modio_game_id: null,
  modio_name: null,
  ambiguous: true,
  nexus_candidates: [
    { id: "palworld", name: "Palworld", score: 90 },
    { id: "palworldtest", name: "Palworld Test", score: 70 },
  ],
  thunderstore_candidates: [
    { id: "palworld", name: "Palworld", score: 90 },
  ],
  modio_candidates: [],
};

describe("CatalogMatchDialog", () => {
  it("lists close suggestions and applies a pick into catalog fields", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);
    const submitted: unknown[] = [];

    await act(async () => {
      root.render(
        <CatalogMatchDialog
          game={game}
          engine="unreal"
          suggestion={suggestion}
          lookingUp={false}
          projectName="Pal"
          busy={false}
          onClose={() => {}}
          onSubmit={(values) => {
            submitted.push(values);
          }}
        />,
      );
    });

    expect(document.body.textContent).toContain("Match catalog for Palworld Deluxe Edition");
    expect(document.body.textContent).toContain("Palworld Test");
    expect(document.body.textContent).toContain("Several close matches");

    const alt = Array.from(document.querySelectorAll("button.catalog-match-row")).find((btn) =>
      btn.textContent?.includes("Palworld Test"),
    ) as HTMLButtonElement;
    expect(alt).toBeTruthy();
    await act(async () => {
      alt.click();
    });

    const domain = document.querySelector(
      'input[placeholder="e.g. palworld"]',
    ) as HTMLInputElement;
    expect(domain.value).toBe("palworldtest");

    const manage = Array.from(document.querySelectorAll("button")).find(
      (btn) => btn.textContent === "Manage",
    ) as HTMLButtonElement;
    await act(async () => {
      manage.click();
    });

    expect(submitted).toEqual([
      {
        nexusDomain: "palworldtest",
        thunderstoreCommunity: "palworld",
        modioGameId: null,
        projectName: "Pal",
      },
    ]);

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });

  it("explains when no close catalog page was found", async () => {
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root: Root = createRoot(host);

    await act(async () => {
      root.render(
        <CatalogMatchDialog
          game={game}
          engine="bepinex"
          suggestion={{
            nexus_domain: null,
            nexus_name: null,
            thunderstore_community: null,
            thunderstore_name: null,
            modio_game_id: null,
            modio_name: null,
            nexus_candidates: [],
            thunderstore_candidates: [],
            modio_candidates: [],
            ambiguous: false,
          }}
          lookingUp={false}
          busy={false}
          onClose={() => {}}
          onSubmit={() => {}}
        />,
      );
    });

    expect(document.body.textContent).toContain("could not match a catalog page");
    expect(document.body.textContent).toContain("No close catalog page found");

    await act(async () => {
      root.unmount();
    });
    host.remove();
  });
});
