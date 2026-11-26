import { useState } from "react";
import { SettingsSection, SettingsToggle } from "./SettingsSection";
import { api } from "./api";
import type { BackgroundSettings } from "./types";

export const DEFAULT_BACKGROUND_SETTINGS: BackgroundSettings = {
  run_in_background: false,
  close_to_tray: true,
  minimize_to_tray: false,
  start_on_login: false,
  start_minimized: false,
};

export type BackgroundTraySettingsProps = {
  value?: BackgroundSettings;
  trayAvailable?: boolean;
  onUpdated: () => Promise<unknown> | void;
  onError?: (msg: string) => void;
};

export function BackgroundTraySettings({
  value,
  trayAvailable = true,
  onUpdated,
  onError,
}: BackgroundTraySettingsProps) {
  const current = value ?? DEFAULT_BACKGROUND_SETTINGS;
  const [saving, setSaving] = useState(false);

  async function update(patch: Partial<BackgroundSettings>) {
    const next: BackgroundSettings = { ...current, ...patch };
    setSaving(true);
    try {
      await api.setBackgroundSettings(next);
      await onUpdated();
    } catch (err) {
      onError?.(String(err));
    } finally {
      setSaving(false);
    }
  }

  const subOptionsDisabled = saving || !trayAvailable || !current.run_in_background;

  return (
    <SettingsSection title="Background & tray">
      <p className="muted">
        Keep Emperor Mod Manager running in the system tray so your Mobile Companion
        stays connected, active downloads continue, and friend sync remains online
        when the window is closed or minimized.
      </p>

      {!trayAvailable && (
        <p className="note" role="status">
          System tray support was not detected in this desktop session. On Linux,
          install an AppIndicator/StatusNotifier host (for example{" "}
          <code>libayatana-appindicator</code> or the GNOME AppIndicator extension)
          to enable background tray mode.
        </p>
      )}

      <SettingsToggle
        checked={current.run_in_background}
        disabled={saving || !trayAvailable}
        onChange={(checked) => void update({ run_in_background: checked })}
      >
        Run in background (system tray)
      </SettingsToggle>

      <div className="background-tray-suboptions">
        <SettingsToggle
          checked={current.close_to_tray}
          disabled={subOptionsDisabled}
          onChange={(checked) => void update({ close_to_tray: checked })}
        >
          Close to tray — hide to the system tray instead of quitting when closing the window
        </SettingsToggle>

        <SettingsToggle
          checked={current.minimize_to_tray}
          disabled={subOptionsDisabled}
          onChange={(checked) => void update({ minimize_to_tray: checked })}
        >
          Minimize to tray — hide to the system tray instead of the taskbar when minimizing
        </SettingsToggle>

        <SettingsToggle
          checked={current.start_on_login}
          disabled={subOptionsDisabled}
          onChange={(checked) => void update({ start_on_login: checked })}
        >
          Start on login — launch Emperor automatically when you sign in
        </SettingsToggle>

        <SettingsToggle
          checked={current.start_minimized}
          disabled={subOptionsDisabled}
          onChange={(checked) => void update({ start_minimized: checked })}
        >
          Start minimized to tray — start hidden in the system tray
        </SettingsToggle>
      </div>
    </SettingsSection>
  );
}
