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
    fake: { state: null as unknown, inputs: [] as Array<{ hostId: string; includeArchived: boolean; enabled?: boolean }>, refresh: vi.fn(), hostStatus: "online",
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
  PanResponder: { create: () => ({ panHandlers: {} }) }, Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View",
}));
vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: (contract: { name: string }) => (input: Record<string, unknown>) => { const handler = (fake as unknown as { rpc?: Record<string, (input: Record<string, unknown>) => Promise<unknown>> }).rpc?.[contract.name]; return handler ? handler(input) : Promise.reject(new Error("Unavailable in this fixture")); },
  getPaseoClient: () => { throw new Error("No host client in this fixture"); },
  useHosts: () => [{ serverId: "host-a", status: fake.hostStatus }],
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ FlatList: "FlatList", Icon: "Icon", ScrollView: "ScrollView", TextInput: "TextInput", useToast: () => fake.toast }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: (input: { hostId: string; includeArchived: boolean; enabled?: boolean }) => { fake.inputs.push(input); return { state: fake.state, refresh: fake.refresh }; } }));

import { SessionDetailsButton, SessionsScreen } from "../client/sessions-screen";
import type { DirectoryState } from "../client/session-directory";
import { clearSessionViews, patchSessionView } from "../client/session-view-store";
import { deriveSessionProjection, MISSING_OWNER_ADAPTER, type SessionProjection, type SessionProjectionInput } from "../shared/session-lifecycle";
import type { SessionRetirementRead } from "../client/use-session-lifecycle";
import { SessionRetirementConflict, SessionRetirementDetails } from "../client/session-lifecycle-view";
import { sessionEntry } from "./session-fixtures";

