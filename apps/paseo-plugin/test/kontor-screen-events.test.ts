import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import type { PaseoProject } from "@getpaseo/client";
import { WorkspaceDescriptorPayloadSchema } from "@getpaseo/protocol/messages";
import type { PluginHostSummary, PluginScreenProps } from "@getpaseo/plugin/client";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DirectoryEntry } from "../client/session-directory";
import { confirmedEpic } from "../client/page-data";
import { canonicalGraphSchema, jiraHistorySchema, type ReadSourceSummary, type SourceRead } from "../shared/read-sources";
import { Linking } from "react-native";

const model = vi.hoisted(() => ({
  projects: [] as PaseoProject[],
  workspaces: [] as import("@getpaseo/client").PaseoWorkspace[],
  slots: [] as unknown[], cursor: 0, entries: [] as DirectoryEntry[], sources: [] as ReadSourceSummary[], reads: {} as Record<string, SourceRead>, values: {} as object,
  hosts: [] as PluginHostSummary[], toastError: vi.fn(), toastShow: vi.fn(), copyText: vi.fn(async (_text: string) => {}), updates: 0,
  keyboardSave: vi.fn(async () => true),
  keyboard: { status: "loading" } as { status: "loading" } | { status: "ready"; values: { backShortcut: "off" | "cmd-backquote" } },
  shortcut: null as null | { shortcut: string; web: boolean; root: { readonly current: unknown }; canGoBack: () => boolean; back: () => void },
  useState<T>(initial: T | (() => T)): [T, (next: T | ((previous: T) => T)) => void] {
    const index = this.cursor++;
    if (!(index in this.slots)) this.slots[index] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [this.slots[index] as T, (next) => { this.updates++; this.slots[index] = typeof next === "function" ? (next as (previous: T) => T)(this.slots[index] as T) : next; }];
  },
}));
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(), useState: <T,>(initial: T | (() => T)) => model.useState(initial),
  useRef: <T,>(initial: T) => model.useState(() => ({ current: initial }))[0],
  useMemo: (factory: () => unknown) => factory(), useCallback: (callback: unknown) => callback,
  useEffect: () => {},
}));
vi.mock("react-native", () => ({
  Linking: { openURL: vi.fn(async () => {}) },
  SafeAreaView: "SafeAreaView", KeyboardAvoidingView: "KeyboardAvoidingView", ScrollView: "ScrollView", View: "View", Text: "Text", Pressable: "Pressable",
}));
vi.mock("@getpaseo/plugin/client", () => ({ useHosts: () => model.hosts }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({
  Modal: Object.assign(() => null, { Content: () => null }), ScrollView: "ScrollView", FlatList: "FlatList", TextInput: "TextInput", Icon: "Icon", useToast: () => ({ error: model.toastError, show: model.toastShow }),
  copyText: (text: string) => model.copyText(text),
}));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: { entries: model.entries, status: "ready", live: false, lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false }, refresh: () => {} }) }));
vi.mock("../client/use-organization-settings", () => ({ useOrganizationSettings: () => ({ state: { status: "ready", values: model.values, saving: false, saveError: null }, save: async () => true }) }));
vi.mock("../client/use-read-sources", () => ({ useReadSources: () => ({ sources: model.sources, reads: model.reads, teamsSnapshots: {}, error: null, loading: false, available: true }) }));
vi.mock("../client/use-workspace-directory", () => ({ useWorkspaceDirectory: () => ({ entries: model.workspaces, error: null, readAt: null, projects: model.projects, projectError: null, projectsReadAt: null }) }));
vi.mock("../client/use-keyboard-settings", () => ({ useKeyboardSettings: () => ({ state: model.keyboard, save: model.keyboardSave }) }));
vi.mock("../client/keyboard-shortcuts", async (original) => ({
  ...await original<typeof import("../client/keyboard-shortcuts")>(),
  useBackShortcut: (shortcut: string, web: boolean, root: { readonly current: unknown }, canGoBack: () => boolean, back: () => void) => { model.shortcut = { shortcut, web, root, canGoBack, back }; },
}));

import { KontorScreen } from "../client/kontor-screen";
import { DESTINATIONS, hostNavigation, navigate, readRouteView, sessionScopeKey, type KontorRoute } from "../client/navigation-state";
import { readSessionView } from "../client/session-view-store";
import type { BoardSessions } from "../client/board-sessions";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { sessionEntry } from "./session-fixtures";
import { GraphPage } from "../client/graph-page";
import { historicalPlanProgress } from "../client/page-data";
import { AnchoredMenu, MenuItem } from "../client/anchored-menu";
import { SessionsScreen } from "../client/sessions-screen";
import { NativeFreshnessPill } from "../client/native-freshness";
import { DEFAULT_PLAN_VIEW, type PlanView } from "../client/plan-reader";
import { ReadOnlyPage } from "../client/read-only-pages";
import { TeamsSourcePage } from "../client/teams-source-page";
import { OrganizationSettingsPage } from "../client/organization-settings-page";
import { KeyboardSettingsCard } from "../client/keyboard-settings-card";

