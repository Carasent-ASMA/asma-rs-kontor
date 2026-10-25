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
    fake: { rpc: {} as Record<string, (input: Record<string, unknown>) => Promise<unknown>>, refresh: vi.fn(), state: null as unknown, hostStatus: "online",
      toast: { show: vi.fn(), error: vi.fn() } },
  };
});
vi.mock("react", async (importOriginal) => ({ ...(await importOriginal<typeof import("react")>()), useContext: () => null, memo: (component: unknown) => component,
  useCallback: harness.useCallback, useEffect: harness.useEffect, useLayoutEffect: harness.useEffect, useMemo: harness.useMemo, useRef: harness.useRef, useState: harness.useState,
  useSyncExternalStore: (_subscribe: unknown, read: () => unknown) => read() }));
vi.mock("react-native", () => ({
  AccessibilityInfo: { addEventListener: () => ({ remove: () => undefined }), isReduceMotionEnabled: async () => false, isScreenReaderEnabled: async () => false, setAccessibilityFocus: () => undefined },
  ActivityIndicator: "ActivityIndicator", Animated: { Value: class { setValue() {} stopAnimation() {} }, View: "Animated.View", timing: () => ({ start: (done?: () => void) => done?.(), stop: () => undefined }) },
  BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, findNodeHandle: () => null,
  PanResponder: { create: () => ({ panHandlers: {} }) }, Platform: { OS: "ios" }, Pressable: "Pressable", Text: "Text", View: "View",
}));
vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: (contract: { name: string }) => (input: Record<string, unknown>) => { const handler = fake.rpc[contract.name]; return handler ? handler(input) : Promise.reject(new Error("Unavailable in this fixture")); },
  getPaseoClient: () => { throw new Error("No host client in this fixture"); },
  useHosts: () => [{ serverId: "host-a", status: fake.hostStatus }],
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ FlatList: "FlatList", Icon: "Icon", ScrollView: "ScrollView", TextInput: "TextInput", useToast: () => fake.toast }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: fake.state, refresh: fake.refresh }) }));

import { CompactSearch, MobileQuickToolbar } from "../client/mobile-quick-toolbar";
import { SessionLayoutControl } from "../client/session-layout-control";
import { SwitchRowItem } from "../client/start-switch-sheet";
import { RecentFooter } from "../client/recent-visits";
import { SessionsScreen } from "../client/sessions-screen";
import type { DirectoryState } from "../client/session-directory";
import { clearSessionViews, patchSessionView, readSessionView } from "../client/session-view-store";
import { clearRecent, configureRecentStorage, readRecent, recordVisit } from "../client/recent-sessions";
import { sessionEntry } from "./session-fixtures";

type Element = { readonly type?: unknown; readonly props: Record<string, unknown> };
/** The list's header, empty state and footer too; the hook-free Recent footer is drawn so its text is visible. */
const walk = (node: unknown): Element[] => {
  if (node === null || node === undefined || typeof node !== "object") return [];
  if (Array.isArray(node)) return node.flatMap(walk);
  const element = node as Element;
  const drawn = element.type === RecentFooter ? RecentFooter(element.props as Parameters<typeof RecentFooter>[0]) : null;
  return [element, ...walk(element.props.children), ...walk(element.props.ListHeaderComponent), ...walk(element.props.ListEmptyComponent), ...walk(element.props.ListFooterComponent), ...walk(drawn)];
};
const all = () => walk(harness.flush());
const byTestID = (testID: string) => all().filter((element) => element.props.testID === testID);
const textOf = (node: unknown): string => walk(node).filter((element) => element.type === "Text").map((element) => [element.props.children].flat(Infinity).join("")).join("\n");
const colors = { accent: "#05f", accentForeground: "#fff", border: "#ccc", foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#f4f4f4", surface2: "#eee", statusDanger: "#c00", statusSuccess: "#080", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
// Twelve ordinary sessions, one in error, one archived; more than the sheet's eight.
const ENTRIES = [
  ...Array.from({ length: 12 }, (_, index) => sessionEntry(`s-${index}`, { title: `Session ${index}`, status: "idle" })),
  sessionEntry("delta-error", { title: "Delta", status: "error" }),
  sessionEntry("alpha-archived", { title: "Alpha archived", status: "closed", archivedAt: "2026-10-20T10:00:00.000Z" }),
];
const ready = (patch: Partial<DirectoryState> = {}): DirectoryState => ({ entries: ENTRIES, status: "ready", live: true, error: null, staleReason: null, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, includeArchived: true, ...patch });
const opened: string[] = [];

function mount(compact = false) {
  harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Fixture host" }, layout: { compact, platform: compact ? "ios" : "web" },
    navigation: { openAgent: ({ agentId }: { agentId: string }) => { opened.push(agentId); }, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {} } as Parameters<typeof SessionsScreen>[0]));
  const list = byTestID("sessions-list")[0];
  if (list) (list.props.ref as { current: unknown }).current = { scrollToOffset: () => undefined, scrollToIndex: () => undefined };
}
const press = (testID: string) => { (byTestID(testID)[0]!.props.onPress as () => void)(); harness.flush(); };
type ListRow = { kind: string; entry?: { id: string } };
const listedIds = () => (byTestID("sessions-list")[0]!.props.data as ListRow[]).flatMap((row) => row.entry && (row.kind === "session" || row.kind === "result") ? [row.entry.id] : []);

