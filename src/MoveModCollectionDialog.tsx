import { useMemo, useState } from "react";
import { Modal } from "./Modal";
import type { InstalledCollection, StagedMod } from "./types";

function sameCollectionSelection(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false;
  const setA = new Set(a);
  if (setA.size !== b.length) return false;
  return b.every((id) => setA.has(id));
}

export function MoveModCollectionDialog({
  mod,
  collections,
  busy,
  onSave,
  onClose,
}: {
  mod: StagedMod;
  collections: InstalledCollection[];
  busy: boolean;
  onSave: (collectionIds: string[]) => Promise<void> | void;
  onClose: () => void;
}) {
  const initialIds = useMemo(() => {
    const valid = new Set(collections.map((c) => c.id));
    return (mod.collection_ids ?? []).filter((id) => valid.has(id));
  }, [mod.collection_ids, collections]);

  const initialIdSet = useMemo(() => new Set(initialIds), [initialIds]);

  const [selectedIds, setSelectedIds] = useState<string[]>(initialIds);

  const unchanged = sameCollectionSelection(selectedIds, initialIds);

  function toggleCollection(id: string) {
    const col = collections.find((c) => c.id === id);
    if (col?.friend_sync) {
      return;
    }
    setSelectedIds((prev) =>
      prev.includes(id) ? prev.filter((item) => item !== id) : [...prev, id],
    );
  }

  return (
    <Modal
      className="modal-move-collection"
      title="Move between collections"
      subtitle={mod.name}
      onClose={onClose}
      footer={
        <div className="modal-foot-actions">
          <button
            type="button"
            className="linkish"
            disabled={busy}
            onClick={onClose}
          >
            Cancel
          </button>
          <button
            type="button"
            className="accent"
            disabled={busy || unchanged}
            onClick={() => void onSave(selectedIds)}
          >
            {busy ? "Saving…" : "Save"}
          </button>
        </div>
      }
    >
      <p className="note">
        Choose which installed collections include this mod. Uncheck all to keep
        it as a standalone mod.
      </p>
      <div className="move-collection-list" role="group" aria-label="Installed collections">
        {collections.map((col) => {
          const checked = selectedIds.includes(col.id);
          const isSynced = Boolean(col.friend_sync);
          const isAlreadyMember = initialIdSet.has(col.id);
          return (
            <label
              key={col.id}
              className={[
                "move-collection-option",
                checked ? "is-selected" : "",
                isSynced ? "is-disabled" : "",
              ]
                .filter(Boolean)
                .join(" ")}
              title={
                isSynced
                  ? isAlreadyMember
                    ? "Membership in this synced collection is managed by its author"
                    : "Mods cannot be added to a synced collection"
                  : undefined
              }
            >
              <input
                type="checkbox"
                checked={checked}
                disabled={busy || isSynced}
                onChange={() => toggleCollection(col.id)}
              />
              <span className="move-collection-option-name">{col.name}</span>
              {isSynced && (
                <span className="installed-collection-sync-pill">Synced</span>
              )}
              <span className="move-collection-option-meta">
                {col.mod_ids.length} mod{col.mod_ids.length === 1 ? "" : "s"}
              </span>
            </label>
          );
        })}
      </div>
    </Modal>
  );
}
