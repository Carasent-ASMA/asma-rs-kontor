import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";

// Refs, state and effects persist across renders of one row, so a pending open can meet a later press or mode.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])),
  useContext: () => undefined,
  useId: () => "help" }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, View: "View", Text: "Text", Pressable: "Pressable", Image: "Image", ScrollView: "ScrollView",
  BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", copyText: async () => {},
  Modal: Object.assign(function Modal() { return null; }, { Content: function ModalContent() { return null; } }) }));

import { DenseSessionRow, type DenseRowMode } from "../client/dense-session-row";
import { SessionActionsMenu } from "../client/session-actions-menu";
import type { SessionTotals } from "../client/session-row-format";
import { sessionEntry } from "./session-fixtures";

type Element = ReactElement<Record<string, unknown>>;
function elements(value: ReactNode): Element[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(value)) return [];
  return [value, ...elements(value.props.children as ReactNode)];
}
const byTestId = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id)!;
const text = (node: Element) => elements(node).filter((child) => child.type === "Text").map((child) => [child.props.children].flat(Infinity).join("")).join("\n");

const NOW = Date.parse("2026-10-22T12:00:00.000Z");
const COLORS = { surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", accent: "#00f", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;

function row(mode?: DenseRowMode, totals?: SessionTotals | null, note?: string) {
  harness.fresh();
  const onPress = vi.fn(), onSelectStart = vi.fn();
  const draw = (next = mode) => {
    const tree = elements(harness.current.render(() => DenseSessionRow({ entry: sessionEntry("agent-a", { title: "Route review" }), colors: COLORS, clock: { now: NOW, zone: "UTC" },
      opened: null, totals, note, spokenDetail: "Native ID agent-a", mode: next, onPress, onSelectStart })));
    return { tree, body: byTestId(tree, "session-row-body"), state: byTestId(tree, "session-row-state") };
  };
  return { ...draw(), draw, onPress, onSelectStart };
}
const tap = (node: Element) => (node.props.onPress as () => void)();
const settle = () => new Promise((resolve) => setTimeout(resolve, 320));

afterEach(() => { vi.useRealTimers(); });

describe("DenseSessionRow", () => {
  it.each([["select", "checkbox"], ["pick", "radio"]] as const)("toggles once from the body in %s mode, with its %s inside the body", (kind, role) => {
    const { tree, body, onPress } = row({ kind, selected: false });
    expect(body.props.role).toBe(role);
    expect(body.props.accessibilityState).toEqual({ checked: false, disabled: false, busy: false });
    tap(body);
    expect(onPress).toHaveBeenCalledTimes(1);
    const inside = elements(body);
    expect(inside).toContain(byTestId(tree, "session-row-choice"));
    expect(inside.filter((node) => node.props.onPress)).toEqual([body]);
  });

  it("opens after the double-tap window on a single tap of the state icon", () => {
    vi.useFakeTimers();
    const { state, onPress, onSelectStart } = row();
    tap(state);
    expect(onPress).not.toHaveBeenCalled();
    vi.advanceTimersByTime(279);
    expect(onPress).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(onPress).toHaveBeenCalledTimes(1);
    expect(onSelectStart).not.toHaveBeenCalled();
  });

  it("enters selection on a double tap of the state icon without opening", () => {
    vi.useFakeTimers();
    const { state, onPress, onSelectStart } = row();
    tap(state);
    vi.advanceTimersByTime(200);
    tap(state);
    vi.advanceTimersByTime(1_000);
    expect(onSelectStart).toHaveBeenCalledTimes(1);
    expect(onPress).not.toHaveBeenCalled();
  });

  it("toggles a selectable row once on Space, ignoring a held key and other keys", () => {
    const { body, onPress } = row({ kind: "select", selected: false });
    const key = (name: string, repeat = false) => (body.props.onKeyDown as (event: object) => void)({ key: name, repeat, preventDefault: () => {}, stopPropagation: () => {} });
    key(" "); key(" ", true); key("a");
    expect(onPress).toHaveBeenCalledTimes(1);
    expect(row().body.props.onKeyDown).toBeUndefined();
  });

  it("keeps a row that cannot be chosen in Tab with its reason spoken, and Space leaves it alone", () => {
    const { body, onPress } = row({ kind: "select", selected: false, unavailable: "Archived sessions are not selected." });
    expect(body.props).toMatchObject({ disabled: true, tabIndex: 0, "aria-disabled": true });
    expect(body.props.accessibilityLabel).toContain("Archived sessions are not selected.");
    (body.props.onKeyDown as (event: object) => void)({ key: " ", preventDefault: () => {}, stopPropagation: () => {} });
    expect(onPress).not.toHaveBeenCalled();
  });

  it("draws a checked mark in the green where it keeps 3:1, and in the foreground on the dark surfaces where it does not", () => {
    const mark = (colors: object) => {
      harness.fresh();
      const tree = elements(harness.current.render(() => DenseSessionRow({ entry: sessionEntry("agent-a", { title: "Route review" }), colors: colors as never, clock: { now: NOW, zone: "UTC" },
        opened: null, mode: { kind: "select", selected: true }, onPress: vi.fn() })));
      return elements(byTestId(tree, "session-row-choice")!).find((node) => node.props.name === "CircleCheck")!.props.color;
    };
    // The prototype host's two palettes; the forest green is 2.5:1 on the dark row surface.
    const light = { surface0: "#ffffff", surface1: "#fafafa", surface2: "#f4f4f5", foreground: "#1a1a1e", foregroundMuted: "#71717a", accent: "#20744A" };
    const dark = { surface0: "#181B1A", surface1: "#1E2120", surface2: "#272A29", foreground: "#fafafa", foregroundMuted: "#A1A5A4", accent: "#20744A" };
    expect(mark(light)).toBe("#20744A");
    expect(mark(dark)).toBe("#fafafa");
  });

  it("keeps an unavailable choice listed but not pressable, and says why", () => {
    const { tree, body } = row({ kind: "select", selected: false, unavailable: "Archived sessions are not selected for archive." });
    expect(body.props).toMatchObject({ disabled: true, "aria-disabled": true, accessibilityState: { checked: false, disabled: true } });
    expect(body.props.accessibilityLabel).toContain("Archived sessions are not selected for archive.");
    expect(byTestId(tree, "session-row-choice").props.style).toMatchObject({ opacity: 0.4 });
  });

  it("shows and speaks the note, and speaks the detail without showing it", () => {
    const { tree, body } = row(undefined, undefined, "Earlier error · now closed");
    expect(text(byTestId(tree, "session-row-note"))).toBe("Earlier error · now closed");
    expect(body.props.accessibilityLabel).toContain("Earlier error · now closed");
    expect(body.props.accessibilityLabel).toContain("Native ID agent-a");
    expect(text(tree[0]!)).not.toContain("Native ID agent-a");
  });

  // Real timers: the pending open is a real setTimeout, and nothing may fire after the window.
  it("drops a pending state-icon open when the body is pressed, so the session opens once", async () => {
    const { state, body, onPress } = row();
    tap(state);
    tap(body);
    expect(onPress).toHaveBeenCalledTimes(1);
    await settle();
    expect(onPress).toHaveBeenCalledTimes(1);
  });

  it("drops a pending state-icon open on a long press, which only selects", async () => {
    const { state, body, onPress, onSelectStart } = row();
    tap(state);
    (body.props.onLongPress as () => void)();
    await settle();
    expect(onSelectStart).toHaveBeenCalledTimes(1);
    expect(onPress).not.toHaveBeenCalled();
  });

  it("drops a pending state-icon open when the row's menu is touched", async () => {
    const { state, onPress, draw } = row();
    const withMenu = draw({ kind: "open", actions: "menu" });
    tap(withMenu.state);
    const wrapper = withMenu.tree.find((node) => typeof node.props.onStartShouldSetResponderCapture === "function")!;
    expect((wrapper.props.onStartShouldSetResponderCapture as () => boolean)()).toBe(false);
    await settle();
    expect(onPress).not.toHaveBeenCalled();
    expect(state).toBeDefined();
  });

  it("drops a pending state-icon open when the row enters selection before the window ends", async () => {
    const { state, onPress, draw } = row();
    tap(state);
    draw({ kind: "select", selected: true });
    await settle();
    expect(onPress).not.toHaveBeenCalled();
  });

  // R9: one delayed open is shared by every row, so a later intent anywhere in the list drops an earlier row's open.
  it("drops row A's pending open when row B's menu is touched within the window", async () => {
    const a = row();
    tap(a.state);
    const b = row(undefined).draw({ kind: "open", actions: "menu" });
    const wrapper = b.tree.find((node) => typeof node.props.onStartShouldSetResponderCapture === "function")!;
    (wrapper.props.onStartShouldSetResponderCapture as () => boolean)();
    await settle();
    expect(a.onPress).not.toHaveBeenCalled();
  });

  it("drops row A's pending open when a menu opens by keyboard, which no touch precedes", async () => {
    const a = row();
    tap(a.state);
    harness.fresh();
    const menu = elements(harness.current.render(() => SessionActionsMenu({ entry: { id: "agent-b", title: "Billing" }, colors: COLORS, actions: [] })));
    // Enter or Space on the focused ellipsis presses it; no responder capture runs first.
    (menu.find((node) => node.props["aria-haspopup"] === "menu")!.props.onPress as () => void)();
    await settle();
    expect(a.onPress).not.toHaveBeenCalled();
  });

  it("lets row B's state tap replace row A's pending open, so only B opens, once", async () => {
    const a = row();
    tap(a.state);
    const b = row();
    tap(b.state);
    await settle();
    expect(a.onPress).not.toHaveBeenCalled();
    expect(b.onPress).toHaveBeenCalledTimes(1);
    expect(b.onSelectStart).not.toHaveBeenCalled();
  });

  it("drops row A's pending open when row B's body is pressed or held", async () => {
    const a = row();
    tap(a.state);
    tap(row().body);
    await settle();
    expect(a.onPress).not.toHaveBeenCalled();
    const c = row();
    tap(c.state);
    (row().body.props.onLongPress as () => void)();
    await settle();
    expect(c.onPress).not.toHaveBeenCalled();
  });

  it("enters selection on a long press of the body", () => {
    const { body, onPress, onSelectStart } = row();
    (body.props.onLongPress as () => void)();
    expect(onSelectStart).toHaveBeenCalledTimes(1);
    expect(onPress).not.toHaveBeenCalled();
  });

  it("labels provider cumulative values in visible and spoken row content", () => {
    const { tree, body } = row(undefined, { input: 1200, output: 8, scope: "provider-cumulative", source: "Fixture cumulative; history unverified" });
    expect(text(byTestId(tree, "session-row-tokens"))).toBe("reported ↓ 1.2k   ↑ 8");
    expect(body.props.accessibilityLabel).toContain("Provider-reported cumulative: 1.2k input, 8 output tokens");
    expect(body.props.accessibilityLabel).not.toContain("Session totals:");
  });

  it.each([["absent", undefined], ["null", null], ["both unknown", { input: null, output: null }]] as const)("reads unknown totals as dashes, never zero, when %s", (_, totals) => {
    const { tree, body } = row(undefined, totals);
    expect(text(byTestId(tree, "session-row-tokens"))).toBe("↓ —   ↑ —");
    expect(body.props.accessibilityLabel).toContain("Session token totals unknown");
  });
});

it("shows recorded child response usage separately while cumulative totals stay unknown", () => {
 const { tree, body } = row(undefined, { input: null, output: null, scope: "provider-cumulative", source: "Fork cumulative unqualified",
  recordedChildUsage: { scope: "recorded-child-responses", label: "Recorded child response usage", qualification: "Excludes parent history; completeness and billing unverified.", input: 1200, output: 20, total: 1220, responses: 2 } });
 expect(text(byTestId(tree, "session-row-tokens"))).toContain("↓ —");
 const detail = text(byTestId(tree, "session-row-recorded-child-usage"));
 expect(detail).toContain("Recorded child response usage: ↓ 1,200 / ↑ 20");
 expect(detail).toContain("Excludes parent history; completeness and billing unverified.");
 expect(body.props.accessibilityLabel).toContain("Recorded child response usage");
});
