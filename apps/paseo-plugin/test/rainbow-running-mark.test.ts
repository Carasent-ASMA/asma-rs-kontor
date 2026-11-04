import { findSvgIconProblem } from "@getpaseo/plugin";
import { describe, expect, it } from "vitest";
import type { Colors } from "../client/page-controls";
import { RAINBOW_FRAMES, RAINBOW_STOPS, RUNNING_MARK_PATHS, rainbowFrameInterpolation, rainbowMarkSvg, rainbowThemeKey, runningMarkShape } from "../client/running-mark";

const dark: Colors = {
  surface0: "#181B1A", surface1: "#1E2120", surface2: "#272A29", border: "#252B2A",
  foreground: "#fafafa", foregroundMuted: "#A1A5A4", accent: "#20744A", accentForeground: "#ffffff",
  statusSuccess: "#6cb17b", statusWarning: "#c09664", statusDanger: "#d8847b",
};
const light: Colors = {
  surface0: "#ffffff", surface1: "#fafafa", surface2: "#f4f4f5", border: "#e4e4e7",
  foreground: "#1a1a1e", foregroundMuted: "#71717a", accent: "#20744A", accentForeground: "#ffffff",
  statusSuccess: "#3e704a", statusWarning: "#7b5d39", statusDanger: "#9d433b",
};

// Transcribed from the pinned host renderer's lucide-react 1.50.0 dist (icons/play.mjs, icons/building-complex.mjs).
const HOST_LUCIDE = {
  play: ["M5 5a2 2 0 0 1 3.008-1.728l11.997 6.998a2 2 0 0 1 .003 3.458l-12 7A2 2 0 0 1 5 19z"],
  building: ["M10 12h4", "M10 8h4", "M14 21v-3a2 2 0 0 0-4 0v3", "M6 10H4a2 2 0 0 0-2 2v7a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2V9a2 2 0 0 0-2-2h-2", "M6 21V5a2 2 0 0 1 2-2h8a2 2 0 0 1 2 2v16"],
} as const;

