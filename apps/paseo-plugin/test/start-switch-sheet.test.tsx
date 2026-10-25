import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { CreateOptions, CreateOutcome, CreateScope } from "../shared/session-create";
import type { DirectoryEntry, DirectoryState } from "../client/session-directory";

const model = vi.hoisted(() => ({
  slots: [] as unknown[], cursor: 0, effect: 0, deps: [] as (readonly unknown[] | undefined)[],
  cleanups: [] as (void | (() => void))[],
  rpc: {} as Record<string, ReturnType<typeof vi.fn>>,
  focusControl: vi.fn(),
  platform: "web",
  hosts: [] as { serverId: string; label: string; status: string }[],
  toast: { error: vi.fn(), show: vi.fn() },
  directory: null as unknown as DirectoryState,
  directoryArgs: [] as object[],
  useState<T>(initial: T | (() => T)): [T, (next: T | ((previous: T) => T)) => void] {
    const index = this.cursor++;
    if (!(index in this.slots)) this.slots[index] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [this.slots[index] as T, (next) => { this.slots[index] = typeof next === "function" ? (next as (previous: T) => T)(this.slots[index] as T) : next; }];
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
}));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  useState: <T,>(initial: T | (() => T)) => model.useState(initial),
  useRef: <T,>(initial: T) => model.useState(() => ({ current: initial }))[0],
  useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) => model.useEffect(effect, deps),
  useSyncExternalStore: <T,>(_subscribe: unknown, read: () => T) => read() }));
vi.mock("react-native", () => ({ Platform: { get OS() { return model.platform; } }, useWindowDimensions: () => ({ width: 390, height: 844 }), ScrollView: "ScrollView", View: "View", Text: "Text", Pressable: "Pressable", BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => model.rpc[contract.name], useHosts: () => model.hosts }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon", useToast: () => model.toast }));
vi.mock("../client/mobile-filters", () => ({ focusControl: model.focusControl }));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: (args: object) => { model.directoryArgs.push(args); return { state: model.directory, refresh: () => {} }; } }));

import { createAvailability, StartSwitchPopover, StartSwitchSheet, switchRows, type CreateOptionsState } from "../client/start-switch-sheet";
import { NewLsaForm, newRequestId, personaLine, resolveDraft } from "../client/new-lsa-form";
import { openCreatedSession, openHandOff, takeHandOff, withdrawHandOff, type OpenSessionResult, type SessionNavigation } from "../client/session-actions";
import { AnchoredMenu } from "../client/anchored-menu";
import { configureRecentStorage, recordVisit, readRecent } from "../client/recent-sessions";
import { Action, Copy } from "../client/page-controls";
import { sessionEntry } from "./session-fixtures";

type Element = ReactElement<Record<string, unknown>>;
/** The element tree; the hook-free `Choices` and `SwitchRowItem` are drawn so their semantics are visible. */
function elements(value: ReactNode): Element[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(value)) return [];
  const drawn = typeof value.type === "function" && ["Choices", "SwitchRowItem", "SearchPicker", "JiraIssuePicker", "SourceSessionPicker"].includes(value.type.name)
    ? (value.type as (props: object) => ReactNode)(value.props) : null;
  return [value, ...elements(value.props.children as ReactNode), ...elements(value.props.header as ReactNode), ...elements(drawn)];
}
const text = (tree: Element[]) => tree.filter((node) => node.type === "Text" || node.type === Copy).map((node) => [node.props.children].flat(Infinity).join("")).join("\n");
const byTestId = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id);
const action = (tree: Element[], label: string) => tree.find((node) => node.type === Action && node.props.label === label);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const NOW = Date.parse("2026-10-22T12:00:00.000Z");
const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const ISSUE_EPIC = { id: "100", key: "ASMA-100", summary: "Deliver the desk layout", created: "2026-10-01T10:00:00.000+0000", type: "Epic", epic: true, status: "In Development", statusCategory: "In Progress", open: true, parent: null } as const;
const ISSUE_STORY = { id: "103", key: "ASMA-103", summary: "Replace New LSA with a role-aware New session", created: "2026-10-04T10:00:00.000+0000", type: "User Story", epic: false, status: "In Development", statusCategory: "In Progress", open: true, parent: null } as const;
const OPTIONS: CreateOptions = {
  enabled: true, disabledReason: null, pending: null, notes: [], catalogProjectId: "prj-adam", issues: [ISSUE_EPIC], jiraProblem: null,
  desks: [
    { projectId: "prj-adam", project: "DESK • ADAM", desk: "ADAM", kind: "adam", cwd: "/fixtures/adam", lead: { ADHOC: "wks-adhoc", LSA: "wks-lsa", TPM: "wks-tpm" } },
    { projectId: "prj-tickets", project: "DESK • TICKETS", desk: "TICKETS", kind: "tickets", cwd: "/fixtures/tickets", lead: { ADHOC: "wks-t-adhoc", LSA: "wks-t-lsa", TPM: "wks-t-tpm" } },
    { projectId: "prj-pr", project: "DESK • PR REVIEW", desk: "PR REVIEW", kind: "pr-review", cwd: "/fixtures/pr", lead: { ADHOC: null, LSA: "wks-pr-lsa", TPM: "wks-pr-tpm" } },
  ],
  providers: [
    { id: "claude", label: "Claude", status: "ready", available: true, note: null, persona: "supported", defaultModel: "opus", defaultMode: "default",
      models: [{ id: "opus", label: "Opus", efforts: [{ id: "high", label: "High" }, { id: "max", label: "Max" }], defaultEffort: "high" }, { id: "sonnet", label: "Sonnet", efforts: [], defaultEffort: null }],
      modes: [{ id: "default", label: "Default", dangerous: false }, { id: "bypassPermissions", label: "Bypass", dangerous: true }] },
    { id: "gemini", label: "Gemini", status: "error", available: false, note: "Provider error: not signed in.", persona: "unverified", defaultModel: null, defaultMode: null, models: [], modes: [] },
    { id: "opencode", label: "OpenCode", status: "ready", available: true, note: null, persona: "unverified", defaultModel: "big", defaultMode: null, models: [{ id: "big", label: "Big", efforts: [], defaultEffort: null }], modes: [] },
  ],
  profile: { name: "lsa", provider: "claude", model: "opus", modeId: "default", effort: "max" },
};
const outcome = (state: CreateOutcome["state"], patch: Partial<CreateOutcome> = {}): CreateOutcome => ({ requestId: "00000000-0000-4000-8000-000000000001", state, title: "LSA • ADHOC • Route review",
  agentId: state === "verified" || state === "differences" ? "agent-new" : null, workspaceId: "wks-adhoc", message: `Fixture ${state}.`, differences: [], persona: "supported", checkedAt: "2026-10-22", ...patch });
const reply = (state: CreateOutcome["state"], patch: Partial<CreateOutcome> = {}) => async (input: { requestId: string }) => outcome(state, { ...patch, requestId: input.requestId });
const prompted = (id: string, at: number, overrides: Parameters<typeof sessionEntry>[1] = {}) => ({ ...sessionEntry(id, overrides), lastUserMessageAt: new Date(at).toISOString() });
const directory = (entries: DirectoryEntry[], live = true): DirectoryState => ({ entries, status: "ready", live, lastUpdatedAt: NOW, error: null, staleReason: null, pages: 1, includeArchived: false });

beforeEach(() => {
  model.unmount();
  model.slots = []; model.cursor = 0; model.effect = 0; model.deps = [];
  model.focusControl.mockClear();
  model.platform = "web";
  model.hosts = [{ serverId: "host-a", label: "Host A", status: "online" }];
  model.toast = { error: vi.fn(), show: vi.fn() };
  model.directoryArgs = [];
  withdrawHandOff();
  model.rpc = { "sessions.create-options": vi.fn(async () => OPTIONS), "sessions.create": vi.fn(), "sessions.create-check": vi.fn() };
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
  });

  it("matches every token across the directory, Recent first, at most 20 shown with the full count", () => {
    const entries = Array.from({ length: 25 }, (_, index) => sessionEntry(`agent-${index}`, { title: `Route review ${index}` }));
    entries.push(sessionEntry("agent-other", { title: "Billing" }), sessionEntry("agent-archived", { title: "Route review old", archivedAt: "2026-10-22" }));
    recordVisit("host-a", "agent-24", NOW);
    const { rows, total } = switchRows(entries, "  review   ROUTE ", NOW, readRecent("host-a", NOW).record);
    expect(total).toBe(25);
    expect(rows).toHaveLength(20);
    expect(rows[0]!.entry.id).toBe("agent-24");
    expect(rows.map((row) => row.entry.id)).not.toContain("agent-archived");
  });

  it("says why New session is not offered", () => {
    expect(createAvailability({ status: "offline" })).toContain("host offline");
    expect(createAvailability({ status: "loading" })).toBeNull();
    expect(createAvailability({ status: "error", message: "timeout" })).toBe("New session unavailable: Kontor could not read this host's options (timeout).");
    expect(createAvailability({ status: "ready", options: { ...OPTIONS, enabled: false, disabledReason: "New session is off on this host." } })).toBe("New session is off on this host.");
    expect(createAvailability({ status: "ready", options: OPTIONS })).toBeNull();
  });
});

