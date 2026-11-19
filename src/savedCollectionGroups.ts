import type {
  DetectedGame,
  GamePluginInfo,
  ManagedGame,
  SavedCollectionEntry,
  ShareGameHint,
} from "./types";

export type SavedCollectionGameGroup = {
  key: string;
  title: string;
  coverPath: string | null;
  managed: ManagedGame | null;
  collections: SavedCollectionEntry[];
};

const GENERIC_ENGINE_PLUGINS = new Set(["bepinex", "unreal"]);

function isGenericEnginePlugin(pluginId: string | null | undefined): boolean {
  const id = pluginId?.trim().toLowerCase();
  return Boolean(id && GENERIC_ENGINE_PLUGINS.has(id));
}

function titlesMatch(
  left: string | null | undefined,
  right: string | null | undefined,
): boolean {
  const a = left?.trim().toLowerCase();
  const b = right?.trim().toLowerCase();
  return Boolean(a && b && a === b);
}

function hintHasCatalog(hint: ShareGameHint): boolean {
  return Boolean(
    hint.nexus_domain?.trim() ||
      hint.thunderstore_community?.trim() ||
      (hint.modio_game_id != null && hint.modio_game_id > 0),
  );
}

function genericEngineKey(hint: ShareGameHint): string | null {
  if (!isGenericEnginePlugin(hint.plugin_id)) return null;
  const title = hint.title?.trim().toLowerCase();
  const project = hint.project_name?.trim().toLowerCase();
  const plugin = hint.plugin_id!.trim().toLowerCase();
  if (plugin === "unreal" && title && project) {
    return `title:${title}:project:${project}`;
  }
  if (title) return `title:${title}`;
  return null;
}

export function savedCollectionGameKey(entry: SavedCollectionEntry): string {
  const h = entry.game;
  if (h.nexus_domain?.trim()) {
    return `nexus:${h.nexus_domain.trim().toLowerCase()}`;
  }
  if (h.thunderstore_community?.trim()) {
    return `ts:${h.thunderstore_community.trim().toLowerCase()}`;
  }
  if (h.modio_game_id != null && h.modio_game_id > 0) {
    return `modio:${h.modio_game_id}`;
  }
  if (h.plugin_id?.trim() && !isGenericEnginePlugin(h.plugin_id)) {
    return `plugin:${h.plugin_id.trim()}`;
  }
  const generic = genericEngineKey(h);
  if (generic) return generic;
  if (h.plugin_id?.trim()) {
    return `plugin:${h.plugin_id.trim()}`;
  }
  if (entry.source_game_id?.trim()) {
    return `source:${entry.source_game_id.trim()}`;
  }
  if (h.id?.trim()) {
    return `id:${h.id.trim()}`;
  }
  return "unknown";
}

export function fallbackGameTitle(hint: ShareGameHint, sourceGameId?: string | null): string {
  if (hint.title?.trim()) return hint.title.trim();
  if (hint.nexus_domain?.trim()) return hint.nexus_domain.trim();
  if (hint.thunderstore_community?.trim()) return hint.thunderstore_community.trim();
  if (hint.modio_game_id != null && hint.modio_game_id > 0) {
    return `mod.io ${hint.modio_game_id}`;
  }
  if (hint.plugin_id?.trim()) return hint.plugin_id.trim();
  if (sourceGameId?.trim()) return sourceGameId.trim();
  if (hint.id?.trim()) return hint.id.trim();
  return "Unknown game";
}

export function savedCollectionMatchesGame(
  entry: SavedCollectionEntry,
  game: ManagedGame,
): boolean {
  const h = entry.game;
  if (
    h.nexus_domain &&
    game.nexus_domain &&
    h.nexus_domain.toLowerCase() === game.nexus_domain.toLowerCase()
  ) {
    return true;
  }
  if (
    h.thunderstore_community &&
    game.thunderstore_community &&
    h.thunderstore_community.toLowerCase() ===
      game.thunderstore_community.toLowerCase()
  ) {
    return true;
  }
  if (
    h.modio_game_id != null &&
    h.modio_game_id > 0 &&
    game.modio_game_id === h.modio_game_id
  ) {
    return true;
  }
  if (entry.source_game_id && entry.source_game_id === game.id) return true;
  if (h.id && h.id === game.id) return true;
  if (!hintHasCatalog(h) && titlesMatch(h.title, game.title)) {
    if (
      h.plugin_id?.trim().toLowerCase() === "unreal" &&
      h.project_name?.trim()
    ) {
      return titlesMatch(h.project_name, game.project_name);
    }
    return true;
  }
  return false;
}

