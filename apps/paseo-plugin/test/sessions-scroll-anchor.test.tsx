// ASMA-8644: top stays top, bottom stays bottom; in between the exact first visible session keeps its
// viewport-relative offset while it remains in the filter. Only a missing session permits a neighbor fallback. While the reader scrolls, or their own choice places the list, the screen holds
// none. Platform anchoring (native maintainVisibleContentPosition, browser scroll anchoring) follows the first
// visible row even when the update re-sorts it, back to the start of the list; the screen asks for neither and
// holds the place itself, from row heights measured per row. sessions-scroll-anchor.browser.cjs shows both in a
// browser.
import type { PluginTheme } from "@getpaseo/plugin";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Slot = { initialized?: boolean; value?: unknown; deps?: readonly unknown[]; cleanup?: () => void };
type Queued = { readonly slot: Slot; readonly fn: () => void | (() => void); readonly deps?: readonly unknown[] };
const { harness, fake } = vi.hoisted(() => {
  const slots: Slot[] = [];
  let cursor = 0, dirty = false, component: (() => unknown) | null = null, tree: unknown = null, effects: Queued[] = [];
  let commit: ((tree: unknown) => void) | null = null;
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
      const refCommit = commit; commit = null; refCommit?.(tree);
      for (const item of effects) { if (item.deps && same(item.slot.deps, item.deps)) continue; item.slot.cleanup?.(); const cleanup = item.fn(); item.slot.deps = item.deps; item.slot.cleanup = typeof cleanup === "function" ? cleanup : undefined; }
    } while (dirty && ++guard < 20);
    return tree;
  }
  return {
    harness: { ...hooks, beforeEffects(fn: (tree: unknown) => void) { commit = fn; }, render(fn: () => unknown) { component = fn; return draw(); }, flush: draw,
      unmount() { for (const slot of slots) slot.cleanup?.(); slots.length = 0; component = null; tree = null; commit = null; } },
    fake: { state: null as unknown, toast: { show: vi.fn(), error: vi.fn() } },
  };
});
vi.mock("react", async (importOriginal) => ({ ...(await importOriginal<typeof import("react")>()), useContext: () => null, memo: (component: unknown) => component,
  useCallback: harness.useCallback, useEffect: harness.useEffect, useLayoutEffect: harness.useEffect, useMemo: harness.useMemo, useRef: harness.useRef, useState: harness.useState,
  useSyncExternalStore: (_subscribe: unknown, read: () => unknown) => read() }));
vi.mock("react-native", () => ({
  AccessibilityInfo: { addEventListener: () => ({ remove: () => undefined }), isReduceMotionEnabled: async () => false, isScreenReaderEnabled: async () => false, setAccessibilityFocus: () => undefined },
  ActivityIndicator: "ActivityIndicator", Animated: { Value: class { setValue() {} stopAnimation() {} }, View: "Animated.View", timing: () => ({ start: (done?: () => void) => done?.(), stop: () => undefined }) },
  BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, findNodeHandle: () => null,
  useWindowDimensions: () => ({ width: 390, height: 844, fontScale: 1, scale: 1 }),
  PanResponder: { create: () => ({ panHandlers: {} }) }, Platform: { OS: "ios" }, Pressable: "Pressable", Text: "Text", View: "View",
}));
vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: () => () => Promise.reject(new Error("Unavailable in this fixture")),
  getPaseoClient: () => { throw new Error("No host client in this fixture"); },
  useHosts: () => [{ serverId: "host-a", status: "online" }],
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ FlatList: "FlatList", Icon: "Icon", ScrollView: "ScrollView", TextInput: "TextInput", useToast: () => fake.toast }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: fake.state, refresh: () => undefined }) }));

import { createRowLayout, SessionsScreen } from "../client/sessions-screen";
import type { DirectoryEntry, DirectoryState } from "../client/session-directory";
import { clearSessionViews, patchSessionView, readSessionView } from "../client/session-view-store";
import { sessionEntry } from "./session-fixtures";
import { configureRecentStorage, recordVisit } from "../client/recent-sessions";

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
/** The user has scrolled; the screen only learns it from scroll events. */
function scrolled(offset: number, content = 9000) {
  (list().props.onScroll as (event: unknown) => void)({ nativeEvent: { contentOffset: { y: offset }, contentSize: { height: content }, layoutMeasurement: { height: 700 } } });
  harness.flush();
}
/** The list reports its content this tall. */
function contentSize(height: number) {
  (list().props.onContentSizeChange as (width: number, height: number) => void)(320, height);
  harness.flush();
}
/** The scroll has ended: the reader no longer owns the offset. */
const settle = () => vi.setSystemTime(Date.now() + 1000);
/** A live upsert: the session now runs, so the directory sorts it to the top. */
let clock = 0;
function liveUpdate(id: string) {
  const current = (fake.state as DirectoryState).entries;
  const moved = { ...current.find((entry) => entry.id === id)!, status: "running" as const, updatedAt: minute(-(clock += 1)) };
  fake.state = ready([moved, ...current.filter((entry) => entry.id !== id)]);
  harness.flush();
}
/** A session that starts streaming, or one that leaves the directory. */
function arrive(id: string) {
  fake.state = ready([sessionEntry(id, { title: `Session ${id}`, status: "running", updatedAt: minute(-(clock += 1)) }), ...(fake.state as DirectoryState).entries]);
  harness.flush();
}
function leave(id: string) {
  fake.state = ready((fake.state as DirectoryState).entries.filter((entry) => entry.id !== id));
  harness.flush();
}
type ItemLayout = (data: unknown, index: number) => { readonly offset: number; readonly length: number };
/** Each row's key and its distance from the viewport top, as the list lays it out at this offset. */
function inView(offset: number) {
  const { data, keyExtractor, getItemLayout } = list().props as { data: unknown[]; keyExtractor: (row: unknown) => string; getItemLayout: ItemLayout };
  return data.flatMap((row, index) => {
    const { offset: top, length } = getItemLayout(data, index);
    return top + length > offset && top < offset + 700 ? [{ key: keyExtractor(row), top: top - offset }] : [];
  });
}
const offsetNow = () => readSessionView("host-a").scrollOffset;

