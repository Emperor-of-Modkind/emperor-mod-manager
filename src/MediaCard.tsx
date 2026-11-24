import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent as ReactKeyboardEvent,
  type ReactNode,
  type TransitionEvent as ReactTransitionEvent,
} from "react";
import { createPortal } from "react-dom";
import { FontAwesomeIcon } from "@fortawesome/react-fontawesome";
import { faCheck } from "@fortawesome/free-solid-svg-icons";
import { useQueuedImageSrc } from "./imageLoadQueue";
import { MotionCard } from "./motion";
import { SourceMark } from "./SourceMark";
import type { CatalogSource } from "./types";

export function PlaceholderIcon() {
  return (
    <div className="media-placeholder" aria-hidden>
      <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5">
        <rect x="3" y="6" width="18" height="12" rx="2" />
        <path d="M8 12h.01M12 10v4M16 12h.01" strokeLinecap="round" />
      </svg>
    </div>
  );
}

export function useDecodedImageReveal(src: string | null) {
  const { activeSrc, status, markLoaded, markFailed } = useQueuedImageSrc(src);
  const imgRef = useRef<HTMLImageElement | null>(null);
  const activeSrcRef = useRef(activeSrc);
  activeSrcRef.current = activeSrc;
  const [fadeDone, setFadeDone] = useState(false);

  useEffect(() => {
    if (status !== "ready") {
      setFadeDone(false);
      return;
    }
    if (fadeDone) return;
    const timer = window.setTimeout(() => setFadeDone(true), 220);
    return () => window.clearTimeout(timer);
  }, [fadeDone, status]);

  const handleImgLoad = useCallback(() => {
    const img = imgRef.current;
    const expected = activeSrcRef.current;
    if (!expected) return;
    if (img && typeof img.decode === "function") {
      img.decode().then(
        () => {
          if (activeSrcRef.current === expected) markLoaded();
        },
        () => {
          if (activeSrcRef.current === expected) markLoaded();
        },
      );
    } else {
      markLoaded();
    }
  }, [markLoaded]);

  useLayoutEffect(() => {
    const img = imgRef.current;
    if (activeSrc && status === "loading" && img && img.complete && img.naturalWidth > 0) {
      handleImgLoad();
    }
  }, [activeSrc, handleImgLoad, status]);

  const handleTransitionEnd = useCallback(
    (e: ReactTransitionEvent<HTMLImageElement>) => {
      if (e.propertyName === "opacity" && status === "ready") {
        setFadeDone(true);
      }
    },
    [status],
  );

  return {
    imgRef,
    activeSrc,
    status,
    showSkeleton: Boolean(src && status !== "error" && !(status === "ready" && fadeDone)),
    handleImgLoad,
    markFailed,
    handleTransitionEnd,
  };
}

