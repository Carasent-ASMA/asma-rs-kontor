import { Icon } from "@getpaseo/plugin/client/react-native";
import { useId, useMemo } from "react";
import type { Colors } from "./page-controls";
import { surfaceIsDark } from "./session-status-ink";

/**
 * The running mark keeps the host's exact Lucide geometry (lucide-react 1.50.0, the pinned renderer:
 * Play and Building2, viewBox 24, stroke-width 2, round caps and joins); only the paint changes.
 * The rainbow is one linear gradient wrapped violet → … → blue → violet, so the wrapped ends meet
 * without a seam while frames rotate about the centre.
 */
export const RUNNING_MARK_PATHS = {
  play: ["M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"],
  building: ["M10 12h4", "M10 8h4", "M14 21v-3a2 2 0 0 0-4 0v3", "M6 10H4a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-2", "M6 21V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v16"],
} as const;

export type RunningMarkShape = keyof typeof RUNNING_MARK_PATHS;

/** The composer keeps Building2; session rows and controls use the running filter's Play. */
export function runningMarkShape(name: string): RunningMarkShape | undefined {
  return name === "Building2" ? "building" : name === "Play" ? "play" : undefined;
}

type RainbowStops = readonly [string, string, string, string, string, string, string, string];

/** Light and dark stop sets; every stop keeps ≥3:1 non-text contrast on surface0/1/2, host tokens and the
 * Kontor appearance overrides alike (WCAG 1.4.11). The light orange is deepened for Kontor's #e6ebf2 chrome. */
export const RAINBOW_STOPS = {
  light: ["#7c3aed", "#db2777", "#dc2626", "#c2410c", "#a16207", "#15803d", "#0d9488", "#2563eb"],
  dark: ["#a78bfa", "#f472b6", "#f87171", "#fb923c", "#facc15", "#4ade80", "#2dd4bf", "#60a5fa"],
} as const satisfies Record<"light" | "dark", RainbowStops>;

/** Frames pre-render the gradient rotated 360/K about the centre; frame 0 is the resting rainbow. */
export const RAINBOW_FRAMES = 6;
const BASE_ANGLE = 20;

/** Ids are themed, so light and dark marks on one screen keep distinct gradients; repeats stay identical. */
export function rainbowThemeKey(colors: Colors): "light" | "dark" {
  return surfaceIsDark(colors) ? "dark" : "light";
}

/**
 * One gradient id per theme, frame and mounted instance: identical ids across N marks would be invalid SVG
 * and would silently paint the first mark's gradient if per-mark tuning ever landed (review-b N1). The
 * instance part is the component's React useId with non-alphanumerics dropped, stable per mounted instance.
 */
export function rainbowGradientId(theme: "light" | "dark", frame: number, instance?: string): string {
  const suffix = instance ? `-${instance.replace(/[^a-zA-Z0-9]/g, "")}` : "";
  return `kontor-rainbow-${theme}-${frame}${suffix}`;
}

/** Inline SVG for one frame: the host renders it through SvgXml at `size` and validates the markup. */
export function rainbowMarkSvg(shape: RunningMarkShape, colors: Colors, frame: number, instance?: string): string {
  const theme = rainbowThemeKey(colors);
  const stops = RAINBOW_STOPS[theme];
  const gradient = rainbowGradientId(theme, frame, instance);
  const angle = BASE_ANGLE + (360 / RAINBOW_FRAMES) * frame;
  const stopsMarkup = stops.map((color, index) => `<stop offset="${(index / stops.length).toFixed(3)}" stop-color="${color}"/>`).join("")
    + `<stop offset="1" stop-color="${stops[0]}"/>`;
  const paths = RUNNING_MARK_PATHS[shape].map((d) => `<path d="${d}"/>`).join("");
  // The gradient span runs 4..20 (not 0..24) so the wrapped violet ends are visible on the stroke at both
  // extremes, matching Igor's reference frame; rotation stays about the centre (review-b S1).
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="url(#${gradient})"`
    + ` stroke-width="2" stroke-linecap="round" stroke-linejoin="round">`
    + `<linearGradient id="${gradient}" x1="4" y1="12" x2="20" y2="12" gradientUnits="userSpaceOnUse"`
    + ` gradientTransform="rotate(${angle.toFixed(2)} 12 12)">${stopsMarkup}</linearGradient>${paths}</svg>`;
}

/**
 * Compositing windows for one frame on the shared clock, so the visible stroke is ALWAYS a full-coverage
 * opaque blend of two frames (WCAG 1.4.11): during a handoff the outgoing frame stays opaque beneath and
 * only the incoming topmost layer fades in — two half-transparent layers would drop coverage to 0.75 and
 * thin the light-theme stroke below 3:1. Frames render bottom-to-top in index order; the wrap handoff
 * (K-1 → 0) therefore fades the TOPMOST frame out over the always-opaque frame 0 beneath it. Every drop
 * happens beneath an already-opaque layer, where it is invisible.
 */
export function rainbowFrameInterpolation(frame: number, frames: number): { inputRange: number[]; outputRange: number[]; extrapolate: "clamp" } {
  const at = (step: number) => Number((step / frames).toFixed(6));
  const after = (step: number) => Number((step / frames + 1e-4).toFixed(6));
  // Degenerate stacks (frames < 3) keep every layer solid: the input range stays monotonic for every K >= 1
  // (Animated rejects a non-monotonic range) and the stack alpha stays exactly 1, with the topmost frame showing.
  if (frames < 3) return { inputRange: [0, 1], outputRange: [1, 1], extrapolate: "clamp" };
  if (frame === 0) return { inputRange: [0, at(1), after(1), at(frames - 1), 1], outputRange: [1, 1, 0, 1, 1], extrapolate: "clamp" };
  if (frame === frames - 1) return { inputRange: [at(frame - 1), at(frame), 1], outputRange: [0, 1, 0], extrapolate: "clamp" };
  return { inputRange: [at(frame - 1), at(frame), after(frame + 1), 1], outputRange: [0, 1, 1, 0], extrapolate: "clamp" };
}

/** The resting rainbow for controls, legends and reduced motion: one static frame, no clock. */
export function RunningRainbowMark({ colors, size, shape }: { colors: Colors; size: number; shape: RunningMarkShape }) {
  const instance = useId();
  const svg = useMemo(() => rainbowMarkSvg(shape, colors, 0, instance), [shape, colors, instance]);
  return <Icon icon={{ svg }} size={size} />;
}
