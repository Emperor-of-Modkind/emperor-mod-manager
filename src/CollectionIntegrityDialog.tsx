import { useMemo, useState } from "react";
import { Modal } from "./Modal";
import { SourceMark } from "./SourceMark";
import type {
  CollectionIntegrityReport,
  CollectionModIssue,
  CollectionModIssueKind,
} from "./types";

type CollectionIntegrityDialogProps = {
  report: CollectionIntegrityReport;
  retrying: boolean;
  onRetry: (issues: CollectionModIssue[]) => void;
  onClose: () => void;
};

export function describeCollectionModIssue(issue: CollectionModIssueKind): string {
  switch (issue.kind) {
    case "missing_from_load_order":
      return "Missing from load order";
    case "missing_staging_dir":
      return "Missing staging folder";
    case "corrupted_staging":
      if (issue.expected_files != null) {
        return `Corrupted (${issue.actual_files}/${issue.expected_files} files)`;
      }
      return issue.actual_files === 0
        ? "Empty staging folder"
        : `Corrupted (${issue.actual_files} files)`;
    case "missing_from_manifest":
      return "Missing from collection";
  }
}

export function CollectionIntegrityDialog({
  report,
  retrying,
  onRetry,
  onClose,
}: CollectionIntegrityDialogProps) {
  const retryableIds = useMemo(
    () => report.issues.filter((i) => i.can_retry).map((i) => i.mod_id),
    [report.issues],
  );
  const [selectedIds, setSelectedIds] = useState<Set<string>>(
    () => new Set(retryableIds),
  );

  const selectedIssues = useMemo(
    () => report.issues.filter((i) => i.can_retry && selectedIds.has(i.mod_id)),
    [report.issues, selectedIds],
  );

  function toggleOne(modId: string) {
    setSelectedIds((prev) => {
      const next = new Set(prev);
      if (next.has(modId)) next.delete(modId);
      else next.add(modId);
      return next;
    });
  }

  return (
    <Modal
      title="Missing or corrupted mods"
      subtitle={`${report.collection_name} — ${report.issues.length} of ${report.checked_count} mod${report.checked_count === 1 ? "" : "s"} need attention`}
      className="collection-integrity-modal"
      onClose={onClose}
      footer={
        <>
          <button type="button" onClick={onClose} disabled={retrying}>
            Close
          </button>
          <button
            type="button"
            className="primary"
            disabled={retrying || selectedIssues.length === 0}
            onClick={() => onRetry(selectedIssues)}
          >
            {retrying
              ? "Retrying…"
              : `Retry selected (${selectedIssues.length})`}
          </button>
        </>
      }
    >
      <p className="note">
        Select the missing or corrupted mods you want to re-download and stage.
      </p>
      {retryableIds.length > 1 && (
        <div className="collection-integrity-bulk">
          <button
            type="button"
            disabled={retrying || selectedIds.size === retryableIds.length}
            onClick={() => setSelectedIds(new Set(retryableIds))}
          >
            Select all
          </button>
          <button
            type="button"
            disabled={retrying || selectedIds.size === 0}
            onClick={() => setSelectedIds(new Set())}
          >
            Select none
          </button>
        </div>
      )}
      <ul className="collection-integrity-list">
        {report.issues.map((issue) => {
          const checked = issue.can_retry && selectedIds.has(issue.mod_id);
          return (
            <li key={issue.mod_id} className="collection-integrity-item">
              <label className="collection-integrity-row">
                <input
                  type="checkbox"
                  checked={checked}
                  disabled={!issue.can_retry || retrying}
                  onChange={() => toggleOne(issue.mod_id)}
                />
                <span className="collection-integrity-info">
                  <span className="collection-integrity-title">
                    <SourceMark source={issue.source} />
                    <strong>{issue.name}</strong>
                    {issue.version ? (
                      <small className="collection-integrity-version">
                        v{issue.version}
                      </small>
                    ) : null}
                  </span>
                  <span className="collection-integrity-status">
                    {describeCollectionModIssue(issue.issue)}
                    {!issue.can_retry
                      ? " · Cannot auto-retry (reinstall manually)"
                      : ""}
                  </span>
                </span>
              </label>
            </li>
          );
        })}
      </ul>
    </Modal>
  );
}
