import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";

import type { PluginToolRecommendation } from "./types";

export function ToolRecommendationNotice({
  recommendations,
  onOpenTools,
  onDismiss,
}: {
  recommendations: PluginToolRecommendation[];
  onOpenTools: () => void;
  onDismiss?: () => void;
}) {
  if (recommendations.length === 0) {
    return null;
  }

  return (
    <div className="wine-override-notice tool-recommendation-notice">
      <div>
        <strong>Recommended tools</strong>
        <p className="note">
          Enabled mods for this game need {recommendations.length === 1 ? "a tool" : "tools"} that{" "}
          {recommendations.length === 1 ? "is" : "are"} not installed yet. Install them from the
          Tools modal, or dismiss if you manage them yourself.
        </p>
        <ul>
          {recommendations.map((rec) => (
            <li key={rec.tool_id}>
              <strong>{rec.tool_name}</strong>
              {" — "}
              {rec.reason}
            </li>
          ))}
        </ul>
      </div>
      <div className="wine-override-actions">
        {onDismiss && (
          <div className="wine-override-icon-actions">
            <button
              type="button"
              className="icon-btn"
              aria-label="Dismiss"
              title="Dismiss"
              onClick={onDismiss}
            >
              <FontAwesomeIcon icon={faXmark} />
            </button>
          </div>
        )}
        <button type="button" onClick={onOpenTools}>
          Open Tools
        </button>
      </div>
    </div>
  );
}
