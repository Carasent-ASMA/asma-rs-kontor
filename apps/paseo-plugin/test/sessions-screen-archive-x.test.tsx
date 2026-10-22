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

import { SessionsScreen, SlimToolbar } from "../client/sessions-screen";
import { SessionLifecycleDialog } from "../client/session-lifecycle-view";
import { DEFAULT_RECOVERY_TARGET, TAKEOVER_LIMIT, type LifecycleRow, type UnresolvedRef } from "../shared/session-lifecycle";
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
const settle = async () => { for (let n = 0; n < 10; n++) await new Promise((resolve) => setTimeout(resolve, 0)); harness.flush(); };
function deferred<T>() { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; }
const colors = { accent: "#05f", accentForeground: "#fff", border: "#ccc", foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#f4f4f4", surface2: "#eee", statusDanger: "#c00", statusSuccess: "#080", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
const SA = "fixture-sa", SWE = "fixture-swe", OTHER = "fixture-other";
const reviewId = (id: string) => `00000000-0000-4000-8000-${Buffer.from(id).toString("hex").slice(-12).padStart(12, "0")}`;
const review = (action: "archive" | "resume", ids: string[], unresolved?: UnresolvedRef[]) => ({ id: reviewId(`${action}:${ids.join()}`.slice(-6)), createdAt: "2026-10-22", action, scope: "filtered", target: DEFAULT_RECOVERY_TARGET,
  rows: ids.map((id) => row(id)), limitation: TAKEOVER_LIMIT, ...(unresolved ? { unresolved } : {}) });
const row = (id: string, patch: Partial<LifecycleRow> = {}) => ({ id, title: `Session ${id}`, workspaceId: null, cwd: "/fixture", provider: "codex", model: null, effort: null, mode: null, nativeStatus: "idle", fingerprint: `fp-${id}`,
  category: "candidate", eligible: true, reason: "Fixture.", children: [], outcome: "pending", message: null, ...patch }) as LifecycleRow;
const archived = (id: string, runtime: LifecycleRow["runtime"], message: string) => row(id, { outcome: "archived", runtime, message,
  availability: { agentId: id, state: "readable", reason: "Fixture read.", checkedAt: new Date().toISOString(), revision: "2026-10-22", archivedAt: "2026-10-22", nativeStatus: runtime === "closed" ? "closed" : "idle" } });
const UNCONFIRMED = "Archived. The archive reply did not confirm completion (Timeout), so stopping its runtime is not confirmed.";

function mount(layout: "full" | "slim" | "compact") {
  const compact = layout === "compact";
  harness.render(() => SessionsScreen({ host: { id: fake.host, label: "Fixture host" }, layout: { compact, platform: compact ? "ios" : "web" },
    navigation: { openAgent: () => undefined, openBrowser: () => undefined, openWorkspace: () => undefined }, theme: { colors } as PluginTheme, params: {} } as Parameters<typeof SessionsScreen>[0]));
  const list = byTestID("sessions-list")[0]!;
  (list.props.ref as { current: unknown }).current = { scrollToOffset: () => undefined, scrollToIndex: () => undefined };
  for (const [testID, height] of [["sessions-full-toolbar", 220], ["sessions-slim-toolbar-slot", 44]] as const) {
    const slot = byTestID(testID)[0]; (slot?.props.onLayout as ((event: unknown) => void) | undefined)?.({ nativeEvent: { layout: { x: 0, y: 0, width: 400, height } } });
  }
  if (layout === "slim") {
    (byTestID("sessions-list")[0]!.props.onScroll as (event: unknown) => void)({ nativeEvent: { contentOffset: { x: 0, y: 300 }, contentSize: { width: 400, height: 2000 }, layoutMeasurement: { width: 400, height: 600 } } });
    harness.flush();
    expect(all().find((element) => element.type === SlimToolbar)?.props.action).toBeDefined();
  }
}
type ListRow = { kind: string; entry?: { id: string } };
const listedIds = () => (byTestID("sessions-list")[0]!.props.data as ListRow[]).flatMap((item) => item.entry && (item.kind === "session" || item.kind === "result") ? [item.entry.id] : []);
function archiveX(id: string): Element {
  const list = byTestID("sessions-list")[0]!, item = (list.props.data as ListRow[]).find((candidate) => candidate.entry?.id === id && (candidate.kind === "session" || candidate.kind === "result"))!;
  return ((list.props.renderItem as (info: { item: unknown }) => Element)({ item }).props.lifecycleAction) as Element;
}
const resumeControls = () => all().flatMap((element) => element.type === SlimToolbar && element.props.action ? [element.props.action as Element] : element.props.accessibilityLabel === "Resume sessions" ? [element] : []);
const recoveryLine = () => { const found = byTestID("sessions-archive-recovery"); expect(found.length).toBeLessThanOrEqual(1); return found[0]; };
const lineText = () => { const line = recoveryLine(); return line ? walk(line.props.children).filter((element) => element.type === "Text").map((element) => element.props.children).join("") : null; };
const checkAgain = () => walk(recoveryLine()?.props.children).find((element) => element.props.label === "Check again" || element.props.label === "Checking…");
const count = (name: string) => fake.calls.filter(([called]) => called === name).length;
const details = () => walk(recoveryLine()?.props.children).find((element) => element.props.accessibilityLabel === "Archive details");
const dialog = () => all().find((element) => element.type === SessionLifecycleDialog);
const press = (element: Element | undefined) => (element!.props.onPress as () => void)();

describe.each(["full", "slim", "compact"] as const)("Archive X on the %s layout", (layout) => {
  let unresolved: UnresolvedRef[];
  let executing: Array<(value: LifecycleRow) => void>;
  let durable: Map<string, ReturnType<typeof review>>;
  beforeEach(() => {
    clearSessionViews(); patchSessionView("host-a", { layout: "flat" });
    unresolved = []; executing = []; durable = new Map(); fake.calls = []; fake.host = "host-a"; fake.refresh = vi.fn(() => new Promise(() => {}));
    fake.toast.show.mockReset(); fake.toast.error.mockReset();
    fake.state = { entries: [SA, SWE, OTHER].map((id) => sessionEntry(id, { status: "idle" })), status: "ready", live: true, error: null, staleReason: null, pages: 1, lastUpdatedAt: 0, inventoryComplete: true, includeArchived: false };
    fake.rpc = {
      "sessions.lifecycle-review": async (input) => input.reviewId ? durable.get(input.reviewId as string) ?? null : review("resume", [OTHER], [...unresolved]),
      "sessions.lifecycle-preview": async (input) => review(input.action as "archive" | "resume", input.agentIds as string[]),
      "sessions.lifecycle-execute-item": () => { const next = deferred<LifecycleRow>(); executing.push(next.resolve); return next.promise; },
      "sessions.lifecycle-reconcile-item": async (input) => { unresolved = []; return archived(input.agentId as string, "closed", "Archived. An exact-ID read shows its runtime closed. No additional archive request was sent."); },
      "sessions.lifecycle-dispositions": async () => ({}), "sessions.availability": async () => ({}),
    };
  });
  afterEach(() => harness.unmount());

  it("archives quietly, keeps SA recoverable through SWE, a Resume review and a remount, then checks SA exactly", async () => {
    mount(layout); await settle();
    expect(recoveryLine()).toBeUndefined(); expect(listedIds()).toEqual(expect.arrayContaining([SA, SWE]));
    (archiveX(SA).props.onPress as () => void)(); await settle();
    // Pending: no sheet, no Resume or Check, no stale banner, nothing refreshed, and the pressed X reports busy.
    expect(all().some((element) => element.type === SessionLifecycleDialog)).toBe(false);
    expect(resumeControls().length).toBeGreaterThan(0); expect(resumeControls().every((control) => control.props.disabled === true)).toBe(true);
    expect(recoveryLine()).toBeUndefined(); expect(byTestID("sessions-source-banner")).toHaveLength(0);
    expect(archiveX(SA).props).toMatchObject({ pending: true, disabled: true }); expect(archiveX(SWE).props).toMatchObject({ pending: false, disabled: true });
    expect(fake.refresh).not.toHaveBeenCalled(); expect(listedIds()).toContain(SA); expect(count("sessions.lifecycle-execute-item")).toBe(1);
    unresolved = [{ reviewId: review("archive", [SA]).id, agentId: SA }];
    executing.shift()!(archived(SA, "unconfirmed", UNCONFIRMED)); await settle();
    expect(fake.toast.error).toHaveBeenCalledExactlyOnceWith(UNCONFIRMED); expect(fake.toast.show).not.toHaveBeenCalled();
    expect(listedIds()).not.toContain(SA); expect(fake.refresh).toHaveBeenCalledTimes(1);
    expect(lineText()).toBe("1 archive not confirmed"); expect(checkAgain()?.props.disabled).toBe(false);

    (archiveX(SWE).props.onPress as () => void)(); await settle();
    expect(checkAgain()?.props.disabled).toBe(true); expect(all().some((element) => element.type === SessionLifecycleDialog)).toBe(false);
    executing.shift()!(archived(SWE, "closed", "Archived. An exact-ID read shows its runtime closed. Durable history and files remain with the host.")); await settle();
    expect(fake.toast.show).toHaveBeenCalledExactlyOnceWith(`Archived Session ${SWE}.`); expect(listedIds()).not.toContain(SWE);
    expect(lineText()).toBe("1 archive not confirmed");

    (resumeControls()[0]!.props.onPress as () => void)(); harness.flush();
    const dialog = all().find((element) => element.type === SessionLifecycleDialog)!;
    await (dialog.props.lifecycle as { prepare(action: string, scope: string, ids: string[]): Promise<unknown> }).prepare("resume", "filtered", [OTHER]); await settle();
    (all().find((element) => element.type === SessionLifecycleDialog)!.props.onClose as () => void)(); await settle();
    expect(lineText()).toBe("1 archive not confirmed");

    harness.unmount(); mount(layout); await settle();
    expect(lineText()).toBe("1 archive not confirmed");
    const before = count("sessions.lifecycle-execute-item");
    (checkAgain()!.props.onPress as () => void)(); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-reconcile-item").map(([, input]) => input)).toEqual([{ reviewId: review("archive", [SA]).id, agentId: SA }]);
    expect(count("sessions.lifecycle-execute-item")).toBe(before); expect(before).toBe(2);
    expect(recoveryLine()).toBeUndefined();
  });

  // SA's X leaves an unconfirmed archive; the durable owner review is what Details and Check both name.
  async function unconfirmedSA() {
    mount(layout); await settle();
    press(archiveX(SA)); await settle();
    const owner = review("archive", [SA]);
    unresolved = [{ reviewId: owner.id, agentId: SA }]; durable.set(owner.id, { ...owner, rows: [archived(SA, "unconfirmed", UNCONFIRMED)] });
    executing.shift()!(archived(SA, "unconfirmed", UNCONFIRMED)); await settle();
    expect(lineText()).toBe("1 archive not confirmed");
    return owner.id;
  }

  it("holds every Resume, Details and the X while a Check runs, then frees them (QA P2 / AUD P2-02)", async () => {
    await unconfirmedSA();
    expect(resumeControls().length).toBeGreaterThan(0); expect(resumeControls().every((control) => control.props.disabled === false)).toBe(true);
    expect(details()?.props.disabled).toBe(false); expect(archiveX(SWE).props.disabled).toBe(false);
    const checked = deferred<LifecycleRow>(); fake.rpc["sessions.lifecycle-reconcile-item"] = () => checked.promise;
    press(checkAgain()); await settle();
    const before = { preview: count("sessions.lifecycle-preview"), execute: count("sessions.lifecycle-execute-item"), read: count("sessions.lifecycle-review") };
    expect(checkAgain()?.props).toMatchObject({ label: "Checking…", disabled: true });
    expect(resumeControls().every((control) => control.props.disabled === true)).toBe(true);
    expect(details()?.props.disabled).toBe(true); expect(archiveX(SWE).props.disabled).toBe(true);
    for (const control of [...resumeControls(), details(), archiveX(SWE), checkAgain()]) press(control);
    await settle();
    expect(dialog()).toBeUndefined(); expect(archiveX(SWE).props.pending).toBe(false);
    expect([count("sessions.lifecycle-preview"), count("sessions.lifecycle-execute-item"), count("sessions.lifecycle-review"), count("sessions.lifecycle-reconcile-item")]).toEqual([before.preview, before.execute, before.read, 1]);
    unresolved = []; checked.resolve(archived(SA, "closed", "Archived. An exact-ID read shows its runtime closed. No additional archive request was sent.")); await settle();
    expect(recoveryLine()).toBeUndefined(); expect(archiveX(SWE).props.disabled).toBe(false);
    expect(resumeControls().every((control) => control.props.disabled === false)).toBe(true);
    press(resumeControls()[0]); harness.flush(); expect(dialog()).toBeDefined();
  });

  it("opens SA's own review from Details after SWE's X and after a remount (AUD P2-01)", async () => {
    const owner = await unconfirmedSA();
    press(archiveX(SWE)); await settle();
    executing.shift()!(archived(SWE, "closed", "Archived. An exact-ID read shows its runtime closed. Durable history and files remain with the host.")); await settle();
    expect(lineText()).toBe("1 archive not confirmed");
    for (const remount of [false, true]) {
      if (remount) { harness.unmount(); mount(layout); await settle(); expect(lineText()).toBe("1 archive not confirmed"); }
      fake.calls = [];
      press(details()); await settle();
      expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-review").map(([, input]) => input)).toContainEqual({ reviewId: owner });
      const shown = (dialog()!.props.lifecycle as { review: ReturnType<typeof review> | null }).review;
      expect(shown?.id).toBe(owner); expect(shown?.rows.map((entry) => [entry.id, entry.runtime])).toEqual([[SA, "unconfirmed"]]);
      (dialog()!.props.onClose as () => void)(); await settle();
    }
    expect(count("sessions.lifecycle-execute-item")).toBe(0); expect(count("sessions.lifecycle-reconcile-item")).toBe(0);
  });

  it("never opens SWE's or any other review when Details cannot read SA's own review (QA1-ARCHIVE-B-P2-02)", async () => {
    const owner = await unconfirmedSA(), kept = durable.get(owner)!;
    press(archiveX(SWE)); await settle();
    executing.shift()!(archived(SWE, "closed", "Archived. An exact-ID read shows its runtime closed. Durable history and files remain with the host.")); await settle();
    const latestRead = fake.rpc["sessions.lifecycle-review"]!;
    const answers: Array<[() => Promise<unknown>, string]> = [
      [async () => { throw new Error("Fixture read failure."); }, "Fixture read failure."],
      [async () => null, "The host did not return this archive's own review."],
      [async () => ({ ...review("archive", [SWE]), rows: [archived(SWE, "closed", "Fixture.")] }), "The host did not return this archive's own review."],
      // Another review that lists SA, and the owner's review ID without SA's own row: neither is SA's Details.
      [async () => ({ ...review("resume", [SA]), id: "00000000-0000-4000-8000-0000000000e5", rows: [archived(SA, "unconfirmed", UNCONFIRMED)] }), "The host did not return this archive's own review."],
      [async () => ({ ...kept, rows: [archived(SWE, "closed", "Fixture.")] }), "The host did not return this archive's own review."],
    ];
    for (const [answer, message] of answers) {
      fake.rpc["sessions.lifecycle-review"] = (input) => input.reviewId ? answer() : latestRead(input);
      fake.toast.error.mockReset();
      press(details()); await settle();
      expect(dialog()).toBeUndefined();
      expect(fake.toast.error).toHaveBeenCalledExactlyOnceWith(`Archive details could not be read: ${message}`);
      expect(lineText()).toBe("1 archive not confirmed"); expect(details()?.props.disabled).toBe(false);
    }
    fake.rpc["sessions.lifecycle-review"] = (input) => input.reviewId === owner ? Promise.resolve(kept) : latestRead(input);
    press(details()); await settle();
    expect((dialog()!.props.lifecycle as { review: ReturnType<typeof review> | null }).review?.id).toBe(owner);
    expect(count("sessions.lifecycle-execute-item")).toBe(2); expect(count("sessions.lifecycle-reconcile-item")).toBe(0);
  });

  it("drops a Details read the earlier host view started, after A→B and after A→B→A (AUD r4)", async () => {
    const owner = await unconfirmedSA(), kept = durable.get(owner)!, latestRead = fake.rpc["sessions.lifecycle-review"]!;
    fake.toast.error.mockReset();
    // A late success or a late failure from the earlier view: neither opens, nor reports on the current view.
    for (const [path, fails] of [[["host-b"], false], [["host-b", "host-a"], false], [["host-b"], true], [["host-b", "host-a"], true]] as const) {
      let finish!: () => void;
      const held = new Promise<unknown>((resolve, reject) => { finish = () => fails ? reject(new Error("Fixture late failure.")) : resolve(kept); });
      fake.rpc["sessions.lifecycle-review"] = (input) => input.reviewId ? held : latestRead(input);
      fake.host = "host-a"; harness.flush(); await settle();
      expect(lineText()).toBe("1 archive not confirmed");
      press(details()); await settle();
      for (const next of path) { fake.host = next; harness.flush(); await settle(); }
      finish(); await settle();
      expect(dialog()).toBeUndefined(); expect(fake.toast.error).not.toHaveBeenCalled();
    }
    expect(count("sessions.lifecycle-execute-item")).toBe(1); expect(count("sessions.lifecycle-reconcile-item")).toBe(0);
  });

  // Production keys SessionsScreen by host (kontor-screen.tsx:243): a host switch unmounts A and mounts B.
  async function onHost(id: "host-a" | "host-b", refs: UnresolvedRef[]) {
    harness.unmount(); fake.host = id; unresolved = refs; patchSessionView(id, { layout: "flat" }); mount(layout); await settle();
    fake.toast.error.mockReset(); fake.toast.show.mockReset();
  }
  const quiet = () => { expect(dialog()).toBeUndefined(); expect(fake.toast.error).not.toHaveBeenCalled(); expect(fake.toast.show).not.toHaveBeenCalled(); };

  it("acts on nothing an unmounted host view's Details, X or Check finishes later (QA1-ARCHIVE-B-P2-03)", async () => {
    const owner = await unconfirmedSA(), kept = durable.get(owner)!, latestRead = fake.rpc["sessions.lifecycle-review"]!, saRef = [{ reviewId: owner, agentId: SA }];
    for (const fails of [true, false]) {
      let finish!: () => void;
      const held = new Promise<unknown>((resolve, reject) => { finish = () => fails ? reject(new Error("Fixture read on unmounted host A.")) : resolve(kept); });
      fake.rpc["sessions.lifecycle-review"] = (input) => input.reviewId ? held : latestRead(input);
      await onHost("host-a", saRef); press(details()); await settle();
      await onHost("host-b", []); expect(recoveryLine()).toBeUndefined();
      finish(); await settle(); quiet();
    }
    fake.rpc["sessions.lifecycle-review"] = latestRead;
    await onHost("host-a", saRef); press(archiveX(SWE)); await settle();
    await onHost("host-b", []); const refreshed = fake.refresh.mock.calls.length;
    executing.shift()!(archived(SWE, "closed", "Archived. An exact-ID read shows its runtime closed.")); await settle();
    quiet(); expect(fake.refresh).toHaveBeenCalledTimes(refreshed);
    // A late X failure reports nothing; a preview that lands after the unmount dispatches nothing.
    const execute = fake.rpc["sessions.lifecycle-execute-item"]!, preview = fake.rpc["sessions.lifecycle-preview"]!;
    let refuse!: (error: Error) => void, previewed!: () => void;
    fake.rpc["sessions.lifecycle-execute-item"] = () => new Promise((_resolve, reject) => { refuse = reject; });
    await onHost("host-a", saRef); press(archiveX(SWE)); await settle();
    await onHost("host-b", []); refuse(new Error("Fixture failure on unmounted host A.")); await settle(); quiet();
    fake.rpc["sessions.lifecycle-execute-item"] = execute;
    fake.rpc["sessions.lifecycle-preview"] = (input) => new Promise((resolve) => { previewed = () => resolve(preview(input)); });
    await onHost("host-a", saRef); press(archiveX(SWE)); await settle();
    const dispatched = count("sessions.lifecycle-execute-item");
    await onHost("host-b", []); previewed(); await settle();
    quiet(); expect(count("sessions.lifecycle-execute-item")).toBe(dispatched);
    fake.rpc["sessions.lifecycle-preview"] = preview;
    const checked = deferred<LifecycleRow>(); fake.rpc["sessions.lifecycle-reconcile-item"] = () => checked.promise;
    await onHost("host-a", [...saRef, { reviewId: review("archive", [OTHER]).id, agentId: OTHER }]); press(checkAgain()); await settle();
    await onHost("host-b", []); const reconciled = count("sessions.lifecycle-reconcile-item"), refreshedB = fake.refresh.mock.calls.length;
    checked.resolve(archived(SA, "closed", "Archived. An exact-ID read shows its runtime closed. No additional archive request was sent.")); await settle();
    quiet(); expect(reconciled).toBe(1); expect(count("sessions.lifecycle-reconcile-item")).toBe(1); expect(fake.refresh).toHaveBeenCalledTimes(refreshedB);
  });

  it("acts on nothing an unmounted host view's Resume batch or row Check finishes later (QA1-ARCHIVE-B-P2-03 class)", async () => {
    type Lifecycle = { prepare(action: string, scope: string, ids: string[]): Promise<unknown>; run(): Promise<void>; reconcile(id: string): Promise<void> };
    for (const step of ["run", "reconcile"] as const) {
      await onHost("host-a", []);
      press(resumeControls()[0]); harness.flush();
      await (dialog()!.props.lifecycle as Lifecycle).prepare("resume", "filtered", [OTHER]); await settle();
      const checked = deferred<LifecycleRow>(); fake.rpc["sessions.lifecycle-reconcile-item"] = () => checked.promise;
      void (dialog()!.props.lifecycle as Lifecycle)[step](OTHER as never); await settle();
      await onHost("host-b", []); const refreshed = fake.refresh.mock.calls.length;
      if (step === "run") executing.shift()!(row(OTHER, { outcome: "submitted", message: "Fixture." })); else checked.resolve(row(OTHER, { outcome: "blocked", message: "Fixture." }));
      await settle(); quiet(); expect(fake.refresh).toHaveBeenCalledTimes(refreshed);
    }
    expect(count("sessions.lifecycle-execute-item")).toBe(1); expect(count("sessions.lifecycle-reconcile-item")).toBe(1);
  });

  it("reports the pressed X busy until its result, and a refused archive stays listed with its X free", async () => {
    mount(layout); await settle();
    press(archiveX(OTHER)); await settle();
    const x = archiveX(OTHER), pressable = walk((x.type as (props: unknown) => unknown)(x.props)).find((element) => element.type === "Pressable")!;
    expect(pressable.props).toMatchObject({ accessibilityLabel: `Archiving Session ${OTHER}…`, accessibilityState: { disabled: true, busy: true } });
    executing.shift()!(row(OTHER, { outcome: "blocked", message: "Fixture refusal." })); await settle();
    expect(fake.toast.error).toHaveBeenCalledExactlyOnceWith("Fixture refusal."); expect(listedIds()).toContain(OTHER);
    expect(archiveX(OTHER).props).toMatchObject({ pending: false, disabled: false });
  });
});
