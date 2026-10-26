import { isValidElement } from "react";
import { describe, expect, it, vi, type Mock } from "vitest";

// The header and bar are hook-free; a session row keeps its own hooks, drawn on a fresh renderer per row.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])),
  useContext: () => undefined, useId: () => "help" }));
const screen = vi.hoisted(() => ({ width: 390 }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, useWindowDimensions: () => ({ width: screen.width, height: 844, fontScale: 1 }), Pressable: "Pressable", Text: "Text", View: "View", Image: "Image", ScrollView: "ScrollView", BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));

import { SelectionBar, SelectionHeader, type SelectionControl } from "../client/session-selection";
import { DenseSessionRow, type DenseRowMode } from "../client/dense-session-row";
import { sessionEntry } from "./session-fixtures";

type Element = { type: unknown; props: Record<string, unknown> };
/** The bar's buttons are hook-free and drawn in place, so their labels and states are visible. */
const walk = (node: unknown): Element[] => Array.isArray(node) ? node.flatMap(walk) : !isValidElement<Record<string, unknown>>(node) ? []
  : typeof node.type === "function" && node.type.name === "BarButton" ? [node, ...walk((node.type as (props: object) => unknown)(node.props))] : [node, ...walk(node.props.children)];
const texts = (tree: Element[]) => tree.filter((node) => node.type === "Text").map((node) => [node.props.children].flat(Infinity).join(""));
const byTestId = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id);
const press = (node: Element | undefined) => (node!.props.onPress as () => void)();

const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const control = (unavailable: string | null = null): SelectionControl & { onPress: Mock<() => void> } => ({ unavailable, onPress: vi.fn() });

describe("SelectionHeader", () => {
  const header = (count: number, total: number, hidden: number, selectAll: SelectionControl & { checked: boolean } = { ...control(), checked: false }, onDone = vi.fn()) =>
    walk(SelectionHeader({ count, total, hidden, selectAll, colors: COLORS, onDone }));

  it("counts only the shown selection over the current matches", () => {
    expect(byTestId(header(5, 12, 2), "selection-count")!.props.children).toBe("3 / 12 selected");
    expect(byTestId(header(4, 12, 0), "selection-count")!.props.children).toBe("4 / 12 selected");
  });

  it("names selected sessions the filters hide, in the singular or plural", () => {
    expect(byTestId(header(3, 12, 1), "selection-hidden")!.props.children).toBe("1 selected session not shown by the current filters; it stays selected.");
    expect(byTestId(header(3, 12, 2), "selection-hidden")!.props.children).toBe("2 selected sessions not shown by the current filters; they stay selected.");
    expect(byTestId(header(3, 12, 0), "selection-hidden")).toBeUndefined();
  });

  it("is a checkbox that selects every match, or clears them when all are selected", () => {
    const off = { ...control(), checked: false }, on = { ...control(), checked: true };
    const unchecked = byTestId(header(0, 12, 0, off), "selection-all")!;
    expect(unchecked.props).toMatchObject({ accessibilityRole: "checkbox", accessibilityLabel: "Select all 12 matching sessions", accessibilityState: { checked: false, disabled: false }, "aria-checked": false });
    press(unchecked);
    expect(off.onPress).toHaveBeenCalledTimes(1);
    // React Native Web activates only a button on Space; the checkbox toggles on it once, not while held.
    const space = (repeat = false) => (unchecked.props.onKeyDown as (event: object) => void)({ key: " ", repeat, preventDefault: () => {}, stopPropagation: () => {} });
    space(); space(true);
    expect(off.onPress).toHaveBeenCalledTimes(2);
    expect(byTestId(header(12, 12, 0, on), "selection-all")!.props).toMatchObject({ accessibilityLabel: "Clear the 12 matching sessions", accessibilityState: { checked: true, disabled: false }, "aria-checked": true });
  });

  it("keeps an unavailable select-all visible and disabled with its reason, and ignores presses", () => {
    const refused = { ...control("More than 50 match; narrow the view."), checked: false };
    const tree = header(0, 80, 0, refused);
    const box = byTestId(tree, "selection-all")!;
    expect(box.props).toMatchObject({ accessibilityState: { checked: false, disabled: true }, "aria-disabled": true, disabled: true, tabIndex: 0, accessibilityHint: "More than 50 match; narrow the view." });
    expect(texts(tree)).toContain("More than 50 match; narrow the view.");
    press(box);
    expect(refused.onPress).not.toHaveBeenCalled();
  });

  it("ends selection with Done", () => {
    const onDone = vi.fn();
    const done = byTestId(header(2, 5, 0, undefined, onDone), "selection-done")!;
    expect(done.props).toMatchObject({ accessibilityRole: "button", accessibilityLabel: "Done selecting" });
    press(done);
    expect(onDone).toHaveBeenCalledTimes(1);
  });
});

