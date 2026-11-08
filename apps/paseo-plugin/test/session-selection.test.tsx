import { isValidElement } from "react";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import { TAKEOVER_EFFECTS } from "../client/session-takeover";

// The header is hook-free. The BAR carries hooks (the pin: pinnedToEnd, lastScrollY, heights), and
// the deferred-react harness stores hook state in slots that persist across renders — so each
// SelectionBar test resets to a real fresh mount (pinned, y 0, heights −1) via beforeEach below;
// without that reset a test inherits whatever pin state the previous test left behind (r16b F1).
// A session row likewise keeps its own hooks and resets per draw (harness.fresh() in its describe).
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
const helpDouble = vi.hoisted(() => ({ open: false, holds: 0, dismisses: 0, lastText: null as string | null, heldOpen: false }));
vi.mock("../client/control-help", () => ({ useControlHelp: (text: string | null) => { helpDouble.lastText = text; return { bindings: {}, open: helpDouble.open, heldOpen: helpDouble.heldOpen,
  hold: () => { helpDouble.holds++; helpDouble.open = true; helpDouble.heldOpen = true; },
  dismiss: () => { helpDouble.dismisses++; helpDouble.open = false; helpDouble.heldOpen = false; },
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
  // R16b F1: reset the deferred-react hook slots before EVERY test — without this, each bar test
  // inherits the previous test's pin, lastY and heights (the r16 stable-y gate passed vacuously
  // off the heights gate's released pin). A test that re-renders one mount inside its own body
  // still keeps that mount across its renders; only the STARTING state is fresh.
  beforeEach(() => harness.fresh());
  const bar = (count: number, takeover = control(), auto = control(), archive = control(), disclosure: string | null = null, hasLimits = true) => ({ takeover, auto, archive, tree: harness.current.render(() => walk(SelectionBar({ count, takeover, auto, archive, colors: COLORS, disclosure, hasLimits }))) });
  const ids = ["selection-takeover", "selection-takeover-auto", "selection-archive"];
  const buttons = (tree: Element[]) => ids.map((id) => tree.find((node) => node.type === "Pressable" && node.props.testID === id)!);

  it("persists taps on the bar scroller so a keyboard-up first tap reaches the controls (R12 RA-F1/B-F1)", () => {
    // RN 0.81.5 ScrollView defaults keyboardShouldPersistTaps to "never": its responder capture takes
    // the first tap to dismiss the keyboard and the bar control never acts. The bar must behave like
    // every other plugin scroller ("handled").
    const scroller = bar(1).tree.find((node) => node.type === "ScrollView")!;
    expect(scroller).toBeDefined();
    expect(scroller.props.keyboardShouldPersistTaps).toBe("handled");
  });

  it("hides the off-screen label measurer from accessibility (R12 N1)", () => {
    const measurer = byTestId(bar(2).tree, "selection-label-measure")!;
    expect(measurer.props["aria-hidden"]).toBe(true);
    expect(measurer.props.accessibilityElementsHidden).toBe(true);
    expect(measurer.props.importantForAccessibility).toBe("no-hide-descendants");
  });

  it("re-pins on the scroller's OWN layout change under the same armed gate as content changes (R12 T3)", () => {
    // R38-D2-r3: re-pins only on a content-size or own-height change. The web fixture cannot
    // distinguish the two (RNW's content container tracks the scroller's box), so this pins the
    // wiring itself: onLayout reaches repinToEnd under exactly the disarm/re-arm gate that
    // onContentSizeChange obeys. The mutant drops exactly the onLayout prop.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onLayout: () => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    expect(typeof scroller.props.onLayout).toBe("function");
    const scrollToEnd = vi.fn();
    (scroller.props.ref as { current: unknown }).current = { scrollToEnd };
    // Armed (the pin starts true): an own-height layout change re-pins to the end.
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(1);
    expect(scrollToEnd.mock.calls[0]![0]).toMatchObject({ animated: false });
    // The echo of that programmatic scroll, at the end: armed, and the offset is remembered (R13).
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 216 }, layoutMeasurement: { height: 84 }, contentSize: { height: 300 } } });
    // A user scrolls up to read: the offset DECREASES, the pin releases, and an own-height change
    // must not fight them.
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 0 }, layoutMeasurement: { height: 84 }, contentSize: { height: 300 } } });
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(1);
    // Back at the end, the pin re-arms and the next own-height change re-pins again.
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 216 }, layoutMeasurement: { height: 84 }, contentSize: { height: 300 } } });
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(2);
  });

  it("keeps the rest pin across our own scroll echo measuring settled layout (R13 race, QA r12)", () => {
    // The QA r12 failure at 393x420 text200, replayed in the logged order: content and clip settle
    // together while the echo of our own scrollToEnd — the offset it set for the intermediate
    // 136/86 layout — arrives against the settled 97/188: 50 + 97 < 188 − 2. R38-D2-r4 keeps the
    // pin: the echo is same-direction AND its geometry differs from the previous event's.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onLayout: () => void; onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    const scrollToEnd = vi.fn();
    (scroller.props.ref as { current: unknown }).current = { scrollToEnd };
    // The intermediate settle, exactly as the log shows it: content 136 in a clip 86, both repins
    // fire, and the echo of the second lands at its end (50 + 86 = 136).
    scroller.props.onContentSizeChange(369, 136);
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(2);
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 50 }, layoutMeasurement: { height: 86 }, contentSize: { height: 136 } } });
    // The next echo carries the same offset against the settled geometry (97/188) — same
    // direction, no user input, and changed geometry: the pin survives it.
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 50 }, layoutMeasurement: { height: 97 }, contentSize: { height: 188 } } });
    // The settling content-size and own-height changes must still re-pin.
    scroller.props.onContentSizeChange(369, 188);
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(4);
    expect(scrollToEnd.mock.calls[3]![0]).toMatchObject({ animated: false });
    // At the end the pin stays armed for the next change.
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 91 }, layoutMeasurement: { height: 97 }, contentSize: { height: 188 } } });
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(5);
  });

  it("keeps the pin through the mirror race: a lower echo under changed geometry is not a user scroll (R14 F2, D2-r4)", () => {
    // The AUD r13 mirror sequence (393x420 text200 pulse), replayed in the observed order with the
    // pin's own scrolls moving the offset: 91/97/188 → 36/108/144 → 0/97/136 → settle 188/97. The
    // direction-only rule read the 0/97/136 echo (0 < 36, not at end) as a user scroll up and
    // released the pin; under D2-r4 its geometry differs from the previous event's, so the pin
    // survives and the settling re-pins restore the end.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onLayout: () => void; onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: { scrollToEnd: (opts: { animated: boolean }) => void } } } } }).children;
    let y = 0, clip = 97, content = 188;
    const scrollToEnd = vi.fn((opts: { animated: boolean }) => { y = content - clip; void opts; });
    scroller.props.ref.current = { scrollToEnd };
    const echo = (offset: number) => { y = offset; scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } }); };
    scroller.props.onLayout();
    echo(91);
    expect(y).toBe(91);
    content = 144; clip = 108;
    scroller.props.onContentSizeChange(369, content);
    scroller.props.onLayout();
    echo(36);
    // The observed real ordering: the lower offset is echoed only after the clip shrinks again.
    content = 136; clip = 97;
    echo(0);
    content = 188;
    scroller.props.onContentSizeChange(369, content);
    scroller.props.onLayout();
    expect(y).toBe(91);
    // Armed for the next change: the settle's own-height callback re-pinned (calls 4 and 5 above).
    expect(scrollToEnd).toHaveBeenCalledTimes(5);
  });

  it("an equal-offset echo after a lower event under changed geometry keeps the pin: every event stores y (r15 SF1, D2-r4)", () => {
    // Review-B survivor probe (M-rb-r14-lasty-only-at-end): if only at-end events stored y, the
    // stale end offset (91) would judge the repeated 60/110/188 echo (60 < 91, geometry now
    // stable) a user scroll up and release the pin with no input — the settling content-size
    // change would then skip its re-pin and the bar would rest mid-content. D2-r4 stores y on
    // EVERY event, so the repeat is judged against 60 and stays same-direction. The gate asserts
    // behaviour — the offset rests at the end after the change — never the ref value.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: { scrollToEnd: (opts: { animated: boolean }) => void } } } } }).children;
    let y = 0; let clip = 97; let content = 188;
    const scrollToEnd = vi.fn(() => { y = content - clip; });
    scroller.props.ref.current = { scrollToEnd };
    const event = (offset: number, clipHeight: number, contentHeight: number) => scroller.props.onScroll({ nativeEvent: { contentOffset: { y: offset }, layoutMeasurement: { height: clipHeight }, contentSize: { height: contentHeight } } });
    // The pin echo, at its end: armed, y stored.
    event(91, 97, 188);
    // A lower offset under CHANGED geometry: a settle artifact, not a user scroll — the pin holds
    // and this event's y (60) is stored too.
    event(60, 110, 188);
    // The same offset again under the now-stable geometry: same-direction, the pin holds.
    event(60, 110, 188);
    // The settle: still armed, so the content-size change re-pins and the bar rests at its end.
    content = 200; clip = 110;
    scroller.props.onContentSizeChange(369, content);
    expect(scrollToEnd).toHaveBeenCalledTimes(1);
    expect(y + clip).toBeGreaterThanOrEqual(content - 2);
  });

  it("a lower event at the previous non-end event's geometry releases the pin: every event stores both heights (r16 SF1-r15, D2-r4)", () => {
    // Review-B survivor probe (NV-sf1-heights-only-at-end): if only at-end events stored the
    // heights, the 60/110/188 settle would leave the stale {188,97} stored, so the 40/110/188
    // drag (stable geometry in truth) would read as a geometry change and KEEP the pin — the
    // settling content-size change would then re-pin over a user who scrolled up. Every event
    // stores both heights, so the drag releases and no re-pin fires. Asserts behaviour only.
    // Kills (r16b results.json): NV-sf1-heights-only-at-end, M-r16-y-stableGeometry,
    // M-r16-y-removed, M-r16-heights-stableGeometry, M-r16-heights-removed,
    // M-r16-heights-content-kept, M-r16-heights-clip-kept, M-r16-y-above-rule, M-pin-always,
    // M-drag-unpin-lost.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    const scrollToEnd = vi.fn();
    scroller.props.ref.current = { scrollToEnd };
    const event = (y: number, clip: number, content: number) => scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } });
    event(91, 97, 188);
    event(60, 110, 188);
    event(40, 110, 188);
    scroller.props.onContentSizeChange(369, 200);
    expect(scrollToEnd).toHaveBeenCalledTimes(0);
  });

  it("a lower event after a higher stable-geometry event releases the pin: every event stores y (r16 SF1-r15, D2-r4)", () => {
    // Review-B survivor probe (NV-sf1-only-changed-geometry): a y-store condition that skips a
    // stable event's y leaves lastY stale (10 here — the first event is unstable and stores its 10),
    // so the 50 and 30 drags read as same-direction and the pin never releases — the settle then
    // re-pins over the user. With every event storing y the 30 < 50 release is real and the settle
    // must not re-pin. Asserts behaviour only.
    // Kills (r16b results.json): NV-sf1-only-changed-geometry, NV-sf1-heights-only-at-end,
    // M-rb-r14-lasty-only-at-end (the y-atEnd matrix cell), M-r16-y-removed,
    // M-r16-heights-stableGeometry, M-r16-heights-removed, M-r16-heights-content-kept,
    // M-r16-heights-clip-kept, M-r16-y-above-rule, M-pin-always, M-drag-unpin-lost.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    const scrollToEnd = vi.fn();
    scroller.props.ref.current = { scrollToEnd };
    const event = (y: number, clip: number, content: number) => scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } });
    event(10, 97, 188);
    event(50, 97, 188);
    event(30, 97, 188);
    scroller.props.onContentSizeChange(369, 200);
    expect(scrollToEnd).toHaveBeenCalledTimes(0);
  });

  it("an at-end event stores y too: the re-armed offset judges the next move (r16 matrix, D2-r4)", () => {
    // Matrix cell M-r16-y-notAtEnd (store y only on non-end events): re-arming at the end must
    // store the new offset, or the next stable down-move (70) is judged against the stale 50 and
    // keeps the pin — the settle re-pins over a user who scrolled up. Asserts behaviour only.
    // Kills (r16b results.json): NV-sf1-only-changed-geometry, M-r16-y-notAtEnd, M-r16-y-removed,
    // M-r16-heights-stableGeometry, M-r16-heights-removed, M-r16-heights-content-kept,
    // M-r16-heights-clip-kept, M-r16-y-above-rule, M-pin-always, M-drag-unpin-lost. It is the sole
    // killer of none of these.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    const scrollToEnd = vi.fn();
    scroller.props.ref.current = { scrollToEnd };
    const event = (y: number, clip: number, content: number) => scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } });
    event(50, 97, 188);
    event(91, 97, 188);
    event(70, 97, 188);
    scroller.props.onContentSizeChange(369, 200);
    expect(scrollToEnd).toHaveBeenCalledTimes(0);
  });

  it("a geometry round trip restores the guard: the unstable event's heights are stored (r16 matrix, D2-r4)", () => {
    // Guards that an UNSTABLE event stores its heights, so the guard stays correct across a
    // geometry round trip: the 36/108/144 settle stores {144,108}, which makes the round-trip echo
    // 30/97/188 a CHANGED-geometry event (not a stable drag up), so the pin survives it and the
    // settle re-pins the bar to its end. (r16b correction: this gate does NOT kill
    // M-r16-heights-notStableGeometry — that cell stores on every unstable event, including this
    // settle, so it is an equivalence claim; its skip branch is exactly an identity write.)
    // Kills (r16b results.json): M-r16-heights-above-stable, M-rb-repin-skips-own-height,
    // M-r14-mirror-back, M-geom-ignored, M-r13-race-back, M-echo-unpins, M-bar-rest-top.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onLayout: () => void; onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: { scrollToEnd: (opts: { animated: boolean }) => void } } } } }).children;
    let y = 0; let clip = 97; let content = 188;
    const scrollToEnd = vi.fn(() => { y = content - clip; });
    scroller.props.ref.current = { scrollToEnd };
    const echo = (offset: number) => { y = offset; scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } }); };
    scroller.props.onLayout();
    echo(91);
    // The settle: geometry changes to {144,108}, both re-pins fire, the echo rests at its end.
    content = 144; clip = 108;
    scroller.props.onContentSizeChange(369, content);
    scroller.props.onLayout();
    echo(36);
    // The round trip: clip back to 97 with the original content — the echo carries a LOWER offset
    // under geometry that differs from the last stored {144,108}, so the pin must survive it.
    content = 188; clip = 97;
    echo(30);
    // And the settle back to {188,97} re-pins; the bar rests at its end.
    scroller.props.onContentSizeChange(369, content);
    scroller.props.onLayout();
    expect(y).toBe(91);
  });

  it.each([
    ["rearm-geometry", [[91, 97, 188], [50, 97, 188], [78, 110, 188], [60, 110, 188]]],
    ["rearm-stable-y", [[91, 97, 188], [50, 97, 188], [91, 97, 188], [70, 97, 188]]],
    ["rearm-content-change", [[30, 97, 188], [10, 97, 188], [91, 110, 200], [60, 110, 200]]],
  ] as Array<[string, Array<[number, number, number]>]>)("the re-arm event stores y and both heights: the first lower event after it releases the pin (%s, r16 SF1-r16c, D2-r4/D2-r3)", (_name, events) => {
    // Review-B finding SF1-r16: the nearest r16 gate ("the re-armed offset") was never released, so
    // its at-end event was not a re-arm. r16c: three re-arm shapes. rearm-geometry: the pin IS
    // released (50), the at-end event (78 at changed geometry) re-arms AND stores, and the first
    // lower event after the re-arm (60, stable against the fresh store) must release — a y store
    // skipped on the re-arm leaves the stale 50 (V-y-wasPinned) so the 60 reads same-direction; a
    // heights store skipped on the re-arm leaves the 50 event's content 188 / clip 97
    // (V-h-wasPinned), so the 60 at clip 110 reads CHANGED geometry. Either way the pin holds
    // through the 60 and the settle re-pins over the user. rearm-stable-y (SF1-r16c): a re-arm at
    // STABLE geometry (91 back at the end) must store too — a y store skipped there
    // (N-y-skip-rearm-stable) leaves the stale 50 and the next lower 70 never releases.
    // rearm-content-change (SF1-r16c): a re-arm that changes only the content must store it
    // (N-h-content-kept-on-rearm keeps the old content, so the next lower event reads changed
    // geometry and holds the pin). Asserts behaviour only.
    // Kills (r16d results.json): N-y-skip-rearm-stable, N-h-content-kept-on-rearm (its own r16c
    // seeds), V-y-wasPinned, V-h-wasPinned, M-r16-y-notAtEnd, M-r16-y-stableGeometry,
    // M-r16-y-removed, M-r16-heights-notAtEnd, M-r16-heights-stableGeometry, M-r16-heights-removed,
    // M-r16-heights-content-kept, M-r16-heights-clip-kept, M-r16-y-above-rule, M-pin-always,
    // M-drag-unpin-lost. Unique killer of none (the D2-r4 differential gate also kills its seeds).
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    const scrollToEnd = vi.fn();
    scroller.props.ref.current = { scrollToEnd };
    const event = (y: number, clip: number, content: number) => scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } });
    for (const [y, clip, content] of events) event(y, clip, content);
    scroller.props.onContentSizeChange(369, 200);
    expect(scrollToEnd).toHaveBeenCalledTimes(0);
  });

  it.each([
    ["shrink-both-axes", [[10, 110, 200], [30, 97, 188], [10, 97, 188]]],
    ["shrink-content-only", [[10, 97, 200], [30, 97, 188], [10, 97, 188]]],
    ["shrink-clip-only", [[10, 110, 188], [30, 97, 188], [10, 97, 188]]],
    ["shrink-content-at-end", [[10, 97, 200], [78, 110, 188], [10, 110, 188]]],
    ["shrink-clip-at-end", [[10, 110, 188], [91, 97, 188], [10, 97, 188]]],
  ] as Array<[string, Array<[number, number, number]>]>)("a height that shrinks and stays smaller is stored: the guard follows the real geometry (%s, r16 SF2-r16c, D2-r4/D2-r3)", (_name, events) => {
    // Review-B finding SF2-r16: no gate covered a shrink that STAYS. r16c: five shrink shapes. A
    // store kept at the old high water reads every later event as changed geometry, so the last
    // drop never releases and the content-size settle re-pins over the user for as long as the
    // height stays below its peak. The rows pin the single-axis shrinks (content-only
    // [N-h-content-high-water-clip-same], clip-only [N-h-clip-high-water-content-same]) and the
    // shrinks that land ON an at-end event (N-h-content-high-water-at-end,
    // N-h-clip-high-water-at-end — the high water must not apply to the re-arm either). Asserts
    // behaviour only.
    // Kills (r16d results.json): N-h-content-high-water-clip-same, N-h-clip-high-water-content-same,
    // N-h-content-high-water-at-end, N-h-clip-high-water-at-end (its own r16c seeds), V-h-content-high-water,
    // V-h-clip-high-water, NV-sf1-heights-only-at-end, M-rb-r14-lasty-only-at-end,
    // M-r16-y-stableGeometry, M-r16-y-notAtEnd, M-r16-heights-notAtEnd. Unique killer of none.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    const scrollToEnd = vi.fn();
    scroller.props.ref.current = { scrollToEnd };
    const event = (y: number, clip: number, content: number) => scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } });
    for (const [y, clip, content] of events) event(y, clip, content);
    scroller.props.onContentSizeChange(369, 200);
    expect(scrollToEnd).toHaveBeenCalledTimes(0);
  });

  it.each([
    ["fraction-drop", [[30.4, 97, 188], [30.2, 97, 188]], 0],
    ["fraction-equal-y", [[30.2, 97, 188], [30.2, 97, 188]], 1],
    ["fraction-hundredths-y", [[30.44, 97, 188], [30.42, 97, 188]], 0],
  ] as Array<[string, Array<[number, number, number]>, number]>)("a fractional offset is stored unrounded: the pin judges the exact value (%s, r16 N1-r16c, D2-r4)", (_name, events, settles) => {
    // Review-B finding N1-r16: every store gate used whole numbers; r16c adds the directions where
    // rounding UP or to one decimal flips the result. An EQUAL fractional echo (30.2 twice) is not
    // a user move: the pin holds through it and the settle re-pins once — a stored Math.ceil or
    // Math.round (N-y-ceil, V-y-rounded) reads the echo as lower and releases where the head keeps.
    // A drop in the hundredths (30.44 → 30.42) IS a user move: the pin releases — a store rounded
    // to one decimal (N-y-toFixed1) makes both events read 30.4 and never releases. Asserts
    // behaviour only.
    // Kills (r16d results.json): N-y-ceil, N-y-toFixed1 (its own r16c seeds), V-y-rounded,
    // NV-sf1-heights-only-at-end, M-rb-r14-lasty-only-at-end, M-bar-rest-top, M-echo-unpins,
    // M-r13-race-back. Unique killer of none.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    const scrollToEnd = vi.fn();
    scroller.props.ref.current = { scrollToEnd };
    const event = (y: number, clip: number, content: number) => scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } });
    for (const [y, clip, content] of events) event(y, clip, content);
    scroller.props.onContentSizeChange(369, 200);
    expect(scrollToEnd).toHaveBeenCalledTimes(settles);
  });

  it.each([
    ["fraction-echo", [[30.4, 97, 188.4], [30.2, 97, 188]], 1],
    ["fraction-content-stable", [[30.4, 97, 188.4], [30.2, 97, 188.4]], 0],
    ["fraction-content-hundredths-stable", [[30.4, 97, 188.44], [30.2, 97, 188.44]], 0],
    ["fraction-clip-echo", [[30.4, 97.4, 188], [30.2, 97, 188]], 1],
  ] as Array<[string, Array<[number, number, number]>, number]>)("a fractional height is stored unrounded: the changed-geometry echo after a fraction is not a user scroll (%s, r16 N1-r16c, D2-r4)", (_name, events, settles) => {
    // Review-B finding N1-r16, heights half: the store keeps the exact fraction, so "changed
    // geometry" is judged on the real values. fraction-echo: at content 188.4 the next event's 188
    // is CHANGED geometry — the lower echo 30.2 must NOT release, and the settle re-pins once. A
    // stored Math.round/ceil on content (V-h-rounded, N-h-ceil, N-h-ceil-content-only) makes 188
    // read STABLE, the echo releases as a drag up and the settle has nothing to re-pin.
    // fraction-content-stable and its hundredths twin: a drop with the SAME fractional content is a
    // real user move and releases (settles 0) — rounding up on content (N-h-ceil,
    // N-h-ceil-content-only: 188.4 → 189 ≠ 188.4 keeps "changed" and holds the pin) flips it.
    // fraction-clip-echo: a clip fraction (97.4 → 97) is also changed geometry; rounding the clip
    // alone (N-h-round-clip-only) reads it stable and releases where the head keeps.
    // Asserts behaviour only.
    // Kills (r16d results.json): N-h-ceil, N-h-ceil-content-only, N-h-toFixed1,
    // N-h-round-clip-only (its own r16c seeds), V-h-rounded, V-y-rounded, M-r16-heights-above-stable,
    // M-r14-mirror-back, M-geom-ignored, M-geom-content-only, M-geom-clip-only, M-r13-race-back,
    // M-echo-unpins. Unique killer of none.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    const scrollToEnd = vi.fn();
    scroller.props.ref.current = { scrollToEnd };
    const event = (y: number, clip: number, content: number) => scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } });
    for (const [y, clip, content] of events) event(y, clip, content);
    scroller.props.onContentSizeChange(369, 200);
    expect(scrollToEnd).toHaveBeenCalledTimes(settles);
  });

  it("matches a test-side D2-r4 model over seeded random traces from the union alphabet and the edge classes (R38-STOP-r16c)", () => {
    // R38-STOP-r16c (LSA-adopted): near variants of the two store lines are closed by this gate, not
    // by an ever-longer row list. The REAL component must agree, probe by probe, with a test-side
    // model of the store rule. The model is written from the RULING TEXT of R38-D2-r4/D2-r3, NOT
    // from session-selection.tsx, and the rule is named beside each model line. Deterministic:
    // seed 8539, 2048 traces x 10 events, every trace from a real fresh mount (the beforeEach reset
    // plus harness.fresh() here). Alphabet: the union of review-B's five alphabets
    // (model/store-diff.cjs:91-95, numbers verbatim) plus the LSA edge classes — the end band on
    // BOTH sides of the boundary for every (clip, content) pair, overscroll past the end, a
    // negative offset (top bounce), content shorter than clip, a height that shrinks and stays
    // smaller and one that grows. Fractional values ride the fractional and hundredths alphabets;
    // repeated stable geometry arises from repeated draws; the first event after mount (heights
    // still −1) arises because every trace starts fresh. Determinism: the seed is fixed below.
    // Kills (r16d results.json, which caps failing_tests at 6 per seed): N-y-skip-rearm-stable,
    // N-h-content-kept-on-rearm, N-h-content-high-water-clip-same, N-h-clip-high-water-content-same,
    // N-h-content-high-water-at-end, N-h-clip-high-water-at-end, N-y-ceil, N-y-toFixed1, N-h-ceil,
    // N-h-ceil-content-only, N-h-toFixed1, N-h-round-clip-only, V-y-wasPinned, V-h-wasPinned,
    // V-h-content-high-water, V-h-clip-high-water, V-y-rounded, V-h-rounded, NV-sf1-only-changed-geometry,
    // M-geom-ignored, M-geom-content-only, M-geom-clip-only, M-r14-mirror-back,
    // M-r16-heights-above-stable, M-r16-heights-notAtEnd. Its full alone-run kill list — 40 of the
    // 92 seeds, including EVERY seed that edits session-selection.tsx:178-183 (R38-STOP-r16c
    // direction (b)), while all six equivalent mutants pass it (direction (c)) — is recorded in the
    // round evidence at runs/differential-directions.json. Unique killer of none of the 92's
    // seeds: every kill here is shared with a named row gate.
    const SEED = 8539, TRACES = 2048, EVENTS = 10;
    let s = SEED;
    const rnd = (n: number) => { s = (Math.imul(s, 1103515245) + 12345) & 0x7fffffff; return (s >>> 8) % n; };
    const cross = (ys: number[], clips: number[], contents: number[]) => ys.flatMap((y) => clips.flatMap((l) => contents.map((c) => [y, l, c] as [number, number, number])));
    const union: Array<[number, number, number]> = [
      ...cross([10, 30, 40, 50, 60, 70, 78, 91, 103], [97, 110], [188, 200]),
      ...cross([0, 36, 50, 60, 91], [86, 97, 108], [136, 144, 188]),
      ...cross([30.2, 30.4, 60, 91, 91.4], [97, 97.4], [188, 188.4]),
      ...cross([30.42, 30.44, 30.6, 60, 91], [97, 97.4], [188, 188.4]),
      ...cross([30.2, 30.4, 60, 91], [97, 97.44], [188, 188.44]),
    ];
    // Edge classes. End band: for every (clip, content) pair the boundary itself (y + clip =
    // content − 2, atEnd) and the first value outside it (content − clip − 3, not atEnd). Below
    // the band: a five-step ladder (−4 … −12) of stable-geometry offsets, so a trace can draw the
    // full same-geometry sequence a stable re-arm needs — release below the band, re-arm at the
    // band, then a lower offset inside the window the skipped re-arm store would have filled.
    const pairs = new Set(union.map(([, l, c]) => `${l}|${c}`));
    for (const key of pairs) {
      const [clip, content] = key.split("|").map(Number) as [number, number];
      union.push([content - clip - 2, clip, content], [content - clip - 3, clip, content]);
      for (const step of [4, 6, 8, 10, 12]) union.push([content - clip - step, clip, content]);
    }
    // Overscroll past the end; a negative offset (top bounce); content shorter than clip; a height
    // that shrinks and stays smaller; one that grows.
    union.push([120, 97, 188], [150, 110, 200], [-10, 97, 188], [-1, 110, 200], [0, 97, 80], [30, 110, 80], [60, 97, 80], [10, 97, 150], [10, 97, 250]);
    // Sampler: every event is drawn from the union pool above, but the seeded draw prefers the
    // previous event's (clip, content) 60% of the time, so traces exercise same-geometry
    // sequences — the shape a stable re-arm store (and its skip variants) actually lives in. The
    // bias changes which pool members are drawn, never which members exist.
    const byGeo = new Map<string, Array<[number, number, number]>>();
    for (const e of union) {
      const k = `${e[1]}|${e[2]}`;
      if (!byGeo.has(k)) byGeo.set(k, []);
      byGeo.get(k)!.push(e);
    }
    const t0 = Date.now();
    let geo: string | null = null;
    for (let t = 0; t < TRACES; t++) {
      geo = null;
      harness.fresh();
      const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
      const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
      // The probe's mock has NO side effects: repinToEnd reads pinnedToEnd and calls scrollToEnd,
      // writing no ref, so it reveals the pin in every state and leaves every ref unchanged.
      const scrollToEnd = vi.fn();
      scroller.props.ref.current = { scrollToEnd };
      // Model state — R38-D2-r4: the refs start at the fresh-mount values.
      let pinned = true;                                    // D2-r4: the bar mounts pinned to the end
      let lastY = 0, lastContent = -1, lastClip = -1;       // D2-r4: heights start at −1, so the first event can never unpin
      const seen: Array<[number, number, number]> = [];
      for (let i = 0; i < EVENTS; i++) {
        const sameGeo: Array<[number, number, number]> | null = geo !== null && rnd(100) < 60 ? byGeo.get(geo)! : null;
        const e: [number, number, number] = sameGeo ? sameGeo[rnd(sameGeo.length)]! : union[rnd(union.length)]!;
        geo = `${e[1]}|${e[2]}`;
        seen.push(e);
        const [y, clip, content] = e;
        scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } });
        const atEnd = y + clip >= content - 2;              // D2-r4: atEnd — the end band, y + clip >= content − 2
        const stable = lastContent === content && lastClip === clip; // D2-r4: stable geometry — both heights equal the previous scroll event's
        // D2-r4: atEnd re-arms; the event unpins only when NOT at the end, moving up (y < lastY),
        // AND stable — a lower offset under changed geometry is a settle artifact, not a user scroll.
        const nextPinned: boolean = atEnd || (pinned && (y >= lastY || !stable));
        lastY = y; lastContent = content; lastClip = clip;  // D2-r4: every event stores y and both heights
        pinned = nextPinned;                                // D2-r4: no re-pin happens in onScroll
        // R38-D2-r3: the probe (a content-size change) re-pins only while pinned — after a user
        // scroll-up the bar must never scroll back over them.
        const before = scrollToEnd.mock.calls.length;
        scroller.props.onContentSizeChange(369, 200);
        const fired = scrollToEnd.mock.calls.length > before;
        if (fired !== pinned) {
          throw new Error(`D2-r4 differential mismatch at trace ${t}, event ${i}: the model says ${pinned ? "PINNED (scrollToEnd expected)" : "RELEASED (no scrollToEnd)"}, the component ${fired ? "scrolled to the end" : "did not scroll"}. Events so far (y/clip/content): ${JSON.stringify(seen)}`);
        }
      }
    }
    console.log(`[r16d D2-r4 differential gate] seed ${SEED}, ${TRACES} traces x ${EVENTS} events, pool ${union.length} events, ${Date.now() - t0} ms`);
  });

  it("keeps the pin through a content-only settle (R14 F2 one-axis variant)", () => {
    // One-axis replay: the clip never changes; only contentSize.height moves under the echo.
    // M-geom-clip-only (guarding on the clip alone) reads stable geometry and releases here.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onLayout: () => void; onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: { scrollToEnd: (opts: { animated: boolean }) => void } } } } }).children;
    let y = 0; const clip = 97; let content = 188;
    const scrollToEnd = vi.fn(() => { y = content - clip; });
    scroller.props.ref.current = { scrollToEnd };
    const echo = (offset: number) => { y = offset; scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } }); };
    scroller.props.onLayout();
    echo(91);
    content = 136;
    scroller.props.onContentSizeChange(369, content);
    scroller.props.onLayout();
    echo(0);
    content = 188;
    scroller.props.onContentSizeChange(369, content);
    scroller.props.onLayout();
    expect(y).toBe(91);
  });

  it("keeps the pin through a clip-only settle (R14 F2 one-axis variant)", () => {
    // One-axis replay: contentSize.height never changes; only layoutMeasurement.height moves.
    // M-geom-content-only (guarding on the content alone) reads stable geometry and releases here.
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onLayout: () => void; onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: { scrollToEnd: (opts: { animated: boolean }) => void } } } } }).children;
    let y = 0; let clip = 97; const content = 188;
    const scrollToEnd = vi.fn(() => { y = content - clip; });
    scroller.props.ref.current = { scrollToEnd };
    const echo = (offset: number) => { y = offset; scroller.props.onScroll({ nativeEvent: { contentOffset: { y }, layoutMeasurement: { height: clip }, contentSize: { height: content } } }); };
    scroller.props.onLayout();
    echo(91);
    clip = 149;
    scroller.props.onLayout();
    echo(39);
    clip = 97;
    // The observed drop, echoed after the shrink and before the settle's own re-pin.
    echo(0);
    scroller.props.onLayout();
    expect(y).toBe(91);
  });

  it("unpins only on an upward scroll with stable geometry; a drag up never gets fought (R13/D2-r4)", () => {
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onLayout: () => void; onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; ref: { current: unknown } } } }).children;
    const scrollToEnd = vi.fn();
    (scroller.props.ref as { current: unknown }).current = { scrollToEnd };
    scroller.props.onContentSizeChange(369, 136);
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(2);
    // The echo of the mount pin, at its end: armed, offset and geometry remembered.
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 50 }, layoutMeasurement: { height: 86 }, contentSize: { height: 136 } } });
    // The content settles; the re-pins fire; the settled echo arms at the end (91/97/188).
    scroller.props.onContentSizeChange(369, 188);
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(4);
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 91 }, layoutMeasurement: { height: 97 }, contentSize: { height: 188 } } });
    // A user drag UP with STABLE geometry: the offset decreases while not at the end — the pin
    // releases, and no layout change fights the user.
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 20 }, layoutMeasurement: { height: 97 }, contentSize: { height: 188 } } });
    scroller.props.onContentSizeChange(369, 200);
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(4);
    // A further downward-but-not-at-end scroll (reading back) stays released.
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 60 }, layoutMeasurement: { height: 97 }, contentSize: { height: 188 } } });
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(4);
    // Dragging back to the end re-arms, and the next own-height change re-pins.
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 98 }, layoutMeasurement: { height: 97 }, contentSize: { height: 188 } } });
    scroller.props.onLayout();
    expect(scrollToEnd).toHaveBeenCalledTimes(5);
  });

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
    expect(limits.props).toMatchObject({ accessibilityRole: "button", accessibilityState: { expanded: false }, "aria-expanded": false, hitSlop: 6 });
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

  it("makes Limits a one-decision disclosure toggle per press through pointer and keyboard, with aria-expanded on the DOM (R9 AUD-F1/F3)", () => {
    helpDouble.open = false; helpDouble.holds = 0; helpDouble.dismisses = 0;
    const takeover = control(), auto = control(), archive = control();
    let tree = harness.current.render(() => walk(SelectionBar({ count: 1, takeover, auto, archive, colors: COLORS, disclosure: `Take over: ${TAKEOVER_EFFECTS}`, hasLimits: true })));
    const draw = () => { tree = harness.current.render(() => walk(SelectionBar({ count: 1, takeover, auto, archive, colors: COLORS, disclosure: `Take over: ${TAKEOVER_EFFECTS}`, hasLimits: true }))); };
    draw();
    const expanded = () => ({ aria: byTestId(tree, "selection-limits")!.props["aria-expanded"], state: (byTestId(tree, "selection-limits")!.props.accessibilityState as { expanded: boolean }).expanded });
    const pressLimits = (pointer: boolean) => {
      const node = byTestId(tree, "selection-limits")!.props;
      if (pointer) (node.onPressIn as (() => void) | undefined)?.();
      (node.onPress as () => void)();
    };
    // AUD-F1: the DOM carries aria-expanded beside the native state (RNW 0.21.4 maps neither
    // accessibilityState nor accessibilityExpanded for web readers).
    expect(expanded()).toEqual({ aria: false, state: false });
    // A pointer press decides ONCE from the press-start state: open -> closed -> open (AUD-F3).
    pressLimits(true); draw();
    expect(expanded()).toEqual({ aria: true, state: true });
    pressLimits(true); draw();
    expect(expanded()).toEqual({ aria: false, state: false });
    pressLimits(true); draw();
    expect(expanded()).toEqual({ aria: true, state: true });
    // Keyboard (Enter/Space) fires no press start: the decision comes from the live state — closing
    // from open and opening from closed, never double-toggling.
    pressLimits(false); draw();
    expect(expanded()).toEqual({ aria: false, state: false });
    pressLimits(false); draw();
    expect(expanded()).toEqual({ aria: true, state: true });
    // The press-start capture is consumed and reset by its own decision: after a pointer press, a
    // keyboard press closes and the NEXT keyboard press from closed OPENS (a stale capture would
    // keep dismissing and stay closed).
    pressLimits(true); draw();
    expect(expanded()).toEqual({ aria: false, state: false });
    pressLimits(false); draw();
    expect(expanded()).toEqual({ aria: true, state: true });
    expect(helpDouble.holds).toBeGreaterThanOrEqual(4);
  });

  it("makes ONE live-state decision per activation and needs no press capture (R11 R38-D1-r2)", () => {
    helpDouble.open = false; helpDouble.holds = 0; helpDouble.dismisses = 0; helpDouble.heldOpen = false;
    const takeover = control(), auto = control(), archive = control();
    let tree = harness.current.render(() => walk(SelectionBar({ count: 1, takeover, auto, archive, colors: COLORS, disclosure: `Take over: ${TAKEOVER_EFFECTS}`, hasLimits: true })));
    const draw = () => { tree = harness.current.render(() => walk(SelectionBar({ count: 1, takeover, auto, archive, colors: COLORS, disclosure: `Take over: ${TAKEOVER_EFFECTS}`, hasLimits: true }))); };
    const expanded = () => (byTestId(tree, "selection-limits")!.props.accessibilityState as { expanded: boolean }).expanded;
    const props = () => byTestId(tree, "selection-limits")!.props;
    const press = () => { (props().onPress as () => void)(); draw(); };
    // Held open: the press CLOSES, whether or not a mid-gesture root dismissal disturbed the state —
    // the decision is the live state at onPress; there is no press-start capture to consume or leak.
    helpDouble.open = true; helpDouble.heldOpen = true; draw();
    expect(expanded()).toBe(true);
    press();
    expect(expanded()).toBe(false);
    press();
    expect(expanded()).toBe(true);
    // Same press with the root-dismissal disturbance flushed before onPress (the web click order):
    // still one live decision — the press that opened it closes it again.
    helpDouble.open = true; draw();
    press();
    expect(expanded()).toBe(false);
    // A CANCELLED gesture (press down, released outside — the client's press events never fire
    // onPress): the next activation opens from the live state; no capture is left behind.
    helpDouble.open = true; helpDouble.heldOpen = true; draw();
    expect(expanded()).toBe(true);
    // the press-down itself touches nothing: onPress decides only when the activation lands
    helpDouble.open = false; draw();
    press();
    expect(expanded()).toBe(true);
  });

  it("stops the root touch-start on Limits only while HELD, and an unheld-focus tap opens the held popover (R11 R38-D1-r2, R10-F2)", () => {
    helpDouble.open = true; helpDouble.heldOpen = true; helpDouble.holds = 0; helpDouble.dismisses = 0;
    const takeover = control(), auto = control(), archive = control();
    let tree = harness.current.render(() => walk(SelectionBar({ count: 1, takeover, auto, archive, colors: COLORS, disclosure: `Take over: ${TAKEOVER_EFFECTS}`, hasLimits: true })));
    const draw = () => { tree = harness.current.render(() => walk(SelectionBar({ count: 1, takeover, auto, archive, colors: COLORS, disclosure: `Take over: ${TAKEOVER_EFFECTS}`, hasLimits: true }))); };
    const props = () => byTestId(tree, "selection-limits")!.props;
    expect(typeof props().onTouchStart).toBe("function");
    let stopped = 0;
    const evt = { stopPropagation: () => { stopped += 1; } };
    (props().onTouchStart as (e: unknown) => void)(evt);
    expect(stopped).toBe(1);
    // Native keyboard-focus help is unheld: the touch propagates, so the root dismissal runs.
    helpDouble.heldOpen = false; draw();
    (props().onTouchStart as (e: unknown) => void)(evt);
    expect(stopped).toBe(1);
    // The unheld-focus tap: the root touch-start dismissal closes the unheld help (the touch was
    // NOT stopped), and onPress then decides from the live closed state — the held popover OPENS.
    helpDouble.open = true; helpDouble.heldOpen = false; draw();
    helpDouble.open = false; draw();
    (props().onPress as () => void)(); draw();
    expect(helpDouble.open).toBe(true);
    expect(helpDouble.heldOpen).toBe(true);
  });

  it("renders no Limits control when the host states no limits, while the effects sentence is NOT clamped (R38 V2, R38-D2-r3 addendum)", () => {
    // Without limits there is no Limits control, so the full effects sentence shows unclamped and
    // scrolls in the bar like the other text (one conditional on hasLimits).
    let tree = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`, false).tree;
    const unclamped = byTestId(tree, "selection-takeover-disclosure")!;
    expect(unclamped).toBeDefined();
    expect(unclamped.props.numberOfLines).toBeUndefined();
    expect(byTestId(tree, "selection-limits")).toBeUndefined();
    // With limits, the clamp to two lines stays (the Limits popover holds the whole sentence).
    tree = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`, true).tree;
    expect(byTestId(tree, "selection-takeover-disclosure")!.props.numberOfLines).toBe(2);
    expect(byTestId(tree, "selection-limits")).toBeDefined();
  });

  it("starts the bar's scroll at its end when the content does not fit, and never fights a user who scrolled up (R38-D2-r3)", () => {
    helpDouble.open = false; helpDouble.holds = 0; helpDouble.dismisses = 0;
    const { tree } = bar(2, control(), control(), control(), `Take over: ${TAKEOVER_EFFECTS}`);
    const scroller = (byTestId(tree, "selection-bar")!.props as { children: { props: { onContentSizeChange: (w: number, h: number) => void; onScroll: (event: unknown) => void; scrollEventThrottle: number; ref: { current: unknown } } } }).children;
    // The bar wires its scroller for the rest-end pin: a content-size change re-pins to the end only
    // while the scroller sits at the end; a user scroll up un-pins it.
    expect(typeof scroller.props.onContentSizeChange).toBe("function");
    expect(typeof scroller.props.onScroll).toBe("function");
    // R38-D2-r3: RNW emits onScroll during a drag only when the throttle feeds it every frame.
    expect(scroller.props.scrollEventThrottle).toBe(16);
    const scrollToEnd = vi.fn();
    (scroller.props.ref as { current: unknown }).current = { scrollToEnd };
    scroller.props.onContentSizeChange(360, 300);
    expect(scrollToEnd).toHaveBeenCalledTimes(1);
    expect(scrollToEnd.mock.calls[0]![0]).toMatchObject({ animated: false });
    // The echo of that programmatic scroll, at the end for the 84 pt clip: the pin stays armed (R13).
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 216 }, layoutMeasurement: { height: 84 }, contentSize: { height: 300 } } });
    // A user scrolls up to read: the offset DECREASES, the pin releases...
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 0 }, layoutMeasurement: { height: 84 }, contentSize: { height: 300 } } });
    // ...so further content changes (a refetch, a label width settling) do not re-pin.
    scroller.props.onContentSizeChange(360, 300);
    expect(scrollToEnd).toHaveBeenCalledTimes(1);
    // Back at the end, the pin re-arms and the next content change re-pins again.
    scroller.props.onScroll({ nativeEvent: { contentOffset: { y: 216 }, layoutMeasurement: { height: 84 }, contentSize: { height: 300 } } });
    scroller.props.onContentSizeChange(360, 300);
    expect(scrollToEnd).toHaveBeenCalledTimes(2);
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
