import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi, beforeEach } from "vitest";
vi.mock("react-native", () => ({ View: "View", Text: "Text" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
// SessionStateMarker reads the pulse context through useContext; outside a renderer the stub hands back
// the value each test sets (live by default, not-live where a badge must stay muted).
const pulseContext = vi.hoisted(() => ({ current: { live: true, visible: true } }));
vi.mock("react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("react")>();
  return { ...actual, useContext: () => pulseContext.current };
});
import { SessionStateMarker, SessionStatusIcon, runningMarkNode } from "../client/session-state-marker";
import { ComposerPulse } from "../client/composer-pulse";
import { RunningRainbowMark } from "../client/running-mark";
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
  const symbol = elements(root).find(node => node.type === "Icon" || node.type === ComposerPulse)!;
  return { root, symbol, color: symbol.props.color as string | undefined };
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
  beforeEach(() => { pulseContext.current = { live: true, visible: true }; });
  it("animates every running mark on the shared carrier: rows, filters and legends alike (D6)", () => {
    const marker = SessionStatusIcon({ status: "running", colors: dark, session: true });
    expect(elements(marker).some((node) => node.type === "Icon")).toBe(false);
    for (const status of Object.keys(QUICK_FILTER_ICONS) as StatusFilter[]) {
      const runningMark = SessionStatusIcon({ status, colors: dark });
      if (status === "running") {
        // D6 withdraws the static-control rule: the running filter/legend animates on the shared carrier.
        expect(elements(runningMark).some((node) => node.type === ComposerPulse)).toBe(true);
        expect(elements(runningMark).some((node) => node.type === RunningRainbowMark)).toBe(false);
        expect(elements(SessionStatusIcon({ status, colors: dark, session: true })).some((node) => node.type === "Icon")).toBe(false);
      } else {
        expect(elements(runningMark).some((node) => node.type === "Icon")).toBe(true);
        expect(elements(SessionStatusIcon({ status, colors: dark, session: true })).some((node) => node.type === "Icon")).toBe(true);
      }
    }
  });

  it("hands the running filter and legend to the shared pulse carrier with the Play shape, in every theme", () => {
    for (const colors of [dark, light]) {
      const mark = elements(SessionStatusIcon({ status: "running", colors })).find(node => node.type === ComposerPulse)!;
      expect(mark.props).toEqual({ colors, size: 16, name: "Play" });
    }
  });

  it("keeps last-known running muted on today's ink: the rainbow never claims live", () => {
    const muted = runningMarkNode({ live: false, visible: false }, dark, 22);
    expect(muted.type).toBe("Icon");
    expect(muted.props).toMatchObject({ name: QUICK_FILTER_ICONS.running, color: dark.foregroundMuted, size: 22 });
    const lastKnownRow = SessionStatusIcon({ status: "running", colors: dark, session: true });
    expect(elements(lastKnownRow).some((node) => node.type === RunningRainbowMark)).toBe(false);
  });

  it("hands a live running-session mark to the shared pulse, viewability on and off", () => {
    const live = runningMarkNode({ live: true, visible: true }, dark, 22);
    expect(live.type).toBe(ComposerPulse);
    expect(live.props).toEqual({ name: QUICK_FILTER_ICONS.running, colors: dark, size: 22, enabled: true });
    const hidden = runningMarkNode({ live: true, visible: false }, dark, 22);
    expect(hidden.type).toBe(ComposerPulse);
    expect(hidden.props).toEqual({ name: QUICK_FILTER_ICONS.running, colors: dark, size: 22, enabled: false });
  });

  it("keeps a stale group's running badge muted: Last known never animates (LSA-R2-S1)", () => {
    pulseContext.current = { live: false, visible: true };
    const stale = SessionStatusIcon({ status: "running", colors: dark, stale: true });
    expect(elements(stale).some((node) => node.type === ComposerPulse)).toBe(false);
    expect(elements(stale).some((node) => node.type === RunningRainbowMark)).toBe(false);
    const muted = elements(stale).find((node) => node.type === "Icon")!;
    expect(muted.props).toMatchObject({ name: QUICK_FILTER_ICONS.running, color: dark.foregroundMuted });
    // The muted branch is runningMarkNode's, not a duplicate.
    expect(muted.props).toMatchObject(runningMarkNode({ live: false, visible: true }, dark, 16).props);
    // Context liveness alone drives the gate: a not-live tree mutes the badge with no stale prop.
    const notLive = SessionStatusIcon(SessionStateMarker({ state: "running", colors: dark }).props);
    expect(elements(notLive).some((node) => node.type === ComposerPulse)).toBe(false);
    expect(elements(notLive).find((node) => node.type === "Icon")!.props).toMatchObject({ name: QUICK_FILTER_ICONS.running, color: dark.foregroundMuted });
  });

  it("keeps non-stale group running badges on the animated carrier (control for LSA-R2-S1)", () => {
    const fresh = SessionStatusIcon({ status: "running", colors: dark });
    expect(elements(fresh).some((node) => node.type === ComposerPulse)).toBe(true);
    expect(elements(fresh).some((node) => node.type === "Icon")).toBe(false);
    // A live tree animates the badge through the same context gate (D6 lists organization rows).
    const liveBadge = SessionStatusIcon(SessionStateMarker({ state: "running", colors: dark }).props);
    expect(elements(liveBadge).some((node) => node.type === ComposerPulse)).toBe(true);
    expect(elements(liveBadge).some((node) => node.type === "Icon")).toBe(false);
    // An explicit stale flag wins even over a live context: the banner's own condition.
    const overridden = SessionStatusIcon(SessionStateMarker({ state: "running", colors: dark, stale: true }).props);
    expect(elements(overridden).some((node) => node.type === ComposerPulse)).toBe(false);
    // Filter affordances keep the pulse regardless of any directory staleness.
    expect(elements(SessionStatusIcon({ status: "running", colors: dark, stale: false })).some((node) => node.type === ComposerPulse)).toBe(true);
  });

  it("uses the agreed session icons and keeps history and filter choices distinct", () => {
    expect(QUICK_FILTER_ICONS).toEqual({
      recent: "Clock", activity: "Activity", current: "Activity", all: "LayoutGrid", running: "Play", idle: "Pause", error: "CircleAlert",
      needs_input: "MessageCircleQuestion", attention: "Bell", favorites: "Star",
      initializing: "Loader", closed: "Square", earlier_error: "History", provider_blocked: "Ban",
    });
  });
  it("distinguishes all seven states without hue or repeated status words", () => {
    const monochrome = { ...dark, accent: dark.foregroundMuted, statusSuccess: dark.foregroundMuted, statusWarning: dark.foregroundMuted, statusDanger: dark.foregroundMuted };
    const drawings = states.map(state => drawing(state, monochrome));
    // Running carries the rainbow Play instead of a named glyph; its shape still separates it from all six inks.
    const identities = drawings.map(value => value.symbol.type === ComposerPulse ? "rainbow-play" : value.symbol.props.name);
    expect(new Set(identities).size).toBe(7);
    expect(identities).toEqual(states.map(state => state === "running" ? "rainbow-play" : QUICK_FILTER_ICONS[state === "failed" ? "error" : state]));
    for (const { root, symbol } of drawings) {
      expect(root.props.style).toMatchObject({ width: 16, height: 18, flexShrink: 0 });
      if (symbol.type !== ComposerPulse) expect(symbol.props.children).toBeUndefined();
    }
  });

  it.each([dark, light])("shares exact quick-filter icon identity and semantic ink without selection-dependent color", colors => {
    for (const status of Object.keys(QUICK_FILTER_ICONS) as StatusFilter[]) {
      const root = SessionStatusIcon({ status, colors });
      if (status === "running") continue; // running is the rainbow mark; its stops carry their own themed contrast
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

  it.each([dark, light])("keeps every inked shape at least 3:1 on every row surface", colors => {
    for (const state of states) {
      const { color } = drawing(state, colors);
      if (color === undefined) continue; // running paints with gradient stops, gated in rainbow-running-mark.test.ts
      for (const surface of [colors.surface0, colors.surface1, colors.surface2]) expect(contrast(color, surface)).toBeGreaterThanOrEqual(3);
    }
  });

  it("keeps a readable ink helper for any future accent consumer despite the rainbow paint", () => {
    expect(contrast(dark.accent, dark.surface2)).toBeLessThan(3);
    expect(sessionStatusInk("running", dark)).toBe(dark.foreground);
    expect(sessionStatusInk("running", light)).toBe(light.accent);
    expect(drawing("failed", dark).color).toBe(dark.statusDanger);
  });
});
