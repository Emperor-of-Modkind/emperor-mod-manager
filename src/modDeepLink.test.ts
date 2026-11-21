import { describe, expect, it } from "vitest";
import {
  EMPEROR_WEB_BASE_URL,
  catalogHitFromModDeepLink,
  extractFriendJoinCode,
  friendDeepLinkFromCode,
  friendShareUrlFromCode,
  managedGameForModDeepLink,
  modDeepLinkFromHit,
  modShareUrlFromHit,
  parseFriendDeepLink,
  parseModDeepLink,
  type EmperorOpenMod,
} from "./modDeepLink";
import type { CatalogHit, ManagedGame } from "./types";

function hit(partial: Partial<CatalogHit> & Pick<CatalogHit, "source" | "id" | "name">): CatalogHit {
  return {
    summary: null,
    picture_url: null,
    author: null,
    downloads: null,
    endorsements: null,
    category: null,
    tags: [],
    mod_id: null,
    domain_name: null,
    community: null,
    namespace: null,
    package_name: null,
    full_name: null,
    package_url: null,
    rating_score: null,
    latest_version: null,
    modio_game_id: null,
    modio_mod_id: null,
    profile_url: null,
    ...partial,
  };
}

function game(partial: Partial<ManagedGame> = {}): ManagedGame {
  return {
    id: "g",
    title: "Game",
    nexus_domain: "skyrimspecialedition",
    install_path: "/games/skyrim",
    launcher: "steam",
    plugin_id: "skyrim",
    thunderstore_community: "skyrimse",
    ...partial,
  };
}

describe("modDeepLinkFromHit", () => {
  it("builds a Nexus link", () => {
    expect(
      modDeepLinkFromHit(
        hit({
          source: "nexus",
          id: "nexus:skyrimspecialedition:12604",
          name: "SkyUI",
          domain_name: "skyrimspecialedition",
          mod_id: 12604,
        }),
      ),
    ).toBe("emperor://mod/nexus/skyrimspecialedition/12604");
  });

  it("builds a Thunderstore link", () => {
    expect(
      modDeepLinkFromHit(
        hit({
          source: "thunderstore",
          id: "thunderstore:lethal-company:BepInEx:BepInExPack",
          name: "BepInExPack",
          community: "lethal-company",
          namespace: "BepInEx",
          package_name: "BepInExPack",
        }),
      ),
    ).toBe("emperor://mod/thunderstore/lethal-company/BepInEx/BepInExPack");
  });

  it("builds a mod.io link", () => {
    expect(
      modDeepLinkFromHit(
        hit({
          source: "modio",
          id: "modio:2248:99",
          name: "Cool",
          modio_game_id: 2248,
          modio_mod_id: 99,
        }),
      ),
    ).toBe("emperor://mod/modio/2248/99");
  });

  it("returns null when identity is incomplete", () => {
    expect(
      modDeepLinkFromHit(
        hit({ source: "nexus", id: "x", name: "x", domain_name: "skyrim" }),
      ),
    ).toBeNull();
  });
});

describe("modShareUrlFromHit", () => {
  it("builds clickable HTTPS redirect URLs for Nexus / Thunderstore / mod.io", () => {
    expect(
      modShareUrlFromHit(
        hit({
          source: "nexus",
          id: "nexus:skyrimspecialedition:12604",
          name: "SkyUI",
          domain_name: "skyrimspecialedition",
          mod_id: 12604,
        }),
      ),
    ).toBe(`${EMPEROR_WEB_BASE_URL}/open/?to=mod/nexus/skyrimspecialedition/12604`);

    expect(
      modShareUrlFromHit(
        hit({
          source: "thunderstore",
          id: "thunderstore:lethal-company:BepInEx:BepInExPack",
          name: "BepInExPack",
          community: "lethal-company",
          namespace: "BepInEx",
          package_name: "BepInExPack",
        }),
      ),
    ).toBe(
      `${EMPEROR_WEB_BASE_URL}/open/?to=mod/thunderstore/lethal-company/BepInEx/BepInExPack`,
    );

    expect(
      modShareUrlFromHit(
        hit({
          source: "modio",
          id: "modio:2248:99",
          name: "Cool",
          modio_game_id: 2248,
          modio_mod_id: 99,
        }),
      ),
    ).toBe(`${EMPEROR_WEB_BASE_URL}/open/?to=mod/modio/2248/99`);
  });

  it("percent-encodes special characters and returns null on incomplete hits", () => {
    expect(
      modShareUrlFromHit(
        hit({
          source: "thunderstore",
          id: "thunderstore:lc:Author:100%Mod",
          name: "100%Mod",
          community: "lc",
          namespace: "Author",
          package_name: "100%Mod",
        }),
      ),
    ).toBe(`${EMPEROR_WEB_BASE_URL}/open/?to=mod/thunderstore/lc/Author/100%25Mod`);

    expect(
      modShareUrlFromHit(
        hit({ source: "nexus", id: "x", name: "x", domain_name: "skyrim" }),
      ),
    ).toBeNull();
  });
});

