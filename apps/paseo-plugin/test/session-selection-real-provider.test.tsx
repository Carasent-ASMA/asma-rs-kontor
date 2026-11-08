import { isValidElement, type ReactElement } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { View } from "react-native";

// The REAL ControlHelpProvider drives a REAL SelectionBar: the provider element's controller value is
// handed to useContext, so the Limits control's heldOpen mirror is the provider's own subscription
// notification — the exact input of its touch-start stop decision (R38-D1-r2). The deferred react
// here memoizes by deps, so the controller keeps ONE identity and the provider's document-listener
// effect registers once (its cleanup dismisses, so re-registering per render would dismiss every
// help immediately); the notify effect still re-registers when help changes, which is the mirror's
// live update path.
const harness = await vi.hoisted(() => {
  const slots: Array<Record<string, unknown>> = [];
  const effects: Array<() => void> = [];
  let cursor = 0;
  const slot = () => slots[cursor] ?? (slots[cursor] = {});
  const same = (a: unknown[] | undefined, b: unknown[] | undefined) => !!a && !!b && a.length === b.length && a.every((d, i) => Object.is(d, b[i]));
  const react = {
    useState: (initial: unknown) => { const s = slot(); cursor++; s.queue ??= []; if (!("value" in s)) s.value = typeof initial === "function" ? (initial as () => unknown)() : initial;
      for (const update of (s.queue as Array<(value: unknown) => unknown>).splice(0)) s.value = update(s.value);
      return [s.value, (next: unknown) => (s.queue as unknown[]).push(typeof next === "function" ? next : () => next)]; },
    useRef: (initial: unknown) => { const s = slot(); cursor++; return s.value ?? (s.value = { current: initial }); },
    useMemo: (fn: () => unknown, deps: unknown[]) => { const s = slot(); cursor++; if (same(s.deps as unknown[], deps)) return s.value; s.deps = deps; s.value = fn(); return s.value; },
    useCallback: (fn: unknown, deps: unknown[]) => { const s = slot(); cursor++; if (same(s.deps as unknown[], deps)) return s.value; s.deps = deps; s.value = fn; return s.value; },
    useEffect: (fn: () => (() => void) | void, deps: unknown[]) => { const s = slot(); cursor++; if (same(s.deps as unknown[], deps)) return; s.deps = deps;
      effects.push(() => { (s.cleanup as (() => void) | void)?.(); s.cleanup = fn(); }); },
  };
  const box = { current: { react, render: (draw: () => unknown) => { cursor = 0; const value = draw(); for (const run of effects.splice(0)) run(); return value; } }, controller: null as unknown,
    reset: () => { slots.length = 0; effects.length = 0; cursor = 0; } };
  return box;
});
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name as keyof typeof harness.current.react] as (...values: unknown[]) => unknown)(...args)])),
  useContext: () => harness.controller, useId: () => "help" }));
vi.mock("react-native", () => ({ Easing: { linear: (t: number) => t }, Platform: { OS: "web" }, useWindowDimensions: () => ({ width: 390, height: 844, fontScale: 1 }), Pressable: "Pressable", Text: "Text", View: "View", Image: "Image", ScrollView: "ScrollView", BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
const panel = vi.hoisted(() => ({ open: false }));
vi.mock("../client/anchored-menu", async (original) => ({ ...await original<typeof import("../client/anchored-menu")>(), anchoredPanelOpen: () => panel.open }));

import { ControlHelpProvider } from "../client/control-help";
import { SelectionBar, type SelectionControl } from "../client/session-selection";

type Element = ReactElement<Record<string, unknown>>;
function all(node: unknown): Element[] {
  if (Array.isArray(node)) return node.flatMap(all);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...all(node.props.children)];
}
const byTestId = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id)!;
const measure = (x: number, y: number, width: number, height: number) => ({ measureInWindow: (callback: (...values: number[]) => void) => callback(x, y, width, height) });
const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const control = (): SelectionControl & { onPress: () => void } => ({ unavailable: null, onPress: vi.fn() });
let handlers: Map<string, (event: never) => void>;
beforeEach(() => {
  vi.useFakeTimers(); harness.reset(); harness.controller = null; panel.open = false;
  handlers = new Map();
  const target = { addEventListener: (type: string, fn: (event: never) => void) => handlers.set(type, fn), removeEventListener: (type: string) => handlers.delete(type) };
  vi.stubGlobal("document", target); vi.stubGlobal("window", target);
});
afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); });

/** One render pass draws the provider first (its controller feeds useContext), then the bar. */
function mount(disclosureText: string) {
  const props = { count: 1, takeover: control(), auto: control(), archive: control(), colors: COLORS, disclosure: disclosureText, hasLimits: true };
  let providerEl!: Element; let barTree: Element[] = [];
  const renderAll = () => {
    providerEl = ControlHelpProvider({ children: "screen", colors: {} as Parameters<typeof ControlHelpProvider>[0]["colors"] }) as Element;
    harness.controller = providerEl.props.value;
    barTree = all(SelectionBar(props));
  };
  const render = () => { harness.current.render(renderAll); };
  render();
  const root = all(providerEl).find((node) => node.type === "View")!;
  (root.props.ref as { current: unknown }).current = measure(0, 0, 390, 320);
  // bindings.ref is the hook's own commit function — call it, as React would commit the ref.
  (byTestId(barTree, "selection-limits")!.props.ref as (node: unknown) => void)(measure(20, 30, 100, 30) as unknown as View);
  render();
  // Handles are ACCESSORS: every render draws a fresh tree, so a captured element would go stale.
  // The root touch surface is the PROVIDER's own root View (control-help.tsx) — its onTouchStart is
  // the outside-touch dismissal the Limits stop decision defends against.
  return {
    render,
    limits: () => byTestId(barTree, "selection-limits")!,
    surface: () => all(providerEl).find((node) => node.type === "View")!,
    tooltip: () => all(providerEl).find((node) => node.props.role === "tooltip"),
  };
}

describe("R12 T1: the real provider's held flag drives the Limits stop decision (not a mirror of open)", () => {
  it("an unheld focus help does not stop the touch: the root dismissal runs and one press opens held", () => {
    const s = mount("Host-stated limits beyond the fixed effects sentence.");
    // Keyboard-focus help on the Limits control: open and NOT held.
    (s.limits().props.onFocus as () => void)();
    s.render(); s.render(); s.render();
    expect(s.tooltip()).toBeDefined();
    expect(s.limits().props["aria-expanded"]).toBe(true);
    // The stop decision reads the live mirror: unheld -> the touch is NOT stopped.
    const stop = vi.fn();
    (s.limits().props.onTouchStart as (event: unknown) => void)({ stopPropagation: stop, target: s.limits() });
    expect(stop.mock.calls.length).toBe(0);
    // So the provider root's dismissal runs and the unheld focus help closes…
    (s.surface().props.onTouchStart as (event: unknown) => void)({ target: s.limits() });
    s.render(); s.render(); s.render();
    expect(s.tooltip()).toBeUndefined();
    // …and the press then opens the HELD disclosure from the live closed state.
    (s.limits().props.onPress as () => void)();
    s.render(); s.render(); s.render();
    expect(s.tooltip()).toBeDefined();
    expect(s.limits().props["aria-expanded"]).toBe(true);
    // Held now: the next touch-start on the control stops (the root dismissal never runs).
    const stopHeld = vi.fn();
    (s.limits().props.onTouchStart as (event: unknown) => void)({ stopPropagation: stopHeld, target: s.limits() });
    expect(stopHeld.mock.calls.length).toBe(1);
  });
});