describe("Start or switch sheet", () => {
  function sheet(props: Partial<Parameters<typeof StartSwitchSheet>[0]> = {}) {
    recordVisit("host-a", "agent-a", Date.now() - 60_000);
    const onOpen = vi.fn(), onOpenCreated = vi.fn(() => ({ ok: true } as const)), onShowAll = vi.fn(), onClose = vi.fn();
    const all = { anchor: { current: null }, root: { current: null }, rootSize: { width: 1440, height: 900 }, compact: false, colors: COLORS, hostId: "host-a", hostStatus: "online" as const,
      directory: directory([prompted("agent-a", Date.now() - 60_000, { title: "LSA • ASMA-1 • Route" })]), canOpen: true, onOpen, onOpenCreated, onShowAll, onClose, ...props };
    let tree: Element[] = [];
    const render = () => { model.cursor = 0; model.effect = 0; tree = elements(StartSwitchSheet(all)); return tree; };
    render();
    return { render, onOpen, onShowAll, onClose, get tree() { return tree; } };
  }

  it("is a labelled dialog whose rows are menu items that open through the screen, with no archive control", () => {
    const s = sheet();
    const root = s.tree[0]!;
    expect(root.props).toMatchObject({ testID: "kontor-switch-sheet", dialog: true, label: "Start or switch session", placement: { align: "end", width: 400, maxHeight: 560 } });
    expect(byTestId(s.tree, "kontor-switch-list")?.props).toMatchObject({ role: "menu", accessibilityLabel: "Recent sessions" });
    const row = byTestId(s.tree, "kontor-switch-row-agent-a")!;
    expect(row.props).toMatchObject({ role: "menuitem", accessibilityLabel: expect.stringMatching(/^LSA • ASMA-1 • Route, Opened .+ · /) });
    (row.props.onPress as () => void)();
    expect(s.onOpen).toHaveBeenCalledWith(expect.objectContaining({ id: "agent-a" }));
    expect(text(s.tree)).not.toMatch(/Archive|✕|×/);
    expect(byTestId(s.tree, "kontor-switch-search")?.props).toMatchObject({ accessibilityLabel: "Jump to any session", autoFocus: true });
  });

  it("asks for the desk of the project context it was opened in", async () => {
    const s = sheet({ initialScope: { projectId: "prj-adam" } });
    await settle();
    expect(model.rpc["sessions.create-options"]).toHaveBeenCalledWith({ projectId: "prj-adam" });
    expect(s.tree[0]!.props.testID).toBe("kontor-switch-sheet");
  });

  it("covers the screen on a phone and does not steal focus there", () => {
    const s = sheet({ compact: true, rootSize: { width: 320, height: 640 } });
    expect(s.tree[0]!.props.placement).toEqual({ align: "stretch", width: 0, cover: true });
    expect(byTestId(s.tree, "kontor-switch-search")?.props.autoFocus).toBe(false);
  });

  it("restores trigger focus through the shared native helper on Close and panel dismissal", async () => {
    const s = sheet();
    (action(s.tree, "Close")!.props.onPress as () => void)();
    expect(s.onClose).toHaveBeenCalledTimes(1);
    await settle();
    expect(model.focusControl).toHaveBeenCalledTimes(1);
    (s.tree[0]!.props.onClose as () => void)();
    await settle();
    expect(model.focusControl).toHaveBeenCalledTimes(2);
  });

  it("offers New session only after the host's options say it is on, and opens the form in place", async () => {
    const s = sheet();
    expect(action(s.tree, "New session")).toBeUndefined();
    await settle(); s.render();
    expect(model.rpc["sessions.create-options"]).toHaveBeenCalledTimes(1);
    (action(s.tree, "New session")!.props.onPress as () => void)(); s.render();
    expect(s.tree[0]!.props.label).toBe("New session");
    const form = s.tree.find((node) => node.type === NewLsaForm)!;
    expect(form.props).toMatchObject({ options: { status: "ready", options: OPTIONS }, canOpen: true });
    expect(form.props.hostId).toBe("host-a");
    expect(form.props.onOpenCreated).toBeDefined();
    (form.props.onCancel as () => void)(); s.render();
    expect(byTestId(s.tree, "kontor-switch-list")).toBeDefined();
  });

  it.each(["Close", "Back", "Cancel"])("invalidates the creation continuation immediately on %s", async (leave) => {
    const s = sheet();
    await settle(); s.render();
    (action(s.tree, "New session")!.props.onPress as () => void)(); s.render();
    const form = s.tree.find((node) => node.type === NewLsaForm)!;
    const active = form.props.isActive as () => boolean;
    expect(active()).toBe(true);
    if (leave === "Cancel") (form.props.onCancel as () => void)();
    else if (leave === "Close") (action(s.tree, "Close")!.props.onPress as () => void)();
    else (s.tree.find((node) => node.props.label === "Back to recent sessions")!.props.onPress as () => void)();
    expect(active()).toBe(false);
  });

  it.each([
    ["the switch is off", async () => ({ ...OPTIONS, enabled: false, disabledReason: "New session is off on this host. The operator turns it on at the host computer." }), "New session is off on this host. The operator turns it on at the host computer."],
    ["the options read failed", async () => { throw new Error("handler crashed"); }, "New session unavailable: Kontor could not read this host's options (handler crashed)."],
  ])("states why New session is absent when %s", async (_label, read, message) => {
    model.rpc["sessions.create-options"] = vi.fn(read);
    const s = sheet();
    await settle(); s.render();
    expect(action(s.tree, "New session")).toBeUndefined();
    expect(byTestId(s.tree, "kontor-switch-create-status")?.props.children).toBe(message);
  });

  it("asks nothing of an offline host and shows the last known list", () => {
    const s = sheet({ hostStatus: "offline", directory: directory([prompted("agent-a", Date.now() - 60_000)], false) });
    expect(model.rpc["sessions.create-options"]).not.toHaveBeenCalled();
    expect(byTestId(s.tree, "kontor-switch-create-status")?.props.children).toContain("host offline");
    expect(byTestId(s.tree, "kontor-switch-freshness")).toBeDefined();
    expect(byTestId(s.tree, "kontor-switch-row-agent-a")).toBeDefined();
  });

  it("keeps Check reachable for a pending creation after the operator turns creation off", async () => {
    model.rpc["sessions.create-options"]!.mockResolvedValue({ ...OPTIONS, enabled: false, disabledReason: "New session is off on this host.", pending: outcome("unknown") });
    const s = sheet();
    await settle(); s.render();
    expect(action(s.tree, "New session")).toBeUndefined();
    (action(s.tree, "Check creation")!.props.onPress as () => void)(); s.render();
    expect(s.tree.find((node) => node.type === NewLsaForm)?.props.options).toMatchObject({ status: "ready", options: { enabled: false, pending: { state: "unknown" } } });
  });

  it("searches, hands a long result to Sessions, and says when nothing matches", () => {
    const entries = Array.from({ length: 22 }, (_, index) => sessionEntry(`agent-${index}`, { title: `Route review ${index}` }));
    const s = sheet({ directory: directory(entries) });
    (byTestId(s.tree, "kontor-switch-search")!.props.onChangeText as (value: string) => void)(" route "); s.render();
    expect(byTestId(s.tree, "kontor-switch-list")?.props.accessibilityLabel).toBe("Matching sessions");
    (action(s.tree, "Show all 22 in Sessions")!.props.onPress as () => void)();
    expect(s.onShowAll).toHaveBeenCalledWith("route");
    (byTestId(s.tree, "kontor-switch-search")!.props.onChangeText as (value: string) => void)("nothing like it"); s.render();
    expect(byTestId(s.tree, "kontor-switch-empty")?.props.children).toBe("No session matches “nothing like it”.");
  });

  it("clears Recent on this device and says so", () => {
    recordVisit("host-a", "agent-a", Date.now());
    const s = sheet({ directory: directory([sessionEntry("agent-a")]) });
    expect(byTestId(s.tree, "kontor-switch-row-agent-a")).toBeDefined();
    (action(s.tree, "Clear recent on this device")!.props.onPress as () => void)(); s.render();
    expect(byTestId(s.tree, "kontor-switch-row-agent-a")).toBeUndefined();
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
    expect(byTestId(s.tree, "kontor-switch-row-legacy-id")).toBeUndefined();
    expect(text(s.tree)).toContain("Native sidebar opens and other devices are not included.");
    expect(text(s.tree)).not.toContain("Prompted");
    expect(text(s.tree)).toContain("Saved entries from an earlier version (1), visit time and host unknown, are listed under Recent in Sessions.");
    const all = action(s.tree, "Show all 1 recent in Sessions")!;
    expect(all.props.accessibilityLabel).toBe("Show all 1 recent sessions and saved entries in Sessions");
    (all.props.onPress as () => void)();
    expect(s.onShowAll).toHaveBeenCalledWith("");
    (action(s.tree, "Clear recent on this device")!.props.onPress as () => void)(); s.render();
    expect(values.get("kontor.recent@1")).toBe(raw);
    expect(text(s.tree)).toContain("Saved entries from an earlier version (1)");
  });

  it.each([320, 390])("keeps the existing compact sheet and accessible 44px rows at %ipx", (width) => {
    const s = sheet({ compact: true, rootSize: { width, height: 844 } });
    expect(s.tree[0]!.props.placement).toEqual({ align: "stretch", width: 0, cover: true });
    const row = byTestId(s.tree, "kontor-switch-row-agent-a")!;
    expect(row.props.role).toBe("menuitem");
    expect(row.props.accessibilityLabel).toMatch(/^LSA .*Opened/);
    expect((row.props.style as (state: { pressed: boolean }) => object)({ pressed: false })).toMatchObject({ minHeight: 44 });
  });
});

