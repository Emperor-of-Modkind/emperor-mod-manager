type ThrobberProps = {
  className?: string;
};

/** Indeterminate circular indicator. Track and arc colors come from CSS. */
export function Throbber({ className }: ThrobberProps) {
  return (
    <svg
      className={["throbber", className].filter(Boolean).join(" ")}
      viewBox="0 0 24 24"
      aria-hidden="true"
    >
      <circle className="throbber-track" cx="12" cy="12" r="9" />
      <circle className="throbber-arc" cx="12" cy="12" r="9" />
    </svg>
  );
}
