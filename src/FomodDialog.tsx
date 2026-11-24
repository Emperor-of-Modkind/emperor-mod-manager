import { useEffect, useMemo, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";

import { api } from "./api";
import { Modal } from "./Modal";
import type {
  FomodGroupType,
  FomodGroupView,
  FomodInstallerView,
  FomodPendingReview,
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
  const allUpdateReviews = mods.every((mod) => Boolean(mod.fomod_pending));
  const label = allUpdateReviews
    ? mods.length === 1
      ? "1 mod has new installer options"
      : `${mods.length} mods have new installer options`
    : mods.length === 1
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

function groupIsActionable(group: FomodGroupView) {
  if (group.type === "SelectAll") return false;
  let usable = 0;
  let choosable = 0;
  for (const p of group.plugins) {
    if (!pluginDisabled(p)) {
      usable += 1;
      if (!pluginLockedOn(p)) choosable += 1;
    }
  }
  if (choosable === 0) return false;
  if (group.type === "SelectExactlyOne" || group.type === "SelectAtLeastOne") {
    return usable > 1;
  }
  return true;
}

function groupMatchesPending(
  group: FomodGroupView,
  pending: FomodPendingReview | null | undefined,
): boolean {
  if (!pending || !groupIsActionable(group)) return false;
  if (
    pending.new_group_ids.includes(group.id) ||
    pending.invalidated_group_ids.includes(group.id)
  ) {
    return true;
  }
  return group.plugins.some(
    (p) =>
      pending.new_plugin_ids.includes(p.id) &&
      !pluginDisabled(p) &&
      !pluginLockedOn(p),
  );
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
  const [showOnlyNew, setShowOnlyNew] = useState(false);
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
        setShowOnlyNew(Boolean(next?.pending_review));
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

  const pendingReview = view?.pending_review ?? null;

  const visibleSteps = useMemo(
    () => (view?.steps ?? []).filter((s) => s.visible),
    [view],
  );

  const filteredSteps = useMemo(() => {
    if (!showOnlyNew || !pendingReview) return visibleSteps;
    const matched = visibleSteps.filter((step) =>
      step.groups.some((group) => groupMatchesPending(group, pendingReview)),
    );
    return matched.length > 0 ? matched : visibleSteps;
  }, [visibleSteps, showOnlyNew, pendingReview]);

  const clampedStepIndex = Math.min(stepIndex, Math.max(filteredSteps.length - 1, 0));
  const current = filteredSteps[clampedStepIndex];
  const isLast = clampedStepIndex >= filteredSteps.length - 1;
  const canFinish = Boolean(view && selection && !view.unsupported_scripted);

  const displayedGroups = useMemo(() => {
    if (!current) return [];
    if (!showOnlyNew || !pendingReview) return current.groups;
    const matched = current.groups.filter((group) =>
      groupMatchesPending(group, pendingReview),
    );
    return matched.length > 0 ? matched : current.groups;
  }, [current, showOnlyNew, pendingReview]);

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
          const nextPending = refreshed.pending_review ?? null;
          const nextFiltered =
            showOnlyNew && nextPending
              ? (() => {
                  const matched = nextVisible.filter((s) =>
                    s.groups.some((g) => groupMatchesPending(g, nextPending)),
                  );
                  return matched.length > 0 ? matched : nextVisible;
                })()
              : nextVisible;
          const idx = nextFiltered.findIndex((s) => s.id === currentStepId);
          if (idx >= 0) {
            setStepIndex(idx);
          } else {
            setStepIndex((i) => Math.min(i, Math.max(nextFiltered.length - 1, 0)));
          }
        }
      })
      .catch(() => {
        /* keep local draft selection if preview refresh fails */
      });
  };

  const toggleShowOnlyNew = () => {
    const nextOnlyNew = !showOnlyNew;
    setShowOnlyNew(nextOnlyNew);
    if (current) {
      const nextList =
        nextOnlyNew && pendingReview
          ? (() => {
              const matched = visibleSteps.filter((s) =>
                s.groups.some((g) => groupMatchesPending(g, pendingReview)),
              );
              return matched.length > 0 ? matched : visibleSteps;
            })()
          : visibleSteps;
      const idx = nextList.findIndex((s) => s.id === current.id);
      setStepIndex(idx >= 0 ? idx : 0);
    } else {
      setStepIndex(0);
    }
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
        {filteredSteps.length > 1
          ? `Step ${Math.min(clampedStepIndex + 1, filteredSteps.length)} of ${filteredSteps.length}. Deploy again to apply.`
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
        {isLast || filteredSteps.length === 0 ? (
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
      {view && selection && !view.unsupported_scripted && pendingReview ? (
        <div className="fomod-update-notice">
          <small>
            {showOnlyNew
              ? "Previous choices were kept. Showing only new or changed settings."
              : "Previous choices were kept. Showing all installer settings."}
          </small>
          <button
            type="button"
            className="linkish"
            onClick={toggleShowOnlyNew}
            disabled={saving}
          >
            {showOnlyNew ? "Show all settings" : "Show only new settings"}
          </button>
        </div>
      ) : null}
      {view && selection && !view.unsupported_scripted && current ? (
        <div className="fomod-step">
          {displayedGroups.map((group) => {
            const ids = selectedIn(group.id);
            const exact = isExact(group.type);
            const isNewGroup = Boolean(pendingReview?.new_group_ids.includes(group.id));
            const isInvalidated = Boolean(
              pendingReview?.invalidated_group_ids.includes(group.id),
            );
            return (
              <section key={group.id} className="fomod-group">
                <h3>
                  {group.name}
                  {isNewGroup ? <span className="fomod-badge-new">New</span> : null}
                </h3>
                {isInvalidated ? (
                  <p className="field-hint">
                    Previous choice was removed — choose an option.
                  </p>
                ) : null}
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
                    const isNewPlugin =
                      !isNewGroup &&
                      Boolean(pendingReview?.new_plugin_ids.includes(plugin.id));
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
                            <strong>
                              {plugin.name}
                              {isNewPlugin ? (
                                <span className="fomod-badge-new">New</span>
                              ) : null}
                            </strong>
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
      {view && selection && !view.unsupported_scripted && filteredSteps.length === 0 ? (
        <p>No installer steps to show. You can finish with the required files only.</p>
      ) : null}
    </Modal>
  );
}
