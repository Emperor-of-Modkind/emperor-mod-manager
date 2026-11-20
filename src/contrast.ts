/** WCAG content colors for accent fills, accent text, and muted labels. */

export const MIN_CONTRAST = 4.5;

type RGB = [number, number, number];

function channel(c: number): number {
  const s = c / 255;
  return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
}

export function parseColor(input: string): RGB | null {
  const v = input.trim().toLowerCase();
  const hex = /^#([0-9a-f]{3}|[0-9a-f]{6})$/.exec(v);
  if (hex) {
    const h = hex[1];
    if (h.length === 3) {
      return [
        parseInt(h[0] + h[0], 16),
        parseInt(h[1] + h[1], 16),
        parseInt(h[2] + h[2], 16),
      ];
    }
    return [
      parseInt(h.slice(0, 2), 16),
      parseInt(h.slice(2, 4), 16),
      parseInt(h.slice(4, 6), 16),
    ];
  }
  const rgb = /^rgba?\(\s*([\d.]+)[,\s]+([\d.]+)[,\s]+([\d.]+)/.exec(v);
  if (!rgb) return null;
  return [Math.round(Number(rgb[1])), Math.round(Number(rgb[2])), Math.round(Number(rgb[3]))];
}

export function toHex(rgb: RGB): string {
  const h = (n: number) =>
    Math.max(0, Math.min(255, Math.round(n)))
      .toString(16)
      .padStart(2, "0");
  return `#${h(rgb[0])}${h(rgb[1])}${h(rgb[2])}`;
}

export function relativeLuminance(rgb: RGB): number {
  return 0.2126 * channel(rgb[0]) + 0.7152 * channel(rgb[1]) + 0.0722 * channel(rgb[2]);
}

export function contrastRatio(a: RGB, b: RGB): number {
  const l1 = relativeLuminance(a);
  const l2 = relativeLuminance(b);
  const [hi, lo] = l1 >= l2 ? [l1, l2] : [l2, l1];
  return (hi + 0.05) / (lo + 0.05);
}

function mix(a: RGB, b: RGB, t: number): RGB {
  const lerp = (x: number, y: number) => Math.round(x * (1 - t) + y * t);
  return [lerp(a[0], b[0]), lerp(a[1], b[1]), lerp(a[2], b[2])];
}

function rgbToHsl(rgb: RGB): [number, number, number] {
  const r = rgb[0] / 255;
  const g = rgb[1] / 255;
  const b = rgb[2] / 255;
  const max = Math.max(r, g, b);
  const min = Math.min(r, g, b);
  const l = (max + min) / 2;
  if (max === min) return [0, 0, l];
  const d = max - min;
  const s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
  let h: number;
  if (max === r) h = (g - b) / d + (g < b ? 6 : 0);
  else if (max === g) h = (b - r) / d + 2;
  else h = (r - g) / d + 4;
  return [h / 6, s, l];
}

function hue2rgb(p: number, q: number, t: number): number {
  let x = t;
  if (x < 0) x += 1;
  if (x > 1) x -= 1;
  if (x < 1 / 6) return p + (q - p) * 6 * x;
  if (x < 1 / 2) return q;
  if (x < 2 / 3) return p + (q - p) * (2 / 3 - x) * 6;
  return p;
}

function hslToRgb(h: number, s: number, l: number): RGB {
  if (s <= 0) {
    const v = Math.round(l * 255);
    return [v, v, v];
  }
  const q = l < 0.5 ? l * (1 + s) : l + s - l * s;
  const p = 2 * l - q;
  return [
    Math.round(hue2rgb(p, q, h + 1 / 3) * 255),
    Math.round(hue2rgb(p, q, h) * 255),
    Math.round(hue2rgb(p, q, h - 1 / 3) * 255),
  ];
}

/** Move `start` toward `extreme` until it clears `min` against `fill`. */
function ensureContrast(start: RGB, extreme: RGB, fill: RGB, min: number): RGB {
  if (contrastRatio(start, fill) >= min) return start;
  if (contrastRatio(extreme, fill) < min) return extreme;
  let lo = 0;
  let hi = 1;
  let best = extreme;
  for (let i = 0; i < 18; i++) {
    const t = (lo + hi) / 2;
    const sample = mix(start, extreme, t);
    if (contrastRatio(sample, fill) >= min) {
      best = sample;
      hi = t;
    } else {
      lo = t;
    }
  }
  return best;
}

function darkTone(fill: RGB): RGB {
  const [h, s] = rgbToHsl(fill);
  const sat = s < 0.08 ? 0 : Math.min(0.72, Math.max(0.4, s));
  return hslToRgb(h, sat, 0.16);
}

function lightTone(fill: RGB): RGB {
  const [h, s] = rgbToHsl(fill);
  return hslToRgb(h, Math.min(s, 0.14), 0.97);
}

/**
 * Label on an accent fill.
 * A toolkit foreground is kept when it already clears 4.5:1.
 * Otherwise light text is used when it clears that floor (dark fills);
 * light fills get a same-hue dark tone, which is the stronger pair.
 */
export function onAccent(fill: string, osForeground?: string | null): string {
  const bg = parseColor(fill);
  if (!bg) return "#ffffff";
  if (osForeground) {
    const os = parseColor(osForeground);
    if (os && contrastRatio(os, bg) >= MIN_CONTRAST) return toHex(os);
  }
  const light = ensureContrast(lightTone(bg), [255, 255, 255], bg, MIN_CONTRAST);
  if (contrastRatio(light, bg) >= MIN_CONTRAST) return toHex(light);
  const dark = ensureContrast(darkTone(bg), [0, 0, 0], bg, MIN_CONTRAST);
  return toHex(dark);
}

/** Accent shifted in lightness until it clears 4.5:1 on every surface. */
export function accentInk(accent: string, surfaces: string[]): string {
  const color = parseColor(accent);
  const surfs = surfaces
    .map(parseColor)
    .filter((c): c is RGB => c !== null);
  if (!color || surfs.length === 0) return accent;
  const minOf = (c: RGB) => Math.min(...surfs.map((s) => contrastRatio(c, s)));
  if (minOf(color) >= MIN_CONTRAST) return toHex(color);

  const [h, s, l] = rgbToHsl(color);
  let best = color;
  let bestMin = minOf(color);
  let bestDist = Infinity;
  for (let i = 0; i <= 48; i++) {
    const nl = i / 48;
    const ns = s * (0.65 + 0.35 * (1 - Math.min(1, Math.abs(nl - 0.5) * 2)));
    const sample = hslToRgb(h, Math.min(1, Math.max(0, ns)), nl);
    const score = minOf(sample);
    const dist = Math.abs(nl - l);
    const passes = score >= MIN_CONTRAST;
    const bestPasses = bestMin >= MIN_CONTRAST;
    if ((passes && (!bestPasses || dist < bestDist)) || (!bestPasses && score > bestMin)) {
      best = sample;
      bestMin = score;
      bestDist = dist;
    }
  }
  return toHex(best);
}

/** Pull muted toward ink only when it misses 4.5:1 on a surface. */
export function repairMuted(muted: string, bg: string, panel: string, ink: string): string {
  const m = parseColor(muted);
  const inkRgb = parseColor(ink);
  const surfs = [bg, panel].map(parseColor).filter((c): c is RGB => c !== null);
  if (!m || !inkRgb || surfs.length === 0) return muted;
  const passes = (c: RGB) => surfs.every((s) => contrastRatio(c, s) >= MIN_CONTRAST);
  if (passes(m)) return toHex(m);
  for (let i = 1; i <= 24; i++) {
    const mixed = mix(m, inkRgb, i / 24);
    if (passes(mixed)) return toHex(mixed);
  }
  return toHex(inkRgb);
}

/**
 * Hover fill for an accent control.
 * Prefers mixing 18% toward white (82% accent). Mixes toward black when
 * that would drop `--on-accent` under 4.5:1.
 */
export function accentHoverFill(accent: string, onAccentColor: string): string {
  const fill = parseColor(accent);
  const ink = parseColor(onAccentColor);
  if (!fill || !ink) return accent;
  const lightened = mix(fill, [255, 255, 255], 0.18);
  if (contrastRatio(ink, lightened) >= MIN_CONTRAST) return toHex(lightened);
  for (let i = 1; i <= 20; i++) {
    const darkened = mix(fill, [0, 0, 0], i / 20);
    if (contrastRatio(ink, darkened) >= MIN_CONTRAST) return toHex(darkened);
  }
  return "#000000";
}

export type ContentRoles = {
  onAccent: string;
  accentInk: string;
  muted: string;
  accentHover: string;
};

export function contentRoles(input: {
  accent: string;
  bg: string;
  panel: string;
  ink: string;
  muted: string;
  osOnAccent?: string | null;
}): ContentRoles {
  const on = onAccent(input.accent, input.osOnAccent);
  return {
    onAccent: on,
    accentInk: accentInk(input.accent, [input.bg, input.panel]),
    muted: repairMuted(input.muted, input.bg, input.panel, input.ink),
    accentHover: accentHoverFill(input.accent, on),
  };
}
