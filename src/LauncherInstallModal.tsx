import { useEffect, useState } from "react";
import { api } from "./api";
import { Modal } from "./Modal";
import type {
  InstallMethod,
  LauncherInstallOptions,
  LauncherInstallProgress,
  LauncherInstallResult,
  LibraryGame,
} from "./types";

function formatBytes(bytes: number | null | undefined): string {
  if (bytes == null || bytes <= 0) return "—";
  const units = ["B", "KB", "MB", "GB", "TB"];
  let value = bytes;
  let idx = 0;
  while (value >= 1024 && idx < units.length - 1) {
    value /= 1024;
    idx += 1;
  }
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[idx]}`;
}

export type LauncherInstallModalProps = {
  game: LibraryGame;
  coverSrc: string | null;
  progress: LauncherInstallProgress | null;
  onClose: () => void;
  onRescan: () => void;
  onInstallStarted: (result: LauncherInstallResult) => void;
  onCancelInstall: (jobId: string) => void;
};

export function LauncherInstallModal({
  game,
  coverSrc,
  progress,
  onClose,
  onRescan,
  onInstallStarted,
  onCancelInstall,
}: LauncherInstallModalProps) {
  const [options, setOptions] = useState<LauncherInstallOptions | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [handoffSent, setHandoffSent] = useState<LauncherInstallResult | null>(null);

  // Form state for Heroic CLI options
  const [method, setMethod] = useState<InstallMethod>("handoff");
  const [installDir, setInstallDir] = useState("");
  const [platform, setPlatform] = useState("windows");
  const [language, setLanguage] = useState("");
  const [selectedDlcs, setSelectedDlcs] = useState<string[]>([]);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    setError(null);
    setHandoffSent(null);

    api
      .getLauncherInstallOptions(game.source, game.external_id)
      .then((opts) => {
        if (cancelled) return;
        setOptions(opts);
        setMethod(opts.cli_available ? "heroic_cli" : "handoff");
        setInstallDir(opts.default_install_dir ?? "");
        setPlatform(opts.platforms[0] ?? "windows");
        setLanguage(opts.languages[0] ?? "");
        setSelectedDlcs([]);
      })
      .catch((err) => {
        if (cancelled) return;
        setError(String(err));
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });

    return () => {
      cancelled = true;
    };
  }, [game.source, game.external_id]);

  const isHeroic =
    game.source === "heroic_epic" ||
    game.source === "heroic_gog" ||
    game.source === "heroic_amazon";

  const isInstalling =
    progress != null &&
    (progress.phase === "starting" ||
      progress.phase === "downloading" ||
      progress.phase === "verifying");

  async function pickInstallFolder() {
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        directory: true,
        multiple: false,
        defaultPath: installDir || undefined,
        title: `Choose install folder for ${game.title}`,
      });
      if (typeof selected === "string" && selected.trim()) {
        setInstallDir(selected);
      }
    } catch (err) {
      setError(`Could not open folder picker: ${String(err)}`);
    }
  }

  function toggleDlc(appName: string) {
    setSelectedDlcs((prev) =>
      prev.includes(appName) ? prev.filter((id) => id !== appName) : [...prev, appName],
    );
  }

  async function handleStartInstall() {
    setSubmitting(true);
    setError(null);
    try {
      const res = await api.requestLauncherInstall({
        game_id: game.id,
        source: game.source,
        external_id: game.external_id,
        title: game.title,
        method,
        install_dir: method === "heroic_cli" ? installDir.trim() || null : null,
        platform: method === "heroic_cli" ? platform : null,
        language: method === "heroic_cli" && language ? language : null,
        dlc_ids: method === "heroic_cli" ? selectedDlcs : [],
        metadata: game.metadata,
      });
      onInstallStarted(res);
      if (res.method === "handoff") {
        setHandoffSent(res);
      }
    } catch (err) {
      setError(String(err));
    } finally {
      setSubmitting(false);
    }
  }

  const primaryLabel =
    method === "heroic_cli"
      ? `Install via ${options?.cli_name ?? "Heroic CLI"}`
      : `Install in ${game.launcher}`;

  const footer = (
    <div className="row-actions">
      {isInstalling && progress ? (
        <>
          <button
            type="button"
            className="danger"
            onClick={() => onCancelInstall(progress.job_id)}
          >
            Cancel install
          </button>
          <button type="button" onClick={onClose}>
            Continue in background
          </button>
        </>
      ) : handoffSent ? (
        <>
          <button
            type="button"
            onClick={() => {
              onRescan();
              onClose();
            }}
          >
            Rescan library
          </button>
          <button type="button" className="secondary" onClick={onClose}>
            Done
          </button>
        </>
      ) : (
        <>
          <button type="button" className="secondary" onClick={onClose}>
            Cancel
          </button>
          <button
            type="button"
            disabled={
              loading ||
              submitting ||
              (method === "heroic_cli" && !installDir.trim())
            }
            onClick={() => void handleStartInstall()}
          >
            {submitting ? "Requesting…" : primaryLabel}
          </button>
        </>
      )}
    </div>
  );

  return (
    <Modal
      title={game.title}
      subtitle={`Install via ${game.launcher}`}
      kicker="Uninstalled Library Title"
      className="launcher-install-modal"
      onClose={onClose}
      footer={footer}
    >
      <div className="install-modal-hero">
        {coverSrc ? (
          <img src={coverSrc} alt={game.title} className="install-modal-poster" />
        ) : (
          <div className="install-modal-poster install-modal-poster-placeholder" />
        )}
        <div className="install-modal-meta">
          <div className="row" style={{ gap: "0.5rem", flexWrap: "wrap" }}>
            <span className="pill">{game.launcher}</span>
            {game.supported ? (
              <span className="pill ok">Supported by Emperor</span>
            ) : (
              <span className="pill">No built-in plugin (UE/Unity detectable after install)</span>
            )}
            {game.install_size_bytes ? (
              <span className="pill">{formatBytes(game.install_size_bytes)}</span>
            ) : null}
          </div>
          {options?.handoff_note && <p className="note">{options.handoff_note}</p>}
        </div>
      </div>

      {loading && <p className="note">Loading installation options…</p>}
      {error && <p className="error-text">{error}</p>}

      {handoffSent && (
        <div className="status-card" data-testid="install-handoff-confirmation">
          <strong>{handoffSent.message}</strong>
          <p className="note">
            Once the launcher finishes downloading, click <strong>Rescan library</strong> to
            manage this game in Emperor.
          </p>
        </div>
      )}

      {progress && (
        <div className="install-progress-card" data-testid="install-progress-card">
          <div className="install-progress-head">
            <strong>
              {progress.phase === "completed"
                ? "Installation complete"
                : progress.phase === "failed"
                  ? "Installation failed"
                  : progress.phase === "cancelled"
                    ? "Installation cancelled"
                    : progress.phase === "verifying"
                      ? "Verifying files…"
                      : "Downloading…"}
            </strong>
            <span>
              {progress.percent != null ? `${progress.percent.toFixed(1)}%` : ""}
              {progress.speed_bps ? ` · ${formatBytes(progress.speed_bps)}/s` : ""}
            </span>
          </div>
          <div className="install-progress-track">
            <div
              className="install-progress-fill"
              style={{ width: `${Math.min(100, Math.max(0, progress.percent ?? 0))}%` }}
            />
          </div>
          {progress.message && <p className="note">{progress.message}</p>}
          {progress.error && <p className="error-text">{progress.error}</p>}
        </div>
      )}

      {!loading && options && !handoffSent && !isInstalling && (
        <div className="install-options-form">
          {options.steam_libraries.length > 0 && (
            <div className="install-option-group">
              <h4>Detected Steam Library Folders</h4>
              <ul className="steam-library-folders">
                {options.steam_libraries.map((lib) => (
                  <li key={lib.path}>
                    <code>{lib.path}</code>
                    <span className="note">
                      {lib.free_bytes != null ? `${formatBytes(lib.free_bytes)} free` : ""}
                    </span>
                  </li>
                ))}
              </ul>
            </div>
          )}

          {isHeroic && (
            <>
              <div className="install-option-group">
                <h4>Install Method</h4>
                <div className="row" style={{ gap: "0.75rem", flexWrap: "wrap" }}>
                  <label className="radio-option">
                    <input
                      type="radio"
                      name="heroic-install-method"
                      checked={method === "heroic_cli"}
                      disabled={!options.cli_available}
                      onChange={() => setMethod("heroic_cli")}
                    />
                    <span>
                      Direct install via {options.cli_name ?? "Heroic CLI"}
                      {!options.cli_available ? " (CLI not found)" : ""}
                    </span>
                  </label>
                  <label className="radio-option">
                    <input
                      type="radio"
                      name="heroic-install-method"
                      checked={method === "handoff"}
                      onChange={() => setMethod("handoff")}
                    />
                    <span>Open in Heroic Launcher</span>
                  </label>
                </div>
              </div>

              {method === "heroic_cli" && (
                <>
                  <div className="install-option-group">
                    <label htmlFor="heroic-install-dir">Install Folder</label>
                    <div className="row">
                      <input
                        id="heroic-install-dir"
                        type="text"
                        value={installDir}
                        onChange={(e) => setInstallDir(e.target.value)}
                        placeholder="/path/to/Games/Heroic"
                      />
                      <button type="button" className="secondary" onClick={() => void pickInstallFolder()}>
                        Browse…
                      </button>
                    </div>
                    {options.default_dir_free_bytes != null && (
                      <p className="note">
                        Available space: {formatBytes(options.default_dir_free_bytes)}
                      </p>
                    )}
                  </div>

                  {options.platforms.length > 1 && (
                    <div className="install-option-group">
                      <label htmlFor="heroic-platform">Platform</label>
                      <select
                        id="heroic-platform"
                        value={platform}
                        onChange={(e) => setPlatform(e.target.value)}
                      >
                        {options.platforms.map((p) => (
                          <option key={p} value={p}>
                            {p.charAt(0).toUpperCase() + p.slice(1)}
                          </option>
                        ))}
                      </select>
                    </div>
                  )}

                  {options.languages.length > 1 && (
                    <div className="install-option-group">
                      <label htmlFor="heroic-language">Language</label>
                      <select
                        id="heroic-language"
                        value={language}
                        onChange={(e) => setLanguage(e.target.value)}
                      >
                        {options.languages.map((lang) => (
                          <option key={lang} value={lang}>
                            {lang}
                          </option>
                        ))}
                      </select>
                    </div>
                  )}

                  {options.dlcs.length > 0 && (
                    <div className="install-option-group">
                      <div className="row" style={{ justifyContent: "space-between" }}>
                        <h4>DLCs ({selectedDlcs.length}/{options.dlcs.length})</h4>
                        <div className="row" style={{ gap: "0.5rem" }}>
                          <button
                            type="button"
                            className="link-button"
                            onClick={() => setSelectedDlcs(options.dlcs.map((d) => d.app_name))}
                          >
                            Select all
                          </button>
                          <button
                            type="button"
                            className="link-button"
                            onClick={() => setSelectedDlcs([])}
                          >
                            Clear
                          </button>
                        </div>
                      </div>
                      <ul className="install-dlc-list">
                        {options.dlcs.map((dlc) => (
                          <li key={dlc.app_name}>
                            <label>
                              <input
                                type="checkbox"
                                checked={selectedDlcs.includes(dlc.app_name)}
                                onChange={() => toggleDlc(dlc.app_name)}
                              />
                              <span>{dlc.title}</span>
                            </label>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                </>
              )}
            </>
          )}
        </div>
      )}
    </Modal>
  );
}
