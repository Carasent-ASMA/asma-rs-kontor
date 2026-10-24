import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
import { ArchiveRecoverySheet } from "../client/archive-recovery";
import { configureArchiveNoticeStorage } from "../client/archive-feedback";
import { createLifecycleService, type LifecycleClient } from "../server/session-lifecycle";
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
// Two rounds: a read an effect starts in the first round's render (each listed ref's owner review) lands before the second.
const settle = async () => { for (let round = 0; round < 2; round++) { for (let n = 0; n < 10; n++) await new Promise((resolve) => setTimeout(resolve, 0)); harness.flush(); } };
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
const ONE = `Session ${SA}: archived, runtime stop not confirmed`;

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
const reviewButton = () => walk(recoveryLine()?.props.children).find((element) => element.props.label === "Review archive" || element.props.label === "Review archives");
const count = (name: string) => fake.calls.filter(([called]) => called === name).length;
const sheet = () => all().find((element) => element.type === ArchiveRecoverySheet);
type Recovery = { items: Array<{ ref: UnresolvedRef; state: string; row: LifecycleRow | null; error: string | null }>; refreshing: string | null; refresh(only?: UnresolvedRef): Promise<void>; reload(): Promise<void> };
const recovery = () => sheet()!.props.recovery as Recovery;
/** Review archive opens the sheet; its own presses are covered by archive-recovery.test, so these call what they call. */
const openSheet = async () => { press(reviewButton()); await settle(); expect(sheet()).toBeDefined(); return recovery(); };
const dialog = () => all().find((element) => element.type === SessionLifecycleDialog);
const press = (element: Element | undefined) => (element!.props.onPress as () => void)();