const actualGraph = canonicalGraphSchema.parse(JSON.parse(readFileSync(new URL("./fixtures/plan-graph-asma-8371.json", import.meta.url), "utf8")));
const capturedHistory = jiraHistorySchema.parse(JSON.parse(readFileSync(new URL("./fixtures/jira-history.json", import.meta.url), "utf8")));
function elements(value: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(value)) return [];
  return [value, ...elements(value.props.children as ReactNode)];
}
let sequence = 0;
beforeEach(() => { model.projects = []; model.workspaces = []; vi.clearAllMocks(); model.slots = []; model.cursor = 0; model.updates = 0; model.values = DEFAULT_ORGANIZATION_SETTINGS; model.entries = [sessionEntry("native", { labels: { epic: "ASMA-1" } })]; model.sources = []; model.reads = {}; model.hosts = []; model.keyboard = { status: "loading" }; model.shortcut = null; });
/** Renders the header selector as the screen would, so its own semantics and style are visible. */
function pickerField(picker: ReactElement<Record<string, unknown>>): ReactElement<Record<string, unknown>> {
  return (picker.type as (props: Record<string, unknown>) => ReactElement<Record<string, unknown>>)(picker.props);
}
/** The open More menu's views, which must sit in a shrinkable scroll view so a short screen scrolls them instead of cutting them off. */
function moreViews(menu: ReactElement<Record<string, unknown>> | undefined): string[] {
  const children = [menu?.props.children].flat() as ReactElement<Record<string, unknown>>[];
  expect(children.map((child) => child.type)).toEqual(["ScrollView"]);
  expect(children[0]!.props).toMatchObject({ testID: "kontor-more-choices", keyboardShouldPersistTaps: "handled", style: { flexGrow: 0, flexShrink: 1 } });
  const items = [children[0]!.props.children].flat(Infinity) as ReactElement<Record<string, unknown>>[];
  expect(items.every((item) => item.type === MenuItem)).toBe(true);
  return items.map((item) => String(item.props.label));
}
function mount(compact: boolean, initial?: Partial<KontorRoute>) {
  const props: PluginScreenProps = { host: { id: `shell-${++sequence}`, label: "Host" }, layout: { compact, platform: "ios" }, params: {}, navigation: { openAgent: vi.fn(), openWorkspace: vi.fn(), openBrowser: vi.fn() },
    theme: { colors: { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } } };
  if (initial) navigate(props.host.id, initial);
  const entry = KontorScreen(props);
  const component = entry.type as (props: PluginScreenProps) => ReactNode;
  let tree: ReactNode, drawn = 0;
  const render = () => { model.cursor = 0; drawn = model.updates; tree = component(props); };
  render();
  return {
    props, render,
    /** Like React, draws again only when a state update was scheduled since the last render. */
    flush() { if (model.updates !== drawn) render(); },
    action(label: string) { const action = elements(tree).find((element) => element.props.label === label); if (!action) throw new Error(`Missing action ${label}`); return action; },
    click(label: string) { const action = this.action(label); const press = action.props.onPress; expect(typeof press).toBe("function"); if (typeof press === "function") press(); render(); },
    elements() { return elements(tree); },
    picker() { return elements(tree).find((node) => typeof node.type === "function" && node.type.name === "ContextPicker"); },
    /** Opens the header selector, optionally checking the title it shows first. */
    openPicker(title?: string) { const picker = this.picker(); if (!picker) throw new Error("Missing context picker"); if (title !== undefined) expect(picker.props.label).toBe(title); (picker.props.onPress as () => void)(); render(); },
    menu() { return elements(tree).find((node) => node.type === AnchoredMenu); },
    menuItems() { return elements(tree).filter((node) => node.type === MenuItem); },
    choose(name: string) {
      const item = this.menuItems().find((node) => node.props.label === name || node.props.accessibilityLabel === name || String(node.props.accessibilityLabel).startsWith(`${name}, `));
      if (!item) throw new Error(`Missing menu item ${name}`);
      (item.props.onPress as () => void)(); render();
    },
    closeMenu() { const menu = this.menu(); if (!menu) throw new Error("No open menu"); (menu.props.onClose as () => void)(); render(); },
    /** Plan choices in the open picker, without their Jira key. */
    choices() { return this.menuItems().map((node) => String(node.props.accessibilityLabel)).filter((label) => label.startsWith("Plan: ")).map((label) => label.split(", ")[0]); },
  };
}
describe("Kontor shell native actions", () => {
  it("offers a native desk with no workspaces or sessions and supplies it to Sessions", () => {
    model.projects = [{ projectId: "prj_empty", projectKey: "fixture:empty", projectDisplayName: "Desk", projectCustomName: "DESK • RESEARCH", projectRootPath: "/fixture/empty", projectKind: "non_git", projectCustomIconRevision: null, projectIconRevision: "automatic:none:v1" }];
    const screen = mount(true, { tab: "Overview" });
    screen.openPicker();
    expect(screen.menuItems().map((node) => node.props.label)).toContain("DESK • RESEARCH");
    screen.choose("DESK • RESEARCH");
    expect(hostNavigation(screen.props.host.id).route.projectId).toBe("prj_empty");
    screen.click("Sessions");
    const sessions = screen.elements().find((node) => node.type === SessionsScreen)!;
    expect(sessions.props.projects).toEqual([{ id: "prj_empty", name: "DESK • RESEARCH", root: "/fixture/empty" }, { id: "prj_1", name: "Project" }]);
    expect(sessions.props.workspaces).toEqual([]);
  });

  it.each([true, false])("keeps the title bounded and the native project distinct from the epic in the accessible label (compact=%s)", (compact) => {
    const title = "A long configured source title that must remain available to assistive technology";
    model.sources = [{ id: "long-plan", title, kind: "plan", path: "/configured/long.md", projectId: null, epic: "ASMA-8539", workstream: null, revisionKind: "working copy" }];
    const screen = mount(compact, { tab: "Plan", sourceId: "long-plan", epicKey: "ASMA-8539" });
    const row = screen.elements().find((node) => node.props.testID === "kontor-context-row")!;
    expect(row.props.style).toMatchObject({ minHeight: 51 });
    expect(elements(row).some((node) => node.props.accessibilityRole === "header")).toBe(false);
    expect(elements(row).some((node) => node.props.children === screen.props.host.label)).toBe(false);
    expect(screen.picker()!.props).toMatchObject({ label: title, epicKey: "ASMA-8539", expanded: false });
    const field = pickerField(screen.picker()!);
    expect(field.props).toMatchObject({ testID: "kontor-context-picker", accessibilityRole: "button", accessibilityLabel: `Plan: ${title}. ASMA-8539 · All projects`, accessibilityState: { expanded: false } });
    // React Native Web drops accessibilityState; the web trigger states open or closed through ARIA.
    expect(field.props["aria-expanded"]).toBe(false);
    expect(field.props.style).toMatchObject({ minHeight: 44, maxWidth: 420 });
    expect(elements(field).find((node) => node.props.numberOfLines === 1)?.props.ellipsizeMode).toBe("tail");
    const key = elements(field).find((node) => node.props.testID === "kontor-context-epic")!;
    expect(key.props.children).toBe("ASMA-8539 · All projects"); expect(key.props.style).toMatchObject({ fontSize: 11 });
    expect(key.props.accessible).toBe(false); expect(key.props.pointerEvents).toBe("none");
    screen.openPicker(title);
    expect(screen.picker()!.props.expanded).toBe(true);
    expect(pickerField(screen.picker()!).props["aria-expanded"]).toBe(true);
    expect(screen.menu()!.props).toMatchObject({ testID: "kontor-context-menu", dialog: true, label: "Choose plan or project" });
    expect(screen.menuItems().find((node) => node.props.label === title)?.props).toMatchObject({ accessibilityLabel: `Plan: ${title}, ASMA-8539`, detail: "ASMA-8539", selected: true });
    screen.choose("All projects");
    expect(screen.menu()).toBeUndefined();
    expect(screen.picker()!.props).toMatchObject({ label: "All projects", epicKey: null, expanded: false });
    expect(pickerField(screen.picker()!).props["aria-expanded"]).toBe(false);
    expect(pickerField(screen.picker()!).props.accessibilityLabel).toBe("All projects");
    // Dismissing without a choice (Escape, a tap outside, Android back) closes through onClose.
    screen.openPicker("All projects");
    expect(pickerField(screen.picker()!).props["aria-expanded"]).toBe(true);
    screen.closeMenu();
    expect(screen.menu()).toBeUndefined();
    expect(pickerField(screen.picker()!).props["aria-expanded"]).toBe(false);
    expect(elements(pickerField(screen.picker()!)).some((node) => node.props.testID === "kontor-context-epic")).toBe(false);
    expect(screen.elements().find((node) => node.props.testID === "kontor-context-row")?.props.style).toMatchObject({ minHeight: 51 });
  });
  it.each([true, false])("does not display an unconfirmed source epic as a Jira key (compact=%s)", (compact) => {
    model.sources = [{ id: "source", title: "Source", kind: "plan", path: "/configured/source.md", projectId: null, epic: "unconfirmed", workstream: null, revisionKind: "working copy" }];
    const screen = mount(compact, { tab: "Plan", sourceId: "source" });
    expect(screen.picker()!.props).toMatchObject({ label: "Source", epicKey: null });
    expect(pickerField(screen.picker()!).props.accessibilityLabel).toBe("Plan: Source. All projects");
    expect(elements(pickerField(screen.picker()!)).find((node) => node.props.testID === "kontor-context-epic")?.props.children).toBe("All projects");
    screen.openPicker("Source");
    expect(screen.menuItems().find((node) => node.props.label === "Source")?.props).toMatchObject({ accessibilityLabel: "Plan: Source, unconfirmed", detail: null });
  });
  it("forwards actual SDK host status to the shared native freshness pill", () => {
    const screen = mount(true);
    for (const status of ["idle", "connecting", "online", "offline", "error"] as const) {
      model.hosts = [{ serverId: screen.props.host.id, label: "Host", status }]; screen.render();
      expect(screen.elements().find((node) => node.type === NativeFreshnessPill)?.props.hostStatus).toBe(status);
    }
  });
  it("keeps desktop Back available in place and enables it only when navigation has history", () => {
    const screen = mount(false);
    expect(screen.action("Back").props.disabled).toBe(true);
    const route = { ...hostNavigation(screen.props.host.id).route };
    screen.click("Overview");
    expect(screen.action("Back").props.disabled).toBe(false);
    screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route).toEqual(route);
    expect(screen.action("Back").props.disabled).toBe(true);
    expect(screen.elements().filter((node) => node.props.accessibilityRole === "tablist")).toHaveLength(1);
  });
  it("keeps host attribution available after removing the duplicate visible host label", () => {
    const screen = mount(false);
    expect(screen.elements().find((node) => node.type === NativeFreshnessPill)?.props.hostLabel).toBe("Host");
  });
  it("opens standalone decision evidence through a guarded callback without a selected plan and reports opener failures", async () => {
    model.sources = [{ id: "decisions", title: "Decisions", kind: "decisions", path: "/configured/decisions.json", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }];
    const screen = mount(true, { tab: "Decisions" });
    const page = screen.elements().find((node) => typeof node.props.onExternalLink === "function")!;
    const open = page.props.onExternalLink as (target: string) => void;
    expect(page.props.sourceId).toBeNull();
    open("https://example.com/decision"); expect(Linking.openURL).toHaveBeenCalledWith("https://example.com/decision");
    for (const url of ["http://example.com", "javascript:alert(1)", "https://user:secret@example.com", "https://example..com", "https://example.com:99999", "https:/malformed"]) open(url);
    expect(Linking.openURL).toHaveBeenCalledTimes(1); expect(model.toastError).toHaveBeenCalledTimes(6);
    vi.mocked(Linking.openURL).mockRejectedValueOnce(new Error("No opener"));
    open("https://example.com/evidence"); await Promise.resolve();
    expect(model.toastError).toHaveBeenLastCalledWith("This device could not open the source link.");
  });
  it.each(["Plan", "Graph"] as const)("copies %s exports to the clipboard and says when this device refuses", async (tab) => {
    const screen = mount(true, { tab });
    const page = screen.elements().find((node) => typeof node.props.onCopy === "function")!;
    const copy = page.props.onCopy as (text: string, what: string) => void;
    copy("exact text", `${tab} export`); await Promise.resolve(); await Promise.resolve();
    expect(model.copyText).toHaveBeenCalledExactlyOnceWith("exact text");
    expect(model.toastShow).toHaveBeenCalledExactlyOnceWith(`${tab} export copied`); expect(model.toastError).not.toHaveBeenCalled();
    model.copyText.mockRejectedValueOnce(new Error("denied"));
    copy("exact text", `${tab} export`); await Promise.resolve(); await Promise.resolve();
    expect(model.toastError).toHaveBeenCalledExactlyOnceWith(`${tab} export not copied: the clipboard is unavailable or denied on this device.`);
    expect(model.toastShow).toHaveBeenCalledTimes(1);
  });
  it("reserves the compact context/freshness row while keeping conversation return accessible", () => {
    const compact = mount(true, { tab: "Plan", originAgentId: "native" });
    const row = compact.elements().find((node) => node.props.testID === "kontor-context-row")!;
    expect(elements(row).some((node) => node.props.label === "Return to conversation")).toBe(false);
    expect(compact.action("Return to conversation").props.disabled).toBe(false);
    expect(compact.elements().some((node) => node.props.testID === "kontor-conversation-return-row")).toBe(true);
    const desktop = mount(false, { tab: "Plan", originAgentId: "native" });
    const wideRow = desktop.elements().find((node) => node.props.testID === "kontor-context-row")!;
    expect(elements(wideRow).some((node) => node.props.label === "Return to conversation")).toBe(true);
  });
  it("resolves epic → Graph/Plan only for an exact unique catalog plan, and preserves Back", () => {
    model.sources = [{ id: "explicit", title: "Chosen source", kind: "plan", path: "/plans/explicit.md", projectId: null, epic: "ASMA-1", workstream: null, revisionKind: "working copy" }];
    const screen = mount(true);
    expect(screen.picker()!.props.label).toBe("All projects");
    const sessions = screen.elements().find((node) => node.type === SessionsScreen)!;
    expect(sessions.props.planSources).toEqual({ "ASMA-1": ["explicit"] });
    (sessions.props.onEpicTab as (tab: string, epicKey: string, sourceId: string | null) => void)("Graph", "ASMA-1", null); screen.render();
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Graph", epicKey: "ASMA-1", sourceId: "explicit", originAgentId: null });
    expect(screen.picker()!.props).toMatchObject({ label: "Chosen source", epicKey: "ASMA-1" });
    screen.click("Plan"); expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Plan", epicKey: "ASMA-1", sourceId: "explicit" });
    screen.click("Back"); expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Graph", epicKey: "ASMA-1", sourceId: "explicit" });
    screen.click("Back"); expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Sessions", epicKey: null, sourceId: null });
    // Back also restores the earlier work context, so Plan opens unscoped again.
    screen.click("Plan"); expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Plan", epicKey: null, sourceId: null });
  });
  it("keeps multiple linked sources as accessible choices and reserves the context row before desktop tabs", () => {
    model.sources = ["one", "two"].map((id) => ({ id, title: id, kind: "plan", path: `/plans/${id}.md`, projectId: null, epic: "ASMA-1", workstream: null, revisionKind: "working copy" }));
    const screen = mount(false, { tab: "Graph", epicKey: "ASMA-1" });
    expect(hostNavigation(screen.props.host.id).route.sourceId).toBeNull();
    expect(screen.picker()!.props).toMatchObject({ label: "Choose plan · 2 linked", epicKey: "ASMA-1" });
    const graph = screen.elements().find((node) => node.type === GraphPage)!;
    (graph.props.onChooseSource as () => void)(); screen.render();
    expect(screen.menu()?.props.testID).toBe("kontor-context-menu");
    expect(screen.choices()).toEqual(["Plan: one", "Plan: two"]);
    expect(screen.elements().findIndex((node) => node.props.testID === "kontor-context-row")).toBeLessThan(screen.elements().findIndex((node) => node.props.accessibilityRole === "tablist"));
    screen.choose("Plan: two"); expect(hostNavigation(screen.props.host.id).route.sourceId).toBe("two");
  });
  it("routes Plan document links through configured sources and exact native IDs", () => {
    model.sources = ["a", "b"].map((id) => ({ id, title: id, kind: "plan", path: `/plans/${id}.md`, projectId: null, epic: "ASMA-1", workstream: null, revisionKind: "working copy" }));
    const screen = mount(true, { tab: "Plan", sourceId: "a" });
    const page = () => screen.elements().find((node) => typeof node.props.onDocumentLink === "function")!;
    (page().props.onDocumentLink as (target: string) => void)("paseo://agents/native");
    expect(screen.props.navigation?.openAgent).toHaveBeenCalledWith({ agentId: "native", serverId: screen.props.host.id });
    (page().props.onDocumentLink as (target: string) => void)("paseo://agents/missing"); expect(screen.props.navigation?.openAgent).toHaveBeenCalledTimes(1);
    (page().props.onDocumentLink as (target: string) => void)("https://carasent.atlassian.net/browse/ASMA-8541");
    expect(Linking.openURL).toHaveBeenCalledWith("https://carasent.atlassian.net/browse/ASMA-8541");
    (page().props.onDocumentLink as (target: string) => void)("./b.md"); screen.render();
    expect(hostNavigation(screen.props.host.id).route.sourceId).toBe("b"); screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route.sourceId).toBe("a");
  });
  it("wires Board scroll state into history before opening Plan and restores it on Back", () => {
    const screen = mount(true, { tab: "Board", sourceId: "plan-a", sourceRevision: "sha-a" });
    const board = () => screen.elements().find((element) => element.type === GraphPage)!;
    (board().props.onScroll as (offset: number) => void)(800);
    (board().props.onPlan as (sourceId: string) => void)("plan-a"); screen.render();
    expect(hostNavigation(screen.props.host.id).route.tab).toBe("Plan"); screen.click("Back");
    expect(board().props.initialOffset).toBe(800);
  });
  it("opens Sessions unscoped with a Board task's exact label join and the workspace through host navigation", () => {
    const screen = mount(true, { tab: "Board", sourceId: "plan-a", sourceRevision: "sha-a" });
    const sessions = screen.elements().find((element) => element.type === GraphPage)!.props.sessions as BoardSessions;
    sessions.onOpenWorkspace!("wks_a");
    expect(screen.props.navigation?.openWorkspace).toHaveBeenCalledWith({ workspaceId: "wks_a", serverId: screen.props.host.id });
    const task = { jira: "ASMA-7", kontorId: null, label: "ASMA-7" };
    sessions.onShowInSessions(task); screen.render();
    const route = hostNavigation(screen.props.host.id).route;
    expect(route).toMatchObject({ tab: "Sessions", projectId: null, epicKey: null });
    expect(readSessionView(sessionScopeKey(screen.props.host.id, route))).toMatchObject({ task, query: "", status: "all", scrollOffset: 0 });
    screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route.tab).toBe("Board");
  });
  it("keeps the native conversation and its association boundary while choosing a linked plan", () => {
    model.values = { ...DEFAULT_ORGANIZATION_SETTINGS, planAssociations: { native: ["linked-a", "linked-b"] } };
    model.sources = ["linked-a", "linked-b", "unrelated"].map((id, index) => ({ id, title: id, kind: "plan", workstream: null, epic: `ASMA-${index + 1}`, projectId: `project-${index + 1}`, path: `/configured/${id}.md`, revisionKind: "working copy" }));
    const screen = mount(true, { tab: "Plan", originAgentId: "native" });
    const page = () => screen.elements().find((element) => typeof element.props.onSource === "function")!;
    expect((page().props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(["linked-a", "linked-b"]);
    (page().props.onSource as (id: string) => void)("linked-b"); screen.render();
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ sourceId: "linked-b", originAgentId: "native" });
    expect(screen.action("Return to conversation").props.disabled).toBe(false);
    expect((page().props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(["linked-b"]);
    screen.openPicker("linked-b");
    expect(screen.choices()).toEqual(["Plan: linked-a", "Plan: linked-b"]);
    expect(screen.menuItems().map((node) => node.props.label)).toEqual(["All projects", "linked-a", "linked-b"]);
    screen.closeMenu(); screen.click("More"); screen.choose("Graph");
    const linkedGraph = screen.elements().find((node) => node.type === GraphPage)!;
    (linkedGraph.props.onPlan as (sourceId: string) => void)("linked-b"); screen.render();
    expect(hostNavigation(screen.props.host.id).route.originAgentId).toBe("native");
    screen.click("Return to conversation");
    expect(screen.props.navigation?.openAgent).toHaveBeenCalled();
  });
  it.each(["Graph", "Board"] as const)("keeps the originating conversation and linked choices after selecting another plan from %s", (tab) => {
    model.values = { ...DEFAULT_ORGANIZATION_SETTINGS, planAssociations: { native: ["linked-a", "linked-b"] } };
    model.sources = ["linked-a", "linked-b", "unrelated"].map((id, index) => ({ id, title: id, kind: "plan", path: `/configured/${id}.md`, projectId: index === 0 ? "project-a" : "project-b", epic: index === 0 ? "ASMA-1" : "ASMA-2", workstream: null, revisionKind: "working copy" }));
    const screen = mount(true, { tab: "Plan", originAgentId: "native" });
    const page = screen.elements().find((node) => typeof node.props.onSource === "function")!;
    (page.props.onSource as (id: string) => void)("linked-a"); screen.render();
    if (tab === "Graph") { screen.click("More"); screen.choose("Graph"); } else screen.click(tab);
    screen.openPicker("linked-a"); screen.choose("Plan: linked-b");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab, sourceId: "linked-b", originAgentId: "native" });
    expect(screen.action("Return to conversation").props.disabled).toBe(false);
    screen.click("Return to conversation");
    expect(screen.props.navigation?.openAgent).toHaveBeenCalledWith({ agentId: "native", serverId: screen.props.host.id });
    screen.openPicker("linked-b");
    const choices = () => screen.choices();
    expect(choices()).toEqual(["Plan: linked-a", "Plan: linked-b"]);
    screen.closeMenu(); screen.click("Plan");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Plan", sourceId: "linked-b", originAgentId: "native" });
    const selectedPlan = screen.elements().find((node) => typeof node.props.onSource === "function")!;
    expect((selectedPlan.props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(["linked-b"]);
    screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab, sourceId: "linked-b", originAgentId: "native" });
    screen.openPicker("linked-b"); expect(choices()).toEqual(["Plan: linked-a", "Plan: linked-b"]);
    screen.choose("All projects");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ originAgentId: null, sourceId: null, projectId: null, epicKey: null });
    screen.openPicker("All projects"); expect(choices()).toContain("Plan: unrelated"); screen.closeMenu();
    screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab, sourceId: "linked-b", originAgentId: "native" });
    screen.openPicker("linked-b"); expect(choices()).toEqual(["Plan: linked-a", "Plan: linked-b"]);
  });
  it.each([
    { tab: "Graph", projectId: null }, { tab: "Graph", projectId: "linked-project" },
    { tab: "Board", projectId: null }, { tab: "Board", projectId: "linked-project" },
  ] as const)("retains exact-key historical evidence on linked $tab with history project $projectId", ({ tab, projectId }) => {
    const plan: SourceRead = { source: { id: "linked-plan", title: "Linked plan", kind: "plan", path: actualGraph.source, projectId: "linked-project", epic: actualGraph.epic.jira, workstream: null, revisionKind: "working copy" }, status: "ready", revision: actualGraph.source_sha256, readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: "# Configured plan", graph: actualGraph, records: null, diagnostics: [] };
    const jira: SourceRead = { ...plan, source: { ...plan.source, id: "jira-history", title: "Historical Jira", kind: "jira", epic: null, projectId, path: "/configured/history.json" }, revision: capturedHistory.provenance.sha256, content: null, graph: null, jiraHistory: capturedHistory };
    const unrelated: SourceRead = { ...plan, source: { ...plan.source, id: "unrelated-plan" } };
    const otherProject: SourceRead = { ...jira, source: { ...jira.source, id: "other-project-history", projectId: "other-project" } };
    model.values = { ...DEFAULT_ORGANIZATION_SETTINGS, planAssociations: { native: [plan.source.id] } };
    model.sources = [plan.source, jira.source, unrelated.source, otherProject.source];
    model.reads = Object.fromEntries([plan, jira, unrelated, otherProject].map((read) => [read.source.id, read]));
    const screen = mount(true, { tab: "Plan", originAgentId: "native", sourceId: plan.source.id, sourceRevision: plan.revision, projectId: plan.source.projectId, epicKey: plan.source.epic });
    if (tab === "Graph") { screen.click("More"); screen.choose("Graph"); } else screen.click(tab);
    const component = screen.elements().find((node) => node.type === GraphPage)!;
    const forwarded = component.props.reads as Record<string, SourceRead>;
    expect((component.props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(["linked-plan", "jira-history"]);
    expect(Object.keys(forwarded)).toEqual(["linked-plan", "jira-history"]);
    expect(forwarded["jira-history"]).toBe(jira);
    expect(forwarded["jira-history"]?.jiraHistory?.provenance).toEqual(capturedHistory.provenance);
    const progress = historicalPlanProgress(actualGraph, Object.values(forwarded));
    expect(progress).toEqual(historicalPlanProgress(actualGraph, [jira]));
    expect(progress.known).toBe(19); expect(progress.unknown).toBe(0);
    expect(progress.states[0]?.evidence?.observations).toMatchObject([{ issue: { key: actualGraph.nodes[0]!.jira, status: "Closed", cat: "done" }, sourceId: "jira-history", sourceTitle: "Historical Jira", observedAt: "2026-10-18T17:00+00:00" }]);
    expect(hostNavigation(screen.props.host.id).route.originAgentId).toBe("native");
    screen.click("Plan");
    const planPage = screen.elements().find((node) => typeof node.props.onSource === "function")!;
    expect(planPage.props.reads).toEqual(forwarded);
    screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab, sourceId: "linked-plan", originAgentId: "native" });
    const returned = screen.elements().find((node) => node.type === GraphPage)!;
    expect(returned.props.reads).toEqual(forwarded);
  });
  it.each(["unrelated", "linked-next"] as const)("leaves conversation filtering when Overview selects %s, and Back restores the linked Plan", (targetId) => {
    const linked: SourceRead = { source: { id: "linked-plan", title: "linked-plan", kind: "plan", path: actualGraph.source, projectId: null, epic: actualGraph.epic.jira, workstream: null, revisionKind: "working copy" }, status: "ready", revision: actualGraph.source_sha256, readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: "# Linked plan", graph: actualGraph, records: null, diagnostics: [] };
    const otherPlans: SourceRead[] = ["linked-next", "unrelated"].map((id) => ({ ...linked, source: { ...linked.source, id, title: id, path: `/configured/${id}.md` }, revision: "b".repeat(64), graph: null, content: `# Configured ${id}` }));
    const jira: SourceRead = { ...linked, source: { ...linked.source, id: "jira-history", title: "Historical Jira", kind: "jira", path: "/configured/history.json", epic: null }, revision: capturedHistory.provenance.sha256, graph: null, content: null, jiraHistory: capturedHistory };
    const configured = [linked, ...otherPlans, jira];
    model.sources = configured.map((read) => read.source); model.reads = Object.fromEntries(configured.map((read) => [read.source.id, read]));
    model.values = { ...DEFAULT_ORGANIZATION_SETTINGS, planAssociations: { native: ["linked-plan", "linked-next"] } };
    const screen = mount(true, { tab: "Plan", sourceId: linked.source.id, sourceRevision: linked.revision, epicKey: linked.source.epic, originAgentId: "native" });
    const page = () => screen.elements().find((node) => typeof node.props.onPlanView === "function")!;
    const linkedRoute = { ...hostNavigation(screen.props.host.id).route };
    (page().props.onPlanView as (view: PlanView) => void)({ mode: "source", target: null, revealed: false });
    (page().props.onScroll as (offset: number) => void)(120);
    screen.click("Overview");
    expect(hostNavigation(screen.props.host.id).route).toEqual({ ...linkedRoute, tab: "Overview" });
    (page().props.onScroll as (offset: number) => void)(800);
    screen.openPicker("linked-plan"); expect(screen.choices()).toEqual(["Plan: linked-plan", "Plan: linked-next", "Plan: unrelated"]);
    screen.choose(`Plan: ${targetId}`);
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", sourceId: targetId, originAgentId: null });
    screen.click("Plan");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Plan", sourceId: targetId, originAgentId: null });
    expect((page().props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(configured.map((read) => read.source.id));
    expect(page().props.reads).toEqual(model.reads);
    expect(page().props.planView).toEqual(DEFAULT_PLAN_VIEW); expect(page().props.initialOffset).toBe(0);
    expect(historicalPlanProgress(actualGraph, Object.values(page().props.reads as Record<string, SourceRead>))).toMatchObject({ known: 19, unknown: 0 });
    expect((page().props.reads as Record<string, SourceRead>)["jira-history"]).toBe(jira);
    expect(page().props.sourceId).toBe(targetId);
    expect((page().props.reads as Record<string, SourceRead>)[targetId]?.content).toBe(`# Configured ${targetId}`);
    expect(screen.elements().some((node) => node.props.label === "Return to conversation")).toBe(false);
    screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", sourceId: targetId, originAgentId: null });
    screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route).toEqual({ ...linkedRoute, tab: "Overview" });
    expect(page().props.initialOffset).toBe(800);
    screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route).toEqual(linkedRoute);
    expect(page().props.planView).toMatchObject({ mode: "source" }); expect(page().props.initialOffset).toBe(120);
    expect(screen.action("Return to conversation").props.disabled).toBe(false);
    expect(readRouteView(`other-${screen.props.host.id}`, linkedRoute)).toEqual({ query: "", offset: 0 });
  });
  it("clears shared selection only through All projects; Back restores the previous context", () => {
    model.sources = ["linked-plan", "linked-next"].map((id) => ({ id, title: id, kind: "plan", path: `/configured/${id}.md`, projectId: null, epic: "ASMA-1", workstream: null, revisionKind: "working copy" }));
    model.values = { ...DEFAULT_ORGANIZATION_SETTINGS, planAssociations: { native: ["linked-plan", "linked-next"] } };
    const screen = mount(true, { tab: "Plan", sourceId: "linked-plan", epicKey: "ASMA-1", originAgentId: "native" });
    const linkedRoute = { ...hostNavigation(screen.props.host.id).route };
    screen.click("Overview"); screen.openPicker("linked-plan"); screen.choose("All projects");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", sourceId: null, epicKey: null, originAgentId: null });
    screen.click("Plan"); screen.click("Sessions");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Sessions", sourceId: null, epicKey: null, projectId: null, originAgentId: null });
    expect(screen.picker()!.props.label).toBe("All projects");
    screen.click("Back"); screen.click("Back"); screen.click("Back");
    expect(hostNavigation(screen.props.host.id).route).toEqual({ ...linkedRoute, tab: "Overview" });
    expect(screen.action("Return to conversation").props.disabled).toBe(false);
  });
  it.each([true, false])("keeps native project and exact plan through every main view (compact=%s)", (compact) => {
    model.entries = [sessionEntry("one", { projectKey: "native-project", projectName: "Long native project name", labels: { epic: "ASMA-8539" } })];
    model.sources = [{ id: "plan", title: "Chosen plan", kind: "plan", path: "/configured/plan.md", projectId: "native-project", epic: "ASMA-8539", workstream: null, revisionKind: "working copy" }];
    const screen = mount(compact);
    screen.openPicker("All projects"); screen.choose("Plan: Chosen plan");
    const chosen = { ...hostNavigation(screen.props.host.id).route };
    expect(pickerField(screen.picker()!).props.accessibilityLabel).toBe("Plan: Chosen plan. Epic sessions · ASMA-8539 · Project: Long native project name");
    for (const tab of ["Overview", "Plan", "Board", "Graph", "Delivery", "Sessions"] as const) {
      if (compact && (tab === "Graph" || tab === "Delivery")) { screen.click("More"); screen.choose(tab); } else screen.click(tab);
      expect(hostNavigation(screen.props.host.id).route).toEqual({ ...chosen, tab });
      expect(screen.picker()!.props).toMatchObject({ label: "Chosen plan", epicKey: "ASMA-8539", projectLabel: "Project: Long native project name" });
    }
    screen.click("Back"); expect(hostNavigation(screen.props.host.id).route).toEqual({ ...chosen, tab: "Delivery" });
    screen.openPicker(); screen.choose("All projects"); screen.openPicker(); screen.choose("Long native project name");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ projectId: "native-project", sourceId: null, epicKey: null });
    expect(pickerField(screen.picker()!).props.accessibilityLabel).toBe("Project: Long native project name");
    screen.click("Sessions"); expect(screen.elements().find((node) => node.type === SessionsScreen)?.props.projectId).toBe("native-project");
  });
  it("hands the Back shortcut the saved choice once read, on web only, and the same Back as the button", () => {
    const screen = mount(false);
    // Unread, the shortcut stays off; outside the web, there are no page keys to listen to.
    expect(model.shortcut).toMatchObject({ shortcut: "off", web: false });
    model.keyboard = { status: "ready", values: { backShortcut: "cmd-backquote" } };
    screen.props.layout = { compact: false, platform: "web" }; screen.render();
    expect(model.shortcut).toMatchObject({ shortcut: "cmd-backquote", web: true });
    expect(model.shortcut!.canGoBack()).toBe(false);
    screen.click("Board");
    expect(hostNavigation(screen.props.host.id).route.tab).toBe("Board");
    expect(model.shortcut!.canGoBack()).toBe(true);
    model.shortcut!.back(); screen.flush();
    expect(hostNavigation(screen.props.host.id).route.tab).toBe("Sessions");
    expect(model.shortcut!.canGoBack()).toBe(false);
    // It closes an open menu like the Back button does.
    screen.click("Board"); screen.click("More");
    expect(screen.menu()).toBeDefined();
    model.shortcut!.back(); screen.flush();
    expect(screen.menu()).toBeUndefined();
    expect(hostNavigation(screen.props.host.id).route.tab).toBe("Sessions");
    model.keyboard = { status: "ready", values: { backShortcut: "off" } }; screen.render();
    expect(model.shortcut).toMatchObject({ shortcut: "off", web: true });
  });
  it.each(["web", "ios"] as const)("gives Settings the Keyboard card with the keyboard document, the platform, the screen root and the clipboard (%s)", async (platform) => {
    model.keyboard = { status: "ready", values: { backShortcut: "cmd-backquote" } };
    const screen = mount(false, { tab: "Settings" });
    screen.props.layout = { compact: false, platform }; screen.render();
    const settings = screen.elements().find((node) => node.type === OrganizationSettingsPage)!;
    const card = settings.props.keyboard as ReactElement<Record<string, unknown>>;
    expect(card.type).toBe(KeyboardSettingsCard);
    expect(card.props.state).toBe(model.keyboard);
    expect(card.props.save).toBe(model.keyboardSave);
    expect(card.props.web).toBe(platform === "web");
    // The test watches the same screen root as the Back shortcut, and copies through the shell's clipboard action.
    expect(card.props.root).toBe(model.shortcut!.root);
    (card.props.copy as (text: string, what: string) => void)("Kontor received Cmd+`.", "Test result"); await Promise.resolve(); await Promise.resolve();
    expect(model.copyText).toHaveBeenCalledExactlyOnceWith("Kontor received Cmd+`.");
    expect(model.toastShow).toHaveBeenCalledExactlyOnceWith("Test result copied");
  });
  it("restores the Plan reader view through Back and keeps it per plan", () => {
    model.sources = ["plan-a", "plan-b"].map((id) => ({ id, title: id, kind: "plan", path: `/configured/${id}.md`, workstream: null, epic: null, projectId: null, revisionKind: "working copy" }));
    const screen = mount(true, { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
    const page = () => screen.elements().find((element) => typeof element.props.onPlanView === "function")!;
    (page().props.onPlanView as (view: PlanView) => void)({ mode: "source", target: null, revealed: false });
    (page().props.onScroll as (offset: number) => void)(120);
    screen.click("Overview"); screen.click("Back");
    expect(page().props.planView).toEqual({ mode: "source", target: null, revealed: false });
    expect(page().props.initialOffset).toBe(120);
    (page().props.onSource as (id: string) => void)("plan-b"); screen.render();
    expect(page().props.planView).toEqual(DEFAULT_PLAN_VIEW);
    expect(page().props.initialOffset).toBe(0);
    screen.click("Back"); expect(page().props.planView).toMatchObject({ mode: "source" });
  });
  it("opens a graph task at its exact declaration instead of a search query, and offers the graph from Plan", () => {
    const read: SourceRead = { source: { id: "plan-a", title: "plan-a", kind: "plan", path: "/configured/plan-a.md", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }, status: "ready", revision: "sha-a", readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: "# Plan", graph: null, records: null, diagnostics: [] };
    model.sources = [read.source]; model.reads = { "plan-a": read };
    const screen = mount(true, { tab: "Graph", sourceId: "plan-a", sourceRevision: "sha-a" });
    const graph = () => screen.elements().find((element) => element.type === GraphPage)!;
    (graph().props.onPlan as (sourceId: string, taskId?: string) => void)("plan-a", "T3"); screen.render();
    const route = hostNavigation(screen.props.host.id).route;
    expect(route).toMatchObject({ tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
    expect(readRouteView(screen.props.host.id, route).query).toBe("");
    const page = () => screen.elements().find((element) => typeof element.props.onPlanView === "function")!;
    expect(page().props.planView).toEqual({ mode: "formatted", target: { taskId: "T3", revision: "sha-a" }, revealed: false });
    expect(page().props.query).toBe("");
    (page().props.onGraph as () => void)(); screen.render();
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Graph", sourceId: "plan-a" });
    (graph().props.onPlan as (sourceId: string, taskId?: string) => void)("plan-a"); screen.render();
    expect(page().props.planView).toMatchObject({ target: { taskId: "T3" } });
  });
  it("opens configured teams from Settings with the configuration section expanded, and Back returns", () => {
    const screen = mount(true); screen.click("More"); screen.choose("Settings");
    const settings = screen.elements().find((element) => typeof element.props.onOpenTeams === "function")!;
    (settings.props.onOpenTeams as () => void)(); screen.render();
    const route = hostNavigation(screen.props.host.id).route;
    expect(route.tab).toBe("AI teams & sessions");
    const page = screen.elements().find((element) => typeof element.props.onTeamsView === "function")!;
    expect(page.props.teamsView).toMatchObject({ configured: true });
    screen.click("Back"); expect(hostNavigation(screen.props.host.id).route.tab).toBe("Settings");
  });
  it.each([true, false])("redraws the mounted Teams page as soon as its view changes, without leaving it (compact=%s)", (compact) => {
    model.sources = [{ id: "authoring", title: "Authoring fleet", kind: "teams", path: "/configured/authoring.yml", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }];
    const screen = mount(compact); screen.click("More"); screen.choose("Settings");
    const settings = screen.elements().find((element) => typeof element.props.onOpenTeams === "function")!;
    (settings.props.onOpenTeams as () => void)(); screen.render();
    const draw = (element: ReactElement<Record<string, unknown>>) => (element.type as (props: Record<string, unknown>) => ReactNode)(element.props);
    // Disclosures as the mounted page draws them now, through ReadOnlyPage and TeamsSourcePage.
    const disclosure = (label: string) => {
      const teams = elements(draw(screen.elements().find((element) => element.type === ReadOnlyPage)!)).find((element) => element.type === TeamsSourcePage)!;
      return elements(draw(teams)).find((element) => String(element.props.label).startsWith(label))!;
    };
    const toggle = (label: string, open: boolean) => { (disclosure(label).props.onToggle as (open: boolean) => void)(open); screen.flush(); };
    expect(disclosure("Configured teams & templates").props.open).toBe(true);
    toggle("Configured teams & templates", false);
    expect(disclosure("Configured teams & templates").props.open).toBe(false);
    toggle("Configured teams & templates", true);
    expect(disclosure("Configured teams & templates").props.open).toBe(true);
    // A per-source drill-down takes the same path as the role and template cards.
    expect(disclosure("Source details").props.open).toBe(false);
    toggle("Source details", true);
    expect(disclosure("Source details").props.open).toBe(true);
  });
  it("selects an exact plan in the compact Overview context picker and isolates hosts", () => {
    model.entries = [sessionEntry("one", { projectKey: "p-one", labels: { epic: "ASMA-1" } }), sessionEntry("two", { projectKey: "p-two", labels: { epic: "ASMA-2" } })];
    model.sources = [{ id: "source-one", title: "First plan", kind: "plan", path: "/configured/one.md", projectId: "p-one", epic: "ASMA-1", workstream: null, revisionKind: "working copy" }, { id: "source-two", title: "Second plan", kind: "plan", path: "/configured/two.md", projectId: "p-two", epic: "ASMA-2", workstream: null, revisionKind: "working copy" }];
    model.sources.push({ id: "decisions-one", title: "Recorded decisions", kind: "decisions", path: "/configured/decisions.json", projectId: "p-one", epic: "ASMA-1", workstream: null, revisionKind: "working copy" });
    const screen = mount(true); screen.click("Overview"); screen.openPicker("All projects"); screen.choose("Plan: First plan");
    const page = () => screen.elements().find((element) => typeof element.props.onPlanView === "function")!;
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", sourceId: "source-one", projectId: "p-one", epicKey: "ASMA-1" });
    expect((page().props.entries as DirectoryEntry[]).map((entry) => entry.id)).toEqual(["one"]);
    expect((page().props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(["source-one", "decisions-one"]);
    expect(screen.picker()!.props).toMatchObject({ label: "First plan", epicKey: "ASMA-1" });
    expect(hostNavigation("independent-overview-host").route.sourceId).toBeNull();
    screen.openPicker("First plan"); screen.choose("All projects");
    expect((page().props.entries as DirectoryEntry[]).map((entry) => entry.id)).toEqual(["one", "two"]);
    expect(screen.elements().filter((element) => element.props.tab === true)[0]?.props.icon).toBe("List");
  });
  it("reaches every destination on compact layouts through four tabs and an anchored More menu", () => {
    const screen = mount(true);
    const tabs = screen.elements().filter((element) => element.props.tab === true);
    expect(tabs.map((element) => element.props.label)).toEqual(["Sessions", "Overview", "Plan", "Board"]);
    expect(tabs.every((element) => element.props.stacked === true)).toBe(true);
    expect(screen.action("More").props).toMatchObject({ stacked: true, expanded: false, selected: false, accessibilityLabel: "More Kontor views" });
    expect(screen.action("More").props.tab).toBeUndefined();
    for (const tab of DESTINATIONS) {
      if (["Sessions", "Overview", "Plan", "Board"].includes(tab)) { screen.click(tab); expect(screen.action("More").props.selected).toBe(false); }
      else {
        screen.click("More");
        expect(screen.action("More").props.expanded).toBe(true);
        expect(screen.menu()!.props).toMatchObject({ testID: "kontor-more-menu", label: "More Kontor views", placement: { align: "stretch" } });
        expect(screen.menu()!.props.dialog).toBeUndefined();
        expect(screen.menuItems().every((item) => item.props.tall === true)).toBe(true);
        const views = moreViews(screen.menu()), current = hostNavigation(screen.props.host.id).route.tab;
        expect(views).toEqual(["Graph", "Delivery", "Decisions", "AI teams & sessions", "Activity", "Inbox", "Search", "Settings"]);
        // The page that is open stays marked inside the scrolling list.
        expect(screen.menuItems().filter((item) => item.props.selected === true).map((item) => item.props.label)).toEqual(views.includes(current) ? [current] : []);
        screen.choose(tab);
        expect(screen.menu()).toBeUndefined();
        expect(screen.action("More").props).toMatchObject({ selected: true, expanded: false, accessibilityLabel: `More Kontor views, current: ${tab}` });
      }
      expect(hostNavigation(screen.props.host.id).route.tab).toBe(tab);
    }
  });
  it("keeps Graph and Delivery directly accessible on desktop with the remaining pages in an end-aligned More dropdown", () => {
    const screen = mount(false);
    expect(screen.elements().filter((element) => element.props.tab === true).map((element) => element.props.label)).toEqual(["Sessions", "Overview", "Plan", "Board", "Graph", "Delivery"]);
    expect(screen.action("More").props).toMatchObject({ trailing: "⌄", expanded: false });
    screen.click("Graph"); expect(hostNavigation(screen.props.host.id).route.tab).toBe("Graph");
    screen.click("More");
    expect(screen.menu()!.props.placement).toEqual({ align: "end", width: 190 });
    expect(moreViews(screen.menu())).toEqual(["Decisions", "AI teams & sessions", "Activity", "Inbox", "Search", "Settings"]);
    screen.choose("Inbox"); expect(screen.action("More").props.selected).toBe(true);
    screen.click("More"); expect(screen.menuItems().find((item) => item.props.label === "Inbox")?.props.selected).toBe(true);
    screen.closeMenu();
    expect(screen.action("More").props.expanded).toBe(false);
    expect(hostNavigation(screen.props.host.id).route.tab).toBe("Inbox");
    screen.click("More"); screen.click("More"); expect(screen.menu()).toBeUndefined();
  });

  it("opens More and the context picker as their own panels whichever one the shared slot held before", () => {
    const screen = mount(false, { tab: "Overview" });
    const opening = () => { const menu = screen.menu(); return menu && AnchoredMenu(menu.props as Parameters<typeof AnchoredMenu>[0]); };
    // Each from closed.
    screen.click("More");
    const more = opening()!;
    expect([more.props.testID, more.props.dialog]).toEqual(["kontor-more-menu", undefined]);
    screen.closeMenu();
    screen.openPicker();
    const picker = opening()!;
    expect([picker.props.testID, picker.props.dialog]).toEqual(["kontor-context-menu", true]);
    expect(picker.key).not.toBe(more.key);
    screen.closeMenu();
    expect(opening()).toBeUndefined();
    // More open, then the picker, which Tab reaches behind the menu: it takes the slot without a close and opens as from closed.
    screen.click("More");
    screen.openPicker();
    expect([opening()!.props.testID, opening()!.key]).toEqual(["kontor-context-menu", picker.key]);
    // And back: a press on More with the picker open swaps More in as from closed.
    screen.click("More");
    expect([opening()!.props.testID, opening()!.key]).toEqual(["kontor-more-menu", more.key]);
    // Closed, then each reopened as before.
    screen.closeMenu();
    expect(opening()).toBeUndefined();
    screen.openPicker();
    expect(opening()!.key).toBe(picker.key);
    screen.click("More");
    expect(opening()!.key).toBe(more.key);
    screen.click("More");
    expect(opening()).toBeUndefined();
  });
  it("scopes Overview by a confirmed epic without a plan, returns with Back, and clears to All projects", () => {
    const screen = mount(true);
    expect(screen.picker()!.props.label).toBe("All projects");
    screen.click("Overview"); screen.openPicker("All projects"); screen.choose("ASMA-1");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", projectId: null, epicKey: "ASMA-1" });
    expect(screen.picker()!.props).toMatchObject({ label: "No plan linked", epicKey: "ASMA-1" });
    screen.click("Back"); expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", epicKey: null });
    screen.openPicker(); screen.choose("ASMA-1"); screen.click("Sessions");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Sessions", epicKey: "ASMA-1" });
    screen.click("Overview"); expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", epicKey: "ASMA-1" });
    screen.openPicker("No plan linked"); screen.choose("All projects");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ projectId: null, epicKey: null, sourceId: null, tab: "Overview" });
  });
  it("files a task-labelled session under its captured epic in the picker, Overview and Sessions, and never writes it to native", () => {
    const source: ReadSourceSummary = { id: "jira-history", title: "Historical Jira", kind: "jira", path: "/configured/history.json", projectId: null, epic: null, workstream: null, revisionKind: "working copy" };
    model.entries = [sessionEntry("task", { labels: { "asma.task": "ASMA-8541" } })];
    model.sources = [source];
    model.reads = { [source.id]: { source, status: "ready", revision: capturedHistory.provenance.sha256, readAt: "2026-10-21T09:00:00Z", modifiedAt: null, content: null, graph: null, records: null, diagnostics: [], jiraHistory: capturedHistory } };
    const screen = mount(false); screen.click("Overview");
    const overview = screen.elements().find((node) => node.type === ReadOnlyPage)!;
    expect((overview.props.entries as DirectoryEntry[]).map(confirmedEpic)).toEqual(["ASMA-8539"]);
    screen.openPicker("All projects"); screen.choose("ASMA-8539");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ epicKey: "ASMA-8539" });
    expect((screen.elements().find((node) => node.type === ReadOnlyPage)!.props.entries as DirectoryEntry[]).map(({ id }) => id)).toEqual(["task"]);
    screen.click("Sessions");
    const sessions = screen.elements().find((node) => node.type === SessionsScreen)!;
    expect((sessions.props.directory as { state: { entries: DirectoryEntry[] } }).state.entries[0]!.jira?.epics.get("ASMA-8541")).toBe("ASMA-8539");
    expect(model.entries[0]!.jira).toBeUndefined();
  });
  it("keeps captured, uncaptured and conflicting task attribution distinct through the header and Board", () => {
    const history: ReadSourceSummary = { id: "history", title: "Historical Jira", kind: "jira", path: "/configured/history.json", projectId: null, epic: null, workstream: null, revisionKind: "working copy" };
    const plan: ReadSourceSummary = { id: "captured-plan", title: "Captured epic plan", kind: "plan", path: "/configured/plan.md", projectId: null, epic: "ASMA-8539", workstream: null, revisionKind: "working copy" };
    model.sources = [history];
    model.reads = { history: { source: history, status: "ready", revision: capturedHistory.provenance.sha256, readAt: "2026-10-21T09:00:00Z", modifiedAt: null, content: null, graph: null, records: null, diagnostics: [], jiraHistory: capturedHistory } };
    model.entries = [sessionEntry("captured", { labels: { "asma.task": "ASMA-8541" } }), sessionEntry("uncaptured", { labels: { task: "ASMA-99999" } }), sessionEntry("conflict", { labels: { epic: "ASMA-1", task: "ASMA-8541" } })];
    const screen = mount(false);
    const directory = screen.elements().find(node => node.type === SessionsScreen)!.props.directory as { state: { entries: DirectoryEntry[] } };
    expect(directory.state.entries.map(confirmedEpic)).toEqual(["ASMA-8539", null, null]);
    screen.openPicker();
    expect(screen.menuItems().find(node => node.props.label === "ASMA-8539")!.props.detail).toBe("1 session");
    expect(screen.menuItems().some(node => node.props.label === "ASMA-1")).toBe(false);
    screen.closeMenu(); model.sources = [history, plan]; screen.render(); screen.openPicker();
    screen.choose("Plan: Captured epic plan"); screen.click("Overview");
    expect((screen.elements().find(node => node.type === ReadOnlyPage)!.props.entries as DirectoryEntry[]).map(entry => entry.id)).toEqual(["captured"]);
    screen.click("Board");
    const board = screen.elements().find(node => node.type === GraphPage)!;
    const sessions = board.props.sessions as { state: { entries: DirectoryEntry[] } };
    expect(sessions.state.entries.map(confirmedEpic)).toEqual(["ASMA-8539", null, null]);
    expect(model.entries.every(entry => entry.jira === undefined)).toBe(true);
  });
  it("groups and searches picker choices, reports no match, and starts each opening unfiltered", () => {
    model.entries = [sessionEntry("one", { projectKey: "p-one", projectName: "Kontor", labels: { epic: "ASMA-1" } }), sessionEntry("two", { projectKey: "p-two", projectName: "Billing", labels: { epic: "ASMA-2" } })];
    model.sources = [{ id: "source-one", title: "First plan", kind: "plan", path: "/configured/one.md", projectId: "p-one", epic: "ASMA-1", workstream: null, revisionKind: "working copy" }];
    const screen = mount(false); screen.click("Overview"); screen.openPicker("All projects");
    const groups = () => screen.elements().filter((node) => String(node.key).startsWith("group:")).map((node) => node.props.children);
    const labels = () => screen.menuItems().map((node) => node.props.label);
    expect(screen.menu()!.props.placement).toEqual({ align: "start", width: 380, maxHeight: 460 });
    expect(groups()).toEqual(["CONFIGURED PLANS", "EPICS WITHOUT A CONFIGURED PLAN", "NATIVE PROJECTS"]);
    expect(labels()).toEqual(["All projects", "First plan", "ASMA-2", "Kontor", "Billing"]);
    expect(screen.menuItems().find((node) => node.props.label === "ASMA-2")?.props.detail).toBe("1 session");
    const search = () => screen.menu()!.props.header as ReactElement<Record<string, unknown>>;
    expect(search().props).toMatchObject({ accessibilityLabel: "Search plans, epics and projects", autoFocus: true, value: "" });
    (search().props.onChangeText as (value: string) => void)("bill"); screen.render();
    expect(groups()).toEqual(["NATIVE PROJECTS"]); expect(labels()).toEqual(["All projects", "Billing"]);
    (search().props.onChangeText as (value: string) => void)("asma-1"); screen.render();
    expect(labels()).toEqual(["All projects", "First plan"]);
    (search().props.onChangeText as (value: string) => void)("zzz"); screen.render();
    expect(labels()).toEqual(["All projects"]);
    expect(JSON.stringify(screen.menu()!.props.children)).toContain("zzz");
    screen.closeMenu(); screen.openPicker();
    expect(search().props.value).toBe(""); expect(labels()).toHaveLength(5);
    screen.choose("Billing");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", projectId: "p-two", epicKey: null, sourceId: null });
    expect(screen.picker()!.props).toMatchObject({ label: "Billing", epicKey: null });
  });
});