describe("parseModDeepLink", () => {
  it("round-trips Nexus / Thunderstore / mod.io", () => {
    const cases: Array<[string, EmperorOpenMod]> = [
      [
        "emperor://mod/nexus/skyrimspecialedition/12604",
        { source: "nexus", domain: "skyrimspecialedition", mod_id: 12604 },
      ],
      [
        "emperor://mod/thunderstore/lethal-company/BepInEx/BepInExPack",
        {
          source: "thunderstore",
          community: "lethal-company",
          namespace: "BepInEx",
          package_name: "BepInExPack",
        },
      ],
      [
        "emperor://mod/modio/2248/12345",
        { source: "modio", game_id: 2248, mod_id: 12345 },
      ],
      [
        `${EMPEROR_WEB_BASE_URL}/open/?to=mod/nexus/skyrimspecialedition/12604`,
        { source: "nexus", domain: "skyrimspecialedition", mod_id: 12604 },
      ],
      [
        `${EMPEROR_WEB_BASE_URL}/open/?to=mod/thunderstore/lc/Author/100%25Mod`,
        {
          source: "thunderstore",
          community: "lc",
          namespace: "Author",
          package_name: "100%Mod",
        },
      ],
      [
        `${EMPEROR_WEB_BASE_URL}/open/#mod/modio/2248/12345`,
        { source: "modio", game_id: 2248, mod_id: 12345 },
      ],
    ];
    for (const [url, expected] of cases) {
      expect(parseModDeepLink(url)).toEqual(expected);
    }
  });

  it("rejects garbage and unrelated HTTPS URLs", () => {
    expect(parseModDeepLink("nxm://skyrim/mods/1/files/2")).toBeNull();
    expect(parseModDeepLink("emperor://mod/nexus/onlydomain")).toBeNull();
    expect(parseModDeepLink("emperor://mod/nexus/domain/0")).toBeNull();
    expect(
      parseModDeepLink("https://example.com/open/?to=mod/nexus/skyrimspecialedition/12604"),
    ).toBeNull();
    expect(
      parseModDeepLink(`${EMPEROR_WEB_BASE_URL}/other/?to=mod/nexus/skyrimspecialedition/12604`),
    ).toBeNull();
    expect(
      parseModDeepLink(`${EMPEROR_WEB_BASE_URL}/open/?to=other/nexus/skyrimspecialedition/12604`),
    ).toBeNull();
    expect(parseModDeepLink(`${EMPEROR_WEB_BASE_URL}/open/`)).toBeNull();
  });
});

describe("catalogHitFromModDeepLink + managedGameForModDeepLink", () => {
  it("builds a hit and finds the managed game", () => {
    const payload: EmperorOpenMod = {
      source: "nexus",
      domain: "skyrimspecialedition",
      mod_id: 12604,
    };
    const catalog = catalogHitFromModDeepLink(payload);
    expect(catalog).toMatchObject({
      source: "nexus",
      id: "nexus:skyrimspecialedition:12604",
      mod_id: 12604,
      domain_name: "skyrimspecialedition",
    });
    expect(managedGameForModDeepLink([game()], payload)?.id).toBe("g");
    expect(
      managedGameForModDeepLink(
        [game({ nexus_domain: "fallout4" })],
        payload,
      ),
    ).toBeNull();
  });
});

describe("friend deep links", () => {
  const code = "emperor-friend1:abc_123-XYZ";

  it("builds emperor:// and clickable HTTPS friend invite links", () => {
    expect(friendDeepLinkFromCode(code)).toBe(`emperor://friend/${code}`);
    expect(friendShareUrlFromCode(code)).toBe(
      `${EMPEROR_WEB_BASE_URL}/open/?to=friend/${code}`,
    );
    expect(friendDeepLinkFromCode("not-a-code")).toBeNull();
    expect(friendShareUrlFromCode("emperor-friend1:")).toBeNull();
    expect(friendShareUrlFromCode("emperor-friend1:bad!char")).toBeNull();
  });

  it("parses and extracts friend join codes from deep links and share URLs", () => {
    expect(parseFriendDeepLink(`emperor://friend/${code}`)).toBe(code);
    expect(parseFriendDeepLink(`emperor:///friend/${code}`)).toBe(code);
    expect(
      parseFriendDeepLink(`${EMPEROR_WEB_BASE_URL}/open/?to=friend/${code}`),
    ).toBe(code);
    expect(
      parseFriendDeepLink(`${EMPEROR_WEB_BASE_URL}/open/#friend/${code}`),
    ).toBe(code);
    expect(extractFriendJoinCode(code)).toBe(code);
    expect(
      extractFriendJoinCode(`${EMPEROR_WEB_BASE_URL}/open/?to=friend/${code}`),
    ).toBe(code);

    expect(
      parseFriendDeepLink(`https://example.com/open/?to=friend/${code}`),
    ).toBeNull();
    expect(parseFriendDeepLink("emperor://mod/nexus/skyrim/1")).toBeNull();
    expect(parseFriendDeepLink("emperor://friend/not-a-code")).toBeNull();
  });
});