function detectedMatchesHint(game: DetectedGame, hint: ShareGameHint): boolean {
  if (
    hint.nexus_domain &&
    game.nexus_domain &&
    hint.nexus_domain.toLowerCase() === game.nexus_domain.toLowerCase()
  ) {
    return true;
  }
  if (hint.plugin_id && game.plugin_id && hint.plugin_id === game.plugin_id) {
    if (isGenericEnginePlugin(hint.plugin_id) && hint.title?.trim()) {
      return titlesMatch(hint.title, game.title);
    }
    return true;
  }
  return false;
}

function pluginMatchesHint(
  plugin: GamePluginInfo,
  hint: ShareGameHint,
): boolean {
  if (hint.plugin_id && hint.plugin_id === plugin.id) {
    if (isGenericEnginePlugin(hint.plugin_id) && hint.title?.trim()) return false;
    return true;
  }
  if (
    hint.nexus_domain &&
    plugin.nexus_domain &&
    hint.nexus_domain.toLowerCase() === plugin.nexus_domain.toLowerCase()
  ) {
    return true;
  }
  return false;
}

function resolveGroupMeta(
  representative: SavedCollectionEntry,
  managed: ManagedGame[],
  detected: DetectedGame[],
  plugins: GamePluginInfo[],
): Pick<SavedCollectionGameGroup, "title" | "coverPath" | "managed"> {
  const managedMatch =
    managed.find((g) => savedCollectionMatchesGame(representative, g)) ?? null;
  if (managedMatch) {
    return {
      title: managedMatch.title,
      coverPath: managedMatch.cover_path ?? null,
      managed: managedMatch,
    };
  }

  const detectedMatch =
    detected.find((g) => detectedMatchesHint(g, representative.game)) ?? null;
  if (detectedMatch) {
    return {
      title: detectedMatch.title,
      coverPath: detectedMatch.cover_path,
      managed: null,
    };
  }

  const pluginMatch =
    plugins.find((p) => pluginMatchesHint(p, representative.game)) ?? null;
  if (pluginMatch) {
    return {
      title: pluginMatch.display_name,
      coverPath: null,
      managed: null,
    };
  }

  return {
    title: fallbackGameTitle(representative.game, representative.source_game_id),
    coverPath: null,
    managed: null,
  };
}

export function groupSavedCollectionsByGame(
  collections: SavedCollectionEntry[],
  managed: ManagedGame[],
  detected: DetectedGame[],
  plugins: GamePluginInfo[],
): SavedCollectionGameGroup[] {
  const byKey = new Map<string, SavedCollectionEntry[]>();
  for (const entry of collections) {
    const key = savedCollectionGameKey(entry);
    const list = byKey.get(key);
    if (list) list.push(entry);
    else byKey.set(key, [entry]);
  }

  const groups: SavedCollectionGameGroup[] = [];
  for (const [key, entries] of byKey) {
    const sorted = [...entries].sort(
      (a, b) =>
        new Date(b.created_at).getTime() - new Date(a.created_at).getTime(),
    );
    const meta = resolveGroupMeta(sorted[0]!, managed, detected, plugins);
    groups.push({
      key,
      title: meta.title,
      coverPath: meta.coverPath,
      managed: meta.managed,
      collections: sorted,
    });
  }

  groups.sort((a, b) => a.title.localeCompare(b.title));
  return groups;
}

export function formatCollectionGameVersions(
  versions: string[] | null | undefined,
): string | null {
  if (!versions?.length) return null;
  return versions.join(", ");
}
