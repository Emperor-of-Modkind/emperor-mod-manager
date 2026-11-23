import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faRotateRight } from "@fortawesome/free-solid-svg-icons";
import type { MobileConnectionState } from "./types";

type MobileStatusBarProps = {
  state: MobileConnectionState;
  syncing?: boolean;
  onSyncNow?: () => void;
};

export function MobileStatusBar({
  state,
  syncing = false,
  onSyncNow,
}: MobileStatusBarProps) {
  if (!state.paired) return null;

  const label =
    state.mode === "lan_direct"
      ? `Connected · LAN (${state.desktop_name ?? "Desktop"})`
      : state.mode === "iroh_relay"
        ? `Connected · Relay (${state.desktop_name ?? "Desktop"})`
        : state.mode === "connecting"
          ? "Connecting to desktop…"
          : "Offline · Cached";

  return (
    <div
      className={`mobile-status-pill mode-${state.mode}`}
      role="status"
      aria-live="polite"
    >
      <span className="mobile-status-dot" aria-hidden="true" />
      <span className="mobile-status-label">{label}</span>
      {onSyncNow && (
        <button
          type="button"
          className="mobile-status-sync-btn"
          onClick={onSyncNow}
          disabled={syncing}
          aria-label="Sync with desktop now"
          title="Sync now"
        >
          <FontAwesomeIcon icon={faRotateRight} spin={syncing} />
        </button>
      )}
    </div>
  );
}