describe("Start or switch popover from the composer pill", () => {
  function pop(patch: { compact?: boolean; entries?: DirectoryEntry[]; live?: boolean } = {}) {
    recordVisit("host-a", "agent-a", Date.now() - 60_000);
    model.directory = directory(patch.entries ?? [prompted("agent-a", Date.now() - 60_000, { title: "LSA • ASMA-1 • Route" }), sessionEntry("agent-b", { title: "QA • ASMA-1 • Review" })], patch.live ?? true);
    const openScreen = vi.fn(), close = vi.fn();
    const all = { host: { id: "host-a", label: "Host A" }, theme: { colors: COLORS }, layout: { compact: patch.compact ?? false, platform: "web" as const },
      context: "agent" as const, workspaceId: "wks-adam", agentId: "agent-current", close, openScreen, screenId: "sessions" } as unknown as Parameters<typeof StartSwitchPopover>[0];
    let tree: Element[] = [];
    const render = () => { model.cursor = 0; model.effect = 0; tree = elements(StartSwitchPopover(all)); return tree; };
    render();
    const search = (value: string) => { (byTestId(tree, "kontor-switch-search")!.props.onChangeText as (value: string) => void)(value); render(); };
    return { render, search, openScreen, close, get tree() { return tree; } };
  }
  const navigation = () => ({ openAgent: vi.fn(), openWorkspace: vi.fn() }) as unknown as SessionNavigation & { openAgent: ReturnType<typeof vi.fn> };

  it("draws the shared body in the host's surface: no nested sheet, title, Close or second scroller, with Open Kontor inside", async () => {
    const p = pop();
    await settle(); p.render();
    expect(model.rpc["sessions.create-options"]).toHaveBeenCalledWith({ workspaceId: "wks-adam" });
    expect(p.tree.some((node) => node.type === AnchoredMenu)).toBe(false);
    expect(text(p.tree)).not.toContain("Start or switch");
    expect(action(p.tree, "Close")).toBeUndefined();
    expect(action(p.tree, "Open Kontor")?.props).toMatchObject({ icon: "Building2" });
    expect(action(p.tree, "New session")).toBeDefined();
    expect(byTestId(p.tree, "kontor-switch-list")).toMatchObject({ type: "View", props: { role: "menu", accessibilityLabel: "Recent sessions" } });
    expect(p.tree.some((node) => node.type === "ScrollView")).toBe(false);
    expect(byTestId(p.tree, "kontor-switch-search")?.props.autoFocus).toBe(true);
    expect(pop({ compact: true }).tree.find((node) => node.props.testID === "kontor-switch-search")?.props.autoFocus).toBe(false);
  });

  it("draws the same Recent rows, search and footnotes as the Kontor screen's sheet", () => {
    const p = pop();
    const sheetTree = elements(StartSwitchSheet({ anchor: { current: null }, root: { current: null }, rootSize: { width: 1440, height: 900 }, compact: false, colors: COLORS, hostId: "host-a", hostStatus: "online",
      directory: model.directory, canOpen: true, onOpen: vi.fn(), onOpenCreated: vi.fn(), onShowAll: vi.fn(), onClose: vi.fn() }));
    const rows = (tree: Element[]) => tree.filter((node) => String(node.props.testID).startsWith("kontor-switch-row-")).map((node) => node.props.accessibilityLabel);
    expect(rows(p.tree)).toEqual(rows(sheetTree));
    expect(rows(p.tree)).toHaveLength(1);
    for (const line of ["Opened from Kontor · this host and app profile only", "Native sidebar opens and other devices are not included."]) {
      expect(text(p.tree)).toContain(line);
      expect(text(sheetTree)).toContain(line);
    }
  });

  it("reads the shared directory without setting its Archived choice", () => {
    pop();
    expect(model.directoryArgs[0]).toEqual({ hostId: "host-a", hostStatus: "online" });
    expect(model.directoryArgs[0]).not.toHaveProperty("includeArchived");
  });

  it("opens Kontor without a request and closes", () => {
    const p = pop();
    (action(p.tree, "Open Kontor")!.props.onPress as () => void)();
    expect(p.openScreen).toHaveBeenCalledExactlyOnceWith({ screenId: "sessions" });
    expect(p.close).toHaveBeenCalledTimes(1);
  });

  it("hands a row's exact ID to the Kontor screen and closes; only the screen's successful open records a visit", () => {
    const p = pop();
    p.search("review qa");
    const before = readRecent("host-a").record.visits.map((visit) => visit.id);
    (byTestId(p.tree, "kontor-switch-row-agent-b")!.props.onPress as () => void)();
    expect(p.close).toHaveBeenCalledTimes(1);
    const input = p.openScreen.mock.calls[0]![0] as { screenId: string; params: { target: string } };
    expect(input).toEqual({ screenId: "sessions", params: { target: expect.any(String) } });
    expect(JSON.stringify(input)).not.toContain("agent-b");
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(before);
    const nav = navigation();
    expect(openHandOff({ hostId: "host-a", key: input.params.target, entries: model.directory.entries, navigation: nav, onSearch: vi.fn() })).toEqual({ ok: true });
    expect(nav.openAgent).toHaveBeenCalledExactlyOnceWith({ agentId: "agent-b", serverId: "host-a" });
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(["agent-b", ...before]);
  });

  it("hands a long search to Sessions through the same request", () => {
    const entries = Array.from({ length: 22 }, (_, index) => sessionEntry(`agent-${index}`, { title: `Route review ${index}` }));
    const p = pop({ entries });
    p.search(" route ");
    (action(p.tree, "Show all 22 in Sessions")!.props.onPress as () => void)();
    const key = (p.openScreen.mock.calls[0]![0] as { params: { target: string } }).params.target;
    expect(takeHandOff("host-a", key)).toEqual({ kind: "search", query: "route" });
  });

  it("keeps the popover open, withdraws the request and says why when Kontor cannot open", () => {
    const p = pop();
    p.openScreen.mockImplementation(() => { throw new Error("Plugin screen is unavailable: sessions"); });
    (byTestId(p.tree, "kontor-switch-row-agent-a")!.props.onPress as () => void)();
    expect(p.close).not.toHaveBeenCalled();
    expect(model.toast.error).toHaveBeenCalledExactlyOnceWith("Plugin screen is unavailable: sessions");
    const key = (p.openScreen.mock.calls[0]![0] as { params: { target: string } }).params.target;
    expect(takeHandOff("host-a", key)).toBeNull();
  });

  it("hands a created session to Kontor as a hand-off, not an open, and keeps the outcome when Kontor cannot open", async () => {
    const p = pop();
    await settle(); p.render();
    (action(p.tree, "New session")!.props.onPress as () => void)(); p.render();
    const form = p.tree.find((node) => node.type === NewLsaForm)!;
    expect(form.props).toMatchObject({ hostId: "host-a", canOpen: true, options: { status: "ready" } });
    expect(text(p.tree)).toContain("New session");
    expect(action(p.tree, "Close")).toBeUndefined();
    const onOpenCreated = form.props.onOpenCreated as (outcome: CreateOutcome) => OpenSessionResult;
    expect(onOpenCreated(outcome("verified"))).toEqual({ ok: true, handoff: true });
    expect(p.close).toHaveBeenCalledTimes(1);
    const key = (p.openScreen.mock.calls[0]![0] as { params: { target: string } }).params.target;
    expect(takeHandOff("host-a", key)).toMatchObject({ kind: "created", outcome: { agentId: "agent-new" } });
    p.openScreen.mockImplementation(() => { throw new Error("router busy"); });
    expect(onOpenCreated(outcome("verified"))).toEqual({ ok: false, reason: "navigation", message: "router busy" });
    expect(p.close).toHaveBeenCalledTimes(1);
    expect(model.toast.error).not.toHaveBeenCalled();
  });

  it("asks nothing of an offline host and shows its last known rows", () => {
    model.hosts = [{ serverId: "host-a", label: "Host A", status: "offline" }];
    const p = pop({ live: false });
    expect(model.directoryArgs[0]).toMatchObject({ hostStatus: "offline" });
    expect(model.rpc["sessions.create-options"]).not.toHaveBeenCalled();
    expect(byTestId(p.tree, "kontor-switch-create-status")?.props.children).toContain("host offline");
    expect(byTestId(p.tree, "kontor-switch-freshness")).toBeDefined();
    expect(byTestId(p.tree, "kontor-switch-row-agent-a")).toBeDefined();
  });
});

