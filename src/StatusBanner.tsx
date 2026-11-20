import { useEffect, useLayoutEffect, useRef, useState } from "react";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faXmark } from "@fortawesome/free-solid-svg-icons";

import { requirementsNeedAttention } from "./RequirementsDialog";
import type { BannerTimeouts, DeployResult, MissingRequirements, Settings } from "./types";

export type BannerAction = { label: string; run: () => void };

export type BannerKind = "error" | "warn" | "info";

export const DEFAULT_BANNER_TIMEOUTS: BannerTimeouts = {
  info: 10,
  warn: 300,
  error: null,
};

export function bannerTimeoutFor(
  kind: BannerKind,
  settings: Settings | null | undefined,
): number | null {
  const configured = settings?.banner_timeouts?.[kind];
  return configured === undefined ? DEFAULT_BANNER_TIMEOUTS[kind] : configured;
}

export type StatusNotice = {
  kind: "ok" | "warn";
  message: string;
  details?: string[];
  action?: BannerAction;
};

export type StackedBanner = {
  id: number;
  kind: BannerKind;
  message: string;
  details?: string[];
  action?: BannerAction | null;
};

function sameBannerContent(
  a: Pick<StackedBanner, "kind" | "message" | "details">,
  b: Pick<StackedBanner, "kind" | "message" | "details">,
): boolean {
  if (a.kind !== b.kind || a.message !== b.message) return false;
  const da = a.details ?? [];
  const db = b.details ?? [];
  return da.length === db.length && da.every((line, i) => line === db[i]);
}

/**
 * Adds a banner to the stack. Green (`info`) banners replace any previous
 * green banner; amber (`warn`) and red (`error`) banners stack newest-first,
 * deduplicating identical entries and capping at `maxStacked`.
 */
export function pushBanner(
  stack: StackedBanner[],
  next: Omit<StackedBanner, "id">,
  id: number,
  maxStacked = 10,
): StackedBanner[] {
  if (next.kind === "info") {
    return [{ ...next, id }, ...stack.filter((b) => b.kind !== "info")];
  }
  const withoutDup = stack.filter((b) => !sameBannerContent(b, next));
  const combined = [{ ...next, id }, ...withoutDup];
  let keptStacked = 0;
  return combined.filter((b) => {
    if (b.kind === "info") return true;
    keptStacked += 1;
    return keptStacked <= maxStacked;
  });
}

export function dismissBanner(stack: StackedBanner[], id: number): StackedBanner[] {
  return stack.filter((b) => b.id !== id);
}

type StatusBannerProps = {
  kind: BannerKind;
  message: string;
  details?: string[];
  action?: BannerAction | null;
  timeoutSeconds?: number | null;
  onDismiss: () => void;
};

/** Detail lines that are not already the whole collapsed message. */
export function hiddenBannerDetails(message: string, details?: string[]): string[] {
  const lines = (details ?? []).map((line) => line.trim()).filter(Boolean);
  if (lines.length === 0) return [];
  if (lines.length === 1 && lines[0] === message.trim()) return [];
  return lines;
}

export function deployStatusNotice(
  result: DeployResult,
  onReviewRequirements: (requirements: MissingRequirements) => void,
): StatusNotice {
  const files = `${result.file_count} file${result.file_count === 1 ? "" : "s"}`;
  const mods = `${result.enabled_mods} mod${result.enabled_mods === 1 ? "" : "s"}`;
  if (result.warnings.length === 0) {
    return {
      kind: "ok",
      message: `Deployed ${files} from ${result.enabled_mods} enabled mod${result.enabled_mods === 1 ? "" : "s"} and launched game.`,
    };
  }
  const payload = reviewableRequirements(result);
  return {
    kind: "warn",
    message: `Launched with ${files} from ${mods}.`,
    details: result.warnings,
    action: payload
      ? {
          label: "Review requirements",
          run: () => onReviewRequirements(payload),
        }
      : undefined,
  };
}

function reviewableRequirements(result: DeployResult): MissingRequirements | null {
  const payload = result.missing_requirements;
  if (!payload || (result.plugin_skips?.length ?? 0) === 0) return null;
  if (!requirementsNeedAttention(payload)) return null;
  return payload;
}

export function StatusBanner({
  kind,
  message,
  details,
  action,
  timeoutSeconds,
  onDismiss,
}: StatusBannerProps) {
  const messageRef = useRef<HTMLParagraphElement>(null);
  const onDismissRef = useRef(onDismiss);
  onDismissRef.current = onDismiss;
  const [expanded, setExpanded] = useState(false);
  const [overflows, setOverflows] = useState(false);
  const hidden = hiddenBannerDetails(message, details);
  const bannerKey = `${message}\0${hidden.join("\0")}`;
  const [seenKey, setSeenKey] = useState(bannerKey);
  if (seenKey !== bannerKey) {
    setSeenKey(bannerKey);
    setExpanded(false);
    setOverflows(false);
  }

  useEffect(() => {
    if (timeoutSeconds == null || timeoutSeconds <= 0) return;
    const timer = window.setTimeout(() => {
      onDismissRef.current();
    }, timeoutSeconds * 1000);
    return () => window.clearTimeout(timer);
  }, [kind, bannerKey, timeoutSeconds]);

  useLayoutEffect(() => {
    const el = messageRef.current;
    if (!el || expanded) return;
    const measure = () => {
      setOverflows(el.scrollHeight > el.clientHeight + 1);
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  }, [bannerKey, expanded]);

  const canExpand = overflows || hidden.length > 0;

  return (
    <div className={`banner ${kind}`}>
      <div className="banner-copy">
        <p
          ref={messageRef}
          className={expanded ? "banner-message expanded" : "banner-message"}
        >
          {message}
        </p>
        {expanded && hidden.length > 0 && (
          <ul className="banner-details">
            {hidden.map((line, index) => (
              <li key={`${index}:${line}`}>{line}</li>
            ))}
          </ul>
        )}
      </div>
      <span className="banner-actions">
        {action ? (
          <button className="linkish" type="button" onClick={() => action.run()}>
            {action.label}
          </button>
        ) : null}
        {canExpand ? (
          <button
            className="linkish"
            type="button"
            aria-expanded={expanded}
            onClick={() => setExpanded((open) => !open)}
          >
            {expanded ? "Show less" : "Show more"}
          </button>
        ) : null}
        <button
          className="linkish icon-btn"
          type="button"
          onClick={onDismiss}
          aria-label="Dismiss"
          title="Dismiss"
        >
          <FontAwesomeIcon icon={faXmark} />
        </button>
      </span>
    </div>
  );
}
