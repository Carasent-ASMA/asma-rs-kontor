import type { PluginTheme } from "@getpaseo/plugin";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Slot = { initialized?: boolean; value?: unknown; deps?: readonly unknown[]; cleanup?: () => void };
type Queued = { readonly slot: Slot; readonly fn: () => void | (() => void); readonly deps?: readonly unknown[] };
const { harness, fake } = vi.hoisted(() => {
  const slots: Slot[] = [];
  let cursor = 0, dirty = false, component: (() => unknown) | null = null, tree: unknown = null, effects: Queued[] = [];
  const same = (a?: readonly unknown[], b?: readonly unknown[]) => !!a && !!b && a.length === b.length && a.every((value, index) => Object.is(value, b[index]));
  const next = () => slots[cursor] ?? (slots[cursor] = {});
  const hooks = {
    useState(initial: unknown) {
      const slot = next(); cursor++;
      if (!slot.initialized) { slot.value = typeof initial === "function" ? (initial as () => unknown)() : initial; slot.initialized = true; }
      return [slot.value, (value: unknown) => { const resolved = typeof value === "function" ? (value as (previous: unknown) => unknown)(slot.value) : value; if (!Object.is(resolved, slot.value)) { slot.value = resolved; dirty = true; } }];
    },
    useRef(initial: unknown) { const slot = next(); cursor++; if (!slot.initialized) { slot.value = { current: initial }; slot.initialized = true; } return slot.value; },
    useMemo(factory: () => unknown, deps?: readonly unknown[]) { const slot = next(); cursor++; if (!slot.initialized || !same(slot.deps, deps)) { slot.value = factory(); slot.deps = deps; slot.initialized = true; } return slot.value; },
    useCallback<T>(fn: T) { next(); cursor++; return fn; },
    useEffect(fn: () => void | (() => void), deps?: readonly unknown[]) { effects.push({ slot: next(), fn, deps }); cursor++; },
  };
  function draw() {
    let guard = 0;
    do {
      dirty = false; cursor = 0; effects = []; tree = component!();
      for (const item of effects) { if (item.deps && same(item.slot.deps, item.deps)) continue; item.slot.cleanup?.(); const cleanup = item.fn(); item.slot.deps = item.deps; item.slot.cleanup = typeof cleanup === "function" ? cleanup : undefined; }
    } while (dirty && ++guard < 20);
    return tree;
  }
  return {
    harness: { ...hooks, render(fn: () => unknown) { component = fn; return draw(); }, flush: draw,
      unmount() { for (const slot of slots) slot.cleanup?.(); slots.length = 0; component = null; tree = null; } },
    fake: { rpc: {} as Record<string, (input: Record<string, unknown>) => Promise<unknown>>, calls: [] as Array<[string, Record<string, unknown>]>,
      toast: { show: vi.fn(), error: vi.fn() }, refresh: vi.fn(), state: null as unknown },
  };
});
vi.mock("react", async (importOriginal) => ({ ...(await importOriginal<typeof import("react")>()), useContext: () => null, memo: (component: unknown) => component,
  useCallback: harness.useCallback, useEffect: harness.useEffect, useLayoutEffect: harness.useEffect, useMemo: harness.useMemo, useRef: harness.useRef, useState: harness.useState }));
vi.mock("react-native", () => ({
  AccessibilityInfo: { addEventListener: () => ({ remove: () => undefined }), isReduceMotionEnabled: async () => false, isScreenReaderEnabled: async () => false },
  ActivityIndicator: "ActivityIndicator", Animated: { Value: class { setValue() {} stopAnimation() {} }, View: "Animated.View", timing: () => ({ start: (done?: () => void) => done?.(), stop: () => undefined }) },
  BackHandler: { addEventListener: () => ({ remove: () => undefined }) },
  PanResponder: { create: () => ({ panHandlers: {} }) }, Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View", useWindowDimensions: () => ({ fontScale: 1 }),
}));
vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: (contract: { name: string }) => (input: Record<string, unknown>) => { fake.calls.push([contract.name, input]); const handler = fake.rpc[contract.name]; return handler ? handler(input) : Promise.reject(new Error("Unavailable in this fixture")); },
  getPaseoClient: () => { throw new Error("No host client in this fixture"); },
  useHosts: () => [{ serverId: "host-a", status: "online" }],
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ FlatList: "FlatList", Icon: "Icon", ScrollView: "ScrollView", TextInput: "TextInput", useToast: () => fake.toast }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: fake.state, refresh: fake.refresh }) }));

