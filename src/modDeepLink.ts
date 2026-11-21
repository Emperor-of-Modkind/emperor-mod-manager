import type { CatalogHit, CatalogSource, ManagedGame } from "./types";

/** Base URL of the Emperor Mod Manager GitHub Pages site hosting `/open/`. */
export const EMPEROR_WEB_BASE_URL = "https://emperor-of-modkind.github.io";

export const FRIEND_JOIN_CODE_PREFIX = "emperor-friend1:";
const FRIEND_JOIN_CODE_RE = /^emperor-friend1:[A-Za-z0-9_-]+$/;

/** Payload emitted by the Rust `emperor-open-mod` event / take command. */
export type EmperorOpenMod =
  | { source: "nexus"; domain: string; mod_id: number }
  | {
      source: "thunderstore";
      community: string;
      namespace: string;
      package_name: string;
    }
  | { source: "modio"; game_id: number; mod_id: number };

function blankHit(source: CatalogSource, id: string): CatalogHit {
  return {
    source,
    id,
    name: id,
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
  };
}

/** Build an `emperor://mod/...` custom-protocol URI from a catalog hit. */
export function modDeepLinkFromHit(hit: CatalogHit): string | null {
  if (hit.source === "nexus") {
    const domain = hit.domain_name?.trim();
    const modId = hit.mod_id;
    if (!domain || modId == null || modId <= 0) return null;
    return `emperor://mod/nexus/${encodeURIComponent(domain)}/${modId}`;
  }
  if (hit.source === "thunderstore") {
    const community = hit.community?.trim();
    const namespace = hit.namespace?.trim();
    const packageName = hit.package_name?.trim();
    if (!community || !namespace || !packageName) return null;
    return `emperor://mod/thunderstore/${encodeURIComponent(community)}/${encodeURIComponent(namespace)}/${encodeURIComponent(packageName)}`;
  }
  if (hit.source === "modio") {
    const gameId = hit.modio_game_id;
    const modId = hit.modio_mod_id;
    if (gameId == null || gameId <= 0 || modId == null || modId <= 0) return null;
    return `emperor://mod/modio/${gameId}/${modId}`;
  }
  return null;
}

/**
 * Build a clickable HTTPS share link (via the GitHub Pages `/open/` static
 * redirect) that launches the corresponding `emperor://mod/...` deep link.
 */
export function modShareUrlFromHit(hit: CatalogHit): string | null {
  const deepLink = modDeepLinkFromHit(hit);
  if (!deepLink) return null;
  const route = deepLink.slice("emperor://".length);
  const base = EMPEROR_WEB_BASE_URL.replace(/\/+$/, "");
  return `${base}/open/?to=${route}`;
}

/** Minimal CatalogHit from a parsed emperor deep-link payload. */
export function catalogHitFromModDeepLink(payload: EmperorOpenMod): CatalogHit {
  if (payload.source === "nexus") {
    return {
      ...blankHit("nexus", `nexus:${payload.domain}:${payload.mod_id}`),
      name: `Mod ${payload.mod_id}`,
      mod_id: payload.mod_id,
      domain_name: payload.domain,
    };
  }
  if (payload.source === "thunderstore") {
    return {
      ...blankHit(
        "thunderstore",
        `thunderstore:${payload.community}:${payload.namespace}:${payload.package_name}`,
      ),
      name: `${payload.namespace}-${payload.package_name}`,
      community: payload.community,
      namespace: payload.namespace,
      package_name: payload.package_name,
      full_name: `${payload.namespace}-${payload.package_name}`,
    };
  }
  return {
    ...blankHit("modio", `modio:${payload.game_id}:${payload.mod_id}`),
    name: `Mod ${payload.mod_id}`,
    modio_game_id: payload.game_id,
    modio_mod_id: payload.mod_id,
  };
}

/** Find a managed game that matches the deep-link catalog identity. */
export function managedGameForModDeepLink(
  managed: ManagedGame[],
  payload: EmperorOpenMod,
): ManagedGame | null {
  if (payload.source === "nexus") {
    const domain = payload.domain.toLowerCase();
    return (
      managed.find((g) => g.nexus_domain?.toLowerCase() === domain) ?? null
    );
  }
  if (payload.source === "thunderstore") {
    const community = payload.community.toLowerCase();
    return (
      managed.find(
        (g) => g.thunderstore_community?.toLowerCase() === community,
      ) ?? null
    );
  }
  return (
    managed.find((g) => g.modio_game_id != null && g.modio_game_id === payload.game_id) ??
    null
  );
}

function safeDecodeSegment(raw: string): string | null {
  try {
    return decodeURIComponent(raw);
  } catch {
    return null;
  }
}

function parseModSegments(parts: string[]): EmperorOpenMod | null {
  const kind = parts[0]?.toLowerCase();
  if (kind === "nexus") {
    if (parts.length !== 3) return null;
    const domain = safeDecodeSegment(parts[1] ?? "");
    const modId = Number(parts[2]);
    if (!domain || !Number.isInteger(modId) || modId <= 0) return null;
    return { source: "nexus", domain, mod_id: modId };
  }
  if (kind === "thunderstore") {
    if (parts.length !== 4) return null;
    const community = safeDecodeSegment(parts[1] ?? "");
    const namespace = safeDecodeSegment(parts[2] ?? "");
    const packageName = safeDecodeSegment(parts[3] ?? "");
    if (!community || !namespace || !packageName) return null;
    return {
      source: "thunderstore",
      community,
      namespace,
      package_name: packageName,
    };
  }
  if (kind === "modio") {
    if (parts.length !== 3) return null;
    const gameId = Number(parts[1]);
    const modId = Number(parts[2]);
    if (
      !Number.isInteger(gameId) ||
      gameId <= 0 ||
      !Number.isInteger(modId) ||
      modId <= 0
    ) {
      return null;
    }
    return { source: "modio", game_id: gameId, mod_id: modId };
  }
  return null;
}