describe("New session draft", () => {
  it("starts from the host's LSA profile, then each provider's own defaults", () => {
    const draft = resolveDraft(OPTIONS, { topic: " Route   review " });
    expect(draft).toMatchObject({ desk: { projectId: "prj-adam" }, role: "LSA", start: "ad-hoc", provider: { id: "claude" }, model: { id: "opus" }, effort: { id: "max" }, mode: { id: "default" },
      topic: "Route review", title: "LSA • ADHOC • Route review", problem: null, fromProfile: true });
    expect(resolveDraft(OPTIONS, { topic: "Route review", provider: "opencode" })).toMatchObject({ model: { id: "big" }, effort: null, mode: null, fromProfile: false, problem: null });
    expect(resolveDraft({ ...OPTIONS, profile: null }, { topic: "Route review" })).toMatchObject({ provider: { id: "claude" }, effort: { id: "high" }, fromProfile: false });
  });

  it.each([
    [{ ...OPTIONS, enabled: false, disabledReason: "Switched off." }, "Route review", "Switched off."],
    [{ ...OPTIONS, pending: outcome("unknown") }, "Route review", "An earlier creation, “LSA • ADHOC • Route review”, is not settled. Check it first."],
    [{ ...OPTIONS, desks: [] }, "Route review", "No desk on this host has lead workspaces."],
    [{ ...OPTIONS, providers: [OPTIONS.providers[1]!] }, "Route review", "Provider claude is no longer active. The host's lsa profile chooses it. Choose an active provider."],
    [OPTIONS, "Route review", "Gemini: Provider error: not signed in. Choose an active provider.", "gemini"],
    [OPTIONS, "ab", "Type a topic of at least 3 characters."],
    [OPTIONS, "A • B", "A topic cannot contain • or ·, which separate title parts."],
  ])("names the reason Create is disabled", (options, topic, problem, provider?: string) => {
    expect(resolveDraft(options, { topic, provider }).problem).toBe(problem);
  });

  it("starts on the desk the options were scoped to", () => {
    expect(resolveDraft({ ...OPTIONS, catalogProjectId: "prj-tickets" }, { topic: "Route review" }).desk?.projectId).toBe("prj-tickets");
  });

  it("makes version-4 request IDs", () => {
    const ids = new Set(Array.from({ length: 50 }, newRequestId));
    expect(ids.size).toBe(50);
    for (const id of ids) expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  });

  it("promises the persona only where it is confirmed", () => {
    expect(personaLine("supported", "Claude")).toContain("Claude and Codex apply it");
    expect(personaLine("unverified", "OpenCode")).toBe("Persona: delivered as a bootstrap prompt before the first task, because Kontor has not confirmed that OpenCode applies a system prompt.");
  });

  it("keeps invalidated explicit and saved choices invalid instead of replacing them", () => {
    expect(resolveDraft(OPTIONS, { topic: "Route review", provider: "removed" })).toMatchObject({ provider: null, problem: expect.stringContaining("Provider removed is no longer active") });
    expect(resolveDraft(OPTIONS, { topic: "Route review", model: "removed" })).toMatchObject({ model: null, problem: expect.stringContaining("Model removed is no longer active") });
    expect(resolveDraft({ ...OPTIONS, profile: { ...OPTIONS.profile!, model: "removed" } }, { topic: "Route review" })).toMatchObject({ model: null, problem: expect.stringContaining("Model removed is no longer active") });
    expect(resolveDraft(OPTIONS, { topic: "Route review", effort: "removed" })).toMatchObject({ effort: null, problem: expect.stringContaining("Effort removed is no longer offered") });
    expect(resolveDraft(OPTIONS, { topic: "Route review", modeId: "removed" })).toMatchObject({ mode: null, problem: expect.stringContaining("Mode removed is no longer offered") });
  });

  it.each(["effort", "modeId"] as const)("rejects a cleared %s once the current catalog requires a value", (setting) => {
    const draft = { topic: "Route review", [setting]: null };
    expect(resolveDraft(OPTIONS, draft)).toMatchObject({ problem: expect.stringContaining(setting === "effort" ? "Choose an available effort" : "Choose an available mode") });
  });
});

