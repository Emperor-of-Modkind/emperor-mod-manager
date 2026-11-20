import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api } from "./api";
import { Modal } from "./Modal";
import type {
  FomodGroupType,
  FomodGroupView,
  FomodInstallerView,
  FomodPluginView,
  FomodSelection,
  StagedMod,
} from "./types";

type Props = {
  gameId: string;
  mod: StagedMod;
  /** Installer-list position for a Configure or Deploy pass. Omitted for one mod opened from its row. */
  progress?: { current: number; total: number } | null;
  onClose: (reason?: "saved" | "cancel") => void;
  onSaved: (selection: FomodSelection) => void;
};

const FOMOD_BANNER_PREVIEW = 4;

export function FomodConfigureBanner({
  mods,
  onConfigure,
}: {
  mods: StagedMod[];
  onConfigure: () => void;
}) {
  if (mods.length === 0) return null;
  const preview = mods.slice(0, FOMOD_BANNER_PREVIEW);
  const extra = mods.length - preview.length;
  const label =
    mods.length === 1
      ? "1 mod needs installer choices"
      : `${mods.length} mods need installer choices`;
  return (
    <div className="fomod-configure">
      <div>
        <strong>{label}</strong>
        <ul>
          {preview.map((mod) => (
            <li key={mod.id}>{mod.name}</li>
          ))}
          {extra > 0 ? <li>and {extra} more</li> : null}
        </ul>
      </div>
      <button type="button" onClick={onConfigure}>
        Configure
      </button>
    </div>
  );
}

function OptionImage({ path, alt }: { path: string | null; alt: string }) {
  if (!path) return null;
  return <img className="mod-option-image" src={convertFileSrc(path)} alt={alt} loading="lazy" />;
}

function isExact(type: FomodGroupType) {
  return type === "SelectExactlyOne" || type === "SelectAtMostOne";
}

function pluginLockedOn(plugin: FomodPluginView) {
  return plugin.plugin_type === "Required";
}

function pluginDisabled(plugin: FomodPluginView) {
  return plugin.plugin_type === "NotUsable";
}

function reconcileSelectionWithView(
  draft: FomodSelection,
  refreshed: FomodInstallerView,
): FomodSelection {
  const next: Record<string, string[]> = { ...draft.selected };
  for (const step of refreshed.steps) {
    for (const group of step.groups) {
      if (!(group.id in next)) {
        next[group.id] = refreshed.selection.selected[group.id] ?? [];
        continue;
      }
      const allowed = new Set(
        group.plugins
          .filter((p) => !pluginDisabled(p))
          .map((p) => p.id),
      );
      const filtered = (next[group.id] ?? []).filter((id) => allowed.has(id));
      for (const p of group.plugins) {
        if (pluginLockedOn(p) && !filtered.includes(p.id)) {
          filtered.push(p.id);
        }
      }
      if (
        (group.type === "SelectExactlyOne" || group.type === "SelectAtLeastOne") &&
        filtered.length === 0
      ) {
        next[group.id] = refreshed.selection.selected[group.id] ?? [];
      } else if (group.type === "SelectAll") {
        next[group.id] = group.plugins
          .filter((p) => !pluginDisabled(p))
          .map((p) => p.id);
      } else {
        next[group.id] = filtered;
      }
    }
  }
  return { selected: next };
}

