import type { PluginTheme } from "@getpaseo/plugin";
import { isValidElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
/** The state toolbar's measured row width, kept across draws as a mounted component keeps its state, and the text scale. */
const layout = vi.hoisted(() => ({ width: 0, fontScale: 1 }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useRef: () => ({ current: null }), useEffect: () => {}, useImperativeHandle: () => {},
  useState: () => [layout.width, (width: number) => { layout.width = width; }] }));
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View", ScrollView: "ScrollView", useWindowDimensions: () => ({ fontScale: layout.fontScale }) }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon" }));
vi.mock("../client/control-help", () => ({ useControlHelp: () => ({ dismiss() {} }) }));
import { MobileQuickChooser } from "../client/mobile-quick-chooser";
import { MobileQuickToolbar, StateToolbar } from "../client/mobile-quick-toolbar";
import type { SessionStateCount } from "../client/session-state-counts";
import { QUICK_FILTER_ICONS, visibleQuickFilters } from "../client/mobile-quick-filter-options";
import { MORE_STATUS_FILTERS, QUICK_STATUS_FILTERS, type StatusFilter } from "../client/session-view";

function walk(node: ReactNode): { type?: unknown; props: Record<string, unknown> }[] {
  if (Array.isArray(node)) return node.flatMap(walk);
  if (!isValidElement<{ children?: ReactNode }>(node)) return [];
  // The shared state toolbar is drawn in place, so its tiles and pinned utilities are visible.
  if (node.type === StateToolbar) return [node, ...walk(StateToolbar(node.props as Parameters<typeof StateToolbar>[0]))];
  return [node, ...walk(node.props.children)];
}

