import { describe, expect, it } from "vitest";
import {
  COLLECTION_COVER_PREFIX,
  applyCatalogCovers,
  catalogCoverRequestKey,
  catalogCoverRequests,
  collectionCatalogCoverRequests,
  mergeCollectionCatalogCovers,
  mergeCollectionCatalogTitles,
} from "./catalogCovers";
import type { SavedCollectionGameGroup } from "./savedCollectionGroups";
import type {
  CatalogSuggestion,
  DetectedGame,
  GamePluginInfo,
  SavedCollectionEntry,
} from "./types";

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

function collectionGroup(
  partial: Partial<SavedCollectionGameGroup> & Pick<SavedCollectionGameGroup, "key">,
): SavedCollectionGameGroup {
  const entry: SavedCollectionEntry = {
    id: "c1",
    name: "Pack",
    code: "code",
    created_at: "2026-01-01T00:00:00Z",
    updated_at: "2026-01-01T00:00:00Z",
    game: {},
    mod_count: 1,
    nexus_count: 1,
    thunderstore_count: 0,
    modio_count: 0,
  };
  return {
    title: "Game",
    coverPath: null,
    managed: null,
    collections: [entry],
    ...partial,
  };
}

describe("collectionCatalogCoverRequests", () => {
  it("requests posters for unmanaged/uninstalled collection groups with nexus_domain or modio_game_id", () => {
    const groups = [
      collectionGroup({
        key: "nexus:cyberpunk2077",
        collections: [
          {
            ...collectionGroup({ key: "nexus:cyberpunk2077" }).collections[0]!,
            game: { nexus_domain: "cyberpunk2077" },
          },
        ],
      }),
      collectionGroup({
        key: "modio:42",
        collections: [
          {
            ...collectionGroup({ key: "modio:42" }).collections[0]!,
            game: { modio_game_id: 42 },
          },
        ],
      }),
    ];
    expect(collectionCatalogCoverRequests(groups, [])).toEqual([
      {
        id: `${COLLECTION_COVER_PREFIX}nexus:cyberpunk2077`,
        nexus_domain: "cyberpunk2077",
        modio_game_id: null,
        icon_path: null,
      },
      {
        id: `${COLLECTION_COVER_PREFIX}modio:42`,
        nexus_domain: null,
        modio_game_id: 42,
        icon_path: null,
      },
    ]);
  });

  it("falls back to plugin nexus_domain when hint only has plugin_id", () => {
    const plugins: GamePluginInfo[] = [
      { id: "skyrimse", display_name: "Skyrim SE", nexus_domain: "skyrimspecialedition" },
    ];
    const groups = [
      collectionGroup({
        key: "plugin:skyrimse",
        collections: [
          {
            ...collectionGroup({ key: "plugin:skyrimse" }).collections[0]!,
            game: { plugin_id: "skyrimse" },
          },
        ],
      }),
    ];
    expect(collectionCatalogCoverRequests(groups, plugins)).toEqual([
      {
        id: `${COLLECTION_COVER_PREFIX}plugin:skyrimse`,
        nexus_domain: "skyrimspecialedition",
        modio_game_id: null,
        icon_path: null,
      },
    ]);
  });

  it("skips groups that already have a real cover, but requests when only an icon is present", () => {
    const det = detected({
      id: "det-1",
      icon_path: "/games/icon.png",
    });
    const groups = [
      collectionGroup({
        key: "has-remote",
        coverPath: "https://images.nexusmods.com/images/games/v2/1704/tile.jpg",
        collections: [
          {
            ...collectionGroup({ key: "has-remote" }).collections[0]!,
            game: { nexus_domain: "skyrimspecialedition" },
          },
        ],
      }),
      collectionGroup({
        key: "has-local",
        coverPath: "/covers/skyrim.jpg",
        collections: [
          {
            ...collectionGroup({ key: "has-local" }).collections[0]!,
            game: { nexus_domain: "skyrimspecialedition" },
          },
        ],
      }),
      collectionGroup({
        key: "only-icon",
        coverPath: "/games/icon.png",
        collections: [
          {
            ...collectionGroup({ key: "only-icon" }).collections[0]!,
            game: { nexus_domain: "cyberpunk2077" },
          },
        ],
      }),
    ];
    expect(collectionCatalogCoverRequests(groups, [], [det])).toEqual([
      {
        id: `${COLLECTION_COVER_PREFIX}only-icon`,
        nexus_domain: "cyberpunk2077",
        modio_game_id: null,
        icon_path: null,
      },
    ]);
  });
});

