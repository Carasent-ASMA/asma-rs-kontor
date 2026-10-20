import type { PluginTheme } from "@getpaseo/plugin";

/**
 * Paseo 0.11.4 theme colours as the app passes them to plugins (toPluginTheme
 * over the bundled default dark and light themes). Fonts are not part of the
 * plugin theme; the web build uses the browser's system UI font.
 */
export const PASEO_DARK: PluginTheme["colors"] = {
  surface0: "#181B1A", surface1: "#1E2120", surface2: "#272A29", border: "#252B2A",
  foreground: "#fafafa", foregroundMuted: "#A1A5A4", accent: "#20744A", accentForeground: "#ffffff",
  statusSuccess: "#6cb17b", statusWarning: "#c09664", statusDanger: "#d8847b",
};
export const PASEO_LIGHT: PluginTheme["colors"] = {
  surface0: "#ffffff", surface1: "#fafafa", surface2: "#f4f4f5", border: "#e4e4e7",
  foreground: "#1a1a1e", foregroundMuted: "#71717a", accent: "#20744A", accentForeground: "#ffffff",
  statusSuccess: "#3e704a", statusWarning: "#7b5d39", statusDanger: "#9d433b",
};

const params = new URLSearchParams(window.location.search);
export const themeName: "dark" | "light" = params.get("theme") === "light" ? "light" : "dark";
export function shellColors(): PluginTheme["colors"] {
  return themeName === "light" ? PASEO_LIGHT : PASEO_DARK;
}
