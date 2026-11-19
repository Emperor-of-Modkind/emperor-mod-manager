import { describe, expect, it } from "vitest";
import {
  fallbackGameTitle,
  formatCollectionGameVersions,
  groupSavedCollectionsByGame,
  savedCollectionGameKey,
  savedCollectionMatchesGame,
} from "./savedCollectionGroups";
import type {
  DetectedGame,
  GamePluginInfo,
  ManagedGame,
  SavedCollectionEntry,
} from "./types";

function entry(
  partial: Partial<SavedCollectionEntry> & Pick<SavedCollectionEntry, "id" | "name">,
): SavedCollectionEntry {
  return {
    code: "#emperor1\nabc",
    created_at: "2024-01-01T00:00:00Z",
    updated_at: "2024-01-01T00:00:00Z",
    game: {},
    mod_count: 1,
    nexus_count: 1,
    thunderstore_count: 0,
    modio_count: 0,
    ...partial,
  };
}

const managedSkyrim: ManagedGame = {
  id: "skyrim-1",
  title: "Skyrim Special Edition",
  nexus_domain: "skyrimspecialedition",
  install_path: "/games/skyrim",
  launcher: "steam",
  plugin_id: "skyrimse",
  cover_path: "/covers/skyrim.jpg",
};

describe("savedCollectionGameKey", () => {
  it("prefers nexus domain", () => {
    expect(
      savedCollectionGameKey(
        entry({
          id: "1",
          name: "A",
          game: {
            nexus_domain: "SkyrimSpecialEdition",
            thunderstore_community: "lethal-company",
          },
        }),
      ),
    ).toBe("nexus:skyrimspecialedition");
  });
});

describe("groupSavedCollectionsByGame", () => {
  it("groups by game and resolves managed cover/title", () => {
    const groups = groupSavedCollectionsByGame(
      [
        entry({
          id: "a",
          name: "Pack A",
          game: { nexus_domain: "skyrimspecialedition" },
          created_at: "2024-02-01T00:00:00Z",
        }),
        entry({
          id: "b",
          name: "Pack B",
          game: { nexus_domain: "skyrimspecialedition" },
          created_at: "2024-03-01T00:00:00Z",
        }),
        entry({
          id: "c",
          name: "Lethal",
          game: { thunderstore_community: "lethal-company" },
        }),
      ],
      [managedSkyrim],
      [],
      [],
    );
    expect(groups).toHaveLength(2);
    const skyrim = groups.find((g) => g.key === "nexus:skyrimspecialedition");
    expect(skyrim?.title).toBe("Skyrim Special Edition");
    expect(skyrim?.coverPath).toBe("/covers/skyrim.jpg");
    expect(skyrim?.managed?.id).toBe("skyrim-1");
    expect(skyrim?.collections.map((c) => c.id)).toEqual(["b", "a"]);
  });

  it("resolves plugin display name when unmanaged", () => {
    const plugins: GamePluginInfo[] = [
      {
        id: "nomanssky",
        display_name: "No Man's Sky",
        nexus_domain: "nomanssky",
      },
    ];
    const groups = groupSavedCollectionsByGame(
      [
        entry({
          id: "nms",
          name: "NMS pack",
          game: { nexus_domain: "nomanssky" },
        }),
      ],
      [],
      [],
      plugins,
    );
    expect(groups[0]?.title).toBe("No Man's Sky");
    expect(groups[0]?.managed).toBeNull();
  });

  it("uses detected cover when available", () => {
    const detected: DetectedGame[] = [
      {
        id: "det-1",
        title: "Detected Skyrim",
        install_path: "/g",
        launcher: "steam",
        supported: true,
        plugin_id: "skyrimse",
        nexus_domain: "skyrimspecialedition",
        cover_path: "/det.jpg",
        engine_hint: null,
      },
    ];
    const groups = groupSavedCollectionsByGame(
      [
        entry({
          id: "x",
          name: "X",
          game: { nexus_domain: "skyrimspecialedition" },
        }),
      ],
      [],
      detected,
      [],
    );
    expect(groups[0]?.title).toBe("Detected Skyrim");
    expect(groups[0]?.coverPath).toBe("/det.jpg");
  });

  it("uses a catalog poster when the detected game has no local file", () => {
    const detected: DetectedGame[] = [
      {
        id: "det-1",
        title: "Detected Skyrim",
        install_path: "/g",
        launcher: "steam",
        supported: true,
        plugin_id: "skyrimse",
        nexus_domain: "skyrimspecialedition",
        cover_path: null,
        cover_url: "https://images.nexusmods.com/images/games/v2/1704/tile.jpg",
        engine_hint: null,
      },
    ];
    const groups = groupSavedCollectionsByGame(
      [
        entry({
          id: "x",
          name: "X",
          game: { nexus_domain: "skyrimspecialedition" },
        }),
      ],
      [],
      detected,
      [],
    );
    expect(groups[0]?.coverPath).toBe(
      "https://images.nexusmods.com/images/games/v2/1704/tile.jpg",
    );
  });
});