describe("applyCatalogCovers", () => {
  it("re-applies resolved catalog posters onto a fresh scan snapshot", () => {
    const games = [
      detected({ id: "doom", title: "Doom - The Dark Ages" }),
      detected({
        id: "valor",
        title: "Valor Mortis",
        cover_path: "/home/luu/.config/heroic/icons/valor.png",
        icon_path: "/home/luu/.config/heroic/icons/valor.png",
      }),
    ];
    const resolved = new Map([
      ["doom", "https://staticdelivery.nexusmods.com/images/games/v2/doom.jpg"],
      ["valor", "https://staticdelivery.nexusmods.com/images/games/v2/valor.jpg"],
    ]);

    const updated = applyCatalogCovers(games, resolved);
    expect(updated[0]).toMatchObject({
      id: "doom",
      cover_url: "https://staticdelivery.nexusmods.com/images/games/v2/doom.jpg",
      cover_path: null,
    });
    expect(updated[1]).toMatchObject({
      id: "valor",
      cover_url: "https://staticdelivery.nexusmods.com/images/games/v2/valor.jpg",
      cover_path: null,
    });
  });

  it("does not overwrite a real local poster or existing cover_url", () => {
    const games = [
      detected({
        id: "steam-game",
        cover_path: "/steam/librarycache/1234/library_600x900.jpg",
      }),
      detected({
        id: "heroic-game",
        cover_url: "https://heroic.example/poster.jpg",
      }),
    ];
    const resolved = new Map([
      ["steam-game", "https://nexus.example/steam.jpg"],
      ["heroic-game", "https://nexus.example/heroic.jpg"],
    ]);

    expect(applyCatalogCovers(games, resolved)).toBe(games);
  });

  it("prefers cached local cover_path over cover_url when present", () => {
    const games = [
      detected({
        id: "heroic-no-cover",
        cover_path: "/home/u/.config/heroic/icons/999.png",
        icon_path: "/home/u/.config/heroic/icons/999.png",
      }),
    ];
    const resolved = new Map([
      [
        "heroic-no-cover",
        {
          coverPath: "/cache/covers/catalog-nexus-doom.jpg",
          coverUrl: "https://images.nexusmods.com/images/games/v2/doom/tile.jpg",
        },
      ],
    ]);

    const next = applyCatalogCovers(games, resolved);
    expect(next[0]).toMatchObject({
      id: "heroic-no-cover",
      cover_path: "/cache/covers/catalog-nexus-doom.jpg",
      cover_url: null,
    });
  });
});

describe("mergeCollectionCatalogCovers & mergeCollectionCatalogTitles", () => {
  it("strips collection prefix, merges new covers/titles, and preserves reference when unchanged", () => {
    const hits = [
      {
        id: `${COLLECTION_COVER_PREFIX}nexus:stardewvalley`,
        cover_url: "https://images.nexusmods.com/images/games/v2/1303/tile.jpg",
        title: "Stardew Valley",
      },
      {
        id: `${COLLECTION_COVER_PREFIX}modio:42`,
        cover_url: "https://thumb.modcdn.io/42.png",
        title: "  ",
      },
    ];

    const covers1 = mergeCollectionCatalogCovers({}, hits);
    expect(covers1).toEqual({
      "nexus:stardewvalley":
        "https://images.nexusmods.com/images/games/v2/1303/tile.jpg",
      "modio:42": "https://thumb.modcdn.io/42.png",
    });
    expect(mergeCollectionCatalogCovers(covers1, hits)).toBe(covers1);

    const titles1 = mergeCollectionCatalogTitles({}, hits);
    expect(titles1).toEqual({
      "nexus:stardewvalley": "Stardew Valley",
    });
    expect(mergeCollectionCatalogTitles(titles1, hits)).toBe(titles1);
  });

  it("prefers cached cover_path over cover_url in mergeCollectionCatalogCovers", () => {
    const hits = [
      {
        id: `${COLLECTION_COVER_PREFIX}nexus:stardewvalley`,
        cover_url: "https://images.nexusmods.com/images/games/v2/1303/tile.jpg",
        cover_path: "/cache/covers/catalog-nexus-stardewvalley.jpg",
      },
    ];
    expect(mergeCollectionCatalogCovers({}, hits)).toEqual({
      "nexus:stardewvalley": "/cache/covers/catalog-nexus-stardewvalley.jpg",
    });
  });

  it("formats deduplication keys consistently", () => {
    expect(
      catalogCoverRequestKey({
        id: `${COLLECTION_COVER_PREFIX}nexus:stardewvalley`,
        nexus_domain: "stardewvalley",
        modio_game_id: null,
        icon_path: null,
      }),
    ).toBe(`${COLLECTION_COVER_PREFIX}nexus:stardewvalley:stardewvalley:`);
  });
});