beforeEach(() => { clearSessionViews(); configureRecentStorage(null); fake.state = ready(); fake.hostStatus = "online"; opened.length = 0; });
afterEach(() => harness.unmount());

describe("Recent in Sessions", () => {
  it("lands on Recent and lists every kept visit in open order, whatever its state, in one list that keeps the saved layout", () => {
    const order = ["s-3", "missing", "delta-error", ...Array.from({ length: 12 }, (_, index) => `s-${index}`).filter((id) => id !== "s-3"), "alpha-archived"];
    [...order].reverse().forEach((id, index) => recordVisit("host-a", id, 1_000 + index));
    mount();
    expect(readSessionView("host-a").status).toBe("recent");
    expect(listedIds()).toEqual(order.filter((id) => id !== "missing" && id !== "alpha-archived"));
    const layout = all().find((element) => element.type === SessionLayoutControl)!;
    expect(layout.props.disabled).toMatch(/^Recent lists sessions in the order you opened them/);
    expect(textOf(byTestID("sessions-recent-account")[0])).toBe("Recent keeps the last 500 sessions you opened from Kontor on this host and device, with no age limit: 15 kept · 13 shown · 1 hidden by search, scope or filters · 1 not found in this host's current list. Native sidebar opens and other devices are not included.");
    harness.unmount(); patchSessionView("host-a", { sort: "title" }); mount();
    expect(textOf(byTestID("sessions-applied-filters")[0])).toContain("not open order");
    expect(listedIds()[0]).toBe("delta-error");
  });

  it("stays on an empty or cleared Recent, offering Current and All instead of falling back", () => {
    mount();
    expect(listedIds()).toEqual([]);
    expect(textOf(byTestID("sessions-empty")[0])).toContain("No recent sessions yet.");
    recordVisit("host-a", "s-1"); clearRecent("host-a", Date.parse("2026-10-25T10:00:00Z"));
    harness.flush();
    expect(textOf(byTestID("sessions-empty")[0])).toContain("Recent was cleared on this device at");
    expect(readSessionView("host-a").status).toBe("recent");
    press("sessions-empty-current");
    expect(readSessionView("host-a").status).toBe("current");
    expect((byTestID("sessions-list")[0]!.props.data as ListRow[]).length).toBeGreaterThan(0);
    harness.unmount(); mount();
    // An explicit choice holds for this app session.
    expect(readSessionView("host-a").status).toBe("current");
  });

  it("lists saved entries from an earlier version apart, with no time or host, and opening one adds a real visit", () => {
    const legacy = JSON.stringify({ clearedAt: null, visits: [{ id: "s-5", at: 1 }, { id: "gone", at: 2 }] });
    const values = new Map([["kontor.recent@1", legacy]]);
    configureRecentStorage({ read: (key) => values.get(key), write: (key, value) => { if (value !== null) values.set(key, value); } });
    mount();
    expect(listedIds()).toEqual([]);
    const saved = byTestID("sessions-recent-saved")[0]!;
    expect(textOf(saved)).toContain("Saved entries · visit time and host unknown");
    expect(textOf(byTestID("sessions-recent-saved-absent")[0])).toBe("1 not found in this host's current list: gone");
    const row = walk(saved).find((element) => element.type === SwitchRowItem)!;
    expect(row.props).toMatchObject({ role: "button", row: { entry: expect.objectContaining({ id: "s-5" }), detail: expect.stringMatching(/^Saved entry · /) } });
    expect(String((row.props.row as { detail: string }).detail)).not.toMatch(/Opened|ago/);
    (row.props.onPress as () => void)(); harness.flush();
    expect(opened).toEqual(["s-5"]);
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(["s-5"]);
    expect(listedIds()).toEqual(["s-5"]);
    expect(walk(byTestID("sessions-recent-saved")[0]).some((element) => element.type === SwitchRowItem)).toBe(false);
    expect(values.get("kontor.recent@1")).toBe(legacy);
  });

  it("puts compact Search first in the row with Select and layout, and leaves it out of the quick filters", () => {
    mount(true);
    const controls = byTestID("sessions-view-controls")[0]!;
    const first = [controls.props.children].flat(Infinity).find((child) => child !== null && child !== false) as Element;
    expect(first.type).toBe(CompactSearch);
    expect(all().find((element) => element.type === MobileQuickToolbar)!.props).not.toHaveProperty("onQuery");
    (first.props.onQuery as (query: string) => void)("Session 1"); harness.flush();
    expect(readSessionView("host-a").query).toBe("Session 1");
  });
});
