import { useLayoutEffect, useRef, useState, type MouseEvent, type ReactNode, type ToggleEvent } from "react";
import { motion, useReducedMotion } from "motion/react";

import { MOTION_EASE, MOTION_FAST } from "./motion";

const collapseSection = new WeakMap<HTMLDetailsElement, () => void>();

export function SettingsSection({
  title,
  children,
  defaultOpen = false,
}: {
  title: string;
  children: ReactNode;
  defaultOpen?: boolean;
}) {
  const detailsRef = useRef<HTMLDetailsElement>(null);
  const expandedRef = useRef(defaultOpen);
  const closingRef = useRef(false);
  const holdOpenRef = useRef(defaultOpen);
  const [expanded, setExpanded] = useState(defaultOpen);
  const reduced = useReducedMotion() === true;

  function collapse() {
    if (!expandedRef.current) return;
    expandedRef.current = false;
    closingRef.current = true;
    setExpanded(false);
  }

  function finishClose() {
    if (!closingRef.current || expandedRef.current) return;
    closingRef.current = false;
    holdOpenRef.current = false;
    if (detailsRef.current) detailsRef.current.open = false;
  }

  function openThis() {
    const el = detailsRef.current;
    if (!el) return;
    const parent = el.parentElement;
    if (parent) {
      for (const other of parent.querySelectorAll<HTMLDetailsElement>(
        ":scope > details.settings-section",
      )) {
        if (other !== el) collapseSection.get(other)?.();
      }
    }
    closingRef.current = false;
    holdOpenRef.current = true;
    expandedRef.current = true;
    el.open = true;
    setExpanded(true);
  }

  function onSummaryClick(event: MouseEvent<HTMLElement>) {
    event.preventDefault();
    if (expandedRef.current) collapse();
    else openThis();
  }

  function keepOpenThroughAnimation(event: ToggleEvent<HTMLDetailsElement>) {
    const el = event.currentTarget;
    if (holdOpenRef.current && !el.open) el.open = true;
  }

  useLayoutEffect(() => {
    const el = detailsRef.current;
    if (!el) return;
    if (defaultOpen) el.open = true;
    collapseSection.set(el, collapse);
    return () => {
      collapseSection.delete(el);
    };
  }, [defaultOpen]);

  return (
    <details
      ref={detailsRef}
      className="settings-section"
      data-expanded={expanded ? "true" : "false"}
      onToggle={keepOpenThroughAnimation}
    >
      <summary onClick={onSummaryClick}>{title}</summary>
      <motion.div
        className="settings-section-body"
        initial={false}
        animate={{ height: expanded ? "auto" : 0 }}
        transition={{ duration: reduced ? 0 : MOTION_FAST, ease: MOTION_EASE }}
        onAnimationComplete={finishClose}
        style={{ overflow: "hidden" }}
      >
        <div className="settings-section-inner">{children}</div>
      </motion.div>
    </details>
  );
}

export interface SettingsToggleProps {
  checked: boolean;
  disabled?: boolean;
  onChange: (checked: boolean) => void;
  children: ReactNode;
}

/** Accessible switch toggle styled to match inline settings rows. */
export function SettingsToggle({
  checked,
  disabled = false,
  onChange,
  children,
}: SettingsToggleProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      disabled={disabled}
      className="setting-toggle"
      onClick={() => onChange(!checked)}
    >
      <span className="switch-track" aria-hidden="true">
        <span className="switch-thumb" />
      </span>
      <span className="setting-toggle-label">{children}</span>
    </button>
  );
}