it.each([true, false])("retains a header-only epic when selecting a graphless plan (compact=%s)", async (compact) => {
  const fixture = await import("./plan-workstream-fixtures");
  const source = { ...fixture.source, epic: null };
  model.sources = [source]; model.reads = { [source.id]: { ...fixture.read, source } };
  model.entries = [sessionEntry("unrelated", { labels: { epic: "ASMA-2" } })];
  const screen = mount(compact, { tab: "Plan" }); screen.openPicker();
  expect(screen.menuItems().find((node) => node.props.label === source.title)?.props.detail).toBe("ASMA-7575 · adopus-graphql");
  screen.choose(source.title);
  expect(hostNavigation(screen.props.host.id).route).toMatchObject({ sourceId: source.id, sourceRevision: fixture.read.revision, epicKey: "ASMA-7575", projectId: null });
  expect(screen.picker()?.props).toMatchObject({ label: source.title, epicKey: "ASMA-7575" });
  expect(screen.elements().find((node) => node.type === ReadOnlyPage)!.props.entries).toEqual([]);
});

it.each([true, false])("projects a project-bound plan onto its existing empty workspace through HostScreen (compact=%s)", async (compact) => {
  const fixture = await import("./plan-workstream-fixtures");
  const source = { ...fixture.source, projectId: "native-project" };
  model.sources = [source]; model.reads = { [source.id]: { ...fixture.read, source } };
  model.entries = [];
  model.workspaces = [WorkspaceDescriptorPayloadSchema.parse({ id: "empty", projectId: "native-project", projectDisplayName: "Existing project", projectRootPath: "/fixture/project", projectKind: "non_git", workspaceKind: "directory", name: "Existing empty workspace", status: "done", activityAt: null })];
  model.values = { ...DEFAULT_ORGANIZATION_SETTINGS, displayMappings: { "workspace:empty": { epic: "ASMA-7575" } } };
  const screen = mount(compact, { tab: "Sessions" });
  const sessions = screen.elements().find((node) => node.type === SessionsScreen)!;
  expect(sessions.props.workspaces).toEqual([{ id: "empty", name: "Existing empty workspace", projectKey: "native-project", projectName: "Existing project" }]);
  expect(sessions.props.planMappings).toMatchObject({ "workspace:empty": { epic: "ASMA-7575", workstream: "adopus-graphql" } });
  expect((sessions.props.directory as { state: { entries: DirectoryEntry[] } }).state.entries).toEqual([]);
});

