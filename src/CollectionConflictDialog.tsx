import { useMemo, useState } from "react";
import { Modal } from "./Modal";
import type {
  CollectionConflictPlan,
  CollectionResolutionChoices,
  ConflictAction,
  ModSource,
  ModVersionConflict,
} from "./types";

type CollectionConflictDialogProps = {
  plan: CollectionConflictPlan;
  onConfirm: (choices: CollectionResolutionChoices) => void;
  onCancel: () => void;
};

function sourceLabel(source: ModSource): string {
  switch (source) {
    case "nexus":
      return "Nexus";
    case "thunderstore":
      return "Thunderstore";
    case "modio":
      return "mod.io";
  }
}

function versionText(
  version?: string | null,
  fileName?: string | null,
  fileId?: number | null,
): string {
  const parts: string[] = [];
  if (version && version.trim()) {
    parts.push(`v${version.trim().replace(/^v/i, "")}`);
  } else if (fileId && fileId > 0) {
    parts.push(`file #${fileId}`);
  } else {
    parts.push("unknown version");
  }
  if (fileName && fileName.trim()) {
    parts.push(`(${fileName.trim()})`);
  }
  return parts.join(" ");
}

function initialChoices(plan: CollectionConflictPlan): {
  disableIds: Set<string>;
  modActions: Record<string, ConflictAction>;
} {
  const modActions: Record<string, ConflictAction> = {};
  const disableIds = new Set<string>();
  for (const c of plan.conflicts) {
    modActions[c.conflictKey] = c.recommendedAction;
    if (c.recommendedAction === "disable_other_collections") {
      for (const col of c.conflictingCollections) {
        disableIds.add(col.id);
      }
    }
  }
  return { disableIds, modActions };
}

function actionHint(
  conflict: ModVersionConflict,
  action: ConflictAction,
  isSyncedTarget: boolean,
): string | null {
  switch (action) {
    case "disable_other_collections":
      return "Disables conflicting collection(s) while keeping their old versions staged on disk, and installs this collection's version.";
    case "disable_old_and_install_new":
      return "Disables only the installed version of this mod and installs the incoming version alongside it.";
    case "replace_existing":
      if (conflict.conflictingCollections.length > 0) {
        const names = conflict.conflictingCollections.map((c) => c.name).join(", ");
        return `Replaces the installed version in-place (this also updates ${names}).`;
      }
      return "Replaces the standalone installed version in-place.";
    case "keep_existing":
      return isSyncedTarget
        ? "Skips downloading and uses your installed version instead (differs from the synced collection's version)."
        : "Skips downloading and uses your currently installed version.";
    case "keep_both_enabled":
      return "Installs the incoming file alongside the existing file with both enabled (use for companion patches/addons).";
  }
}