describe("savedCollectionMatchesGame", () => {
  it("matches nexus domain", () => {
    expect(
      savedCollectionMatchesGame(
        entry({
          id: "1",
          name: "A",
          game: { nexus_domain: "skyrimspecialedition" },
        }),
        managedSkyrim,
      ),
    ).toBe(true);
  });
});

describe("formatCollectionGameVersions", () => {
  it("joins versions", () => {
    expect(formatCollectionGameVersions(["1.6.1170", "AE"])).toBe(
      "1.6.1170, AE",
    );
  });
  it("returns null when empty", () => {
    expect(formatCollectionGameVersions([])).toBeNull();
    expect(formatCollectionGameVersions(null)).toBeNull();
  });
});

describe("fallbackGameTitle", () => {
  it("falls back through hint fields", () => {
    expect(fallbackGameTitle({ thunderstore_community: "lethal-company" })).toBe(
      "lethal-company",
    );
  });

  it("prefers the stored game title", () => {
    expect(
      fallbackGameTitle({
        title: "My Unity Game",
        plugin_id: "bepinex",
        nexus_domain: "some-domain",
      }),
    ).toBe("My Unity Game");
  });
});

describe("generic engine titles", () => {
  const genericPlugin: GamePluginInfo = {
    id: "bepinex",
    display_name: "Unity / BepInEx (generic)",
    nexus_domain: "",
  };

  it("keeps two generic BepInEx shares in separate groups", () => {
    const groups = groupSavedCollectionsByGame(
      [
        entry({
          id: "a",
          name: "Pack A",
          game: { plugin_id: "bepinex", title: "Game One" },
        }),
        entry({
          id: "b",
          name: "Pack B",
          game: { plugin_id: "bepinex", title: "Game Two" },
        }),
      ],
      [],
      [],
      [genericPlugin],
    );
    expect(groups).toHaveLength(2);
    expect(groups.map((g) => g.title).sort()).toEqual(["Game One", "Game Two"]);
  });

  it("matches a managed game by title when catalog ids are empty", () => {
    const gameOne: ManagedGame = {
      id: "u1",
      title: "Game One",
      nexus_domain: "",
      install_path: "/games/one",
      launcher: "steam",
      plugin_id: "bepinex",
    };
    const gameTwo: ManagedGame = {
      ...gameOne,
      id: "u2",
      title: "Game Two",
      install_path: "/games/two",
    };
    const pack = entry({
      id: "a",
      name: "Pack",
      game: { plugin_id: "bepinex", title: "game one" },
    });
    expect(savedCollectionMatchesGame(pack, gameOne)).toBe(true);
    expect(savedCollectionMatchesGame(pack, gameTwo)).toBe(false);
  });
});