it("finds a graphless plan by its explicit workstream and shows exact Plan context without linking unrelated sessions", async () => {
  const { source, read } = await import("./plan-workstream-fixtures");
  model.sources = [source]; model.reads = { [source.id]: read };
  model.entries = [sessionEntry("unrelated", { labels: { epic: "ASMA-2" } })];
  const screen = mount(true, { tab: "Plan" }); screen.openPicker();
  const input = elements(screen.menu()!.props.header as ReactNode).find((node) => node.type === "TextInput")!;
  (input.props.onChangeText as (value: string) => void)("adopus-graphql"); screen.render();
  expect(screen.choices()).toEqual(["Plan: Declared plan"]);
  expect(screen.menuItems().find((node) => node.props.label === source.title)?.props.detail).toBe("ASMA-7575 · adopus-graphql");
  screen.choose(source.title);
  expect(screen.picker()?.props).toMatchObject({ label: source.title, epicKey: "ASMA-7575" });
  const page = screen.elements().find((node) => node.type === ReadOnlyPage)!;
  expect(page.props.entries).toEqual([]);
  for (const tab of ["Sessions", "Overview", "Board", "Graph", "Delivery", "Plan"]) {
    if (tab === "Graph" || tab === "Delivery") { screen.click("More"); screen.choose(tab); }
    else screen.click(tab);
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab, sourceId: source.id, sourceRevision: read.revision, epicKey: "ASMA-7575" });
    expect(screen.picker()?.props).toMatchObject({ label: source.title, epicKey: "ASMA-7575" });
  }
});