type Element = { readonly type?: unknown; readonly props: Record<string, unknown> };
const walk = (node: unknown): Element[] => {
  if (node === null || node === undefined || typeof node !== "object") return [];
  if (Array.isArray(node)) return node.flatMap(walk);
  const element = node as Element;
  return [element, ...walk(element.props.children), ...walk(element.props.ListHeaderComponent)];
};
const all = () => walk(harness.flush());
const byTestID = (testID: string) => all().filter((element) => element.props.testID === testID);
const textOf = (value: unknown): string => {
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (Array.isArray(value)) return value.map(textOf).join(" ");
  if (value === null || value === undefined || typeof value !== "object") return "";
  const element = value as Element;
  return textOf(element.props.children);
};
/** Retirement components are pure; expand them without invoking a screen hook. */
const pure = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(pure);
  if (value === null || value === undefined || typeof value !== "object") return value;
  const element = value as Element & { type?: unknown };
  if (typeof element.type === "function") return pure((element.type as (props: Record<string, unknown>) => unknown)(element.props));
  return { ...element, props: { ...element.props, children: pure(element.props.children) } };
};
const pureText = (value: unknown): string => textOf(pure(value));
const colors = { accent: "#05f", accentForeground: "#fff", border: "#ccc", foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#f4f4f4", surface2: "#eee", statusDanger: "#c00", statusSuccess: "#080", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
const coverage = { archiveInventory: "complete" as const, observedAt: "2026-10-24T08:00:02.000Z", detail: "complete fixture inventory" };
const release = { state: "unavailable" as const, reason: MISSING_OWNER_ADAPTER };
const projectionInput = (id: string, patch: Partial<SessionProjectionInput["session"]> = {}): SessionProjectionInput => ({
  hostId: "host-a", coverage,
  intent: { state: "none", reviewId: null, sessionId: id, observedAt: null, detail: "No unresolved archive receipt." },
  release,
  session: { id, status: "idle", archived: false, archivedAt: null, activeTurn: false, pendingInteraction: false, title: `Session ${id}`, workspaceId: "wks", projectKey: "project", provider: "fixture", model: null,
    updatedAt: "2026-10-24T08:00:00.000Z", observedAt: "2026-10-24T08:00:01.000Z", ...patch },
  now: "2026-10-24T08:05:00.000Z",
});
const ordinaryProjection = deriveSessionProjection(projectionInput("ordinary"));
const retiredProjection = deriveSessionProjection(projectionInput("retired", { status: "running", archived: true, archivedAt: "2026-10-24T08:02:00.000Z", activeTurn: true }));
const retirement = (projections: readonly SessionProjection[]): SessionRetirementRead => ({
  projections: new Map(projections.map((projection) => [projection.sessionId, projection])),
  receipts: { state: "ready", complete: true, error: null, readAt: "2026-10-24T08:00:03.000Z" },
  reload: vi.fn(),
});

const ready = (entries: DirectoryState["entries"] = [sessionEntry("ordinary"), sessionEntry("retired", { status: "running", archivedAt: "2026-10-24T08:02:00.000Z", activeTurn: true })], patch: Partial<DirectoryState> = {}): DirectoryState =>
  ({ entries, status: "ready", live: true, error: null, staleReason: null, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, includeArchived: true, ...patch });
function renderScreen(props: Parameters<typeof SessionsScreen>[0]) {
  harness.render(() => SessionsScreen(props));
  const list = byTestID("sessions-list")[0];
  if (list) (list.props.ref as { current: unknown }).current = { scrollToOffset: () => undefined, scrollToIndex: () => undefined };
}
function mount(retirementValue: SessionRetirementRead | undefined = retirement([ordinaryProjection, retiredProjection]), propsPatch: Partial<Parameters<typeof SessionsScreen>[0]> = {}) {
  renderScreen({ host: { id: "host-a", label: "Fixture host" }, layout: { compact: false, platform: "web" },
    navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {}, retirement: retirementValue, ...propsPatch } as Parameters<typeof SessionsScreen>[0]);
}
function mountWithoutRetirement() {
  renderScreen({ host: { id: "host-a", label: "Fixture host" }, layout: { compact: false, platform: "web" },
    navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {} } as Parameters<typeof SessionsScreen>[0]);
}
const list = () => byTestID("sessions-list")[0]!;
const row = (id: string) => {
  const data = list().props.data as Array<{ kind: string; entry?: { id: string } }>;
  const item = data.find((candidate) => candidate.entry?.id === id)!;
  return (list().props.renderItem as (info: { item: unknown }) => Element)({ item });
};
const rowFeedback = (id: string): Element => row(id).props.retirementFeedback as Element;
const openRowDetails = (id: string) => { const details = row(id).props.details as Element; (details.props.onPress as () => void)(); harness.flush(); };
const press = (testID: string) => { const found = byTestID(testID)[0]!; (found.props.onPress as () => void)(); harness.flush(); };

beforeEach(() => { clearSessionViews(); patchSessionView("host-a", { query: "", status: "all", membership: "sessions", includeArchived: false, layout: "flat", grouping: "flat", advancedOpen: true, toolbarOpen: true }); fake.state = ready(); fake.inputs = []; fake.hostStatus = "online"; });
afterEach(() => harness.unmount());

describe("Sessions retirement integration", () => {
  it("draws the disposition on the row and the read-only evidence in details", () => {
    mount();
    const feedback = rowFeedback("ordinary");
    expect(pureText(feedback)).toContain("Retirement: Unknown");
    expect(pureText(feedback)).toContain("Native idle · Not archived");
    openRowDetails("ordinary");
    const panel = byTestID("sessions-details")[0]!;
    expect(panel).toBeTruthy();
    const retirementDetails = all().find((element) => element.type === SessionRetirementDetails)!;
    expect(retirementDetails.props.projection).toMatchObject({ sessionId: "ordinary" });
    const text = pureText(retirementDetails);
    expect(text).toContain("Retirement evidence");
    expect(text).toContain("Missing evidence");
    expect(text).toContain("Evidence sources");
    expect(text).toContain("native · native idle; not archived");
    // The local fallback requests the full inventory once, even for ordinary membership.
    expect(fake.inputs.at(-1)?.includeArchived).toBe(true);
  });

  it("keeps an archived conflict reachable from an ordinary list and opens its exact details", () => {
    mount();
    expect(byTestID("sessions-row-retired")).toHaveLength(0);
    const index = byTestID("sessions-retirement-conflicts")[0]!;
    expect(textOf(index)).toContain("Archived conflicts");
    const conflict = byTestID("sessions-retirement-conflict-retired")[0]!;
    expect(textOf(conflict)).toContain("retired · native running");
    const details = all().find((element) => element.type === SessionDetailsButton && element.props.title === "retired")!;
    (details.props.onPress as () => void)(); harness.flush();
    const panel = byTestID("sessions-details")[0]!;
    expect(panel).toBeTruthy();
    const retirementDetails = all().find((element) => element.type === SessionRetirementDetails)!;
    expect(retirementDetails.props.projection).toMatchObject({ sessionId: "retired" });
    const text = pureText(retirementDetails);
    expect(text).toContain("Retirement evidence");
    expect(text).toContain("Retain");
    expect(text).toContain("Native status running");
    expect(text).toContain("Archive membership Archived");
    expect(text).toContain("archived-with-active-execution");
  });

  it("keeps the archived conflict reachable when the Archived search is empty", () => {
    patchSessionView("host-a", { query: "nothing-matches", membership: "archived", includeArchived: true, status: "all" });
    mount();
    expect(list().props.data).toHaveLength(0);
    expect(byTestID("sessions-retirement-conflicts")).toHaveLength(1);
    const conflict = byTestID("sessions-retirement-conflict-retired")[0]!;
    expect(textOf(conflict)).toContain("retired · native running");
    const details = all().find((element) => element.type === SessionDetailsButton && element.props.title === "retired")!;
    (details.props.onPress as () => void)(); harness.flush();
    const retirementDetails = all().find((element) => element.type === SessionRetirementDetails)!;
    const text = pureText(retirementDetails);
    expect(text).toContain("Native status running");
    expect(text).toContain("Archive membership Archived");
    expect(text).toContain("archived-with-active-execution");
  });

  it("keeps the archived conflict reachable across a nonmatching project scope", () => {
    mount(retirement([ordinaryProjection, retiredProjection]), { projectId: "another-project" });
    expect(list().props.data).toHaveLength(0);
    expect(byTestID("sessions-retirement-conflict-retired")).toHaveLength(1);
  });

  it("keeps the archived conflict reachable across a nonmatching epic scope", () => {
    mount(retirement([ordinaryProjection, retiredProjection]), { epicKey: "ASMA-OTHER" });
    expect(list().props.data).toHaveLength(0);
    expect(byTestID("sessions-retirement-conflict-retired")).toHaveLength(1);
  });

  it("does not show a conflict index for an unarchived projection or without a retirement read", () => {
    const unarchivedConflict = deriveSessionProjection({
      ...projectionInput("ordinary"),
      evidenceConflicts: [{ kind: "archived-with-active-execution", detail: "Synthetic evidence conflict for an unarchived session.", sources: [] }],
    });
    mount(retirement([unarchivedConflict]));
    expect(byTestID("sessions-retirement-conflicts")).toHaveLength(0);
    harness.unmount();
    mountWithoutRetirement();
    expect(byTestID("sessions-retirement-conflicts")).toHaveLength(0);
  });

  it("counts ordinary matches from the archive-inclusive read and says excluding archived", () => {
    fake.state = ready([sessionEntry("ordinary"), sessionEntry("retired", { status: "idle", archivedAt: "2026-10-18T11:00:00Z" })]);
    patchSessionView("host-a", { query: "ordinary", status: "all", membership: "sessions", includeArchived: false });
    mount();
    const header = byTestID("sessions-applied-filters")[0]!;
    expect(String(header.props.accessibilityLabel)).toContain("1 matching session");
    const inventory = byTestID("sessions-options-count")[0]!;
    expect(pureText(inventory)).toMatch(/^Inventory: 1 session in this host context, excluding archived/);
    expect(fake.inputs.at(-1)?.includeArchived).toBe(true);
  });
});
