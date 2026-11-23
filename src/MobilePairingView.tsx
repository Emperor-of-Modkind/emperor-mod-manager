import { useCallback, useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import {
  faDesktop,
  faRotateRight,
  faWifi,
} from "@fortawesome/free-solid-svg-icons";
import { api } from "./api";
import type { DiscoveredDesktop, MobileConnectionState } from "./types";

type MobilePairingViewProps = {
  onPaired: (state: MobileConnectionState) => void;
};

export function MobilePairingView({ onPaired }: MobilePairingViewProps) {
  const [desktops, setDesktops] = useState<DiscoveredDesktop[]>([]);
  const [scanning, setScanning] = useState<boolean>(false);
  const [selected, setSelected] = useState<DiscoveredDesktop | null>(null);
  const [manualMode, setManualMode] = useState<boolean>(false);
  const [manualEndpointId, setManualEndpointId] = useState<string>("");
  const [manualLanAddr, setManualLanAddr] = useState<string>("");
  const [code, setCode] = useState<string>("");
  const [submitting, setSubmitting] = useState<boolean>(false);
  const [error, setError] = useState<string | null>(null);

  const scan = useCallback(async () => {
    setScanning(true);
    setError(null);
    try {
      const found = await api.discoverLanDesktops();
      setDesktops(found);
      if (found.length === 1 && found[0].pairing_active) {
        setSelected(found[0]);
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setScanning(false);
    }
  }, []);

  useEffect(() => {
    void scan();
  }, [scan]);

  async function submitPair(e: React.FormEvent) {
    e.preventDefault();
    const digits = code.replace(/\D/g, "");
    if (digits.length !== 6) {
      setError("Enter the 6-digit code shown on your desktop.");
      return;
    }

    const endpointId = manualMode
      ? manualEndpointId.trim()
      : selected?.desktop_endpoint_id ?? "";
    const lanAddrs = manualMode
      ? manualLanAddr.trim()
        ? [manualLanAddr.trim()]
        : []
      : selected?.lan_addrs ?? [];

    if (!endpointId) {
      setError("Select a desktop or enter its endpoint ID.");
      return;
    }

    setSubmitting(true);
    setError(null);
    try {
      const state = await api.pairWithDesktop({
        desktopEndpointId: endpointId,
        lanAddrs,
        code: digits,
        deviceName: "Android Companion",
        deviceModel: navigator.userAgent.includes("Android") ? "Android" : "Mobile",
      });
      onPaired(state);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="mobile-pairing-screen">
      <div className="mobile-pairing-card">
        <div className="mobile-pairing-header">
          <FontAwesomeIcon icon={faWifi} />
          <h2>Pair with Desktop</h2>
          <p className="muted">
            On your PC, open <strong>Settings → Mobile Companion → Pair Mobile Device</strong>{" "}
            to display a 6-digit pairing code.
          </p>
        </div>

        {!manualMode && (
          <div className="mobile-pairing-discovery">
            <div className="mobile-pairing-discovery-bar">
              <span>Desktops on Wi-Fi</span>
              <button type="button" onClick={() => void scan()} disabled={scanning}>
                <FontAwesomeIcon icon={faRotateRight} spin={scanning} />{" "}
                {scanning ? "Scanning…" : "Rescan"}
              </button>
            </div>

            {desktops.length === 0 ? (
              <p className="muted">
                {scanning
                  ? "Looking for Emperor Mod Manager on your local network…"
                  : "No desktops detected yet. Make sure both devices are on the same Wi-Fi and the pairing modal is open."}
              </p>
            ) : (
              <div className="mobile-desktop-list">
                {desktops.map((desk) => (
                  <button
                    key={desk.desktop_endpoint_id}
                    type="button"
                    className={`mobile-desktop-item${
                      selected?.desktop_endpoint_id === desk.desktop_endpoint_id
                        ? " selected"
                        : ""
                    }`}
                    onClick={() => setSelected(desk)}
                  >
                    <FontAwesomeIcon icon={faDesktop} />
                    <div>
                      <strong>{desk.host_name}</strong>
                      <div className="muted">
                        {desk.pairing_active
                          ? "Ready to pair"
                          : "Open pairing modal on desktop"}
                      </div>
                    </div>
                  </button>
                ))}
              </div>
            )}
          </div>
        )}

        <button
          type="button"
          className="link-button"
          onClick={() => setManualMode((v) => !v)}
        >
          {manualMode ? "Back to automatic Wi-Fi discovery" : "Enter desktop address manually"}
        </button>

        <form className="mobile-pairing-form" onSubmit={(e) => void submitPair(e)}>
          {manualMode && (
            <>
              <label>
                Desktop Endpoint ID
                <input
                  type="text"
                  value={manualEndpointId}
                  onChange={(e) => setManualEndpointId(e.target.value)}
                  placeholder="Paste desktop endpoint ID"
                />
              </label>
              <label>
                Desktop LAN IP:Port
                <input
                  type="text"
                  value={manualLanAddr}
                  onChange={(e) => setManualLanAddr(e.target.value)}
                  placeholder="192.168.1.42:12345"
                />
              </label>
            </>
          )}

          <label>
            6-Digit Pairing Code
            <input
              type="text"
              inputMode="numeric"
              autoComplete="one-time-code"
              maxLength={7}
              value={code}
              onChange={(e) => setCode(e.target.value)}
              placeholder="482-917"
              className="mobile-code-input"
            />
          </label>

          {error && (
            <p className="pair-mobile-warning" role="alert">
              {error}
            </p>
          )}

          <button
            type="submit"
            className="primary mobile-pair-submit"
            disabled={submitting || (!manualMode && !selected)}
          >
            {submitting ? "Pairing & Syncing…" : "Pair Device"}
          </button>
        </form>
      </div>
    </div>
  );
}
