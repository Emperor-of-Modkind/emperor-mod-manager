import { useEffect, useMemo, useState } from "react";

import { api } from "./api";
import { applyIniEdits, isIniFilename, parseIniDocument } from "./iniConfig";
import { Modal } from "./Modal";
import type { ModConfigFile, StagedMod } from "./types";

type Props = {
  gameId: string;
  mod: StagedMod;
  onClose: () => void;
  onSaved: (fileName: string) => void;
};

export function ModConfigDialog({ gameId, mod, onClose, onSaved }: Props) {
  const [files, setFiles] = useState<ModConfigFile[]>([]);
  const [activePath, setActivePath] = useState<string>("");
  const [drafts, setDrafts] = useState<Record<string, string>>({});
  const [viewMode, setViewMode] = useState<"form" | "raw">("form");
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    api
      .listModConfigFiles(gameId, mod.id)
      .then((list) => {
        if (cancelled) return;
        setFiles(list);
        const initialDrafts: Record<string, string> = {};
        for (const f of list) initialDrafts[f.rel_path] = f.content;
        setDrafts(initialDrafts);
        if (list.length > 0) {
          setActivePath(list[0].rel_path);
          setViewMode(isIniFilename(list[0].name) ? "form" : "raw");
        } else {
          setError("No editable configuration files found in this mod.");
        }
      })
      .catch((e) => !cancelled && setError(String(e)))
      .finally(() => !cancelled && setLoading(false));
    return () => {
      cancelled = true;
    };
  }, [gameId, mod.id]);

  const activeFile = useMemo(
    () => files.find((f) => f.rel_path === activePath) ?? files[0] ?? null,
    [files, activePath],
  );

  const activeContent = activeFile ? (drafts[activeFile.rel_path] ?? activeFile.content) : "";
  const isIni = activeFile ? isIniFilename(activeFile.name) : false;
  const sections = useMemo(
    () => (isIni ? parseIniDocument(activeContent) : []),
    [isIni, activeContent],
  );
  const canUseForm = isIni && sections.length > 0;
  const effectiveMode = canUseForm ? viewMode : "raw";

  const isDirty = activeFile
    ? (drafts[activeFile.rel_path] ?? activeFile.content) !== activeFile.content
    : false;

  const updateIniField = (lineIndex: number, value: string) => {
    if (!activeFile) return;
    const next = applyIniEdits(activeContent, { [lineIndex]: value });
    setDrafts((prev) => ({ ...prev, [activeFile.rel_path]: next }));
  };

  const updateRaw = (value: string) => {
    if (!activeFile) return;
    setDrafts((prev) => ({ ...prev, [activeFile.rel_path]: value }));
  };

  const selectFile = (file: ModConfigFile) => {
    setActivePath(file.rel_path);
    setViewMode(isIniFilename(file.name) ? "form" : "raw");
    setError(null);
  };

  const save = async () => {
    if (!activeFile) return;
    setSaving(true);
    setError(null);
    try {
      await api.saveModConfigFile(gameId, mod.id, activeFile.rel_path, activeContent);
      setFiles((prev) =>
        prev.map((f) =>
          f.rel_path === activeFile.rel_path ? { ...f, content: activeContent } : f,
        ),
      );
      onSaved(activeFile.name);
      onClose();
    } catch (e) {
      setError(String(e));
    } finally {
      setSaving(false);
    }
  };

  const footer = (
    <>
      <small className="field-hint">
        Saved to the staged mod; changes apply on your next Launch Modded.
      </small>
      <div className="modal-foot-actions">
        <button type="button" className="linkish" onClick={onClose} disabled={saving}>
          Cancel
        </button>
        <button type="button" onClick={save} disabled={saving || !activeFile || !isDirty}>
          {saving ? "Saving…" : "Save config"}
        </button>
      </div>
    </>
  );

  return (
    <Modal
      title={`${mod.name} — Configuration`}
      subtitle={activeFile ? activeFile.rel_path : "Mod configuration files"}
      onClose={onClose}
      footer={activeFile ? footer : undefined}
    >
      {error ? <p className="error">{error}</p> : null}
      {loading ? <p>Reading configuration files…</p> : null}
      {!loading && activeFile ? (
        <div className="mod-config-editor">
          <div className="mod-config-toolbar">
            {files.length > 1 ? (
              <div className="mod-config-tabs" role="tablist" aria-label="Configuration files">
                {files.map((f) => (
                  <button
                    key={f.rel_path}
                    type="button"
                    role="tab"
                    aria-selected={f.rel_path === activeFile.rel_path}
                    className={
                      f.rel_path === activeFile.rel_path
                        ? "mod-config-tab active"
                        : "mod-config-tab"
                    }
                    onClick={() => selectFile(f)}
                    title={f.rel_path}
                  >
                    {f.name}
                  </button>
                ))}
              </div>
            ) : (
              <span className="mod-config-filename">{activeFile.name}</span>
            )}
            {canUseForm ? (
              <div className="mod-config-mode" role="group" aria-label="Editor view">
                <button
                  type="button"
                  className={effectiveMode === "form" ? "mod-config-tab active" : "mod-config-tab"}
                  onClick={() => setViewMode("form")}
                >
                  Form
                </button>
                <button
                  type="button"
                  className={effectiveMode === "raw" ? "mod-config-tab active" : "mod-config-tab"}
                  onClick={() => setViewMode("raw")}
                >
                  Raw
                </button>
              </div>
            ) : null}
          </div>

          {effectiveMode === "form" ? (
            <div className="mod-config-sections">
              {sections.map((section) => (
                <section key={section.name} className="mod-config-section">
                  <h4>{section.name}</h4>
                  <div className="mod-config-fields">
                    {section.fields.map((field) => {
                      const needsValue = field.requiredWhenEmpty && field.value.trim() === "";
                      return (
                        <label
                          key={field.lineIndex}
                          className={
                            needsValue
                              ? "mod-config-field mod-config-field-required"
                              : "mod-config-field"
                          }
                        >
                          <span className="mod-config-key">
                            <code>{field.key}</code>
                            {needsValue ? (
                              <span className="mod-config-badge">Required</span>
                            ) : null}
                          </span>
                          <input
                            type="text"
                            value={field.value}
                            placeholder={needsValue ? "Enter a value before launching" : ""}
                            onChange={(e) => updateIniField(field.lineIndex, e.target.value)}
                          />
                          {field.comments.length > 0 ? (
                            <small className="mod-config-hint">
                              {field.comments.join(" ")}
                            </small>
                          ) : null}
                        </label>
                      );
                    })}
                  </div>
                </section>
              ))}
            </div>
          ) : (
            <textarea
              className="mod-config-raw"
              value={activeContent}
              onChange={(e) => updateRaw(e.target.value)}
              spellCheck={false}
              rows={18}
            />
          )}
        </div>
      ) : null}
    </Modal>
  );
}
