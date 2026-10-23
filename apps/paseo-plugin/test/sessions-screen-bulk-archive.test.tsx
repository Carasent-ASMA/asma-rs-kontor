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
      toast: { show: vi.fn(), error: vi.fn() }, refresh: vi.fn(), state: null as unknown, host: "host-a" },
  };
});
vi.mock("react", async (importOriginal) => ({ ...(await importOriginal<typeof import("react")>()), useContext: () => null, memo: (component: unknown) => component,
  useCallback: harness.useCallback, useEffect: harness.useEffect, useLayoutEffect: harness.useEffect, useMemo: harness.useMemo, useRef: harness.useRef, useState: harness.useState }));
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
import { ArchiveSelectionCheckbox, BulkArchiveDialog, BulkArchiveToolbar } from "../client/bulk-archive";
import { DEFAULT_RECOVERY_TARGET, type LifecycleRow } from "../shared/session-lifecycle";
import { clearSessionViews, patchSessionView } from "../client/session-view-store";
import { sessionEntry } from "./session-fixtures";

type Element = { type?: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk) : [node as Element, ...walk((node as Element).props.children)];
const all = () => walk(harness.flush());
const toolbar = () => all().find((element) => element.type === BulkArchiveToolbar)!;
const sheet = () => all().find((element) => element.type === BulkArchiveDialog);
const confirm = () => {
  const tree = BulkArchiveDialog(sheet()!.props as Parameters<typeof BulkArchiveDialog>[0]);
  const button = walk(tree).find((element) => typeof element.props.label === "string" && /^Archive \d+ sessions?$/.test(element.props.label))!;
  void (button.props.onPress as () => Promise<void>)();
};
type Selection = { start(): void; toggle(id: string): void; selected: string[] };
const selection = () => toolbar().props.selection as Selection;
const list = () => all().find((element) => element.props.testID === "sessions-list")!;
const listed = () => (list().props.data as Array<{ entry?: { id: string } }>).flatMap((row) => row.entry ? [row.entry.id] : []);
const drawnRow = (id: string) => (list().props.renderItem as (input: unknown) => Element)({ item: (list().props.data as Array<{ entry?: { id: string } }>).find((row) => row.entry?.id === id) });
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
    clearSessionViews(); patchSessionView("host-a", { layout: "flat" }); fake.host = "host-a"; fake.calls = []; executing = [];
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
    selection().start(); harness.flush(); selection().toggle("a"); selection().toggle("b"); harness.flush();
    expect(selection().selected).toEqual(["a", "b"]);
    expect((drawnRow("a").props.lifecycleAction as Element).type).toBe(ArchiveSelectionCheckbox);
    (toolbar().props.onReview as () => void)(); await settle();
    expect(sheet()).toBeDefined(); expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-preview").at(-1)?.[1]).toMatchObject({ action: "archive", scope: "filtered", agentIds: ["a", "b"] });
    expect(executing).toHaveLength(0); expect(listed()).toEqual(["a", "b", "other"]);
    confirm(); await settle();
    expect((drawnRow("a").props.lifecycleAction as Element).props.pending).toBe(true);
    expect(listed()).toContain("a");
    executing.shift()!(row("a", { outcome: "archived", runtime: "closed", availability: { agentId: "a", state: "readable", checkedAt: new Date().toISOString(), revision: sessionEntry("a").updatedAt, reason: "Archived", archivedAt: "now" } })); await settle();
    expect(listed()).not.toContain("a"); expect(listed()).toEqual(expect.arrayContaining(["b", "other"]));
    executing.shift()!(row("b", { outcome: "blocked", message: "Unselected managed child" })); await settle();
    expect(listed()).toEqual(expect.arrayContaining(["b", "other"]));
    expect(fake.toast.show).toHaveBeenCalledExactlyOnceWith("Archived 1 · 1 blocked");
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-execute-item").map(([, input]) => input.agentId)).toEqual(["a", "b"]);
    expect(fake.refresh).not.toHaveBeenCalled();
  });
  it("keeps a one-item bulk selection behind confirmation and closes without execution", async () => {
    mount(); await settle(); selection().start(); harness.flush(); selection().toggle("a"); harness.flush();
    (toolbar().props.onReview as () => void)(); await settle();
    expect(executing).toHaveLength(0); (sheet()!.props.onClose as () => void)(); await settle(); expect(sheet()).toBeUndefined(); expect(executing).toHaveLength(0);
  });
  it("enters selection through a row long press and resets host-local selection", async () => {
    mount(); await settle();
    (drawnRow("a").props.onLongPress as (entry: unknown) => void)(sessionEntry("a")); harness.flush();
    expect(selection().selected).toEqual(["a"]);
    fake.host = "host-b"; mount(); await settle(); expect(selection().selected).toEqual([]); expect(sheet()).toBeUndefined();
  });
  it("keeps uncertainty inline and checks the exact outcome without resending archive", async () => {
    mount(); await settle(); selection().toggle("a"); harness.flush();
    (toolbar().props.onReview as () => void)(); await settle(); confirm(); await settle();
    executing.shift()!(row("a", { outcome: "uncertain", message: "Lost acknowledgement; do not resend" })); await settle();
    expect(listed()).toContain("a"); expect(fake.toast.show).toHaveBeenCalledExactlyOnceWith("Archived 0 · 1 not confirmed");
    expect((drawnRow("a").props.lifecycleFeedback as Element).props.row).toMatchObject({ outcome: "uncertain" });
    (sheet()!.props.onClose as () => void)(); await settle();
    fake.rpc["sessions.lifecycle-reconcile-item"] = async () => row("a", { outcome: "archived", runtime: "closed", availability: { agentId: "a", state: "readable", checkedAt: new Date().toISOString(), revision: sessionEntry("a").updatedAt, reason: "Archived", archivedAt: "now" } });
    ((drawnRow("a").props.lifecycleAction as Element).props.onCheck as () => void)(); await settle();
    expect(listed()).not.toContain("a"); expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-execute-item")).toHaveLength(1);
    expect(fake.calls.find(([name]) => name === "sessions.lifecycle-reconcile-item")?.[1]).toMatchObject({ agentId: "a", reviewId: review(["a"]).id });
  });
});
