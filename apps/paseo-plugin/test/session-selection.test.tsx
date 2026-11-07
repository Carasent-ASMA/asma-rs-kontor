import { isValidElement } from "react";
import { describe, expect, it, vi, type Mock } from "vitest";
import { TAKEOVER_EFFECTS } from "../client/session-takeover";

// The header and bar are hook-free; a session row keeps its own hooks, drawn on a fresh renderer per row.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])),
  useContext: () => undefined, useId: () => "help" }));
const screen = vi.hoisted(() => ({ width: 390 }));
vi.mock("react-native", () => ({ Easing: { linear: (t: number) => t }, Platform: { OS: "web" }, useWindowDimensions: () => ({ width: screen.width, height: 844, fontScale: 1 }), Pressable: "Pressable", Text: "Text", View: "View", Image: "Image", ScrollView: "ScrollView", BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
// The ControlHelp provider is replaced by a stateful double: hold/dismiss move `open` exactly as the
// provider would, and a test simulates the provider's own dismissals (Escape, outside press) by setting
// `open` back — the control's expanded flag must follow, not its own press history (R38 F1).
const helpDouble = vi.hoisted(() => ({ open: false, holds: 0, dismisses: 0, lastText: null as string | null }));
vi.mock("../client/control-help", () => ({ useControlHelp: (text: string | null) => { helpDouble.lastText = text; return { bindings: {}, open: helpDouble.open,
  hold: () => { helpDouble.holds++; helpDouble.open = true; },
  dismiss: () => { helpDouble.dismisses++; helpDouble.open = false; },
  forget: () => {} }; },
  useActionHelp: () => ({ bindings: {}, forget: () => {} }) }));

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
  const bar = (count: number, takeover = control(), auto = control(), archive = control(), disclosure: string | null = null, hasLimits = true) => ({ takeover, auto, archive, tree: harness.current.render(() => walk(SelectionBar({ count, takeover, auto, archive, colors: COLORS, disclosure, hasLimits }))) });
  const ids = ["selection-takeover", "selection-takeover-auto", "selection-archive"];
  const buttons = (tree: Element[]) => ids.map((id) => tree.find((node) => node.type === "Pressable" && node.props.testID === id)!);

  it("labels Take over N, Auto and Archive N for the selection and runs each when allowed", () => {
    const b = bar(3);
    expect(buttons(b.tree).map((button) => texts(walk(button)).join(""))).toEqual(["Take over 3", "Auto", "Archive 3"]);
    expect(buttons(b.tree).map((button) => (button.props.accessibilityState as { disabled: boolean }).disabled)).toEqual([false, false, false]);
    buttons(b.tree).forEach(press);
    expect([b.takeover.onPress, b.auto.onPress, b.archive.onPress].map((fn) => fn.mock.calls.length)).toEqual([1, 1, 1]);
  });

  it("carries every refused control's full reason on its accessibilityHint (R38 D1)", () => {
    const b = bar(2, control("This host is offline."), control(), control("Archive waits for the host and any running archive."));
    const hints = buttons(b.tree).map((button) => button.props.accessibilityHint as string | undefined);
    expect(hints[0]).toContain("This host is offline.");
    expect(hints[1]).toContain("This host is offline."); // the takeover group shares the group's refusal
    expect(hints[2]).toContain("Archive waits for the host and any running archive.");
    const cleared = bar(2);
    expect(buttons(cleared.tree).map((button) => button.props.accessibilityHint)).toEqual([undefined, undefined, undefined]);
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
    } finally { screen.width = 390; }
  });

  it("lays the buttons row out without flexWrap, inside a bar that scrolls within a maxHeight (R38-D3-r1/D2)", () => {
    const tree = bar(2).tree;
    // R38-D3-r1: Yoga's StretchFlexBasis errata inflates a wrapped row to two lines while every button
    // sits on line one, so the row never wraps; stacking is the measured choice below.
    const row = byTestId(tree, "selection-buttons-row")!;
    const rowStyle = row.props.style as Record<string, unknown>;
    expect(rowStyle).toMatchObject({ flexDirection: "row", gap: 8 });
    expect(rowStyle.flexWrap).toBeUndefined();
    // R38 D2: the bar caps at 60% of its parent and scrolls inside that instead of pushing the list out.
    const root = byTestId(tree, "selection-bar")!;
    expect(root.props.style).toMatchObject({ maxHeight: "60%", flexShrink: 1, paddingTop: 6, borderTopWidth: 1 });
    // R38-D2-r2: the bar carries NO minHeight of its own — on a short root it takes what the capped
    // list floor leaves and its scroller owns the buttons (M-bar-floor-uncapped dies here).
    expect(JSON.stringify(root.props.style)).not.toContain('"minHeight"');
    const scroller = root.props.children as { type: unknown; props: { style: Record<string, unknown> } };
    expect(scroller.type).toBe("ScrollView");
    expect(scroller.props.style).toMatchObject({ flexGrow: 0, flexShrink: 1 });
  });

  it("stacks Archive under the takeover group exactly when the measured intrinsic widths overflow the row, and keeps it stacked (R38-D3-r1)", () => {
    // The decision reads the row's measured width and the labels' intrinsic widths, off the layout path.
    const draw = () => harness.current.render(() => walk(SelectionBar({ count: 2, takeover: control(), auto: control(), archive: control(), colors: COLORS })));
    const layout = (element: Element, width: number) => (element.props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 0, width, height: 44 } } });
    // One line: everything fits.
    let tree = draw();
    layout(byTestId(tree, "selection-buttons-row")!, 400);
    const measurer = byTestId(tree, "selection-label-measure")!;
    const labels = (measurer.props.children as Element[]).map((child) => (child.props.onLayout as (event: unknown) => void));
    labels[0]!({ nativeEvent: { layout: { x: 0, y: 0, width: 77, height: 16 } } });
    labels[1]!({ nativeEvent: { layout: { x: 0, y: 0, width: 30, height: 16 } } });
    labels[2]!({ nativeEvent: { layout: { x: 0, y: 0, width: 58, height: 16 } } });
    tree = draw();
    expect(byTestId(tree, "selection-buttons-row")!.props.style).toMatchObject({ flexDirection: "row" });
    expect(tree.filter((node) => (node.type as { name?: string }).name === "BarButton")[2]!.props.grow).toBe(2);
    // Stacked: the same labels measured against a narrower row (text200-like widths overflow it).
    tree = draw();
    layout(byTestId(tree, "selection-buttons-row")!, 300);
    const wide = byTestId(tree, "selection-label-measure")!;
    const wideLabels = (wide.props.children as Element[]).map((child) => (child.props.onLayout as (event: unknown) => void));
    wideLabels[0]!({ nativeEvent: { layout: { x: 0, y: 0, width: 200, height: 16 } } });
    wideLabels[1]!({ nativeEvent: { layout: { x: 0, y: 0, width: 60, height: 16 } } });
    wideLabels[2]!({ nativeEvent: { layout: { x: 0, y: 0, width: 200, height: 16 } } });
    tree = draw();
    expect(tree.filter((node) => (node.type as { name?: string }).name === "BarButton")[2]!.props.grow).toBe(0);
    // Stable once stacked: the same measurements draw the same stack again.
    tree = draw();
    expect(byTestId(tree, "selection-buttons-row")!.props.style).toMatchObject({ flexDirection: "column" });
  });

  it("clamps the takeover disclosure to two lines with a Limits control that starts collapsed, flips expanded on press and takes a 44pt target (R38-D1)", () => {
    helpDouble.open = false; helpDouble.holds = 0; helpDouble.dismisses = 0;
    const disclosure = `${TAKEOVER_EFFECTS} Plus a wall of host limits long enough to fill twenty-five lines on a phone.`;
    let tree = bar(2, control(), control(), control(), disclosure).tree;
    const line = byTestId(tree, "selection-takeover-disclosure")!;
    // R38 V2: the inline line is EXACTLY the fixed effects sentence — the host limits leave the text
    // entirely and ride only the Limits popover (the hook receives the full `disclosure` prop).
    const inline = (line.props.children as string[] | string); expect(String(Array.isArray(inline) ? inline.flat(Infinity).join("") : inline)).toBe(`Take over: ${TAKEOVER_EFFECTS}`);
    expect(line.props.numberOfLines).toBe(2);
    expect(line.props.style).toMatchObject({ lineHeight: 22 }); // two lines fill the 44pt Limits row: no dead band under the last line (R38 F2)
    // No second, unclamped copy of the effects text anywhere in the bar (R38 D1): the only carrier is
    // the two-line disclosure; anything else starting "Take over: " is the inline wall returning.
    const walls = tree.filter((node) => node.type === "Text" && [node.props.children].flat(Infinity).join("").startsWith("Take over: "));
    expect(walls.map((node) => node.props.numberOfLines)).toEqual([2]);
    const limits = byTestId(tree, "selection-limits")!;
    expect(limits.props).toMatchObject({ accessibilityRole: "button", accessibilityState: { expanded: false }, hitSlop: 6 });
    const limitStyle = (limits.props.style as (state: object) => Record<string, unknown>)({ pressed: false, focused: false });
    expect(limitStyle.minHeight).toBeGreaterThanOrEqual(44);
    expect(limitStyle.minWidth).toBeGreaterThanOrEqual(44);
    press(limits);
    tree = bar(2, control(), control(), control(), disclosure).tree;
    expect(byTestId(tree, "selection-limits")!.props.accessibilityState).toMatchObject({ expanded: true });
    // F1: the provider's own dismissals (Escape, an outside press, scroll, resize, back) close the state
    // too — expanded tracks the popover, not this control's press history — and the first press after
    // any such dismissal OPENS the limits again rather than dead-dismissing a closed popover.
    helpDouble.open = false;
    tree = bar(2, control(), control(), control(), disclosure).tree;
    expect(byTestId(tree, "selection-limits")!.props.accessibilityState).toMatchObject({ expanded: false });
    press(byTestId(tree, "selection-limits"));
    expect(helpDouble.holds).toBe(2);
    expect(helpDouble.dismisses).toBe(0);
    // R38 V2: the Limits popover keeps the FULL effects-plus-limits disclosure — the hook's text is the
    // whole prop, not the clipped inline sentence (M-limits-without-limits dies here).
    expect(helpDouble.lastText).toBe(`Take over: ${disclosure}`);
  });

  it("renders no Limits control when the host states no limits, while the effects line stays (R38 V2)", () => {
    let tree = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`, false).tree;
    expect(byTestId(tree, "selection-takeover-disclosure")).toBeDefined();
    expect(byTestId(tree, "selection-limits")).toBeUndefined();
    tree = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`, true).tree;
    expect(byTestId(tree, "selection-limits")).toBeDefined();
  });

  it("shows the effects line only while Take over can start, and clamps every reason line to one", () => {
    const refusal = bar(2, control("This host is offline."), control(), control(), "Take over: effects text.").tree;
    expect(byTestId(refusal, "selection-takeover-disclosure")).toBeUndefined();
    expect(byTestId(refusal, "selection-limits")).toBeUndefined();
    const enabled = bar(2, control(), control(), control("Archive waits."), "Take over: effects text.").tree;
    expect(byTestId(enabled, "selection-takeover-disclosure")!.props.numberOfLines).toBe(2);
    const reasons = enabled.filter((node) => node.type === "Text" && node.props.numberOfLines === 1 && !node.props.testID && (node.props.children as string) !== "Auto");
    expect(reasons.map((node) => node.props.children)).toContain("Archive waits.");
    const noneSelected = bar(0).tree;
    expect(texts(noneSelected)).toContain("Select sessions first.");
    expect(byTestId(noneSelected, "selection-takeover-disclosure")).toBeUndefined();
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