function rawQueryParam(search: string, name: string): string | null {
  const qs = search.startsWith("?") ? search.slice(1) : search;
  for (const pair of qs.split("&")) {
    if (!pair) continue;
    const eq = pair.indexOf("=");
    const key = eq === -1 ? pair : pair.slice(0, eq);
    if (key === name) {
      return eq === -1 ? "" : pair.slice(eq + 1);
    }
  }
  return null;
}

/**
 * Parse an `emperor://mod/...` URI or an Emperor HTTPS redirect URL
 * (`https://emperor-of-modkind.github.io/open/?to=mod/...`).
 */
export function parseModDeepLink(raw: string): EmperorOpenMod | null {
  const trimmed = raw.trim();
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    return null;
  }

  if (url.protocol === "emperor:") {
    const host = url.hostname;
    let parts = url.pathname.split("/").filter(Boolean);
    if (host.toLowerCase() === "mod") {
      // emperor://mod/nexus/... → hostname=mod, pathname=/nexus/...
    } else if (!host) {
      if (parts[0]?.toLowerCase() === "mod") {
        parts = parts.slice(1);
      } else {
        return null;
      }
    } else {
      return null;
    }
    return parseModSegments(parts);
  }

  if (url.protocol === "https:" || url.protocol === "http:") {
    let base: URL;
    try {
      base = new URL(EMPEROR_WEB_BASE_URL);
    } catch {
      return null;
    }
    if (url.origin !== base.origin) return null;
    const basePath = base.pathname.replace(/\/+$/, "");
    const normalizedPath = url.pathname.replace(/\/+$/, "");
    if (
      normalizedPath !== `${basePath}/open` &&
      normalizedPath !== `${basePath}/open/index.html`
    ) {
      return null;
    }
    const route =
      rawQueryParam(url.search, "to")?.trim() ||
      url.hash.replace(/^#/, "").trim();
    if (!route) return null;
    const parts = route.split("/").filter(Boolean);
    if (parts[0]?.toLowerCase() !== "mod") return null;
    return parseModSegments(parts.slice(1));
  }

  return null;
}

function validateFriendJoinCode(raw: string): string | null {
  const trimmed = raw.trim();
  return FRIEND_JOIN_CODE_RE.test(trimmed) ? trimmed : null;
}

/** Build an `emperor://friend/{joinCode}` custom-protocol URI. */
export function friendDeepLinkFromCode(joinCode: string): string | null {
  const valid = validateFriendJoinCode(joinCode);
  if (!valid) return null;
  return `emperor://friend/${valid}`;
}

/** Build a clickable HTTPS share link for a friend join code. */
export function friendShareUrlFromCode(joinCode: string): string | null {
  const valid = validateFriendJoinCode(joinCode);
  if (!valid) return null;
  const base = EMPEROR_WEB_BASE_URL.replace(/\/+$/, "");
  return `${base}/open/?to=friend/${valid}`;
}

/**
 * Parse an `emperor://friend/{joinCode}` URI or an Emperor HTTPS redirect URL
 * (`https://emperor-of-modkind.github.io/open/?to=friend/{joinCode}`).
 */
export function parseFriendDeepLink(raw: string): string | null {
  const trimmed = raw.trim();
  let url: URL;
  try {
    url = new URL(trimmed);
  } catch {
    return null;
  }

  if (url.protocol === "emperor:") {
    const host = url.hostname;
    let parts = url.pathname.split("/").filter(Boolean);
    if (host.toLowerCase() === "friend") {
      // emperor://friend/emperor-friend1:... → hostname=friend, pathname=/emperor-friend1:...
    } else if (!host) {
      if (parts[0]?.toLowerCase() === "friend") {
        parts = parts.slice(1);
      } else {
        return null;
      }
    } else {
      return null;
    }
    if (parts.length !== 1) return null;
    const decoded = safeDecodeSegment(parts[0] ?? "");
    return decoded ? validateFriendJoinCode(decoded) : null;
  }

  if (url.protocol === "https:" || url.protocol === "http:") {
    let base: URL;
    try {
      base = new URL(EMPEROR_WEB_BASE_URL);
    } catch {
      return null;
    }
    if (url.origin !== base.origin) return null;
    const basePath = base.pathname.replace(/\/+$/, "");
    const normalizedPath = url.pathname.replace(/\/+$/, "");
    if (
      normalizedPath !== `${basePath}/open` &&
      normalizedPath !== `${basePath}/open/index.html`
    ) {
      return null;
    }
    const route =
      rawQueryParam(url.search, "to")?.trim() ||
      url.hash.replace(/^#/, "").trim();
    if (!route) return null;
    const parts = route.split("/").filter(Boolean);
    if (parts.length !== 2 || parts[0]?.toLowerCase() !== "friend") return null;
    const decoded = safeDecodeSegment(parts[1] ?? "");
    return decoded ? validateFriendJoinCode(decoded) : null;
  }

  return null;
}

/** Extract a raw `emperor-friend1:...` join code from a raw code or invite link. */
export function extractFriendJoinCode(raw: string): string | null {
  return validateFriendJoinCode(raw) ?? parseFriendDeepLink(raw);
}
