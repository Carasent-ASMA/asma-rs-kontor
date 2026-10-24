// ASMA-8644: a live update re-sorts rows, never the list's place. The anchoring itself happens in the
// platform (native maintainVisibleContentPosition, browser scroll anchoring); sessions-scroll-anchor.browser.cjs
// shows the jump and its absence in a browser. Here the screen must ask for neither and scroll nothing itself.
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
    fake: { state: null as unknown, toast: { show: vi.fn(), error: vi.fn() } },
  };
});
vi.mock("react", async (importOriginal) => ({ ...(await importOriginal<typeof import("react")>()), useContext: () => null, memo: (component: unknown) => component,
  useCallback: harness.useCallback, useEffect: harness.useEffect, useLayoutEffect: harness.useEffect, useMemo: harness.useMemo, useRef: harness.useRef, useState: harness.useState }));
vi.mock("react-native", () => ({
  AccessibilityInfo: { addEventListener: () => ({ remove: () => undefined }), isReduceMotionEnabled: async () => false, isScreenReaderEnabled: async () => false, setAccessibilityFocus: () => undefined },
  ActivityIndicator: "ActivityIndicator", Animated: { Value: class { setValue() {} stopAnimation() {} }, View: "Animated.View", timing: () => ({ start: (done?: () => void) => done?.(), stop: () => undefined }) },
  BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, findNodeHandle: () => null,
  PanResponder: { create: () => ({ panHandlers: {} }) }, Platform: { OS: "ios" }, Pressable: "Pressable", Text: "Text", View: "View",
}));
vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: () => () => Promise.reject(new Error("Unavailable in this fixture")),
  getPaseoClient: () => { throw new Error("No host client in this fixture"); },
  useHosts: () => [{ serverId: "host-a", status: "online" }],
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ FlatList: "FlatList", Icon: "Icon", ScrollView: "ScrollView", TextInput: "TextInput", useToast: () => fake.toast }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: fake.state, refresh: () => undefined }) }));

import { SessionsScreen } from "../client/sessions-screen";
import type { DirectoryEntry, DirectoryState } from "../client/session-directory";
import { clearSessionViews, patchSessionView, readSessionView } from "../client/session-view-store";
import { sessionEntry } from "./session-fixtures";

type Element = { readonly type?: unknown; readonly props: Record<string, unknown> };
const walk = (node: unknown): Element[] => {
  if (node === null || node === undefined || typeof node !== "object") return [];
  if (Array.isArray(node)) return node.flatMap(walk);
  const element = node as Element;
  return [element, ...walk(element.props.children), ...walk(element.props.ListHeaderComponent)];
};
const list = () => walk(harness.flush()).find((element) => element.props.testID === "sessions-list")!;
const colors = { accent: "#05f", accentForeground: "#fff", border: "#ccc", foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#f4f4f4", surface2: "#eee", statusDanger: "#c00", statusSuccess: "#080", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
// Newest first, as the directory orders them: a long idle history under the live rows.
const minute = (n: number) => new Date(Date.UTC(2026, 9, 24, 12, 0) - n * 60_000).toISOString();
const HISTORY = Array.from({ length: 60 }, (_, index) => sessionEntry(`s${index}`, { title: `Session ${index}`, updatedAt: minute(index) }));
const ready = (entries: readonly DirectoryEntry[]): DirectoryState => ({ entries: [...entries], status: "ready", live: true, error: null, staleReason: null, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, includeArchived: false });
const ids = () => (list().props.data as { kind: string; entry?: { id: string } }[]).flatMap((row) => row.entry ? [row.entry.id] : []);
const scrollTo = vi.fn(), scrollToIndex = vi.fn();

function mount() {
  harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Fixture host" }, layout: { compact: true, platform: "ios" },
    navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {} } as Parameters<typeof SessionsScreen>[0]));
  (list().props.ref as { current: unknown }).current = { scrollToOffset: scrollTo, scrollToIndex, getScrollableNode: () => null };
}
/** Opens every group through its row's own toggle, as a user would. */
function expandAll() {
  for (let pass = 0; pass < 10; pass += 1) {
    const closed = (list().props.data as { kind: string; expanded?: boolean }[]).filter((row) => row.kind === "group" && !row.expanded);
    if (!closed.length) return;
    for (const item of closed) ((list().props.renderItem as (info: { item: unknown }) => Element)({ item }).props.onToggle as () => void)();
    harness.flush();
  }
}
/** The user has scrolled deep into the list; the screen only learns it from scroll events. */
function scrolled(offset: number) {
  (list().props.onScroll as (event: unknown) => void)({ nativeEvent: { contentOffset: { y: offset }, contentSize: { height: 9000 }, layoutMeasurement: { height: 700 } } });
  harness.flush();
}
/** A live upsert: the session now runs, so the directory sorts it to the top. */
let clock = 0;
function liveUpdate(id: string) {
  const current = (fake.state as DirectoryState).entries;
  const moved = { ...current.find((entry) => entry.id === id)!, status: "running" as const, updatedAt: minute(-(clock += 1)) };
  fake.state = ready([moved, ...current.filter((entry) => entry.id !== id)]);
  harness.flush();
}

beforeEach(() => { clearSessionViews(); clock = 0; fake.state = ready(HISTORY); scrollTo.mockClear(); scrollToIndex.mockClear(); });
afterEach(() => { harness.unmount(); delete (globalThis as { document?: unknown }).document; });

describe("Sessions list place across live updates", () => {
  for (const [name, view] of [["All, flat", { status: "all", layout: "flat" }], ["Current, grouped", { status: "current", layout: "grouped" }]] as const) {
    it(`${name}: re-sorting the row in view moves the row, not the list`, () => {
      patchSessionView("host-a", view);
      mount();
      expandAll();
      scrolled(4200);
      const shown = ids().length;
      expect(shown).toBe(60);
      // The row at the top of the viewport is the one the platform anchor would have followed to the top.
      for (const id of ["s30", "s31", "s45", "s31"]) liveUpdate(id);
      if (view.layout === "flat") expect(ids().slice(0, 3)).toEqual(["s31", "s45", "s30"]);
      expect(ids()).toHaveLength(shown);
      expect(list().props.maintainVisibleContentPosition).toBeUndefined();
      expect(scrollTo).not.toHaveBeenCalled();
      expect(scrollToIndex).not.toHaveBeenCalled();
      expect(readSessionView("host-a").scrollOffset).toBe(4200);
    });
  }

  it("native keeps its own scroll view; only a browser opts out of scroll anchoring", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    expect(list().props.style).toBeUndefined();
    harness.unmount();
    (globalThis as { document?: unknown }).document = {};
    mount();
    expect(list().props.style).toEqual({ overflowAnchor: "none" });
    expect(list().props.maintainVisibleContentPosition).toBeUndefined();
  });
});
