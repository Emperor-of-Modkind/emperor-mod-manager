import { useEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowLeft,
  faCloudArrowDown,
  faCopy,
  faFloppyDisk,
  faLayerGroup,
  faRotate,
  faShieldHalved,
  faTrash,
} from "@fortawesome/free-solid-svg-icons";

import { api } from "./api";
import { SettingsToggle } from "./SettingsSection";
import { SourceMark } from "./SourceMark";
import type {
  CollectionIntegrityReport,
  InstalledCollection,
  ManagedGame,
  SavedCollectionDetail,
  SavedCollectionEntry,
  ShareModEntry,
  ShareModMetadataHit,
  StagedMod,
  StagedModUpdate,
} from "./types";

function shareModKey(m: ShareModEntry): string | null {
  if (m.s === "nexus") {
    const domain = m.domain?.trim().toLowerCase();
    if (!domain || !m.mod_id) return null;
    return `nexus:${domain}:${m.mod_id}`;
  }
  if (m.s === "thunderstore") {
    const c = m.community?.trim().toLowerCase();
    const ns = m.namespace?.trim().toLowerCase();
    const n = m.name?.trim().toLowerCase();
    if (!c || !ns || !n) return null;
    return `thunderstore:${c}:${ns}:${n}`;
  }
  if (m.s === "modio") {
    if (!m.game_id || !m.mod_id) return null;
    return `modio:${m.game_id}:${m.mod_id}`;
  }
  return null;
}

type Props = {
  activeGame: ManagedGame | null;
  installedCollections: InstalledCollection[];
  savedCollections: SavedCollectionEntry[];
  mods: StagedMod[];
  updatesByModId: Record<string, StagedModUpdate>;
  online: boolean;
  onRefresh: () => Promise<void>;
  onToggleCollection: (col: InstalledCollection, enabled: boolean) => Promise<void>;
  onToggleMod: (mod: StagedMod, enabled: boolean) => Promise<void>;
  onCheckUpdates: () => Promise<void>;
  onUpdateAll: () => Promise<void>;
  onImportShareCode: (code: string) => Promise<void>;
  onNotify: (kind: "info" | "warn" | "error", msg: string) => void;
  onPushBack?: (handler: () => void) => void;
};

type Selection =
  | { kind: "installed"; id: string }
  | { kind: "saved"; id: string }
  | null;

