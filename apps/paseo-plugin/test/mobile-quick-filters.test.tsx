import type { PluginTheme } from "@getpaseo/plugin";
import { isValidElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useRef: () => ({ current: null }) }));
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon" }));
vi.mock("../client/control-help", () => ({ useControlHelp: () => ({ dismiss() {} }) }));
import { MobileQuickChooser } from "../client/mobile-quick-chooser";
import { QUICK_FILTER_ICONS, visibleQuickFilters } from "../client/mobile-quick-filter-options";
import { QUICK_STATUS_FILTERS, type StatusFilter } from "../client/session-view";

function walk(node: ReactNode): { props: Record<string, unknown> }[] {
  if (Array.isArray(node)) return node.flatMap(walk);
  if (!isValidElement<{ children?: ReactNode }>(node)) return [];
  return [node, ...walk(node.props.children)];
}

describe("mobile quick filter discovery", () => {
  it.each([296, 366, 820])("keeps the selected state reachable at %dpt without changing the eight canonical choices", (width) => {
    for (const status of [...QUICK_STATUS_FILTERS, "closed", "earlier_error"] as StatusFilter[]) {
      const choices = visibleQuickFilters(width, status);
      expect(choices).toContain(status);
      expect(new Set(choices).size).toBe(choices.length);
      expect((choices.length + 3) * 44 + (choices.length + 2) * 4).toBeLessThanOrEqual(width);
    }
    expect(QUICK_STATUS_FILTERS).toEqual(["current", "all", "running", "idle", "error", "needs_input", "attention", "favorites"]);
    expect(new Set(QUICK_STATUS_FILTERS.map((choice) => QUICK_FILTER_ICONS[choice])).size).toBe(8);
  });

  it("opening/reading/closing the named legend never filters; only pressing a choice applies it once", () => {
    const onStatus = vi.fn(), onClose = vi.fn();
    const counts = Object.fromEntries(QUICK_STATUS_FILTERS.map((status, index) => [status, index])) as Record<StatusFilter, number>;
    const ref = { current: null };
    const tree = MobileQuickChooser({ root: ref, rootSize: { width: 320, height: 320 }, anchor: ref, colors: { foreground: "#111111", foregroundMuted: "#555555", surface0: "#ffffff", surface1: "#f8f8f8", surface2: "#eeeeee", accent: "#0060c0", statusDanger: "#b00020", statusWarning: "#805000", statusSuccess: "#006b3c" } as PluginTheme["colors"], status: "needs_input", counts, onStatus, onClose });
    const choices = walk(tree).filter((node) => String(node.props.testID).startsWith("sessions-choose-"));
    expect(choices).toHaveLength(8);
    expect(choices.map((node) => node.props["aria-pressed"])).toEqual([false, false, false, false, false, true, false, false]);
    expect(onStatus).not.toHaveBeenCalled();
    tree.props.onClose();
    expect(onStatus).not.toHaveBeenCalled();
    (choices[5]!.props.onPress as () => void)();
    expect(onStatus).toHaveBeenCalledExactlyOnceWith("needs_input");
    expect(choices[5]!.props.accessibilityLabel).toContain("Needs input, 5 sessions");
  });
});