export function FomodDialog({ gameId, mod, progress, onClose, onSaved }: Props) {
  const [view, setView] = useState<FomodInstallerView | null>(null);
  const [selection, setSelection] = useState<FomodSelection | null>(null);
  const [stepIndex, setStepIndex] = useState(0);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const refreshReqRef = useRef(0);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  useEffect(() => {
    let cancelled = false;
    refreshReqRef.current += 1;
    setLoading(true);
    api
      .getFomodInstaller(gameId, mod.id)
      .then((next) => {
        if (cancelled) return;
        setView(next);
        setSelection(next?.selection ?? null);
        setStepIndex(0);
        if (!next) {
          setError("This mod no longer has a FOMOD installer.");
        } else if (next.unsupported_scripted) {
          setError("This mod uses a scripted FOMOD installer, which is not supported yet.");
        } else {
          setError(null);
        }
      })
      .catch((e) => !cancelled && setError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [gameId, mod.id]);

  const visibleSteps = useMemo(
    () => (view?.steps ?? []).filter((s) => s.visible),
    [view],
  );

  const clampedStepIndex = Math.min(stepIndex, Math.max(visibleSteps.length - 1, 0));
  const current = visibleSteps[clampedStepIndex];
  const isLast = clampedStepIndex >= visibleSteps.length - 1;
  const canFinish = Boolean(view && selection && !view.unsupported_scripted);

  const selectedIn = (groupId: string) => selection?.selected[groupId] ?? [];

  const setGroupSelection = (group: FomodGroupView, nextIds: string[]) => {
    if (!selection) return;
    const nextSelection: FomodSelection = {
      selected: { ...selection.selected, [group.id]: nextIds },
    };
    setSelection(nextSelection);
    const req = ++refreshReqRef.current;
    const currentStepId = current?.id ?? null;
    void api
      .getFomodInstaller(gameId, mod.id, nextSelection)
      .then((refreshed) => {
        if (!mountedRef.current || req !== refreshReqRef.current || !refreshed) return;
        setView(refreshed);
        setSelection((prev) =>
          prev ? reconcileSelectionWithView(prev, refreshed) : refreshed.selection,
        );
        if (currentStepId) {
          const nextVisible = refreshed.steps.filter((s) => s.visible);
          const idx = nextVisible.findIndex((s) => s.id === currentStepId);
          if (idx >= 0) {
            setStepIndex(idx);
          } else {
            setStepIndex((i) => Math.min(i, Math.max(nextVisible.length - 1, 0)));
          }
        }
      })
      .catch(() => {
        /* keep local draft selection if preview refresh fails */
      });
  };

  const togglePlugin = (group: FomodGroupView, plugin: FomodPluginView) => {
    if (pluginDisabled(plugin) || pluginLockedOn(plugin)) return;
    const currentIds = selectedIn(group.id);
    if (isExact(group.type)) {
      if (group.type === "SelectAtMostOne" && currentIds.includes(plugin.id)) {
        setGroupSelection(group, []);
        return;
      }
      setGroupSelection(group, [plugin.id]);
      return;
    }
    if (group.type === "SelectAll") return;
    const on = currentIds.includes(plugin.id);
    const next = on
      ? currentIds.filter((id) => id !== plugin.id)
      : [...currentIds, plugin.id];
    // Keep required plugins selected.
    for (const p of group.plugins) {
      if (pluginLockedOn(p) && !next.includes(p.id)) next.push(p.id);
    }
    setGroupSelection(group, next);
  };

  const groupValid = (group: FomodGroupView) => {
    const ids = selectedIn(group.id);
    switch (group.type) {
      case "SelectExactlyOne":
        return ids.length === 1;
      case "SelectAtLeastOne":
        return ids.length >= 1;
      case "SelectAtMostOne":
        return ids.length <= 1;
      default:
        return true;
    }
  };

  const stepValid = current ? current.groups.every(groupValid) : true;

  const finish = async () => {
    if (!selection || !canFinish) return;
    setSaving(true);
    setError(null);
    try {
      const saved = await api.setFomodSelection(gameId, mod.id, selection);
      onSaved(saved);
      onClose("saved");
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const footer = (
    <>
      <small className="field-hint">
        {visibleSteps.length > 1
          ? `Step ${Math.min(stepIndex + 1, visibleSteps.length)} of ${visibleSteps.length}. Deploy again to apply.`
          : "Deploy again to apply these choices."}
      </small>
      <div className="modal-foot-actions">
        <button
          type="button"
          className="linkish"
          onClick={() => onClose("cancel")}
          disabled={saving}
        >
          Cancel
        </button>
        {clampedStepIndex > 0 ? (
          <button
            type="button"
            className="linkish"
            onClick={() => setStepIndex(Math.max(0, clampedStepIndex - 1))}
            disabled={saving}
          >
            Back
          </button>
        ) : null}
        {isLast || visibleSteps.length === 0 ? (
          <button type="button" onClick={finish} disabled={saving || !canFinish || !stepValid}>
            {saving ? "Saving…" : "Finish"}
          </button>
        ) : (
          <button
            type="button"
            onClick={() => setStepIndex(clampedStepIndex + 1)}
            disabled={saving || !stepValid}
          >
            Next
          </button>
        )}
      </div>
    </>
  );

  return (
    <Modal
      title={view?.module_name || mod.name}
      subtitle={current?.name ?? "FOMOD installer"}
      kicker={progress ? `${progress.current} of ${progress.total}` : undefined}
      onClose={() => onClose("cancel")}
      footer={view && selection && !view.unsupported_scripted ? footer : undefined}
    >
      {error ? <p className="error">{error}</p> : null}
      {loading ? <p>Reading installer…</p> : null}
      {view && selection && !view.unsupported_scripted && current ? (
        <div className="fomod-step">
          {current.groups.map((group) => {
            const ids = selectedIn(group.id);
            const exact = isExact(group.type);
            return (
              <section key={group.id} className="fomod-group">
                <h3>{group.name}</h3>
                {!groupValid(group) ? (
                  <p className="field-hint">
                    {group.type === "SelectExactlyOne"
                      ? "Choose one option."
                      : group.type === "SelectAtLeastOne"
                        ? "Choose at least one option."
                        : null}
                  </p>
                ) : null}
                <ul className="mod-option-list">
                  {group.plugins.map((plugin) => {
                    const checked = ids.includes(plugin.id);
                    const disabled =
                      pluginDisabled(plugin) ||
                      pluginLockedOn(plugin) ||
                      group.type === "SelectAll";
                    const off = !checked && !pluginLockedOn(plugin);
                    return (
                      <li key={plugin.id} className={off ? "mod-option-off" : undefined}>
                        <label className="check">
                          <input
                            type={exact ? "radio" : "checkbox"}
                            name={exact ? `fomod-${group.id}` : undefined}
                            checked={checked || pluginLockedOn(plugin)}
                            disabled={disabled && !pluginLockedOn(plugin)}
                            onChange={() => togglePlugin(group, plugin)}
                          />
                          <OptionImage path={plugin.image} alt={plugin.name} />
                          <div>
                            <strong>{plugin.name}</strong>
                            {plugin.description ? <small>{plugin.description}</small> : null}
                            {plugin.plugin_type === "Recommended" ? (
                              <small className="field-hint">Recommended</small>
                            ) : null}
                            {plugin.plugin_type === "Required" ? (
                              <small className="field-hint">Required</small>
                            ) : null}
                          </div>
                        </label>
                      </li>
                    );
                  })}
                </ul>
              </section>
            );
          })}
        </div>
      ) : null}
      {view && selection && !view.unsupported_scripted && visibleSteps.length === 0 ? (
        <p>No installer steps to show. You can finish with the required files only.</p>
      ) : null}
    </Modal>
  );
}