beforeEach(() => { vi.useFakeTimers({ toFake: ["Date"] }); clearSessionViews(); clock = 0; fake.state = ready(HISTORY); configureRecentStorage(null); for (const entry of [...HISTORY].reverse()) recordVisit("host-a", entry.id); scrollTo.mockClear(); scrollToIndex.mockClear(); });
afterEach(() => { harness.unmount(); vi.useRealTimers(); delete (globalThis as { document?: unknown }).document; });

describe("Sessions list place across live updates", () => {
  function pendingVisibleCell() {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    const props = list().props;
    const reads: ((...values: number[]) => void)[] = [];
    const cell = (props.CellRendererComponent as (props: object) => Element)({ cellKey: "s43", index: 43, item: (props.data as unknown[])[43], children: null });
    (cell.props.ref as (node: unknown) => void)({ measure: (done: (...values: number[]) => void) => reads.push(done) });
    scrolled(4265, 6000);
    expect(reads).toHaveLength(1);
    settle();
    return reads;
  }

  function pendingWindow() {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    const props = list().props;
    const reads = new Map<number, (...values: number[]) => void>();
    for (const index of [1, 43, 44]) {
      const cell = (props.CellRendererComponent as (props: object) => Element)({ cellKey: `s${index}`, index, item: (props.data as unknown[])[index], children: null });
      (cell.props.ref as (node: unknown) => void)({ measure: (done: (...values: number[]) => void) => reads.set(index, done) });
    }
    const top = (props.getItemLayout as ItemLayout)(props.data, 43).offset;
    scrolled(top + 65, 6000);
    settle();
    return { reads, top };
  }

  it("keeps a settled exact ID authoritative after its cell detaches before a live reorder", () => {
    const reads = pendingVisibleCell();
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    const props = list().props;
    const cell = (props.CellRendererComponent as (props: object) => Element)({ cellKey: "s43", index: 43, item: (props.data as unknown[])[43], children: null });
    (cell.props.ref as (node: unknown) => void)(null);
    liveUpdate("s43");
    expect(ids()[0]).toBe("s43");
    expect(offsetNow()).toBe(65);
    expect(reads).toHaveLength(1);
  });

  it("accounts for header-only growth before the queued capture settles without new reads", () => {
    const reads = pendingVisibleCell();
    ((list().props.ListHeaderComponent as Element).props.onLayout as (event: unknown) => void)(layoutEvent(100));
    expect(reads).toHaveLength(1);
    reads[0]!(0, 4200, 320, 96, 0, 4200); // Valid geometry from before the header grew.
    expect(offsetNow()).toBe(4365);
    expect(reads).toHaveLength(1);
  });

  it("keeps the same header-only offset when geometry completes before header growth", () => {
    const reads = pendingVisibleCell();
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    ((list().props.ListHeaderComponent as Element).props.onLayout as (event: unknown) => void)(layoutEvent(100));
    expect(reads).toHaveLength(2); // Existing settled-row refinement reads the new public geometry.
    reads[1]!(0, 4300, 320, 96, 0, 4300);
    expect(offsetNow()).toBe(4365);
  });

  it("uses the final header layout when several header changes precede the queued reply", () => {
    const reads = pendingVisibleCell();
    const changed = (list().props.ListHeaderComponent as Element).props.onLayout as (event: unknown) => void;
    changed(layoutEvent(100));
    changed(layoutEvent(60));
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(4325);
    expect(reads).toHaveLength(1);
  });

  it("accounts for both header growth and a live reorder during the original capture", () => {
    const reads = pendingVisibleCell();
    ((list().props.ListHeaderComponent as Element).props.onLayout as (event: unknown) => void)(layoutEvent(100));
    liveUpdate("s43");
    const props = list().props;
    const top = (props.getItemLayout as ItemLayout)(props.data, 0).offset;
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(top + 65);
    expect(reads).toHaveLength(1);
  });

  it("does not restore an old header capture over a newer reader scroll", () => {
    const reads = pendingVisibleCell();
    ((list().props.ListHeaderComponent as Element).props.onLayout as (event: unknown) => void)(layoutEvent(100));
    scrolled(2000, 6100);
    scrollTo.mockClear();
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(2000);
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("uses visible geometry when a detached offscreen cell never replies", () => {
    const { reads, top } = pendingWindow();
    reads.get(43)!(0, top, 320, 96, 0, top);
    reads.get(44)!(0, top + 96, 320, 96, 0, top + 96);
    // Cell s1 detached before its queued public measure ran: there is no callback to wait for.
    arrive("arrival-with-detached-cell");
    expect(offsetNow()).toBe(top + 96 + 65);
  });

  it("accepts the earlier visible ID's late reply without waiting for a detached cell", () => {
    const { reads, top } = pendingWindow();
    liveUpdate("s43");
    reads.get(44)!(0, top + 96, 320, 96, 0, top + 96);
    reads.get(43)!(0, top, 320, 96, 0, top);
    expect(offsetNow()).toBe(65);
    expect(ids()[0]).toBe("s43");
  });

  it("retains valid queued geometry when the moved exact cell unmounts before its reply", () => {
    const reads = pendingVisibleCell();
    harness.beforeEffects((tree) => {
      const props = walk(tree).find((element) => element.props.testID === "sessions-list")!.props;
      const cell = (props.CellRendererComponent as (props: object) => Element)({ cellKey: "s43", index: 0, item: (props.data as unknown[])[0], children: null });
      (cell.props.ref as (node: unknown) => void)(null);
    });
    liveUpdate("s43");
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(65);
    expect(reads).toHaveLength(1);
  });

  it("does not replace queued pre-update geometry when post-update cell refs commit before follow", () => {
    const reads = pendingVisibleCell();
    harness.beforeEffects((tree) => {
      const props = walk(tree).find((element) => element.props.testID === "sessions-list")!.props;
      const item = (props.data as unknown[])[0];
      const cell = (props.CellRendererComponent as (props: object) => Element)({ cellKey: "s43", index: 0, item, children: null });
      (cell.props.ref as (node: unknown) => void)({ measure: (done: (...values: number[]) => void) => reads.push(done) });
    });
    liveUpdate("s43");
    expect(reads).toHaveLength(1);
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(65);
  });

  it("resolves a pending capture from its original viewport after a native content clamp", () => {
    const reads = pendingVisibleCell();
    liveUpdate("s43");
    scrolled(4000, 4700); // Browser/native clamp, not a reader move: max = 4700 - 700.
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(65);
    expect(reads).toHaveLength(1);
  });

  it("does not apply a delayed live-update capture over a newly started drag", () => {
    const reads = pendingVisibleCell();
    liveUpdate("s43");
    (list().props.onScrollBeginDrag as () => void)();
    scrollTo.mockClear();
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(4265);
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("keeps a queued pre-update exact-ID capture when a live reorder settles before its callback", () => {
    const reads = pendingVisibleCell();
    liveUpdate("s43");
    expect(ids()[0]).toBe("s43");
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(65); // The same exact s43 stays at -65px, rather than estimated s44 winning.
    expect(scrollTo).toHaveBeenLastCalledWith({ offset: 65, animated: false });
    expect(reads).toHaveLength(1); // Consume the queued geometry; do not request replacement evidence.
  });

  it("keeps the same anchor when its geometry arrives before the live reorder", () => {
    const reads = pendingVisibleCell();
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    liveUpdate("s43");
    expect(offsetNow()).toBe(65);
  });

  it("lets a newer reader scroll invalidate geometry queued before a live reorder", () => {
    const reads = pendingVisibleCell();
    liveUpdate("s43");
    scrolled(2000, 6000);
    scrollTo.mockClear();
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(2000);
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("maps a queued exact-ID capture through every intervening live update", () => {
    const reads = pendingVisibleCell();
    liveUpdate("s43");
    arrive("arrival-after-reorder");
    liveUpdate("s2");
    const props = list().props;
    const index = ids().indexOf("s43");
    const top = (props.getItemLayout as ItemLayout)(props.data, index).offset;
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offsetNow()).toBe(top + 65);
    expect(reads).toHaveLength(1);
  });

  it("keeps the original anchor through a temporary notice resize and a delayed duplicate scroll", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    const props = list().props;
    const item = (props.data as unknown[])[43];
    let top = 43 * 96;
    const cell = (props.CellRendererComponent as (props: object) => Element)({ cellKey: "r43", index: 43, item, children: null });
    (cell.props.ref as (node: unknown) => void)({ measure: (done: (...values: number[]) => void) => done(0, top, 320, 96, 0, top) });
    scrolled(4200, 5760);
    settle();
    top -= 30;
    contentSize(5730);
    expect(offsetNow()).toBe(4170);
    settle();
    top += 30;
    scrolled(4170, 5760); // Delayed end sample from the screen's placement, with no reader movement.
    contentSize(5760);
    expect(offsetNow()).toBe(4200);
  });

  for (const [name, view] of [["All, flat", { status: "all", layout: "flat" }], ["Current, grouped", { status: "current", layout: "grouped" }], ["Recent, flat", { status: "recent", layout: "flat" }]] as const) {
    it(`${name}: re-sorting rows keeps the first row in view where it was, not the list on a moved row`, () => {
      patchSessionView("host-a", view);
      mount();
      expandAll();
      scrolled(4200);
      settle();
      const shown = ids().length;
      expect(shown).toBe(60);
      const [first] = inView(4200);
      // s45 is in view below the first row and sorts above it; the platform anchor would follow s45 to the top.
      for (const id of ["s30", "s31", "s45", "s31"]) liveUpdate(id);
      if (view.status === "all") expect(ids().slice(0, 3)).toEqual(["s31", "s45", "s30"]);
      expect(ids()).toHaveLength(shown);
      expect(inView(offsetNow())[0]).toEqual(first);
      expect(offsetNow()).toBeGreaterThanOrEqual(4200);
      if (view.status === "recent") expect(scrollTo).not.toHaveBeenCalled();
      else expect(scrollTo).toHaveBeenLastCalledWith({ offset: offsetNow(), animated: false });
      expect(list().props.maintainVisibleContentPosition).toBeUndefined();
      expect(scrollToIndex).not.toHaveBeenCalled();
    });
  }

  it("preserves the exact first visible ID and visual offset when that session itself re-sorts", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    scrolled(4200);
    settle();
    const [moved, next] = inView(4200);
    liveUpdate(moved!.key.slice("result:".length));
    expect(ids()[0]).toBe(moved!.key.slice("result:".length));
    // A present exact ID stays the anchor even when its native sort position changes.
    expect(inView(offsetNow()).find((row) => row.key === moved!.key)).toEqual(moved);
  });

  it("passes the place to the next row in view when the first one leaves", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    scrolled(4200);
    settle();
    const [gone, next] = inView(4200);
    leave(gone!.key.slice("result:".length));
    expect(inView(offsetNow()).find((row) => row.key === next!.key)).toEqual(next);
    expect(offsetNow()).toBe(4200 - 96);
  });

  it("keeps a reader at the top at the top, and one at the end at the end", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    scrolled(0);
    settle();
    arrive("new-top");
    liveUpdate("s45");
    expect(ids()[0]).toBe("s45");
    expect(scrollTo).not.toHaveBeenCalled();
    // The end of 61 estimated rows: arrivals grow the list under the reader, who stays at its end.
    scrolled(61 * 96 - 700, 61 * 96);
    settle();
    arrive("new-end");
    expect(offsetNow()).toBe(62 * 96 - 700);
    leave("s3");
    expect(offsetNow()).toBe(61 * 96 - 700);
  });

  it("keeps the reader's row in place when a row or the header above it changes height", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    const { data, CellRendererComponent: Cell } = list().props as { data: unknown[]; CellRendererComponent: (props: object) => Element };
    const measure = (index: number, height: number) => (Cell({ cellKey: "cell", index, item: data[index], style: undefined, children: null }).props.onLayout as (event: unknown) => void)(layoutEvent(height));
    for (let index = 0; index < 60; index += 1) measure(index, 100);
    scrolled(4200);
    settle();
    const [first] = inView(4200);
    measure(40, 160);
    ((list().props.ListHeaderComponent as Element).props.onLayout as (event: unknown) => void)(layoutEvent(30));
    expect(offsetNow()).toBe(4290);
    expect(inView(offsetNow())[0]).toEqual(first);
    // A row below the reader grows without moving them.
    measure(50, 200);
    expect(offsetNow()).toBe(4290);
  });

  it("keeps the reader's row through the browser pulling the offset back to the end of a shorter list", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    const { data, CellRendererComponent: Cell } = list().props as { data: unknown[]; CellRendererComponent: (props: object) => Element };
    const measure = (index: number, height: number) => (Cell({ cellKey: "cell", index, item: data[index], style: undefined, children: null }).props.onLayout as (event: unknown) => void)(layoutEvent(height));
    for (let index = 0; index < 60; index += 1) measure(index, 96);
    scrolled(4200);
    settle();
    const [first] = inView(4200);
    // Ten rows above the reader grow while the host is offline, and the reader's row stays where it was.
    for (let index = 30; index < 40; index += 1) measure(index, 164);
    expect(offsetNow()).toBe(4880);
    // Back online they shrink: the browser pulls the offset to the end of the shorter list before their
    // measurements arrive. That is not the reader's scroll, and it does not move their row.
    scrolled(4600, 5300);
    for (let index = 30; index < 40; index += 1) measure(index, 96);
    expect(offsetNow()).toBe(4200);
    expect(inView(4200)[0]).toEqual(first);
    // A scroll the reader made a moment ago keeps the next change from moving them.
    scrollTo.mockClear();
    scrolled(3000, 4904);
    measure(11, 200);
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("keeps a reader at the end at the end the list reports, not where late row measurements put it", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    const { data, CellRendererComponent: Cell } = list().props as { data: unknown[]; CellRendererComponent: (props: object) => Element };
    const measure = (index: number, height: number) => (Cell({ cellKey: "cell", index, item: data[index], style: undefined, children: null }).props.onLayout as (event: unknown) => void)(layoutEvent(height));
    for (let index = 0; index < 60; index += 1) measure(index, 96);
    scrolled(60 * 96 - 700, 60 * 96);
    settle();
    // A notice above the list takes 46 px of its viewport: the reader follows the end down.
    (list().props.onLayout as (event: unknown) => void)(layoutEvent(654));
    expect(scrollTo).toHaveBeenLastCalledWith({ offset: 60 * 96 - 654, animated: false });
    // The rows the list shows grow, and it reports the longer content before their measurements arrive.
    contentSize(60 * 96 + 46);
    expect(scrollTo).toHaveBeenLastCalledWith({ offset: 60 * 96 + 46 - 654, animated: false });
    scrollTo.mockClear();
    measure(0, 142);
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("leaves the offset to the reader while they scroll and to their own choices", () => {
    patchSessionView("host-a", { status: "current", layout: "grouped" });
    mount();
    expandAll();
    scrolled(4200);
    liveUpdate("s45");
    expect(scrollTo).not.toHaveBeenCalled();
    settle();
    (list().props.onScrollBeginDrag as () => void)();
    liveUpdate("s46");
    expect(scrollTo).not.toHaveBeenCalled();
    (list().props.onScrollEndDrag as () => void)();
    settle();
    (list().props.onMomentumScrollBegin as () => void)();
    liveUpdate("s44");
    expect(scrollTo).not.toHaveBeenCalled();
    (list().props.onMomentumScrollEnd as () => void)();
    settle();
    // A browser reports no drag, only the finger on the list.
    (list().props.onTouchStart as () => void)();
    liveUpdate("s43");
    expect(scrollTo).not.toHaveBeenCalled();
    (list().props.onTouchEnd as () => void)();
    settle();
    // A group the reader closes and opens again moves the rows under the offset by their choice.
    const toggle = () => {
      const open = (list().props.data as { kind: string; key?: string }[]).find((row) => row.kind === "group")!;
      ((list().props.renderItem as (info: { item: unknown }) => Element)({ item: open }).props.onToggle as () => void)();
      harness.flush();
    };
    toggle();
    toggle();
    expect(scrollTo).not.toHaveBeenCalled();
    // Then a streamed change keeps the reader's place again.
    liveUpdate("s47");
    expect(scrollTo).toHaveBeenCalledTimes(1);
  });

  it("Back restores the exact result after jumping across unmeasured rows and measuring a different peek", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    scrolled(4200);
    settle();
    const first = inView(4200)[0]!;
    const row = (list().props.data as { kind: string; entry: DirectoryEntry }[]).find(row => `result:${row.entry.id}` === first.key)!;
    const rendered = (list().props.renderItem as (info: { item: unknown }) => Element)({ item: row });
    // A result row holds Show in tree in its actions menu.
    const mode = walk(rendered).find((element) => (element.props.mode as { kind?: string } | undefined)?.kind === "open")!.props.mode as { actions: Element };
    void (mode.actions.props.actions as { key: string; perform: () => unknown }[]).find((action) => action.key === "show-in-tree")!.perform();
    harness.flush();
    const peekRow = (list().props.data as { kind: string; entry?: DirectoryEntry }[]).find(row => row.kind === "session")!;
    const Cell = list().props.CellRendererComponent as (props: object) => Element;
    (Cell({ cellKey: "peek", index: 0, item: peekRow, children: null }).props.onLayout as (event: unknown) => void)(layoutEvent(240, 390));
    harness.flush();
    const back = walk(harness.flush()).find(element => String(element.props.label ?? "").startsWith("Back to "))!;
    (back.props.onPress as () => void)();
    harness.flush();
    expect(inView(offsetNow()).find(row => row.key === first.key)).toEqual(first);
  });

  it("gives a real drag precedence even when it lands on a recent own placement", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    scrolled(4200);
    settle();
    arrive("first-arrival");
    const prior = offsetNow();
    arrive("second-arrival");
    (list().props.onScrollBeginDrag as () => void)();
    scrolled(prior);
    scrollTo.mockClear();
    liveUpdate("s43");
    expect(offsetNow()).toBe(prior);
    expect(scrollTo).not.toHaveBeenCalled();
  });

  it("uses a surviving neighbor when an exact ID leaves Current but stays in the native directory", () => {
    patchSessionView("host-a", { status: "current", layout: "flat" });
    mount();
    scrolled(4200);
    settle();
    const [first, next] = inView(4200);
    const id = first!.key.slice("result:".length);
    fake.state = ready((fake.state as DirectoryState).entries.map(entry => entry.id === id ? { ...entry, status: "closed" as const } : entry));
    harness.flush();
    expect((fake.state as DirectoryState).entries.some(entry => entry.id === id)).toBe(true);
    expect(ids()).not.toContain(id);
    expect(inView(offsetNow()).find(row => row.key === next!.key)).toEqual(next);
  });

  it("does not mistake delayed own-scroll callbacks for a reader gesture", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    scrolled(4200);
    settle();
    const first = inView(4200)[0]!;
    arrive("first-arrival");
    const older = offsetNow();
    arrive("second-arrival");
    const latest = offsetNow();
    scrolled(older); // The first placement's event arrives after the screen requested the second.
    scrolled(latest);
    liveUpdate(first.key.slice("result:".length));
    expect(inView(offsetNow()).find(row => row.key === first.key)).toEqual(first);
  });

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

type Layout = ReturnType<typeof createRowLayout>;
const result = (id: string) => ({ kind: "result" as const, entry: sessionEntry(id) });
const layoutEvent = (height: number, width = 320) => ({ nativeEvent: { layout: { x: 0, y: 0, width, height } } });
/** Lays a cell out the way the list does: through the cell component's own onLayout. */
function measure(layout: Layout, item: unknown, height: number, width = 320) {
  const cell = layout.Cell({ cellKey: "cell", index: 0, item, style: undefined, children: null } as Parameters<Layout["Cell"]>[0]) as unknown as Element;
  (cell.props.onLayout as (event: unknown) => void)(layoutEvent(height, width));
}

describe("row layout by measured row", () => {
  it("applies queued pre-update geometry to a present exact ID after follow has already run", () => {
    let offset = 4265;
    const moved = vi.fn((by?: number, from?: number) => { if (by !== undefined) offset = (from ?? offset) + by; });
    const layout = createRowLayout(moved);
    const rows = Array.from({ length: 60 }, (_, index) => result(`r${index}`));
    const reads: ((...values: number[]) => void)[] = [];
    const cell = layout.Cell({ cellKey: "r43", index: 43, item: rows[43]!, children: null } as never) as unknown as Element;
    (cell.props.ref as (node: unknown) => void)({ measure: (done: (...values: number[]) => void) => reads.push(done) });
    layout.capture(rows, false, offset, 300, 6000);
    const next = [rows[43]!, ...rows.filter((_, index) => index !== 43)];
    offset += layout.follow(rows, next, false, offset, 300, 6000, true);
    reads[0]!(0, 4200, 320, 96, 0, 4200);
    expect(offset).toBe(65);
    expect(reads).toHaveLength(1);
  });

  it("captures the exact first session when its virtualized cell mounts after the scroll event", () => {
    const layout = createRowLayout();
    const rows = Array.from({ length: 60 }, (_, index) => result(`r${index}`));
    layout.capture(rows, false, 4265, 300, 6000); // Estimates nominate r44, but the actual viewport starts at r43.
    const cell = layout.Cell({ cellKey: "r43", index: 43, item: rows[43]!, children: null } as never) as unknown as Element;
    (cell.props.ref as (node: unknown) => void)({ measure: (done: (...values: number[]) => void) => done(0, 4200, 320, 96, 0, 4200) });
    const next = [rows[43]!, ...rows.filter((_, index) => index !== 43)];
    expect(layout.follow(rows, next, false, 4265, 300, 6000, true)).toBe(-4200);
  });

  it("discards an earlier asynchronous geometry read after a new reader scroll", () => {
    const layout = createRowLayout();
    const rows = Array.from({ length: 60 }, (_, index) => result(`r${index}`));
    const reads: ((...values: number[]) => void)[] = [];
    const cell = layout.Cell({ cellKey: "r43", index: 43, item: rows[43]!, children: null } as never) as unknown as Element;
    (cell.props.ref as (node: unknown) => void)({ measure: (done: (...values: number[]) => void) => reads.push(done) });
    layout.capture(rows, false, 4265, 300, 6000);
    layout.capture(rows, false, 2000, 300, 6000);
    reads[0]!(0, 4200, 320, 96, 0, 4200); // A delayed answer from the earlier viewport cannot capture r43.
    reads[1]!(0, 4200, 320, 96, 0, 4200);
    const next = [rows[43]!, ...rows.filter((_, index) => index !== 43)];
    expect(layout.follow(rows, next, false, 2000, 300, 6000, true)).toBe(96);
  });

  it("uses current cell geometry after a spacer moves without a new size layout event", () => {
    const moves = vi.fn();
    const layout = createRowLayout(moves);
    const rows = Array.from({ length: 60 }, (_, index) => result(`r${index}`));
    let top = 4200;
    const cell = layout.Cell({ cellKey: "r43", index: 43, item: rows[43]!, children: null } as never) as unknown as Element;
    (cell.props.ref as (node: unknown) => void)({ measure: (done: (...values: number[]) => void) => done(0, top, 320, 96, 0, top) });
    (cell.props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 3900, width: 320, height: 96 } } });
    // The virtualized spacer moved this same-size cell. Its old onLayout y is no longer its position.
    layout.capture(rows, false, 4265, 300, 6000);
    const next = [rows[43]!, ...rows.filter((_, index) => index !== 43)];
    expect(layout.follow(rows, next, false, 4265, 300, 6000, true)).toBe(-4200);
    top = 33;
    layout.restate(next, false);
    expect(moves).toHaveBeenLastCalledWith(33);
  });

  it("keeps a row's offset through a re-sort that keeps the rows above it", () => {
    const layout = createRowLayout();
    const rows = ["a", "b", "c", "d", "e"].map(result);
    rows.forEach((row, index) => measure(layout, row, 100 + 10 * index));
    layout.onHeaderLayout(layoutEvent(40) as never);
    expect(layout.layout(rows, 4, true)).toEqual({ index: 4, length: 140, offset: 40 + 100 + 110 + 120 + 130 });
    // d turns running and sorts first: e keeps its place, d now opens the list under the header.
    const resorted = [rows[3]!, rows[0]!, rows[1]!, rows[2]!, rows[4]!];
    expect(layout.layout(resorted, 4, true).offset).toBe(40 + 100 + 110 + 120 + 130);
    expect(layout.layout(resorted, 0, true)).toEqual({ index: 0, length: 130, offset: 40 });
    expect(layout.layout(resorted, 1, false).offset).toBe(130);
  });

  it("counts a row not measured at this width as the average of those that are", () => {
    const layout = createRowLayout();
    const rows = ["a", "b", "c"].map(result);
    expect(layout.layout(rows, 2, false)).toEqual({ index: 2, length: 96, offset: 192 });
    measure(layout, rows[0], 100, 700);
    measure(layout, rows[0], 120);
    measure(layout, rows[1], 160);
    expect(layout.layout(rows, 2, false)).toEqual({ index: 2, length: 140, offset: 280 });
    // A new width: rows not yet measured at it count as the average of those that are.
    measure(layout, rows[2], 50, 400);
    expect(layout.layout(rows, 1, false)).toEqual({ index: 1, length: 50, offset: 50 });
  });

  it("retains a reported anchor position when only estimates or rows below it change", () => {
    const moves = vi.fn();
    const layout = createRowLayout(moves);
    const rows = Array.from({ length: 60 }, (_, index) => result(`r${index}`));
    const cell = layout.Cell({ cellKey: "r43", index: 43, item: rows[43]!, children: null } as never) as unknown as Element;
    (cell.props.ref as (node: unknown) => void)?.({ measure: (done: (...values: number[]) => void) => done(0, 4200, 320, 96, 0, 4200) });
    (cell.props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 4200, width: 320, height: 96 } } });
    layout.capture(rows, false, 4265, 300, 6000);
    measure(layout, rows[55]!, 160);
    const next = [...rows.slice(0, 50), rows[55]!, ...rows.slice(50, 55), ...rows.slice(56)];
    expect(layout.follow(rows, next, false, 4265, 300, 6000, true)).toBe(0);
  });

  it("anchors the reported visible cell rather than an estimated index after an unmeasured jump", () => {
    const moves = vi.fn();
    const layout = createRowLayout(moves);
    const rows = Array.from({ length: 60 }, (_, index) => result(`r${index}`));
    const cell = layout.Cell({ cellKey: "r43", index: 43, item: rows[43]!, children: null } as never) as unknown as Element;
    (cell.props.ref as (node: unknown) => void)?.({ measure: (done: (...values: number[]) => void) => done(0, 4200, 320, 96, 0, 4200) });
    (cell.props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 4200, width: 320, height: 96 } } });
    layout.capture(rows, false, 4265, 300, 6000);
    const next = [rows[43]!, ...rows.filter((_, index) => index !== 43)];
    expect(layout.follow(rows, next, false, 4265, 300, 6000, true)).toBe(-4200);
    const redrawn = layout.Cell({ cellKey: "r43", index: 0, item: rows[43]!, children: null } as never) as unknown as Element;
    (redrawn.props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 33, width: 320, height: 96 } } });
    (redrawn.props.ref as (node: unknown) => void)({ measure: (done: (...values: number[]) => void) => done(0, 33, 320, 96, 0, 33) });
    expect(layout.restate(next, false)).toBe(0);
    expect(moves).toHaveBeenLastCalledWith(33);
  });

  it("holds a present exact ID through reorders and subsequent height measurements", () => {
    const layout = createRowLayout();
    const rows = "abcdefghij".split("").map(result);
    for (const row of rows) measure(layout, row, 100);
    layout.capture(rows, false, 250, 300, 1000);
    const moved = [...rows.slice(0, 2), ...rows.slice(3, 8), rows[2]!, ...rows.slice(8)];
    const offset = 250 + layout.follow(rows, moved, false, 250, 300, 1000, true);
    expect(layout.layout(moved, 7, false).offset - offset).toBe(-50);
    measure(layout, rows[0], 130);
    const resized = offset + layout.restate(moved, false);
    expect(layout.layout(moved, 7, false).offset - resized).toBe(-50);
    const returned = resized + layout.follow(moved, rows, false, resized, 300, 1030, true);
    expect(layout.layout(rows, 2, false).offset - returned).toBe(-50);
  });

  it("keeps the reader at the top, at the end, or at the first row in view the change did not move", () => {
    const layout = createRowLayout();
    // Ten rows of 96 under no header; a 300 viewport at 200 shows c (192) to f (480).
    const rows = "abcdefghij".split("").map(result);
    const [a, b, c] = rows as [ReturnType<typeof result>, ReturnType<typeof result>, ReturnType<typeof result>];
    const x = result("x");
    // c remains present at the top: preserve c itself at its old viewport-relative offset.
    expect(layout.follow(rows, [c, a, b, ...rows.slice(3)], false, 200, 300, 960)).toBe(-192);
    // An arrival above: c moves down by its height, and the offset with it.
    expect(layout.follow(rows, [x, ...rows], false, 200, 300, 960)).toBe(96);
    // Every row in view left: the offset holds.
    expect(layout.follow(rows, [a, b, ...rows.slice(6)], false, 200, 300, 960)).toBe(0);
    expect(layout.follow(rows, [x, ...rows], false, 0, 300, 960)).toBe(0);
    expect(layout.follow(rows, [x, ...rows], false, 660, 300, 960)).toBe(96);
    expect(layout.follow([x, ...rows], rows, false, 756, 300, 1056)).toBe(-96);
    // A list too short to scroll is at its top.
    expect(layout.follow([a], [x, a], false, 0, 300, 96)).toBe(0);
    // A session the reader sees holds the place before a group heading above it: x arrives under the heading.
    const heading = { kind: "group", group: { key: "group:g" } } as never;
    expect(layout.follow([a, b, heading, ...rows.slice(2)], [a, b, heading, x, ...rows.slice(2)], false, 200, 300, 1056)).toBe(96);
  });

  it("restates the reader's place after a row or the header above it changes height", () => {
    const layout = createRowLayout();
    const rows = "abcdefghij".split("").map(result);
    for (const row of rows) measure(layout, row, 96);
    layout.capture(rows, true, 200, 300, 960);
    measure(layout, rows[0], 120);
    expect(layout.restate(rows, true)).toBe(24);
    layout.onHeaderLayout(layoutEvent(40) as never);
    expect(layout.restate(rows, true)).toBe(40);
    measure(layout, rows[9], 300);
    expect(layout.restate(rows, true)).toBe(0);
    layout.capture(rows, true, 0, 300, 960);
    measure(layout, rows[1], 200);
    expect(layout.restate(rows, true)).toBe(0);
  });

  it("holds the first session in view, not a group heading above it that changes height", () => {
    const layout = createRowLayout();
    const heading = { kind: "group", group: { key: "group:g" } } as never;
    const rows = [result("a"), heading, ..."bcdefghij".split("").map(result)];
    for (const row of rows) measure(layout, row, 96);
    // At 150 the heading (96 to 192) shows first, and b (192 to 288) under it.
    layout.capture(rows, false, 150, 300, 1056);
    measure(layout, heading, 120);
    expect(layout.restate(rows, false)).toBe(24);
  });

  it("keeps the reader's own session through a change while a smaller viewport shows only headings", () => {
    const layout = createRowLayout();
    const heading = (key: string) => ({ kind: "group", group: { key } }) as never;
    const top = heading("group:g1");
    const rows = [top, heading("group:g2"), ..."abcdefgh".split("").map(result)];
    // At 10 the headings show first, and a (192 to 288) under them.
    layout.capture(rows, false, 10, 300, 960);
    // A notice shrinks the viewport to 56, which shows only the first heading, and a row arrives above a.
    expect(layout.follow(rows, [top, result("x"), ...rows.slice(1)], false, 10, 56, 1056, true)).toBe(96);
  });

  it("holds the session the reader sees once a row leaving lets it into view", () => {
    const layout = createRowLayout();
    const rows = "abcdefghij".split("").map(result);
    const left = [rows[0]!, ...rows.slice(2)];
    // At 150, b (96 to 192) shows first. It leaves: c keeps its distance, which brings a into view at 54.
    layout.capture(rows, false, 150, 300, 960);
    expect(layout.follow(rows, left, false, 150, 300, 960, true)).toBe(-96);
    // a is now the first session the reader sees: a row arriving under it moves c, not a.
    expect(layout.follow(left, [rows[0]!, result("x"), ...rows.slice(2)], false, 54, 300, 864, true)).toBe(0);
  });

  it("keeps the reader's session, not a row the change moved or redrew into view above it, as that row is measured again", () => {
    const heading = { kind: "group", group: { key: "group:g" } } as never;
    const rows = [result("a"), result("b"), heading, ..."cdefghij".split("").map(result)];
    // At 200 the heading (192 to 288) shows first, and c (288 to 384) under it. j re-sorts to just above c, or d,
    // redrawn in a new state, swaps with c: either way c keeps its distance, which brings that row into view above it.
    for (const [moved, next] of [[rows[10]!, [...rows.slice(0, 3), rows[10]!, ...rows.slice(3, 10)]], [result("d"), [...rows.slice(0, 3), result("d"), rows[3]!, ...rows.slice(5)]]] as const) {
      const layout = createRowLayout();
      for (const row of rows) measure(layout, row, 96);
      layout.capture(rows, false, 200, 300, 1056);
      expect(layout.follow(rows, next, false, 200, 300, 1056, true)).toBe(96);
      // Its new state lays it out shorter: c, the reader's session, keeps its place.
      measure(layout, moved, 60);
      expect(layout.restate(next, false)).toBe(-36);
    }
  });

  it("reads a place the screen restored against its rows, not a content height the list has yet to report", () => {
    const layout = createRowLayout();
    const rows = "abcdefghij".split("").map(result);
    // Back restores 400 while the list still reports the peek's shorter content.
    layout.capture(rows, false, 400, 300, 500);
    expect(layout.atEnd()).toBe(false);
    layout.capture(rows, false, 660, 300, 500);
    expect(layout.atEnd()).toBe(true);
  });

  it("the screen lays its list out by measured row and header", () => {
    patchSessionView("host-a", { status: "all", layout: "flat" });
    mount();
    const layout = () => list().props.getItemLayout as (data: unknown, index: number) => { offset: number; length: number };
    const cellOf = (index: number) => ({ cellKey: "cell", index, item: (list().props.data as unknown[])[index], style: undefined, children: null });
    const Cell = list().props.CellRendererComponent as (props: ReturnType<typeof cellOf>) => Element;
    for (let index = 0; index < 60; index += 1) (Cell(cellOf(index)).props.onLayout as (event: unknown) => void)(layoutEvent(index === 31 ? 150 : 100));
    ((list().props.ListHeaderComponent as Element).props.onLayout as (event: unknown) => void)(layoutEvent(30));
    expect(ids()[31]).toBe("s31");
    expect(layout()(list().props.data, 40)).toEqual({ index: 40, length: 100, offset: 30 + 40 * 100 + 50 });
    liveUpdate("s31");
    expect(ids()[0]).toBe("s31");
    expect(layout()(list().props.data, 0)).toEqual({ index: 0, length: 150, offset: 30 });
    expect(layout()(list().props.data, 40)).toEqual({ index: 40, length: 100, offset: 30 + 40 * 100 + 50 });
  });
});
