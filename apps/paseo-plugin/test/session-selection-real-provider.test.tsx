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
function mount(disclosureText: string | null, hasLimits = true, takeover: ReturnType<typeof control> = control()) {
  const props: { count: number; takeover: ReturnType<typeof control>; auto: ReturnType<typeof control>; archive: ReturnType<typeof control>; colors: typeof COLORS; disclosure: string | null; hasLimits: boolean } =
    { count: 1, takeover, auto: control(), archive: control(), colors: COLORS, disclosure: disclosureText, hasLimits };
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
    setProps: (patch: Partial<typeof props>) => { Object.assign(props, patch); },
    limits: () => byTestId(barTree, "selection-limits"),
    surface: () => all(providerEl).find((node) => node.type === "View")!,
    tooltip: () => all(providerEl).find((node) => node.props.role === "tooltip"),
  };
}

describe("R14 F1: the held help ends when the Limits control leaves while the bar stays mounted", () => {
  const DISCLOSURE = "Host-stated limits beyond the fixed effects sentence.";
  const boundaries: Array<[string, Parameters<typeof mount>[1] | null, Partial<{ disclosure: string | null; hasLimits: boolean; takeover: ReturnType<typeof control> }>]> = [
    ["the host stops stating limits", null, { hasLimits: false }],
    ["the disclosure drops (effects null)", null, { disclosure: null }],
    ["Take over is refused", null, { takeover: { unavailable: "The host is offline.", onPress: vi.fn() } as never }],
  ];
  it.each(boundaries)("hold, then %s while the bar stays mounted: 0 panels (R14 F1/AUD-R13-F1)", (_label, _none, patch) => {
    // AUD-R13-F1: useControlHelp belongs to SelectionBar, so removing only the Limits Pressable
    // never runs the hook's unmount cleanup — the provider kept the held help for a dead anchor
    // (1 bar, 0 controls, 1 panel). The presence-forget effect must end it on every path.
    const s = mount(DISCLOSURE);
    (s.limits()!.props.onPress as () => void)();
    s.render(); s.render(); s.render();
    expect(s.tooltip()).toBeDefined();
    expect(s.limits()).toBeDefined();
    // The control leaves; the bar stays. The held help must go with it.
    s.setProps(patch);
    s.render(); s.render(); s.render();
    expect(s.limits()).toBeUndefined();
    expect(s.tooltip()).toBeUndefined();
    // Restoring the limits state must not resurrect the dismissed help.
    s.setProps({ disclosure: DISCLOSURE, hasLimits: true, takeover: control() });
    s.render(); s.render();
    expect(s.tooltip()).toBeUndefined();
  });
});

