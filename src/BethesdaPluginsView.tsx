import { useCallback, useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faArrowDown,
  faArrowUp,
  faArrowDownWideShort,
  faArrowsRotate,
  faRotateLeft,
  faTriangleExclamation,
  faCircleExclamation,
  faCircleInfo,
  faBroom,
} from "@fortawesome/free-solid-svg-icons";
import { api } from "./api";
import type {
  BethesdaPluginsState,
  PluginEntry,
  SavedPluginEntry,
} from "./types";

const LOOT_PLUGIN_IDS = new Set([
  "skyrimspecialedition",
  "skyrim",
  "fallout4",
  "starfield",
  "newvegas",
  "fallout3",
  "oblivion",
  "oblivionremastered",
  "morrowind",
]);

export function isBethesdaLootPluginId(pluginId?: string | null): boolean {
  return Boolean(pluginId && LOOT_PLUGIN_IDS.has(pluginId));
}

type Props = {
  gameId: string;
  onNotify: (
    kind: "info" | "warn" | "error",
    message: string,
    details?: string[],
  ) => void;
  onPluginCountChange?: (count: number) => void;
};

function formatMasterlistDate(iso: string | null): string {
  if (!iso) return "Not downloaded yet";
  const d = new Date(iso);
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleString(undefined, {
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  });
}

