import { useCallback, useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faMobileScreenButton, faTrashCan } from "@fortawesome/free-solid-svg-icons";
import { SettingsSection, SettingsToggle } from "./SettingsSection";
import { PairMobileModal } from "./PairMobileModal";
import { api } from "./api";
import type { PairedDevice } from "./types";

export type MobileDevicesSettingsProps = {
  runInBackground?: boolean;
};

export function MobileDevicesSettings({
  runInBackground = false,
}: MobileDevicesSettingsProps = {}) {
  const [devices, setDevices] = useState<PairedDevice[]>([]);
  const [allowRelay, setAllowRelay] = useState<boolean>(true);
  const [showPairModal, setShowPairModal] = useState<boolean>(false);
  const [busyPeerId, setBusyPeerId] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [list, relay] = await Promise.all([
        api.listPairedDevices(),
        api.getRemoteAllowRelay(),
      ]);
      setDevices(list);
      setAllowRelay(relay);
    } catch {
      // Desktop command may be unavailable in tests/mobile
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  async function toggleRelay(next: boolean) {
    setAllowRelay(next);
    try {
      await api.setRemoteAllowRelay(next);
    } catch {
      setAllowRelay(!next);
    }
  }

  async function toggleRemoteLaunch(device: PairedDevice, next: boolean) {
    setBusyPeerId(device.peer_id);
    try {
      const updated = await api.setPairedDevicePermissions(device.peer_id, next);
      setDevices((prev) =>
        prev.map((d) => (d.peer_id === updated.peer_id ? updated : d)),
      );
    } finally {
      setBusyPeerId(null);
    }
  }

  async function revoke(peerId: string) {
    setBusyPeerId(peerId);
    try {
      await api.revokePairedDevice(peerId);
      setDevices((prev) => prev.filter((d) => d.peer_id !== peerId));
    } finally {
      setBusyPeerId(null);
    }
  }

  return (
    <SettingsSection title="Mobile Companion">
      <p className="muted">
        Pair your Android phone or foldable over local Wi-Fi to sync your library,
        collections, and accounts, and queue downloads or edit loadouts remotely via iroh.
      </p>

      <div className="mobile-devices-actions">
        <button
          type="button"
          className="primary"
          onClick={() => setShowPairModal(true)}
        >
          <FontAwesomeIcon icon={faMobileScreenButton} /> Pair Mobile Device
        </button>
      </div>

      <div className="mobile-devices-option">
        <SettingsToggle
          checked={allowRelay}
          onChange={(checked) => void toggleRelay(checked)}
        >
          Allow remote control outside local network (via iroh relay)
        </SettingsToggle>
      </div>

      {devices.length > 0 && !runInBackground && (
        <p className="note">
          Tip: Enable <strong>Run in background</strong> under{" "}
          <strong>Background &amp; tray</strong> so your mobile companion can still
          reach this PC after you close the window.
        </p>
      )}

      {devices.length === 0 ? (
        <p className="muted">No mobile devices paired yet.</p>
      ) : (
        <ul className="paired-devices-list">
          {devices.map((device) => (
            <li key={device.peer_id} className="paired-device-row">
              <div className="paired-device-info">
                <div className="paired-device-title">
                  <strong>{device.device_name}</strong>
                  <span
                    className={`paired-device-badge${device.connected ? " online" : ""}`}
                  >
                    {device.connected ? "Connected" : "Offline"}
                  </span>
                </div>
                <div className="muted paired-device-meta">
                  {device.device_model} · Paired{" "}
                  {new Date(device.paired_at).toLocaleDateString()}
                </div>
                <SettingsToggle
                  checked={device.allow_remote_launch}
                  disabled={busyPeerId === device.peer_id}
                  onChange={(checked) => void toggleRemoteLaunch(device, checked)}
                >
                  Allow remote game launch
                </SettingsToggle>
              </div>
              <button
                type="button"
                className="danger"
                disabled={busyPeerId === device.peer_id}
                onClick={() => void revoke(device.peer_id)}
                title="Revoke paired device"
              >
                <FontAwesomeIcon icon={faTrashCan} /> Unpair
              </button>
            </li>
          ))}
        </ul>
      )}

      {showPairModal && (
        <PairMobileModal
          onClose={() => setShowPairModal(false)}
          onPaired={() => void refresh()}
        />
      )}
    </SettingsSection>
  );
}
