// Provider-only cases reused from shared dependency 403960076464f2e40732f1a300e98fa987777f32.
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { View } from "react-native";
const hooks = vi.hoisted(() => ({ slots: [] as unknown[], cursor: 0, effects: [] as (() => void | (() => void))[], panelOpen: false, controller: null as unknown, platform: "web", backAdded: 0, backRemoved: 0 }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  useContext: () => hooks.controller, useId: () => "test-help",
  useRef: (current: unknown) => { const i = hooks.cursor++; return hooks.slots[i] ??= { current }; },
  useState: (value: unknown) => { const i = hooks.cursor++; if (!(i in hooks.slots)) hooks.slots[i] = value; return [hooks.slots[i], (next: unknown) => { hooks.slots[i] = next; }]; },
  useMemo: (factory: () => unknown) => { const i = hooks.cursor++; return hooks.slots[i] ??= factory(); },
  useEffect: (effect: () => void | (() => void)) => { const i = hooks.cursor++; if (!(i in hooks.slots)) { hooks.slots[i] = true; hooks.effects.push(effect); } },
}));
vi.mock("../client/anchored-menu", async (original) => ({ ...await original<typeof import("../client/anchored-menu")>(), anchoredPanelOpen: () => hooks.panelOpen }));
vi.mock("react-native", () => ({ Platform: { get OS() { return hooks.platform; } }, Pressable: "Pressable", View: "View", Text: "Text", ScrollView: "ScrollView", BackHandler: { addEventListener: () => { hooks.backAdded++; return { remove() { hooks.backRemoved++; } }; } } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { ControlHelpProvider, useControlHelp } from "../client/control-help";
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
  hooks.platform = "web"; hooks.backAdded = 0; hooks.backRemoved = 0;
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
  const control = useControlHelp("Explanation: no execution or freshness inferred.");
  control.bindings.ref(measure(20 + width - 34, 30 + height - 34, 28, 28) as View);
  return { control, render, tooltip: () => all(tree).find(e => e.props.role === "tooltip"), root };
}
describe("hover and focus help", () => {
  it.each(["ios", "android"])("mounts and cleans up on %s when native window aliases the global without DOM listeners", (platform) => {
    hooks.platform = platform;
    vi.stubGlobal("document", undefined);
    vi.stubGlobal("window", globalThis);
    expect(Reflect.get(globalThis, "window")).toBe(globalThis);
    expect("addEventListener" in globalThis).toBe(false);
    const screen = mount();
    expect(hooks.backAdded).toBe(1);
    screen.control.bindings.onFocus(); screen.render(); expect(screen.tooltip()).toBeDefined();
    screen.control.dismiss(); screen.render(); expect(screen.tooltip()).toBeUndefined();
    cleanup.forEach(fn => fn()); cleanup = [];
    expect(hooks.backRemoved).toBe(1);
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
    const style = screen.tooltip()!.props.style as { left: number; width: number; bottom: number; maxHeight: number };
    expect(style.left).toBeGreaterThanOrEqual(6); expect(style.left + style.width).toBeLessThanOrEqual(width - 6); expect(style.bottom).toBeGreaterThan(0); expect(style.maxHeight).toBeLessThanOrEqual(height - 12);
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
});
