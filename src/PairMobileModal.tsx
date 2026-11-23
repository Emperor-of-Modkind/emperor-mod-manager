import { useCallback, useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCheck, faMobileScreenButton, faRotateRight } from "@fortawesome/free-solid-svg-icons";
import { Modal } from "./Modal";
import { api } from "./api";
import type { MobilePairingSession, PairedDevice } from "./types";

type PairMobileModalProps = {
  onClose: () => void;
  onPaired?: (device: PairedDevice) => void;
};

function formatCountdown(secs: number): string {
  const clamped = Math.max(0, Math.floor(secs));
  const m = Math.floor(clamped / 60);
  const s = clamped % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

export function PairMobileModal({ onClose, onPaired }: PairMobileModalProps) {
  const [session, setSession] = useState<MobilePairingSession | null>(null);
  const [remainingSecs, setRemainingSecs] = useState<number>(0);
  const [error, setError] = useState<string | null>(null);
  const [loading, setLoading] = useState<boolean>(true);
  const [pairedDevice, setPairedDevice] = useState<PairedDevice | null>(null);
  const [attemptWarning, setAttemptWarning] = useState<string | null>(null);

  const startNewSession = useCallback(async () => {
    setLoading(true);
    setError(null);
    setAttemptWarning(null);
    setPairedDevice(null);
    try {
      const next = await api.startMobilePairing();
      setSession(next);
      setRemainingSecs(next.ttl_secs);
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void startNewSession();
    return () => {
      void api.cancelMobilePairing().catch(() => {});
    };
  }, [startNewSession]);

  useEffect(() => {
    if (!session || pairedDevice) return;
    const id = window.setInterval(() => {
      setRemainingSecs((prev) => (prev > 0 ? prev - 1 : 0));
    }, 1000);
    return () => window.clearInterval(id);
  }, [session, pairedDevice]);

  useEffect(() => {
    let active = true;
    const cleanups: (() => void)[] = [];

    listen<PairedDevice>("mobile-paired", (event) => {
      if (!active) return;
      setPairedDevice(event.payload);
      setAttemptWarning(null);
      onPaired?.(event.payload);
    })
      .then((fn) => {
        if (active) cleanups.push(fn);
        else fn();
      })
      .catch(() => {});

    listen<{ remaining_attempts: number; message: string }>(
      "mobile-pairing-failed",
      (event) => {
        if (!active) return;
        setAttemptWarning(event.payload.message);
        if (event.payload.remaining_attempts <= 0) {
          setRemainingSecs(0);
        }
      },
    )
      .then((fn) => {
        if (active) cleanups.push(fn);
        else fn();
      })
      .catch(() => {});

    return () => {
      active = false;
      for (const fn of cleanups) fn();
    };
  }, [onPaired]);

  const expired = !pairedDevice && session !== null && remainingSecs <= 0;

  return (
    <Modal
      title="Pair Mobile Device"
      subtitle="Connect Emperor Mod Manager on Android over your local Wi-Fi network."
      kicker="Local Network Pairing"
      className="pair-mobile-modal"
      onClose={onClose}
      footer={
        pairedDevice ? (
          <button type="button" className="primary" onClick={onClose}>
            Done
          </button>
        ) : (
          <div className="pair-mobile-footer">
            <button
              type="button"
              onClick={() => void startNewSession()}
              disabled={loading}
            >
              <FontAwesomeIcon icon={faRotateRight} /> Generate New Code
            </button>
            <button type="button" onClick={onClose}>
              Cancel
            </button>
          </div>
        )
      }
    >
      {pairedDevice ? (
        <div className="pair-mobile-success" role="status">
          <div className="pair-mobile-success-icon">
            <FontAwesomeIcon icon={faCheck} />
          </div>
          <h4>Paired with {pairedDevice.device_name}</h4>
          <p className="muted">
            {pairedDevice.device_model} is now authorized to sync your library,
            collections, and queue downloads over iroh.
          </p>
        </div>
      ) : error ? (
        <div className="pair-mobile-error" role="alert">
          <p>{error}</p>
        </div>
      ) : loading || !session ? (
        <p className="muted">Starting local pairing session…</p>
      ) : (
        <div className="pair-mobile-body">
          <p className="pair-mobile-steps">
            Open <strong>Emperor Mod Manager</strong> on your Android device on the same
            Wi-Fi network, select <strong>{session.host_name}</strong>, and enter this
            code:
          </p>

          <div
            className={`pair-mobile-code${expired ? " expired" : ""}`}
            aria-label={`Pairing code ${session.code.replace("-", " ")}`}
          >
            {session.code}
          </div>

          <div className="pair-mobile-meta">
            <span className="pair-mobile-timer">
              <FontAwesomeIcon icon={faMobileScreenButton} />{" "}
              {expired
                ? "Code expired — generate a new code"
                : `Expires in ${formatCountdown(remainingSecs)}`}
            </span>
            {session.lan_addrs.length > 0 && (
              <span className="muted">LAN: {session.lan_addrs[0]}</span>
            )}
          </div>

          {attemptWarning && (
            <p className="pair-mobile-warning" role="alert">
              {attemptWarning}
            </p>
          )}
        </div>
      )}
    </Modal>
  );
}
