import type { Colors } from "./page-controls";
import type { OrganizationState } from "./session-organization";
import type { StatusFilter } from "./session-view";

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

/** Selection keeps its border/check; status icons share readable ink on all row/control surfaces. */
export function sessionStatusInk(status: StatusFilter, colors: Colors): string {
  const ink = statusColor(status, colors);
  const foreground = luminance(ink);
  const readable = [colors.surface0, colors.surface1, colors.surface2].every(surface => {
    const background = luminance(surface);
    return (Math.max(foreground, background) + 0.05) / (Math.min(foreground, background) + 0.05) >= 3;
  });
  return readable ? ink : colors.foreground;
}
