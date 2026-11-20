import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  GameVersionReport,
  ManagedGame,
  MissingRequirements,
  Settings,
  StagedMod,
} from "./types";

type Gate<T> = {
  promise: Promise<T>;
  resolve: (value: T) => void;
};

type RequirementCall = Gate<MissingRequirements> & { gameId: string };
type VersionCall = Gate<GameVersionReport> & { gameId: string };

const requirementCalls: RequirementCall[] = [];
const versionCalls: VersionCall[] = [];

function gate<T>(): Gate<T> {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

const games: ManagedGame[] = [
  {
    id: "a",
    title: "Game A",
    nexus_domain: "gamea",
    install_path: "/games/a",
    launcher: "steam",
    plugin_id: "gamea",
  },
  {
    id: "b",
    title: "Game B",
    nexus_domain: "gameb",
    install_path: "/games/b",
    launcher: "steam",
    plugin_id: "gameb",
  },
];

const modsByGame: Record<string, StagedMod[]> = {
  a: [stagedMod("a", "Mod A", 11)],
  b: [stagedMod("b", "Mod B", 22)],
};

const settings: Settings = {
  adult_content: false,
  game_version_warnings: true,
  autoclick_free_download: false,
  last_active_game_id: "a",
  theme: "dark",
  theme_style: "app",
  has_api_key: false,
  initial_setup_complete: true,
  nexus_website_signed_in: false,
  user: null,
  config_dir: "/cfg",
  data_dir: "/data",
  cache_dir: "/cache",
  platform_linux: false,
  platform_windows: true,
};

function stagedMod(gameId: string, name: string, modId: number): StagedMod {
  return {
    id: `${gameId}-mod`,
    name,
    source: "nexus",
    nexus_mod_id: modId,
    nexus_file_id: 1,
    version: "1",
    domain: gameId === "a" ? "gamea" : "gameb",
    staging_path: `/staging/${gameId}`,
    enabled: true,
    order: 0,
    picture_url: "",
    supported_game_versions: [],
    file_name: `${name}.zip`,
  };
}

function missing(name: string): MissingRequirements {
  return {
    nexus: [
      {
        domain: "gamea",
        mod_id: 42,
        file_id: 7,
        name,
        version: "1",
        notes: null,
        url: "https://example.test/mod",
        required_by: [],
        required_by_names: ["Mod A"],
      },
    ],
    external: [],
    dlc: [],
    error: null,
  };
}

function versionReport(current: string): GameVersionReport {
  return {
    installed: current,
    override_version: null,
    current,
    source: "detected",
    mods: [],
  };
}

vi.mock("@tauri-apps/api/core", () => {
  Object.defineProperty(window, "matchMedia", {
    writable: true,
    value: (query: string) => ({
      matches: query.includes("prefers-reduced-motion"),
      media: query,
      onchange: null,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      dispatchEvent: () => false,
    }),
  });
  Element.prototype.scrollTo = () => {};
  (
    globalThis as typeof globalThis & { IS_REACT_ACT_ENVIRONMENT?: boolean }
  ).IS_REACT_ACT_ENVIRONMENT = true;
  return {
  convertFileSrc: (path: string) => path,
  invoke: (cmd: string, args?: Record<string, unknown>) => {
    const gameId = typeof args?.gameId === "string" ? args.gameId : "";
    switch (cmd) {
      case "get_settings":
        return Promise.resolve(settings);
      case "list_managed":
        return Promise.resolve(games);
      case "scan_games":
      case "get_game_health":
      case "resolve_catalog_covers":
      case "list_installed_collections":
      case "list_mods_with_options":
      case "list_mods_needing_fomod":
      case "check_staged_mod_updates":
        return Promise.resolve([]);
      case "scan_mod_orphans":
        return Promise.resolve({
          legacy_game_ids: [],
          missing_staging: [],
          untracked_staging: [],
          deploy_without_loadorder: [],
        });
      case "check_app_update":
        return Promise.resolve({
          current_version: "0.7.5",
          latest_version: null,
          update_available: false,
          asset_name: null,
          release_url: null,
          install_via: null,
        });
      case "take_pending_emperor_mod_link":
      case "set_active_game":
      case "wine_override_advice":
        return Promise.resolve(null);
      case "get_game":
        return Promise.resolve({
          id: 1,
          name: String(args?.domain ?? ""),
          domain_name: String(args?.domain ?? ""),
          genre: null,
          forum_url: null,
          nexusmods_url: null,
          mods: null,
          file_count: null,
          downloads: null,
          categories: [],
        });
      case "list_mods":
        return Promise.resolve(modsByGame[gameId] ?? []);
      case "missing_nexus_requirements": {
        const pending = gate<MissingRequirements>();
        requirementCalls.push({ gameId, ...pending });
        return pending.promise;
      }
      case "check_mod_game_versions": {
        const pending = gate<GameVersionReport>();
        versionCalls.push({ gameId, ...pending });
        return pending.promise;
      }
      default:
        return Promise.reject(new Error(`unexpected invoke ${cmd}`));
    }
  },
  };
});

vi.mock("@tauri-apps/api/event", () => ({
  listen: () => Promise.resolve(() => {}),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: () => Promise.resolve(),
  revealItemInDir: () => Promise.resolve(),
}));

import App from "./App";

function latestRequirement(gameId: string): RequirementCall {
  const call = [...requirementCalls].reverse().find((item) => item.gameId === gameId);
  if (!call) throw new Error(`no requirement check for ${gameId}`);
  return call;
}

function latestVersion(gameId: string): VersionCall {
  const call = [...versionCalls].reverse().find((item) => item.gameId === gameId);
  if (!call) throw new Error(`no version check for ${gameId}`);
  return call;
}

function clickCard(title: string) {
  const card = [...document.querySelectorAll(".media-card")].find(
    (el) => el.querySelector(".media-card-title")?.textContent === title,
  );
  if (!card) throw new Error(`missing card ${title}`);
  card.dispatchEvent(new MouseEvent("click", { bubbles: true }));
}

function clickButton(label: string) {
  const button = [...document.querySelectorAll("button")].find(
    (el) => el.textContent?.trim() === label,
  );
  if (!button) throw new Error(`missing button ${label}`);
  button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
}

function openVersionField() {
  const button = document.querySelector('button[aria-label="Filters"]');
  if (!button) throw new Error("missing filters button");
  if (button.getAttribute("aria-expanded") !== "true") {
    button.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  }
}

function versionInput(): HTMLInputElement {
  const input = document.querySelector(
    'input[aria-label="Game version"]',
  ) as HTMLInputElement | null;
  if (!input) throw new Error("missing game version field");
  return input;
}

async function waitFor(predicate: () => boolean, label: string) {
  const started = Date.now();
  while (!predicate()) {
    if (Date.now() - started > 4000) {
      throw new Error(`${label}\n${document.body.textContent ?? ""}`);
    }
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 15));
    });
  }
}