import { SessionsScreen, SlimToolbar } from "../client/sessions-screen";
import { NoticesButton, NoticeTray, presentHidden, type Notice } from "../client/notice-tray";
import { MobileQuickToolbar } from "../client/mobile-quick-toolbar";
import { MobileFiltersPage } from "../client/mobile-filters";
import { DEFAULT_RECOVERY_TARGET, TAKEOVER_LIMIT, type LifecycleRow, type UnresolvedRef } from "../shared/session-lifecycle";
import { sessionEntry } from "./session-fixtures";
import { clearSessionViews, patchSessionView, readSessionView } from "../client/session-view-store";

type Element = { readonly type?: unknown; readonly props: Record<string, unknown> };
const walk = (node: unknown): Element[] => {
  if (node === null || node === undefined || typeof node !== "object") return [];
  if (Array.isArray(node)) return node.flatMap(walk);
  const element = node as Element;
  return [element, ...walk(element.props.children), ...walk(element.props.ListHeaderComponent)];
};
const all = () => walk(harness.flush());
const settle = async () => { for (let n = 0; n < 10; n++) await new Promise((resolve) => setTimeout(resolve, 0)); harness.flush(); };
const colors = { accent: "#05f", accentForeground: "#fff", border: "#ccc", foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#f4f4f4", surface2: "#eee", statusDanger: "#c00", statusSuccess: "#080", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
const SA = "fixture-sa", SX = "fixture-sx";
const reviewId = (id: string) => `00000000-0000-4000-8000-${Buffer.from(id).toString("hex").slice(-12).padStart(12, "0")}`;
const ref = (id: string): UnresolvedRef => ({ reviewId: reviewId(`archive:${id}`.slice(-6)), agentId: id });
const latest = (unresolved: UnresolvedRef[]) => ({ id: reviewId("latest"), createdAt: "2026-10-22", action: "resume", scope: "filtered", target: DEFAULT_RECOVERY_TARGET, rows: [], limitation: TAKEOVER_LIMIT, unresolved });
const closed = (id: string) => ({ id, title: `Session ${id}`, workspaceId: null, cwd: "/fixture", provider: "codex", model: null, effort: null, mode: null, nativeStatus: "closed", fingerprint: `fp-${id}`,
  category: "candidate", eligible: true, reason: "Fixture.", children: [], outcome: "archived", runtime: "closed", message: "Archived. An exact-ID read shows its runtime closed. No additional archive request was sent." }) as LifecycleRow;

type Layout = "full" | "slim" | "compact-320" | "compact-390";
function mount(layout: Layout, options: { navigation?: boolean; host?: string } = {}) {
  const compact = layout.startsWith("compact");
  const tree = harness.render(() => SessionsScreen({ host: { id: options.host ?? "host-a", label: "Fixture host" }, layout: { compact, platform: compact ? "ios" : "web" },
    navigation: options.navigation === false ? undefined : { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {} } as Parameters<typeof SessionsScreen>[0])) as Element;
  (tree.props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { width: compact ? Number(layout.slice(-3)) : 1280, height: 800 } } });
  const list = all().find((element) => element.props.testID === "sessions-list")!;
  (list.props.ref as { current: unknown }).current = { scrollToOffset: () => undefined, scrollToIndex: () => undefined };
  for (const [testID, height] of [["sessions-full-toolbar", 220], ["sessions-slim-toolbar-slot", 44]] as const) {
    const slot = all().find((element) => element.props.testID === testID); (slot?.props.onLayout as ((event: unknown) => void) | undefined)?.({ nativeEvent: { layout: { x: 0, y: 0, width: 1280, height } } });
  }
  if (layout === "slim") {
    (all().find((element) => element.props.testID === "sessions-list")!.props.onScroll as (event: unknown) => void)({ nativeEvent: { contentOffset: { x: 0, y: 300 }, contentSize: { width: 1280, height: 2000 }, layoutMeasurement: { width: 1280, height: 600 } } });
    harness.flush();
  }
}
const trays = () => all().filter((element) => element.type === NoticeTray);
const tray = () => { const found = trays(); expect(found.length).toBeLessThanOrEqual(1); return found[0]; };
const triggers = () => [...all().filter((element) => element.type === NoticesButton), ...all().flatMap((element) => element.type === SlimToolbar && (element.props.notice as Element | null)?.type === NoticesButton ? [element.props.notice as Element] : [])];
const trigger = () => { const found = triggers(); expect(found.length).toBeLessThanOrEqual(1); return found[0]; };
const summaries = (button: Element | undefined) => (button?.props.notices as Notice[] | undefined)?.map((notice) => notice.summary);
const recovery = () => walk(tray()?.props.children).find((element) => element.props.testID === "sessions-archive-recovery");
const lineText = () => walk(recovery()?.props.children).filter((element) => element.type === "Text").map((element) => element.props.children).join("") || null;
const action = (label: string) => walk(recovery()?.props.children).find((element) => element.props.accessibilityLabel === label);
const count = (name: string) => fake.calls.filter(([called]) => called === name).length;
const press = (element: Element | undefined) => (element!.props.onPress as () => void)();
const button = (props: Record<string, unknown>) => (NoticesButton as unknown as (props: Record<string, unknown>) => Element)(props);
const isResume = (element: Element) => element.props.accessibilityLabel === "Resume sessions";
/**
 * Wide: the status button sits beside Resume in the same row (the slim row passes both to SlimToolbar), so it adds no row.
 * Compact: it sits in the count row before Grouped / Flat, and the Resume column holds only Resume.
 */
