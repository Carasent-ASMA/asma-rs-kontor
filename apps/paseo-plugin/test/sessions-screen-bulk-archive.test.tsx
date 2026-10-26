import type { PluginTheme } from "@getpaseo/plugin";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Slot = { initialized?: boolean; value?: unknown; deps?: readonly unknown[]; cleanup?: () => void };
type Queued = { readonly slot: Slot; readonly fn: () => void | (() => void); readonly deps?: readonly unknown[] };
const { harness, fake } = vi.hoisted(() => {
  let slots: Slot[] = [], cursor = 0, dirty = false, component: (() => unknown) | null = null, tree: unknown = null, effects: Queued[] = [];
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
    harness: { ...hooks, render(fn: () => unknown) { component = fn; return draw(); }, flush: draw, current: () => tree,
      /** Draws a child component on throwaway hook slots, so the screen's own slots stay its own. */
      child<T>(fn: () => T): T { const outer = { slots, cursor, effects }; slots = []; cursor = 0; effects = []; try { return fn(); } finally { ({ slots, cursor, effects } = outer); } },
      unmount() { for (const slot of slots) slot.cleanup?.(); slots.length = 0; component = null; tree = null; } },
    fake: { rpc: {} as Record<string, (input: Record<string, unknown>) => Promise<unknown>>, calls: [] as Array<[string, Record<string, unknown>]>,
      toast: { show: vi.fn(), error: vi.fn() }, refresh: vi.fn(), state: null as unknown, host: "host-a" },
  };
});
vi.mock("react", async (importOriginal) => ({ ...(await importOriginal<typeof import("react")>()), useContext: () => null, memo: (component: unknown) => component,
  useCallback: harness.useCallback, useEffect: harness.useEffect, useLayoutEffect: harness.useEffect, useMemo: harness.useMemo, useRef: harness.useRef, useState: harness.useState,
  useSyncExternalStore: (_subscribe: unknown, read: () => unknown) => read() }));
vi.mock("react-native", () => ({
  AccessibilityInfo: { addEventListener: () => ({ remove: () => undefined }), isReduceMotionEnabled: async () => false, isScreenReaderEnabled: async () => false },
  ActivityIndicator: "ActivityIndicator", Animated: { Value: class { setValue() {} stopAnimation() {} }, View: "Animated.View", timing: () => ({ start: (done?: () => void) => done?.(), stop: () => undefined }) },
  PanResponder: { create: () => ({ panHandlers: {} }) }, Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View",
}));
vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: (contract: { name: string }) => (input: Record<string, unknown>) => { fake.calls.push([contract.name, input]); const handler = fake.rpc[contract.name]; return handler ? handler(input) : Promise.reject(new Error("Unavailable in this fixture")); },
  getPaseoClient: () => { throw new Error("No host client in this fixture"); },
  useHosts: () => [{ serverId: "host-a", status: "online" }, { serverId: "host-b", status: "online" }],
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ FlatList: "FlatList", Icon: "Icon", ScrollView: "ScrollView", TextInput: "TextInput", useToast: () => fake.toast }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: fake.state, refresh: fake.refresh }) }));

import { SessionsScreen } from "../client/sessions-screen";
import { ArchiveRowFeedback, BulkArchiveDialog, BulkArchiveToolbar } from "../client/bulk-archive";
import { DenseSessionRow } from "../client/dense-session-row";
import { CompactSearch } from "../client/mobile-quick-toolbar";
import { SheetFooter } from "../client/operation-sheet";
import { Action } from "../client/page-controls";
import { SelectionBar, SelectionHeader, type SelectionControl } from "../client/session-selection";
import type { SessionAction } from "../client/session-actions-menu";
import { DEFAULT_RECOVERY_TARGET, type LifecycleRow } from "../shared/session-lifecycle";
import { clearSessionViews, patchSessionView } from "../client/session-view-store";
import { sessionEntry } from "./session-fixtures";

