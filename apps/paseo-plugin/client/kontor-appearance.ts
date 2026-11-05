import type { Colors } from "./page-controls";
import { KONTOR_BLUE_DARK, KONTOR_BLUE_LIGHT } from "./session-status-ink";

/**
 * The Kontor appearance override: the plugin's own surfaces for a saved Light or Dark choice, replacing the
 * host tokens for every Kontor surface. Marks drawn on Kontor chrome sit on THESE surfaces, so contrast
 * gates must check them next to the host tokens. `system` keeps the host theme untouched.
 */
export function appearanceColors(colors: Colors, appearance: string): Colors {
  if (appearance === "dark") return { ...colors, foreground: "#ecedf0", foregroundMuted: "#a8adb7", surface0: "#15171c", surface1: "#1f222a", surface2: "#2e333e", border: "#414754", accent: KONTOR_BLUE_DARK };
  if (appearance === "light") return { ...colors, foreground: "#20242c", foregroundMuted: "#5e6570", surface0: "#ffffff", surface1: "#f5f6f8", surface2: "#e6ebf2", border: "#c7cdd7", accent: KONTOR_BLUE_LIGHT };
  return colors;
}