function placed(layout: Layout) {
  const tree = all();
  if (layout.startsWith("compact")) {
    const bar = tree.find((element) => element.props.testID === "sessions-layout-bar");
    const column = tree.find((element) => ([element.props.children].flat(2) as Element[]).some((kid) => !!kid && typeof kid === "object" && isResume(kid)));
    return !!bar && walk(bar).some((element) => element.type === NoticesButton) && !walk(column).some((element) => element.type === NoticesButton);
  }
  const slim = tree.find((element) => element.type === SlimToolbar && (element.props.notice as Element | null)?.type === NoticesButton);
  if (slim) return isResume(slim.props.action as Element);
  return tree.some((element) => { const kids = ([element.props.children].flat(2) as Element[]).filter((kid) => !!kid && typeof kid === "object"); return kids.some((kid) => kid.type === NoticesButton) && kids.some(isResume); });
}

describe.each(["full", "slim", "compact-320", "compact-390"] as const)("Notice tray on the %s layout", (layout) => {
  let unresolved: UnresolvedRef[];
  beforeEach(() => {
    clearSessionViews(); patchSessionView("host-a", { layout: "flat" });
    unresolved = [ref(SA)]; fake.calls = []; fake.refresh = vi.fn(() => new Promise(() => {}));
    fake.toast.show.mockReset(); fake.toast.error.mockReset();
    fake.state = { entries: [sessionEntry("fixture-row", { status: "idle" })], status: "ready", live: true, error: null, staleReason: null, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, includeArchived: false };
    fake.rpc = {
      "sessions.lifecycle-review": async () => latest([...unresolved]),
      "sessions.lifecycle-reconcile-item": async (input) => { unresolved = []; return closed(input.agentId as string); },
      "sessions.lifecycle-dispositions": async () => ({}), "sessions.availability": async () => ({}),
    };
  });
  afterEach(() => harness.unmount());

  it("hides the unconfirmed archive behind one status button (beside Resume, or in the compact count row), with no action and no false success", async () => {
    mount(layout); await settle();
    expect(lineText()).toBe("1 archive not confirmed"); expect(trigger()).toBeUndefined();
    const before = fake.calls.length;
    (tray()!.props.onHide as () => void)(); await settle();
    // Hidden: no tray, no recovery line holding top space, one status button that still says what is unresolved.
    expect(trays()).toHaveLength(0); expect(recovery()).toBeUndefined();
    expect(summaries(trigger())).toEqual(["1 archive not confirmed"]); expect(placed(layout)).toBe(true);
    const rendered = button(trigger()!.props);
    expect(rendered.props).toMatchObject({ accessibilityRole: "button", accessibilityLabel: "Show notices: 1 archive not confirmed", "aria-expanded": false, accessibilityState: { expanded: false } });
    expect(walk(rendered).find(element => element.type === "Icon")!.props.color).toBe(colors.statusWarning);
    // Presentation only: no read, check, archive or toast, so nothing is reported closed.
    expect(fake.calls.length).toBe(before); expect(fake.toast.show).not.toHaveBeenCalled(); expect(fake.toast.error).not.toHaveBeenCalled();
    expect(count("sessions.lifecycle-reconcile-item")).toBe(0); expect(count("sessions.lifecycle-execute-item")).toBe(0);
  });

  it("stays hidden across a re-render and a remount, then shows Check again and Details working", async () => {
    mount(layout); await settle();
    (tray()!.props.onHide as () => void)(); await settle();
    harness.flush(); expect(trays()).toHaveLength(0);
    harness.unmount(); mount(layout); await settle();
    expect(trays()).toHaveLength(0); expect(summaries(trigger())).toEqual(["1 archive not confirmed"]);
    press(trigger()); await settle();
    expect(trigger()).toBeUndefined(); expect(lineText()).toBe("1 archive not confirmed");
    expect(action("Check unconfirmed archives again")?.props.disabled).toBe(false); expect(action("Archive details")?.props.disabled).toBe(false);
    press(action("Check unconfirmed archives again")); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-reconcile-item").map(([, input]) => input)).toEqual([{ reviewId: ref(SA).reviewId, agentId: SA }]);
    // Resolved by the Check, not by hiding: the line and the button both go.
    expect(trays()).toHaveLength(0); expect(trigger()).toBeUndefined();
  });

  it("opens again for a new unconfirmed archive or a read error, and stays hidden when one is confirmed", async () => {
    unresolved = [ref(SA), ref(SX)];
    mount(layout); await settle();
    (tray()!.props.onHide as () => void)(); await settle();
    harness.unmount(); unresolved = [ref(SA)]; mount(layout); await settle();
    expect(trays()).toHaveLength(0); expect(summaries(trigger())).toEqual(["1 archive not confirmed"]);
    harness.unmount(); unresolved = [ref(SA), { reviewId: reviewId("archive:new"), agentId: "fixture-new" }]; mount(layout); await settle();
    expect(trigger()).toBeUndefined(); expect(lineText()).toBe("2 archives not confirmed");
    (tray()!.props.onHide as () => void)(); await settle();
    harness.unmount(); fake.rpc["sessions.lifecycle-review"] = async () => { throw new Error("Fixture read failure."); }; mount(layout); await settle();
    expect(trigger()).toBeUndefined(); expect(lineText()).toBe("Archive checks could not be read: Fixture read failure.");
  });

  it("moves focus from Hide to the status button, and from Show to Hide", async () => {
    mount(layout); await settle();
    (tray()!.props.onHide as () => void)(); harness.flush();
    const button = { focus: vi.fn() }; (trigger()!.props.ref as { current: unknown }).current = button;
    await settle(); expect(button.focus).toHaveBeenCalledTimes(1);
    press(trigger()); harness.flush();
    const hide = { focus: vi.fn() }; (tray()!.props.hideRef as { current: unknown }).current = hide;
    await settle(); expect(hide.focus).toHaveBeenCalledTimes(1);
  });

  it("names the tray, keeps Hide reachable, and gives the status button a touch target", async () => {
    mount(layout); await settle();
    expect(tray()!.props).toMatchObject({ compact: layout.startsWith("compact"), disabled: false });
    (tray()!.props.onHide as () => void)(); await settle();
    const style = (button(trigger()!.props).props.style as (state: object) => { minHeight: number; minWidth: number })({});
    expect(style.minHeight).toBeGreaterThanOrEqual(44); expect(style.minWidth).toBeGreaterThanOrEqual(44);
    if (layout.startsWith("compact")) {
      // 320 and 390 keep the quick toolbar at its width; the button sits in the 80 pt Resume column and shows a count.
      expect(all().find((element) => element.type === MobileQuickToolbar)?.props.width).toBe(Number(layout.slice(-3)) - 112);
      expect(walk(button(trigger()!.props)).find((element) => element.type === "Text")?.props.children).toBe(1);
    }
  });
});

