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
  useCallback: harness.useCallback, useEffect: harness.useEffect, useLayoutEffect: harness.useEffect, useMemo: harness.useMemo, useRef: harness.useRef, useState: harness.useState }));
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

import { ClearFiltersButton, matchingCount, SessionsScreen } from "../client/sessions-screen";
import type { DirectoryState } from "../client/session-directory";
import { MobileFiltersPage } from "../client/mobile-filters";
import { MobileQuickToolbar } from "../client/mobile-quick-toolbar";
import { SessionLayoutControl } from "../client/session-layout-control";
import { DEFAULT_RECOVERY_TARGET, TAKEOVER_LIMIT, type LifecycleRow } from "../shared/session-lifecycle";
import { sessionEntry } from "./session-fixtures";
import { clearSessionViews, patchSessionView } from "../client/session-view-store";

type Element = { readonly type?: unknown; readonly props: Record<string, unknown> };
const walk = (node: unknown): Element[] => {
  if (node === null || node === undefined || typeof node !== "object") return [];
  if (Array.isArray(node)) return node.flatMap(walk);
  const element = node as Element;
  return [element, ...walk(element.props.children), ...walk(element.props.ListHeaderComponent)];
};
const all = () => walk(harness.flush());
const byTestID = (testID: string) => all().filter((element) => element.props.testID === testID);
const colors = { accent: "#05f", accentForeground: "#fff", border: "#ccc", foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#f4f4f4", surface2: "#eee", statusDanger: "#c00", statusSuccess: "#080", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
// Lookalike titles and every native state the Current filter has to tell apart.
const ENTRIES = [
  sessionEntry("alpha-idle", { title: "Alpha review", status: "idle" }),
  sessionEntry("beta-running", { title: "Beta build", status: "running" }),
  sessionEntry("alpha-closed", { title: "Alpha closed", status: "closed" }),
  sessionEntry("delta-error", { title: "Delta", status: "error" }),
  sessionEntry("alpha-archived", { title: "Alpha archived", status: "closed", archivedAt: "2026-10-20T10:00:00.000Z" }),
  sessionEntry("gamma-input", { title: "Gamma", status: "running", pendingPermissionCount: 1 }),
];
const ready = (patch: Partial<DirectoryState> = {}): DirectoryState => ({ entries: ENTRIES, status: "ready", live: true, error: null, staleReason: null, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, includeArchived: false, ...patch });

function mount() {
  harness.render(() => SessionsScreen({ host: { id: "host-a", label: "Fixture host" }, layout: { compact: true, platform: "ios" },
    navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {} } as Parameters<typeof SessionsScreen>[0]));
  const list = byTestID("sessions-list")[0];
  if (list) (list.props.ref as { current: unknown }).current = { scrollToOffset: () => undefined, scrollToIndex: () => undefined };
}
const open = () => { (all().find((element) => element.type === MobileQuickToolbar)!.props.onFilters as () => void)(); harness.flush(); };
const back = () => { (all().find((element) => element.type === MobileFiltersPage)!.props.onBack as () => void)(); harness.flush(); };
const press = (testID: string) => { (byTestID(testID)[0]!.props.onPress as () => void)(); harness.flush(); };
const layout = (value: "grouped" | "flat") => { (all().find((element) => element.type === SessionLayoutControl)!.props.onChange as (layout: string) => void)(value); harness.flush(); };
const search = (query: string) => { (byTestID("sessions-search")[0]!.props.onChangeText as (text: string) => void)(query); harness.flush(); };
// The page draws this beside its heading (mobile-filters.test.tsx); the screen only derives it.
const header = () => (all().find((element) => element.type === MobileFiltersPage)?.props.count ?? null) as { text: string; label: string } | null;
type ListRow = { kind: string; entry?: { id: string } };
const listedIds = () => (byTestID("sessions-list")[0]!.props.data as ListRow[]).flatMap((row) => row.entry && (row.kind === "session" || row.kind === "result") ? [row.entry.id] : []);
const settle = async () => { for (let n = 0; n < 10; n++) await new Promise((resolve) => setTimeout(resolve, 0)); harness.flush(); };
const row = (id: string, patch: Partial<LifecycleRow> = {}) => ({ id, title: `Session ${id}`, workspaceId: null, cwd: "/fixture", provider: "codex", model: null, effort: null, mode: null, nativeStatus: "idle", fingerprint: `fp-${id}`,
  category: "candidate", eligible: true, reason: "Fixture.", children: [], outcome: "pending", message: null, ...patch }) as LifecycleRow;
const review = (ids: string[]) => ({ id: "00000000-0000-4000-8000-000000000001", createdAt: "2026-10-22", action: "archive", scope: "filtered", target: DEFAULT_RECOVERY_TARGET, rows: ids.map((id) => row(id)), limitation: TAKEOVER_LIMIT });
/** The header names exactly the sessions the flat list draws after Back. */
function expectCount(ids: readonly string[]) {
  expect(header()).toEqual({ text: `${ids.length} session${ids.length === 1 ? "" : "s"}`, label: `${ids.length} matching session${ids.length === 1 ? "" : "s"}` });
  back(); expect(listedIds().sort()).toEqual([...ids].sort()); open();
}

beforeEach(() => { clearSessionViews(); patchSessionView("host-a", { layout: "flat" }); fake.state = ready(); fake.hostStatus = "online"; fake.refresh = vi.fn(); });
afterEach(() => harness.unmount());

describe("mobile Filters & view matching count", () => {
  it("counts the final matching sessions the list draws and follows each in-page change, Clear and Reset", () => {
    mount(); open();
    expectCount(["alpha-idle", "beta-running", "gamma-input"]);
    press("sessions-filter-all");
    expectCount(["alpha-idle", "beta-running", "alpha-closed", "delta-error", "gamma-input"]);
    search("alpha");
    expectCount(["alpha-idle", "alpha-closed"]);
    press("sessions-filter-error");
    // A complete, live read with nothing left is a confirmed 0.
    expect(header()).toEqual({ text: "0 sessions", label: "0 matching sessions" });
    (all().find((element) => element.type === ClearFiltersButton)!.props.onPress as () => void)(); harness.flush();
    expectCount(["alpha-idle", "beta-running", "alpha-closed", "delta-error", "gamma-input"]);
    press("sessions-reset-view");
    // Reset returns to collapsed Grouped: the list draws group rows, the count still names the sessions.
    expect(header()).toEqual({ text: "3 sessions", label: "3 matching sessions" });
    back(); expect(listedIds()).toEqual([]);
    layout("flat"); open();
    expectCount(["alpha-idle", "beta-running", "gamma-input"]);
  });

  it("keeps the count when only layout, sort or group disclosure changes", () => {
    mount(); open(); press("sessions-filter-all"); search("a");
    const before = header();
    expect(before?.text).toBe("5 sessions");
    back(); layout("grouped");
    open(); expect(header()).toEqual(before);
    press("sessions-sort");
    (all().find((element) => element.props.label === "Title")!.props.onPress as () => void)(); harness.flush();
    expect(header()).toEqual(before);
  });

  it.each<[string, Partial<DirectoryState>, string, string]>([
    ["nothing read yet", { entries: [], status: "loading", live: false, lastUpdatedAt: null, inventoryComplete: false }, "…", "Reading matching sessions"],
    ["a failed first read", { entries: [], status: "error", live: false, lastUpdatedAt: null, inventoryComplete: false }, "—", "Matching sessions unavailable: the session list could not be read"],
    ["a partial live read", { inventoryComplete: false }, "3 sessions · partial", "3 sessions match a partial read; the count may be incomplete"],
    ["a stale read", { status: "stale", live: false }, "3 sessions · stale", `3 sessions matched the read at ${new Date(0).toLocaleTimeString()}; Stale`],
    ["a ready read without its live stream", { live: false }, "3 sessions · stale", `3 sessions matched the read at ${new Date(0).toLocaleTimeString()}; Stale`],
    ["a stale read with no current match", { status: "stale", live: false, entries: ENTRIES.filter((entry) => entry.status === "closed") }, "…", "Matching sessions updating: Stale"],
  ])("qualifies %s instead of claiming an exact or empty count", (_, patch, text, label) => {
    mount(); open();
    fake.state = ready(patch);
    expect(header()).toEqual({ text, label });
  });

  it("names the host state when the host is offline, and treats a read at time 0 as read", () => {
    fake.hostStatus = "offline";
    mount(); open();
    expect(header()?.text).toBe("3 sessions · host offline");
    fake.hostStatus = "online";
    expect(header()?.text).toBe("3 sessions");
  });

  it("shows Reading, never 0, while the Archived read has not replaced the ordinary one", () => {
    mount(); open(); press("sessions-filter-all");
    press("sessions-advanced-archived");
    expect(header()).toEqual({ text: "…", label: "Reading matching sessions" });
    fake.state = ready({ includeArchived: true });
    expectCount(["alpha-archived"]);
  });

  it("drops an archived session at once from a live, complete read without walking the directory again", async () => {
    fake.rpc = {
      "sessions.lifecycle-review": async () => null, "sessions.lifecycle-preview": async (input) => review(input.agentIds as string[]),
      "sessions.lifecycle-execute-item": async (input) => row(input.agentId as string, { outcome: "archived", runtime: "closed", message: "Archived.",
        availability: { agentId: input.agentId as string, state: "readable", reason: "Fixture read.", checkedAt: new Date().toISOString(), revision: "2026-10-22", archivedAt: "2026-10-22", nativeStatus: "closed" } }),
      "sessions.lifecycle-dispositions": async () => ({}), "sessions.availability": async () => ({}),
    };
    mount(); await settle();
    const list = byTestID("sessions-list")[0]!, item = (list.props.data as ListRow[]).find((candidate) => candidate.entry?.id === "beta-running")!;
    (((list.props.renderItem as (info: { item: unknown }) => Element)({ item }).props.lifecycleAction as Element).props.onPress as () => void)(); await settle();
    open();
    expectCount(["alpha-idle", "gamma-input"]);
    expect(fake.refresh).not.toHaveBeenCalled();
  });

  it("says Not evaluated, never 0 or the host total, while an assignment filter cannot be evaluated", () => {
    patchSessionView("host-a", { assignment: "me" });
    mount(); open();
    expect(header()).toEqual({ text: "Not evaluated", label: "Matching sessions not evaluated: Assigned to me is not evaluated" });
  });
});

describe("matchingCount", () => {
  it("is exact only for a live, complete read of the membership in view", () => {
    const count = (patch: Partial<DirectoryState>, includeArchived = false, freshness = "Live") => matchingCount({ count: 1, state: ready(patch), includeArchived, freshness, blocked: null }).text;
    expect(count({})).toBe("1 session");
    expect(count({}, true)).toBe("…");
    expect(count({ includeArchived: true, status: "error", lastUpdatedAt: null })).toBe("…");
    expect(count({ inventoryComplete: undefined })).toBe("1 session · partial");
    expect(count({}, false, "Reconnecting")).toBe("1 session · reconnecting");
  });
});