describe("mobile quick filter discovery", () => {
  it("keeps the nine shortcuts in the agreed order with their original icons", () => {
    expect(QUICK_STATUS_FILTERS).toEqual(["recent", "activity", "running", "needs_input", "error", "all", "idle", "attention", "favorites"]);
    expect(QUICK_STATUS_FILTERS.map((choice) => QUICK_FILTER_ICONS[choice])).toEqual(["Clock", "Activity", "Play", "MessageCircleQuestion", "CircleAlert", "LayoutGrid", "Pause", "Bell", "Star"]);
  });

  const counts = Object.fromEntries([...QUICK_STATUS_FILTERS, ...MORE_STATUS_FILTERS].map((status) => [status, { text: "2/9", label: "2 of 9 sessions", numerator: "2", denominator: "9" }])) as Record<StatusFilter, SessionStateCount>;
  const colors = { foreground: "#111111", foregroundMuted: "#555555", surface0: "#ffffff", surface1: "#f8f8f8", surface2: "#eeeeee", accent: "#0060c0", statusDanger: "#b00020", statusWarning: "#805000", statusSuccess: "#006b3c" } as PluginTheme["colors"];
  /** Kontor's compact toolbar as its row measures `width`, or before it is measured. */
  const draw = (status: StatusFilter, width = 0, fontScale = 1) => {
    layout.width = width; layout.fontScale = fontScale;
    return walk(MobileQuickToolbar({ colors, status, counts, activeCount: 1, filtersTriggerRef: { current: null }, chooserRef: { current: null }, chooserOpen: false,
      helpDismissRef: null, onStatus: vi.fn(), onFilters: vi.fn(), onChooser: vi.fn() }));
  };
  const shortcuts = (tree: ReturnType<typeof draw>) => tree.filter((node) => String(node.props.testID).startsWith("sessions-quick-")).map((node) => String(node.props.testID).slice("sessions-quick-".length));
  const trigger = (tree: ReturnType<typeof draw>) => tree.find((node) => node.props.testID === "sessions-mobile-filters-trigger")!.props;

  it.each([[0, 4], [296, 4], [366, 5], [600, 9]] as const)("fits whole tiles in a %ipx row with More and Filters pinned after them, never scrolling", (width, shown) => {
    const tree = draw("recent", width), row = tree.find((node) => node.props.testID === "sessions-mobile-quick-row" && node.props.onLayout)!;
    expect(shortcuts(tree)).toEqual(QUICK_STATUS_FILTERS.slice(0, shown));
    expect((row.props.children as { props: Record<string, unknown> }[]).slice(1).map((node) => node.props.testID)).toEqual(["sessions-mobile-quick-chooser-trigger", "sessions-mobile-filters-trigger"]);
    expect(tree.filter((node) => String(node.props.testID).startsWith("sessions-quick-") || String(node.props.testID).startsWith("sessions-mobile-")).filter((node) => "target" in node.props).map((node) => node.props.target)).toEqual(Array(shown + 2).fill(44));
    expect(tree.some((node) => node.type === "ScrollView")).toBe(false);
    expect(tree.find((node) => node.props.testID === "sessions-quick-recent")!.props).toMatchObject({ label: "Last visited, 2 of 9 sessions", count: counts.recent, icon: "Clock", selected: true });
  });

  it("keeps a shortcut chosen past the tiles in the last tile, once, as the row narrows and widens", () => {
    expect(shortcuts(draw("attention", 296))).toEqual(["recent", "activity", "running", "attention"]);
    expect(shortcuts(draw("attention", 600))).toEqual(QUICK_STATUS_FILTERS);
    expect(shortcuts(draw("attention", 366))).toEqual(["recent", "activity", "running", "needs_input", "attention"]);
    expect(draw("attention", 296).find((node) => node.props.testID === "sessions-quick-attention")!.props).toMatchObject({ selected: true, icon: "Bell", count: counts.attention });
  });

  // Compact Sessions at 320 points keeps 208 beside Attention's Check IDs: room for More and Filters at 72 points, not a state too.
  it.each([[208, 2, []], [296, 2, ["attention"]], [208, 1, ["recent", "attention"]]] as const)("draws only whole targets beside More in a %ipx row at fontScale %i", (width, fontScale, shown) => {
    const tree = draw("attention", width, fontScale), target = fontScale === 1 ? 44 : 72;
    expect(shortcuts(tree)).toEqual(shown);
    expect(tree.filter((node) => "target" in node.props).map((node) => node.props.target)).toEqual(Array(shown.length + 2).fill(target));
    expect((shown.length + 2) * (target + 4) - 4).toBeLessThanOrEqual(width);
    expect(tree.find((node) => node.props.testID === "sessions-mobile-quick-chooser-trigger")!.props.label).toBe("All 9 quick filters and icon meanings, current: Attention");
  });

  it("never draws a state tile that would overlap More, at any row width with room for More and the trailing action", () => {
    for (const target of [44, 72]) for (let width = 2 * target + 4; width <= 800; width++) {
      const controls = visibleQuickFilters(width, "attention", target).length + 2;
      expect(controls * (target + 4) - 4, `${width}px at ${target}`).toBeLessThanOrEqual(width);
    }
  });

  it("shows a state chosen in Filters on the Filters trigger, never as a tenth shortcut", () => {
    const closed = draw("closed", 296);
    expect(shortcuts(closed)).toEqual(["recent", "activity", "running", "needs_input"]);
    expect(trigger(closed)).toMatchObject({ label: "Filters and view, state: Closed, 2 of 9 sessions, 1 active filters or view choices", icon: QUICK_FILTER_ICONS.closed, selected: true, count: counts.closed, caption: undefined });
    expect(trigger(draw("running", 296))).toMatchObject({ label: "Filters and view, 1 active filters or view choices", icon: "SlidersHorizontal", caption: "Filters", selected: undefined, count: undefined });
  });

  it("opening/reading/closing the named legend never filters; only pressing a choice applies it once", () => {
    const onStatus = vi.fn(), onClose = vi.fn();
    const counts = Object.fromEntries(QUICK_STATUS_FILTERS.map((status, index) => [status, { text: String(index), label: `${index} sessions` }])) as Record<StatusFilter, SessionStateCount>;
    const ref = { current: null };
    const tree = MobileQuickChooser({ root: ref, rootSize: { width: 320, height: 320 }, anchor: ref, colors: { foreground: "#111111", foregroundMuted: "#555555", surface0: "#ffffff", surface1: "#f8f8f8", surface2: "#eeeeee", accent: "#0060c0", statusDanger: "#b00020", statusWarning: "#805000", statusSuccess: "#006b3c" } as PluginTheme["colors"], status: "needs_input", counts, onStatus, onClose });
    const choices = walk(tree).filter((node) => String(node.props.testID).startsWith("sessions-choose-"));
    expect(choices).toHaveLength(9);
    expect(choices.map((node) => node.props["aria-pressed"])).toEqual([false, false, false, true, false, false, false, false, false]);
    expect(onStatus).not.toHaveBeenCalled();
    tree.props.onClose();
    expect(onStatus).not.toHaveBeenCalled();
    (choices[3]!.props.onPress as () => void)();
    expect(onStatus).toHaveBeenCalledExactlyOnceWith("needs_input");
    expect(choices[3]!.props.accessibilityLabel).toContain("Questions, 3 sessions");
  });
});