export function MobileCollectionsView({
  activeGame,
  installedCollections,
  savedCollections,
  mods,
  updatesByModId,
  online,
  onRefresh,
  onToggleCollection,
  onToggleMod,
  onCheckUpdates,
  onUpdateAll,
  onImportShareCode,
  onNotify,
  onPushBack,
}: Props) {
  const [selected, setSelected] = useState<Selection>(null);
  const [savedDetail, setSavedDetail] = useState<SavedCollectionDetail | null>(null);
  const [savedModMeta, setSavedModMeta] = useState<Record<string, ShareModMetadataHit>>({});
  const [savedLoading, setSavedLoading] = useState(false);
  const [integrity, setIntegrity] = useState<CollectionIntegrityReport | null>(null);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [shareCodeInput, setShareCodeInput] = useState("");
  const onNotifyRef = useRef(onNotify);
  onNotifyRef.current = onNotify;

  const openSelection = (next: Exclude<Selection, null>) => {
    setSelected(next);
    onPushBack?.(() => setSelected(null));
  };

  const selectedKey = selected ? `${selected.kind}:${selected.id}` : null;
  const savedId = selected?.kind === "saved" ? selected.id : null;

  useEffect(() => {
    setIntegrity(null);
  }, [selectedKey]);

  useEffect(() => {
    if (!savedId) {
      setSavedDetail(null);
      setSavedModMeta({});
      setSavedLoading(false);
      return;
    }
    let cancelled = false;
    setSavedLoading(true);
    setSavedDetail(null);
    setSavedModMeta({});
    api
      .mobileGetSavedCollection(savedId)
      .then((detail) => {
        if (cancelled) return;
        setSavedDetail(detail);
        if (detail.mods.length > 0) {
          void api
            .mobileCatalogRpc<ShareModMetadataHit[]>("resolve_share_mod_metadata", {
              mods: detail.mods,
            })
            .then((hits) => {
              if (cancelled || !Array.isArray(hits)) return;
              const map: Record<string, ShareModMetadataHit> = {};
              for (const h of hits) map[h.key] = h;
              setSavedModMeta(map);
            })
            .catch(() => {});
        }
      })
      .catch((e) => {
        if (!cancelled)
          onNotifyRef.current("error", `Failed to load saved collection: ${String(e)}`);
      })
      .finally(() => {
        if (!cancelled) setSavedLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [savedId]);

  const installedSelected =
    selected?.kind === "installed"
      ? installedCollections.find((c) => c.id === selected.id) ?? null
      : null;

  const savedSelected =
    selected?.kind === "saved"
      ? savedCollections.find((c) => c.id === selected.id) ?? null
      : null;

  const updateCount = Object.keys(updatesByModId).length;

  const runAction = async (label: string, fn: () => Promise<void>) => {
    setBusyAction(label);
    try {
      await fn();
    } catch (e) {
      onNotify("error", String(e));
    } finally {
      setBusyAction(null);
    }
  };

  if (installedSelected && activeGame) {
    const colMods = mods.filter((m) => (m.collection_ids ?? []).includes(installedSelected.id));
    const colEnabled = colMods.length > 0 && colMods.some((m) => m.enabled);
    const colUpdateCount = colMods.filter((m) => Boolean(updatesByModId[m.id])).length;

    return (
      <div className="mobile-detail-view">
        <button
          type="button"
          className="ghost mobile-back-btn"
          onClick={() => setSelected(null)}
        >
          <FontAwesomeIcon icon={faArrowLeft} /> Back to Collections
        </button>

        <div className="mobile-surface-card mobile-detail-header-card">
          <div className="mobile-detail-title-row">
            <div>
              <h2>{installedSelected.name}</h2>
              <p className="muted">
                {colMods.length} mods · {activeGame.title}
                {installedSelected.revision ? ` · Rev ${installedSelected.revision}` : ""}
                {installedSelected.friend_sync ? " · Synced" : ""}
              </p>
            </div>
            <SettingsToggle
              checked={colEnabled}
              disabled={!online}
              onChange={(checked) => void onToggleCollection(installedSelected, checked)}
            >
              {colEnabled ? "Enabled" : "Disabled"}
            </SettingsToggle>
          </div>

          <div className="mobile-action-bar">
            <button
              type="button"
              className="secondary"
              disabled={!online || busyAction !== null}
              onClick={() =>
                void runAction("updates", async () => {
                  await onCheckUpdates();
                })
              }
            >
              <FontAwesomeIcon icon={faRotate} spin={busyAction === "updates"} /> Check Updates
            </button>
            {colUpdateCount > 0 || updateCount > 0 ? (
              <button
                type="button"
                className="primary"
                disabled={!online || busyAction !== null}
                onClick={() =>
                  void runAction("update-all", async () => {
                    await onUpdateAll();
                  })
                }
              >
                <FontAwesomeIcon icon={faCloudArrowDown} /> Update All ({colUpdateCount || updateCount})
              </button>
            ) : null}
            <button
              type="button"
              className="secondary"
              disabled={!online || busyAction !== null}
              onClick={() =>
                void runAction("integrity", async () => {
                  const report = await api.mobileCheckCollectionIntegrity(
                    activeGame.id,
                    installedSelected.id,
                  );
                  setIntegrity(report);
                  onNotify("info", `Integrity check finished for ${installedSelected.name}`);
                })
              }
            >
              <FontAwesomeIcon icon={faShieldHalved} /> Check Integrity
            </button>
            {!installedSelected.friend_sync ? (
              <button
                type="button"
                className="secondary"
                disabled={!online || busyAction !== null}
                onClick={() =>
                  void runAction("save", async () => {
                    await api.mobileSaveCollectionChanges(activeGame.id, installedSelected.id);
                    await onRefresh();
                    onNotify("info", `Saved changes to ${installedSelected.name}`);
                  })
                }
              >
                <FontAwesomeIcon icon={faFloppyDisk} /> Save Changes
              </button>
            ) : null}
          </div>

          {!installedSelected.friend_sync ? (
            <div className="mobile-detail-toggles">
              <SettingsToggle
                checked={Boolean(installedSelected.auto_save)}
                disabled={!online || busyAction !== null}
                onChange={(checked) =>
                  void runAction("autosave", async () => {
                    await api.mobileSetCollectionAutoSave(
                      activeGame.id,
                      installedSelected.id,
                      checked,
                    );
                    await onRefresh();
                  })
                }
              >
                Auto-save changes to this collection
              </SettingsToggle>
            </div>
          ) : null}

          {integrity ? (
            <div className="mobile-integrity-box">
              <strong>Integrity Report</strong>
              <p className="muted">
                {integrity.checked_count} mods checked
                {integrity.issues.length > 0
                  ? ` · ${integrity.issues.length} issue${integrity.issues.length === 1 ? "" : "s"} found`
                  : " · All intact"}
              </p>
            </div>
          ) : null}
        </div>

        <section className="mobile-detail-section">
          <h3>Mods in Collection ({colMods.length})</h3>
          {colMods.length === 0 ? (
            <p className="muted">No staged mods are currently linked to this collection.</p>
          ) : (
            <div className="mobile-mod-list">
              {colMods.map((mod) => {
                const upd = updatesByModId[mod.id];
                return (
                  <div
                    key={mod.id}
                    className={`mobile-mod-row ${mod.enabled ? "" : "disabled"}`}
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
                        <FontAwesomeIcon icon={faLayerGroup} />
                      </div>
                    )}
                    <div className="mobile-mod-info">
                      <strong>{mod.name}</strong>
                      <span className="muted">
                        {mod.version ? `v${mod.version}` : "Local"}
                        {upd?.available_version ? ` → v${upd.available_version}` : ""}
                      </span>
                    </div>
                    <SettingsToggle
                      checked={mod.enabled}
                      disabled={!online}
                      onChange={(checked) => void onToggleMod(mod, checked)}
                    >
                      {mod.enabled ? "On" : "Off"}
                    </SettingsToggle>
                  </div>
                );
              })}
            </div>
          )}
        </section>

        <div className="mobile-danger-row">
          <button
            type="button"
            className="ghost danger"
            disabled={!online || busyAction !== null}
            onClick={() =>
              void runAction("uninstall", async () => {
                await api.mobileUninstallCollection(activeGame.id, installedSelected.id, false);
                await onRefresh();
                setSelected(null);
                onNotify("info", `Removed collection ${installedSelected.name}`);
              })
            }
          >
            <FontAwesomeIcon icon={faTrash} /> Uninstall Collection
          </button>
        </div>
      </div>
    );
  }

  if (savedSelected) {
    return (
      <div className="mobile-detail-view">
        <button
          type="button"
          className="ghost mobile-back-btn"
          onClick={() => setSelected(null)}
        >
          <FontAwesomeIcon icon={faArrowLeft} /> Back to Collections
        </button>

        <div className="mobile-surface-card mobile-detail-header-card">
          <h2>{savedSelected.name}</h2>
          <p className="muted">
            {savedSelected.game.title || savedSelected.game.plugin_id} · {savedSelected.mod_count} mods
          </p>
          <div className="mobile-action-bar">
            <button
              type="button"
              className="primary"
              disabled={!online || !activeGame || busyAction !== null}
              onClick={() =>
                void runAction("import-saved", async () => {
                  await onImportShareCode(savedSelected.code);
                })
              }
            >
              <FontAwesomeIcon icon={faCloudArrowDown} /> Install on PC
            </button>
            <button
              type="button"
              className="secondary"
              onClick={() => {
                void navigator.clipboard?.writeText(savedSelected.code);
                onNotify("info", "Share code copied to clipboard");
              }}
            >
              <FontAwesomeIcon icon={faCopy} /> Copy Share Code
            </button>
          </div>
        </div>

        <section className="mobile-detail-section">
          <h3>Saved Mod List</h3>
          {savedLoading ? (
            <p className="muted">Loading mod list from desktop…</p>
          ) : savedDetail && savedDetail.mods.length > 0 ? (
            <div className="mobile-mod-list">
              {savedDetail.mods.map((m, idx) => {
                const key = shareModKey(m);
                const meta = key ? savedModMeta[key] : undefined;
                const stagedMatch = mods.find(
                  (sm) =>
                    (m.s === "nexus" && m.mod_id && sm.nexus_mod_id === m.mod_id) ||
                    (m.display_name && sm.name === m.display_name),
                );
                const art = meta?.picture_url ?? stagedMatch?.picture_url ?? null;
                return (
                  <div key={`${m.display_name || m.name || idx}:${idx}`} className="mobile-mod-row">
                    {art ? (
                      <img
                        className="mobile-mod-thumb"
                        src={art}
                        alt={m.display_name || m.name || "Mod"}
                        loading="lazy"
                      />
                    ) : (
                      <div className="mobile-mod-thumb mobile-mod-thumb-placeholder">
                        <SourceMark source={m.s} />
                      </div>
                    )}
                    <div className="mobile-mod-info">
                      <strong>{m.display_name || m.name || `Mod #${idx + 1}`}</strong>
                      <span className="muted">
                        {meta?.author ? `${meta.author} · ` : ""}
                        {m.s}
                        {m.version ? ` · v${m.version}` : ""}
                      </span>
                    </div>
                    <SourceMark source={m.s} />
                  </div>
                );
              })}
            </div>
          ) : (
            <p className="muted">No mod entries found in this saved collection.</p>
          )}
        </section>
      </div>
    );
  }

  return (
    <div className="mobile-collections-pane">
      <div className="mobile-toolbar-row">
        <h2>Collections</h2>
        <div className="mobile-action-bar">
          <button
            type="button"
            className="secondary"
            disabled={!online || !activeGame || busyAction !== null}
            onClick={() =>
              void runAction("updates", async () => {
                await onCheckUpdates();
              })
            }
          >
            <FontAwesomeIcon icon={faRotate} spin={busyAction === "updates"} /> Check Updates
          </button>
          {updateCount > 0 ? (
            <button
              type="button"
              className="primary"
              disabled={!online || !activeGame || busyAction !== null}
              onClick={() =>
                void runAction("update-all", async () => {
                  await onUpdateAll();
                })
              }
            >
              <FontAwesomeIcon icon={faCloudArrowDown} /> Update All ({updateCount})
            </button>
          ) : null}
        </div>
      </div>

      {activeGame ? (
        <form
          className="mobile-share-import-row"
          onSubmit={(e) => {
            e.preventDefault();
            const code = shareCodeInput.trim();
            if (!code) return;
            void runAction("import-code", async () => {
              await onImportShareCode(code);
              setShareCodeInput("");
            });
          }}
        >
          <input
            type="text"
            value={shareCodeInput}
            onChange={(e) => setShareCodeInput(e.target.value)}
            placeholder="Paste Emperor share code or Nexus slug…"
          />
          <button
            type="submit"
            className="primary"
            disabled={!online || !shareCodeInput.trim() || busyAction !== null}
          >
            Import
          </button>
        </form>
      ) : null}

      <section className="mobile-section">
        <h3>Installed {activeGame ? `(${activeGame.title})` : ""}</h3>
        {installedCollections.length === 0 ? (
          <p className="muted">No installed collections for the active game.</p>
        ) : (
          <div className="mobile-card-list">
            {installedCollections.map((col) => {
              const colMods = mods.filter((m) => (m.collection_ids ?? []).includes(col.id));
              const colEnabled = colMods.length > 0 && colMods.some((m) => m.enabled);
              return (
                <div key={col.id} className="mobile-collection-card">
                  <button
                    type="button"
                    className="mobile-collection-card-main"
                    onClick={() => openSelection({ kind: "installed", id: col.id })}
                  >
                    <strong>{col.name}</strong>
                    <span className="muted">
                      {colMods.length} mods
                      {col.revision ? ` · Rev ${col.revision}` : ""}
                      {col.friend_sync ? " · Synced" : col.auto_save ? " · Auto-save" : ""}
                    </span>
                  </button>
                  <SettingsToggle
                    checked={colEnabled}
                    disabled={!online}
                    onChange={(checked) => void onToggleCollection(col, checked)}
                  >
                    {colEnabled ? "On" : "Off"}
                  </SettingsToggle>
                </div>
              );
            })}
          </div>
        )}
      </section>

      <section className="mobile-section">
        <h3>Saved Collections ({savedCollections.length})</h3>
        {savedCollections.length === 0 ? (
          <p className="muted">No saved collections on your desktop yet.</p>
        ) : (
          <div className="mobile-card-list">
            {savedCollections.map((sc) => (
              <button
                key={sc.id}
                type="button"
                className="mobile-collection-card mobile-collection-card-btn"
                onClick={() => openSelection({ kind: "saved", id: sc.id })}
              >
                <div className="mobile-collection-card-main">
                  <strong>{sc.name}</strong>
                  <span className="muted">
                    {sc.game.title || sc.game.plugin_id} · {sc.mod_count} mods
                  </span>
                </div>
                <span className="pill">View</span>
              </button>
            ))}
          </div>
        )}
      </section>
    </div>
  );
}