type Element = { type?: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk) : [node as Element, ...walk((node as Element).props.children), ...walk((node as Element).props.ListHeaderComponent)];
const all = () => walk(harness.flush());
const toolbar = () => all().find((element) => element.type === BulkArchiveToolbar)!;
const sheet = () => all().find((element) => element.type === BulkArchiveDialog);
/** The dialog drawn once, with its footer's buttons drawn in place. */
const dialog = () => {
  const tree = BulkArchiveDialog(sheet()!.props as Parameters<typeof BulkArchiveDialog>[0]) as Element;
  return [...walk(tree), ...walk(tree.props.header)].flatMap((element) => element.type === SheetFooter ? [element, ...walk(SheetFooter(element.props as Parameters<typeof SheetFooter>[0]))] : [element]);
};
/** Closes D4 from its own chrome, so the dialog reports the sessions its reviews saw archived. */
const closeReview = () => (dialog()[0]!.props.onClose as () => void)();
const pressIn = (tree: Element[], label: string | RegExp) => { const button = tree.find((element) => typeof element.props.label === "string" && (typeof label === "string" ? element.props.label === label : label.test(element.props.label)))!; void (button.props.onPress as () => unknown)(); };
const byId = (tree: Element[], testID: string) => tree.find((element) => element.props.testID === testID)!;
/** Presses D4's Archive N in one drawn dialog and returns that drawing, so a later Close belongs to the same dialog. */
const confirm = () => { const tree = dialog(); void (byId(tree, "bulk-archive-confirm").props.onPress as () => unknown)(); return tree; };
const summary = () => dialog().find((element) => element.props.testID === "bulk-archive-summary")?.props.children;
/** The toolbar's actions entry, where Resume stood; while selecting, board D1's header and bar stand in its place. */
const toggle = () => all().find((element) => (element.props.entry as { id?: string } | undefined)?.id === "toolbar");
const start = () => { void (toggle()!.props.actions as SessionAction[]).find((action) => action.key === "select")!.perform(); harness.flush(); };
const header = () => all().find((element) => element.type === SelectionHeader);
const headerText = () => walk(SelectionHeader(header()!.props as Parameters<typeof SelectionHeader>[0])).filter((element) => element.type === "Text").map((element) => String(element.props.children));
const selectAll = () => header()!.props.selectAll as SelectionControl & { checked: boolean };
const bar = () => all().find((element) => element.type === SelectionBar);
const archiveControl = () => bar()!.props.archive as SelectionControl;
/** The bar's Archive N, pressed while the bar allows it: it opens the review of the frozen selection. */
const openReview = () => { expect(archiveControl().unavailable).toBeNull(); archiveControl().onPress(); };
const list = () => all().find((element) => element.props.testID === "sessions-list")!;
const listed = () => (list().props.data as Array<{ entry?: { id: string } }>).flatMap((row) => row.entry ? [row.entry.id] : []);
const drawnRow = (id: string) => (list().props.renderItem as (input: unknown) => Element)({ item: (list().props.data as Array<{ entry?: { id: string } }>).find((row) => row.entry?.id === id) });
/** The screen's dense row for one session; its open-mode menu actions; its body drawn (the checkbox while selecting); its feedback lines. */
const dense = (id: string) => walk(drawnRow(id)).find((element) => element.type === DenseSessionRow)!;
const menu = (id: string) => ((dense(id).props.mode as { actions: Element }).actions.props.actions as SessionAction[]);
const body = (id: string) => { const element = dense(id); return walk(harness.child(() => (element.type as (props: object) => unknown)(element.props))).find((node) => node.props.testID === "session-row-body")!; };
const feedback = (id: string) => walk(drawnRow(id)).find((element) => element.type === ArchiveRowFeedback)!;
/** Taps each row's checkbox while selecting. */
const pick = (...ids: string[]) => { for (const id of ids) { (body(id).props.onPress as () => void)(); harness.flush(); } };
/** The listed rows whose checkbox is checked. */
const checked = () => listed().filter((id) => (dense(id).props.mode as { selected?: boolean }).selected === true);
const hold = (id: string) => { (body(id).props.onLongPress as () => void)(); harness.flush(); };
/** Types a search: the toolbar's field, or the compact search on a phone. */
const search = (query: string) => {
  const field = all().find((element) => element.props.testID === "sessions-search"), phone = all().find((element) => element.type === CompactSearch);
  ((field?.props.onChangeText ?? phone!.props.onQuery) as (value: string) => void)(query); harness.flush();
};
const settle = async () => { for (let n = 0; n < 8; n++) await new Promise((resolve) => setTimeout(resolve, 0)); harness.flush(); };
const colors = { accent: "#05f", accentForeground: "#fff", border: "#ccc", foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#f4f4f4", surface2: "#eee", statusDanger: "#c00", statusSuccess: "#080", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
const row = (id: string, patch: Partial<LifecycleRow> = {}): LifecycleRow => ({ id, title: `Session ${id}`, workspaceId: null, cwd: "/fixture", provider: "codex", model: null, effort: null, mode: null, nativeStatus: "idle", fingerprint: id,
  category: "candidate", eligible: true, reason: "Ready", children: [], outcome: "pending", message: null, ...patch });
const review = (ids: string[]) => ({ id: "00000000-0000-4000-8000-000000000001", createdAt: "2026-10-22", action: "archive", scope: "filtered", target: DEFAULT_RECOVERY_TARGET, rows: ids.map((id) => row(id)), limitation: "" });

describe.each(["full", "slim", "compact"] as const)("bulk archive on the %s Sessions screen", (layout) => {
  let executing: Array<(row: LifecycleRow) => void>;
  const mount = () => {
    harness.render(() => SessionsScreen({ host: { id: fake.host, label: "Fixture host" }, layout: { compact: layout === "compact", platform: layout === "compact" ? "ios" : "web" },
      theme: { colors } as PluginTheme, navigation: { openAgent: vi.fn(), openBrowser: vi.fn(), openWorkspace: vi.fn() }, params: {} } as Parameters<typeof SessionsScreen>[0]));
    (list().props.ref as { current: unknown }).current = { scrollToOffset() {}, scrollToIndex() {} };
    if (layout === "slim") {
      for (const [testID, height] of [["sessions-full-toolbar", 220], ["sessions-slim-toolbar-slot", 44]] as const) {
        const element = all().find((element) => element.props.testID === testID);
        (element?.props.onLayout as ((event: unknown) => void) | undefined)?.({ nativeEvent: { layout: { x: 0, y: 0, width: 1440, height } } });
      }
      (list().props.onScroll as (event: unknown) => void)({ nativeEvent: { contentOffset: { x: 0, y: 300 }, contentSize: { width: 1440, height: 2000 }, layoutMeasurement: { width: 1440, height: 600 } } }); harness.flush();
    }
  };
  beforeEach(() => {
    clearSessionViews({ status: "current" }); patchSessionView("host-a", { layout: "flat" }); fake.host = "host-a"; fake.calls = []; executing = [];
    fake.refresh = vi.fn(() => new Promise(() => {})); fake.toast.show.mockReset(); fake.toast.error.mockReset();
    fake.state = { entries: ["a", "b", "other"].map((id) => sessionEntry(id, { status: "idle" })), status: "ready", live: true, error: null, staleReason: null, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, includeArchived: false };
    fake.rpc = { "sessions.lifecycle-review": async () => null, "sessions.lifecycle-dispositions": async () => ({}),
      "sessions.lifecycle-preview": async (input) => review(input.agentIds as string[]),
      "sessions.lifecycle-execute-item": () => new Promise((resolve) => executing.push(resolve)),
    };
  });
  afterEach(() => harness.unmount());
  it("selects exact shown rows, confirms once, keeps pending/blocked rows and reports partial results truthfully", async () => {
    mount(); await settle();
    // The toolbar's actions entry stands where Resume stood, with the rows' Select sessions and no Resume.
    expect(toggle()!.props.actions).toEqual([expect.objectContaining({ key: "select", label: "Select sessions", unavailable: null })]);
    start(); pick("a", "b");
    expect(checked()).toEqual(["a", "b"]);
    // While selecting, each shown row is its own checkbox; D1's header counts them over every match and Select steps aside.
    expect(dense("a").props.mode).toEqual({ kind: "select", selected: true, unavailable: null, pending: false });
    expect(body("a").props).toMatchObject({ role: "checkbox", "aria-checked": true, accessibilityState: { checked: true, disabled: false, busy: false } });
    expect(body("other").props).toMatchObject({ role: "checkbox", "aria-checked": false });
    const mark = (id: string) => walk(walk(body(id)).find((node) => node.props.testID === "session-row-choice")).find((node) => node.type === "Icon")!.props.name;
    expect([mark("a"), mark("other")]).toEqual(["CircleCheck", "Circle"]);
    expect(toggle()).toBeUndefined(); expect(headerText()).toEqual(["2 / 3 selected", "Done"]); expect(bar()!.props.count).toBe(2);
    expect(all().some((element) => element.type === BulkArchiveToolbar)).toBe(false);
    openReview(); await settle();
    expect(sheet()).toBeDefined(); expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-preview").at(-1)?.[1]).toMatchObject({ action: "archive", scope: "filtered", agentIds: ["a", "b"] });
    expect(executing).toHaveLength(0); expect(listed()).toEqual(["a", "b", "other"]);
    confirm(); await settle();
    expect(dense("a").props.mode).toMatchObject({ kind: "select", selected: true, pending: true });
    expect(body("a").props.accessibilityState).toMatchObject({ checked: true, busy: true });
    expect(walk(body("a")).find((node) => node.type === "Icon")!.props.name).toBe("Loader");
    expect(listed()).toContain("a");
    executing.shift()!(row("a", { outcome: "archived", runtime: "closed", availability: { agentId: "a", state: "readable", checkedAt: new Date().toISOString(), revision: sessionEntry("a").updatedAt, reason: "Archived", archivedAt: "now" } })); await settle();
    expect(listed()).not.toContain("a"); expect(listed()).toEqual(expect.arrayContaining(["b", "other"]));
    executing.shift()!(row("b", { outcome: "blocked", message: "Unselected managed child" })); await settle();
    expect(listed()).toEqual(expect.arrayContaining(["b", "other"]));
    // The open dialog states the outcome once; no toast repeats it.
    expect(summary()).toBe("Archived 1 · 1 blocked"); expect(fake.toast.show).not.toHaveBeenCalled();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-execute-item").map(([, input]) => input.agentId)).toEqual(["a", "b"]);
    expect(fake.refresh).not.toHaveBeenCalled();
  });
  it("keeps a one-item bulk selection behind confirmation and returns focus to Archive N only after the bar re-enables it", async () => {
    mount(); await settle(); start(); pick("a");
    const focus = vi.fn(() => expect((walk(harness.current()).find((element) => element.type === SelectionBar)!.props.archive as SelectionControl).unavailable).toBeNull());
    (bar()!.props.archiveRef as { current: unknown }).current = { focus };
    openReview(); await settle();
    expect(sheet()!.props.anchor).toBe(bar()!.props.archiveRef);
    expect(archiveControl().unavailable).toBe("The archive review is open.");
    expect(executing).toHaveLength(0); closeReview(); expect(focus).not.toHaveBeenCalled();
    await settle(); expect(sheet()).toBeUndefined(); expect(executing).toHaveLength(0); expect(focus).toHaveBeenCalledTimes(1);
  });
  it.each([320, 360])("bounds pending bulk controls on a %spt short screen and keeps Stop and readbacks available", async (height) => {
    mount(); await settle();
    const root = harness.current() as Element, fallback = { focus: vi.fn() };
    (root.props.ref as { current: unknown }).current = fallback;
    (root.props.onLayout as (event: unknown) => void)({ nativeEvent: { layout: { x: 0, y: 0, width: layout === "compact" ? 390 : 1440, height } } });
    hold("a"); openReview(); await settle(); confirm(); await settle();
    closeReview(); await settle();
    expect(fallback.focus).toHaveBeenCalledTimes(1);
    const pending = BulkArchiveToolbar(toolbar().props as Parameters<typeof BulkArchiveToolbar>[0]);
    expect(pending.props.style).toMatchObject({ flexGrow: 0, flexShrink: 0, maxHeight: layout === "compact" ? 88 : undefined });
    expect(walk(pending).some((element) => element.props.label === "Stop before next session")).toBe(true);
    expect(walk(pending).find((element) => element.props.testID === "sessions-bulk-status")!.props.accessibilityLiveRegion).toBe("polite");
    expect(executing).toHaveLength(1);
  });
  it("enters selection through a row long press or the row menu's Select sessions, and resets host-local selection", async () => {
    mount(); await settle();
    hold("a");
    expect(checked()).toEqual(["a"]); expect(dense("b").props.mode).toMatchObject({ kind: "select", selected: false });
    fake.host = "host-b"; mount(); await settle();
    expect(header()).toBeUndefined(); expect(bar()).toBeUndefined(); expect(toggle()).toBeDefined(); expect(sheet()).toBeUndefined();
    // Back on host A nothing is selected; the row menu enters selection with that row.
    fake.host = "host-a"; mount(); await settle(); expect(dense("a").props.mode).toMatchObject({ kind: "open" });
    menu("b").find((action) => action.key === "select")!.perform(); harness.flush();
    expect(checked()).toEqual(["b"]); expect(headerText()[0]).toBe("1 / 3 selected");
  });
  it("selects every match from D1's header within one batch of a complete list, and keeps a selection the filters hide", async () => {
    const titled = fake.state as { entries: ReturnType<typeof sessionEntry>[] };
    fake.state = { ...titled, entries: [sessionEntry("a", { title: "Alpha route" }), sessionEntry("b", { title: "Beta route" }), sessionEntry("other", { title: "Gamma review" })] };
    mount(); await settle(); start();
    expect(selectAll()).toMatchObject({ checked: false, unavailable: null });
    selectAll().onPress(); harness.flush();
    expect(checked()).toEqual(["a", "b", "other"]); expect(headerText()[0]).toBe("3 / 3 selected"); expect(selectAll().checked).toBe(true);
    selectAll().onPress(); harness.flush(); expect(checked()).toEqual([]);
    // Frozen: a search that hides a selected session keeps it; the header counts it apart, so N never exceeds the total.
    pick("a", "other");
    search("gamma");
    expect(listed()).toEqual(["other"]); expect(headerText()).toEqual(["1 / 1 selected", "Done", "1 selected session not shown by the current filters; it stays selected."]);
    expect(bar()!.props.count).toBe(2);
    search(""); expect(checked()).toEqual(["a", "other"]);
    // Select all is refused, with its reason, while the list may be incomplete or its matches exceed one batch.
    const base = fake.state as { entries: ReturnType<typeof sessionEntry>[] };
    fake.state = { ...base, inventoryComplete: false }; harness.flush();
    expect(selectAll().unavailable).toBe("The session list is not complete yet; select sessions one by one.");
    expect(headerText()).toContain("The session list is not complete yet; select sessions one by one.");
    fake.state = { ...base, entries: [...base.entries, ...Array.from({ length: 48 }, (_, n) => sessionEntry(`extra-${n}`, { status: "idle" }))] }; harness.flush();
    expect(selectAll().unavailable).toBe("More than 50 sessions: narrow the view, or select one by one.");
    expect(checked()).toEqual(["a", "other"]);
  });
  it("keeps uncertainty inline and checks the exact outcome without resending archive", async () => {
    mount(); await settle(); hold("a");
    openReview(); await settle(); confirm(); await settle();
    executing.shift()!(row("a", { outcome: "uncertain", message: "Lost acknowledgement; do not resend" })); await settle();
    expect(listed()).toContain("a"); expect(summary()).toBe("Archived 0 · 1 not confirmed"); expect(fake.toast.show).not.toHaveBeenCalled();
    expect(feedback("a").props.row).toMatchObject({ outcome: "uncertain" });
    closeReview(); await settle();
    fake.rpc["sessions.lifecycle-reconcile-item"] = async () => row("a", { outcome: "archived", runtime: "closed", availability: { agentId: "a", state: "readable", checkedAt: new Date().toISOString(), revision: sessionEntry("a").updatedAt, reason: "Archived", archivedAt: "now" } });
    const check = walk(drawnRow("a")).find((element) => element.type === Action && element.props.label === "Refresh status")!;
    expect(check.props).toMatchObject({ accessibilityLabel: "Refresh archive status: Session a (a)", disabled: false });
    (check.props.onPress as () => void)(); await settle();
    expect(listed()).not.toContain("a"); expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-execute-item")).toHaveLength(1);
    expect(fake.calls.find(([name]) => name === "sessions.lifecycle-reconcile-item")?.[1]).toMatchObject({ agentId: "a", reviewId: review(["a"]).id });
  });
  it("toasts once for a batch that finishes after its dialog closed, and the closed dialog stops before the next session", async () => {
    mount(); await settle(); hold("a"); pick("b");
    openReview(); await settle();
    const drawn = confirm(); await settle();
    pressIn(drawn, "Close"); await settle(); expect(sheet()).toBeUndefined();
    executing.shift()!(row("a", { outcome: "archived", runtime: "closed", availability: { agentId: "a", state: "readable", checkedAt: new Date().toISOString(), revision: sessionEntry("a").updatedAt, reason: "Archived", archivedAt: "now" } })); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-execute-item").map(([, input]) => input.agentId)).toEqual(["a"]);
    expect(fake.toast.show).toHaveBeenCalledExactlyOnceWith("Stopped before the next session. Archived 1 · 1 not attempted");
    expect(toolbar().props.lifecycle).toMatchObject({ batch: { announcement: "Stopped before the next session. Archived 1 · 1 not attempted" } });
  });
  it("adds only an idle child by exact ID for a second review, never a running one, and sends nothing before confirmation", async () => {
    const children = [{ id: "idle-child", title: "Idle child", status: "idle", workspaceId: null }, { id: "busy-child", title: "Busy child", status: "running", workspaceId: null }];
    fake.rpc["sessions.lifecycle-preview"] = async (input) => ({ ...review([]), rows: (input.agentIds as string[]).map((id) => id === "b"
      ? row("b", { eligible: false, category: "excluded", reason: "Archive blocked: native archive can cascade to these children. Preserve or finish them through the owning workflow first.", children }) : row(id)) });
    mount(); await settle(); hold("a"); pick("b");
    openReview(); await settle();
    expect(summary()).toBe("1 ready · 1 blocked");
    pressIn(dialog(), "Add 1 idle child"); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-preview").map(([, input]) => input.agentIds)).toEqual([["a", "b"], ["a", "b", "idle-child"]]);
    const second = dialog();
    expect(second.some((element) => element.props.label === "Add 1 idle child")).toBe(false);
    expect(second.some((element) => element.type === "Text" && element.props.children === "1 active child not selected · 1 running")).toBe(true);
    expect(byId(second, "bulk-archive-confirm").props).toMatchObject({ label: "Archive 2", accessibilityLabel: "Archive 2 sessions" });
    expect(fake.calls.some(([name]) => name === "sessions.lifecycle-execute-item")).toBe(false);
    expect(checked()).toEqual(["a", "b"]); expect(bar()!.props.count).toBe(2);
  });
  it("reviews only blocked rows again, then deselects them or dismisses results without any archive request", async () => {
    mount(); await settle(); hold("a"); pick("b");
    openReview(); await settle(); confirm(); await settle();
    executing.shift()!(row("a", { outcome: "archived", runtime: "closed", availability: { agentId: "a", state: "readable", checkedAt: new Date().toISOString(), revision: sessionEntry("a").updatedAt, reason: "Archived", archivedAt: "now" } })); await settle();
    executing.shift()!(row("b", { outcome: "blocked", message: "Session changed or disappeared while review was open. Nothing dispatched; create a fresh review." })); await settle();
    // D4's result footer: Close LEFT, Review N again RIGHT.
    expect(dialog().filter((element) => element.props.testID === "bulk-archive-close" || element.props.testID === "bulk-archive-again").map((element) => element.props.label)).toEqual(["Close", "Review 1 again"]);
    pressIn(dialog(), "Review 1 again"); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-preview").at(-1)?.[1]).toMatchObject({ agentIds: ["b"] });
    // A second batch from that review: b blocked again, then the toolbar's selection-only actions.
    confirm(); await settle();
    executing.shift()!(row("b", { outcome: "blocked", message: "Still blocked." })); await settle();
    closeReview(); await settle();
    const results = () => walk(BulkArchiveToolbar(toolbar().props as Parameters<typeof BulkArchiveToolbar>[0]));
    expect(feedback("b").props.row).toMatchObject({ outcome: "blocked" });
    pressIn(results(), "Deselect blocked"); harness.flush(); expect(checked()).toEqual([]); expect(listed()).toContain("b");
    pressIn(results(), "Dismiss"); harness.flush();
    // Nothing is drawn under the row once its result is dismissed.
    expect(feedback("b")?.props.row).toBeUndefined();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-execute-item").map(([, input]) => input.agentId)).toEqual(["a", "b", "b"]);
  });
});