describe("SelectionBar", () => {
  const bar = (count: number, takeover = control(), auto = control(), archive = control()) => ({ takeover, auto, archive, tree: walk(SelectionBar({ count, takeover, auto, archive, colors: COLORS })) });
  const ids = ["selection-takeover", "selection-takeover-auto", "selection-archive"];
  const buttons = (tree: Element[]) => ids.map((id) => tree.find((node) => node.type === "Pressable" && node.props.testID === id)!);

  it("labels Take over N, Auto and Archive N for the selection and runs each when allowed", () => {
    const b = bar(3);
    expect(buttons(b.tree).map((button) => texts(walk(button)).join(""))).toEqual(["Take over 3", "Auto", "Archive 3"]);
    expect(buttons(b.tree).map((button) => (button.props.accessibilityState as { disabled: boolean }).disabled)).toEqual([false, false, false]);
    buttons(b.tree).forEach(press);
    expect([b.takeover.onPress, b.auto.onPress, b.archive.onPress].map((fn) => fn.mock.calls.length)).toEqual([1, 1, 1]);
  });

  it("keeps every label whole on a narrow phone by dropping the bar's icons, each button first taking its label's width", () => {
    const icons = (tree: Element[]) => tree.filter((node) => node.type === "Icon").map((node) => node.props.name);
    const style = (button: Element) => (button.props.style as (state: object) => Record<string, unknown>)({ pressed: false });
    expect(icons(bar(12).tree)).toEqual(["RefreshCw", "ChevronDown", "Archive"]);
    screen.width = 320;
    try {
      const tree = bar(12).tree;
      expect(icons(tree)).toEqual(["ChevronDown"]);
      expect(buttons(tree).map((button) => texts(walk(button)).join(""))).toEqual(["Take over 12", "Auto", "Archive 12"]);
      expect(buttons(tree).map((button) => [style(button).flexBasis, style(button).flexGrow, style(button).flexShrink])).toEqual([["auto", 2, undefined], ["auto", 0, undefined], ["auto", 2, undefined]]);
      // Large text wraps Archive below the Take over group rather than cutting any label.
      expect((byTestId(tree, "selection-bar")!.props.children as { props: { style: object } }[])[0]!.props.style).toMatchObject({ flexDirection: "row", flexWrap: "wrap" });
    } finally { screen.width = 390; }
  });
  it("disables every action with nothing selected and says so once", () => {
    const b = bar(0, control("This host is offline."), control(), control("This host is offline."));
    expect(buttons(b.tree).map((button) => [button.props.accessibilityHint, (button.props.accessibilityState as { disabled: boolean }).disabled])).toEqual([
      ["Select sessions first.", true], ["Select sessions first.", true], ["Select sessions first.", true]]);
    expect(texts(b.tree).filter((line) => line === "Select sessions first.")).toHaveLength(1);
    expect(texts(b.tree)).not.toContain("This host is offline.");
    buttons(b.tree).forEach(press);
    expect([b.takeover.onPress, b.auto.onPress, b.archive.onPress].every((fn) => fn.mock.calls.length === 0)).toBe(true);
  });

  it("writes a reason shared by Take over and Archive once", () => {
    const b = bar(2, control("This host is offline."), control(), control("This host is offline."));
    expect(texts(b.tree).filter((line) => line === "This host is offline.")).toHaveLength(1);
    expect(texts(bar(2, control("Bulk takeover is not qualified here."), control(), control("Archive is busy.")).tree)).toEqual(expect.arrayContaining(["Bulk takeover is not qualified here.", "Archive is busy."]));
  });

  it("blocks only Auto on its own reason and ignores presses on blocked actions", () => {
    const b = bar(2, control(), control("Choosing a model is not supported here."), control());
    const [takeover, auto] = buttons(b.tree);
    expect((takeover!.props.accessibilityState as { disabled: boolean }).disabled).toBe(false);
    expect(auto!.props).toMatchObject({ accessibilityHint: "Choosing a model is not supported here.", accessibilityState: { disabled: true } });
    expect(texts(b.tree)).toContain("Choosing a model is not supported here.");
    press(auto);
    expect(b.auto.onPress).not.toHaveBeenCalled();
  });
});

describe("selected session rows", () => {
  const mark = (mode: DenseRowMode) => {
    harness.fresh();
    const tree = walk(harness.current.render(() => DenseSessionRow({ entry: sessionEntry("agent-a", { title: "Route review" }), colors: COLORS, clock: { now: Date.parse("2026-10-26T12:00:00Z"), zone: "UTC" },
      opened: null, mode, onPress: vi.fn() })));
    return walk(byTestId(tree, "session-row-choice")).find((node) => node.type === "Icon")!.props.name;
  };

  it("marks a selectable row with a circle and a selected one with a checked circle", () => {
    expect(mark({ kind: "select", selected: false })).toBe("Circle");
    expect(mark({ kind: "select", selected: true })).toBe("CircleCheck");
  });
});
