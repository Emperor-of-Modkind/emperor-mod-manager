import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faSliders, faXmark } from "@fortawesome/free-solid-svg-icons";

import { api } from "./api";
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
  onClose: () => void;
  onSaved: () => Promise<void> | void;
};

function isSingleChoice(type: FomodGroupType): boolean {
  return type === "SelectExactlyOne" || type === "SelectAtMostOne";
}

function RemoteFomodImage({ path, alt }: { path: string | null; alt: string }) {
  const [src, setSrc] = useState<string | null>(null);
  useEffect(() => {
    if (!path) return;
    let cancelled = false;
    api
      .mobileGetFomodImage(path)
      .then((res) => {
        if (!cancelled && res?.data_url) setSrc(res.data_url);
      })
      .catch(() => {});
    return () => {
      cancelled = true;
    };
  }, [path]);
  if (!src) return null;
  return <img className="mod-option-image" src={src} alt={alt} loading="lazy" />;
}

export function MobileFomodModal({ gameId, mod, onClose, onSaved }: Props) {
  const [view, setView] = useState<FomodInstallerView | null>(null);
  const [selection, setSelection] = useState<FomodSelection>({ selected: {} });
  const [stepIdx, setStepIdx] = useState(0);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    api
      .mobileGetFomodInstaller(gameId, mod.id)
      .then((data) => {
        if (cancelled) return;
        setView(data);
        setSelection(data.selection ?? { selected: {} });
        setStepIdx(0);
      })
      .catch((e) => {
        if (!cancelled) setError(String(e));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [gameId, mod.id]);

  const visibleSteps = (view?.steps ?? []).filter((s) => s.visible);
  const currentStep = visibleSteps[Math.min(stepIdx, Math.max(0, visibleSteps.length - 1))];

  const togglePlugin = (group: FomodGroupView, plugin: FomodPluginView) => {
    if (plugin.plugin_type === "Required" || plugin.plugin_type === "NotUsable") return;
    setSelection((prev) => {
      const current = prev.selected[group.id] ?? [];
      let next: string[];
      if (isSingleChoice(group.type)) {
        if (group.type === "SelectAtMostOne" && current.includes(plugin.id)) {
          next = [];
        } else {
          next = [plugin.id];
        }
      } else {
        next = current.includes(plugin.id)
          ? current.filter((id) => id !== plugin.id)
          : [...current, plugin.id];
      }
      return {
        selected: {
          ...prev.selected,
          [group.id]: next,
        },
      };
    });
  };

  const handleSave = async () => {
    setSaving(true);
    setError(null);
    try {
      await api.mobileSetFomodSelection(gameId, mod.id, selection);
      await onSaved();
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="mobile-modal-backdrop" role="dialog" aria-modal="true" aria-label={`Configure ${mod.name}`}>
      <div className="mobile-modal-sheet">
        <div className="mobile-modal-header">
          <div className="mobile-modal-title">
            <FontAwesomeIcon icon={faSliders} />
            <div>
              <h3>{view?.module_name || mod.name}</h3>
              {visibleSteps.length > 1 ? (
                <span className="muted">
                  Step {Math.min(stepIdx + 1, visibleSteps.length)} of {visibleSteps.length}
                  {currentStep?.name ? ` · ${currentStep.name}` : ""}
                </span>
              ) : null}
            </div>
          </div>
          <button
            type="button"
            className="ghost icon-btn"
            aria-label="Close FOMOD installer"
            onClick={onClose}
          >
            <FontAwesomeIcon icon={faXmark} />
          </button>
        </div>

        <div className="mobile-modal-body">
          {loading ? (
            <p className="muted">Loading FOMOD installer from desktop…</p>
          ) : error ? (
            <p className="muted">{error}</p>
          ) : view?.unsupported_scripted ? (
            <p className="muted">
              This mod uses a scripted FOMOD installer which cannot be configured remotely.
            </p>
          ) : !currentStep ? (
            <p className="muted">No configurable installer steps found.</p>
          ) : (
            <div className="mobile-fomod-groups">
              {currentStep.groups.map((group) => {
                const chosen = selection.selected[group.id] ?? [];
                const single = isSingleChoice(group.type);
                return (
                  <section key={group.id} className="mobile-fomod-group">
                    <h4>{group.name}</h4>
                    <div className="mobile-fomod-options">
                      {group.plugins.map((plugin) => {
                        const checked =
                          plugin.plugin_type === "Required" || chosen.includes(plugin.id);
                        const disabled =
                          plugin.plugin_type === "Required" ||
                          plugin.plugin_type === "NotUsable";
                        return (
                          <label
                            key={plugin.id}
                            className={`mobile-fomod-option ${checked ? "selected" : ""} ${disabled ? "disabled" : ""}`}
                          >
                            <input
                              type={single ? "radio" : "checkbox"}
                              name={`fomod-${group.id}`}
                              checked={checked}
                              disabled={disabled}
                              onChange={() => togglePlugin(group, plugin)}
                            />
                            <div className="mobile-fomod-option-copy">
                              <strong>
                                {plugin.name}
                                {plugin.plugin_type === "Required" ? " (Required)" : ""}
                                {plugin.plugin_type === "Recommended" ? " (Recommended)" : ""}
                              </strong>
                              {plugin.description ? (
                                <p className="muted">{plugin.description}</p>
                              ) : null}
                              <RemoteFomodImage path={plugin.image} alt={plugin.name} />
                            </div>
                          </label>
                        );
                      })}
                    </div>
                  </section>
                );
              })}
            </div>
          )}
        </div>

        <div className="mobile-modal-footer">
          {stepIdx > 0 ? (
            <button
              type="button"
              className="secondary"
              onClick={() => setStepIdx((s) => Math.max(0, s - 1))}
            >
              Previous
            </button>
          ) : (
            <button type="button" className="ghost" onClick={onClose}>
              Cancel
            </button>
          )}
          {stepIdx < visibleSteps.length - 1 ? (
            <button
              type="button"
              className="primary"
              onClick={() => setStepIdx((s) => Math.min(visibleSteps.length - 1, s + 1))}
            >
              Next
            </button>
          ) : (
            <button
              type="button"
              className="primary"
              disabled={loading || saving || Boolean(view?.unsupported_scripted)}
              onClick={() => void handleSave()}
            >
              {saving ? "Saving…" : "Save Choices"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
