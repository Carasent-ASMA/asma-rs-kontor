import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CreateOptions, CreateOutcome } from "../shared/session-create";
import type { DirectoryEntry, DirectoryState } from "../client/session-directory";

type HookStore = { slots: unknown[]; deps: (readonly unknown[] | undefined)[]; cleanups: (void | (() => void))[] };
const model = vi.hoisted(() => ({
  slots: [] as unknown[], cursor: 0, effect: 0, deps: [] as (readonly unknown[] | undefined)[],
  cleanups: [] as (void | (() => void))[],
  /** Each drawn child's own hook slots, kept across renders as React keeps a mounted instance's state. */
  stores: {} as Record<string, HookStore>,
  rpc: {} as Record<string, ReturnType<typeof vi.fn>>,
  focusControl: vi.fn(),
  platform: "web",
  hosts: [] as { serverId: string; label: string; status: string }[],
  toast: { error: vi.fn(), show: vi.fn() },
  directory: null as unknown as DirectoryState,
  directoryArgs: [] as object[],
  settings: null as unknown as { status: string; values?: object; revision?: string; save: ReturnType<typeof vi.fn> },
  copyText: vi.fn(async (_text: string) => {}),
  useState<T>(initial: T | (() => T)): [T, (next: T | ((previous: T) => T)) => void] {
    const slots = this.slots, index = this.cursor++;
    if (!(index in slots)) slots[index] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [slots[index] as T, (next) => { slots[index] = typeof next === "function" ? (next as (previous: T) => T)(slots[index] as T) : next; }];
  },
  /** Runs an effect when its dependencies change, as React would after a render. */
  useEffect(effect: () => void | (() => void), deps?: readonly unknown[]) {
    const index = this.effect++, previous = this.deps[index];
    if (previous && deps && deps.length === previous.length && deps.every((value, at) => Object.is(value, previous[at]))) return;
    this.cleanups[index]?.();
    this.deps[index] = deps;
    this.cleanups[index] = effect();
  },
  unmount() { this.cleanups.forEach((cleanup) => cleanup?.()); this.cleanups = []; },
  /** Draws a child on its own hook store, then returns to the caller's. */
  scoped<T>(key: string, draw: () => T): T {
    const outer = { slots: this.slots, cursor: this.cursor, deps: this.deps, effect: this.effect, cleanups: this.cleanups };
    const store = this.stores[key] ??= { slots: [], deps: [], cleanups: [] };
    Object.assign(this, { slots: store.slots, cursor: 0, deps: store.deps, effect: 0, cleanups: store.cleanups });
    try { return draw(); } finally { Object.assign(this, outer); }
  },
}));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  useState: <T,>(initial: T | (() => T)) => model.useState(initial),
  useRef: <T,>(initial: T) => model.useState(() => ({ current: initial }))[0],
  useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) => model.useEffect(effect, deps),
  useCallback: <T,>(callback: T) => callback,
  useMemo: <T,>(factory: () => T) => factory(),
  useContext: () => undefined,
  useSyncExternalStore: <T,>(_subscribe: unknown, read: () => T) => read() }));
vi.mock("react-native", () => ({ Platform: { get OS() { return model.platform; } }, useWindowDimensions: () => ({ width: 390, height: 844, fontScale: 1 }), ScrollView: "ScrollView", View: "View", Text: "Text",
  Pressable: "Pressable", Image: "Image", PanResponder: { create: () => ({ panHandlers: {} }) }, BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => model.rpc[contract.name], useHosts: () => model.hosts, useSettings: () => model.settings, getPaseoClient: () => ({}) }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon", useToast: () => model.toast,
  Modal: Object.assign(function Modal() { return null; }, { Content: function ModalContent() { return null; } }), copyText: (text: string) => model.copyText(text) }));
vi.mock("../client/mobile-filters", () => ({ focusControl: model.focusControl }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: (args: object) => { model.directoryArgs.push(args); return { state: model.directory, refresh: () => {} }; } }));

import { createAvailability, popoverBodyHeight, StartSwitchPopover, StartSwitchSheet, switchList, switchRows } from "../client/start-switch-sheet";
import { NewLsaForm } from "../client/new-lsa-form";
import { DenseSessionRow } from "../client/dense-session-row";
import { OrganizationRowView } from "../client/organization-row";
import { openHandOff, takeHandOff, withdrawHandOff, type OpenSessionResult, type SessionNavigation } from "../client/session-actions";
import { AnchoredMenu } from "../client/anchored-menu";
import { configureRecentStorage, recentRows, recordVisit, readRecent } from "../client/recent-sessions";
import { startSwitchCounts, type SwitchView } from "../client/session-state-shortcuts";
import { matchesMembership, matchesQuery, matchesStatus } from "../client/session-view";
import { Action } from "../client/page-controls";
import { sessionEntry } from "./session-fixtures";
import { DEFAULT_RECOVERY_TARGET, type LifecycleRow } from "../shared/session-lifecycle";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { UNSUPPORTED } from "../shared/session-navigator";

type Element = ReactElement<Record<string, unknown>>;
/** Function components drawn in place, each on its own hook store, so their controls and labels are visible. */
const DRAWN = ["SwitchStrip", "StateToolbar", "MorePanel", "DeviceNote", "DenseSessionRow", "SessionActionsMenu", "ActionRow", "OrganizationRowView", "SwitchDetails", "SessionDetailSection"];
const drawn = new WeakMap<object, ReactNode>();
function elements(value: ReactNode): Element[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(value)) return [];
  const type = value.type as string | ((props: object) => ReactNode);
  if (typeof type === "function" && DRAWN.includes(type.name)) {
    if (!drawn.has(value)) drawn.set(value, model.scoped(`${type.name}:${value.key ?? (value.props.entry as DirectoryEntry | undefined)?.id ?? ""}`, () => type(value.props)));
    return [value, ...elements(drawn.get(value))];
  }
  return [value, ...elements(value.props.children as ReactNode), ...elements(value.props.header as ReactNode)];
}
const text = (tree: Element[]) => tree.filter((node) => node.type === "Text").map((node) => [node.props.children].flat(Infinity).join("")).join("\n");
const byTestId = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id);
const action = (tree: Element[], label: string) => tree.find((node) => node.type === Action && node.props.label === label);
const press = (node: Element | undefined) => (node!.props.onPress as () => void)();
/** The state toolbar's drawn row, which measures itself. */
const stateRow = (tree: Element[]) => tree.find((node) => node.props.testID === "kontor-switch-states" && node.props.onLayout)!;
/** Lays the state toolbar out at `width`, as its container measures it, and draws again. */
const measure = (view: { tree: Element[]; render(): unknown }, width: number) => { (stateRow(view.tree).props.onLayout as (event: object) => void)({ nativeEvent: { layout: { width } } }); view.render(); };
/** The drawn subtree under a testID, such as one session row. */
const within = (tree: Element[], id: string) => elements(byTestId(tree, id));
const rowIds = (tree: Element[]) => tree.filter((node) => node.type === DenseSessionRow).map((node) => (node.props.entry as DirectoryEntry).id);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const NOW = Date.parse("2026-10-22T12:00:00.000Z");
const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const OPTIONS: CreateOptions = {
  enabled: true, disabledReason: null, pending: null, notes: [], catalogProjectId: "prj-adam", issues: [], jiraProblem: null,
  desks: [{ projectId: "prj-adam", project: "DESK • ADAM", desk: "ADAM", kind: "adam", cwd: "/fixtures/adam", lead: { ADHOC: "wks-adhoc", LSA: "wks-lsa", TPM: "wks-tpm" } }],
  providers: [{ id: "codex", label: "Codex", status: "ready", available: true, note: null, persona: "supported", defaultModel: "gpt-6-astra", defaultMode: "auto",
    models: [{ id: "gpt-6-astra", label: "GPT-6 Astra", efforts: [], defaultEffort: null }], modes: [{ id: "auto", label: "Default permissions", dangerous: false }] }],
  profile: { name: "lsa", provider: "codex", model: "gpt-6-astra", modeId: "auto", effort: null },
};
const outcome = (state: CreateOutcome["state"], patch: Partial<CreateOutcome> = {}): CreateOutcome => ({ requestId: "00000000-0000-4000-8000-000000000001", state, title: "LSA • ADHOC • Route review",
  agentId: state === "verified" || state === "differences" ? "agent-new" : null, workspaceId: "wks-adhoc", message: `Fixture ${state}.`, differences: [], persona: "supported", checkedAt: "2026-10-22", ...patch });