describe.each(["full", "slim", "compact"] as const)("Archive X on the %s layout", (layout) => {
  let unresolved: UnresolvedRef[];
  let executing: Array<(value: LifecycleRow) => void>;
  let durable: Map<string, ReturnType<typeof review>>;
  beforeEach(() => {
    clearSessionViews(); patchSessionView("host-a", { layout: "flat" });
    unresolved = []; executing = []; durable = new Map(); fake.calls = []; fake.host = "host-a"; fake.refresh = vi.fn(() => new Promise(() => {})); configureArchiveNoticeStorage(null);
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

  it("archives quietly, keeps SA recoverable through SWE, a Resume review and a remount, then refreshes SA exactly", async () => {
    mount(layout); await settle();
    expect(recoveryLine()).toBeUndefined(); expect(listedIds()).toEqual(expect.arrayContaining([SA, SWE]));
    (archiveX(SA).props.onPress as () => void)(); await settle();
    // Pending: no sheet, no Resume or Refresh status, no stale banner, nothing refreshed, and the pressed X reports busy.
    expect(all().some((element) => element.type === SessionLifecycleDialog)).toBe(false);
    expect(resumeControls().length).toBeGreaterThan(0); expect(resumeControls().every((control) => control.props.disabled === true)).toBe(true);
    expect(recoveryLine()).toBeUndefined(); expect(byTestID("sessions-source-banner")).toHaveLength(0);
    expect(archiveX(SA).props).toMatchObject({ pending: true, disabled: true }); expect(archiveX(SWE).props).toMatchObject({ pending: false, disabled: true });
    expect(fake.refresh).not.toHaveBeenCalled(); expect(listedIds()).toContain(SA); expect(count("sessions.lifecycle-execute-item")).toBe(1);
    const owner = review("archive", [SA]);
    unresolved = [{ reviewId: owner.id, agentId: SA }]; durable.set(owner.id, { ...owner, rows: [archived(SA, "unconfirmed", UNCONFIRMED)] });
    executing.shift()!(archived(SA, "unconfirmed", UNCONFIRMED)); await settle();
    // The toast names the state plainly; the host's own wording waits in the sheet's technical details.
    expect(fake.toast.error).toHaveBeenCalledExactlyOnceWith(`${ONE}.`); expect(fake.toast.show).not.toHaveBeenCalled();
    // Settled: only SA leaves; the live, complete directory is not walked again.
    expect(listedIds()).not.toContain(SA); expect(listedIds()).toEqual(expect.arrayContaining([SWE, OTHER])); expect(fake.refresh).not.toHaveBeenCalled();
    expect(lineText()).toBe(ONE); expect(reviewButton()?.props.accessibilityLabel).toBe("Review unconfirmed archive");

    (archiveX(SWE).props.onPress as () => void)(); await settle();
    // Reading stays open while another archive runs; only Refresh status waits.
    await openSheet(); expect(sheet()!.props.canRefresh).toBe(false); (sheet()!.props.onClose as () => void)(); await settle();
    expect(all().some((element) => element.type === SessionLifecycleDialog)).toBe(false);
    executing.shift()!(archived(SWE, "closed", "Archived. An exact-ID read shows its runtime closed. Durable history and files remain with the host.")); await settle();
    expect(fake.toast.show).toHaveBeenCalledExactlyOnceWith(`Archived Session ${SWE}.`); expect(listedIds()).not.toContain(SWE);
    expect(lineText()).toBe(ONE);

    (resumeControls()[0]!.props.onPress as () => void)(); harness.flush();
    const dialog = all().find((element) => element.type === SessionLifecycleDialog)!;
    await (dialog.props.lifecycle as { prepare(action: string, scope: string, ids: string[]): Promise<unknown> }).prepare("resume", "filtered", [OTHER]); await settle();
    (all().find((element) => element.type === SessionLifecycleDialog)!.props.onClose as () => void)(); await settle();
    expect(lineText()).toBe(ONE);

    harness.unmount(); mount(layout); await settle();
    expect(lineText()).toBe(ONE);
    const before = count("sessions.lifecycle-execute-item");
    const shown = await openSheet(); expect(sheet()!.props.canRefresh).toBe(true);
    await shown.refresh(shown.items[0]!.ref); await settle();
    expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-reconcile-item").map(([, input]) => input)).toEqual([{ reviewId: review("archive", [SA]).id, agentId: SA }]);
    expect(count("sessions.lifecycle-execute-item")).toBe(before); expect(before).toBe(2);
    expect(recoveryLine()).toBeUndefined();
  });

  // The walk is the fallback for a directory without a complete live stream, judged when the archive settles.
  const notLive = { live: false, inventoryComplete: false, staleReason: "Live updates are unavailable; the inventory may be incomplete." };
  it.each([
    ["stays live and complete", {}, {}, 0],
    ["loses its live stream before the archive settles", {}, { status: "stale", ...notLive }, 1],
    ["gains its live stream before the archive settles", notLive, {}, 0],
    ["has no live stream", notLive, notLive, 1],
    ["is live but its inventory is not complete", { inventoryComplete: false }, { inventoryComplete: false }, 1],
    ["keeps its complete inventory but drops its live stream", {}, { live: false }, 1],
    ["is marked stale though its stream is live and complete", {}, { status: "stale" }, 1],
  ] as const)("changes only the archived row and walks again only without a complete live stream: directory %s", async (_case, atPress, atSettle, walks) => {
    const base = fake.state as object;
    fake.state = { ...base, ...atPress };
    mount(layout); await settle();
    press(archiveX(SA)); await settle();
    fake.state = { ...base, ...atSettle }; harness.flush();
    const before = fake.refresh.mock.calls.length;
    executing.shift()!(archived(SA, "closed", "Archived. An exact-ID read shows its runtime closed. Durable history and files remain with the host.")); await settle();
    expect(fake.refresh.mock.calls.length - before).toBe(walks);
    expect(listedIds()).not.toContain(SA); expect(listedIds()).toEqual(expect.arrayContaining([SWE, OTHER]));
    expect(fake.toast.show).toHaveBeenCalledExactlyOnceWith(`Archived Session ${SA}.`); expect(fake.toast.error).not.toHaveBeenCalled(); expect(dialog()).toBeUndefined();
  });

  // SA's X leaves an unconfirmed archive; the durable owner review is what the sheet and Refresh status both name.
  async function unconfirmedSA() {
    mount(layout); await settle();
    press(archiveX(SA)); await settle();
    const owner = review("archive", [SA]);
    unresolved = [{ reviewId: owner.id, agentId: SA }]; durable.set(owner.id, { ...owner, rows: [archived(SA, "unconfirmed", UNCONFIRMED)] });
    executing.shift()!(archived(SA, "unconfirmed", UNCONFIRMED)); await settle();
    expect(lineText()).toBe(ONE);
    return owner.id;
  }

  it("holds every Resume, the X and Refresh status while a Refresh status runs, then frees them (QA P2 / AUD P2-02)", async () => {
    await unconfirmedSA();
    expect(resumeControls().length).toBeGreaterThan(0); expect(resumeControls().every((control) => control.props.disabled === false)).toBe(true);
    expect(archiveX(SWE).props.disabled).toBe(false);
    const shown = await openSheet(); expect(sheet()!.props.canRefresh).toBe(true);
    const checked = deferred<LifecycleRow>(); fake.rpc["sessions.lifecycle-reconcile-item"] = () => checked.promise;
    void shown.refresh(shown.items[0]!.ref); await settle();
    const before = { preview: count("sessions.lifecycle-preview"), execute: count("sessions.lifecycle-execute-item"), read: count("sessions.lifecycle-review") };
    expect(sheet()!.props.canRefresh).toBe(false); expect(recovery().refreshing).toBe(`${shown.items[0]!.ref.reviewId}:${SA}`);
    expect(resumeControls().every((control) => control.props.disabled === true)).toBe(true); expect(archiveX(SWE).props.disabled).toBe(true);
    for (const control of [...resumeControls(), archiveX(SWE)]) press(control);
    void recovery().refresh(); await settle();
    expect(dialog()).toBeUndefined(); expect(archiveX(SWE).props.pending).toBe(false);
    expect([count("sessions.lifecycle-preview"), count("sessions.lifecycle-execute-item"), count("sessions.lifecycle-review"), count("sessions.lifecycle-reconcile-item")]).toEqual([before.preview, before.execute, before.read, 1]);
    unresolved = []; checked.resolve(archived(SA, "closed", "Archived. An exact-ID read shows its runtime closed. No additional archive request was sent.")); await settle();
    expect(recoveryLine()).toBeUndefined(); expect(archiveX(SWE).props.disabled).toBe(false); expect(sheet()!.props.canRefresh).toBe(true);
    expect(resumeControls().every((control) => control.props.disabled === false)).toBe(true);
    (sheet()!.props.onClose as () => void)(); await settle(); expect(sheet()).toBeUndefined();
    press(resumeControls()[0]); harness.flush(); expect(dialog()).toBeDefined();
  });

  it("lists SA from its own owner review after SWE's X and after a remount (AUD P2-01)", async () => {
    const owner = await unconfirmedSA();
    press(archiveX(SWE)); await settle();
    executing.shift()!(archived(SWE, "closed", "Archived. An exact-ID read shows its runtime closed. Durable history and files remain with the host.")); await settle();
    expect(lineText()).toBe(ONE);
    for (const remount of [false, true]) {
      fake.calls = [];
      if (remount) { harness.unmount(); mount(layout); await settle(); expect(lineText()).toBe(ONE); expect(fake.calls.filter(([name]) => name === "sessions.lifecycle-review").map(([, input]) => input)).toContainEqual({ reviewId: owner }); }
      const shown = await openSheet();
      expect(shown.items.map((item) => [item.ref.reviewId, item.row?.id, item.row?.runtime, item.state])).toEqual([[owner, SA, "unconfirmed", "unconfirmed"]]);
      expect(dialog()).toBeUndefined(); (sheet()!.props.onClose as () => void)(); await settle();
    }
    expect(count("sessions.lifecycle-execute-item")).toBe(0); expect(count("sessions.lifecycle-reconcile-item")).toBe(0);
  });

  it("never shows SWE's or any other review when SA's own review cannot be read (QA1-ARCHIVE-B-P2-02)", async () => {
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
    await openSheet();
    for (const [answer, message] of answers) {
      fake.rpc["sessions.lifecycle-review"] = (input) => input.reviewId ? answer() : latestRead(input);
      fake.toast.error.mockReset();
      await recovery().reload(); await settle();
      // The last known state and row stay; this read's failure is named beside it.
      expect(recovery().items.map((item) => [item.ref.agentId, item.state, item.row?.id, item.error])).toEqual([[SA, "unconfirmed", SA, message]]);
      expect(dialog()).toBeUndefined(); expect(fake.toast.error).not.toHaveBeenCalled(); expect(lineText()).toBe(ONE);
    }
    fake.rpc["sessions.lifecycle-review"] = (input) => input.reviewId === owner ? Promise.resolve(kept) : latestRead(input);
    await recovery().reload(); await settle();
    expect(recovery().items.map((item) => [item.ref.reviewId, item.error])).toEqual([[owner, null]]);
    expect(count("sessions.lifecycle-execute-item")).toBe(2); expect(count("sessions.lifecycle-reconcile-item")).toBe(0);
  });

  it("drops an owner read the earlier host view started, after A→B and after A→B→A (AUD r4)", async () => {
    const owner = await unconfirmedSA(), kept = durable.get(owner)!, latestRead = fake.rpc["sessions.lifecycle-review"]!;
    fake.toast.error.mockReset();
    // A late success or a late failure from the earlier view: neither lists on, nor reports on, the current view. Host B lists none.
    for (const [path, fails] of [[["host-b"], false], [["host-b", "host-a"], false], [["host-b"], true], [["host-b", "host-a"], true]] as const) {
      fake.rpc["sessions.lifecycle-review"] = (input) => input.reviewId ? Promise.resolve(kept) : latestRead(input);
      fake.host = "host-a"; harness.flush(); await settle();
      expect(lineText()).toBe(ONE);
      let finish!: () => void;
      const held = new Promise<unknown>((resolve, reject) => { finish = () => fails ? reject(new Error("Fixture late failure.")) : resolve(kept); });
      fake.rpc["sessions.lifecycle-review"] = (input) => input.reviewId ? held : fake.host === "host-b" ? Promise.resolve(review("resume", [OTHER], [])) : latestRead(input);
      void (await openSheet()).reload(); await settle(); (sheet()!.props.onClose as () => void)(); await settle();
      for (const next of path) { fake.host = next; harness.flush(); await settle(); }
      finish(); await settle();
      expect(dialog()).toBeUndefined(); expect(fake.toast.error).not.toHaveBeenCalled();
      if (path.at(-1) === "host-b") expect(recoveryLine()).toBeUndefined();
      else expect(lineText()).toBe(fails ? "An archived session: couldn't check archive" : ONE);
    }
    expect(count("sessions.lifecycle-execute-item")).toBe(1); expect(count("sessions.lifecycle-reconcile-item")).toBe(0);
  });

  // Production keys SessionsScreen by host (kontor-screen.tsx:243): a host switch unmounts A and mounts B.
  async function onHost(id: "host-a" | "host-b", refs: UnresolvedRef[]) {
    harness.unmount(); fake.host = id; unresolved = refs; patchSessionView(id, { layout: "flat" }); mount(layout); await settle();
    fake.toast.error.mockReset(); fake.toast.show.mockReset();
  }
  const quiet = () => { expect(dialog()).toBeUndefined(); expect(fake.toast.error).not.toHaveBeenCalled(); expect(fake.toast.show).not.toHaveBeenCalled(); };

  it("acts on nothing an unmounted host view's owner read, X or Refresh status finishes later (QA1-ARCHIVE-B-P2-03)", async () => {
    const owner = await unconfirmedSA(), kept = durable.get(owner)!, latestRead = fake.rpc["sessions.lifecycle-review"]!, saRef = [{ reviewId: owner, agentId: SA }];
    for (const fails of [true, false]) {
      let finish!: () => void;
      const held = new Promise<unknown>((resolve, reject) => { finish = () => fails ? reject(new Error("Fixture read on unmounted host A.")) : resolve(kept); });
      fake.rpc["sessions.lifecycle-review"] = (input) => input.reviewId ? held : latestRead(input);
      await onHost("host-a", saRef); expect(recoveryLine()).toBeUndefined();
      await onHost("host-b", []); expect(recoveryLine()).toBeUndefined();
      finish(); await settle(); quiet(); expect(recoveryLine()).toBeUndefined();
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
    await onHost("host-a", [...saRef, { reviewId: review("archive", [OTHER]).id, agentId: OTHER }]); void (await openSheet()).refresh(); await settle();
    await onHost("host-b", []); const reconciled = count("sessions.lifecycle-reconcile-item"), refreshedB = fake.refresh.mock.calls.length;
    checked.resolve(archived(SA, "closed", "Archived. An exact-ID read shows its runtime closed. No additional archive request was sent.")); await settle();
    quiet(); expect(reconciled).toBe(1); expect(count("sessions.lifecycle-reconcile-item")).toBe(1); expect(fake.refresh).toHaveBeenCalledTimes(refreshedB);
  });

  it("acts on nothing an unmounted host view's Resume batch or row Refresh status finishes later (QA1-ARCHIVE-B-P2-03 class)", async () => {
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

  it.each([
    ["an already-archived target whose exact read fails", () => Promise.reject(new Error("Timeout")), "unknown", "unknown", "Unknown: Native read failed: Timeout. Absence is not confirmed.", false],
    ["an already-archived target whose exact read confirms absence", () => Promise.resolve(null), "missing", "missing", null, false],
    // 139ce565 saved "missing" for an unlisted target whose read failed; hosts still hold such receipts (AUD r3).
    ["an unlisted target whose exact read failed, as 139ce565 saved it", () => Promise.reject(new Error("Timeout")), "missing", "unknown", "Unknown: Native read failed: Timeout. Absence is not confirmed.", true],
  ] as const)("reads back %s, from Resume's last receipt, as exactly that (QA1-FAST1-R2-P2-01)", async (_, refresh, stored, status, unknown, base) => {
    // The receipt is the real service's own record of the X's preview and execute, so the dialog shows what the host saved.
    const root = await mkdtemp(join(tmpdir(), "kontor-archive-x-receipt-"));
    const records = base ? [] : [{ id: SA, title: `Session ${SA}`, provider: "codex", cwd: "/fixture", status: "closed", archivedAt: "2026-10-22", labels: {} }];
    const files = async () => new Map(await Promise.all((await readdir(root)).map(async (name) => [name, await readFile(join(root, name), "utf8")] as const)));
    let archives = 0;
    const client = { agents: {
      list: async (options?: { filter?: { includeArchived?: boolean } }) => ({ requestId: "fixture", pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
        entries: records.filter((record) => options?.filter?.includeArchived || !record.archivedAt).map((agent) => ({ agent, project: null })) }),
      ref: () => ({ refresh, archive: async () => { archives++; return { archivedAt: "2026-10-22" }; }, send: async () => undefined, timeline: { refetch: async () => ({ entries: [], error: null }) } }),
    }, providers: { snapshot: async () => ({ requestId: "fixture", generatedAt: "now", entries: [] }) } } as unknown as LifecycleClient;
    try {
      const service = createLifecycleService(client, root), receipt = await service.preview({ action: "archive", scope: "filtered", agentIds: [SA], target: DEFAULT_RECOVERY_TARGET });
      await service.execute(receipt.id, SA);
      if (base) {
        // That build's review and blocked result differ from this one's only in the stored status.
        const rewritten = [...await files()].filter(([, bytes]) => bytes.includes('"nativeStatus":"unknown"'));
        for (const [name, bytes] of rewritten) await writeFile(join(root, name), bytes.replace('"nativeStatus":"unknown"', '"nativeStatus":"missing"'));
        expect(rewritten.length).toBe(2);
      }
      expect((await service.read(receipt.id))!.rows[0]).toMatchObject({ nativeStatus: stored, availability: { state: status }, eligible: status === "missing" });
      fake.rpc["sessions.lifecycle-review"] = (input) => service.read(input.reviewId as string | undefined);
      const saved = await files();
      mount(layout); await settle(); fake.calls = [];
      press(resumeControls()[0]); harness.flush();
      const body = () => { const shown = dialog()!; return walk((shown.type as (props: unknown) => unknown)(shown.props)); };
      press(body().find((element) => element.props.label === "Read last action receipt"));
      // The restore reads real files, so wait for its review rather than a fixed number of ticks.
      await vi.waitFor(() => { harness.flush(); expect((dialog()!.props.lifecycle as { review: unknown; busy: boolean })).toMatchObject({ review: { id: receipt.id }, busy: false }); }, { timeout: 5_000 });
      const texts = body().flatMap((element) => element.type === "Text" ? [String(element.props.children)] : []);
      expect(texts).toContain(`${SA} · ${status}`);
      if (unknown) { expect(texts).toContain(unknown); expect(texts.join("\n")).not.toMatch(/missing/i); }
      expect(["sessions.lifecycle-preview", "sessions.lifecycle-execute-item", "sessions.lifecycle-reconcile-item"].map(count)).toEqual([0, 0, 0]); expect(archives).toBe(0);
      expect(await files()).toEqual(saved);
    } finally { await rm(root, { recursive: true, force: true }); }
  });
});