describe("New session form", () => {
  function form(options: CreateOptionsState = { status: "ready", options: OPTIONS }, props: Partial<Parameters<typeof NewLsaForm>[0]> = {}) {
    const onRefresh = vi.fn(), onOpenCreated = vi.fn<(outcome: CreateOutcome) => OpenSessionResult>(() => ({ ok: true })), onCancel = vi.fn();
    const all = { hostId: "host-a", colors: COLORS, compact: false, options, onRefresh, canOpen: true, onOpenCreated, onCancel, ...props };
    let tree: Element[] = [];
    const render = () => { model.cursor = 0; model.effect = 0; tree = elements(NewLsaForm(all)); return tree; };
    render();
    const type = (topic: string) => { (byTestId(tree, "kontor-new-lsa-topic")!.props.onChangeText as (value: string) => void)(topic); render(); };
    const press = (id: string) => { ((byTestId(tree, id) ?? action(tree, id))!.props.onPress as () => void)(); render(); };
    return { render, type, press, onRefresh, onOpenCreated, onCancel, unmount: () => model.unmount(), setHost: (hostId: string) => { all.hostId = hostId; render(); }, setOptions: (options: CreateOptionsState) => { all.options = options; render(); }, get tree() { return tree; } };
  }

  it.each([
    [{ status: "loading" } as const, "Reading this host's desks and providers…"],
    [{ status: "offline" } as const, "Host offline. Nothing can be created until it reconnects."],
  ])("waits truthfully while options are %s", (state, message) => {
    expect(text(form(state).tree)).not.toContain("Create LSA");
    expect(text(form(state).tree)).toBe(message);
  });

  it("offers Try again when the options read failed", () => {
    const f = form({ status: "error", message: "timeout" });
    f.press("Try again");
    expect(f.onRefresh).toHaveBeenCalledTimes(1);
  });

  it("handles an empty active catalog with a refresh action and no create", () => {
    const f = form({ status: "ready", options: { ...OPTIONS, profile: null, providers: [] } });
    expect(text(f.tree)).toContain("No active provider is available for this desk.");
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    f.press("Refresh choices");
    expect(f.onRefresh).toHaveBeenCalledTimes(1);
    expect(model.rpc["sessions.create"]).not.toHaveBeenCalled();
  });

  it.each([
    ["loading", "Some providers are still loading."],
    ["unknown", "Some provider availability is unknown."],
    ["error", "Some providers could not be read."],
    ["unavailable", "Some providers are unavailable."],
  ] as const)("distinguishes %s from disabled without offering an inactive radio", (status, message) => {
    const f = form({ status: "ready", options: { ...OPTIONS, profile: null, providers: [{ ...OPTIONS.providers[0]!, label: "Unready provider", available: false, status, note: message }] } });
    expect(text(f.tree)).toContain(message);
    expect(f.tree.some((node) => node.props.accessibilityRole === "radio" && node.props.accessibilityLabel === "Unready provider")).toBe(false);
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
  });

  it("retains an explicit model when a refresh removes it and asks for a new choice", () => {
    const f = form(); f.type("Route review");
    (f.tree.find((node) => node.props.accessibilityLabel === "Sonnet")!.props.onPress as () => void)(); f.render();
    f.setOptions({ status: "ready", options: { ...OPTIONS, providers: OPTIONS.providers.map((provider) => ({ ...provider, models: provider.models.filter((model) => model.id !== "sonnet") })) } });
    expect(text(f.tree)).toContain("Model sonnet is no longer active");
    expect(f.tree.find((node) => node.props.accessibilityRole === "radio" && node.props.accessibilityLabel === "Opus")?.props["aria-checked"]).toBe(false);
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    expect(model.rpc["sessions.create"]).not.toHaveBeenCalled();
  });

  it("requires a catalog scoped to a newly selected desk", () => {
    const f = form(); f.type("Route review");
    (f.tree.find((node) => node.props.accessibilityLabel === "DESK • PR REVIEW")!.props.onPress as () => void)(); f.render();
    expect(f.onRefresh).toHaveBeenCalledWith({ projectId: "prj-pr", role: "LSA", start: "ad-hoc" });
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    expect(text(f.tree)).toContain("Refresh this desk's provider catalog");
    f.setOptions({ status: "ready", options: { ...OPTIONS, catalogProjectId: "prj-pr" } });
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(false);
  });

  it("offers truthful recovery when the selected desk vanishes but another ADAM workspace remains", () => {
    const f = form(); f.type("Route review");
    const vanished = { ...OPTIONS, desks: [OPTIONS.desks[2]!], catalogProjectId: null, providers: [] };
    f.setOptions({ status: "ready", options: vanished });
    expect(text(f.tree)).toContain("The selected desk is no longer available. Choose a listed desk or Refresh desks.");
    expect(text(f.tree)).toContain("Choose an available desk to read its providers.");
    expect(text(f.tree)).not.toContain("No desk on this host has lead workspaces.");
    expect(text(f.tree)).not.toContain("No active provider on this host.");
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    const remaining = () => f.tree.find((node) => node.props.accessibilityRole === "radio" && node.props.accessibilityLabel === "DESK • PR REVIEW")!;
    expect(remaining().props["aria-checked"]).toBe(false);
    f.press("Refresh desks");
    expect(f.onRefresh).toHaveBeenLastCalledWith(expect.objectContaining({ role: "LSA", start: "ad-hoc" }));
    f.setOptions({ status: "ready", options: vanished });
    expect(remaining().props["aria-checked"]).toBe(false);
    (remaining().props.onPress as () => void)(); f.render();
    expect(f.onRefresh).toHaveBeenLastCalledWith({ projectId: "prj-pr", role: "LSA", start: "ad-hoc" });
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    f.setOptions({ status: "ready", options: { ...OPTIONS, desks: vanished.desks, catalogProjectId: "prj-pr" } });
    expect(remaining().props["aria-checked"]).toBe(true);
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(false);
    expect(byTestId(f.tree, "kontor-new-lsa-topic")?.props.value).toBe("Route review");
    expect(model.rpc["sessions.create"]).not.toHaveBeenCalled();
    expect(f.onOpenCreated).not.toHaveBeenCalled();
  });

  it("retains the keyboard-selected desk and draft through loading and failed scoped reads, with no implicit submit", () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    const options = { ...OPTIONS, desks: [OPTIONS.desks[0]!, { projectId: "prj-ops", project: "DESK • OPS", desk: "OPS", kind: "other" as const, cwd: "/fixtures/ops", lead: { ADHOC: null, LSA: null, TPM: null } }] };
    const f = form({ status: "ready", options }); f.type("Route review");
    const desk = (label: string) => f.tree.find((node) => node.props.accessibilityRole === "radio" && node.props.accessibilityLabel === `DESK • ${label}`)!;
    const focus = [vi.fn(), vi.fn()];
    (desk("ADAM").props.onKeyDown as (event: object) => void)({ key: "ArrowRight", preventDefault: vi.fn(), stopPropagation: vi.fn(), currentTarget: { parentElement: { querySelectorAll: () => focus.map((focus) => ({ focus })) } } });
    f.render();
    expect(focus[1]).toHaveBeenCalledTimes(1);
    expect(f.onRefresh).toHaveBeenCalledExactlyOnceWith({ projectId: "prj-ops", role: "LSA", start: "ad-hoc" });
    for (const state of [{ status: "loading" }, { status: "error", message: "scoped read failed" }] as const) {
      f.setOptions(state);
      expect(desk("OPS")?.props).toMatchObject({ "aria-checked": true, tabIndex: 0 });
      expect(desk("ADAM")?.props).toMatchObject({ "aria-checked": false, tabIndex: -1 });
      expect(byTestId(f.tree, "kontor-new-lsa-topic")?.props.value).toBe("Route review");
      expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
      f.press("kontor-new-lsa-create");
      expect(model.rpc["sessions.create"]).not.toHaveBeenCalled();
    }
    f.setOptions({ status: "ready", options: { ...options, catalogProjectId: "prj-ops" } });
    expect(desk("OPS")?.props).toMatchObject({ "aria-checked": true, tabIndex: 0 });
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(false);
    expect(model.rpc["sessions.create"]).not.toHaveBeenCalled();
    f.press("kontor-new-lsa-create");
    expect(model.rpc["sessions.create"]).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ projectId: "prj-ops", topic: "Route review" }));
  });

  it.each(["offline", "host change"])("discards retained presentation options on %s", (change) => {
    const f = form(); f.type("Route review");
    f.setOptions({ status: "loading" });
    expect(byTestId(f.tree, "kontor-new-lsa-topic")?.props.value).toBe("Route review");
    if (change === "offline") f.setOptions({ status: "offline" });
    else f.setHost("host-b");
    expect(byTestId(f.tree, "kontor-new-lsa-create")).toBeUndefined();
    expect(f.tree.some((node) => node.props.accessibilityRole === "radio")).toBe(false);
    expect(model.rpc["sessions.create"]).not.toHaveBeenCalled();
  });

  it("requires an explicit action to clear saved effort and mode when the host no longer offers either", () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    const f = form({ status: "ready", options: { ...OPTIONS, providers: OPTIONS.providers.map((provider) => ({ ...provider, modes: [], defaultMode: null, models: provider.models.map((model) => ({ ...model, efforts: [], defaultEffort: null })) })) } });
    f.type("Route review");
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    f.press("Use current effort default");
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    f.press("Use current mode default");
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(false);
    f.press("kontor-new-lsa-create");
    expect(model.rpc["sessions.create"]).toHaveBeenCalledWith(expect.objectContaining({ effort: null, modeId: null }));
  });

  it("requires explicit recovery when a refresh adds effort and mode after both were cleared", () => {
    const options = { ...OPTIONS, providers: OPTIONS.providers.map((provider) => ({ ...provider, modes: [], defaultMode: null, models: provider.models.map((model) => ({ ...model, efforts: [], defaultEffort: null })) })) };
    const f = form({ status: "ready", options }); f.type("Route review");
    f.press("Use current effort default"); f.press("Use current mode default");
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(false);
    f.setOptions({ status: "loading" });
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    f.setOptions({ status: "ready", options: OPTIONS });
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    expect(text(f.tree)).toContain("Choose an available effort");
    expect(f.tree.filter((node) => node.props.accessibilityRole === "radio" && ["High", "Max", "Default", "Bypass ⚠"].includes(String(node.props.accessibilityLabel))).every((node) => node.props["aria-checked"] === false)).toBe(true);
    f.press("Use current effort default");
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    expect(text(f.tree)).toContain("Choose an available mode");
    f.press("Use current mode default");
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(false);
    expect(model.rpc["sessions.create"]).not.toHaveBeenCalled();
  });

  it("disables Create with a visible, announced reason until the topic is valid", () => {
    const f = form();
    const create = () => byTestId(f.tree, "kontor-new-lsa-create")!;
    expect(create().props).toMatchObject({ disabled: true, accessibilityState: { disabled: true }, accessibilityHint: "Type a topic of at least 3 characters." });
    expect(byTestId(f.tree, "kontor-new-lsa-problem")?.props.children).toBe("Type a topic of at least 3 characters.");
    f.type("Route review");
    expect(create().props).toMatchObject({ disabled: false, accessibilityLabel: "Create LSA • ADHOC • Route review", accessibilityHint: undefined });
    expect(text(f.tree)).toContain("Title: LSA • ADHOC • Route review.");
  });

  it("offers active choices as radios and keeps unavailable providers out of the picker", () => {
    const f = form();
    const radios = f.tree.filter((node) => node.props.accessibilityRole === "radio");
    expect(f.tree.filter((node) => node.props.accessibilityRole === "radiogroup").map((node) => node.props.accessibilityLabel)).toEqual(["Role", "Start", "Desk", "Provider", "Model", "Effort", "Mode"]);
    expect(radios.find((node) => node.props.accessibilityLabel === "DESK • ADAM")?.props.accessibilityState).toEqual({ checked: true, disabled: false });
    expect(radios.some((node) => String(node.props.accessibilityLabel).includes("Gemini"))).toBe(false);
    expect(radios.every((node) => (node.props.style as (state: { focused: boolean }) => { minHeight: number })({ focused: false }).minHeight >= 44)).toBe(true);
  });

  it("warns about a dangerous mode before Create", () => {
    const f = form();
    (f.tree.find((node) => node.props.accessibilityLabel === "Bypass ⚠")!.props.onPress as () => void)(); f.render();
    expect(byTestId(f.tree, "kontor-new-lsa-danger")?.props.children).toEqual(["⚠ ", "Bypass", " is a dangerous permission mode on this host."]);
  });

  it("selects a focused unchecked radio with Space, moves focus with arrows and skips disabled providers", () => {
    const f = form();
    const group = (label: string) => elements(f.tree.find((node) => node.props.accessibilityRole === "radiogroup" && node.props.accessibilityLabel === label)).filter((node) => node.props.accessibilityRole === "radio");
    const key = (label: string, index: number, value: string) => {
      const radios = group(label), focus = radios.map(() => ({ focus: vi.fn() }));
      const event = { key: value, preventDefault: vi.fn(), stopPropagation: vi.fn(), currentTarget: { parentElement: { querySelectorAll: () => focus } } };
      (radios[index]!.props.onKeyDown as (event: object) => void)(event); f.render();
      expect(event.preventDefault).toHaveBeenCalled();
      return focus;
    };
    expect(group("Desk").map((node) => node.props.tabIndex)).toEqual([0, -1, -1]);
    key("Desk", 1, " ");
    expect(group("Desk").map((node) => node.props["aria-checked"])).toEqual([false, true, false]);
    expect(group("Desk").map((node) => node.props.tabIndex)).toEqual([-1, 0, -1]);
    expect(key("Desk", 1, "ArrowLeft")[0]!.focus).toHaveBeenCalledTimes(1);
    expect(group("Desk").map((node) => node.props["aria-checked"])).toEqual([true, false, false]);
    expect(key("Provider", 0, "ArrowRight")[1]!.focus).toHaveBeenCalledTimes(1);
    expect(group("Provider").map((node) => node.props["aria-checked"])).toEqual([false, true]);
    expect(key("Provider", 1, "ArrowDown")[0]!.focus).toHaveBeenCalledTimes(1);
    expect(group("Provider").map((node) => node.props["aria-checked"])).toEqual([true, false]);
  });

  it("keeps native radio and disabled semantics without web-only keyboard or tab props", () => {
    model.platform = "ios";
    const f = form();
    const radios = f.tree.filter((node) => node.props.accessibilityRole === "radio");
    expect(radios.length).toBeGreaterThan(0);
    expect(radios.every((node) => node.props.onKeyDown === undefined && node.props.tabIndex === undefined)).toBe(true);
    expect(radios.find((node) => node.props.accessibilityLabel === "DESK • ADAM")?.props.accessibilityState).toEqual({ checked: true, disabled: false });
    expect(radios.every((node) => !node.props.disabled)).toBe(true);
  });

  it("sends one request and waits for explicit Open of the exact verified result without a directory or redundant options refresh", async () => {
    const f = form();
    let answer: (value: CreateOutcome) => void = () => {};
    model.rpc["sessions.create"]!.mockImplementation(() => new Promise((resolve) => { answer = resolve; }));
    f.type(" Route   review ");
    f.press("kontor-new-lsa-create");
    expect(model.rpc["sessions.create"]).toHaveBeenCalledWith({ requestId: expect.stringMatching(/^[0-9a-f-]{36}$/), role: "LSA", start: "ad-hoc", projectId: "prj-adam", topic: "Route review", sourceAgentId: null, provider: "claude", model: "opus", effort: "max", modeId: "default" });
    expect(text(f.tree)).toContain("Nothing is reported until the host answers.");
    expect(byTestId(f.tree, "kontor-new-lsa-create")).toBeUndefined();
    const requestId = model.rpc["sessions.create"]!.mock.calls[0]![0].requestId;
    answer(outcome("verified", { requestId })); await settle(); f.render();
    expect(byTestId(f.tree, "kontor-new-lsa-verified")?.props).toMatchObject({ accessibilityLiveRegion: "polite" });
    expect(text(f.tree)).toContain("Created and verified");
    expect(f.onOpenCreated).not.toHaveBeenCalled();
    expect(action(f.tree, "Open session")).toBeDefined();
    const open = action(f.tree, "Open session")!.props.onPress as () => void;
    open(); open(); f.render();
    expect(f.onOpenCreated).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ state: "verified", agentId: "agent-new", requestId }));
    expect(f.onRefresh).not.toHaveBeenCalled();
  });

  it("keeps readback differences visible with an exact-ID Open action without a directory wait", async () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("differences", { differences: ["Model reads sonnet."] }));
    const f = form();
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(text(f.tree)).toContain("Created with differences");
    expect(text(f.tree)).toContain("Model reads sonnet.");
    expect(f.onOpenCreated).not.toHaveBeenCalled();
    expect(text(f.tree)).toContain("Session ID: agent-new");
    f.press("Open session");
    expect(f.onOpenCreated).toHaveBeenCalledWith(expect.objectContaining({ state: "differences", agentId: "agent-new" }));
  });

  it("ignores a second submit before React has rendered the busy state", async () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    const f = form();
    f.type("Route review");
    const submit = byTestId(f.tree, "kontor-new-lsa-create")!.props.onPress as () => void;
    submit(); submit();
    expect(model.rpc["sessions.create"]).toHaveBeenCalledTimes(1);
    await settle(); f.render();
    expect(text(f.tree)).toContain("Created and verified");
    expect(f.onOpenCreated).not.toHaveBeenCalled();
    f.press("Open session");
    expect(f.onOpenCreated).toHaveBeenCalledTimes(1);
  });

  it("retains Created and the exact ID after opening fails, then retries only opening", async () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    const f = form();
    f.onOpenCreated.mockReturnValueOnce({ ok: false, reason: "failed", message: "navigation unavailable" });
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(text(f.tree)).toContain("Created and verified");
    expect(text(f.tree)).toContain("Session ID: agent-new");
    expect(f.onOpenCreated).not.toHaveBeenCalled();
    f.press("Open session");
    expect(text(f.tree)).toContain("Opening failed: navigation unavailable");
    expect(action(f.tree, "Back to the form")).toBeUndefined();
    f.press("Open session");
    expect(f.onOpenCreated).toHaveBeenCalledTimes(2);
    expect(model.rpc["sessions.create"]).toHaveBeenCalledTimes(1);
    expect(text(f.tree)).toContain("Opened in Paseo.");
  });

  it("says a hand-off to Kontor is opening there, never that Paseo opened it, and offers no second Open", async () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    const f = form(undefined, { onOpenCreated: vi.fn<(outcome: CreateOutcome) => OpenSessionResult>(() => ({ ok: true, handoff: true })) });
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    f.press("Open session");
    expect(byTestId(f.tree, "kontor-new-lsa-open-status")?.props.children).toBe("Opening in Kontor…");
    expect(text(f.tree)).not.toContain("Opened in Paseo.");
    expect(text(f.tree)).toContain("Session ID: agent-new");
    expect(action(f.tree, "Open session")).toBeUndefined();
  });

  it("records Recently viewed only after a successful user Open, never creation", async () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    const navigation = { openAgent: vi.fn(), openWorkspace: vi.fn() } as unknown as SessionNavigation;
    const f = form(undefined, { onOpenCreated: (outcome) => openCreatedSession({ outcome, serverId: "host-a", navigation }) });
    expect(readRecent("host-a").record.visits).toEqual([]);
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(navigation.openAgent).not.toHaveBeenCalled();
    expect(readRecent("host-a").record.visits).toEqual([]);
    f.press("Open session");
    expect(navigation.openAgent).toHaveBeenCalledExactlyOnceWith({ agentId: "agent-new", serverId: "host-a" });
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(["agent-new"]);
    expect(readRecent("host-b").record.visits).toEqual([]);
  });

  it.each(["close", "unmount", "host switch", "offline"])("rejects an old user Open action after %s without navigation or MRU", async (cancel) => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    let active = true;
    const navigation = { openAgent: vi.fn(), openWorkspace: vi.fn() } as unknown as SessionNavigation;
    const f = form(undefined, { isActive: () => active, onOpenCreated: (outcome) => openCreatedSession({ outcome, serverId: "host-a", navigation }) });
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    const open = action(f.tree, "Open session")!.props.onPress as () => void;
    if (cancel === "close") active = false;
    else if (cancel === "unmount") f.unmount();
    else if (cancel === "host switch") f.setHost("host-b");
    else f.setOptions({ status: "offline" });
    open();
    expect(navigation.openAgent).not.toHaveBeenCalled();
    expect(readRecent("host-a").record.visits).toEqual([]);
    expect(readRecent("host-b").record.visits).toEqual([]);
    expect(model.rpc["sessions.create"]).toHaveBeenCalledTimes(1);
  });

  it.each(["close", "unmount", "host switch"])("never navigates or refreshes a late response after %s", async (cancel) => {
    let answer: (value: CreateOutcome) => void = () => {};
    let active = true;
    model.rpc["sessions.create"]!.mockImplementation(() => new Promise((resolve) => { answer = resolve; }));
    const f = form(undefined, { isActive: () => active });
    f.type("Route review"); f.press("kontor-new-lsa-create");
    const requestId = model.rpc["sessions.create"]!.mock.calls[0]![0].requestId;
    if (cancel === "close") active = false;
    else if (cancel === "unmount") f.unmount();
    else f.setHost("host-b");
    answer(outcome("verified", { requestId })); await settle(); f.render();
    expect(byTestId(f.tree, "kontor-new-lsa-verified")).toBeUndefined();
    expect(f.onOpenCreated).not.toHaveBeenCalled();
    expect(f.onRefresh).not.toHaveBeenCalled();
    expect(model.rpc["sessions.create"]).toHaveBeenCalledTimes(1);
    expect(readRecent("host-a").record.visits).toEqual([]);
  });

  it("keeps a mismatched request reply unknown instead of opening another session", async () => {
    model.rpc["sessions.create"]!.mockResolvedValue(outcome("verified"));
    const f = form(); f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(text(f.tree)).toContain("different request ID");
    expect(f.onOpenCreated).not.toHaveBeenCalled();
    expect(action(f.tree, "Check")).toBeDefined();
    expect(action(f.tree, "Back to the form")).toBeUndefined();
    expect(action(f.tree, "Open session")).toBeUndefined();
  });

  it.each(["close", "unmount", "host switch"])("ignores a late successful Check after %s", async (cancel) => {
    let answer!: (value: CreateOutcome) => void;
    let active = true;
    model.rpc["sessions.create-check"]!.mockImplementation(() => new Promise((resolve) => { answer = resolve; }));
    const pending = outcome("unknown");
    const f = form({ status: "ready", options: { ...OPTIONS, pending } }, { isActive: () => active });
    f.press("Check");
    if (cancel === "close") active = false;
    else if (cancel === "unmount") f.unmount();
    else f.setHost("host-b");
    answer(outcome("verified", { requestId: pending.requestId })); await settle(); f.render();
    expect(byTestId(f.tree, "kontor-new-lsa-verified")).toBeUndefined();
    expect(f.onOpenCreated).not.toHaveBeenCalled();
    expect(model.rpc["sessions.create"]).not.toHaveBeenCalled();
    expect(readRecent("host-a").record.visits).toEqual([]);
  });

  it("keeps the confirmed result visible while options refresh fails or the host disconnects", async () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    const f = form();
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    for (const options of [{ status: "loading" }, { status: "error", message: "read failed" }, { status: "offline" }] as const) {
      f.setOptions(options);
      expect(text(f.tree)).toContain("Created and verified");
      expect(text(f.tree)).not.toContain("Nothing was created");
    }
    expect(f.onOpenCreated).not.toHaveBeenCalled();
    expect(action(f.tree, "Open session")?.props.disabled).toBe(true);
  });

  it("turns an unknown outcome into a Check, never a retry", async () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("unknown"));
    model.rpc["sessions.create-check"]!.mockImplementation(reply("unknown", { message: "No session is visible yet. Creation may still complete." }));
    const f = form();
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(text(f.tree)).toContain("Outcome unknown");
    expect(action(f.tree, "Back to the form")).toBeUndefined();
    expect(action(f.tree, "Open session")).toBeUndefined();
    f.press("Check"); await settle(); f.render();
    expect(model.rpc["sessions.create-check"]).toHaveBeenCalledWith({ requestId: model.rpc["sessions.create"]!.mock.calls[0]![0].requestId });
    expect(text(f.tree)).toContain("Outcome unknown");
    expect(text(f.tree)).toContain("Creation may still complete");
    expect(model.rpc["sessions.create"]).toHaveBeenCalledTimes(1);
  });

  it("treats a lost reply as unknown with Check, using the same request", async () => {
    model.rpc["sessions.create"]!.mockRejectedValue(new Error("socket closed"));
    model.rpc["sessions.create-check"]!.mockImplementation(reply("verified"));
    const f = form();
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(byTestId(f.tree, "kontor-new-lsa-lost")).toBeDefined();
    expect(action(f.tree, "Back to the form")).toBeUndefined();
    expect(text(f.tree)).toContain("may or may not have reached the host");
    const requestId = model.rpc["sessions.create"]!.mock.calls[0]![0].requestId;
    f.press("Check"); await settle(); f.render();
    expect(model.rpc["sessions.create-check"]).toHaveBeenCalledWith({ requestId });
    expect(text(f.tree)).toContain("Created and verified");
    expect(f.onOpenCreated).not.toHaveBeenCalled();
    f.press("Open session");
    expect(f.onOpenCreated).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ requestId, agentId: "agent-new" }));
    expect(model.rpc["sessions.create"]).toHaveBeenCalledTimes(1);
  });

  it("returns a refusal to the form with the draft kept", async () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("refused", { message: "Mode plan is not offered by Claude. Nothing was created." }));
    const f = form();
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(text(f.tree)).toContain("Not created");
    f.press("Back to the form");
    expect(byTestId(f.tree, "kontor-new-lsa-topic")?.props.value).toBe("Route review");
  });

  it("shows an unsettled earlier creation with Check instead of the form", () => {
    const f = form({ status: "ready", options: { ...OPTIONS, pending: outcome("unknown") } });
    expect(byTestId(f.tree, "kontor-new-lsa-topic")).toBeUndefined();
    expect(byTestId(f.tree, "kontor-new-lsa-unknown")).toBeDefined();
    expect(action(f.tree, "Check")).toBeDefined();
  });

  describe("From Jira ticket", () => {
    const UNDER_ACTIVE = { ...ISSUE_STORY, key: "ASMA-101", summary: "Wire the ticket desk", parent: { key: "ASMA-100", summary: ISSUE_EPIC.summary, statusCategory: "In Progress", open: true } } as const;
    const JIRA = { ...OPTIONS, issues: [ISSUE_EPIC, UNDER_ACTIVE, ISSUE_STORY] };
    const radio = (tree: Element[], label: string) => tree.find((node) => node.props.accessibilityRole === "radio" && String(node.props.accessibilityLabel).startsWith(label));
    const choose = (f: ReturnType<typeof form>, label: string) => { (radio(f.tree, label)!.props.onPress as () => void)(); f.render(); };
    const edit = (f: ReturnType<typeof form>, id: string, value: string) => { (byTestId(f.tree, id)!.props.onChangeText as (value: string) => void)(value); f.render(); };
    const flushPicker = async (current: ReturnType<typeof form>) => { await vi.advanceTimersByTimeAsync(250); for (let i = 0; i < 4; i++) await Promise.resolve(); current.render(); };
    const jiraRow = (tree: Element[], key: string) => byTestId(tree, `kontor-jira-picker-row-${key}`);
    const openJiraPicker = async (current: ReturnType<typeof form>) => { current.press("Choose Jira issue"); await flushPicker(current); };
    const pickJira = (current: ReturnType<typeof form>, key: string) => { (jiraRow(current.tree, key)!.props.onPress as () => void)(); current.render(); };
    beforeEach(() => { vi.useFakeTimers(); });
    afterEach(() => { vi.useRealTimers(); });

    it("switches TPM to From Jira ticket, previews the epic's desk and submits the key with the default topic", async () => {
      model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
      model.rpc["sessions.create-options"] = vi.fn(async () => OPTIONS);
      const f = form();
      choose(f, "TPM");
      expect(radio(f.tree, "From Jira ticket")!.props["aria-checked"]).toBe(true);
      expect(byTestId(f.tree, "kontor-new-lsa-problem")?.props.children).toBe("Choose a Jira issue.");
      await openJiraPicker(f);
      pickJira(f, "ASMA-100");
      expect(byTestId(f.tree, "kontor-new-session-desk")?.props.children).toBe("Desk: DESK • ADAM, because ASMA-100 is an epic.");
      expect(byTestId(f.tree, "kontor-new-session-jira-topic")?.props.value).toBe("Deliver the desk layout");
      expect(text(f.tree)).toContain("Title: TPM • ASMA-100 • Deliver the desk layout.");
      f.press("kontor-new-lsa-create");
      expect(model.rpc["sessions.create"]).toHaveBeenCalledWith(expect.objectContaining({ role: "TPM", start: "from-jira", issueKey: "ASMA-100", projectId: "prj-adam", topic: "Deliver the desk layout" }));
    });

    it("routes a standalone ticket opened from ADAM to DESK • TICKETS and reads that desk's catalog first", async () => {
      model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
      model.rpc["sessions.create-options"] = vi.fn(async () => JIRA);
      const f = form({ status: "ready", options: JIRA });
      choose(f, "From Jira ticket");
      await flushPicker(f);
      expect(f.tree.some((node) => node.props.accessibilityRole === "radiogroup" && node.props.accessibilityLabel === "Desk")).toBe(false);
      pickJira(f, "ASMA-103");
      expect(f.onRefresh).toHaveBeenLastCalledWith({ projectId: "prj-tickets", role: "LSA", start: "from-jira", issueKey: "ASMA-103" });
      expect(byTestId(f.tree, "kontor-new-session-desk")?.props.children).toBe("Desk: DESK • TICKETS, because ASMA-103 is a User Story without an active parent epic.");
      expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
      f.setOptions({ status: "ready", options: { ...JIRA, catalogProjectId: "prj-tickets" } });
      expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(false);
      f.press("kontor-new-lsa-create");
      expect(model.rpc["sessions.create"]).toHaveBeenCalledWith(expect.objectContaining({ start: "from-jira", issueKey: "ASMA-103", projectId: "prj-tickets" }));
    });

    it("keeps the exact key when the title is edited or a later search no longer lists the issue", async () => {
      model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
      model.rpc["sessions.create-options"] = vi.fn(async () => JIRA);
      const f = form({ status: "ready", options: JIRA });
      choose(f, "TPM");
      await openJiraPicker(f);
      pickJira(f, "ASMA-100");
      edit(f, "kontor-new-session-jira-topic", "Desk layout follow-up");
      expect(byTestId(f.tree, "kontor-new-session-jira-topic")?.props.accessibilityLabel).toBe("Title topic after TPM • ASMA-100");
      expect(text(f.tree)).toContain("Title: TPM • ASMA-100 • Desk layout follow-up.");
      f.press("Change Jira issue");
      edit(f, "kontor-jira-picker-search", "ticket desk");
      await flushPicker(f);
      expect(model.rpc["sessions.create-options"]).toHaveBeenLastCalledWith(expect.objectContaining({ projectId: "prj-adam", role: "TPM", start: "from-jira", issueKey: "ticket desk" }));
      f.press("Back");
      f.setOptions({ status: "ready", options: { ...JIRA, issues: [UNDER_ACTIVE] } });
      expect(text(f.tree)).toContain("Title: TPM • ASMA-100 • Desk layout follow-up.");
      edit(f, "kontor-new-session-jira-topic", "ASMA-100 • Forged");
      expect(byTestId(f.tree, "kontor-new-lsa-problem")?.props.children).toBe("A topic cannot contain • or ·, which separate title parts.");
      edit(f, "kontor-new-session-jira-topic", "Desk layout follow-up");
      f.press("kontor-new-lsa-create");
      expect(model.rpc["sessions.create"]).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ role: "TPM", issueKey: "ASMA-100", projectId: "prj-adam", topic: "Desk layout follow-up" }));
    });

    it("shows a task under an active epic with its exact reason and a path to the owning epic, never as a choice", async () => {
      model.rpc["sessions.create-options"] = vi.fn(async () => JIRA);
      const f = form({ status: "ready", options: JIRA });
      choose(f, "From Jira ticket");
      await flushPicker(f);
      expect(radio(f.tree, "ASMA-101")).toBeUndefined();
      expect(jiraRow(f.tree, "ASMA-101")?.props.accessibilityState).toMatchObject({ disabled: true });
      expect(String(jiraRow(f.tree, "ASMA-101")?.props.accessibilityLabel)).toContain("ASMA-101 is under the active epic ASMA-100 (In Progress), whose ESW delivers it. Start from ASMA-100 instead.");
      f.press("Start from ASMA-100");
      await flushPicker(f);
      pickJira(f, "ASMA-100");
      expect(byTestId(f.tree, "kontor-new-session-desk")?.props.children).toBe("Desk: DESK • ADAM, because ASMA-100 is an epic.");
    });

    it("looks an unlisted owning epic up by its key on the ADAM catalog", async () => {
      model.rpc["sessions.create-options"] = vi.fn(async (input) => ({ ...JIRA, catalogProjectId: input.projectId ?? JIRA.catalogProjectId, issues: input.issueKey === "ASMA-100" ? [ISSUE_EPIC] : input.projectId === "prj-tickets" ? [UNDER_ACTIVE] : JIRA.issues }));
      const f = form({ status: "ready", options: { ...JIRA, catalogProjectId: "prj-tickets", issues: [UNDER_ACTIVE] } });
      choose(f, "From Jira ticket");
      await flushPicker(f);
      f.press("Start from ASMA-100");
      await flushPicker(f);
      expect(byTestId(f.tree, "kontor-jira-picker-search")?.props.value).toBe("ASMA-100");
      expect(model.rpc["sessions.create-options"]).toHaveBeenLastCalledWith(expect.objectContaining({ issueKey: "ASMA-100", start: "from-jira" }));
      pickJira(f, "ASMA-100");
      expect(f.onRefresh).toHaveBeenLastCalledWith({ projectId: "prj-adam", role: "LSA", start: "from-jira", issueKey: "ASMA-100" });
      f.setOptions({ status: "ready", options: { ...JIRA, catalogProjectId: "prj-adam", issues: [ISSUE_EPIC] } });
      expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(false);
    });

    /** Mirrors session-create options() gate at 0a37388c:441-448 until 702b opens browse from non-lead desks. */
    const serverGatedJiraBrowse = (input: CreateScope, catalog: typeof JIRA) => {
      const desk = catalog.desks.find((entry) => entry.projectId === (input.projectId ?? catalog.catalogProjectId));
      if (input.start !== "from-jira") return catalog.issues;
      if (!desk || desk.kind === "pr-review" || desk.kind === "other") return [];
      return catalog.issues;
    };

    it("offers From Jira on PR REVIEW but the server gate leaves browse empty until 702b opens search", async () => {
      model.rpc["sessions.create-options"] = vi.fn(async (input: CreateScope) => ({
        ...JIRA,
        catalogProjectId: input.projectId ?? JIRA.catalogProjectId,
        issues: serverGatedJiraBrowse(input, JIRA),
      }));
      const f = form({ status: "ready", options: { ...JIRA, catalogProjectId: "prj-adam" } });
      choose(f, "DESK • PR REVIEW");
      f.setOptions({ status: "ready", options: { ...JIRA, catalogProjectId: "prj-pr", issues: [] } });
      expect(radio(f.tree, "From Jira ticket")!.props["aria-disabled"]).toBe(false);
      choose(f, "From Jira ticket");
      await flushPicker(f);
      expect(model.rpc["sessions.create-options"]).toHaveBeenLastCalledWith(expect.objectContaining({ projectId: "prj-pr", start: "from-jira", issueClass: "eligible" }));
      expect(jiraRow(f.tree, "ASMA-100")).toBeUndefined();
      expect(text(f.tree)).toContain("No open ASMA issue matches");
    });

    it("names a ticket desk with no checkout instead of hiding the ticket", async () => {
      model.rpc["sessions.create-options"] = vi.fn(async () => ({ ...JIRA, desks: [OPTIONS.desks[0]!] }));
      const f = form({ status: "ready", options: { ...JIRA, desks: [OPTIONS.desks[0]!] } });
      choose(f, "From Jira ticket");
      await flushPicker(f);
      expect(String(jiraRow(f.tree, "ASMA-103")?.props.accessibilityLabel)).toContain("ASMA-103 starts on DESK • TICKETS, which has no checkout on this host.");
    });
  });

  it("names a disabled profile provider and its cause, promises no persona and substitutes nothing", () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    const work = { ...OPTIONS.providers[0]!, id: "codex-work", label: "Codex · Work (Carasent)", status: "disabled" as const, available: false, note: "Disabled on this host.", persona: "unverified" as const };
    const f = form({ status: "ready", options: { ...OPTIONS, providers: [...OPTIONS.providers, work], profile: { name: "LSA", provider: "codex-work", model: "opus", modeId: "default", effort: "max" } } });
    f.type("Route review");
    expect(byTestId(f.tree, "kontor-new-lsa-problem")?.props.children).toBe("Codex · Work (Carasent): Disabled on this host. The host's LSA profile chooses it. Choose an active provider.");
    expect(text(f.tree)).not.toContain("Persona:");
    expect(f.tree.find((node) => String(node.props.label).startsWith("Provider, model and mode"))?.props.label).toBe("Provider, model and mode: Codex · Work (Carasent), disabled");
    expect(f.tree.some((node) => node.props.accessibilityRole === "radio" && String(node.props.accessibilityLabel).startsWith("Codex"))).toBe(false);
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    f.press("kontor-new-lsa-create");
    expect(model.rpc["sessions.create"]).not.toHaveBeenCalled();
    (f.tree.find((node) => node.props.accessibilityRole === "radio" && node.props.accessibilityLabel === "Claude")!.props.onPress as () => void)(); f.render();
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(false);
  });

  it("says when the chosen role workspace is missing and will be created", () => {
    const f = form({ status: "ready", options: { ...OPTIONS, desks: OPTIONS.desks.map((desk) => desk.projectId === "prj-adam" ? { ...desk, lead: { ...desk.lead, ADHOC: null } } : desk) } });
    f.type("Route review");
    expect(text(f.tree)).toContain("The ADHOC workspace is created on this desk.");
  });

  it("keeps the handed state from a popover hand-off and spends the Open action", async () => {
    model.rpc["sessions.create"]!.mockImplementation(reply("verified"));
    const f = form();
    f.onOpenCreated.mockReturnValueOnce({ ok: true, handoff: true });
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    f.press("Open session");
    expect(text(f.tree)).toContain("Opening in Kontor…");
    expect(action(f.tree, "Open session")).toBeUndefined();
    expect(f.onOpenCreated).toHaveBeenCalledTimes(1);
  });
});
