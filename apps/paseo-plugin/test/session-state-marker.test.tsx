import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ View: "View", Text: "Text" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { SessionStateMarker, SessionStatusIcon } from "../client/session-state-marker";
import { sessionStatusInk } from "../client/session-status-ink";
import { QUICK_FILTER_ICONS } from "../client/mobile-quick-filter-options";
import type { StatusFilter } from "../client/session-view";
import { STATE_LABELS, type OrganizationState } from "../client/session-organization";
import type { Colors } from "../client/page-controls";

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
type Element = ReactElement<Record<string, unknown>>;
const states = Object.keys(STATE_LABELS) as OrganizationState[];
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...elements(node.props.children as ReactNode)];
}
function drawing(state: OrganizationState, colors = dark) {
  const adapted = SessionStateMarker({ state, colors });
  const root = SessionStatusIcon(adapted.props);
  const symbol = elements(root).find(node => node.type === "Icon")!;
  return { root, symbol, color: symbol.props.color as string };
}
function luminance(hex: string): number {
  const channels = hex.match(/[a-f\d]{2}/gi)!.map(value => parseInt(value, 16) / 255);
  const [r, g, b] = channels.map(channel => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
}
function contrast(a: string, b: string): number {
  const values = [luminance(a), luminance(b)].sort((x, y) => x - y);
  return (values[1]! + 0.05) / (values[0]! + 0.05);
}

describe("compact session status markers", () => {
  it("uses the agreed session icons and keeps history and filter choices distinct", () => {
    expect(QUICK_FILTER_ICONS).toEqual({
      current: "Activity", all: "LayoutGrid", running: "Play", idle: "Pause", error: "CircleAlert",
      needs_input: "MessageCircleQuestion", attention: "Bell", favorites: "Star",
      initializing: "Loader", closed: "Square", earlier_error: "History",
    });
  });
  it("distinguishes all seven states without hue or repeated status words", () => {
    const monochrome = { ...dark, accent: dark.foregroundMuted, statusSuccess: dark.foregroundMuted, statusWarning: dark.foregroundMuted, statusDanger: dark.foregroundMuted };
    const drawings = states.map(state => drawing(state, monochrome));
    expect(new Set(drawings.map(value => value.symbol.props.name)).size).toBe(7);
    expect(drawings.map(value => value.symbol.props.name)).toEqual(states.map(state => QUICK_FILTER_ICONS[state === "failed" ? "error" : state]));
    for (const { root, symbol } of drawings) {
      expect(symbol.props.children).toBeUndefined();
      expect(root.props.style).toMatchObject({ width: 16, height: 18, flexShrink: 0 });
    }
  });

  it.each([dark, light])("shares exact quick-filter icon identity and semantic ink without selection-dependent color", colors => {
    for (const status of Object.keys(QUICK_FILTER_ICONS) as StatusFilter[]) {
      const root = SessionStatusIcon({ status, colors });
      const icon = elements(root).find(node => node.type === "Icon")!;
      expect(icon.props).toMatchObject({ name: QUICK_FILTER_ICONS[status], color: sessionStatusInk(status, colors) });
    }
    expect(sessionStatusInk("all", colors)).toBe(colors.foreground);
    expect(sessionStatusInk("favorites", colors)).toBe(colors.foreground);
    expect(sessionStatusInk("earlier_error", colors)).toBe(colors.foregroundMuted);
    expect(sessionStatusInk("earlier_error", colors)).not.toBe(sessionStatusInk("error", colors));
  });

  it("adds no focus target, control or duplicate spoken status to a session row", () => {
    for (const state of states) {
      const { root } = drawing(state);
      expect(root.props).toMatchObject({ accessible: false, accessibilityElementsHidden: true, importantForAccessibility: "no-hide-descendants", "aria-hidden": true });
      for (const node of elements(root)) {
        expect(node.props.tabIndex).toBeUndefined();
        expect(node.props.accessibilityLabel).toBeUndefined();
        expect(node.props.onPress).toBeUndefined();
        expect(node.props.accessibilityRole).toBeUndefined();
      }
    }
  });

  it.each([dark, light])("keeps every meaningful shape at least 3:1 on every row surface", colors => {
    for (const state of states) {
      const { color } = drawing(state, colors);
      for (const surface of [colors.surface0, colors.surface1, colors.surface2]) expect(contrast(color, surface)).toBeGreaterThanOrEqual(3);
    }
  });

  it("uses a contrasting theme token when the dark accent would disappear on hover", () => {
    expect(contrast(dark.accent, dark.surface2)).toBeLessThan(3);
    expect(drawing("running", dark).color).toBe(dark.foreground);
    expect(drawing("running", light).color).toBe(light.accent);
    expect(drawing("failed", dark).color).toBe(dark.statusDanger);
  });
});
