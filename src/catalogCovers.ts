import type {
  CatalogCoverRequest,
  CatalogSuggestion,
  DetectedGame,
  ManagedGame,
} from "./types";

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
