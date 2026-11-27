import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowDown,
  faArrowDownShortWide,
  faArrowLeft,
  faArrowUp,
  faCloudArrowDown,
  faCompass,
  faDownload,
  faGamepad,
  faGear,
  faLayerGroup,
  faPause,
  faPlay,
  faRotateRight,
  faSliders,
  faStop,
  faTrashCan,
  faUsers,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import { api } from "./api";
import { FomodConfigureBanner } from "./FomodDialog";
import { FriendAvatar } from "./FriendAvatar";
import { MediaCard } from "./MediaCard";
import { MobileBrowseView } from "./MobileBrowseView";
import { MobileCollectionsView } from "./MobileCollectionsView";
import { MobileFomodModal } from "./MobileFomodModal";
import { MobileFriendsView } from "./MobileFriendsView";
import { MobilePairingView } from "./MobilePairingView";
import { MobileStatusBar } from "./MobileStatusBar";
import { SettingsToggle } from "./SettingsSection";
import { StatusBanner } from "./StatusBanner";
import { useFoldableLayout } from "./useFoldableLayout";
import type {
  AppInfo,
  AppUpdateStatus,
  DesktopSnapshot,
  DownloadItem,
  InstalledCollection,
  ManagedGame,
  MobileConnectionState,
  RemoteBanner,
  RemoteDownloadRequest,
  StagedMod,
  StagedModUpdate,
  SystemPalette,
} from "./types";
import appLogo from "./assets/app-logo.png";

type MobileTab =
  | "library"
  | "browse"
  | "collections"
  | "downloads"
  | "friends"
  | "settings";

type MobileAppProps = {
  appInfo: AppInfo;
};

type LocalBanner = {
  id: number;
  kind: "info" | "warn" | "error";
  message: string;
  details?: string[];
};

const EMPTY_CONN: MobileConnectionState = {
  paired: false,
  desktop_name: null,
  desktop_peer_id: null,
  mode: "disconnected",
  last_synced_at: null,
  last_error: null,
  allow_remote_launch: false,
};

function BrandMark({ className }: { className: string }) {
  return (
    <div
      className={`brand ${className}`}
      role="img"
      aria-label="Emperor Mod Manager"
      style={{ "--brand-mask": `url(${appLogo})` } as React.CSSProperties}
    />
  );
}

