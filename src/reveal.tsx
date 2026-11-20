import { useEffect, useState, useTransition, type ReactNode } from "react";

/**
 * Becomes true only after `ready` has been true long enough to paint the
 * skeleton, and the heavy tree has been rendered in a transition.
 * A later `ready === false` hides it on that render and cancels any pending frame.
 */
export function useReveal(ready: boolean): boolean {
  const [shown, setShown] = useState(false);
  const [, startReveal] = useTransition();
  useEffect(() => {
    if (!ready) {
      setShown(false);
      return;
    }
    let cancelled = false;
    const id = window.requestAnimationFrame(() => {
      if (cancelled) return;
      startReveal(() => {
        if (!cancelled) setShown(true);
      });
    });
    return () => {
      cancelled = true;
      window.cancelAnimationFrame(id);
    };
  }, [ready]);
  // `shown` flips inside the transition, so the heavy tree renders there
  // while the skeleton stays on screen. Gate on `ready` so leaving the view
  // hides the list on this render, before the effect clears `shown`.
  return ready && shown;
}

/** Crossfades `children` over `skeleton`. The skeleton stays up until the fade ends. */
export function ResultsReveal({
  show,
  skeleton,
  children,
}: {
  show: boolean;
  skeleton: ReactNode;
  children: ReactNode;
}) {
  const [cover, setCover] = useState(!show);
  useEffect(() => {
    if (!show) {
      setCover(true);
      return;
    }
    const id = window.setTimeout(() => setCover(false), 200);
    return () => window.clearTimeout(id);
  }, [show]);
  return (
    <div className={show ? "results-stage is-filled" : "results-stage"}>
      {show ? <div className="results-fill">{children}</div> : null}
      {cover ? <div className={show ? "results-cover" : undefined}>{skeleton}</div> : null}
    </div>
  );
}