describe("stale requirements on game switch", () => {
  let root: Root;
  let host: HTMLDivElement;

  beforeEach(() => {
    requirementCalls.length = 0;
    versionCalls.length = 0;
    host = document.createElement("div");
    document.body.appendChild(host);
    root = createRoot(host);
  });

  afterEach(() => {
    act(() => {
      root.unmount();
    });
    host.remove();
  });

  it("hides the previous game's requirements until the new check returns", async () => {
    await act(async () => {
      root.render(<App />);
    });
    await waitFor(
      () => document.body.textContent?.includes("Game A") === true,
      "library did not list games",
    );

    await act(async () => {
      clickCard("Game A");
    });
    await waitFor(
      () => requirementCalls.some((call) => call.gameId === "a"),
      "game A requirements were not requested",
    );
    await act(async () => {
      latestRequirement("a").resolve(missing("Old Game Requirement"));
      latestVersion("a").resolve(versionReport("1.6.1170"));
    });
    await waitFor(
      () => document.body.textContent?.includes("Old Game Requirement") === true,
      "game A requirements did not appear",
    );
    await act(async () => {
      openVersionField();
    });
    await waitFor(
      () => versionInput().value === "1.6.1170",
      "game A version did not appear",
    );

    const requirementsBeforeSwitch = requirementCalls.length;
    await act(async () => {
      clickButton("Home");
    });
    await act(async () => {
      clickCard("Game B");
    });
    await waitFor(
      () => document.body.textContent?.includes("Mod B") === true,
      "game B mods did not load",
    );
    await act(async () => {
      openVersionField();
    });

    expect(document.body.textContent).not.toContain("Old Game Requirement");
    expect(versionInput().value).toBe("");
    expect(requirementCalls.length).toBeGreaterThan(requirementsBeforeSwitch);

    await act(async () => {
      latestRequirement("b").resolve(missing("Game B Requirement"));
      latestVersion("b").resolve(versionReport("9.9.9"));
    });
    await waitFor(
      () =>
        document.body.textContent?.includes("Game B Requirement") === true &&
        versionInput().value === "9.9.9",
      "game B requirements did not appear",
    );
    expect(document.body.textContent).not.toContain("Old Game Requirement");
    expect(document.body.textContent).not.toContain("1.6.1170");
  });

  it("keeps the current game's requirements visible while that game refreshes", async () => {
    await act(async () => {
      root.render(<App />);
    });
    await waitFor(
      () => document.body.textContent?.includes("Game A") === true,
      "library did not list games",
    );
    await act(async () => {
      clickCard("Game A");
    });
    await waitFor(
      () => requirementCalls.some((call) => call.gameId === "a"),
      "game A requirements were not requested",
    );
    await act(async () => {
      latestRequirement("a").resolve(missing("Old Game Requirement"));
      latestVersion("a").resolve(versionReport("1.6.1170"));
    });
    await waitFor(
      () => document.body.textContent?.includes("Old Game Requirement") === true,
      "game A requirements did not appear",
    );

    const callsBeforeRefresh = requirementCalls.length;
    await act(async () => {
      clickButton("Info");
    });
    await act(async () => {
      clickButton("Mods");
    });
    await waitFor(
      () => requirementCalls.length > callsBeforeRefresh,
      "same-game refresh did not start",
    );
    await act(async () => {
      openVersionField();
    });

    expect(document.body.textContent).toContain("Old Game Requirement");
    expect(versionInput().value).toBe("1.6.1170");

    await act(async () => {
      latestRequirement("a").resolve(missing("Refreshed Requirement"));
      latestVersion("a").resolve(versionReport("2.0.0"));
    });
    await waitFor(
      () =>
        document.body.textContent?.includes("Refreshed Requirement") === true &&
        versionInput().value === "2.0.0",
      "same-game refresh did not update",
    );
    expect(document.body.textContent).not.toContain("Old Game Requirement");
  });

  it("drops an in-flight result from the game that was just left", async () => {
    await act(async () => {
      root.render(<App />);
    });
    await waitFor(
      () => document.body.textContent?.includes("Game A") === true,
      "library did not list games",
    );
    await act(async () => {
      clickCard("Game A");
    });
    await waitFor(
      () => requirementCalls.some((call) => call.gameId === "a"),
      "game A requirements were not requested",
    );
    const staleRequirement = latestRequirement("a");
    const staleVersion = latestVersion("a");

    await act(async () => {
      clickButton("Home");
    });
    await act(async () => {
      clickCard("Game B");
    });
    await waitFor(
      () => requirementCalls.some((call) => call.gameId === "b"),
      "game B requirements were not requested",
    );

    await act(async () => {
      staleRequirement.resolve(missing("Old Game Requirement"));
      staleVersion.resolve(versionReport("1.6.1170"));
    });
    await waitFor(
      () => document.body.textContent?.includes("Mod B") === true,
      "game B mods did not load",
    );
    await act(async () => {
      openVersionField();
    });
    await act(async () => {
      await Promise.resolve();
    });

    expect(document.body.textContent).not.toContain("Old Game Requirement");
    expect(versionInput().value).not.toBe("1.6.1170");

    await act(async () => {
      latestRequirement("b").resolve(missing("Game B Requirement"));
      latestVersion("b").resolve(versionReport("9.9.9"));
    });
    await waitFor(
      () => document.body.textContent?.includes("Game B Requirement") === true,
      "game B requirements did not appear",
    );
    expect(document.body.textContent).not.toContain("Old Game Requirement");
    expect(versionInput().value).toBe("9.9.9");
  });
});