export const MediaCard = memo(function MediaCard(props: {
  title: string;
  imageSrc: string | null;
  fallbackImageSrcs?: string[];
  onImageExhausted?: () => void;
  badge?: string | null;
  overlay?: string | null;
  tags?: string[];
  coverMarks?: { source: CatalogSource; label: string }[];
  meta?: ReactNode;
  installed?: boolean;
  actions?: ReactNode;
  coverActions?: ReactNode;
  coverLabel?: string | null;
  hoverLabel?: string | null;
  unsupported?: boolean;
  /** The game currently open in the app. Library grids keep this title at full strength. */
  active?: boolean;
  /** Library grids are small. Eager avoids WebView2 leaving lazy images blank until a click. */
  loading?: "eager" | "lazy";
  onClick?: () => void;
  enterIndex?: number;
  /** Skip the motion wrapper. Browse virtualization mounts cards while scrolling. */
  plain?: boolean;
}) {
  const {
    title,
    imageSrc,
    fallbackImageSrcs,
    onImageExhausted,
    badge,
    overlay,
    tags,
    coverMarks,
    meta,
    installed,
    actions,
    coverActions,
    coverLabel,
    hoverLabel,
    unsupported,
    active,
    loading = "lazy",
    onClick,
    enterIndex,
    plain,
  } = props;
  const [fallbackIndex, setFallbackIndex] = useState(0);
  const exhaustedRef = useRef(false);
  const onImageExhaustedRef = useRef(onImageExhausted);
  onImageExhaustedRef.current = onImageExhausted;

  useEffect(() => {
    setFallbackIndex(0);
    exhaustedRef.current = false;
  }, [imageSrc]);

  const currentImageSrc =
    fallbackIndex === 0
      ? imageSrc
      : (fallbackImageSrcs?.[fallbackIndex - 1] ?? null);

  const {
    imgRef,
    activeSrc,
    status,
    showSkeleton,
    handleImgLoad,
    markFailed,
    handleTransitionEnd,
  } = useDecodedImageReveal(currentImageSrc);

  useEffect(() => {
    if (status !== "error" || !currentImageSrc) return;
    const totalFallbacks = fallbackImageSrcs?.length ?? 0;
    if (fallbackIndex < totalFallbacks) {
      setFallbackIndex((idx) => idx + 1);
      return;
    }
    if (!exhaustedRef.current) {
      exhaustedRef.current = true;
      onImageExhaustedRef.current?.();
    }
  }, [status, currentImageSrc, fallbackIndex, fallbackImageSrcs]);

  const [glowActive, setGlowActive] = useState(false);
  const [glowMounted, setGlowMounted] = useState(false);
  const [glowVisible, setGlowVisible] = useState(false);
  const [glowHost, setGlowHost] = useState<HTMLElement | null>(null);
  const stackRef = useRef<HTMLDivElement | null>(null);
  const glowRef = useRef<HTMLImageElement | null>(null);

  const coverSrc = currentImageSrc && status !== "error" ? currentImageSrc : null;
  const canGlow = Boolean(onClick && coverSrc && status === "ready");

  const activateGlow = useCallback(() => {
    setGlowHost(stackRef.current?.closest<HTMLElement>(".panel") ?? null);
    setGlowActive(true);
  }, []);

  const deactivateGlow = useCallback(() => {
    setGlowActive(false);
  }, []);

  useEffect(() => {
    if (!canGlow) {
      setGlowActive(false);
      setGlowVisible(false);
      setGlowMounted(false);
      setGlowHost(null);
      return;
    }
    if (glowActive) {
      setGlowMounted(true);
      let raf2 = 0;
      const raf1 = window.requestAnimationFrame(() => {
        raf2 = window.requestAnimationFrame(() => {
          setGlowVisible(true);
        });
      });
      return () => {
        window.cancelAnimationFrame(raf1);
        window.cancelAnimationFrame(raf2);
      };
    }
    setGlowVisible(false);
    if (!glowMounted) return;
    const timer = window.setTimeout(() => {
      setGlowMounted(false);
    }, 550);
    return () => window.clearTimeout(timer);
  }, [canGlow, glowActive, glowMounted]);

  useLayoutEffect(() => {
    if (!canGlow || !glowMounted || !glowHost || !glowHost.isConnected) return;
    const syncPosition = () => {
      const stackEl = stackRef.current;
      const glowEl = glowRef.current;
      if (!stackEl || !glowEl || !glowHost.isConnected) return;
      const stackRect = stackEl.getBoundingClientRect();
      const hostRect = glowHost.getBoundingClientRect();
      const width = stackRect.width * 1.36;
      const height = stackRect.height * 1.36;
      const left = stackRect.left - hostRect.left - glowHost.clientLeft - stackRect.width * 0.18;
      const top = stackRect.top - hostRect.top - glowHost.clientTop - stackRect.height * 0.18;
      glowEl.style.left = `${left}px`;
      glowEl.style.top = `${top}px`;
      glowEl.style.width = `${width}px`;
      glowEl.style.height = `${height}px`;
    };
    syncPosition();
    let rafId = 0;
    const scheduleSync = () => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        syncPosition();
      });
    };
    glowHost.addEventListener("scroll", scheduleSync, { capture: true, passive: true });
    window.addEventListener("resize", scheduleSync, { passive: true });
    return () => {
      glowHost.removeEventListener("scroll", scheduleSync, { capture: true });
      window.removeEventListener("resize", scheduleSync);
      if (rafId) window.cancelAnimationFrame(rafId);
    };
  }, [canGlow, glowMounted, glowHost]);

  const useGlowPortal = Boolean(glowHost && glowHost.isConnected);
  const glowImg =
    canGlow && glowMounted ? (
      <img
        ref={glowRef}
        className={[
          "media-cover-glow",
          useGlowPortal ? "media-cover-glow-portal" : "",
          glowVisible ? "is-visible" : "",
        ]
          .filter(Boolean)
          .join(" ")}
        src={coverSrc!}
        alt=""
        aria-hidden="true"
        decoding="async"
        onTransitionEnd={(e) => {
          if (e.propertyName === "opacity" && !glowActive) {
            setGlowMounted(false);
          }
        }}
      />
    ) : null;

  const classes = [
    "media-card",
    onClick ? "media-card-clickable" : "",
    unsupported ? "media-card-unsupported" : "",
    active ? "is-active" : "",
  ]
    .filter(Boolean)
    .join(" ");
  const onKeyDown = onClick
    ? (event: ReactKeyboardEvent<HTMLElement>) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          onClick();
        }
      }
    : undefined;
  const body = (
    <>
      <div
        ref={stackRef}
        className="media-cover-stack"
        onMouseEnter={onClick ? activateGlow : undefined}
        onMouseLeave={onClick ? deactivateGlow : undefined}
        onFocus={onClick ? activateGlow : undefined}
        onBlur={onClick ? deactivateGlow : undefined}
      >
        {glowImg ? (useGlowPortal ? createPortal(glowImg, glowHost!) : glowImg) : null}
        <div className="media-cover">
          {showSkeleton ? (
            <div className="media-cover-skeleton skeleton" aria-hidden="true" />
          ) : null}
          {overlay || badge ? (
            <div className="media-cover-top-left">
              {overlay ? <span className="media-overlay">{overlay}</span> : null}
              {badge ? <span className="media-badge">{badge}</span> : null}
            </div>
          ) : null}
          {installed ? (
            <span className="media-status" title="Installed" aria-label="Installed">
              <FontAwesomeIcon icon={faCheck} aria-hidden="true" />
            </span>
          ) : null}
          {coverSrc ? (
            activeSrc ? (
              <img
                ref={imgRef}
                key={activeSrc}
                className={status === "ready" ? "cover-defer is-ready" : "cover-defer"}
                src={activeSrc}
                alt=""
                loading={loading}
                decoding="async"
                onLoad={handleImgLoad}
                onError={markFailed}
                onTransitionEnd={handleTransitionEnd}
              />
            ) : null
          ) : (
            <PlaceholderIcon />
          )}
          {coverMarks && coverMarks.length > 0 ? (
            <div className="media-cover-tags">
              {coverMarks.map((mark) => (
                <SourceMark key={mark.source} source={mark.source} label={mark.label} />
              ))}
            </div>
          ) : null}
          {coverLabel ? (
            <span className="media-cover-label">{coverLabel}</span>
          ) : null}
          {hoverLabel ? (
            <span className="media-cover-label media-cover-hover">{hoverLabel}</span>
          ) : null}
          {coverActions ? (
            <div
              className="media-cover-actions"
              onClick={(e) => e.stopPropagation()}
              onKeyDown={(e) => e.stopPropagation()}
            >
              {coverActions}
            </div>
          ) : null}
        </div>
      </div>
      <div className="media-card-title">{title}</div>
      {meta ? <div className="media-card-meta">{meta}</div> : null}
      {tags && tags.length > 0 ? (
        <div className="media-card-tags">
          {tags.map((t) => (
            <span key={t} className="pill">
              {t}
            </span>
          ))}
        </div>
      ) : null}
      {actions ? (
        <div className="actions" onClick={(e) => e.stopPropagation()} onKeyDown={(e) => e.stopPropagation()}>
          {actions}
        </div>
      ) : null}
    </>
  );
  if (plain) {
    return (
      <article
        className={classes}
        onClick={onClick}
        onKeyDown={onKeyDown}
        role={onClick ? "button" : undefined}
        tabIndex={onClick ? 0 : undefined}
      >
        {body}
      </article>
    );
  }
  return (
    <MotionCard
      className={classes}
      enterIndex={enterIndex}
      onClick={onClick}
      onKeyDown={onKeyDown}
      role={onClick ? "button" : undefined}
      tabIndex={onClick ? 0 : undefined}
    >
      {body}
    </MotionCard>
  );
});