export function BethesdaPluginsView({
  gameId,
  onNotify,
  onPluginCountChange,
}: Props) {
  const [state, setState] = useState<BethesdaPluginsState | null>(null);
  const [loading, setLoading] = useState(true);
  const [sorting, setSorting] = useState(false);
  const [updatingMasterlist, setUpdatingMasterlist] = useState(false);
  const [undoOrder, setUndoOrder] = useState<SavedPluginEntry[] | null>(null);
  const [dragIndex, setDragIndex] = useState<number | null>(null);
  const [dropIndex, setDropIndex] = useState<number | null>(null);

  const loadPlugins = useCallback(async () => {
    setLoading(true);
    try {
      const next = await api.getBethesdaPlugins(gameId);
      setState(next);
      onPluginCountChange?.(next.plugins.length);
    } catch (e) {
      onNotify("error", `Failed to load plugins: ${String(e)}`);
    } finally {
      setLoading(false);
    }
  }, [gameId, onNotify, onPluginCountChange]);

  useEffect(() => {
    setUndoOrder(null);
    void loadPlugins();
  }, [loadPlugins]);

  const commitOrder = useCallback(
    async (entries: SavedPluginEntry[]) => {
      try {
        const next = await api.setBethesdaPluginOrder(gameId, entries);
        setState(next);
        onPluginCountChange?.(next.plugins.length);
      } catch (e) {
        onNotify("error", `Failed to update plugin order: ${String(e)}`);
      }
    },
    [gameId, onNotify, onPluginCountChange],
  );

  const handleSortLoot = async () => {
    setSorting(true);
    try {
      const res = await api.sortBethesdaPlugins(gameId);
      setState(res.state);
      onPluginCountChange?.(res.state.plugins.length);
      if (res.moved_count > 0) {
        setUndoOrder(res.previous_order);
        onNotify(
          res.warnings.length > 0 ? "warn" : "info",
          `LOOT sorted ${res.state.plugins.length} plugins (${res.moved_count} moved)${res.used_masterlist ? "" : " — header-only"}.`,
          res.warnings.length > 0 ? res.warnings : undefined,
        );
      } else {
        setUndoOrder(null);
        onNotify(
          res.warnings.length > 0 ? "warn" : "info",
          "LOOT: Plugin load order is already optimal.",
          res.warnings.length > 0 ? res.warnings : undefined,
        );
      }
    } catch (e) {
      onNotify("error", `LOOT sort failed: ${String(e)}`);
    } finally {
      setSorting(false);
    }
  };

  const handleUndoLoot = async () => {
    if (!undoOrder) return;
    const target = undoOrder;
    setUndoOrder(null);
    await commitOrder(target);
    onNotify("info", "Restored previous plugin load order.");
  };

  const handleUpdateMasterlist = async () => {
    setUpdatingMasterlist(true);
    try {
      const status = await api.updateLootMasterlist(gameId);
      onNotify(
        "info",
        `Updated LOOT masterlist (${status.repo}).`,
      );
      await loadPlugins();
    } catch (e) {
      onNotify("error", `Failed to update LOOT masterlist: ${String(e)}`);
    } finally {
      setUpdatingMasterlist(false);
    }
  };

  const togglePlugin = (plugin: PluginEntry) => {
    if (!state || plugin.is_vanilla) return;
    const next: SavedPluginEntry[] = state.plugins.map((p) => ({
      name: p.name,
      enabled: p.name === plugin.name ? !p.enabled : p.enabled,
    }));
    void commitOrder(next);
  };

  const movePlugin = (index: number, delta: -1 | 1) => {
    if (!state) return;
    const target = index + delta;
    if (target < 0 || target >= state.plugins.length) return;
    const next = [...state.plugins];
    const [moved] = next.splice(index, 1);
    next.splice(target, 0, moved);
    void commitOrder(next.map((p) => ({ name: p.name, enabled: p.enabled })));
  };

  const handleDrop = (targetIdx: number) => {
    if (dragIndex === null || dragIndex === targetIdx || !state) {
      setDragIndex(null);
      setDropIndex(null);
      return;
    }
    const next = [...state.plugins];
    const [moved] = next.splice(dragIndex, 1);
    next.splice(targetIdx, 0, moved);
    setDragIndex(null);
    setDropIndex(null);
    void commitOrder(next.map((p) => ({ name: p.name, enabled: p.enabled })));
  };

  const plugins = state?.plugins ?? [];
  const enabledCount = plugins.filter((p) => p.enabled).length;

  return (
    <div className="bethesda-plugins-view">
      <div className="mods-toolbar">
        <div className="mods-toolbar-actions">
          <div className="mods-toolbar-actions-left">
            <button
              type="button"
              className="accent"
              disabled={sorting || loading || plugins.length === 0}
              onClick={() => void handleSortLoot()}
            >
              <FontAwesomeIcon icon={faArrowDownWideShort} />{" "}
              {sorting ? "Sorting with LOOT…" : "Sort with LOOT"}
            </button>
            <button
              type="button"
              disabled={updatingMasterlist || sorting}
              onClick={() => void handleUpdateMasterlist()}
              title={
                state?.masterlist.cached
                  ? `LOOT masterlist (${state.masterlist.repo}) — last updated ${formatMasterlistDate(state.masterlist.updated_at)}`
                  : "Download official LOOT masterlist from GitHub"
              }
            >
              <FontAwesomeIcon
                icon={faArrowsRotate}
                spin={updatingMasterlist}
              />{" "}
              {updatingMasterlist ? "Updating masterlist…" : "Update Masterlist"}
            </button>
            {undoOrder && (
              <button
                type="button"
                onClick={() => void handleUndoLoot()}
              >
                <FontAwesomeIcon icon={faRotateLeft} /> Undo LOOT sort
              </button>
            )}
          </div>
          <div className="mods-toolbar-actions-right">
            <span className="plugins-masterlist-meta">
              {enabledCount}/{plugins.length} enabled
              {state?.masterlist.cached
                ? ` · Masterlist: ${formatMasterlistDate(state.masterlist.updated_at)}`
                : " · Masterlist not cached"}
            </span>
          </div>
        </div>
      </div>

      {loading && plugins.length === 0 ? (
        <div className="empty-state">Scanning plugins…</div>
      ) : plugins.length === 0 ? (
        <div className="empty-state">
          No `.esm`, `.esp`, or `.esl` plugins found in `Data/` or enabled staged
          mods.
        </div>
      ) : (
        <div className="plugins-list">
          {plugins.map((plugin, idx) => {
            const hasErrors =
              plugin.missing_masters.length > 0 ||
              plugin.messages.some((m) => m.kind === "error");
            const hasWarnings =
              plugin.dirty_info.length > 0 ||
              plugin.messages.some((m) => m.kind === "warn");

            return (
              <div
                key={plugin.name}
                draggable
                onDragStart={() => setDragIndex(idx)}
                onDragOver={(e) => {
                  e.preventDefault();
                  setDropIndex(idx);
                }}
                onDragEnd={() => {
                  setDragIndex(null);
                  setDropIndex(null);
                }}
                onDrop={(e) => {
                  e.preventDefault();
                  handleDrop(idx);
                }}
                className={[
                  "plugin-row",
                  !plugin.enabled ? "disabled" : "",
                  hasErrors ? "has-error" : hasWarnings ? "has-warn" : "",
                  dropIndex === idx && dragIndex !== idx ? "drop-target" : "",
                ]
                  .filter(Boolean)
                  .join(" ")}
              >
                <div className="plugin-row-main">
                  <input
                    type="checkbox"
                    checked={plugin.enabled}
                    disabled={plugin.is_vanilla}
                    title={
                      plugin.is_vanilla
                        ? "Required base game master"
                        : plugin.enabled
                          ? "Disable plugin"
                          : "Enable plugin"
                    }
                    onChange={() => togglePlugin(plugin)}
                  />
                  <span className="plugin-order-index">
                    {String(idx + 1).padStart(2, "0")}
                  </span>
                  <div className="plugin-info">
                    <div className="plugin-title-line">
                      <span className="plugin-name">{plugin.name}</span>
                      {plugin.is_master && (
                        <span className="plugin-badge esm">ESM</span>
                      )}
                      {plugin.is_light && (
                        <span className="plugin-badge esl">ESL</span>
                      )}
                      {plugin.is_medium && (
                        <span className="plugin-badge mid">MID</span>
                      )}
                      {plugin.is_vanilla ? (
                        <span className="plugin-badge vanilla">Base Game</span>
                      ) : plugin.owner_mod_name ? (
                        <span className="plugin-owner-tag">
                          {plugin.owner_mod_name}
                        </span>
                      ) : null}
                    </div>
                    {plugin.missing_masters.length > 0 && (
                      <div className="plugin-note error">
                        <FontAwesomeIcon icon={faCircleExclamation} /> Missing
                        master(s): {plugin.missing_masters.join(", ")}
                      </div>
                    )}
                    {plugin.dirty_info.map((note, i) => (
                      <div key={`dirty-${i}`} className="plugin-note warn">
                        <FontAwesomeIcon icon={faBroom} /> {note}
                      </div>
                    ))}
                    {plugin.messages.map((msg, i) => (
                      <div
                        key={`msg-${i}`}
                        className={`plugin-note ${msg.kind}`}
                      >
                        <FontAwesomeIcon
                          icon={
                            msg.kind === "error"
                              ? faCircleExclamation
                              : msg.kind === "warn"
                                ? faTriangleExclamation
                                : faCircleInfo
                          }
                        />{" "}
                        {msg.text}
                      </div>
                    ))}
                  </div>
                  <div className="plugin-row-actions">
                    <button
                      type="button"
                      className="icon-btn"
                      aria-label={`Move ${plugin.name} up`}
                      disabled={idx === 0}
                      onClick={() => movePlugin(idx, -1)}
                    >
                      <FontAwesomeIcon icon={faArrowUp} />
                    </button>
                    <button
                      type="button"
                      className="icon-btn"
                      aria-label={`Move ${plugin.name} down`}
                      disabled={idx === plugins.length - 1}
                      onClick={() => movePlugin(idx, 1)}
                    >
                      <FontAwesomeIcon icon={faArrowDown} />
                    </button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
