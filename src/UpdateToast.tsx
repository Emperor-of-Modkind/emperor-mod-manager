import { Reveal } from "./motion";
import { Throbber } from "./Throbber";

type UpdateToastProps = {
  show: boolean;
  version: string;
  progress: string | null;
  canInstall: boolean;
  checking: boolean;
  installing: boolean;
  onUpdate: () => void;
  onOpenRelease: () => void;
  onLater: () => void;
};

type CheckingUpdatesToastProps = {
  show: boolean;
  /** Staged mods already loaded for the active game. Hidden until that list exists. */
  stagedCount: number | null;
};

/** Quiet status while staged-mod update detection is still running. */
export function CheckingUpdatesToast({ show, stagedCount }: CheckingUpdatesToastProps) {
  const detail =
    stagedCount != null && stagedCount > 0
      ? `${stagedCount} staged mod${stagedCount === 1 ? "" : "s"}`
      : null;
  return (
    <Reveal show={show} className="update-toast-slot">
      <div className="update-toast" role="status">
        <div className="update-toast-row">
          <Throbber />
          <div className="update-toast-copy">
            <p className="update-toast-title">Checking for mod updates…</p>
            {detail ? <p className="update-toast-progress">{detail}</p> : null}
          </div>
        </div>
      </div>
    </Reveal>
  );
}

/** Small offer to install a newer app release, or open the release page. */
export function UpdateToast({
  show,
  version,
  progress,
  canInstall,
  checking,
  installing,
  onUpdate,
  onOpenRelease,
  onLater,
}: UpdateToastProps) {
  const busy = checking || installing || progress != null;
  return (
    <Reveal show={show} className="update-toast-slot">
      <div className="update-toast" role="status">
        <div className={busy ? "update-toast-row" : undefined}>
          {busy ? <Throbber /> : null}
          <div className="update-toast-copy">
            <p className="update-toast-title">Update available</p>
            <p className="update-toast-version">v{version}</p>
            {progress ? <p className="update-toast-progress">{progress}</p> : null}
          </div>
        </div>
        <div className="update-toast-actions">
          {canInstall ? (
            <button type="button" disabled={checking || installing} onClick={onUpdate}>
              {installing ? "Updating…" : "Update"}
            </button>
          ) : (
            <button type="button" disabled={installing} onClick={onOpenRelease}>
              Open release
            </button>
          )}
          <button
            type="button"
            className="update-toast-later"
            disabled={installing}
            onClick={onLater}
          >
            Later
          </button>
        </div>
      </div>
    </Reveal>
  );
}