const prompted = (id: string, at: number, overrides: Parameters<typeof sessionEntry>[1] = {}) => ({ ...sessionEntry(id, overrides), lastUserMessageAt: new Date(at).toISOString() });
const directory = (entries: DirectoryEntry[], live = true): DirectoryState => ({ entries, status: "ready", live, lastUpdatedAt: NOW, error: null, staleReason: null, pages: 1, includeArchived: false });
const complete = (entries: DirectoryEntry[]): DirectoryState => ({ ...directory(entries), inventoryComplete: true });
/** 23 running sessions, two of them Billing; one idle and one archived. */
const RUNNING = [
  ...Array.from({ length: 21 }, (_, index) => sessionEntry(`run-${index}`, { status: "running", title: `Route review ${index}` })),
  sessionEntry("billing-1", { status: "running", title: "Billing fix" }), sessionEntry("billing-2", { status: "running", title: "Billing audit" }),
  sessionEntry("idle", { title: "Route review idle" }), sessionEntry("archived", { status: "running", title: "Billing archived", archivedAt: "2026-10-22" }),
];

/** 25 "Route review N" sessions, Billing and an archived Route review; only agent-3, agent-17, agent-24, Billing and the archive were visited, agent-3 last. */
function visitedRoutes(): DirectoryEntry[] {
  const entries = Array.from({ length: 25 }, (_, index) => sessionEntry(`agent-${index}`, { title: `Route review ${index}` }));
  entries.push(sessionEntry("agent-other", { title: "Billing" }), sessionEntry("agent-archived", { title: "Route review old", archivedAt: "2026-10-22" }));
  ["agent-24", "agent-17", "agent-archived", "agent-other", "agent-3"].forEach((id, index) => recordVisit("host-a", id, NOW - (5 - index) * 60_000));
  return entries;
}

beforeEach(() => {
  model.unmount();
  model.slots = []; model.cursor = 0; model.effect = 0; model.deps = []; model.stores = {};
  model.focusControl.mockClear();
  model.platform = "web";
  model.hosts = [{ serverId: "host-a", label: "Host A", status: "online" }];
  model.toast = { error: vi.fn(), show: vi.fn() };
  model.directoryArgs = [];
  model.settings = { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "rev-1", save: vi.fn(async () => true) };
  model.copyText = vi.fn(async () => {});
  withdrawHandOff();
  // The navigator's own owners are absent here, as on a host without them: every capability names why.
  model.rpc = { "sessions.create-options": vi.fn(async () => OPTIONS), "sessions.create": vi.fn(), "sessions.create-check": vi.fn(),
    "sessions.navigator-capabilities": vi.fn(async () => UNSUPPORTED), "sessions.usage-totals": vi.fn(async () => ({})) };
  configureRecentStorage(null);
});

describe("Start or switch rows", () => {
  it("lists the first eight Recent rows in saved open order with their place and reason, none archived, and counts them all", () => {
    const entries: DirectoryEntry[] = Array.from({ length: 10 }, (_, index) => prompted(`agent-${index}`, NOW - (index + 1) * 60_000, { projectName: "DESK • ADAM", workspaceName: "ADAM" }));
    entries.forEach((entry, index) => recordVisit("host-a", entry.id, NOW - (index + 1) * 60_000));
    entries.push(sessionEntry("agent-archived", { archivedAt: "2026-10-22" }));
    recordVisit("host-a", "agent-archived", NOW); recordVisit("host-a", "agent-9", NOW - 1000);
    const { rows, total } = switchRows(entries, "", NOW, readRecent("host-a", NOW).record);
    expect(total).toBe(10);
    expect(rows.map((row) => row.entry.id)).toEqual(["agent-9", "agent-8", "agent-7", "agent-6", "agent-5", "agent-4", "agent-3", "agent-2"]);
    expect(rows[0]!.detail).toMatch(/^Opened .+ · DESK • ADAM \/ ADAM$/);
    expect(rows[1]!.detail).toMatch(/^Opened .+ · DESK • ADAM \/ ADAM$/);
    expect(switchRows(entries, "", NOW, readRecent("host-a", NOW).record, true).rows.map((row) => row.entry.id))
      .toEqual(["agent-9", "agent-8", "agent-7", "agent-6", "agent-5", "agent-4", "agent-3", "agent-2", "agent-1", "agent-0"]);
  });

  it("narrows Last visited by every token in visit order, never adding unvisited or archived sessions", () => {
    const { rows, total } = switchRows(visitedRoutes(), "  review   ROUTE ", NOW, readRecent("host-a", NOW).record);
    expect(rows.map((row) => row.entry.id)).toEqual(["agent-3", "agent-17", "agent-24"]);
    expect(total).toBe(3);
  });

  it("says why New lead is not offered", () => {
    expect(createAvailability({ status: "offline" })).toBe("New lead: host offline. Nothing can be created until it reconnects.");
    expect(createAvailability({ status: "loading" })).toBeNull();
    expect(createAvailability({ status: "error", message: "timeout" })).toBe("New lead unavailable: Kontor could not read this host's options (timeout).");
    expect(createAvailability({ status: "ready", options: { ...OPTIONS, enabled: false, disabledReason: "New lead is off on this host." } })).toBe("New lead is off on this host.");
    expect(createAvailability({ status: "ready", options: OPTIONS })).toBeNull();
  });
});