export function CollectionConflictDialog({
  plan,
  onConfirm,
  onCancel,
}: CollectionConflictDialogProps) {
  const init = useMemo(() => initialChoices(plan), [plan]);
  const [disableIds, setDisableIds] = useState<Set<string>>(() => new Set(init.disableIds));
  const [modActions, setModActions] = useState<Record<string, ConflictAction>>(
    () => ({ ...init.modActions }),
  );

  function toggleDisableCollection(collectionId: string) {
    const summary = plan.conflictingCollections.find((c) => c.id === collectionId);
    const nextDisabled = new Set(disableIds);
    const enabling = !nextDisabled.has(collectionId);
    if (enabling) {
      nextDisabled.add(collectionId);
    } else {
      nextDisabled.delete(collectionId);
    }
    setDisableIds(nextDisabled);

    if (summary) {
      setModActions((prev) => {
        const next = { ...prev };
        for (const key of summary.conflictKeys) {
          if (enabling) {
            next[key] = "disable_other_collections";
          } else if (next[key] === "disable_other_collections") {
            const conflict = plan.conflicts.find((c) => c.conflictKey === key);
            const stillHasDisabledCol = conflict?.conflictingCollections.some((c) =>
              nextDisabled.has(c.id),
            );
            if (!stillHasDisabledCol) {
              next[key] = "disable_old_and_install_new";
            }
          }
        }
        return next;
      });
    }
  }

  function setModAction(conflict: ModVersionConflict, action: ConflictAction) {
    setModActions((prev) => ({ ...prev, [conflict.conflictKey]: action }));
    if (action === "disable_other_collections") {
      setDisableIds((prev) => {
        const next = new Set(prev);
        for (const col of conflict.conflictingCollections) {
          next.add(col.id);
        }
        return next;
      });
    }
  }

  function applyBulkAction(action: ConflictAction) {
    const nextActions: Record<string, ConflictAction> = {};
    for (const c of plan.conflicts) {
      nextActions[c.conflictKey] = action;
    }
    setModActions(nextActions);
    if (action !== "disable_other_collections") {
      setDisableIds(new Set());
    }
  }

  const downloadsFromConflicts = plan.conflicts.filter((c) => {
    const action = modActions[c.conflictKey] ?? c.recommendedAction;
    return action !== "keep_existing";
  }).length;
  const totalDownloads =
    plan.installCount + plan.repairCount + plan.autoUpdateCount + downloadsFromConflicts;

  function handleConfirm() {
    onConfirm({
      disableCollectionIds: Array.from(disableIds),
      modActions,
    });
  }

  return (
    <Modal
      title={`Resolve version conflicts`}
      subtitle={plan.targetCollectionName}
      kicker={plan.isSyncedTarget ? "Synced Collection" : "Collection Install"}
      className="conflict-dialog"
      onClose={onCancel}
      footer={
        <>
          <button type="button" onClick={onCancel}>
            Cancel
          </button>
          <button type="button" className="accent" onClick={handleConfirm}>
            {totalDownloads > 0
              ? `Continue (${totalDownloads} to download)`
              : "Continue"}
          </button>
        </>
      }
    >
      <div className="conflict-summary-bar">
        {plan.exactMatchCount > 0 && (
          <span className="conflict-stat is-reused">
            {plan.exactMatchCount} already installed (will reuse)
          </span>
        )}
        {plan.installCount + plan.repairCount + plan.autoUpdateCount > 0 && (
          <span className="conflict-stat">
            {plan.installCount + plan.repairCount + plan.autoUpdateCount} new / updated
          </span>
        )}
        <span className="conflict-stat is-conflict">
          {plan.conflicts.length} version conflict
          {plan.conflicts.length === 1 ? "" : "s"}
        </span>
      </div>

      {plan.conflictingCollections.length > 0 && (
        <div className="conflict-collections-box">
          <div className="conflict-section-head">
            <strong>Conflicting collections</strong>
            <small>
              Disable another collection to keep its mods staged while switching to{" "}
              {plan.targetCollectionName}.
            </small>
          </div>
          <ul className="conflict-collection-list">
            {plan.conflictingCollections.map((col) => {
              const checked = disableIds.has(col.id);
              return (
                <li key={col.id} className="conflict-collection-row">
                  <label className="conflict-collection-toggle">
                    <input
                      type="checkbox"
                      checked={checked}
                      onChange={() => toggleDisableCollection(col.id)}
                    />
                    <span className="conflict-collection-meta">
                      <span className="conflict-collection-name">
                        Disable <strong>{col.name}</strong>
                      </span>
                      <span className="conflict-pill">
                        {col.isSynced ? "Synced" : "Non-synced"}
                      </span>
                      <small>
                        {col.conflictCount} conflicting mod
                        {col.conflictCount === 1 ? "" : "s"} · {col.enabledModCount} enabled
                      </small>
                    </span>
                  </label>
                </li>
              );
            })}
          </ul>
        </div>
      )}

      {plan.conflicts.length > 1 && (
        <div className="conflict-bulk-bar">
          <span>Apply to all conflicts:</span>
          <div className="conflict-bulk-actions">
            <button
              type="button"
              onClick={() => applyBulkAction("disable_old_and_install_new")}
            >
              Disable all old &amp; install new
            </button>
            <button
              type="button"
              onClick={() => applyBulkAction("replace_existing")}
            >
              Replace all old
            </button>
            <button
              type="button"
              onClick={() => applyBulkAction("keep_existing")}
            >
              Keep all existing
            </button>
          </div>
        </div>
      )}

      <ul className="conflict-mod-list">
        {plan.conflicts.map((conflict) => {
          const action = modActions[conflict.conflictKey] ?? conflict.recommendedAction;
          const hint = actionHint(conflict, action, plan.isSyncedTarget);
          const hasOtherCols = conflict.conflictingCollections.length > 0;
          return (
            <li key={conflict.conflictKey} className="conflict-mod-row">
              <div className="conflict-mod-head">
                <div className="conflict-mod-title">
                  <strong>{conflict.incomingName}</strong>
                  <span className="conflict-source-pill">
                    {sourceLabel(conflict.source)}
                  </span>
                  {conflict.existingIndependent && (
                    <span className="conflict-pill">Standalone</span>
                  )}
                  {conflict.conflictingCollections.map((col) => (
                    <span key={col.id} className="conflict-pill is-collection">
                      {col.name}
                    </span>
                  ))}
                </div>
                <div className="conflict-version-diff">
                  <span className="conflict-ver-old">
                    Installed:{" "}
                    {versionText(
                      conflict.existingVersion,
                      conflict.existingFileName,
                      conflict.existingFileId,
                    )}
                  </span>
                  <span aria-hidden="true">→</span>
                  <span className="conflict-ver-new">
                    Incoming:{" "}
                    {versionText(
                      conflict.incomingVersion,
                      conflict.incomingFileName,
                      conflict.incomingFileId,
                    )}
                  </span>
                </div>
              </div>

              <div className="conflict-mod-control">
                <select
                  aria-label={`Resolution for ${conflict.incomingName}`}
                  value={action}
                  onChange={(e) =>
                    setModAction(conflict, e.target.value as ConflictAction)
                  }
                >
                  {hasOtherCols && (
                    <option value="disable_other_collections">
                      Disable conflicting collection(s) &amp; install new
                    </option>
                  )}
                  <option value="disable_old_and_install_new">
                    Disable old version &amp; install new
                  </option>
                  <option value="replace_existing">
                    Replace old version with new
                  </option>
                  <option value="keep_existing">
                    Keep existing version (skip download)
                  </option>
                  {conflict.source === "nexus" && (
                    <option value="keep_both_enabled">
                      Keep both enabled (companion file)
                    </option>
                  )}
                </select>
                {hint && <small className="conflict-action-hint">{hint}</small>}
              </div>
            </li>
          );
        })}
      </ul>
    </Modal>
  );
}
