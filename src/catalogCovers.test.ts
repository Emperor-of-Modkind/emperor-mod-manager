import { describe, expect, it } from "vitest";
import { catalogCoverRequests } from "./catalogCovers";
import type { CatalogSuggestion, DetectedGame } from "./types";

function detected(partial: Partial<DetectedGame> & Pick<DetectedGame, "id">): DetectedGame {
  return {
    title: "Game",
    install_path: "/games/game",
    launcher: "Steam",
    supported: false,
    plugin_id: null,
    nexus_domain: null,
    cover_path: null,
    cover_url: null,
    icon_path: null,
    engine_hint: "unreal",
    ...partial,
  };
}

function suggestion(
  partial: Partial<CatalogSuggestion>,
): CatalogSuggestion {
  return {
    nexus_domain: null,
    nexus_name: null,
    thunderstore_community: null,
    thunderstore_name: null,
    modio_game_id: null,
    modio_name: null,
    ...partial,
  };
}

describe("catalogCoverRequests", () => {
  it("skips a game with no stored id and no suggestion", () => {
    expect(catalogCoverRequests([detected({ id: "silver" })], [])).toEqual([]);
  });

  it("requests a poster from a suggestion-only Nexus domain", () => {
    const requests = catalogCoverRequests(
      [detected({ id: "silver", title: "Silver Pines" })],
      [],
      {
        silver: suggestion({
          nexus_domain: "silverpines",
          nexus_name: "Silver Pines",
          ambiguous: true,
        }),
      },
    );
    expect(requests).toEqual([
      {
        id: "silver",
        nexus_domain: "silverpines",
        modio_game_id: null,
        icon_path: null,
      },
    ]);
  });

  it("skips a game that already has a local poster", () => {
    const requests = catalogCoverRequests(
      [
        detected({
          id: "hellraiser",
          cover_path: "/library/hellraiser.jpg",
        }),
      ],
      [],
      { hellraiser: suggestion({ nexus_domain: "clivebarkershellraiserrevival" }) },
    );
    expect(requests).toEqual([]);
  });

  it("still requests when the only local art is an icon or a deferred icon path", () => {
    const requests = catalogCoverRequests(
      [
        detected({
          id: "icon",
          cover_path: "/library/icon.png",
          icon_path: "/library/icon.png",
        }),
        detected({
          id: "deferred",
          cover_path: "/home/luu/.config/heroic/icons/1423049311.png",
        }),
      ],
      [],
      {
        icon: suggestion({ nexus_domain: "valormortis" }),
        deferred: suggestion({ modio_game_id: 42, modio_name: "Valor Mortis" }),
      },
    );
    expect(requests).toEqual([
      {
        id: "icon",
        nexus_domain: "valormortis",
        modio_game_id: null,
        icon_path: "/library/icon.png",
      },
      {
        id: "deferred",
        nexus_domain: null,
        modio_game_id: 42,
        icon_path: null,
      },
    ]);
  });

  it("prefers a stored domain over the suggestion", () => {
    const requests = catalogCoverRequests(
      [detected({ id: "game", nexus_domain: "stored" })],
      [],
      { game: suggestion({ nexus_domain: "suggested" }) },
    );
    expect(requests[0]?.nexus_domain).toBe("stored");
  });
});