describe("Start or switch sheet", () => {
  function sheet(props: Partial<Parameters<typeof StartSwitchSheet>[0]> = {}) {
    recordVisit("host-a", "agent-a", Date.now() - 60_000);
    const onOpen = vi.fn(), onOpenCreated = vi.fn(() => ({ ok: true } as const)), onShowView = vi.fn(), onClose = vi.fn();
    const all = { anchor: { current: null }, root: { current: null }, rootSize: { width: 1440, height: 900 }, compact: false, colors: COLORS, hostId: "host-a", hostStatus: "online" as const,
      directory: directory([prompted("agent-a", Date.now() - 60_000, { title: "LSA • ASMA-1 • Route" })]), canOpen: true, onOpen, onOpenCreated, onShowView, onClose, ...props };
    let tree: Element[] = [];
    const render = () => { model.cursor = 0; model.effect = 0; tree = elements(StartSwitchSheet(all)); return tree; };
    render();
    return { render, props: all, onOpen, onShowView, onClose, get tree() { return tree; } };
  }
  const choose = (s: ReturnType<typeof sheet>, testID: string) => { press(byTestId(s.tree, testID)); s.render(); };
  const search = (s: ReturnType<typeof sheet>, value: string) => { (byTestId(s.tree, "kontor-switch-search")!.props.onChangeText as (value: string) => void)(value); s.render(); };

  it("is a labelled dialog with New lead, search and rows that open by exact ID beside their own actions", async () => {
    const s = sheet();
    await settle(); s.render();
    expect(s.tree[0]!.props).toMatchObject({ testID: "kontor-switch-sheet", dialog: true, label: "Start or switch session", placement: { align: "end", width: 420, maxHeight: 640 } });
    expect(text(s.tree)).toContain("Start or switch session\nThis host");
    expect(byTestId(s.tree, "kontor-switch-new-lead")?.props).toMatchObject({ accessibilityRole: "button", accessibilityLabel: "New lead" });
    expect(action(s.tree, "Close")?.props.accessibilityLabel).toBe("Close start or switch");
    expect(byTestId(s.tree, "kontor-switch-search")?.props).toMatchObject({ accessibilityLabel: "Find a session", placeholder: "Find a session…", autoFocus: true });
    expect(byTestId(s.tree, "kontor-switch-list")?.props).toMatchObject({ role: "list", accessibilityLabel: "Last visited sessions" });
    const row = within(s.tree, "session-row-agent-a");
    expect(byTestId(row, "session-row-body")?.props).toMatchObject({ role: "button", accessibilityLabel: expect.stringMatching(/^LSA • ASMA-1 • Route\./) });
    expect(byTestId(row, "session-actions-agent-a")?.props).toMatchObject({ accessibilityRole: "button", accessibilityLabel: "Actions for LSA • ASMA-1 • Route", "aria-haspopup": "menu" });
    press(byTestId(row, "session-row-body"));
    expect(s.onOpen).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ id: "agent-a" }));
  });

  it("orders a row's menu as the board does, keeps unsupported operations listed with their reason, and runs none until one is chosen", async () => {
    const s = sheet();
    await settle(); s.render();
    expect(byTestId(s.tree, "session-action-archive")).toBeUndefined();
    choose(s, "session-actions-agent-a");
    const items = within(s.tree, "session-row-agent-a").filter((node) => node.props.role === "menuitem");
    expect(items.map((node) => node.props.testID)).toEqual(["session-action-takeover", "session-action-takeover-option", "session-action-favorite", "session-action-summarize",
      "session-action-attach", "session-action-copy-id", "session-action-details", "session-action-select", "session-action-archive"]);
    expect(items.filter((node) => !node.props["aria-disabled"]).map((node) => node.props.accessibilityLabel)).toEqual(["Add to favorites",
      "Copy exact session ID", "Details", "Select sessions", "Archive"]);
    // This host's server qualifies no takeover and has no summary store: each stays listed, refused with its reason.
    expect(items.filter((node) => node.props["aria-disabled"]).map((node) => node.props.accessibilityLabel)).toEqual([`Take over. ${UNSUPPORTED.takeover.reason}`,
      `Choose a model for taking over LSA • ASMA-1 • Route. ${UNSUPPORTED.takeover.reason}`,
      `Summarize & copy. ${UNSUPPORTED.summaries.reason}`, `Attach summary to another seat…. ${UNSUPPORTED.summaries.reason}`]);
    press(byTestId(s.tree, "session-action-copy-id")); await settle(); s.render();
    expect(model.copyText).toHaveBeenCalledExactlyOnceWith("agent-a");
    expect(model.toast.show).toHaveBeenCalledExactlyOnceWith("Copied session ID agent-a");
    expect(byTestId(s.tree, "session-action-archive")).toBeUndefined();
    choose(s, "session-actions-agent-a");
    press(byTestId(s.tree, "session-action-favorite")); await settle();
    expect(model.settings.save).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ favorites: ["agent-a"] }), "rev-1");
    expect(s.onOpen).not.toHaveBeenCalled();
  });

  it("opens a row's Details from its menu with the full title, exact ID, exact times and unknown totals, and Close leaves the list", async () => {
    const s = sheet();
    await settle(); s.render();
    choose(s, "session-actions-agent-a");
    press(byTestId(s.tree, "session-action-details")); s.render();
    const details = within(s.tree, "kontor-switch-details");
    const shown = details.filter((node) => node.type === "Text").map((node) => [node.props.children].flat(Infinity).join(""));
    expect(shown).toEqual(expect.arrayContaining(["LSA • ASMA-1 • Route", "agent-a", "Opened from Kontor on this device", "Unknown: no supported read reports this session's totals."]));
    (byTestId(s.tree, "kontor-switch-details")!.props.footer as { props: { secondary: { onPress(): void } } }).props.secondary.onPress(); s.render();
    expect(byTestId(s.tree, "kontor-switch-details")).toBeUndefined();
    expect(s.onOpen).not.toHaveBeenCalled();
  });

  it("asks for the desk of the project context it was opened in", async () => {
    const s = sheet({ initialScope: { projectId: "prj-adam" } });
    await settle();
    expect(model.rpc["sessions.create-options"]).toHaveBeenCalledWith({ projectId: "prj-adam" });
    expect(s.tree[0]!.props.testID).toBe("kontor-switch-sheet");
  });

  const archiveRow = (patch: Partial<LifecycleRow> = {}) => ({ id: "agent-a", title: "LSA • ASMA-1 • Route", workspaceId: "wks-a", cwd: "/fixture", provider: "codex", model: null, effort: null, mode: null,
    nativeStatus: "idle", fingerprint: "fixture", category: "candidate", eligible: true, reason: "Fixture eligible.", children: [], outcome: "pending", message: null, ...patch }) as LifecycleRow;
  function archiveFixture(execute = vi.fn(async () => archiveRow({ outcome: "archived", runtime: "closed", availability: { agentId: "agent-a", state: "readable", archivedAt: "2026-10-25", checkedAt: "2026-10-25", revision: "fixture", reason: "Exact read." } })), previewRow = archiveRow()) {
    const preview = vi.fn(async () => ({ id: "00000000-0000-4000-8000-000000000002", action: "archive", scope: "filtered", createdAt: "2026-10-25", target: DEFAULT_RECOVERY_TARGET, rows: [previewRow] }));
    model.rpc["sessions.lifecycle-preview"] = preview; model.rpc["sessions.lifecycle-execute-item"] = execute;
    return { preview, execute };
  }
  /** Opens agent-a's menu and returns its Archive item as drawn. */
  const archiveItem = (s: ReturnType<typeof sheet>) => { choose(s, "session-actions-agent-a"); return byTestId(s.tree, "session-action-archive")!; };
  it("Archive reads nothing on mount and performs one exact-ID existing preview/execute despite repeated taps, leaving Open alone", async () => {
    let finish!: (value: LifecycleRow) => void;
    const execute = vi.fn(() => new Promise<LifecycleRow>((resolve) => { finish = resolve; }));
    const rpc = archiveFixture(execute), s = sheet();
    expect(rpc.preview).not.toHaveBeenCalled(); expect(execute).not.toHaveBeenCalled();
    const archive = archiveItem(s).props.onPress as () => void;
    archive(); archive(); await settle(); s.render();
    expect(archiveItem(s).props).toMatchObject({ "aria-disabled": true, disabled: true, accessibilityLabel: "Archive. Archive is not available for this session right now." });
    expect(byTestId(s.tree, "kontor-switch-archiving")?.props).toMatchObject({ role: "status", "aria-busy": true, children: "Archiving LSA • ASMA-1 • Route…" });
    expect(rpc.preview).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ action: "archive", scope: "filtered", agentIds: ["agent-a"] }));
    expect(execute).toHaveBeenCalledExactlyOnceWith({ reviewId: "00000000-0000-4000-8000-000000000002", agentId: "agent-a" });
    expect(s.onOpen).not.toHaveBeenCalled(); expect(s.onClose).not.toHaveBeenCalled();
    finish(archiveRow({ outcome: "archived", runtime: "closed", availability: { agentId: "agent-a", state: "readable", archivedAt: "2026-10-25", checkedAt: "2026-10-25", revision: "fixture", reason: "Confirmed." } }));
    await settle(); s.render();
    expect(rowIds(s.tree)).not.toContain("agent-a");
    expect(text(s.tree)).toContain("Archived LSA • ASMA-1 • Route.");
  });
  it("Archive retains a refused row and reports its existing reason without dispatch", async () => {
    const rpc = archiveFixture(undefined, archiveRow({ eligible: false, outcome: "blocked", reason: "A managed child is protected." })), s = sheet();
    press(archiveItem(s)); await settle(); s.render();
    expect(rpc.execute).not.toHaveBeenCalled(); expect(rowIds(s.tree)).toContain("agent-a");
    expect(byTestId(s.tree, "kontor-switch-archive-result")?.props).toMatchObject({ accessibilityRole: "alert", children: "A managed child is protected." });
  });
  it("Archive keeps a newer unarchived native row when an older archive response settles", async () => {
    let finish!: (value: LifecycleRow) => void;
    const original = sessionEntry("agent-a", { title: "Exact session", updatedAt: "2026-10-25T12:00:00Z" });
    const newer = { ...original, updatedAt: "2026-10-25T12:02:00Z" };
    const rpc = archiveFixture(vi.fn(() => new Promise<LifecycleRow>((resolve) => { finish = resolve; })));
    const s = sheet({ directory: directory([original]) });
    press(archiveItem(s)); await settle(); s.render();
    expect(archiveItem(s).props["aria-disabled"]).toBe(true);
    s.props.directory = directory([newer]); s.render();
    finish(archiveRow({ title: original.title, outcome: "archived", runtime: "closed", availability: { agentId: original.id, state: "readable", archivedAt: "2026-10-25T12:01:00Z", checkedAt: "2026-10-25T12:01:00Z", revision: original.updatedAt, reason: "Older exact-ID read." } }));
    await settle(); s.render();
    expect(byTestId(within(s.tree, "session-row-agent-a"), "session-row-body")?.props.accessibilityLabel).toContain("Exact session");
    expect(s.props.directory.entries[0]).toEqual(newer);
    expect(text(s.tree)).not.toContain("Archived Exact session");
    expect(text(s.tree)).toContain("current native state");
    expect(action(s.tree, "Open Sessions to review archive")).toBeDefined();
    expect(archiveItem(s).props["aria-disabled"]).toBeUndefined();
    expect(rpc.preview).toHaveBeenCalledTimes(1); expect(rpc.execute).toHaveBeenCalledTimes(1);
    expect(s.onOpen).not.toHaveBeenCalled(); expect(s.onClose).not.toHaveBeenCalled();
  });
  it("Archive never calls an uncertain reply success and offers the existing Sessions review path without resending", async () => {
    const rpc = archiveFixture(vi.fn(async () => archiveRow({ outcome: "uncertain", runtime: "unconfirmed", availability: { agentId: "agent-a", state: "readable", archivedAt: null, checkedAt: "2026-10-25", revision: "fixture", reason: "Not archived on this read." } }))), s = sheet();
    press(archiveItem(s)); await settle(); s.render();
    expect(archiveItem(s).props["aria-disabled"]).toBe(true); expect(rowIds(s.tree)).toContain("agent-a");
    expect(text(s.tree)).not.toContain("Archived LSA"); expect(text(s.tree)).toContain("archive result unknown");
    press(action(s.tree, "Open Sessions to review archive"));
    expect(s.onShowView).toHaveBeenCalledExactlyOnceWith({ status: "all", query: "", membership: "sessions" }); expect(rpc.execute).toHaveBeenCalledTimes(1);
  });
  it("Archive keeps a lost reply visible and requires review before another send", async () => {
    const rpc = archiveFixture(vi.fn(async () => { throw new Error("Reply lost"); })), s = sheet();
    press(archiveItem(s)); await settle(); s.render();
    expect(text(s.tree)).toContain("Archive did not report a result: Reply lost");
    expect(archiveItem(s).props["aria-disabled"]).toBe(true);
    press(archiveItem(s)); await settle(); s.render();
    expect(rpc.preview).toHaveBeenCalledTimes(1); expect(rpc.execute).toHaveBeenCalledTimes(1);
    expect(rowIds(s.tree)).toContain("agent-a");
    expect(action(s.tree, "Open Sessions to review archive")).toBeDefined();
  });
  it("Archive follows confirmed native archive membership while keeping runtime-unconfirmed wording", async () => {
    archiveFixture(vi.fn(async () => archiveRow({ outcome: "archived", runtime: "unconfirmed", availability: { agentId: "agent-a", state: "readable", archivedAt: "2026-10-25", checkedAt: "2026-10-25", revision: "fixture", reason: "Membership only." } })));
    const s = sheet(); press(archiveItem(s)); await settle(); s.render();
    expect(rowIds(s.tree)).not.toContain("agent-a");
    expect(text(s.tree)).toContain("runtime stop not confirmed"); expect(text(s.tree)).not.toContain("Archived LSA");
  });
  it.each([false, true])("Archive rejects late results after a host switch, including A→B→A (return=%s)", async (returnToFirstHost) => {
    let finish!: (value: LifecycleRow) => void;
    archiveFixture(vi.fn(() => new Promise<LifecycleRow>((resolve) => { finish = resolve; })));
    const s = sheet(); press(archiveItem(s)); await settle();
    s.props.hostId = "host-b"; s.render();
    if (returnToFirstHost) { s.props.hostId = "host-a"; s.render(); }
    finish(archiveRow({ outcome: "archived", runtime: "closed", availability: { agentId: "agent-a", state: "readable", archivedAt: "2026-10-25", checkedAt: "2026-10-25", revision: "fixture", reason: "Old host result." } }));
    await settle(); s.render();
    expect(byTestId(s.tree, "kontor-switch-archive-result")).toBeUndefined();
    expect(s.props.directory.entries[0]!.archivedAt).toBeNull();
  });

  const STATES = ["recent", "activity", "running", "needs_input", "error", "all", "idle", "attention", "favorites"];
  const tiles = (s: ReturnType<typeof sheet>) => within(s.tree, "kontor-switch-states").filter((node) => String(node.props.testID).startsWith("kontor-switch-state-"));
  const ids = (s: ReturnType<typeof sheet>) => tiles(s).map((node) => String(node.props.testID).slice("kontor-switch-state-".length));

  it.each([[296, 4], [366, 5], [524, 9], [600, 9]] as const)("fits whole 44pt state tiles in a %ipx row, with More and Kontor pinned after them", (width, shown) => {
    const s = sheet();
    measure(s, width);
    const strip = stateRow(s.tree), [states, more, kontor] = strip.props.children as Element[];
    expect(strip.props).toMatchObject({ role: "toolbar", accessibilityLabel: "Session states", style: { flexDirection: "row", alignItems: "center", gap: 4 } });
    expect(ids(s)).toEqual(STATES.slice(0, shown));
    // Whole tiles only: each 44pt tile and its 4pt gap, then More and Kontor at 44pt each, fit the measured row.
    expect(shown * 48 + 2 * 48 - 4).toBeLessThanOrEqual(width);
    expect(tiles(s).map((node) => node.props.target)).toEqual(Array(shown).fill(44));
    expect([more!.props.testID, kontor!.props.testID, more!.props.target, kontor!.props.target]).toEqual(["kontor-switch-more", "kontor-switch-kontor", 44, 44]);
    expect(states!.props.style).toEqual({ flex: 1, minWidth: 0, flexDirection: "row", gap: 4 });
    expect(within(s.tree, "kontor-switch-states").some((node) => node.type === "ScrollView")).toBe(false);
    expect(byTestId(s.tree, "kontor-switch-kontor")?.props).toMatchObject({ icon: "Building2", caption: "Kontor", label: "Open in Kontor" });
    expect(tiles(s).map((node) => [node.props.label, node.props.count])).toEqual(STATES.slice(0, shown).map((state) => [expect.stringMatching(/^[A-Z].*, .+/), expect.objectContaining({ label: expect.any(String) })]));
  });

  it("names each tile with its exact count and keeps the original icons and the selected check on Last visited", () => {
    const s = sheet();
    measure(s, 600);
    expect(tiles(s).map((node) => [String(node.props.label).split(",")[0], node.props.icon, node.props.selected])).toEqual([
      ["Last visited", "Clock", true], ["Last activity", "Activity", false], ["Running", "Play", false], ["Questions", "MessageCircleQuestion", false], ["Errors", "CircleAlert", false],
      ["All", "LayoutGrid", false], ["Idle", "Pause", false], ["Attention", "Bell", false], ["Favorites", "Star", false]]);
  });

  it("keeps a state chosen past the tiles that fit in the last tile, once, and through narrow, wide and narrow again", () => {
    const s = sheet({ directory: complete(RUNNING) });
    measure(s, 296);
    choose(s, "kontor-switch-more");
    choose(s, "kontor-switch-more-favorites");
    expect(byTestId(s.tree, "kontor-switch-more-panel")).toBeUndefined();
    expect(ids(s)).toEqual(["recent", "activity", "running", "favorites"]);
    expect(tiles(s).filter((node) => node.props.selected).map((node) => node.props.testID)).toEqual(["kontor-switch-state-favorites"]);
    search(s, " billing ");
    measure(s, 600);
    expect(ids(s)).toEqual(STATES);
    expect(byTestId(s.tree, "kontor-switch-state-favorites")?.props.selected).toBe(true);
    measure(s, 296);
    expect(ids(s)).toEqual(["recent", "activity", "running", "favorites"]);
    expect(byTestId(s.tree, "kontor-switch-search")?.props.value).toBe(" billing ");
    expect(byTestId(s.tree, "kontor-switch-list")?.props.accessibilityLabel).toBe("Favorites sessions matching billing");
  });

  it("shows a state chosen under More on More, never as a tile", () => {
    const s = sheet();
    measure(s, 296);
    choose(s, "kontor-switch-more"); choose(s, "kontor-switch-more-current");
    expect(byTestId(s.tree, "kontor-switch-more")?.props).toMatchObject({ selected: true, label: "More: Current" });
    expect(ids(s)).toEqual(["recent", "activity", "running", "needs_input"]);
    expect(tiles(s).some((node) => node.props.selected)).toBe(false);
  });

  it("filters Running in place without a hand-off, showing at most 20 of every match", () => {
    const s = sheet({ directory: complete(RUNNING) });
    choose(s, "kontor-switch-state-running");
    expect(byTestId(s.tree, "kontor-switch-state-running")?.props.selected).toBe(true);
    expect(byTestId(s.tree, "kontor-switch-list")?.props.accessibilityLabel).toBe("Running sessions");
    expect(rowIds(s.tree)).toHaveLength(20);
    expect(rowIds(s.tree).filter((id) => !id.startsWith("run-") && !id.startsWith("billing-"))).toEqual([]);
    expect(byTestId(s.tree, "kontor-switch-count")?.props.children).toBe("20 of 23");
    expect(s.onShowView).not.toHaveBeenCalled(); expect(s.onOpen).not.toHaveBeenCalled();
  });

  it("reveals the rest of a capped list in place, in the same order, and starts short again for another state or search", () => {
    const s = sheet({ directory: complete(RUNNING) });
    choose(s, "kontor-switch-state-running");
    const first = rowIds(s.tree);
    press(action(s.tree, "Show 3 more")); s.render();
    expect(rowIds(s.tree)).toHaveLength(23);
    expect(rowIds(s.tree).slice(0, 20)).toEqual(first);
    expect(new Set(rowIds(s.tree)).size).toBe(23);
    expect(byTestId(s.tree, "kontor-switch-count")?.props.children).toBe("23 of 23");
    expect(action(s.tree, "Show 3 more")).toBeUndefined();
    choose(s, "kontor-switch-state-recent"); choose(s, "kontor-switch-state-running");
    expect(byTestId(s.tree, "kontor-switch-count")?.props.children).toBe("20 of 23");
  });

  it("narrows the chosen state by the search and refines its count", () => {
    const s = sheet({ directory: complete(RUNNING) });
    choose(s, "kontor-switch-state-running");
    search(s, " billing ");
    expect(rowIds(s.tree).sort()).toEqual(["billing-1", "billing-2"]);
    expect(byTestId(s.tree, "kontor-switch-count")?.props.children).toBe("2 of 2");
    expect(byTestId(s.tree, "kontor-switch-list")?.props.accessibilityLabel).toBe("Running sessions matching billing");
    expect(byTestId(s.tree, "kontor-switch-state-running")?.props.count).toMatchObject({ text: "2/23" });
    search(s, "nothing like it");
    expect(byTestId(s.tree, "kontor-switch-empty")?.props.children).toBe("No Running session matches “nothing like it”.");
  });

  // R9b: a search, state or layout change drops a waiting state-icon open even while that row stays listed; a lone tap still opens.
  it("drops a waiting state-icon open when the search, state, layout or a group changes with the row still listed", () => {
    vi.useFakeTimers();
    try {
      const s = sheet({ directory: complete([prompted("agent-a", Date.now() - 60_000, { title: "LSA • ASMA-1 • Route" })]) });
      const tap = () => press(byTestId(within(s.tree, "session-row-agent-a"), "session-row-state"));
      const layout = (next: string) => { (s.tree.find((node) => (node.type as { name?: string }).name === "SessionLayoutControl")!.props.onChange as (value: string) => void)(next); s.render(); };
      measure(s, 600);
      for (const intent of [() => search(s, "route"), () => choose(s, "kontor-switch-state-all"), () => layout("flat"), () => search(s, "")]) {
        tap(); intent();
        expect(rowIds(s.tree)).toContain("agent-a");
        vi.advanceTimersByTime(600);
        expect(s.onOpen).not.toHaveBeenCalled();
      }
      tap(); vi.advanceTimersByTime(600);
      expect(s.onOpen).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ id: "agent-a" }));
    } finally { vi.useRealTimers(); }
  });

  it("hands Kontor the chosen state and trimmed search in ordinary Sessions", () => {
    const s = sheet({ directory: complete(RUNNING) });
    choose(s, "kontor-switch-state-running");
    search(s, " billing ");
    press(byTestId(s.tree, "kontor-switch-kontor"));
    expect(s.onShowView).toHaveBeenCalledExactlyOnceWith({ status: "running", query: "billing", membership: "sessions" });
    expect(s.onOpen).not.toHaveBeenCalled();
  });

  it("opens More with the other states and archived sessions; Archived hands Kontor the search in archived membership", () => {
    const s = sheet();
    expect(byTestId(s.tree, "kontor-switch-more-panel")).toBeUndefined();
    choose(s, "kontor-switch-more");
    expect(byTestId(s.tree, "kontor-switch-more")?.props.expanded).toBe(true);
    expect(within(s.tree, "kontor-switch-more-panel").filter((node) => node.props.onPress).map((node) => node.props.label)).toEqual(["Last visited", "Last activity", "Running", "Questions",
      "Errors", "All", "Idle", "Attention", "Favorites", "Current", "Starting", "Closed", "Earlier errors", "Provider blocked (observed)", "Archived sessions"]);
    expect(within(s.tree, "kontor-switch-more-panel").filter((node) => node.props.role === "group").map((node) => node.props.accessibilityLabel)).toEqual(["All states", "More states"]);
    expect(byTestId(s.tree, "kontor-switch-more-recent")?.props).toMatchObject({ selected: true, icon: "Clock" });
    search(s, " route ");
    press(byTestId(s.tree, "kontor-switch-more-archived"));
    expect(s.onShowView).toHaveBeenCalledExactlyOnceWith({ status: "all", query: "route", membership: "archived" });
  });

  it("filters a More state in place and names it on More, but sends Provider blocked to Kontor's exact reads", () => {
    const s = sheet({ directory: complete([...RUNNING, sessionEntry("done", { status: "closed", title: "Route review done" })]) });
    choose(s, "kontor-switch-more"); choose(s, "kontor-switch-more-closed");
    expect(byTestId(s.tree, "kontor-switch-more-panel")).toBeUndefined();
    expect(byTestId(s.tree, "kontor-switch-more")?.props).toMatchObject({ label: "More: Closed", selected: true });
    expect(rowIds(s.tree)).toEqual(["done"]);
    choose(s, "kontor-switch-more"); choose(s, "kontor-switch-more-provider_blocked");
    expect(s.onShowView).toHaveBeenCalledExactlyOnceWith({ status: "provider_blocked", query: "", membership: "sessions" });
    expect(byTestId(s.tree, "kontor-switch-more")?.props.label).toBe("More: Closed");
    expect(rowIds(s.tree)).toEqual(["done"]);
  });

  it("orders Last activity by the last prompt or running turn, never by metadata updates", () => {
    const at = (minutes: number) => new Date(NOW - minutes * 60_000).toISOString();
    const s = sheet({ directory: directory([
      prompted("prompted-3h", NOW - 180 * 60_000, { updatedAt: at(0) }),
      sessionEntry("turn-1h", { status: "running", activeTurnStartedAt: at(60), updatedAt: at(300) }),
      prompted("prompted-2h", NOW - 120 * 60_000, { updatedAt: at(240) }),
      sessionEntry("renamed-only", { updatedAt: at(0) }),
    ]) });
    choose(s, "kontor-switch-state-activity");
    expect(rowIds(s.tree)).toEqual(["turn-1h", "prompted-2h", "prompted-3h"]);
    expect(byTestId(s.tree, "kontor-switch-count")?.props.children).toBe("3 of 3");
  });

  it("opens every group of a grouped state until it is collapsed here, with no favorite stars", () => {
    const s = sheet({ directory: complete(RUNNING) });
    choose(s, "kontor-switch-state-running");
    const headers = () => s.tree.filter((node) => node.type === OrganizationRowView && (node.props.row as { kind: string }).kind === "group")
      .map((node) => elements(node).find((child) => child.props["aria-expanded"] !== undefined)!);
    expect(headers().length).toBeGreaterThan(0);
    expect(headers().every((node) => node.props["aria-expanded"] === true)).toBe(true);
    expect(s.tree.some((node) => /^(Unfavorite|Favorite) /.test(String(node.props.accessibilityLabel)))).toBe(false);
    press(headers()[0]); s.render();
    expect(headers()[0]!.props["aria-expanded"]).toBe(false);
    expect(rowIds(s.tree)).toEqual([]);
  });

  it("offers Clear recent only on Last visited", () => {
    const s = sheet();
    expect(action(s.tree, "Clear recent")).toBeDefined();
    choose(s, "kontor-switch-state-running");
    expect(action(s.tree, "Clear recent")).toBeUndefined();
    choose(s, "kontor-switch-state-recent");
    expect(action(s.tree, "Clear recent")).toBeDefined();
  });

  it("marks the session it was opened from", () => {
    recordVisit("host-a", "agent-b", Date.now() - 600_000);
    const s = sheet({ currentId: "agent-a", directory: directory([prompted("agent-a", Date.now() - 60_000), sessionEntry("agent-b")]) });
    expect(text(within(s.tree, "session-row-agent-a"))).toContain("open now");
    expect(text(within(s.tree, "session-row-agent-b"))).toMatch(/open \d+m/);
    expect(byTestId(s.tree, "session-row-agent-a")?.props.style).toMatchObject({ borderLeftWidth: 2 });
    expect(byTestId(s.tree, "session-row-agent-b")?.props.style).toMatchObject({ borderLeftWidth: 0 });
  });

  it("covers the screen on a phone and does not steal focus there", () => {
    const s = sheet({ compact: true, rootSize: { width: 320, height: 640 } });
    expect(s.tree[0]!.props.placement).toEqual({ align: "stretch", width: 0, cover: true });
    expect(byTestId(s.tree, "kontor-switch-search")?.props.autoFocus).toBe(false);
  });

  it("restores trigger focus through the shared native helper on Close and panel dismissal", async () => {
    const s = sheet();
    press(action(s.tree, "Close"));
    expect(s.onClose).toHaveBeenCalledTimes(1);
    await settle();
    expect(model.focusControl).toHaveBeenCalledTimes(1);
    (s.tree[0]!.props.onClose as () => void)();
    await settle();
    expect(model.focusControl).toHaveBeenCalledTimes(2);
  });

  it("offers New lead only after the host's options say it is on, and opens the form in place", async () => {
    const s = sheet();
    expect(byTestId(s.tree, "kontor-switch-new-lead")).toBeUndefined();
    await settle(); s.render();
    expect(model.rpc["sessions.create-options"]).toHaveBeenCalledTimes(1);
    expect(text(within(s.tree, "kontor-switch-new-lead"))).toBe("New lead");
    choose(s, "kontor-switch-new-lead");
    expect(s.tree[0]!.props.label).toBe("New lead");
    const form = s.tree.find((node) => node.type === NewLsaForm)!;
    expect(form.props).toMatchObject({ options: { status: "ready", options: OPTIONS }, canOpen: true, hostId: "host-a", hostScrolls: false, pickerHeight: undefined });
    expect(form.props.onOpenCreated).toBeDefined();
    (form.props.onCancel as () => void)(); s.render();
    expect(byTestId(s.tree, "kontor-switch-list")).toBeDefined();
  });

  it("keeps the settled hint through a quick LSA→TPM→LSA switch, blocks Create until the latest read answers, and never applies a stale read", async () => {
    const reads: { scope: { role?: string }; answer: (options: CreateOptions) => void }[] = [];
    model.rpc["sessions.create-options"] = vi.fn((scope: { role?: string }) => new Promise<CreateOptions>((answer) => { reads.push({ scope, answer }); }));
    model.rpc["sessions.create-features"] = vi.fn(async () => ({ features: [], problem: null }));
    const s = sheet();
    reads.shift()!.answer(OPTIONS); await settle(); s.render();
    choose(s, "kontor-switch-new-lead");
    // Every frame the form draws: its visible hint, and whether Create is blocked and why.
    const frames: { hint: unknown; blocked: unknown; why: unknown }[] = [];
    let form: Element[] = [];
    const draw = () => {
      s.render();
      form = elements(model.scoped("form", () => NewLsaForm(s.tree.find((node) => node.type === NewLsaForm)!.props as Parameters<typeof NewLsaForm>[0])));
      const create = byTestId(form, "kontor-new-lsa-create")!;
      frames.push({ hint: byTestId(form, "kontor-new-lsa-problem")?.props.children ?? null, blocked: create.props.disabled, why: create.props.accessibilityHint ?? null });
    };
    // A choice re-renders, then its read's effect renders again, as React commits before an effect runs.
    const role = (next: string) => { (form.find((node) => node.props.label === "Role" && node.props.onSelect)!.props.onSelect as (id: string) => void)(next); draw(); draw(); };
    draw();
    (byTestId(form, "kontor-new-lsa-topic")!.props.onChangeText as (topic: string) => void)("Route review"); draw();
    expect(frames.at(-1)).toEqual({ hint: null, blocked: false, why: null });
    const before = frames.length;
    role("TPM"); role("LSA");
    expect(reads.map((read) => read.scope.role)).toEqual(["TPM", "LSA"]);
    const waiting = "Refreshing this desk's choices… Creation stays disabled until the host answers.";
    expect(frames.slice(before + 1).every((frame) => frame.blocked === true && frame.why === waiting)).toBe(true);
    // The latest read answers first; the earlier one arrives late and is never applied.
    const [stale, latest] = reads;
    latest!.answer(OPTIONS); await settle(); draw();
    stale!.answer({ ...OPTIONS, enabled: false, disabledReason: "Stale TPM read." }); await settle(); draw();
    expect(frames.at(-1)).toEqual({ hint: "Choose a Jira issue.", blocked: true, why: "Choose a Jira issue." });
    // The hint changes once, to what the switch settles on: no passing status, no stale or intermediate problem.
    const shown = frames.map((frame) => frame.hint).filter((hint, index, all) => index === 0 || hint !== all[index - 1]);
    expect(shown).toEqual(["Type a topic of at least 3 characters.", null, "Choose a Jira issue."]);
  });

  it.each(["dismissal", "Back", "Cancel"])("invalidates the creation continuation immediately on %s", async (leave) => {
    const s = sheet();
    await settle(); s.render();
    choose(s, "kontor-switch-new-lead");
    const form = s.tree.find((node) => node.type === NewLsaForm)!;
    const active = form.props.isActive as () => boolean;
    expect(active()).toBe(true);
    // The form carries no Close of its own; Escape or the backdrop dismisses the sheet.
    expect(action(s.tree, "Close")).toBeUndefined();
    if (leave === "Cancel") (form.props.onCancel as () => void)();
    else if (leave === "dismissal") (s.tree[0]!.props.onClose as () => void)();
    else press(s.tree.find((node) => node.props.label === "Back to sessions"));
    expect(active()).toBe(false);
  });

  it.each([
    ["the switch is off", async () => ({ ...OPTIONS, enabled: false, disabledReason: "New lead is off on this host. The operator turns it on at the host computer." }), "New lead is off on this host. The operator turns it on at the host computer."],
    ["the options read failed", async () => { throw new Error("handler crashed"); }, "New lead unavailable: Kontor could not read this host's options (handler crashed)."],
  ])("states why New lead is absent when %s", async (_label, read, message) => {
    model.rpc["sessions.create-options"] = vi.fn(read);
    const s = sheet();
    await settle(); s.render();
    expect(byTestId(s.tree, "kontor-switch-new-lead")).toBeUndefined();
    expect(byTestId(s.tree, "kontor-switch-create-status")?.props.children).toBe(message);
  });

  it("keeps catalog freshness in the switch sheet", async () => {
    const s = sheet({
      directory: {
        ...directory([prompted("agent-a", Date.now() - 60_000)]),
        source: "catalog",
        status: "stale",
        live: false,
        inventoryComplete: true,
      },
    });
    await settle();
    s.render();
    expect(byTestId(s.tree, "kontor-switch-freshness")).toBeDefined();
    expect(text(s.tree)).toContain("Last known · Catalog");
    expect(text(s.tree)).not.toContain("Last known · Stale");
  });

  it("asks nothing of an offline host and shows the last known list", () => {
    const s = sheet({ hostStatus: "offline", directory: directory([prompted("agent-a", Date.now() - 60_000)], false) });
    expect(model.rpc["sessions.create-options"]).not.toHaveBeenCalled();
    expect(byTestId(s.tree, "kontor-switch-create-status")?.props.children).toBe("New lead: host offline. Nothing can be created until it reconnects.");
    expect(byTestId(s.tree, "kontor-switch-freshness")).toBeDefined();
    expect(rowIds(s.tree)).toEqual(["agent-a"]);
  });

  it("keeps Check reachable for a pending creation after the operator turns creation off", async () => {
    model.rpc["sessions.create-options"]!.mockResolvedValue({ ...OPTIONS, enabled: false, disabledReason: "New lead is off on this host.", pending: outcome("unknown") });
    const s = sheet();
    await settle(); s.render();
    expect(byTestId(s.tree, "kontor-switch-new-lead")?.props.accessibilityLabel).toBe("Check pending session creation");
    expect(text(within(s.tree, "kontor-switch-new-lead"))).toBe("Check creation");
    choose(s, "kontor-switch-new-lead");
    expect(s.tree.find((node) => node.type === NewLsaForm)?.props.options).toMatchObject({ status: "ready", options: { enabled: false, pending: { state: "unknown" } } });
  });

  it("searches Last visited only, at most 20 of its matches with the full count, and says when nothing matches", () => {
    const visited = Array.from({ length: 22 }, (_, index) => sessionEntry(`agent-${index}`, { title: `Route review ${index}` }));
    for (const entry of visited) recordVisit("host-a", entry.id, Date.now() - 120_000);
    const s = sheet({ directory: directory([...visited, sessionEntry("unvisited", { title: "Route review elsewhere" })]) });
    search(s, " route ");
    expect(byTestId(s.tree, "kontor-switch-list")?.props.accessibilityLabel).toBe("Last visited sessions matching route");
    expect(rowIds(s.tree)).toHaveLength(20);
    expect(rowIds(s.tree)).not.toContain("unvisited");
    expect(byTestId(s.tree, "kontor-switch-count")?.props.children).toBe("20 of 22");
    search(s, "nothing like it");
    expect(byTestId(s.tree, "kontor-switch-empty")?.props.children).toBe("No session matches “nothing like it”.");
  });

  it("counts, lists and hands Kontor one Last visited set for a search", () => {
    const entries = visitedRoutes();
    const s = sheet({ directory: complete(entries) });
    search(s, " route ");
    const record = readRecent("host-a").record, recentIds = recentRows(entries, record).map((row) => row.entry.id);
    const listed = switchList(entries, "recent", " route ", Date.now(), record, []);
    expect(listed.total).toBe(3);
    expect(rowIds(s.tree)).toEqual(listed.rows.map((entry) => entry.id));
    expect(startSwitchCounts(complete(entries), "Live", { query: " route ", recent: recentIds }).recent.numerator).toBe(String(listed.total));
    expect(byTestId(s.tree, "kontor-switch-state-recent")?.props.count).toMatchObject({ numerator: String(listed.total) });
    press(byTestId(s.tree, "kontor-switch-kontor"));
    expect(s.onShowView).toHaveBeenCalledExactlyOnceWith({ status: "recent", query: "route", membership: "sessions" });
    const view = s.onShowView.mock.calls[0]![0] as SwitchView;
    const kontor = entries.filter((entry) => matchesMembership(entry, view.membership) && matchesStatus(entry, view.status, [], recentIds) && matchesQuery(entry, view.query));
    expect(kontor.map((entry) => entry.id).sort()).toEqual(listed.rows.map((entry) => entry.id).sort());
  });

  it("clears Recent on this device and says so", () => {
    recordVisit("host-a", "agent-a", Date.now());
    const s = sheet({ directory: directory([sessionEntry("agent-a")]) });
    expect(rowIds(s.tree)).toEqual(["agent-a"]);
    expect(action(s.tree, "Clear recent")?.props.accessibilityLabel).toBe("Clear recent on this device");
    press(action(s.tree, "Clear recent")); s.render();
    expect(rowIds(s.tree)).toEqual([]);
    expect(String(byTestId(s.tree, "kontor-switch-empty")?.props.children)).toMatch(/^Cleared on this device at /);
    expect(text(s.tree)).toContain("Remembered until Paseo restarts on this device.");
  });

  it("lists without opening when this Paseo app gives plugins no navigation", () => {
    expect(text(sheet({ canOpen: false }).tree)).toContain("does not let plugins open sessions");
  });

  it("discloses native coverage and keeps legacy IDs separate without inferred times", () => {
    const raw = JSON.stringify({ clearedAt: null, visits: [{ id: "legacy-id", at: NOW }] });
    const values = new Map([["kontor.recent@1", raw]]);
    configureRecentStorage({ read: (key) => values.get(key), write: (key, value) => { if (value !== null) values.set(key, value); } });
    const s = sheet({ directory: directory([sessionEntry("legacy-id"), sessionEntry("agent-a")]) });
    expect(rowIds(s.tree)).not.toContain("legacy-id");
    expect(byTestId(s.tree, "kontor-switch-device")?.props.accessibilityLabel).toContain("Native sidebar opens and other devices are not included.");
    expect(text(s.tree)).not.toContain("Prompted");
    expect(text(s.tree)).toContain("Saved entries from an earlier version (1), visit time and host unknown, are listed under Last visited in Sessions.");
    press(action(s.tree, "Clear recent")); s.render();
    expect(values.get("kontor.recent@1")).toBe(raw);
    expect(text(s.tree)).toContain("Saved entries from an earlier version (1)");
  });

  it.each([320, 390])("covers the screen at %ipx with rows whose actions keep a 44pt target", (width) => {
    const s = sheet({ compact: true, rootSize: { width, height: 844 } });
    expect(s.tree[0]!.props.placement).toEqual({ align: "stretch", width: 0, cover: true });
    expect(byTestId(within(s.tree, "session-row-agent-a"), "session-row-body")?.props.role).toBe("button");
    expect((byTestId(s.tree, "session-actions-agent-a")!.props.style as (state: { pressed: boolean }) => object)({ pressed: false })).toMatchObject({ minHeight: 44 });
  });
});