describe("Notice tray with several notices", () => {
  beforeEach(() => {
    clearSessionViews(); patchSessionView("host-a", { layout: "flat" });
    fake.calls = []; fake.refresh = vi.fn(() => new Promise(() => {}));
    fake.rpc = { "sessions.lifecycle-review": async () => latest([ref(SA)]), "sessions.lifecycle-dispositions": async () => ({}), "sessions.availability": async () => ({}) };
  });
  afterEach(() => harness.unmount());
  const state = (status: "stale" | "error") => ({ entries: [sessionEntry("fixture-row")], status, live: false, error: status === "error" ? "Fixture read error." : null, staleReason: status === "stale" ? "Host disconnected." : null,
    pages: 1, lastUpdatedAt: 0, inventoryComplete: true, includeArchived: false });

  it("hides the source, navigation and archive notices with one Hide, and the button keeps the strongest tone", async () => {
    for (const status of ["stale", "error"] as const) {
      harness.unmount(); clearSessionViews(); patchSessionView("host-a", { layout: "flat" }); fake.state = state(status);
      mount("full", { navigation: false }); await settle();
      const ids = walk(tray()!.props.children).map((element) => element.props.testID).filter(Boolean);
      expect(ids).toEqual(expect.arrayContaining(["sessions-source-banner", "sessions-navigation-banner", "sessions-archive-recovery"]));
      (tray()!.props.onHide as () => void)(); await settle();
      expect(all().filter((element) => ["sessions-source-banner", "sessions-navigation-banner", "sessions-archive-recovery"].includes(element.props.testID as string))).toHaveLength(0);
      const read = status === "error" ? "Session read failed" : "Sessions not live";
      expect(summaries(trigger())).toEqual([read, "Session navigation unavailable", "1 archive not confirmed"]);
      const rendered = button(trigger()!.props);
      expect(rendered.props.accessibilityLabel).toBe(`Show notices: ${read}; Session navigation unavailable; 1 archive not confirmed`);
      expect(walk(rendered).find((element) => element.type === "Text")?.props.children).toBe(3);
      expect(walk(rendered).find(element => element.type === "Icon")!.props.color).toBe(status === "error" ? colors.statusDanger : colors.statusWarning);
    }
  });

  it("leaves the button where the tray was on the compact Filters page, and shows the tray there", async () => {
    fake.state = state("stale");
    mount("compact-390"); await settle();
    (tray()!.props.onHide as () => void)(); await settle();
    (all().find((element) => element.type === MobileQuickToolbar)!.props.onFilters as () => void)(); await settle();
    const page = all().find((element) => element.type === MobileFiltersPage)!;
    expect(walk(page.props.children).some((element) => element.type === NoticesButton)).toBe(true); expect(trays()).toHaveLength(0);
    press(trigger()); await settle();
    expect(walk(all().find((element) => element.type === MobileFiltersPage)!.props.children).some((element) => element.type === NoticeTray)).toBe(true);
  });
});

