import type { Colors } from "./page-controls";
import type { OrganizationState } from "./session-organization";
import type { StatusFilter } from "./session-view";

export const KONTOR_BLUE_LIGHT = "#245bba";
export const KONTOR_BLUE_DARK = "#9cbfff";

/** Reuse Kontor's appearance blues: a host's CTA accent can be green. */
export function composerRunningInk(colors: Colors): string {
  return readableInk(luminance(colors.surface0) < 0.5 ? KONTOR_BLUE_DARK : KONTOR_BLUE_LIGHT, colors);
}

function statusColor(status: StatusFilter | null, colors: Colors): string {
  switch (status) {
    case "running": return colors.accent;
    case "error": return colors.statusDanger;
    case "needs_input": return colors.statusWarning;
    case "attention": return colors.statusSuccess;
    case "all": case "favorites": return colors.foreground;
    default: return colors.foregroundMuted;
  }
}

/** Preserve the existing semantic palette for non-icon status cues. */
export function stateColor(state: OrganizationState | null, colors: Colors): string {
  return statusColor(state === "failed" ? "error" : state, colors);
}

function luminance(color: string): number {
  const hex = color.length === 4 ? color.slice(1).split("").map(value => value + value).join("") : color.slice(1);
  const value = Number.parseInt(hex, 16);
  return [16, 8, 0].reduce((sum, shift, index) => {
    const channel = ((value >> shift) & 255) / 255;
    const linear = channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    return sum + linear * [0.2126, 0.7152, 0.0722][index]!;
  }, 0);
}

/** `ink` where it keeps `min` on every row/control surface, otherwise the foreground: 3:1 for an icon or mark (WCAG 1.4.11), 4.5:1 for text (1.4.3). */
export function readableInk(ink: string, colors: Colors, min = 3): string {
  // A theme that names no ink has nothing to measure; the foreground is the readable choice.
  if (!ink) return colors.foreground;
  const foreground = luminance(ink);
  const readable = [colors.surface0, colors.surface1, colors.surface2].filter(Boolean).every(surface => {
    const background = luminance(surface);
    return (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05) >= min;
  });
  return readable ? ink : colors.foreground;
}

/** Selection keeps its border/check; status icons share readable ink on all row/control surfaces. */
export function sessionStatusInk(status: StatusFilter, colors: Colors): string {
  return readableInk(statusColor(status, colors), colors);
}
