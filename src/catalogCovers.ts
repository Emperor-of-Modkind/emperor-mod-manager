import type { SavedCollectionGameGroup } from "./savedCollectionGroups";
import type {
  CatalogCoverRequest,
  CatalogSuggestion,
  DetectedGame,
  GamePluginInfo,
  ManagedGame,
} from "./types";

export const COLLECTION_COVER_PREFIX = "collection:";

export function isDeferredIconPath(path: string): boolean {
  const parts = path.replace(/\\/g, "/").split("/");
  const file = parts[parts.length - 1]?.toLowerCase() ?? "";
  const parent = parts[parts.length - 2]?.toLowerCase() ?? "";
  return parent === "icons" || file.includes("_icon.");
}

export function needsCatalogPoster(
  coverPath?: string | null,
  coverUrl?: string | null,
  iconPath?: string | null,
): boolean {
  if (coverUrl) return false;
  if (!coverPath) return true;
  if (iconPath && coverPath === iconPath) return true;
  return isDeferredIconPath(coverPath);
}

function firstDomain(
  ...values: (string | null | undefined)[]
): string | null {
  for (const value of values) {
    if (value && value.trim()) return value;
  }
  return null;
}

function firstModioId(
  ...values: (number | null | undefined)[]
): number | null {
  for (const value of values) {
    if (value != null && value > 0) return value;
  }
  return null;
}

/** Re-apply resolved catalog posters onto detected games that still lack a real local cover. */
export function applyCatalogCovers(
  games: DetectedGame[],
  resolvedUrls: ReadonlyMap<string, string>,
): DetectedGame[] {
  if (resolvedUrls.size === 0) return games;
  let changed = false;
  const next = games.map((game) => {
    const url = resolvedUrls.get(game.id);
    if (!url) return game;
    if (!needsCatalogPoster(game.cover_path, game.cover_url, game.icon_path)) {
      return game;
    }
    const dropLocal =
      !game.cover_path ||
      game.cover_path === game.icon_path ||
      isDeferredIconPath(game.cover_path);
    const cover_path = dropLocal ? null : game.cover_path ?? null;
    if (game.cover_url === url && (game.cover_path ?? null) === cover_path) {
      return game;
    }
    changed = true;
    return {
      ...game,
      cover_url: url,
      cover_path,
    };
  });
  return changed ? next : games;
}

/** Games that still need a poster and have a stored or suggested catalog id. */
export function catalogCoverRequests(
  detected: DetectedGame[],
  managed: ManagedGame[],
  suggestions?: Record<string, CatalogSuggestion | null | undefined>,
): CatalogCoverRequest[] {
  const requests: CatalogCoverRequest[] = [];
  const seen = new Set<string>();
  for (const game of detected) {
    const managedMatch = managed.find((item) => item.id === game.id);
    const suggestion = suggestions?.[game.id];
    const domain = firstDomain(
      game.nexus_domain,
      managedMatch?.nexus_domain,
      suggestion?.nexus_domain,
    );
    const modio = firstModioId(
      managedMatch?.modio_game_id,
      suggestion?.modio_game_id,
    );
    const coverPath =
      game.cover_path ||
      (managedMatch && !game.cover_url ? managedMatch.cover_path : null);
    const coverUrl = game.cover_url || managedMatch?.cover_url || null;
    if (!needsCatalogPoster(coverPath, coverUrl, game.icon_path)) continue;
    if (!domain && modio == null) continue;
    seen.add(game.id);
    requests.push({
      id: game.id,
      nexus_domain: domain,
      modio_game_id: modio,
      icon_path: game.icon_path ?? null,
    });
  }
  for (const game of managed) {
    if (seen.has(game.id)) continue;
    const suggestion = suggestions?.[game.id];
    const domain = firstDomain(game.nexus_domain, suggestion?.nexus_domain);
    const modio = firstModioId(game.modio_game_id, suggestion?.modio_game_id);
    if (!needsCatalogPoster(game.cover_path, game.cover_url, null)) continue;
    if (!domain && modio == null) continue;
    requests.push({
      id: game.id,
      nexus_domain: domain,
      modio_game_id: modio,
      icon_path: null,
    });
  }
  return requests;
}

/** Collection game groups that still need a poster and have a stored or plugin catalog id. */
export function collectionCatalogCoverRequests(
  groups: SavedCollectionGameGroup[],
  plugins: GamePluginInfo[],
  detected: DetectedGame[] = [],
): CatalogCoverRequest[] {
  const iconPaths = new Set(
    detected.map((d) => d.icon_path).filter((p): p is string => Boolean(p)),
  );
  const requests: CatalogCoverRequest[] = [];
  for (const group of groups) {
    const cover = group.coverPath;
    if (cover) {
      if (/^https?:\/\//i.test(cover)) continue;
      if (!isDeferredIconPath(cover) && !iconPaths.has(cover)) continue;
    }
    const pluginDomain = group.collections
      .map((entry) =>
        plugins.find((p) => p.id && p.id === entry.game.plugin_id)?.nexus_domain,
      )
      .find((d) => Boolean(d && d.trim()));
    const domain = firstDomain(
      ...group.collections.map((c) => c.game.nexus_domain),
      pluginDomain,
    );
    const modio = firstModioId(
      ...group.collections.map((c) => c.game.modio_game_id),
    );
    if (!domain && modio == null) continue;
    requests.push({
      id: `${COLLECTION_COVER_PREFIX}${group.key}`,
      nexus_domain: domain,
      modio_game_id: modio,
      icon_path: null,
    });
  }
  return requests;
}