describe("Notice tray occurrences", () => {
  let unresolved: UnresolvedRef[];
  const source = (status: "stale" | "loading" | "ready", staleReason: string | null = status === "stale" ? "Host disconnected." : null) => ({ entries: [sessionEntry("fixture-row")], status,
    live: status === "ready", error: null, staleReason, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, includeArchived: false });
  const archiveKey = (id: string) => `archive:${ref(id).reviewId}:${id}`;
  const hidden = () => readSessionView("host-a").noticesHidden ?? [];
  const hide = async () => { (tray()!.props.onHide as () => void)(); await settle(); expect(trays()).toHaveLength(0); };
  const remount = async () => { harness.unmount(); mount("full"); await settle(); };
  beforeEach(() => {
    clearSessionViews(); patchSessionView("host-a", { layout: "flat" });
    unresolved = [ref(SA)]; fake.calls = []; fake.refresh = vi.fn(() => new Promise(() => {})); fake.state = source("ready");
    fake.rpc = { "sessions.lifecycle-review": async () => latest([...unresolved]), "sessions.lifecycle-dispositions": async () => ({}), "sessions.availability": async () => ({}) };
  });
  afterEach(() => harness.unmount());

  it("stays hidden while a notice is continuously present, through in-flight and failed reads, and writes nothing on a re-render", async () => {
    fake.state = source("stale");
    mount("full"); await settle(); await hide();
    const kept = readSessionView("host-a").noticesHidden; harness.flush(); harness.flush();
    expect(readSessionView("host-a").noticesHidden).toBe(kept);
    // A directory refresh in flight is unread, not absent: the same stale notice after it stays hidden.
    fake.state = source("loading"); harness.flush(); expect(hidden()).toContain("source:stale:Host disconnected.");
    fake.state = source("stale"); harness.flush(); expect(trays()).toHaveLength(0);
    // A remount lists nothing until its first read lands; the archive is not forgotten meanwhile.
    let release: (value: unknown) => void = () => undefined;
    fake.rpc["sessions.lifecycle-review"] = () => new Promise((resolve) => { release = resolve; });
    harness.unmount(); mount("full"); harness.flush();
    expect(hidden()).toContain(archiveKey(SA));
    release(latest([ref(SA)])); await settle();
    expect(trays()).toHaveLength(0); expect(summaries(trigger())).toEqual(["Sessions not live", "1 archive not confirmed"]);
    // A failed read reports no list, so it never makes the archive absent; its own error is a new notice.
    fake.rpc["sessions.lifecycle-review"] = async () => { throw new Error("Fixture read failure."); };
    await remount(); expect(trigger()).toBeUndefined(); await hide();
    fake.rpc["sessions.lifecycle-review"] = async () => latest([...unresolved]);
    await remount();
    expect(trays()).toHaveLength(0); expect(summaries(trigger())).toEqual(["Sessions not live", "1 archive not confirmed"]);
  });

  it("opens again when the same notice returns after a completed reading no longer reported it", async () => {
    mount("full"); await settle(); await hide();
    unresolved = []; await remount();
    expect(trigger()).toBeUndefined(); expect(hidden()).not.toContain(archiveKey(SA));
    unresolved = [ref(SA)]; await remount();
    expect(trigger()).toBeUndefined(); expect(lineText()).toBe("1 archive not confirmed");
    // The source banner: a live read in between makes the same disconnect a new occurrence.
    fake.state = source("stale"); harness.flush(); await hide();
    fake.state = source("ready"); harness.flush(); expect(hidden()).not.toContain("source:stale:Host disconnected.");
    fake.state = source("stale"); harness.flush();
    expect(walk(tray()?.props.children).some((element) => element.props.testID === "sessions-source-banner")).toBe(true);
  });

  it("keeps a hidden archive across a host switch while the screen stays mounted", async () => {
    mount("full"); await settle(); await hide();
    // Host B lists no archive; on the first render back on host A the archive state is still host B's.
    unresolved = []; mount("full", { host: "host-b" }); await settle();
    unresolved = [ref(SA)]; mount("full"); await settle();
    expect(hidden()).toContain(archiveKey(SA)); expect(trays()).toHaveLength(0); expect(summaries(trigger())).toEqual(["1 archive not confirmed"]);
  });

  it("opens again for a new key while the hidden ones stay present", async () => {
    fake.state = source("stale");
    mount("full"); await settle(); await hide();
    unresolved = [ref(SA), ref(SX)]; await remount();
    expect(trigger()).toBeUndefined(); expect(lineText()).toBe("2 archives not confirmed");
    await hide();
    fake.state = source("stale", "Host restarting."); harness.flush();
    expect(trigger()).toBeUndefined(); expect(walk(tray()!.props.children).some((element) => element.props.testID === "sessions-source-banner")).toBe(true);
  });

  it("forgets only absent keys whose source completed a reading, and returns the same list when none is forgotten", () => {
    const notice: Notice = { keys: ["archive:a"], summary: "1 archive not confirmed", tone: "warning", node: null };
    const list = ["archive:a", "archive:b", "source:stale:x"];
    expect(presentHidden([notice], list, () => false)).toBe(list);
    expect(presentHidden([notice], list, (key) => key.startsWith("archive"))).toEqual(["archive:a", "source:stale:x"]);
    const present = ["archive:a"];
    expect(presentHidden([notice], present, () => true)).toBe(present);
  });
});