export default function MobileApp({ appInfo }: MobileAppProps) {
  const layout = useFoldableLayout();
  const [conn, setConn] = useState<MobileConnectionState | null>(null);
  const [snapshot, setSnapshot] = useState<DesktopSnapshot | null>(null);
  const [tab, setTab] = useState<MobileTab>("library");
  const [libraryView, setLibraryView] = useState<"home" | "game">("home");
  const [selectedGameId, setSelectedGameId] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);

  // In-app back navigation stack wired to Android system back / gesture (popstate)
  const backStackRef = useRef<Array<() => void>>([]);
  const pushBack = useCallback((handler: () => void) => {
    backStackRef.current.push(handler);
    try {
      window.history.pushState({ emmDepth: backStackRef.current.length }, "");
    } catch {
      // ignore in restricted environments
    }
  }, []);

  useEffect(() => {
    const onPopState = () => {
      const handler = backStackRef.current.pop();
      if (handler) {
        handler();
      }
    };
    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  const navigateTab = useCallback(
    (nextTab: MobileTab) => {
      if (nextTab === tab) {
        if (nextTab === "library" && libraryView === "game") {
          setLibraryView("home");
        }
        return;
      }
      const prevTab = tab;
      setTab(nextTab);
      pushBack(() => setTab(prevTab));
    },
    [tab, libraryView, pushBack],
  );

  // Local mobile banners + replicated desktop banners
  const [localBanners, setLocalBanners] = useState<LocalBanner[]>([]);
  const [desktopBanners, setDesktopBanners] = useState<RemoteBanner[]>([]);
  const nextLocalBannerId = useRef(1);

  // Game cover art cache (gameId -> dataURL or http URL)
  const [coverUrls, setCoverUrls] = useState<Record<string, string>>({});
  const fetchedCoversRef = useRef<Set<string>>(new Set());

  // Mod update check results per game
  const [updatesByGame, setUpdatesByGame] = useState<
    Record<string, Record<string, StagedModUpdate>>
  >({});

  // Active FOMOD configurator modal
  const [fomodMod, setFomodMod] = useState<StagedMod | null>(null);

  const [busyAction, setBusyAction] = useState<string | null>(null);
  const mainScrollRef = useRef<HTMLElement | null>(null);

  // Android Material You dynamic palette (falls back to desktop theme when unavailable or disabled)
  const [androidPalette, setAndroidPalette] = useState<SystemPalette | null>(null);
  const [useMaterialYou, setUseMaterialYou] = useState<boolean>(() => {
    try {
      return window.localStorage.getItem("emperor.mobile.material_you") !== "false";
    } catch {
      return true;
    }
  });

  // In-app Android APK update state
  const [updateStatus, setUpdateStatus] = useState<AppUpdateStatus | null>(null);
  const [updateMsg, setUpdateMsg] = useState<string | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [installingUpdate, setInstallingUpdate] = useState(false);

  const pushLocalBanner = useCallback(
    (kind: "info" | "warn" | "error", message: string, details?: string[]) => {
      const id = nextLocalBannerId.current++;
      setLocalBanners((prev) => [{ id, kind, message, details }, ...prev].slice(0, 6));
      if (kind === "info") {
        window.setTimeout(() => {
          setLocalBanners((prev) => prev.filter((b) => b.id !== id));
        }, 7000);
      }
    },
    [],
  );

  // Fetch Android Material You dynamic palette when running on Android 12+
  useEffect(() => {
    let active = true;
    const loadPalette = () => {
      api
        .getMobileSystemPalette()
        .then((pal) => {
          if (active && pal && pal.bg && pal.accent) {
            setAndroidPalette(pal);
          }
        })
        .catch(() => {});
    };
    loadPalette();
    const mq = window.matchMedia?.("(prefers-color-scheme: dark)");
    mq?.addEventListener?.("change", loadPalette);
    return () => {
      active = false;
      mq?.removeEventListener?.("change", loadPalette);
    };
  }, []);

  // Apply Material You palette if enabled & available, otherwise fall back to desktop's synced theme
  useEffect(() => {
    const root = document.documentElement;
    const paletteProps = [
      "--bg",
      "--bg-2",
      "--panel",
      "--ink",
      "--muted",
      "--line",
      "--accent",
      "--accent-2",
      "--on-accent",
      "--input-bg",
    ];
    if (useMaterialYou && androidPalette) {
      root.dataset.theme = androidPalette.dark ? "dark" : "light";
      root.dataset.themeStyle = "system";
      root.style.setProperty("--bg", androidPalette.bg);
      root.style.setProperty("--bg-2", androidPalette.bg_2);
      root.style.setProperty("--panel", androidPalette.panel);
      root.style.setProperty("--ink", androidPalette.ink);
      root.style.setProperty("--muted", androidPalette.muted);
      root.style.setProperty("--line", androidPalette.line);
      root.style.setProperty("--accent", androidPalette.accent);
      root.style.setProperty(
        "--accent-2",
        `color-mix(in srgb, ${androidPalette.accent} 72%, #ff8c00)`,
      );
      if (androidPalette.on_accent) {
        root.style.setProperty("--on-accent", androidPalette.on_accent);
      }
      root.style.setProperty("--input-bg", androidPalette.input_bg);
      return;
    }

    for (const p of paletteProps) {
      root.style.removeProperty(p);
    }
    const acct = snapshot?.account;
    const pref = acct?.theme ?? "system";
    const resolvedTheme =
      pref === "system"
        ? window.matchMedia?.("(prefers-color-scheme: dark)").matches
          ? "dark"
          : "light"
        : pref;
    root.dataset.theme = resolvedTheme;
    root.dataset.themeStyle = acct?.theme_style ?? "app";
    if (acct?.accent_color) {
      root.style.setProperty("--accent", acct.accent_color);
    }
  }, [useMaterialYou, androidPalette, snapshot?.account]);

  const applySnapshot = useCallback((snap: DesktopSnapshot) => {
    setSnapshot(snap);
    if (Array.isArray(snap.banners)) {
      setDesktopBanners(snap.banners);
    }
    setSelectedGameId(
      (prev) => prev ?? snap.active_game_id ?? snap.managed_games[0]?.id ?? null,
    );
  }, []);

  const syncNow = useCallback(
    async (quiet = false) => {
      setSyncing(true);
      try {
        const next = await api.syncMobileNow();
        applySnapshot(next);
        setConn(await api.getMobileConnectionState());
        if (!quiet) {
          pushLocalBanner("info", `Synced with ${next.desktop_name}`);
        }
      } catch (e) {
        if (!quiet) {
          pushLocalBanner("error", e instanceof Error ? e.message : String(e));
        }
        try {
          setConn(await api.getMobileConnectionState());
        } catch {
          // ignore
        }
      } finally {
        setSyncing(false);
      }
    },
    [applySnapshot, pushLocalBanner],
  );

  useEffect(() => {
    let active = true;
    Promise.all([api.getMobileConnectionState(), api.getMobileSnapshot()])
      .then(([state, snap]) => {
        if (!active) return;
        setConn(state);
        if (snap) {
          applySnapshot(snap);
        }
        // Background service in Rust already syncs on startup and emits
        // `remote-snapshot-synced`; only trigger an explicit sync here if we
        // have no cached snapshot yet.
        if (state.paired && !snap) {
          void syncNow(true);
        }
      })
      .catch(() => {
        if (active) setConn(EMPTY_CONN);
      });
    return () => {
      active = false;
    };
  }, [applySnapshot, syncNow]);

  useEffect(() => {
    let active = true;
    const cleanups: (() => void)[] = [];

    listen<MobileConnectionState>("remote-connection-changed", (event) => {
      if (active) setConn(event.payload);
    })
      .then((fn) => (active ? cleanups.push(fn) : fn()))
      .catch(() => {});

    listen<DesktopSnapshot>("remote-snapshot-synced", (event) => {
      if (active) applySnapshot(event.payload);
    })
      .then((fn) => (active ? cleanups.push(fn) : fn()))
      .catch(() => {});

    listen<RemoteBanner[]>("remote-banners-changed", (event) => {
      if (active && Array.isArray(event.payload)) {
        setDesktopBanners(event.payload);
      }
    })
      .then((fn) => (active ? cleanups.push(fn) : fn()))
      .catch(() => {});

    listen<Partial<DownloadItem> & { id?: string }>(
      "remote-download-progress",
      (event) => {
        const patch = event.payload;
        if (!active || !patch?.id) return;
        setSnapshot((prev) => {
          if (!prev) return prev;
          const idx = prev.downloads.findIndex((d) => d.id === patch.id);
          if (idx === -1) return prev;
          const nextDownloads = [...prev.downloads];
          nextDownloads[idx] = { ...nextDownloads[idx], ...patch };
          return { ...prev, downloads: nextDownloads };
        });
      },
    )
      .then((fn) => (active ? cleanups.push(fn) : fn()))
      .catch(() => {});

    listen<string>("app-update-progress", (event) => {
      if (active) setUpdateMsg(event.payload);
    })
      .then((fn) => (active ? cleanups.push(fn) : fn()))
      .catch(() => {});

    return () => {
      active = false;
      for (const fn of cleanups) fn();
    };
  }, [applySnapshot]);

  const games: ManagedGame[] = snapshot?.managed_games ?? [];
  const activeGame = useMemo(
    () => games.find((g) => g.id === selectedGameId) ?? games[0] ?? null,
    [games, selectedGameId],
  );
  const activeMods: StagedMod[] = useMemo(
    () => (activeGame ? snapshot?.mods_by_game[activeGame.id] ?? [] : []),
    [activeGame, snapshot],
  );
  const activeCollections = useMemo(
    () => (activeGame ? snapshot?.installed_collections_by_game[activeGame.id] ?? [] : []),
    [activeGame, snapshot],
  );
  const savedCollections = snapshot?.saved_collections ?? [];
  const downloads = snapshot?.downloads ?? [];
  const friends = snapshot?.friends ?? null;
  const isOnline = conn?.mode === "lan_direct" || conn?.mode === "iroh_relay";

  const activeUpdates = useMemo(
    () => (activeGame ? updatesByGame[activeGame.id] ?? {} : {}),
    [activeGame, updatesByGame],
  );
  const activeUpdateCount = Object.keys(activeUpdates).length;

  const modsNeedingFomod = useMemo(() => {
    if (!activeGame) return [];
    const ids = new Set(snapshot?.mods_needing_fomod_by_game?.[activeGame.id] ?? []);
    return activeMods.filter((m) => ids.has(m.id));
  }, [activeGame, activeMods, snapshot?.mods_needing_fomod_by_game]);

  const hasBethesdaPlugins = Boolean(
    activeGame && snapshot?.bethesda_plugins_by_game?.[activeGame.id],
  );

  // Fetch game cover art (cached locally first, then throttled over iroh when online)
  useEffect(() => {
    if (games.length === 0) return;
    let cancelled = false;

    const httpCovers: Record<string, string> = {};
    const pending: ManagedGame[] = [];

    for (const g of games) {
      if (fetchedCoversRef.current.has(g.id)) continue;
      if (g.cover_url && /^https?:\/\//i.test(g.cover_url)) {
        fetchedCoversRef.current.add(g.id);
        httpCovers[g.id] = g.cover_url;
        continue;
      }
      pending.push(g);
    }

    if (Object.keys(httpCovers).length > 0) {
      setCoverUrls((prev) => ({ ...prev, ...httpCovers }));
    }

    if (pending.length === 0) return;

    // Prioritize the active game first, then fetch at most 3 covers concurrently.
    const ordered = [...pending].sort((a, b) => {
      if (a.id === selectedGameId) return -1;
      if (b.id === selectedGameId) return 1;
      return 0;
    });

    const CONCURRENCY = 3;
    let cursor = 0;

    async function worker() {
      while (!cancelled && cursor < ordered.length) {
        const g = ordered[cursor++];
        if (fetchedCoversRef.current.has(g.id)) continue;
        fetchedCoversRef.current.add(g.id);
        try {
          const dataUrl = await api.mobileGetGameCover(g.id);
          if (!cancelled && dataUrl) {
            setCoverUrls((prev) => ({ ...prev, [g.id]: dataUrl }));
          } else if (!dataUrl && !isOnline) {
            fetchedCoversRef.current.delete(g.id);
          }
        } catch {
          fetchedCoversRef.current.delete(g.id);
        }
      }
    }

    for (let i = 0; i < Math.min(CONCURRENCY, ordered.length); i++) {
      void worker();
    }

    return () => {
      cancelled = true;
    };
  }, [games, isOnline, selectedGameId]);

  async function runRemote(
    label: string,
    fn: () => Promise<unknown>,
    successMsg?: string,
    needsFullSync = false,
  ): Promise<boolean> {
    setBusyAction(label);
    try {
      await fn();
      if (needsFullSync) {
        await syncNow(true);
      }
      if (successMsg) pushLocalBanner("info", successMsg);
      return true;
    } catch (e) {
      pushLocalBanner("error", e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      setBusyAction(null);
    }
  }

  async function selectGameById(gameId: string) {
    setSelectedGameId(gameId);
    if (isOnline) {
      void api.mobileSetActiveGame(gameId).catch(() => {});
    }
  }

  async function toggleMod(mod: StagedMod, enabled: boolean) {
    if (!activeGame) return;
    const gameId = activeGame.id;
    const prevMods = snapshot?.mods_by_game[gameId];
    // Optimistic update so toggling feels instant
    setSnapshot((prev) => {
      if (!prev) return prev;
      const list = prev.mods_by_game[gameId] ?? [];
      return {
        ...prev,
        mods_by_game: {
          ...prev.mods_by_game,
          [gameId]: list.map((m) => (m.id === mod.id ? { ...m, enabled } : m)),
        },
      };
    });
    const ok = await runRemote(`toggle-${mod.id}`, () =>
      api.mobileSetModEnabled(gameId, mod.id, enabled),
    );
    if (!ok && prevMods) {
      setSnapshot((prev) =>
        prev
          ? { ...prev, mods_by_game: { ...prev.mods_by_game, [gameId]: prevMods } }
          : prev,
      );
    }
  }

  async function moveMod(index: number, delta: -1 | 1) {
    if (!activeGame) return;
    const gameId = activeGame.id;
    const nextIdx = index + delta;
    if (nextIdx < 0 || nextIdx >= activeMods.length) return;
    const prevMods = activeMods;
    const reordered = [...activeMods];
    const [movedMod] = reordered.splice(index, 1);
    reordered.splice(nextIdx, 0, movedMod);
    const ids = reordered.map((m) => m.id);
    // Optimistic reorder
    setSnapshot((prev) =>
      prev
        ? {
            ...prev,
            mods_by_game: {
              ...prev.mods_by_game,
              [gameId]: reordered.map((m, i) => ({ ...m, order: i })),
            },
          }
        : prev,
    );
    const ok = await runRemote(`order-${movedMod.id}`, () =>
      api.mobileSetLoadOrder(gameId, ids),
    );
    if (!ok) {
      setSnapshot((prev) =>
        prev
          ? { ...prev, mods_by_game: { ...prev.mods_by_game, [gameId]: prevMods } }
          : prev,
      );
    }
  }

  async function handleAutoSortMods() {
    if (!activeGame) return;
    await runRemote(
      "auto-sort",
      () => api.mobileAutoSortMods(activeGame.id),
      `Auto-sorted load order for ${activeGame.title}`,
    );
  }

  async function handleSortPlugins() {
    if (!activeGame) return;
    await runRemote(
      "sort-plugins",
      () => api.mobileSortBethesdaPlugins(activeGame.id),
      `Sorted plugins (LOOT) for ${activeGame.title}`,
    );
  }

  async function handleCheckUpdates() {
    if (!activeGame) return;
    setBusyAction("check-updates");
    try {
      const found = await api.mobileCheckModUpdates(activeGame.id);
      const map: Record<string, StagedModUpdate> = {};
      for (const u of found ?? []) {
        map[u.staged_id] = u;
      }
      setUpdatesByGame((prev) => ({ ...prev, [activeGame.id]: map }));
      pushLocalBanner(
        "info",
        found.length === 0
          ? `All mods for ${activeGame.title} are up to date.`
          : `${found.length} mod update${found.length === 1 ? "" : "s"} available for ${activeGame.title}.`,
      );
    } catch (e) {
      pushLocalBanner("error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusyAction(null);
    }
  }

  async function handleUpdateMod(mod: StagedMod) {
    if (!activeGame) return;
    setBusyAction(`update-${mod.id}`);
    try {
      const res = await api.mobileUpdateMod(activeGame.id, mod.id);
      setUpdatesByGame((prev) => {
        const nextGame = { ...(prev[activeGame.id] ?? {}) };
        delete nextGame[mod.id];
        return { ...prev, [activeGame.id]: nextGame };
      });
      await syncNow(true);
      if (res && typeof res.queued_assist === "number" && res.queued_assist > 0) {
        pushLocalBanner(
          "info",
          `Queued update for ${mod.name} in Download Assist on ${conn?.desktop_name ?? "PC"}`,
        );
      } else {
        pushLocalBanner("info", `Updated ${mod.name}`);
      }
    } catch (e) {
      pushLocalBanner("error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusyAction(null);
    }
  }

  async function handleUpdateAllMods() {
    if (!activeGame) return;
    setBusyAction("update-all");
    try {
      const res = await api.mobileUpdateAllMods(activeGame.id);
      setUpdatesByGame((prev) => ({ ...prev, [activeGame.id]: {} }));
      await syncNow(true);
      const queuedAssist = res.queued_assist ?? 0;
      const parts: string[] = [];
      if (res.updated > 0) {
        parts.push(`Updated ${res.updated} mod${res.updated === 1 ? "" : "s"}`);
      }
      if (queuedAssist > 0) {
        parts.push(
          `Queued ${queuedAssist} mod${queuedAssist === 1 ? "" : "s"} in Download Assist on ${conn?.desktop_name ?? "PC"}`,
        );
      }
      const summary = parts.join(" · ") || "All mods up to date";
      if (res.errors?.length) {
        pushLocalBanner("warn", `${summary} (${res.errors.length} failed)`, res.errors);
      } else {
        pushLocalBanner("info", summary);
      }
    } catch (e) {
      pushLocalBanner("error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusyAction(null);
    }
  }

  async function toggleCollection(col: InstalledCollection, enabled: boolean) {
    if (!activeGame) return;
    await runRemote(`col-${col.id}`, () =>
      api.mobileSetCollectionEnabled(activeGame.id, col.id, enabled),
    );
  }

  async function handleQueueDownload(req: RemoteDownloadRequest, label: string) {
    setBusyAction(`queue-${label}`);
    try {
      const res = (await api.mobileQueueDownload(req)) as { queued_assist?: number } | null;
      await syncNow(true);
      if (res && typeof res.queued_assist === "number" && res.queued_assist > 0) {
        pushLocalBanner(
          "info",
          `Queued "${label}" in Download Assist on ${conn?.desktop_name ?? "PC"}`,
        );
      } else {
        pushLocalBanner("info", `Queued "${label}" on ${conn?.desktop_name ?? "PC"}`);
      }
    } catch (e) {
      pushLocalBanner("error", e instanceof Error ? e.message : String(e));
    } finally {
      setBusyAction(null);
    }
  }

  async function handleImportShareCode(code: string) {
    if (!activeGame) return;
    await runRemote(
      "import-share",
      () =>
        api.mobileQueueDownload({
          kind: "import_share_code",
          game_id: activeGame.id,
          code,
        }),
      "Queued collection import on your PC",
    );
  }

  async function dismissDesktopBanner(id: number) {
    setDesktopBanners((prev) => prev.filter((b) => b.id !== id));
    if (isOnline) {
      void api.mobileDismissBanner(id).catch(() => {});
    }
  }

  async function checkApkUpdate() {
    setCheckingUpdate(true);
    setUpdateMsg(null);
    try {
      const status = await api.checkAppUpdate();
      setUpdateStatus(status);
      if (!status.update_available) {
        setUpdateMsg(`Up to date (v${status.current_version})`);
      }
    } catch (e) {
      setUpdateMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setCheckingUpdate(false);
    }
  }

  async function installApkUpdate() {
    setInstallingUpdate(true);
    setUpdateMsg("Downloading APK…");
    try {
      const result = await api.installAppUpdate();
      setUpdateMsg(result.message);
    } catch (e) {
      setUpdateMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setInstallingUpdate(false);
    }
  }

  if (conn === null) {
    return (
      <div className="boot-splash">
        <BrandMark className="boot-splash-logo" />
      </div>
    );
  }

  if (!conn.paired) {
    return (
      <MobilePairingView
        onPaired={(nextConn) => {
          setConn(nextConn);
          void syncNow(true);
        }}
      />
    );
  }

  const navItems: { id: MobileTab; label: string; icon: typeof faGamepad }[] = [
    { id: "library", label: "Library", icon: faGamepad },
    { id: "browse", label: "Browse", icon: faCompass },
    { id: "collections", label: "Collections", icon: faLayerGroup },
    { id: "downloads", label: "Downloads", icon: faDownload },
    { id: "friends", label: "Friends", icon: faUsers },
    { id: "settings", label: "Settings", icon: faGear },
  ];

  const activeLaunch = activeGame ? snapshot?.launch_states[activeGame.id] : undefined;
  const isRunning =
    activeLaunch?.phase === "running" || activeLaunch?.phase === "starting";
  const isWide = layout.navMode !== "bottom";

  const activeGameCoverSrc = activeGame
    ? coverUrls[activeGame.id] ?? activeGame.cover_url ?? null
    : null;
  const userName =
    snapshot?.account.p2p_display_name ||
    snapshot?.friends?.display_name ||
    "You";
  const userAvatar =
    snapshot?.account.p2p_profile_picture ??
    snapshot?.friends?.profile_picture ??
    null;

  const openGamePage = (gameId: string) => {
    void selectGameById(gameId);
    setTab("library");
    if (libraryView !== "game") {
      setLibraryView("game");
      pushBack(() => setLibraryView("home"));
    }
  };

  const activeDownloadCount = downloads.filter(
    (d) => d.status === "downloading" || d.status === "queued" || d.status === "extracting",
  ).length;
  const activeEnabledModCount = activeMods.filter((m) => m.enabled).length;

  return (
    <div
      className={`mobile-app${isWide ? " mobile-layout-wide" : " mobile-layout-compact"}${
        layout.hasHinge && layout.hingeOrientation === "vertical"
          ? " mobile-layout-hinged"
          : ""
      }`}
      style={
        activeGameCoverSrc
          ? ({
              "--active-game-cover": `url("${activeGameCoverSrc.replace(/"/g, '\\"')}")`,
            } as React.CSSProperties)
          : undefined
      }
    >
      {activeGameCoverSrc ? (
        <div className="app-game-backdrop" aria-hidden="true" />
      ) : null}

      {isWide && (
        <aside className="mobile-rail" aria-label="Primary navigation">
          <div className="mobile-rail-brand">
            <BrandMark className="mobile-rail-logo" />
          </div>
          <nav className="mobile-rail-nav">
            {navItems.map((item) => (
              <button
                key={item.id}
                type="button"
                className={`mobile-nav-btn${tab === item.id ? " active" : ""}`}
                onClick={() => navigateTab(item.id)}
              >
                <FontAwesomeIcon icon={item.icon} />
                <span>{item.label}</span>
              </button>
            ))}
          </nav>
          <div className="mobile-rail-footer">
            <button
              type="button"
              className="mobile-rail-avatar-btn"
              title={userName}
              onClick={() => navigateTab("settings")}
            >
              <FriendAvatar name={userName} src={userAvatar} size="sm" />
            </button>
          </div>
        </aside>
      )}

      <div className="mobile-shell">
        <header className="mobile-topbar">
          <div className="mobile-topbar-left">
            {tab === "library" && libraryView === "game" && (
              <button
                type="button"
                className="ghost mobile-topbar-back-btn"
                aria-label="Back to Library"
                onClick={() => setLibraryView("home")}
              >
                <FontAwesomeIcon icon={faArrowLeft} />
              </button>
            )}
            {activeGame ? (
              <button
                type="button"
                className={`mobile-topbar-active-game${
                  tab === "library" && libraryView === "game" ? " active" : ""
                }`}
                onClick={() => openGamePage(activeGame.id)}
                title={`Active game: ${activeGame.title} — tap to manage`}
              >
                {activeGameCoverSrc ? (
                  <img
                    src={activeGameCoverSrc}
                    alt=""
                    className="mobile-topbar-active-cover"
                  />
                ) : (
                  <span className="mobile-topbar-active-cover placeholder" aria-hidden="true">
                    <FontAwesomeIcon icon={faGamepad} />
                  </span>
                )}
                <span className="mobile-topbar-active-meta">
                  <span className="mobile-topbar-active-kicker">Active Game</span>
                  <strong className="mobile-topbar-active-title">{activeGame.title}</strong>
                </span>
                <span className="mobile-topbar-active-count">
                  {activeEnabledModCount}/{activeMods.length}
                </span>
              </button>
            ) : (
              <span className="mobile-topbar-empty">No active game</span>
            )}
          </div>
          <div className="mobile-topbar-right">
            <MobileStatusBar
              state={conn}
              syncing={syncing}
              compact={!isWide}
              onSyncNow={() => void syncNow(false)}
            />
          </div>
        </header>

        {(desktopBanners.length > 0 || localBanners.length > 0) && (
          <div className="mobile-banner-region" role="region" aria-label="Status notices">
            <div className="banner-stack">
              {desktopBanners.map((b) => (
                <StatusBanner
                  key={`desk-${b.id}`}
                  kind={b.kind}
                  message={b.message}
                  details={b.details}
                  onDismiss={() => void dismissDesktopBanner(b.id)}
                />
              ))}
              {localBanners.map((b) => (
                <StatusBanner
                  key={`loc-${b.id}`}
                  kind={b.kind}
                  message={b.message}
                  details={b.details}
                  onDismiss={() =>
                    setLocalBanners((prev) => prev.filter((item) => item.id !== b.id))
                  }
                />
              ))}
            </div>
          </div>
        )}

        <main ref={mainScrollRef} className="mobile-main">
          {tab === "library" && libraryView === "home" && (
            <div className="mobile-library-home">
              <div className="mobile-toolbar-row">
                <h2>Library ({games.length})</h2>
                {activeGame && (
                  <button
                    type="button"
                    className="secondary"
                    onClick={() => openGamePage(activeGame.id)}
                  >
                    Open {activeGame.title}
                  </button>
                )}
              </div>
              {games.length === 0 ? (
                <div className="mobile-surface-card">
                  <p className="muted">
                    No managed games synced yet. Manage a game on your desktop and tap Sync.
                  </p>
                </div>
              ) : (
                <div className="mobile-library-grid">
                  {games.map((g) => {
                    const count = snapshot?.mods_by_game[g.id]?.length ?? 0;
                    const enabledCount =
                      snapshot?.mods_by_game[g.id]?.filter((m) => m.enabled).length ?? 0;
                    const cover = coverUrls[g.id] ?? g.cover_url ?? null;
                    return (
                      <MediaCard
                        key={g.id}
                        title={g.title}
                        imageSrc={cover}
                        active={activeGame?.id === g.id}
                        badge={activeGame?.id === g.id ? "Active" : null}
                        meta={
                          <span className="muted">
                            {enabledCount}/{count} mods enabled
                          </span>
                        }
                        loading="eager"
                        onClick={() => openGamePage(g.id)}
                      />
                    );
                  })}
                </div>
              )}
            </div>
          )}

          {tab === "library" && libraryView === "game" && (
            <div className="mobile-detail-view">
              <button
                type="button"
                className="ghost mobile-back-btn"
                onClick={() => setLibraryView("home")}
              >
                <FontAwesomeIcon icon={faArrowLeft} /> Back to Library
              </button>
              {activeGame ? (
                <>
                  <div className="mobile-surface-card mobile-game-header">
                    <div className="mobile-game-header-top">
                      {coverUrls[activeGame.id] ?? activeGame.cover_url ? (
                        <img
                          className="mobile-game-header-cover"
                          src={(coverUrls[activeGame.id] ?? activeGame.cover_url)!}
                          alt={activeGame.title}
                        />
                      ) : null}
                      <div className="mobile-game-header-info">
                        <h2>{activeGame.title}</h2>
                        <span className="muted">
                          {activeMods.filter((m) => m.enabled).length} of {activeMods.length} mods enabled
                        </span>
                      </div>
                    </div>

                    <div className="mobile-action-bar">
                      {conn.allow_remote_launch &&
                        (isRunning ? (
                          <button
                            type="button"
                            className="danger"
                            disabled={!isOnline || busyAction !== null}
                            onClick={() =>
                              void runRemote("close-game", () =>
                                api.mobileCloseGame(activeGame.id),
                              )
                            }
                          >
                            <FontAwesomeIcon icon={faStop} /> Close Game
                          </button>
                        ) : (
                          <button
                            type="button"
                            className="primary"
                            disabled={!isOnline || busyAction !== null}
                            onClick={() =>
                              void runRemote("launch-modded", () =>
                                api.mobileLaunchGame(activeGame.id, true),
                              )
                            }
                          >
                            <FontAwesomeIcon icon={faPlay} /> Launch Modded
                          </button>
                        ))}
                      <button
                        type="button"
                        className="secondary"
                        disabled={busyAction !== null}
                        onClick={() => void handleCheckUpdates()}
                      >
                        <FontAwesomeIcon
                          icon={faRotateRight}
                          spin={busyAction === "check-updates"}
                        />{" "}
                        Check Updates
                      </button>
                      {activeUpdateCount > 0 && (
                        <button
                          type="button"
                          className="primary"
                          disabled={!isOnline || busyAction !== null}
                          onClick={() => void handleUpdateAllMods()}
                        >
                          <FontAwesomeIcon icon={faCloudArrowDown} /> Update All (
                          {activeUpdateCount})
                        </button>
                      )}
                      <button
                        type="button"
                        className="secondary"
                        disabled={!isOnline || busyAction !== null || activeMods.length < 2}
                        onClick={() => void handleAutoSortMods()}
                      >
                        <FontAwesomeIcon icon={faArrowDownShortWide} /> Auto-Sort
                      </button>
                      {hasBethesdaPlugins && (
                        <button
                          type="button"
                          className="secondary"
                          disabled={!isOnline || busyAction !== null}
                          onClick={() => void handleSortPlugins()}
                        >
                          <FontAwesomeIcon icon={faArrowDownShortWide} /> Sort Plugins
                        </button>
                      )}
                    </div>
                  </div>

                  <FomodConfigureBanner
                    mods={modsNeedingFomod}
                    onConfigure={() => {
                      if (modsNeedingFomod[0]) {
                        setFomodMod(modsNeedingFomod[0]);
                        pushBack(() => setFomodMod(null));
                      }
                    }}
                  />

                  {activeMods.length === 0 ? (
                    <div className="mobile-surface-card">
                      <p className="muted">
                        No mods installed for {activeGame.title}. Use the Browse tab to install mods on your PC.
                      </p>
                    </div>
                  ) : (
                    <div className="mobile-mod-list">
                      {activeMods.map((mod, idx) => {
                        const upd = activeUpdates[mod.id];
                        const needsFomod = modsNeedingFomod.some((m) => m.id === mod.id);
                        const hasFomod = needsFomod || Boolean(mod.fomod_selection);
                        return (
                          <div
                            key={mod.id}
                            className={`mobile-mod-row${mod.enabled ? "" : " disabled"}`}
                          >
                            {mod.picture_url ? (
                              <img
                                className="mobile-mod-thumb"
                                src={mod.picture_url}
                                alt={mod.name}
                                loading="lazy"
                              />
                            ) : (
                              <div className="mobile-mod-thumb mobile-mod-thumb-placeholder">
                                <FontAwesomeIcon icon={faGamepad} />
                              </div>
                            )}
                            <div className="mobile-mod-info">
                              <strong>{mod.name}</strong>
                              <span className="muted">
                                #{idx + 1}
                                {mod.version ? ` · v${mod.version}` : ""}
                                {upd?.available_version ? ` → v${upd.available_version}` : ""}
                              </span>
                            </div>
                            <div className="mobile-mod-actions">
                              {upd && (
                                <button
                                  type="button"
                                  className="primary mobile-mini-btn"
                                  disabled={!isOnline || busyAction !== null}
                                  onClick={() => void handleUpdateMod(mod)}
                                >
                                  Update
                                </button>
                              )}
                              {hasFomod && (
                                <button
                                  type="button"
                                  className="secondary mobile-icon-btn"
                                  aria-label={`Configure FOMOD for ${mod.name}`}
                                  title="Configure FOMOD"
                                  disabled={!isOnline}
                                  onClick={() => {
                                    setFomodMod(mod);
                                    pushBack(() => setFomodMod(null));
                                  }}
                                >
                                  <FontAwesomeIcon icon={faSliders} />
                                </button>
                              )}
                              <button
                                type="button"
                                className="ghost mobile-icon-btn"
                                aria-label="Move up"
                                disabled={!isOnline || idx === 0 || busyAction !== null}
                                onClick={() => void moveMod(idx, -1)}
                              >
                                <FontAwesomeIcon icon={faArrowUp} />
                              </button>
                              <button
                                type="button"
                                className="ghost mobile-icon-btn"
                                aria-label="Move down"
                                disabled={
                                  !isOnline ||
                                  idx === activeMods.length - 1 ||
                                  busyAction !== null
                                }
                                onClick={() => void moveMod(idx, 1)}
                              >
                                <FontAwesomeIcon icon={faArrowDown} />
                              </button>
                              <SettingsToggle
                                checked={mod.enabled}
                                disabled={!isOnline || busyAction !== null}
                                onChange={(checked) => void toggleMod(mod, checked)}
                              >
                                {mod.enabled ? "On" : "Off"}
                              </SettingsToggle>
                            </div>
                          </div>
                        );
                      })}
                    </div>
                  )}
                </>
              ) : (
                <p className="muted">Select a game to inspect its load order.</p>
              )}
            </div>
          )}

          {tab === "browse" && (
            <MobileBrowseView
              activeGame={activeGame}
              scrollRef={mainScrollRef}
              online={isOnline}
              onQueueDownload={handleQueueDownload}
              onError={(msg) => pushLocalBanner("error", msg)}
              onPushBack={pushBack}
            />
          )}

          {tab === "collections" && (
            <MobileCollectionsView
              activeGame={activeGame}
              installedCollections={activeCollections}
              savedCollections={savedCollections}
              mods={activeMods}
              updatesByModId={activeUpdates}
              online={isOnline}
              onRefresh={() => syncNow(true)}
              onToggleCollection={toggleCollection}
              onToggleMod={toggleMod}
              onCheckUpdates={handleCheckUpdates}
              onUpdateAll={handleUpdateAllMods}
              onImportShareCode={handleImportShareCode}
              onNotify={pushLocalBanner}
              onPushBack={pushBack}
            />
          )}

          {tab === "downloads" && (
            <div className="mobile-downloads-pane">
              <div className="mobile-toolbar-row">
                <h2>Desktop Downloads ({downloads.length})</h2>
                {downloads.length > 0 && (
                  <button
                    type="button"
                    className="secondary"
                    disabled={!isOnline || busyAction !== null}
                    onClick={() =>
                      void runRemote("clear-dl", () =>
                        api.mobileControlDownload({ action: "clear_recent" }),
                      )
                    }
                  >
                    Clear finished
                  </button>
                )}
              </div>

              {downloads.length === 0 ? (
                <div className="mobile-surface-card">
                  <p className="muted">
                    No active or recent downloads on your desktop. Browse mods to queue a download.
                  </p>
                </div>
              ) : (
                <div className="mobile-card-list">
                  {downloads.map((dl) => {
                    const pct =
                      dl.bytes_total && dl.bytes_total > 0
                        ? Math.min(100, Math.round((dl.bytes_downloaded / dl.bytes_total) * 100))
                        : null;
                    return (
                      <div key={dl.id} className="mobile-download-card">
                        <div className="mobile-download-head">
                          <strong>{dl.label}</strong>
                          <span className="pill">{dl.status}</span>
                        </div>
                        {pct !== null && (
                          <div className="mobile-progress-track">
                            <div
                              className="mobile-progress-fill"
                              style={{ width: `${pct}%` }}
                            />
                          </div>
                        )}
                        <div className="mobile-action-bar">
                          {dl.status === "downloading" && (
                            <button
                              type="button"
                              className="secondary"
                              disabled={!isOnline}
                              onClick={() =>
                                void runRemote(`pause-${dl.id}`, () =>
                                  api.mobileControlDownload({ action: "pause", id: dl.id }),
                                )
                              }
                            >
                              <FontAwesomeIcon icon={faPause} /> Pause
                            </button>
                          )}
                          {dl.status === "paused" && (
                            <button
                              type="button"
                              className="primary"
                              disabled={!isOnline}
                              onClick={() =>
                                void runRemote(`resume-${dl.id}`, () =>
                                  api.mobileControlDownload({ action: "resume", id: dl.id }),
                                )
                              }
                            >
                              <FontAwesomeIcon icon={faPlay} /> Resume
                            </button>
                          )}
                          {(dl.status === "downloading" ||
                            dl.status === "queued" ||
                            dl.status === "paused") && (
                            <button
                              type="button"
                              className="ghost danger"
                              disabled={!isOnline}
                              onClick={() =>
                                void runRemote(`cancel-${dl.id}`, () =>
                                  api.mobileControlDownload({ action: "cancel", id: dl.id }),
                                )
                              }
                            >
                              <FontAwesomeIcon icon={faXmark} /> Cancel
                            </button>
                          )}
                          {(dl.status === "done" ||
                            dl.status === "failed" ||
                            dl.status === "cancelled") && (
                            <button
                              type="button"
                              className="ghost"
                              disabled={!isOnline}
                              onClick={() =>
                                void runRemote(`remove-${dl.id}`, () =>
                                  api.mobileControlDownload({ action: "remove", id: dl.id }),
                                )
                              }
                            >
                              <FontAwesomeIcon icon={faTrashCan} /> Remove
                            </button>
                          )}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          )}

          {tab === "friends" && (
            <MobileFriendsView
              friendsState={friends}
              activeGame={activeGame}
              online={isOnline}
              onRefresh={() => syncNow(true)}
              onNotify={pushLocalBanner}
              onPushBack={pushBack}
            />
          )}

          {tab === "settings" && (
            <div className="mobile-settings-pane">
              <div className="mobile-toolbar-row">
                <h2>Settings</h2>
              </div>
              <section className="mobile-surface-card mobile-settings-card mobile-profile-card">
                <FriendAvatar name={userName} src={userAvatar} size="lg" />
                <div>
                  <h3>{userName}</h3>
                  <p className="muted">
                    Synced from {conn.desktop_name ?? "paired desktop"}
                  </p>
                </div>
              </section>
              <section className="mobile-surface-card mobile-settings-card">
                <h3>Appearance</h3>
                {androidPalette ? (
                  <SettingsToggle
                    checked={useMaterialYou}
                    onChange={(checked) => {
                      setUseMaterialYou(checked);
                      try {
                        window.localStorage.setItem(
                          "emperor.mobile.material_you",
                          checked ? "true" : "false",
                        );
                      } catch {
                        // ignore
                      }
                    }}
                  >
                    Use phone Material You color theme
                  </SettingsToggle>
                ) : (
                  <p className="muted">
                    Following paired desktop theme ({snapshot?.account.theme ?? "system"}).
                  </p>
                )}
              </section>
              <section className="mobile-surface-card mobile-settings-card">
                <h3>Paired Desktop</h3>
                <p>
                  <strong>{conn.desktop_name ?? "Desktop"}</strong>
                </p>
                <p className="muted">
                  Peer ID: <code>{conn.desktop_peer_id?.slice(0, 18) ?? "—"}…</code>
                </p>
                <p className="muted">
                  Remote game launch:{" "}
                  <strong>{conn.allow_remote_launch ? "Allowed" : "Disabled"}</strong>{" "}
                  (configured on Desktop)
                </p>
                <div className="mobile-action-bar">
                  <button
                    type="button"
                    className="secondary"
                    onClick={() => void syncNow(false)}
                    disabled={syncing}
                  >
                    <FontAwesomeIcon icon={faRotateRight} spin={syncing} /> Sync Now
                  </button>
                  <button
                    type="button"
                    className="ghost danger"
                    onClick={() => {
                      void api.unpairDesktop().then(() => {
                        setConn(EMPTY_CONN);
                        setSnapshot(null);
                        setDesktopBanners([]);
                      });
                    }}
                  >
                    Unpair Desktop
                  </button>
                </div>
              </section>

              <section className="mobile-surface-card mobile-settings-card">
                <h3>Synced Accounts</h3>
                <p className="muted">
                  Nexus Mods:{" "}
                  <strong>
                    {snapshot?.account.nexus_connected ? "Connected" : "Not connected"}
                  </strong>
                </p>
                <p className="muted">
                  mod.io:{" "}
                  <strong>
                    {snapshot?.account.modio_connected ? "Connected" : "Not connected"}
                  </strong>
                </p>
                <p className="muted">
                  Account credentials are encrypted on-device and synced automatically from
                  your paired desktop.
                </p>
              </section>

              <section className="mobile-surface-card mobile-settings-card">
                <h3>App Updates</h3>
                <p className="muted">
                  Emperor Mod Manager v{appInfo.version} ({appInfo.platform})
                </p>
                <div className="mobile-action-bar">
                  <button
                    type="button"
                    className="secondary"
                    onClick={() => void checkApkUpdate()}
                    disabled={checkingUpdate || installingUpdate}
                  >
                    {checkingUpdate ? "Checking…" : "Check for Updates"}
                  </button>
                  {updateStatus?.update_available && (
                    <button
                      type="button"
                      className="primary"
                      onClick={() => void installApkUpdate()}
                      disabled={installingUpdate}
                    >
                      {installingUpdate
                        ? "Downloading…"
                        : `Install v${updateStatus.latest_version}`}
                    </button>
                  )}
                </div>
                {updateMsg && <p className="muted">{updateMsg}</p>}
              </section>
            </div>
          )}
        </main>

        {!isWide && (
          <div className="mobile-bottom-dock">
            <nav className="mobile-bottom-nav" aria-label="Primary navigation">
              {navItems.map((item) => (
                <button
                  key={item.id}
                  type="button"
                  className={`mobile-nav-btn${tab === item.id ? " active" : ""}`}
                  onClick={() => navigateTab(item.id)}
                >
                  <span className="mobile-nav-icon-pill">
                    <FontAwesomeIcon icon={item.icon} />
                    {item.id === "downloads" && activeDownloadCount > 0 && (
                      <span className="mobile-nav-badge" aria-label={`${activeDownloadCount} active downloads`}>
                        {activeDownloadCount}
                      </span>
                    )}
                  </span>
                  <span className="mobile-nav-label">{item.label}</span>
                </button>
              ))}
            </nav>
          </div>
        )}
      </div>

      {fomodMod && activeGame && (
        <MobileFomodModal
          gameId={activeGame.id}
          mod={fomodMod}
          onClose={() => setFomodMod(null)}
          onSaved={async () => {
            await syncNow(true);
            pushLocalBanner("info", `Saved FOMOD choices for ${fomodMod.name}`);
          }}
        />
      )}
    </div>
  );
}