describe("R12 T1: the real provider's held flag drives the Limits stop decision (not a mirror of open)", () => {
  it("an unheld focus help does not stop the touch: the root dismissal runs and one press opens held", () => {
    const s = mount("Host-stated limits beyond the fixed effects sentence.");
    // Keyboard-focus help on the Limits control: open and NOT held.
    (s.limits()!.props.onFocus as () => void)();
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

// R15 gates for the review-B survivors (N1/N2), on the committed real-provider harness. Both probe
// the SelectionBar PRESENCE effect — the control leaves while the bar stays mounted — which the
// round-12 unmount-cleanup gate never exercises: that gate runs the provider's cleanup path with
// two controls and opens help through the control's own hold, so forget() and dismiss() are
// indistinguishable in it (only the Limits help is open, and both close exactly that help).
const RB_DISCLOSURE = "Host-stated limits beyond the fixed effects sentence.";
const rbFlip: Array<[string, Partial<{ hasLimits: boolean; disclosure: string | null; takeover: ReturnType<typeof control> }>]> = [
  ["hasLimits false", { hasLimits: false }],
  ["disclosure null", { disclosure: null }],
  ["takeover refused", { takeover: { unavailable: "The host is offline.", onPress: vi.fn() } as never }],
];

describe("R15 N1: UNHELD help on Limits ends when the control leaves", () => {
  // M-rb-r14-presence-held-only gates the forget on heldOpen: hover- or focus-opened help is not
  // held, so under the mutant it outlives its control. Each presence path must end it.
  for (const kind of ["onHoverIn", "onFocus"] as const)
    it.each(rbFlip)(`${kind} help closes when Limits leaves: %s`, (_label, patch) => {
      const s = mount(RB_DISCLOSURE);
      (s.limits()!.props[kind] as () => void)();
      s.render(); s.render(); s.render();
      expect(s.tooltip()).toBeDefined();
      s.setProps(patch as never);
      s.render(); s.render(); s.render();
      expect(s.limits()).toBeUndefined();
      expect(s.tooltip()).toBeUndefined();
    });
});

describe("R15 N2: another owner's help survives the presence drop", () => {
  // M-rb-r14-presence-dismiss-any swaps forget() for the controller-wide dismiss(): whatever help
  // is open would close with the Limits anchor. forget() is anchor-scoped, so another owner's help
  // survives every presence path — and the bar mounting on a host whose Limits then leave.
  it.each(rbFlip)("stays open when Limits leaves: %s", (_label, patch) => {
    const s = mount(RB_DISCLOSURE);
    const other = { current: measure(200, 200, 40, 40) };
    (harness.controller as { show: (a: unknown, t: string, f: boolean, h: boolean) => void }).show(other, "Help for another control", false, true);
    s.render(); s.render(); s.render();
    expect(s.tooltip()).toBeDefined();
    s.setProps(patch as never);
    s.render(); s.render(); s.render();
    expect(s.limits()).toBeUndefined();
    expect(s.tooltip()).toBeDefined();
  });
});

describe("R16: another owner's help survives the bar MOUNTING with no Limits", () => {
  // Review-B survivor probe (NV-n2-dismiss-at-mount), the mount half of N2 that the r15 gate could
  // not reach: the r15 N2 gate mounts the bar WITH Limits present and then flips a presence path,
  // so the effect's mount run always happened under hasLimits=true. The variant adds a mount-only
  // effect (`useEffect(() => { if (!effects || !hasLimits) limits.dismiss(); }, []);` after the
  // presence effect) that fires exactly when the bar mounts with no Limits — dismiss() closes the
  // whole controller, so another owner's already-open help must not go with it. r16c: run as an
  // it.each over ALL THREE presence combinations (review-B finding N2-r16 — the single set covered
  // one of three, so a mount effect limited to either other combination escaped): effects+no-Limits
  // and no-effects+Limits alongside the original no-effects+no-Limits. r16d (N2-r16c): the rows
  // also carry the count and the Take over availability, adding count-zero and
  // takeover-unavailable — a mount effect conditioned on `!count`, `none` or
  // `takeover.unavailable` (N-m-no-count, N-m-none, N-m-takeover-unavailable) escapes the
  // presence-only rows. r16f (SF4-r16d, LSA-CONFIRMED: the
  // it.each table is the bounded mount alphabet): four more rows — nocount-nolimits,
  // unavailable-nolimits, nocount-unavailable and nocount-unavailable-nolimits — covering the four
  // reachable cells of the effects × hasLimits × count × Take over matrix that had no case.
  // Cells (N1, msg 1b286dab): count classes {0, 1, >=2} on the product's plural; every class->=2
  // cell samples 2, and exactly ONE cell also samples 3 (count-three-nolimits). The sixteen rows
  // cover FIFTEEN reachable cells, and the "clamps the takeover disclosure to two lines with a
  // Limits control..." test (T1) covers the sixteenth — the cells: at count 0 — nocount-nolimits,
  // nocount-unavailable, nocount-unavailable-nolimits and count-zero; at count 1 — no effects, no
  // Limits; effects, no Limits; no effects, Limits; effects, Limits (count-one-limits); effects,
  // Limits with Take over refused (takeover-unavailable; client:100 sets effects to null whenever
  // Take over is unavailable, so the refused cells are no-effects cells — N2-r16i labels them by
  // disclosure, and the row's disclosure text is inert there); effects, no Limits with Take over
  // refused (unavailable-nolimits); at count 2 — effects, no Limits (count-two-nolimits); no
  // effects, no Limits; no effects, Limits; Limits refused (count-two-unavailable); and
  // disclosure-effects refused (count-two-unavailable-nolimits; likewise a no-effects cell).
  // Count two with effects AND Limits has no row: the clamps test covers that cell. Kills (r16j results.json at c98e6a6d): the mount gate is the SOLE killer of
  // NV-n2-dismiss-at-mount (the variant fails fifteen of the sixteen cases — every one except
  // count-one-limits, where neither !effects nor !hasLimits holds) and the UNIQUE killer of
  // V-n2-mount-effects-no-limits (its own case plus count-two-nolimits and count-three-nolimits),
  // F-m-count-gt1-nolimits (the four count->=2 no-Limits rows), R-cell-elcu, R-cell-eLcU and
  // R-cell-elcU (each fails exactly its own case), R-cell-elCU (unavailable-nolimits and
  // count-two-unavailable-nolimits), and each H-m-c1-EL/H-m-c2-elu/H-m-c2-eLu/H-m-c2-lU/H-m-c2-LU
  // row seed fails exactly its own row, H-m-c2-noeffects fails the four count-2 rows and
  // H-m-c2-unavailable fails count-two-unavailable and count-two-unavailable-nolimits;
  // N-m-no-count and N-m-none die on count-zero plus the three nocount rows; N-m-takeover-unavailable
  // fails its own case plus every refused-Take-over row (unavailable-nolimits, nocount-unavailable,
  // nocount-unavailable-nolimits, count-two-unavailable, count-two-unavailable-nolimits) — not
  // nocount-nolimits; V-n2-mount-no-effects-with-limits fails SIX cases (its own, count-zero,
  // takeover-unavailable, nocount-unavailable, count-two-noeffects-limits and count-two-unavailable);
  // R-m-nocount-nolimits and R-m-nocount-unavailable each fail their own case plus
  // nocount-unavailable-nolimits, R-m-unavailable-nolimits fails its own case plus
  // nocount-unavailable-nolimits and count-two-unavailable-nolimits; and M-rb-r14-presence-dismiss-any
  // fails fifteen of the sixteen mount cases (every one except count-one-limits) and "stays open
  // when Limits leaves".
  it.each([
    ["no effects, no Limits", 1, null, false, () => control()],
    ["effects, no Limits", 1, "effects text", false, () => control()],
    ["no effects, Limits", 1, null, true, () => control()],
    ["count-zero", 0, "effects text", true, () => control()],
    ["takeover-unavailable", 1, "effects text", true, () => ({ unavailable: "Busy", onPress: vi.fn() })],
    ["count-two-nolimits", 2, "effects text", false, () => control()],
    ["nocount-nolimits", 0, "effects text", false, () => control()],
    ["unavailable-nolimits", 1, "effects text", false, () => ({ unavailable: "Busy", onPress: vi.fn() })],
    ["nocount-unavailable", 0, "effects text", true, () => ({ unavailable: "Busy", onPress: vi.fn() })],
    ["nocount-unavailable-nolimits", 0, "effects text", false, () => ({ unavailable: "Busy", onPress: vi.fn() })],
    ["count-one-limits", 1, "effects text", true, () => control()],
    ["count-two-noeffects-nolimits", 2, null, false, () => control()],
    ["count-two-noeffects-limits", 2, null, true, () => control()],
    ["count-two-unavailable", 2, "effects text", true, () => ({ unavailable: "Busy", onPress: vi.fn() })],
    ["count-two-unavailable-nolimits", 2, "effects text", false, () => ({ unavailable: "Busy", onPress: vi.fn() })],
    ["count-three-nolimits", 3, "effects text", false, () => control()],
  ] as Array<[string, number, string | null, boolean, () => SelectionControl & { onPress: () => void }]>)("stays open when the bar mounts: %s", (_name, count, disclosure, hasLimits, takeover) => {
    let providerEl!: Element;
    const provider = () => { providerEl = ControlHelpProvider({ children: "screen", colors: {} as Parameters<typeof ControlHelpProvider>[0]["colors"] }) as Element; harness.controller = providerEl.props.value; };
    harness.current.render(provider);
    (all(providerEl).find((node) => node.type === "View")!.props.ref as { current: unknown }).current = measure(0, 0, 390, 320);
    (harness.controller as { show: (a: unknown, t: string, f: boolean, h: boolean) => void }).show({ current: measure(200, 200, 40, 40) }, "Help for another control", false, true);
    harness.current.render(provider); harness.current.render(provider); harness.current.render(provider);
    expect(all(providerEl).find((node) => node.props.role === "tooltip")).toBeDefined();
    const props = { count, takeover: takeover(), auto: control(), archive: control(), colors: COLORS, disclosure, hasLimits };
    const renderAll = () => { provider(); all(SelectionBar(props)); };
    harness.current.render(renderAll); harness.current.render(renderAll); harness.current.render(renderAll);
    expect(all(providerEl).find((node) => node.props.role === "tooltip")).toBeDefined();
  });
});
