import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { View } from "react-native";
const hooks = vi.hoisted(() => ({ slots: [] as unknown[], cursor: 0, effects: [] as (() => void | (() => void))[], panelOpen: false, controller: null as unknown }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  useContext: () => hooks.controller, useId: () => "test-help",
  useRef: (current: unknown) => { const i = hooks.cursor++; return hooks.slots[i] ??= { current }; },
  useState: (value: unknown) => { const i = hooks.cursor++; if (!(i in hooks.slots)) hooks.slots[i] = value; return [hooks.slots[i], (next: unknown) => { hooks.slots[i] = next; }]; },
  useMemo: (factory: () => unknown) => { const i = hooks.cursor++; return hooks.slots[i] ??= factory(); },
  useEffect: (effect: () => void | (() => void)) => { const i = hooks.cursor++; if (!(i in hooks.slots)) { hooks.slots[i] = true; hooks.effects.push(effect); } },
}));
vi.mock("../client/anchored-menu", async (original) => ({ ...await original<typeof import("../client/anchored-menu")>(), anchoredPanelOpen: () => hooks.panelOpen }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, Pressable: "Pressable", View: "View", Text: "Text", ScrollView: "ScrollView", BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { ControlHelpProvider, useControlHelp } from "../client/control-help";
import { compactUsage } from "../client/session-usage-density";
import { Action } from "../client/page-controls";
type Element = ReactElement<Record<string, unknown>>;
function all(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(all);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...all(node.props.children as ReactNode)];
}
const measure = (x: number, y: number, width: number, height: number) => ({ measureInWindow: (callback: (...values: number[]) => void) => callback(x, y, width, height) });
const colors = {} as Parameters<typeof ControlHelpProvider>[0]["colors"];
let cleanup: (() => void)[] = [];
let handlers: Map<string, (event: never) => void>;
beforeEach(() => {
  vi.useFakeTimers(); hooks.slots = []; hooks.cursor = 0; hooks.effects = []; hooks.controller = null; hooks.panelOpen = false;
  handlers = new Map();
  const target = { addEventListener: (type: string, fn: (event: never) => void) => handlers.set(type, fn), removeEventListener: (type: string) => handlers.delete(type) };
  vi.stubGlobal("document", target); vi.stubGlobal("window", target);
});
afterEach(() => { cleanup.forEach(fn => fn()); cleanup = []; vi.useRealTimers(); vi.unstubAllGlobals(); });
function mount(width = 390, height = 320) {
  let tree: ReactNode;
  const render = () => { hooks.cursor = 0; tree = ControlHelpProvider({ children: "screen", colors }); return tree; };
  render();
  hooks.controller = (tree as Element).props.value;
  const root = all(tree).find(e => e.type === "View")!;
  (root.props.ref as { current: unknown }).current = measure(20, 30, width, height);
  cleanup = hooks.effects.map(fn => fn()).filter((fn): fn is () => void => typeof fn === "function");
  const controlCursor = hooks.cursor;
  const control = useControlHelp("Explanation: no execution or freshness inferred.");
  const node = measure(20 + width - 34, 30 + height - 34, 28, 28) as View;
  control.bindings.ref(node);
  const update = (text: string) => {
    hooks.cursor = controlCursor;
    const next = useControlHelp(text);
    // React commits the changed callback ref after rendering the control.
    control.bindings.ref(null); next.bindings.ref(node);
    return next;
  };
  return { control, update, render, tooltip: () => all(tree).find(e => e.props.role === "tooltip"), root };
}
describe("hover and focus help", () => {
  it.each(["mouse", "touch"])("does not open focus help during an ordinary %s press, but keeps keyboard help", (pointerType) => {
    const screen = mount();
    handlers.get("pointerdown")?.({ pointerType, target: {} } as never);
    screen.control.bindings.onFocus(); screen.render(); expect(screen.tooltip()).toBeUndefined();
    screen.control.bindings.onBlur();
    handlers.get("keydown")!({ key: "Tab" } as never);
    screen.control.bindings.onFocus(); screen.render(); expect(screen.tooltip()).toBeDefined();
  });
  it("dismisses focused help immediately on an outside press", () => {
    const screen = mount(); screen.control.bindings.onFocus(); screen.render(); expect(screen.tooltip()).toBeDefined();
    handlers.get("pointerdown")?.({ pointerType: "mouse", target: {} } as never);
    screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
  it("does not reopen help revealed under a stationary pointer when a panel closes", () => {
    const screen = mount();
    handlers.get("pointerdown")!({ pointerType: "mouse", clientX: 100, clientY: 80 } as never);
    handlers.get("pointerover")!({ pointerType: "mouse", clientX: 100, clientY: 80 } as never);
    screen.control.bindings.onHoverIn(); screen.control.bindings.onFocus(); screen.render();
    expect(screen.tooltip()).toBeUndefined();
    screen.control.bindings.onBlur();
    handlers.get("pointerover")!({ pointerType: "mouse", clientX: 101, clientY: 80 } as never);
    screen.control.bindings.onHoverIn(); screen.render(); expect(screen.tooltip()).toBeDefined();
  });
  it("keeps help readable when a pointer presses inside its own panel", () => {
    const screen = mount(); screen.control.bindings.onFocus(); screen.render();
    const child = {};
    (screen.tooltip()!.props.ref as { current: unknown }).current = { contains: (target: unknown) => target === child };
    handlers.get("pointerdown")?.({ pointerType: "mouse", target: child } as never);
    screen.render(); expect(screen.tooltip()).toBeDefined();
  });
  it("keeps touch-generated web focus on the action instead of opening supplemental help", () => {
    const screen = mount();
    (screen.root.props.onTouchStart as (() => void) | undefined)?.();
    screen.control.bindings.onFocus(); screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
  it("fits a short status to measured content and keeps it beside a right-edge anchor", () => {
    const screen = mount(1400, 800); screen.update("Idle").bindings.onFocus(); screen.render();
    (screen.tooltip()!.props.onLayout as ((event: unknown) => void) | undefined)?.({ nativeEvent: { layout: { width: 46, height: 40 } } });
    screen.render();
    const style = screen.tooltip()!.props.style as { maxWidth: number; left: number };
    expect(style.maxWidth).toBe(47); // Leave room for a rounded-down text measurement.
    expect(style.left).toBe(1400 - 47 - 6);
    screen.update("A longer explanation that still needs the available reading width."); screen.render();
    expect((screen.tooltip()!.props.style as { maxWidth: number }).maxWidth).toBe(360);
  });
  it.each(["hover", "focus"])("refreshes changed text during stationary %s without another input event", (mode) => {
    const screen = mount();
    const control = screen.update("Idle");
    if (mode === "hover") control.bindings.onHoverIn(); else control.bindings.onFocus();
    screen.render(); expect(all(screen.tooltip()).some(e => e.props.children === "Idle")).toBe(true);
    const updated = screen.update("Running"); screen.render();
    expect(all(screen.tooltip()).some(e => e.props.children === "Running")).toBe(true);
    expect(updated.bindings["aria-describedby"]).toBe(screen.tooltip()!.props.nativeID);
    if (mode === "focus") {
      updated.bindings.onHoverOut(); vi.advanceTimersByTime(1000); screen.render(); expect(screen.tooltip()).toBeDefined();
    }
  });
  it("does not reopen escaped help when the same control commits new text", () => {
    const screen = mount(); const control = screen.update("Idle"); control.bindings.onFocus(); screen.render();
    handlers.get("keydown")!({ key: "Escape", preventDefault() {}, stopPropagation() {} } as never);
    const updated = screen.update("Running"); screen.render(); expect(screen.tooltip()).toBeUndefined();
    updated.bindings.onBlur(); updated.bindings.onFocus(); screen.render();
    expect(all(screen.tooltip()).some(e => e.props.children === "Running")).toBe(true);
  });
  it("keeps the pointer-leave deadline when active text changes and ignores other anchors", () => {
    const screen = mount(); const control = screen.update("Idle");
    const other = useControlHelp("Other control"); const otherNode = measure(40, 50, 28, 28) as View;
    other.bindings.ref(otherNode); control.bindings.onHoverIn(); screen.render(); other.bindings.ref(otherNode);
    screen.render(); expect(all(screen.tooltip()).some(e => e.props.children === "Idle")).toBe(true);
    control.bindings.onHoverOut(); vi.advanceTimersByTime(80);
    screen.update("Running"); screen.render(); expect(all(screen.tooltip()).some(e => e.props.children === "Running")).toBe(true);
    vi.advanceTimersByTime(80); screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
  it("dismisses help when the active control no longer has an explanation", () => {
    const screen = mount(); screen.control.bindings.onFocus(); screen.render();
    expect(screen.update("").bindings["aria-describedby"]).toBeUndefined(); screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
  it("appears on focus, stays while the pointer moves away, and Escape dismisses without moving focus", () => {
    const screen = mount(); screen.control.bindings.onFocus(); screen.render(); expect(screen.tooltip()).toBeDefined();
    expect(screen.control.bindings["aria-describedby"]).toBe(screen.tooltip()!.props.nativeID);
    screen.control.bindings.onHoverOut(); vi.advanceTimersByTime(1000); screen.render(); expect(screen.tooltip()).toBeDefined();
    const event = { key: "Escape", preventDefault: vi.fn(), stopPropagation: vi.fn(), stopImmediatePropagation: vi.fn() };
    handlers.get("keydown")!(event as never); screen.render(); expect(screen.tooltip()).toBeUndefined(); expect(event.stopImmediatePropagation).toHaveBeenCalled();
    vi.advanceTimersByTime(1000); screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
  it("lets one Escape also close an enclosing dialog or menu with its existing focus return", () => {
    const screen = mount(); screen.control.bindings.onFocus(); screen.render(); hooks.panelOpen = true;
    const event = { key: "Escape", preventDefault: vi.fn(), stopPropagation: vi.fn(), stopImmediatePropagation: vi.fn() };
    handlers.get("keydown")!(event as never); screen.render(); expect(screen.tooltip()).toBeUndefined();
    expect(event.preventDefault).not.toHaveBeenCalled(); expect(event.stopImmediatePropagation).not.toHaveBeenCalled();
  });
  it("allows the pointer to enter the tooltip and read indefinitely, then leaves after a short bridge", () => {
    const screen = mount(); screen.control.bindings.onHoverIn(); screen.render(); screen.control.bindings.onHoverOut();
    (screen.tooltip()!.props.onPointerEnter as () => void)(); vi.advanceTimersByTime(10000); screen.render(); expect(screen.tooltip()).toBeDefined();
    (screen.tooltip()!.props.onPointerLeave as () => void)(); vi.advanceTimersByTime(160); screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
  it.each([[320, 240], [390, 320], [1400, 800]])("clamps help inside a %s by %s viewport and opens upward at the bottom", (width, height) => {
    const screen = mount(width, height); screen.control.bindings.onFocus(); screen.render();
    const style = screen.tooltip()!.props.style as { left: number; maxWidth: number; bottom: number; maxHeight: number };
    expect(style.left).toBeGreaterThanOrEqual(6); expect(style.left + style.maxWidth).toBeLessThanOrEqual(width - 6); expect(style.bottom).toBeGreaterThan(0); expect(style.maxHeight).toBeLessThanOrEqual(height - 12);
    expect(all(screen.tooltip()).some(e => e.props.accessibilityRole === "button")).toBe(false);
  });
  it("keeps focus help through source scrolling; hover help closes and tooltip scrolling stays usable", () => {
    const screen = mount(); screen.control.bindings.onFocus(); screen.render();
    const panel = screen.tooltip()!; const child = {};
    (panel.props.ref as { current: unknown }).current = { contains: (target: unknown) => target === child };
    handlers.get("scroll")!({ target: child } as never); screen.render(); expect(screen.tooltip()).toBeDefined();
    handlers.get("scroll")!({ target: {} } as never); screen.render(); expect(screen.tooltip()).toBeDefined();
    screen.control.bindings.onBlur(); vi.advanceTimersByTime(160); screen.render();
    screen.control.bindings.onHoverIn(); screen.render(); handlers.get("scroll")!({ target: {} } as never); screen.render(); expect(screen.tooltip()).toBeUndefined();
    screen.control.bindings.onHoverIn(); screen.render(); handlers.get("resize")!({} as never); screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
  it("keeps touch labels and normal activation; it never makes an action a help-only tap", () => {
    const onPress = vi.fn(); const button = Action({ label: "Copy Markdown", icon: "Copy", iconOnly: true, colors, onPress }) as Element;
    expect(all(button).some(e => e.props.children === "Copy Markdown")).toBe(true);
    (button.props.onPress as () => void)(); expect(onPress).toHaveBeenCalledOnce();
  });
});
describe("compact metadata", () => {
  it("preserves explicit zeros, missing counters, and custom IDs without pretending a selection ran", () => {
    const missing = compactUsage({ provider: "opaque", model: null }); expect(missing.model).toBe("—"); expect(missing.modelIcon).toBe("CircleHelp"); expect(missing.tokens).toBe("—");
    const selected = compactUsage({ provider: "codex", model: "gpt-6.1-sol", telemetry: { lastUsage: { inputTokens: 0 } } });
    expect(selected.model).toBe("sol"); expect(selected.modelIcon).toBe("SlidersHorizontal"); expect(selected.tokens).toBe("Last report 0 ↓ · — ↑"); expect(selected.usageHelp).toContain("Output count not reported");
    expect(compactUsage({ provider: "custom", model: "my-custom-id" }).model).toBe("my-custom-id");
  });
  it("distinguishes the last runtime report and keeps the stale-switch and attribution caveats", () => {
    const report = compactUsage({ provider: "claude", model: "new-selection", telemetry: { runtimeInfo: { sessionId: null, provider: "claude", model: "new-selection", extra: { runtimeModel: "older-reported-model" } } } });
    expect(report.model).toBe("older-reported-model"); expect(report.modelIcon).toBe("Radio"); expect(report.modelHelp).toContain("precede a switch"); expect(report.usageHelp).not.toContain("runtime model");
  });
});
