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
import { ControlHelpProvider, useActionHelp, useControlHelp } from "../client/control-help";
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
function mount(width = 390, height = 320, longPress = false, text = "Explanation: no execution or freshness inferred.") {
  let tree: ReactNode;
  const render = () => { hooks.cursor = 0; tree = ControlHelpProvider({ children: "screen", colors }); return tree; };
  render();
  hooks.controller = (tree as Element).props.value;
  const root = all(tree).find(e => e.type === "View")!;
  (root.props.ref as { current: unknown }).current = measure(20, 30, width, height);
  cleanup = hooks.effects.map(fn => fn()).filter((fn): fn is () => void => typeof fn === "function");
  const control = longPress ? useActionHelp(text) : useControlHelp(text);
  const controlCleanup = longPress ? hooks.effects.at(-1)?.() : undefined;
  if (controlCleanup) cleanup.push(controlCleanup);
  control.bindings.ref(measure(20 + width - 34, 30 + height - 34, 28, 28) as View);
  return { control: control as ReturnType<typeof useActionHelp>, controlCleanup, render, tooltip: () => all(tree).find(e => e.props.role === "tooltip"), root };
}
describe("deliberate long-press help", () => {
  it.each(["ios", "android"])("empty help on %s never measures or renders on focus, hold or long press", (platform) => {
    hooks.platform = platform;
    const screen = mount(390, 320, true, "");
    const measureInWindow = vi.fn();
    (screen.root.props.ref as { current: unknown }).current = { measureInWindow };
    screen.control.bindings.onFocus();
    screen.control.hold();
    screen.control.bindings.onPressIn!({ nativeEvent: { pageX: 10, pageY: 20 } } as never);
    screen.control.bindings.onLongPress!();
    screen.render();
    expect(measureInWindow).not.toHaveBeenCalled();
    expect(screen.tooltip()).toBeUndefined();
  });
  const touch = (x = 10, y = 20) => ({ nativeEvent: { pageX: x, pageY: y } }) as Parameters<NonNullable<ReturnType<typeof useActionHelp>["bindings"]["onPressIn"]>>[0];
  it.each(["web", "ios", "android"])("opens on a recognized %s hold, stays after release, and closes on the next touch", platform => {
    hooks.platform = platform;
    const screen = mount(390, 320, true);
    (screen.root.props.onTouchStart as () => void)();
    screen.control.bindings.onPressIn!(touch()); screen.render(); expect(screen.tooltip()).toBeUndefined();
    screen.control.bindings.onLongPress!(); screen.control.bindings.onPressOut!();
    screen.render(); expect(screen.tooltip()).toBeDefined();
    screen.control.bindings.onHoverOut(); vi.advanceTimersByTime(1000); screen.render(); expect(screen.tooltip()).toBeDefined();
    (screen.root.props.onTouchStart as () => void)(); screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
  it.each(["move", "cancel", "pointercancel", "release"])("ignores a late hold callback after %s", reason => {
    const screen = mount(390, 320, true);
    screen.control.bindings.onPressIn!(touch());
    if (reason === "move") screen.control.bindings.onTouchMove!(touch(30, 20));
    if (reason === "cancel") screen.control.bindings.onTouchCancel!();
    if (reason === "pointercancel") screen.control.bindings.onPointerCancel!();
    if (reason === "release") screen.control.bindings.onPressOut!();
    screen.control.bindings.onLongPress!(); screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
  it("dismisses held help on scroll, pointer cancellation, Escape and unmount", () => {
    const screen = mount(390, 320, true);
    const hold = () => { screen.control.bindings.onPressIn!(touch()); screen.control.bindings.onLongPress!(); screen.render(); expect(screen.tooltip()).toBeDefined(); };
    hold(); handlers.get("scroll")!({ target: {} } as never); screen.render(); expect(screen.tooltip()).toBeUndefined();
    hold(); handlers.get("pointercancel")!({} as never); screen.render(); expect(screen.tooltip()).toBeUndefined();
    hold(); handlers.get("keydown")!({ key: "Escape", preventDefault() {}, stopPropagation() {} } as never); screen.render(); expect(screen.tooltip()).toBeUndefined();
    hold(); screen.controlCleanup?.(); screen.render(); expect(screen.tooltip()).toBeUndefined();
  });
});
describe("hover and focus help", () => {
  it.each(["ios", "android"])("shows help on later %s keyboard/accessibility focus after a root touch without a DOM key event", (platform) => {
    hooks.platform = platform;
    vi.stubGlobal("document", undefined);
    vi.stubGlobal("window", globalThis);
    const screen = mount();
    (screen.root.props.onTouchStart as (() => void) | undefined)?.();
    screen.render(); expect(screen.tooltip()).toBeUndefined();
    screen.control.bindings.onFocus(); screen.render(); expect(screen.tooltip()).toBeDefined();
    expect(handlers.size).toBe(0);
  });
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
});

describe("R10 R9-F2: the press exception covers only the anchor that holds the popover", () => {
  it("dismisses unheld help on a root press at its own source, and preserves the held one", () => {
    const child = { kind: "child HostInstance" };
    const anchor = { current: Object.assign(measure(100, 200, 44, 44), { contains: (target: unknown) => target === child }) as unknown as View };
    const screen = mount(390, 320, false, "unused");
    const controller = hooks.controller as { show(anchor: unknown, text: string, focused: boolean, held: boolean): void };
    // Unheld (hover) help on this anchor: a root press landing on its own child STILL dismisses —
    // the exception covers only the anchor that HOLDS the popover (QA seed 7 regression).
    controller.show(anchor, "Hover explanation", false, false);
    screen.render();
    expect(screen.tooltip()).toBeDefined();
    handlers.get("pointerdown")?.({ pointerType: "mouse", target: child, clientX: 120, clientY: 220 } as never);
    screen.render();
    expect(screen.tooltip()).toBeUndefined();
    // The help that is HELD on the same anchor survives the same root press at its child: one
    // toggle decision belongs to the holding control's own activation.
    controller.show(anchor, "Held explanation", false, true);
    screen.render();
    expect(screen.tooltip()).toBeDefined();
    handlers.get("pointerdown")?.({ pointerType: "mouse", target: child, clientX: 120, clientY: 220 } as never);
    screen.render();
    expect(screen.tooltip()).toBeDefined();
  });
});

describe("R12 T1: subscribe notifies (open, held) — held is the provider's own flag, not a mirror of open", () => {
  it("unheld help gives (true, false), held help (true, true), closed (false, false)", () => {
    hooks.slots = []; hooks.cursor = 0; hooks.effects = []; hooks.controller = null;
    let tree: ReactNode;
    const render = () => { hooks.cursor = 0; tree = ControlHelpProvider({ children: "screen", colors }); return tree; };
    render();
    const controller = (tree as Element).props.value as { subscribe(anchor: unknown, listener: (open: boolean, held: boolean) => void): () => void; show(anchor: unknown, text: string, focused?: boolean, held?: boolean): void; dismiss(): void };
    const root = all(tree).find((e) => e.type === "View")!;
    (root.props.ref as { current: unknown }).current = measure(0, 0, 390, 320);
    cleanup.push(...hooks.effects.map((fn) => fn()).filter((fn): fn is () => void => typeof fn === "function"));
    const anchor = { current: measure(20, 30, 100, 30) as View };
    const seen: Array<[boolean, boolean]> = [];
    controller.subscribe(anchor, (open, held) => seen.push([open, held]));
    // The notification loop is the provider's own effect and this harness registers effects once;
    // re-arm it (drop its slot so the next render re-registers with the fresh help) per step.
    const rearm = (act: () => void) => {
      const slot = hooks.slots.findIndex((s) => s === true);
      expect(slot).toBeGreaterThanOrEqual(0);
      delete hooks.slots[slot];
      act(); render();
      hooks.effects.at(-1)!();
    };
    // Hover help: open and NOT held — held is the provider's flag, not a mirror of open.
    rearm(() => controller.show(anchor, "Hover explanation", false, false));
    expect(seen.at(-1)).toEqual([true, false]);
    // The same anchor held: (true, true) — the flag the Limits touch-stop reads.
    rearm(() => controller.show(anchor, "Held explanation", false, true));
    expect(seen.at(-1)).toEqual([true, true]);
    // Closed: (false, false) — never (false, true).
    rearm(() => controller.dismiss());
    expect(seen.at(-1)).toEqual([false, false]);
    expect(seen).toEqual([[true, false], [true, true], [false, false]]);
  });
});

describe("R12 RA-F2/B-F2: an unmounting control forgets its own anchor", () => {
  function mountTwo(textA: string, textB: string) {
    let tree: ReactNode;
    const render = () => { hooks.cursor = 0; tree = ControlHelpProvider({ children: "screen", colors }); return tree; };
    render();
    hooks.controller = (tree as Element).props.value;
    // The provider root must be measured or place() drops every help: give it a 390x320 window.
    const root = all(tree).find((e) => e.type === "View")!;
    (root.props.ref as { current: unknown }).current = measure(0, 0, 390, 320);
    // Per-control effect isolation: the subscribe effect each hook registers is the newly pushed
    // tail; splicing keeps B's registration separate from A's. The provider's own effects (the
    // document listeners and root measurement) run first, as mount() does.
    const before = hooks.effects.length;
    const controlA = useControlHelp(textA);
    const aEffects = hooks.effects.splice(before);
    const controlB = useControlHelp(textB);
    const bEffects = hooks.effects.splice(before);
    cleanup.push(...hooks.effects.map((fn) => fn()).filter((fn): fn is () => void => typeof fn === "function"));
    controlA.bindings.ref(measure(20, 30, 100, 30) as unknown as View);
    controlB.bindings.ref(measure(200, 30, 100, 30) as unknown as View);
    const run = (effects: (() => void | (() => void))[]) => effects.map((fn) => fn()).filter((fn): fn is () => void => typeof fn === "function");
    const aCleanup = run(aEffects);
    const bCleanup = run(bEffects);
    cleanup.push(...aCleanup, ...bCleanup);
    return { controlA, controlB, aCleanup, bCleanup, render, tooltip: () => all(tree).find((e) => e.props.role === "tooltip") };
  }

  it("hold, then unmount without any pointer press: 0 panels; a fresh control starts collapsed", () => {
    const s = mountTwo("Effects A with limits.", "Effects B with limits.");
    s.controlA.hold(); s.render();
    expect(s.tooltip()).toBeDefined();
    // The bar unmounts (selection ends via keyboard Done/Take over — no pointer press anywhere).
    for (const fn of s.aCleanup) fn();
    s.render();
    expect(s.tooltip()).toBeUndefined();
    // Selecting again: the fresh control reports collapsed — the old help was forgotten, not kept.
    const fresh = useControlHelp("Effects A with limits.");
    fresh.bindings.ref(measure(20, 30, 100, 30) as unknown as View);
    s.render();
    expect(s.tooltip()).toBeUndefined();
  });

  it("another control's open help survives the unmount of an anchor that does not own it", () => {
    const s = mountTwo("Effects A with limits.", "Effects B with limits.");
    s.controlA.hold(); s.render();
    expect(s.tooltip()).toBeDefined();
    // A DIFFERENT control unmounts: forget() dismisses only the help that belongs to that anchor.
    for (const fn of s.bCleanup) fn();
    s.render();
    expect(s.tooltip()).toBeDefined();
    s.controlA.dismiss(); s.render();
    expect(s.tooltip()).toBeUndefined();
  });
});
