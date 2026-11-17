import { useEffect, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCheck, faCopy } from "@fortawesome/free-solid-svg-icons";

import type { WineOverrideAdvice } from "./types";

export function WineOverrideNotice({
  advice,
  busy,
  onApply,
}: {
  advice: WineOverrideAdvice;
  busy: boolean;
  onApply: () => void;
}) {
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    setCopied(false);
  }, [advice.launch_options]);

  useEffect(() => {
    if (!copied) return;
    const timer = window.setTimeout(() => setCopied(false), 2000);
    return () => window.clearTimeout(timer);
  }, [copied]);

  return (
    <div className="wine-override-notice">
      <div>
        <strong>Proton launch options</strong>
        <p className="note">{advice.summary}</p>
        <ul>
          {advice.needs.map((need) => (
            <li key={`${need.dll}:${need.reason}`}>
              <code>{need.dll}.dll</code>
              {" — "}
              {need.reason}
            </li>
          ))}
        </ul>
        <p className="wine-override-command">{advice.launch_options}</p>
      </div>
      <div className="wine-override-actions">
        <button
          type="button"
          className="icon-btn"
          aria-label={copied ? "Copied" : "Copy"}
          title={copied ? "Copied" : "Copy"}
          onClick={() => {
            void navigator.clipboard.writeText(advice.launch_options).then(
              () => setCopied(true),
              () => setCopied(false),
            );
          }}
        >
          <FontAwesomeIcon icon={copied ? faCheck : faCopy} />
        </button>
        {advice.can_apply && (
          <button type="button" onClick={onApply} disabled={busy}>
            Set in Steam
          </button>
        )}
      </div>
    </div>
  );
}
