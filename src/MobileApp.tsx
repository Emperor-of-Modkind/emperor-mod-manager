import { useCallback, useEffect, useMemo, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowDown,
  faArrowUp,
  faDownload,
  faGamepad,
  faGear,
  faLayerGroup,
  faPause,
  faPlay,
  faRotateRight,
  faStop,
  faTrashCan,
  faUsers,
  faXmark,
} from "@fortawesome/free-solid-svg-icons";
import { api } from "./api";
import { MobilePairingView } from "./MobilePairingView";
import { MobileStatusBar } from "./MobileStatusBar";
import { useFoldableLayout } from "./useFoldableLayout";
import type {
  AppInfo,
  AppUpdateStatus,
  DesktopSnapshot,
  ManagedGame,
  MobileConnectionState,
  StagedMod,
} from "./types";
import appLogo from "./assets/app-logo.png";

type MobileTab = "library" | "collections" | "downloads" | "friends" | "settings";

type MobileAppProps = {
  appInfo: AppInfo;
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

export default function MobileApp({ appInfo }: MobileAppProps) {
  const layout = useFoldableLayout();
  const [conn, setConn] = useState<MobileConnectionState | null>(null);
  const [snapshot, setSnapshot] = useState<DesktopSnapshot | null>(null);
  const [tab, setTab] = useState<MobileTab>("library");
  const [selectedGameId, setSelectedGameId] = useState<string | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [banner, setBanner] = useState<{ kind: "ok" | "err"; text: string } | null>(null);

  // Queue download / share code inputs
  const [shareCodeInput, setShareCodeInput] = useState("");
  const [tsCommunity, setTsCommunity] = useState("");
  const [tsNamespace, setTsNamespace] = useState("");
  const [tsName, setTsName] = useState("");
  const [busyAction, setBusyAction] = useState<string | null>(null);

  // In-app Android APK update state
  const [updateStatus, setUpdateStatus] = useState<AppUpdateStatus | null>(null);
  const [updateMsg, setUpdateMsg] = useState<string | null>(null);
  const [checkingUpdate, setCheckingUpdate] = useState(false);
  const [installingUpdate, setInstallingUpdate] = useState(false);

  // Apply synced desktop theme onto :root
  useEffect(() => {
    const acct = snapshot?.account;
    const root = document.documentElement;
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
  }, [snapshot?.account]);

  const syncNow = useCallback(async (quiet = false) => {
    setSyncing(true);
    try {
      const next = await api.syncMobileNow();
      setSnapshot(next);
      setConn(await api.getMobileConnectionState());
      if (!quiet) {
        setBanner({ kind: "ok", text: `Synced with ${next.desktop_name}` });
      }
    } catch (e) {
      if (!quiet) {
        setBanner({ kind: "err", text: e instanceof Error ? e.message : String(e) });
      }
      try {
        setConn(await api.getMobileConnectionState());
      } catch {
        // ignore
      }
    } finally {
      setSyncing(false);
    }
  }, []);

  useEffect(() => {
    let active = true;
    Promise.all([api.getMobileConnectionState(), api.getMobileSnapshot()])
      .then(([state, snap]) => {
        if (!active) return;
        setConn(state);
        if (snap) {
          setSnapshot(snap);
          setSelectedGameId((prev) => prev ?? snap.active_game_id ?? snap.managed_games[0]?.id ?? null);
        }
        if (state.paired) {
          void syncNow(true);
        }
      })
      .catch(() => {
        if (active) setConn(EMPTY_CONN);
      });
    return () => {
      active = false;
    };
  }, [syncNow]);

  useEffect(() => {
    let active = true;
    const cleanups: (() => void)[] = [];

    listen<MobileConnectionState>("remote-connection-changed", (event) => {
      if (active) setConn(event.payload);
    })
      .then((fn) => (active ? cleanups.push(fn) : fn()))
      .catch(() => {});

    listen<DesktopSnapshot>("remote-snapshot-synced", (event) => {
      if (!active) return;
      setSnapshot(event.payload);
      setSelectedGameId(
        (prev) => prev ?? event.payload.active_game_id ?? event.payload.managed_games[0]?.id ?? null,
      );
    })
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
  }, []);

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
  const downloads = snapshot?.downloads ?? [];
  const friends = snapshot?.friends ?? null;
  const isOnline = conn?.mode === "lan_direct" || conn?.mode === "iroh_relay";

  async function runRemote(label: string, fn: () => Promise<unknown>) {
    setBusyAction(label);
    setBanner(null);
    try {
      await fn();
      await syncNow(true);
    } catch (e) {
      setBanner({ kind: "err", text: e instanceof Error ? e.message : String(e) });
    } finally {
      setBusyAction(null);
    }
  }

  async function selectGame(game: ManagedGame) {
    setSelectedGameId(game.id);
    if (isOnline) {
      void api.mobileSetActiveGame(game.id).catch(() => {});
    }
  }

  async function toggleMod(mod: StagedMod) {
    if (!activeGame) return;
    await runRemote(`toggle-${mod.id}`, () =>
      api.mobileSetModEnabled(activeGame.id, mod.id, !mod.enabled),
    );
  }

  async function moveMod(index: number, delta: -1 | 1) {
    if (!activeGame) return;
    const nextIdx = index + delta;
    if (nextIdx < 0 || nextIdx >= activeMods.length) return;
    const ids = activeMods.map((m) => m.id);
    const [moved] = ids.splice(index, 1);
    ids.splice(nextIdx, 0, moved);
    await runRemote(`order-${moved}`, () => api.mobileSetLoadOrder(activeGame.id, ids));
  }

  async function toggleCollection(colId: string, enabled: boolean) {
    if (!activeGame) return;
    await runRemote(`col-${colId}`, () =>
      api.mobileSetCollectionEnabled(activeGame.id, colId, enabled),
    );
  }

  async function importShareCode(e: React.FormEvent) {
    e.preventDefault();
    if (!activeGame || !shareCodeInput.trim()) return;
    await runRemote("import-share", () =>
      api.mobileQueueDownload({
        kind: "import_share_code",
        game_id: activeGame.id,
        code: shareCodeInput.trim(),
      }),
    );
    setShareCodeInput("");
    setBanner({ kind: "ok", text: "Queued collection share code on desktop" });
  }

  async function queueThunderstore(e: React.FormEvent) {
    e.preventDefault();
    if (!activeGame || !tsNamespace.trim() || !tsName.trim()) return;
    const community = tsCommunity.trim() || activeGame.thunderstore_community || "";
    if (!community) {
      setBanner({ kind: "err", text: "Enter a Thunderstore community slug for this game." });
      return;
    }
    await runRemote("queue-ts", () =>
      api.mobileQueueDownload({
        kind: "thunderstore_mod",
        game_id: activeGame.id,
        community,
        namespace: tsNamespace.trim(),
        name: tsName.trim(),
      }),
    );
    setTsNamespace("");
    setTsName("");
    setBanner({ kind: "ok", text: "Queued Thunderstore download on desktop" });
  }

  async function checkUpdates() {
    setCheckingUpdate(true);
    setUpdateMsg(null);
    try {
      const status = await api.checkAppUpdate();
      setUpdateStatus(status);
      setUpdateMsg(
        status.update_available
          ? `Update available: v${status.latest_version}`
          : "You’re on the latest version.",
      );
    } catch (e) {
      setUpdateMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setCheckingUpdate(false);
    }
  }

  async function installUpdate() {
    setInstallingUpdate(true);
    setUpdateMsg("Downloading APK…");
    try {
      const res = await api.installAppUpdate();
      setUpdateMsg(res.message);
    } catch (e) {
      setUpdateMsg(e instanceof Error ? e.message : String(e));
    } finally {
      setInstallingUpdate(false);
    }
  }

  async function unpair() {
    await api.unpairDesktop();
    setSnapshot(null);
    setConn(EMPTY_CONN);
    setBanner(null);
  }

  if (conn === null) {
    return (
      <div className="boot-splash">
        <img src={appLogo} alt="Emperor Mod Manager" className="boot-splash-logo" />
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
    { id: "collections", label: "Collections", icon: faLayerGroup },
    { id: "downloads", label: "Downloads", icon: faDownload },
    { id: "friends", label: "Friends", icon: faUsers },
    { id: "settings", label: "Settings", icon: faGear },
  ];

  const gamesPane = (
    <div className="mobile-pane">
      <h3 className="mobile-pane-title">Managed Games ({games.length})</h3>
      {games.length === 0 ? (
        <p className="muted">No managed games synced from desktop yet.</p>
      ) : (
        <div className="mobile-card-list">
          {games.map((g) => (
            <button
              key={g.id}
              type="button"
              className={`mobile-list-card${activeGame?.id === g.id ? " selected" : ""}`}
              onClick={() => void selectGame(g)}
            >
              {g.cover_url && (
                <img src={g.cover_url} alt="" className="mobile-game-thumb" />
              )}
              <div className="mobile-card-body">
                <strong>{g.title}</strong>
                <span className="muted">
                  {(snapshot?.mods_by_game[g.id] ?? []).length} mods · {g.launcher}
                </span>
              </div>
            </button>
          ))}
        </div>
      )}
    </div>
  );

  const loadoutPane = (
    <div className="mobile-pane">
      {activeGame ? (
        <>
          <div className="mobile-pane-header">
            <div>
              <h3 className="mobile-pane-title">{activeGame.title}</h3>
              <span className="muted">
                {activeMods.filter((m) => m.enabled).length} / {activeMods.length} mods enabled
              </span>
            </div>
            {conn.allow_remote_launch && (
              <div className="mobile-launch-actions">
                <button
                  type="button"
                  className="primary"
                  disabled={!isOnline || busyAction !== null}
                  onClick={() =>
                    void runRemote("launch", () => api.mobileLaunchGame(activeGame.id, true))
                  }
                >
                  <FontAwesomeIcon icon={faPlay} /> Launch
                </button>
                <button
                  type="button"
                  disabled={!isOnline || busyAction !== null}
                  onClick={() =>
                    void runRemote("close", () => api.mobileCloseGame(activeGame.id))
                  }
                >
                  <FontAwesomeIcon icon={faStop} />
                </button>
              </div>
            )}
          </div>

          {activeMods.length === 0 ? (
            <p className="muted">No staged mods for this game.</p>
          ) : (
            <div className="mobile-card-list">
              {activeMods.map((mod, idx) => (
                <div key={mod.id} className="mobile-mod-row">
                  <label className="mobile-mod-toggle">
                    <input
                      type="checkbox"
                      checked={mod.enabled}
                      disabled={!isOnline || busyAction !== null}
                      onChange={() => void toggleMod(mod)}
                    />
                    <div className="mobile-card-body">
                      <strong>{mod.name}</strong>
                      <span className="muted">
                        #{idx + 1}
                        {mod.version ? ` · v${mod.version}` : ""}
                      </span>
                    </div>
                  </label>
                  <div className="mobile-order-btns">
                    <button
                      type="button"
                      disabled={!isOnline || idx === 0 || busyAction !== null}
                      aria-label={`Move ${mod.name} up`}
                      onClick={() => void moveMod(idx, -1)}
                    >
                      <FontAwesomeIcon icon={faArrowUp} />
                    </button>
                    <button
                      type="button"
                      disabled={
                        !isOnline || idx === activeMods.length - 1 || busyAction !== null
                      }
                      aria-label={`Move ${mod.name} down`}
                      onClick={() => void moveMod(idx, 1)}
                    >
                      <FontAwesomeIcon icon={faArrowDown} />
                    </button>
                  </div>
                </div>
              ))}
            </div>
          )}
        </>
      ) : (
        <p className="muted">Select a game to view and edit its loadout.</p>
      )}
    </div>
  );

  return (
    <div className={`mobile-app nav-${layout.navMode} posture-${layout.posture}`}>
      <header className="mobile-header">
        <div className="mobile-brand">
          <img src={appLogo} alt="" className="mobile-brand-logo" />
          <strong>Emperor</strong>
        </div>
        <MobileStatusBar
          state={conn}
          syncing={syncing}
          onSyncNow={() => void syncNow(false)}
        />
      </header>

      {banner && (
        <div
          className={`mobile-banner ${banner.kind}`}
          role={banner.kind === "err" ? "alert" : "status"}
        >
          <span>{banner.text}</span>
          <button
            type="button"
            className="mobile-banner-close"
            onClick={() => setBanner(null)}
            aria-label="Dismiss"
          >
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </div>
      )}

      <div className="mobile-body">
        {layout.navMode !== "bottom" && (
          <nav className="mobile-rail" aria-label="Main navigation">
            {navItems.map((item) => (
              <button
                key={item.id}
                type="button"
                className={`mobile-nav-btn${tab === item.id ? " active" : ""}`}
                onClick={() => setTab(item.id)}
              >
                <FontAwesomeIcon icon={item.icon} />
                <span>{item.label}</span>
              </button>
            ))}
          </nav>
        )}

        <main className="mobile-main">
          {tab === "library" && (
            <div className={layout.isDualPane ? "dual-pane-workspace" : "mobile-stack"}>
              {gamesPane}
              {loadoutPane}
            </div>
          )}

          {tab === "collections" && (
            <div className={layout.isDualPane ? "dual-pane-workspace" : "mobile-stack"}>
              <div className="mobile-pane">
                <h3 className="mobile-pane-title">
                  Installed Collections {activeGame ? `· ${activeGame.title}` : ""}
                </h3>
                {activeCollections.length === 0 ? (
                  <p className="muted">No installed collections for this game.</p>
                ) : (
                  <div className="mobile-card-list">
                    {activeCollections.map((col) => {
                      const colMods = activeMods.filter((m) =>
                        (m.collection_ids ?? []).includes(col.id),
                      );
                      const allEnabled =
                        colMods.length > 0 && colMods.every((m) => m.enabled);
                      return (
                        <div key={col.id} className="mobile-mod-row">
                          <div className="mobile-card-body">
                            <strong>{col.name}</strong>
                            <span className="muted">{colMods.length} mods</span>
                          </div>
                          <button
                            type="button"
                            className={allEnabled ? "" : "primary"}
                            disabled={!isOnline || busyAction !== null}
                            onClick={() => void toggleCollection(col.id, !allEnabled)}
                          >
                            {allEnabled ? "Disable" : "Enable"}
                          </button>
                        </div>
                      );
                    })}
                  </div>
                )}
              </div>

              <div className="mobile-pane">
                <h3 className="mobile-pane-title">Import Share Code on PC</h3>
                <form className="mobile-form" onSubmit={(e) => void importShareCode(e)}>
                  <input
                    type="text"
                    value={shareCodeInput}
                    onChange={(e) => setShareCodeInput(e.target.value)}
                    placeholder="#emperor1..."
                    disabled={!isOnline || !activeGame}
                  />
                  <button
                    type="submit"
                    className="primary"
                    disabled={!isOnline || !activeGame || !shareCodeInput.trim()}
                  >
                    Queue Collection Import
                  </button>
                </form>

                <h3 className="mobile-pane-title" style={{ marginTop: "1.25rem" }}>
                  Saved Collections ({snapshot?.saved_collections.length ?? 0})
                </h3>
                {(snapshot?.saved_collections ?? []).length === 0 ? (
                  <p className="muted">No saved collections synced.</p>
                ) : (
                  <div className="mobile-card-list">
                    {(snapshot?.saved_collections ?? []).map((sc) => (
                      <div key={sc.id} className="mobile-mod-row">
                        <div className="mobile-card-body">
                          <strong>{sc.name}</strong>
                          <span className="muted">{sc.mod_count} mods</span>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>
            </div>
          )}

          {tab === "downloads" && (
            <div className={layout.isDualPane ? "dual-pane-workspace" : "mobile-stack"}>
              <div className="mobile-pane">
                <div className="mobile-pane-header">
                  <h3 className="mobile-pane-title">Desktop Downloads ({downloads.length})</h3>
                  {downloads.length > 0 && (
                    <button
                      type="button"
                      disabled={!isOnline || busyAction !== null}
                      onClick={() =>
                        void runRemote("clear-recent", () =>
                          api.mobileControlDownload({ action: "clear_recent" }),
                        )
                      }
                    >
                      Clear Recent
                    </button>
                  )}
                </div>
                {downloads.length === 0 ? (
                  <p className="muted">No active or recent downloads on desktop.</p>
                ) : (
                  <div className="mobile-card-list">
                    {downloads.map((dl) => (
                      <div key={dl.id} className="mobile-mod-row">
                        <div className="mobile-card-body">
                          <strong>{dl.label}</strong>
                          <span className="muted">
                            {dl.status}
                            {dl.bytes_total
                              ? ` · ${Math.round((dl.bytes_downloaded / dl.bytes_total) * 100)}%`
                              : ""}
                          </span>
                        </div>
                        <div className="mobile-order-btns">
                          {dl.status === "downloading" && (
                            <button
                              type="button"
                              disabled={!isOnline}
                              aria-label="Pause"
                              onClick={() =>
                                void runRemote(`pause-${dl.id}`, () =>
                                  api.mobileControlDownload({ action: "pause", id: dl.id }),
                                )
                              }
                            >
                              <FontAwesomeIcon icon={faPause} />
                            </button>
                          )}
                          {dl.status === "paused" && (
                            <button
                              type="button"
                              disabled={!isOnline}
                              aria-label="Resume"
                              onClick={() =>
                                void runRemote(`resume-${dl.id}`, () =>
                                  api.mobileControlDownload({ action: "resume", id: dl.id }),
                                )
                              }
                            >
                              <FontAwesomeIcon icon={faPlay} />
                            </button>
                          )}
                          {(dl.status === "failed" || dl.status === "cancelled") && (
                            <button
                              type="button"
                              disabled={!isOnline}
                              aria-label="Retry"
                              onClick={() =>
                                void runRemote(`retry-${dl.id}`, () =>
                                  api.mobileControlDownload({ action: "restart", id: dl.id }),
                                )
                              }
                            >
                              <FontAwesomeIcon icon={faRotateRight} />
                            </button>
                          )}
                          <button
                            type="button"
                            disabled={!isOnline}
                            aria-label="Remove"
                            onClick={() =>
                              void runRemote(`rm-${dl.id}`, () =>
                                api.mobileControlDownload({
                                  action:
                                    dl.status === "downloading" || dl.status === "queued"
                                      ? "cancel"
                                      : "remove",
                                  id: dl.id,
                                }),
                              )
                            }
                          >
                            <FontAwesomeIcon icon={faTrashCan} />
                          </button>
                        </div>
                      </div>
                    ))}
                  </div>
                )}
              </div>

              <div className="mobile-pane">
                <h3 className="mobile-pane-title">Queue Thunderstore Mod on PC</h3>
                <form className="mobile-form" onSubmit={(e) => void queueThunderstore(e)}>
                  <input
                    type="text"
                    value={tsCommunity}
                    onChange={(e) => setTsCommunity(e.target.value)}
                    placeholder={
                      activeGame?.thunderstore_community
                        ? `Community (${activeGame.thunderstore_community})`
                        : "Community (e.g. lethal-company)"
                    }
                    disabled={!isOnline || !activeGame}
                  />
                  <input
                    type="text"
                    value={tsNamespace}
                    onChange={(e) => setTsNamespace(e.target.value)}
                    placeholder="Author / Namespace (e.g. BepInEx)"
                    disabled={!isOnline || !activeGame}
                  />
                  <input
                    type="text"
                    value={tsName}
                    onChange={(e) => setTsName(e.target.value)}
                    placeholder="Package Name (e.g. BepInExPack)"
                    disabled={!isOnline || !activeGame}
                  />
                  <button
                    type="submit"
                    className="primary"
                    disabled={!isOnline || !activeGame || !tsNamespace.trim() || !tsName.trim()}
                  >
                    Queue Download on Desktop
                  </button>
                </form>
              </div>
            </div>
          )}

          {tab === "friends" && (
            <div className="mobile-pane">
              <h3 className="mobile-pane-title">
                Friends ({friends?.friends.length ?? 0})
              </h3>
              {!friends || friends.friends.length === 0 ? (
                <p className="muted">No friends synced from desktop.</p>
              ) : (
                <div className="mobile-card-list">
                  {friends.friends.map((f) => (
                    <div key={f.peer_id} className="mobile-mod-row">
                      <div className="mobile-card-body">
                        <strong>{f.display_name}</strong>
                        <span className="muted">
                          {f.online ? "Online" : "Offline"}
                        </span>
                      </div>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {tab === "settings" && (
            <div className="mobile-stack">
              <div className="mobile-pane">
                <h3 className="mobile-pane-title">Paired Desktop</h3>
                <p>
                  <strong>{conn.desktop_name ?? "Desktop"}</strong>
                </p>
                <p className="muted">
                  {conn.last_synced_at
                    ? `Last synced ${new Date(conn.last_synced_at).toLocaleString()}`
                    : "Not yet synced"}
                </p>
                <div className="mobile-settings-actions">
                  <button
                    type="button"
                    className="primary"
                    disabled={syncing}
                    onClick={() => void syncNow(false)}
                  >
                    <FontAwesomeIcon icon={faRotateRight} spin={syncing} /> Sync Now
                  </button>
                  <button type="button" className="danger" onClick={() => void unpair()}>
                    Unpair Desktop
                  </button>
                </div>
              </div>

              <div className="mobile-pane">
                <h3 className="mobile-pane-title">Synced Accounts</h3>
                <p className="muted">
                  Nexus Mods:{" "}
                  <strong>
                    {snapshot?.account.nexus_connected
                      ? snapshot.account.nexus_user?.name ?? "Connected"
                      : "Not connected"}
                  </strong>
                </p>
                <p className="muted">
                  mod.io:{" "}
                  <strong>
                    {snapshot?.account.modio_connected ? "Connected" : "Not connected"}
                  </strong>
                </p>
              </div>

              <div className="mobile-pane">
                <h3 className="mobile-pane-title">App Updates</h3>
                <p className="muted">
                  Emperor Mod Manager v{appInfo.version} (Android)
                </p>
                {updateMsg && <p className="note">{updateMsg}</p>}
                <div className="mobile-settings-actions">
                  <button
                    type="button"
                    disabled={checkingUpdate || installingUpdate}
                    onClick={() => void checkUpdates()}
                  >
                    {checkingUpdate ? "Checking…" : "Check for Updates"}
                  </button>
                  {updateStatus?.update_available && updateStatus.asset_name && (
                    <button
                      type="button"
                      className="primary"
                      disabled={installingUpdate}
                      onClick={() => void installUpdate()}
                    >
                      {installingUpdate
                        ? "Installing…"
                        : `Install v${updateStatus.latest_version}`}
                    </button>
                  )}
                </div>
              </div>
            </div>
          )}
        </main>
      </div>

      {layout.navMode === "bottom" && (
        <nav className="mobile-bottom-nav" aria-label="Main navigation">
          {navItems.map((item) => (
            <button
              key={item.id}
              type="button"
              className={`mobile-nav-btn${tab === item.id ? " active" : ""}`}
              onClick={() => setTab(item.id)}
            >
              <FontAwesomeIcon icon={item.icon} />
              <span>{item.label}</span>
            </button>
          ))}
        </nav>
      )}
    </div>
  );
}