function luminance(hex: string): number {
  const channels = hex.match(/[a-f\d]{2}/gi)!.map(value => parseInt(value, 16) / 255);
  const [r, g, b] = channels.map(channel => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
}
function contrast(a: string, b: string): number {
  const values = [luminance(a), luminance(b)].sort((x, y) => x - y);
  return (values[1]! + 0.05) / (values[0]! + 0.05);
}

/** The same piecewise-linear read Animated performs on the shared clock value. */
function opacityAt({ inputRange, outputRange }: { inputRange: number[]; outputRange: number[] }, value: number): number {
  if (value <= inputRange[0]!) return outputRange[0]!;
  for (let index = 1; index < inputRange.length; index++) {
    if (value <= inputRange[index]!) {
      const span = inputRange[index]! - inputRange[index - 1]!;
      const share = span === 0 ? 1 : (value - inputRange[index - 1]!) / span;
      return outputRange[index - 1]! + share * (outputRange[index]! - outputRange[index - 1]!);
    }
  }
  return outputRange[outputRange.length - 1]!;
}

describe("rainbow running mark", () => {
  it("passes the host's inline-SVG validator for every shape, theme and frame", () => {
    for (const shape of ["play", "building"] as const)
      for (const colors of [dark, light])
        for (let frame = 0; frame < RAINBOW_FRAMES; frame++)
          expect(findSvgIconProblem(rainbowMarkSvg(shape, colors, frame))).toBeNull();
  });

  it("keeps the host Lucide geometry and stroke attributes byte-for-byte", () => {
    expect(RUNNING_MARK_PATHS).toEqual(HOST_LUCIDE);
    for (const shape of ["play", "building"] as const) {
      const svg = rainbowMarkSvg(shape, dark, 0);
      for (const d of HOST_LUCIDE[shape]) expect(svg).toContain(`d="${d}"`);
      expect(svg).toContain('viewBox="0 0 24 24"');
      expect(svg).toContain('stroke-width="2" stroke-linecap="round" stroke-linejoin="round"');
      expect(svg).toContain('fill="none"');
    }
  });

  it("animates only by crossfading pre-rotated gradient frames: no SMIL, style or script", () => {
    for (const shape of ["play", "building"] as const)
      for (const colors of [dark, light])
        for (let frame = 0; frame < RAINBOW_FRAMES; frame++) {
          const svg = rainbowMarkSvg(shape, colors, frame);
          expect(svg).not.toMatch(/<animate/i);
          expect(svg).not.toMatch(/<style|<script/i);
        }
  });

  it("references its gradients internally only, with themed ids that keep repeats identical", () => {
    for (const shape of ["play", "building"] as const)
      for (const colors of [dark, light])
        for (let frame = 0; frame < RAINBOW_FRAMES; frame++) {
          const svg = rainbowMarkSvg(shape, colors, frame);
          expect(svg.match(/href/i)).toBeNull();
          const theme = rainbowThemeKey(colors);
          expect(svg).toContain(`stroke="url(#kontor-rainbow-${theme}-${frame})"`);
          expect(svg).toContain(`id="kontor-rainbow-${theme}-${frame}"`);
        }
    const gradientOf = (svg: string) => svg.match(/<linearGradient[\s\S]*<\/linearGradient>/)![0];
    expect(gradientOf(rainbowMarkSvg("play", dark, 2))).toBe(gradientOf(rainbowMarkSvg("building", dark, 2)));
    expect(rainbowMarkSvg("play", light, 2)).toContain("kontor-rainbow-light-2");
    expect(rainbowMarkSvg("play", dark, 2)).not.toContain("kontor-rainbow-light-2");
  });

  it.each([dark, light])("keeps every gradient stop at least 3:1 on every row surface in both themes", colors => {
    for (const stop of RAINBOW_STOPS[rainbowThemeKey(colors)])
      for (const surface of [colors.surface0, colors.surface1, colors.surface2]) expect(contrast(stop, surface)).toBeGreaterThanOrEqual(3);
  });

  it("picks the deeper amber on light surfaces and the brighter set on dark ones", () => {
    expect(RAINBOW_STOPS.light[4]).toBe("#a16207");
    expect(RAINBOW_STOPS.dark[4]).toBe("#facc15");
    expect(rainbowThemeKey(dark)).toBe("dark");
    expect(rainbowThemeKey(light)).toBe("light");
  });

  it("composites the K frames at full coverage so the rendered stroke never thins", () => {
    const windows = Array.from({ length: RAINBOW_FRAMES }, (_, frame) => rainbowFrameInterpolation(frame, RAINBOW_FRAMES));
    for (let step = 0; step <= 240; step++) {
      const value = step / 240;
      // Bottom-to-top alpha compositing of the mounted frames, as the stacked Animated.Views paint.
      let coverage = 0;
      for (let frame = 0; frame < RAINBOW_FRAMES; frame++) {
        const alpha = opacityAt(windows[frame]!, value);
        coverage = alpha + (1 - alpha) * coverage;
      }
      expect(coverage).toBeGreaterThan(1 - 1e-6);
    }
    // The loop seam is invisible: frame 0 holds the paint at both ends and every other frame is dropped beneath it.
    expect(opacityAt(windows[0]!, 0)).toBe(1);
    expect(opacityAt(windows[0]!, 1)).toBe(1);
    for (const window of windows.slice(1)) expect(opacityAt(window, 1)).toBe(0);
    // The outgoing layer stays opaque beneath the fading incoming one; blends only happen over full paint.
    for (const window of windows) {
      expect(opacityAt(window, 0)).toBeLessThanOrEqual(1);
      expect(window.extrapolate).toBe("clamp");
    }
  });

  it("keeps every blended handoff paint at least 3:1: the composite is an opaque mix of two readable frames", () => {
    // During a handoff the visible stroke is alpha-mixed from two consecutive frames' gradient stops over
    // surface0. Both endpoints are readable, and a mix of two same-side colors stays readable, so the
    // composited stroke keeps its contrast through the whole fade (WCAG 1.4.11).
    for (const colors of [dark, light]) {
      const surface = colors.surface0;
      const stopsOf = (frame: number) => [...rainbowMarkSvg("play", colors, frame).matchAll(/stop-color="([^"]+)"/g)].map((match) => match[1]!);
      for (let frame = 0; frame < RAINBOW_FRAMES; frame++) {
        const next = stopsOf((frame + 1) % RAINBOW_FRAMES);
        for (const from of stopsOf(frame)) for (const to of next) for (const alpha of [0, 0.25, 0.5, 0.75, 1]) {
          const mixed = "#" + [0, 2, 4].map((at) => Math.round(parseInt(from.slice(at + 1, at + 3), 16) * alpha + parseInt(to.slice(at + 1, at + 3), 16) * (1 - alpha))
            .toString(16).padStart(2, "0")).join("");
          expect(contrast(mixed, surface)).toBeGreaterThanOrEqual(3);
        }
      }
    }
  });

  it("pre-renders each frame as the same gradient rotated 360/K about the centre", () => {
    const angles = Array.from({ length: RAINBOW_FRAMES }, (_, frame) =>
      Number(rainbowMarkSvg("play", dark, frame).match(/rotate\(([\d.]+) 12 12\)/)![1]));
    expect(angles[0]).toBe(20);
    for (let frame = 1; frame < RAINBOW_FRAMES; frame++) {
      expect(angles[frame]! - angles[frame - 1]!).toBeCloseTo(360 / RAINBOW_FRAMES, 2);
      const rest = (svg: string) => svg.replace(/rotate\([\d.]+ 12 12\)/, "").replace(/rainbow-dark-\d/g, "");
      expect(rest(rainbowMarkSvg("play", dark, frame))).toBe(rest(rainbowMarkSvg("play", dark, frame - 1)));
    }
  });

  it("maps only the host's running glyphs to a shape and leaves every other icon alone", () => {
    expect(runningMarkShape("Play")).toBe("play");
    expect(runningMarkShape("Building2")).toBe("building");
    expect(runningMarkShape("Loader")).toBeUndefined();
    expect(runningMarkShape("CircleAlert")).toBeUndefined();
  });
});