describe("Start or switch popover from the composer pill", () => {
  function pop(patch: { compact?: boolean; entries?: DirectoryEntry[]; live?: boolean; complete?: boolean } = {}) {
    recordVisit("host-a", "agent-a", Date.now() - 60_000);
    model.directory = directory(patch.entries ?? [prompted("agent-a", Date.now() - 60_000, { title: "LSA • ASMA-1 • Route" }), sessionEntry("agent-b", { title: "QA • ASMA-1 • Review" })], patch.live ?? true);
    if (patch.complete !== undefined) model.directory = { ...model.directory, inventoryComplete: patch.complete };
    const openScreen = vi.fn(), close = vi.fn();
    const all = { host: { id: "host-a", label: "Host A" }, theme: { colors: COLORS }, layout: { compact: patch.compact ?? false, platform: "web" as const },
      context: "agent" as const, workspaceId: "wks-adam", agentId: "agent-current", close, openScreen, screenId: "sessions" } as unknown as Parameters<typeof StartSwitchPopover>[0];
    let tree: Element[] = [];
    const render = () => { model.cursor = 0; model.effect = 0; tree = elements(StartSwitchPopover(all)); return tree; };
    render();
    const search = (value: string) => { (byTestId(tree, "kontor-switch-search")!.props.onChangeText as (value: string) => void)(value); render(); };
    return { render, search, openScreen, close, props: all, get tree() { return tree; } };
  }
  const navigation = () => ({ openAgent: vi.fn(), openWorkspace: vi.fn() }) as unknown as SessionNavigation & { openAgent: ReturnType<typeof vi.fn> };
  const handedKey = (p: ReturnType<typeof pop>, call = 0) => (p.openScreen.mock.calls[call]![0] as { params: { target: string } }).params.target;

  it("draws the shared body in the host's surface: no nested sheet, Close or second scroller", async () => {
    const p = pop();
    await settle(); p.render();
    expect(model.rpc["sessions.create-options"]).toHaveBeenCalledWith({ workspaceId: "wks-adam" });
    expect(p.tree.some((node) => node.type === AnchoredMenu)).toBe(false);
    expect(action(p.tree, "Close")).toBeUndefined();
    expect(text(p.tree)).toContain("Untitled session\nThis host");
    // The compact host sheet shows the title itself, so the body keeps only its scope there.
    const sheet = text(pop({ compact: true }).tree);
    expect(sheet).not.toContain("Start or switch session"); expect(sheet).toContain("This host");
    expect(byTestId(p.tree, "kontor-switch-new-lead")).toBeDefined();
    expect(byTestId(p.tree, "kontor-switch-kontor")).toBeDefined();
    expect(byTestId(p.tree, "kontor-switch-list")).toMatchObject({ type: "View", props: { role: "list", accessibilityLabel: "Last visited sessions" } });
    // The host's surface scrolls the body; nothing inside it scrolls, and the state toolbar shows whole tiles instead.
    expect(p.tree.filter((node) => node.type === "ScrollView")).toEqual([]);
    expect(byTestId(p.tree, "kontor-switch-search")?.props.autoFocus).toBe(true);
    expect(byTestId(pop({ compact: true }).tree, "kontor-switch-search")?.props.autoFocus).toBe(false);
  });

  it("follows the actual composer session and rename without using searched or previewed rows", () => {
    const p = pop({ entries: [sessionEntry("agent-a", { title: "Current A", workspaceId: "wks-adam" }), sessionEntry("agent-b", { title: "Current B", workspaceId: "wks-adam" })] });
    const heading = () => byTestId(p.tree, "kontor-switch-heading")!.props;
    Object.assign(p.props, { agentId: "agent-a" }); p.render();
    expect(heading().children).toBe("Current A");
    p.search("Current B");
    expect(heading().children).toBe("Current A");
    Object.assign(p.props, { agentId: "agent-b" }); p.render();
    expect(heading().children).toBe("Current B");
    model.directory = directory([sessionEntry("agent-a", { title: "Current A", workspaceId: "wks-adam" }), sessionEntry("agent-b", { title: "Renamed B", workspaceId: "wks-adam" })]); p.render();
    expect(heading().children).toBe("Renamed B");
    Object.assign(p.props, { agentId: "not-loaded" }); p.render();
    expect(heading().children).toBe("Untitled session");
    Object.assign(p.props, { context: "draft" }); p.render();
    expect(heading().children).toBe("Start or switch session");
    expect(p.openScreen).not.toHaveBeenCalled();
  });

  it("keeps the complete long title accessible while ellipsizing beside actions, and rejects another workspace", () => {
    const title = "Long current session title ".repeat(30).trim();
    const p = pop({ entries: [sessionEntry("agent-current", { title, workspaceId: "wks-adam" })] });
    const heading = byTestId(p.tree, "kontor-switch-heading")!;
    expect(heading.props).toMatchObject({ children: title, accessibilityLabel: title, accessibilityRole: "header", numberOfLines: 1, ellipsizeMode: "tail", style: expect.objectContaining({ flexShrink: 1 }) });
    Object.assign(p.props, { workspaceId: "another-workspace" }); p.render();
    expect(byTestId(p.tree, "kontor-switch-heading")!.props.children).toBe("Untitled session");
    Object.assign(p.props, { host: { id: "host-b", label: "Host B" } });
    model.directory = directory([sessionEntry("agent-current", { title: "Other host title", workspaceId: "another-workspace" })]); p.render();
    expect(model.directoryArgs.at(-1)).toMatchObject({ hostId: "host-b" });
    expect(byTestId(p.tree, "kontor-switch-heading")!.props.children).toBe("Other host title");
    model.directory = directory([]); p.render();
    expect(byTestId(p.tree, "kontor-switch-heading")!.props.children).toBe("Untitled session");
  });

  it("draws the same Last visited rows and coverage note as the Kontor screen's sheet", () => {
    const p = pop();
    const sheetTree = elements(StartSwitchSheet({ anchor: { current: null }, root: { current: null }, rootSize: { width: 1440, height: 900 }, compact: false, colors: COLORS, hostId: "host-a", hostStatus: "online",
      directory: model.directory, canOpen: true, onOpen: vi.fn(), onOpenCreated: vi.fn(), onShowView: vi.fn(), onClose: vi.fn() }));
    const rows = (tree: Element[]) => tree.filter((node) => node.props.testID === "session-row-body").map((node) => node.props.accessibilityLabel);
    expect(rows(p.tree)).toEqual(rows(sheetTree));
    expect(rows(p.tree)).toHaveLength(1);
    const coverage = (tree: Element[]) => byTestId(tree, "kontor-switch-device")?.props.accessibilityLabel;
    expect(coverage(p.tree)).toContain("Opened from Kontor · this host and app profile only");
    expect(coverage(sheetTree)).toBe(coverage(p.tree));
  });

  it("reads the shared directory without setting its Archived choice", () => {
    pop();
    expect(model.directoryArgs[0]).toEqual({ hostId: "host-a", hostStatus: "online" });
    expect(model.directoryArgs[0]).not.toHaveProperty("includeArchived");
  });

  it("filters a state in place, then hands its view to Kontor once per open despite repeated taps", () => {
    const p = pop({ complete: true, entries: [sessionEntry("run", { status: "running", title: "Route run" }), sessionEntry("question", { needsInput: true }), sessionEntry("error", { status: "error" })] });
    press(byTestId(p.tree, "kontor-switch-state-running")); p.render();
    expect(rowIds(p.tree)).toEqual(["run"]);
    expect(p.openScreen).not.toHaveBeenCalled();
    p.search(" route ");
    const kontor = byTestId(p.tree, "kontor-switch-kontor")!.props.onPress as () => void;
    kontor(); kontor();
    press(byTestId(p.tree, "kontor-switch-more")); p.render();
    press(byTestId(p.tree, "kontor-switch-more-archived"));
    expect(p.openScreen).toHaveBeenCalledTimes(1); expect(p.close).toHaveBeenCalledTimes(1);
    expect(takeHandOff("host-a", handedKey(p))).toEqual({ kind: "view", status: "running", query: "route", membership: "sessions" });
    expect(readRecent("host-a").record.visits).toHaveLength(1);
  });

  it("keeps a failed view hand-off visible and retryable, without closing or retaining its failed request", () => {
    const p = pop();
    p.openScreen.mockImplementationOnce(() => { throw new Error("Screen unavailable"); });
    const kontor = byTestId(p.tree, "kontor-switch-kontor")!.props.onPress as () => void;
    kontor();
    expect(p.close).not.toHaveBeenCalled(); expect(model.toast.error).toHaveBeenCalledWith("Screen unavailable");
    expect(takeHandOff("host-a", handedKey(p))).toBeNull();
    kontor();
    expect(p.openScreen).toHaveBeenCalledTimes(2); expect(p.close).toHaveBeenCalledOnce();
  });

  it("hands a row's exact ID to the Kontor screen and closes; only the screen's successful open records a visit", () => {
    const p = pop();
    measure(p, 600);
    press(byTestId(p.tree, "kontor-switch-state-all")); p.render();
    p.search("review qa");
    const before = readRecent("host-a").record.visits.map((visit) => visit.id);
    press(byTestId(within(p.tree, "session-row-agent-b"), "session-row-body"));
    expect(p.close).toHaveBeenCalledTimes(1);
    const input = p.openScreen.mock.calls[0]![0] as { screenId: string; params: { target: string } };
    expect(input).toEqual({ screenId: "sessions", params: { target: expect.any(String) } });
    expect(JSON.stringify(input)).not.toContain("agent-b");
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(before);
    const nav = navigation();
    expect(openHandOff({ hostId: "host-a", key: input.params.target, entries: model.directory.entries, navigation: nav, onView: vi.fn() })).toEqual({ ok: true });
    expect(nav.openAgent).toHaveBeenCalledExactlyOnceWith({ agentId: "agent-b", serverId: "host-a" });
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(["agent-b", ...before]);
  });

  it("keeps the popover open, withdraws the request and says why when Kontor cannot open", () => {
    const p = pop();
    p.openScreen.mockImplementation(() => { throw new Error("Plugin screen is unavailable: sessions"); });
    press(byTestId(within(p.tree, "session-row-agent-a"), "session-row-body"));
    expect(p.close).not.toHaveBeenCalled();
    expect(model.toast.error).toHaveBeenCalledExactlyOnceWith("Plugin screen is unavailable: sessions");
    expect(takeHandOff("host-a", handedKey(p))).toBeNull();
  });

  it("hands a created session to Kontor as a hand-off, not an open, and keeps the outcome when Kontor cannot open", async () => {
    const p = pop();
    await settle(); p.render();
    press(byTestId(p.tree, "kontor-switch-new-lead")); p.render();
    const form = p.tree.find((node) => node.type === NewLsaForm)!;
    // The host's popover or sheet scrolls, so Model & settings adds no scroller of its own.
    expect(form.props).toMatchObject({ hostId: "host-a", canOpen: true, options: { status: "ready" }, hostScrolls: true });
    expect(text(p.tree)).toContain("New lead");
    expect(action(p.tree, "Close")).toBeUndefined();
    const onOpenCreated = form.props.onOpenCreated as (outcome: CreateOutcome) => OpenSessionResult;
    expect(onOpenCreated(outcome("verified"))).toEqual({ ok: true, handoff: true });
    expect(p.close).toHaveBeenCalledTimes(1);
    expect(takeHandOff("host-a", handedKey(p))).toMatchObject({ kind: "created", outcome: { agentId: "agent-new" } });
    p.openScreen.mockImplementation(() => { throw new Error("router busy"); });
    expect(onOpenCreated(outcome("verified"))).toEqual({ ok: false, reason: "navigation", message: "router busy" });
    expect(p.close).toHaveBeenCalledTimes(1);
    expect(model.toast.error).not.toHaveBeenCalled();
  });

  it("sizes Choose source to the host surface's body, so the host never scrolls its actions away", async () => {
    // The window is 390 by 844: the wide surface shows at most 440 of it, the compact sheet 80% less its chrome and a
    // title row whose line is the picker's own title line as drawn.
    for (const [compact, height] of [[false, () => 416], [true, (line: number) => Math.floor(844 * 0.8 - 70 - line)]] as const) {
      model.unmount(); model.slots = []; model.deps = []; model.stores = {};
      const p = pop({ compact });
      await settle(); p.render();
      press(byTestId(p.tree, "kontor-switch-new-lead")); p.render();
      const given = p.tree.find((node) => node.type === NewLsaForm)?.props.pickerHeight as (line: number) => number;
      for (const line of [17, 34]) expect(given(line)).toBe(height(line));
    }
    expect([popoverBodyHeight(900, false, 34), popoverBodyHeight(400, false, 17), popoverBodyHeight(10, false, 17)]).toEqual([416, 360, 0]);
    expect([popoverBodyHeight(400, true, 17), popoverBodyHeight(400, true, 34), popoverBodyHeight(320, true, 17), popoverBodyHeight(60, true, 34)]).toEqual([233, 216, 169, 0]);
  });

  it("marks the session the composer pill belongs to", () => {
    recordVisit("host-a", "agent-current", Date.now() - 600_000);
    const p = pop({ entries: [prompted("agent-a", Date.now() - 60_000), sessionEntry("agent-current")] });
    expect(text(within(p.tree, "session-row-agent-current"))).toContain("open now");
    expect(byTestId(p.tree, "session-row-agent-current")?.props.style).toMatchObject({ borderLeftWidth: 2 });
    expect(byTestId(p.tree, "session-row-agent-a")?.props.style).toMatchObject({ borderLeftWidth: 0 });
  });

  it("asks nothing of an offline host and shows its last known rows", () => {
    model.hosts = [{ serverId: "host-a", label: "Host A", status: "offline" }];
    const p = pop({ live: false });
    expect(model.directoryArgs[0]).toMatchObject({ hostStatus: "offline" });
    expect(model.rpc["sessions.create-options"]).not.toHaveBeenCalled();
    expect(byTestId(p.tree, "kontor-switch-create-status")?.props.children).toContain("host offline");
    expect(byTestId(p.tree, "kontor-switch-freshness")).toBeDefined();
    expect(rowIds(p.tree)).toEqual(["agent-a"]);
  });
});