describe("NoticeTray keyboard and outside presses", () => {
  type Key = { key: string; defaultPrevented?: boolean; preventDefault: () => void; stopPropagation: () => void };
  let listeners: Array<{ listener: (event: Key) => void; capture: boolean }>;
  const page = { activeElement: null as unknown,
    addEventListener: (_type: string, listener: (event: Key) => void, capture: boolean) => { listeners.push({ listener, capture }); },
    removeEventListener: (_type: string, listener: (event: Key) => void) => { listeners = listeners.filter((entry) => entry.listener !== listener); } };
  beforeEach(() => { listeners = []; vi.stubGlobal("document", page); });
  afterEach(() => { harness.unmount(); vi.unstubAllGlobals(); });

  it("Escape hides only with focus inside, before page handlers, and leaves an Escape another layer took", () => {
    const onHide = vi.fn(), inside = {}, outside = {};
    const tree = harness.render(() => NoticeTray({ colors, compact: false, onHide, children: "Fixture notice" })) as Element;
    (tree.props.ref as { current: unknown }).current = { contains: (node: unknown) => node === inside };
    expect(listeners.map((entry) => entry.capture)).toEqual([true]);
    const key = (name: string, active: unknown, defaultPrevented = false) => {
      page.activeElement = active; const event = { key: name, defaultPrevented, preventDefault: vi.fn(), stopPropagation: vi.fn() };
      for (const entry of listeners) entry.listener(event); return event;
    };
    expect(key("Escape", outside).preventDefault).not.toHaveBeenCalled(); expect(key("Enter", inside).preventDefault).not.toHaveBeenCalled();
    expect(key("Escape", inside, true).preventDefault).not.toHaveBeenCalled(); expect(onHide).not.toHaveBeenCalled();
    const handled = key("Escape", inside);
    expect(onHide).toHaveBeenCalledTimes(1); expect(handled.preventDefault).toHaveBeenCalled(); expect(handled.stopPropagation).toHaveBeenCalled();
    harness.unmount(); expect(listeners).toHaveLength(0);
  });

  it.each([true, false])("is a named region with Hide first and no backdrop, so a press outside never hides it (compact=%s)", (compact) => {
    const onHide = vi.fn();
    const tree = harness.render(() => NoticeTray({ colors, compact, onHide, children: "Fixture notice" })) as Element;
    expect(tree.props).toMatchObject({ role: "region", accessibilityLabel: "Notices" });
    const [hide] = [tree.props.children].flat() as Element[];
    // Compact: a 44 pt close mark, so a notice and its actions keep one row on a phone.
    if (compact) expect([hide!.props.accessibilityLabel, hide!.props.style, walk(hide).find((element) => element.type === "Icon")?.props.name]).toEqual(["Hide notices", expect.objectContaining({ width: 44, height: 44 }), "X"]);
    else expect(hide!.props).toMatchObject({ label: "Hide", icon: "X", iconOnly: true, accessibilityLabel: "Hide notices", small: true });
    expect(walk(tree).filter((element) => typeof element.props.onPress === "function")).toEqual([hide]);
    press(hide); expect(onHide).toHaveBeenCalledTimes(1);
  });
});

describe("NoticesButton pointer sizes", () => {
  afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });
  it("keeps an icon and count in a 44pt target on desktop and touch", async () => {
    const notice: Notice = { keys: ["archive:a"], summary: "1 archive not confirmed", tone: "warning", node: null };
    const sizes: Record<string, number> = {};
    for (const pointer of ["fine", "coarse"] as const) {
      vi.resetModules(); vi.stubGlobal("matchMedia", (query: string) => ({ matches: query === "(pointer: coarse)" && pointer === "coarse" }));
      const { NoticesButton: Button } = await import("../client/notice-tray");
      for (const compact of [false, true]) sizes[`${pointer}:${compact ? "compact" : "wide"}`] = ((Button({ notices: [notice], colors, compact, onPress: () => undefined }) as unknown as Element).props.style as (state: object) => { minHeight: number })({}).minHeight;
    }
    expect(sizes).toEqual({ "fine:wide": 44, "fine:compact": 44, "coarse:wide": 44, "coarse:compact": 44 });
  });
});
