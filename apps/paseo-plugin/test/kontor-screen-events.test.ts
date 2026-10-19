import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import type { PluginScreenProps } from "@getpaseo/plugin/client";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DirectoryEntry } from "../client/session-directory";
import { canonicalGraphSchema, jiraHistorySchema, type ReadSourceSummary, type SourceRead } from "../shared/read-sources";
import { Linking } from "react-native";

const model = vi.hoisted(() => ({
  slots: [] as unknown[], cursor: 0, entries: [] as DirectoryEntry[], sources: [] as ReadSourceSummary[], reads: {} as Record<string, SourceRead>, values: {} as object,
  useState<T>(initial: T | (() => T)): [T, (next: T | ((previous: T) => T)) => void] {
    const index = this.cursor++;
    if (!(index in this.slots)) this.slots[index] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [this.slots[index] as T, (next) => { this.slots[index] = typeof next === "function" ? (next as (previous: T) => T)(this.slots[index] as T) : next; }];
  },
}));
vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(), useState: <T,>(initial: T | (() => T)) => model.useState(initial),
  useMemo: (factory: () => unknown) => factory(), useCallback: (callback: unknown) => callback,
  useEffect: () => {},
}));
vi.mock("react-native", () => ({
  Linking: { openURL: vi.fn(async () => {}) },
  SafeAreaView: "SafeAreaView", KeyboardAvoidingView: "KeyboardAvoidingView", ScrollView: "ScrollView", View: "View", Text: "Text", Pressable: "Pressable",
}));
vi.mock("@getpaseo/plugin/client", () => ({ useHosts: () => [] }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({
  Modal: Object.assign(() => null, { Content: () => null }), ScrollView: "ScrollView", FlatList: "FlatList", TextInput: "TextInput", Icon: "Icon", useToast: () => ({ error: () => {} }),
}));
vi.mock("../client/use-session-directory", () => ({ useSessionDirectory: () => ({ state: { entries: model.entries, status: "ready", live: false, lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false }, refresh: () => {} }) }));
vi.mock("../client/use-organization-settings", () => ({ useOrganizationSettings: () => ({ state: { status: "ready", values: model.values, saving: false, saveError: null }, save: async () => true }) }));
vi.mock("../client/use-read-sources", () => ({ useReadSources: () => ({ sources: model.sources, reads: model.reads, error: null, loading: false, available: true }) }));
vi.mock("../client/use-workspace-directory", () => ({ useWorkspaceDirectory: () => ({ entries: [], error: null, readAt: null }) }));

import { KontorScreen } from "../client/kontor-screen";
import { DESTINATIONS, hostNavigation, navigate, readRouteView, type KontorRoute } from "../client/navigation-state";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { sessionEntry } from "./session-fixtures";
import { GraphPage } from "../client/graph-page";
import { historicalPlanProgress } from "../client/page-data";

const actualGraph = canonicalGraphSchema.parse(JSON.parse(readFileSync(new URL("./fixtures/plan-graph-asma-8371.json", import.meta.url), "utf8")));
const capturedHistory = jiraHistorySchema.parse(JSON.parse(readFileSync(new URL("./fixtures/jira-history.json", import.meta.url), "utf8")));
function elements(value: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(value)) return [];
  return [value, ...elements(value.props.children as ReactNode)];
}
let sequence = 0;
beforeEach(() => { model.slots = []; model.cursor = 0; model.values = DEFAULT_ORGANIZATION_SETTINGS; model.entries = [sessionEntry("native", { labels: { epic: "ASMA-1" } })]; model.sources = []; model.reads = {}; });
function mount(compact: boolean, initial?: Partial<KontorRoute>) {
  const props: PluginScreenProps = { host: { id: `shell-${++sequence}`, label: "Host" }, layout: { compact, platform: "ios" }, params: {}, navigation: { openAgent: vi.fn(), openWorkspace: vi.fn(), openBrowser: vi.fn() },
    theme: { colors: { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } } };
  if (initial) navigate(props.host.id, initial);
  const entry = KontorScreen(props);
  const component = entry.type as (props: PluginScreenProps) => ReactNode;
  let tree: ReactNode;
  const render = () => { model.cursor = 0; tree = component(props); };
  render();
  return {
    props, render,
    action(label: string) { const action = elements(tree).find((element) => element.props.label === label); if (!action) throw new Error(`Missing action ${label}`); return action; },
    click(label: string) { const action = this.action(label); const press = action.props.onPress; expect(typeof press).toBe("function"); if (typeof press === "function") press(); render(); },
    elements() { return elements(tree); },
  };
}
describe("Kontor shell native actions", () => {
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
    const screen = mount(true); screen.click("All projects ▾"); screen.click("ASMA-1"); screen.click("More"); screen.click("Graph");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Graph", epicKey: "ASMA-1", sourceId: "explicit" });
    screen.click("‹ Back"); expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Sessions", epicKey: "ASMA-1" });
    screen.click("Plan"); expect(hostNavigation(screen.props.host.id).route.sourceId).toBe("explicit");
  });
  it("keeps multiple linked sources as accessible choices and reserves the context row before desktop tabs", () => {
    model.sources = ["one", "two"].map((id) => ({ id, title: id, kind: "plan", path: `/plans/${id}.md`, projectId: null, epic: "ASMA-1", workstream: null, revisionKind: "working copy" }));
    const screen = mount(false, { epicKey: "ASMA-1" }); screen.click("Graph");
    expect(hostNavigation(screen.props.host.id).route.sourceId).toBeNull();
    const graph = screen.elements().find((node) => node.type === GraphPage)!;
    (graph.props.onChooseSource as () => void)(); screen.render();
    expect(screen.action("Plan: one")).toBeDefined(); expect(screen.action("Plan: two")).toBeDefined();
    expect(screen.elements().findIndex((node) => node.props.testID === "kontor-context-row")).toBeLessThan(screen.elements().findIndex((node) => node.props.accessibilityRole === "tablist"));
    screen.click("Plan: two"); expect(hostNavigation(screen.props.host.id).route.sourceId).toBe("two");
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
    expect(hostNavigation(screen.props.host.id).route.sourceId).toBe("b"); screen.click("‹ Back");
    expect(hostNavigation(screen.props.host.id).route.sourceId).toBe("a");
  });
  it("wires Board scroll state into history before opening Plan and restores it on Back", () => {
    const screen = mount(true, { tab: "Board", sourceId: "plan-a", sourceRevision: "sha-a" });
    const board = () => screen.elements().find((element) => element.type === GraphPage)!;
    (board().props.onScroll as (offset: number) => void)(800);
    (board().props.onPlan as (sourceId: string) => void)("plan-a"); screen.render();
    expect(hostNavigation(screen.props.host.id).route.tab).toBe("Plan"); screen.click("‹ Back");
    expect(board().props.initialOffset).toBe(800);
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
    screen.click("ASMA-2 · linked-b ▾");
    expect(screen.action("Plan: linked-a")).toBeDefined(); expect(screen.action("Plan: linked-b")).toBeDefined();
    expect(screen.elements().some((element) => element.props.label === "Plan: unrelated")).toBe(false);
    screen.click("Close"); screen.click("More"); screen.click("Graph");
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
    if (tab === "Graph") screen.click("More");
    screen.click(tab); screen.click("ASMA-1 · linked-a ▾"); screen.click("Plan: linked-b");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab, sourceId: "linked-b", originAgentId: "native" });
    expect(screen.action("Return to conversation").props.disabled).toBe(false);
    screen.click("Return to conversation");
    expect(screen.props.navigation?.openAgent).toHaveBeenCalledWith({ agentId: "native", serverId: screen.props.host.id });
    screen.click("ASMA-2 · linked-b ▾");
    const choices = () => screen.elements().filter((node) => String(node.props.label).startsWith("Plan: ")).map((node) => node.props.label);
    expect(choices()).toEqual(["Plan: linked-a", "Plan: linked-b"]);
    screen.click("Close"); screen.click("Plan");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Plan", sourceId: "linked-b", originAgentId: "native" });
    const selectedPlan = screen.elements().find((node) => typeof node.props.onSource === "function")!;
    expect((selectedPlan.props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(["linked-b"]);
    screen.click("‹ Back");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab, sourceId: "linked-b", originAgentId: "native" });
    screen.click("ASMA-2 · linked-b ▾"); expect(choices()).toEqual(["Plan: linked-a", "Plan: linked-b"]);
    screen.click("All projects");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ originAgentId: null, sourceId: null, projectId: null, epicKey: null });
    screen.click("All projects ▾"); expect(choices()).toContain("Plan: unrelated"); screen.click("Close");
    screen.click("‹ Back");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab, sourceId: "linked-b", originAgentId: "native" });
    screen.click("ASMA-2 · linked-b ▾"); expect(choices()).toEqual(["Plan: linked-a", "Plan: linked-b"]);
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
    if (tab === "Graph") screen.click("More");
    screen.click(tab);
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
    screen.click("‹ Back");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab, sourceId: "linked-plan", originAgentId: "native" });
    const returned = screen.elements().find((node) => node.type === GraphPage)!;
    expect(returned.props.reads).toEqual(forwarded);
  });
  it.each(["unrelated", "linked-next", null] as const)("leaves conversation filtering when global Overview selects %s, preserving Plan and Back", (targetId) => {
    const linked: SourceRead = { source: { id: "linked-plan", title: "linked-plan", kind: "plan", path: actualGraph.source, projectId: null, epic: actualGraph.epic.jira, workstream: null, revisionKind: "working copy" }, status: "ready", revision: actualGraph.source_sha256, readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: "# Linked plan", graph: actualGraph, records: null, diagnostics: [] };
    const otherPlans: SourceRead[] = ["linked-next", "unrelated"].map((id) => ({ ...linked, source: { ...linked.source, id, title: id, path: `/configured/${id}.md` }, revision: "b".repeat(64), graph: null, content: `# Configured ${id}` }));
    const jira: SourceRead = { ...linked, source: { ...linked.source, id: "jira-history", title: "Historical Jira", kind: "jira", path: "/configured/history.json", epic: null }, revision: capturedHistory.provenance.sha256, graph: null, content: null, jiraHistory: capturedHistory };
    const configured = [linked, ...otherPlans, jira];
    model.sources = configured.map((read) => read.source); model.reads = Object.fromEntries(configured.map((read) => [read.source.id, read]));
    model.values = { ...DEFAULT_ORGANIZATION_SETTINGS, planAssociations: { native: ["linked-plan", "linked-next"] } };
    const screen = mount(true, { tab: "Plan", sourceId: linked.source.id, sourceRevision: linked.revision, epicKey: linked.source.epic, originAgentId: "native" });
    const page = () => screen.elements().find((node) => typeof node.props.onDocumentScroll === "function")!;
    const linkedRoute = { ...hostNavigation(screen.props.host.id).route };
    (page().props.onDocumentScroll as (offset: number) => void)(900);
    (page().props.onScroll as (offset: number) => void)(120);
    screen.click("Overview"); (page().props.onScroll as (offset: number) => void)(800);
    screen.click(`${actualGraph.epic.jira} · linked-plan ▾`);
    screen.click(targetId ? `Plan: ${targetId}` : "All projects"); screen.click("Plan");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Plan", sourceId: targetId, originAgentId: null });
    expect((page().props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(configured.map((read) => read.source.id));
    expect(page().props.reads).toEqual(model.reads);
    expect(page().props.initialDocumentOffset).toBe(0); expect(page().props.initialOffset).toBe(0);
    expect(historicalPlanProgress(actualGraph, Object.values(page().props.reads as Record<string, SourceRead>))).toMatchObject({ known: 19, unknown: 0 });
    expect((page().props.reads as Record<string, SourceRead>)["jira-history"]).toBe(jira);
    if (targetId === null) {
      (page().props.onSource as (id: string) => void)("unrelated"); screen.render();
    }
    const selected = targetId ?? "unrelated";
    expect(page().props.sourceId).toBe(selected);
    expect((page().props.reads as Record<string, SourceRead>)[selected]?.content).toBe(`# Configured ${selected}`);
    expect(screen.elements().some((node) => node.props.label === "Return to conversation")).toBe(false);
    if (targetId === null) screen.click("‹ Back");
    screen.click("‹ Back");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", sourceId: targetId, originAgentId: null });
    screen.click("‹ Back");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", sourceId: "linked-plan", originAgentId: "native" });
    expect(page().props.initialOffset).toBe(800);
    screen.click("‹ Back");
    expect(hostNavigation(screen.props.host.id).route).toEqual(linkedRoute);
    expect(page().props.initialDocumentOffset).toBe(900); expect(page().props.initialOffset).toBe(120);
    expect(screen.action("Return to conversation").props.disabled).toBe(false);
    expect(readRouteView(`other-${screen.props.host.id}`, linkedRoute)).toMatchObject({ offset: 0, documentOffset: 0 });
  });
  it("wires the inner document position independently from the outer Plan page through Back", () => {
    model.sources = ["plan-a", "plan-b"].map((id) => ({ id, title: id, kind: "plan", path: `/configured/${id}.md`, workstream: null, epic: null, projectId: null, revisionKind: "working copy" }));
    const screen = mount(true, { tab: "Plan", sourceId: "plan-a", sourceRevision: "sha-a" });
    const page = () => screen.elements().find((element) => typeof element.props.onDocumentScroll === "function")!;
    (page().props.onDocumentScroll as (offset: number) => void)(900);
    (page().props.onScroll as (offset: number) => void)(120);
    screen.click("Overview"); screen.click("‹ Back");
    expect(page().props.initialDocumentOffset).toBe(900);
    expect(page().props.initialOffset).toBe(120);
    (page().props.onSource as (id: string) => void)("plan-b"); screen.render();
    expect(page().props.initialDocumentOffset).toBe(0);
    screen.click("‹ Back"); expect(page().props.initialDocumentOffset).toBe(900);
  });
  it("selects an exact plan in the compact Overview context picker and isolates hosts", () => {
    model.entries = [sessionEntry("one", { projectKey: "p-one", labels: { epic: "ASMA-1" } }), sessionEntry("two", { projectKey: "p-two", labels: { epic: "ASMA-2" } })];
    model.sources = [{ id: "source-one", title: "First plan", kind: "plan", path: "/configured/one.md", projectId: "p-one", epic: "ASMA-1", workstream: null, revisionKind: "working copy" }, { id: "source-two", title: "Second plan", kind: "plan", path: "/configured/two.md", projectId: "p-two", epic: "ASMA-2", workstream: null, revisionKind: "working copy" }];
    model.sources.push({ id: "decisions-one", title: "Recorded decisions", kind: "decisions", path: "/configured/decisions.json", projectId: "p-one", epic: "ASMA-1", workstream: null, revisionKind: "working copy" });
    const screen = mount(true); screen.click("Overview"); screen.click("All projects ▾"); screen.click("Plan: First plan");
    const page = () => screen.elements().find((element) => typeof element.props.onDocumentScroll === "function")!;
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ tab: "Overview", sourceId: "source-one", projectId: "p-one", epicKey: "ASMA-1" });
    expect((page().props.entries as DirectoryEntry[]).map((entry) => entry.id)).toEqual(["one"]);
    expect((page().props.sources as ReadSourceSummary[]).map((source) => source.id)).toEqual(["source-one", "decisions-one"]);
    expect(screen.action("ASMA-1 · First plan ▾")).toBeDefined();
    expect(hostNavigation("independent-overview-host").route.sourceId).toBeNull();
    screen.click("ASMA-1 · First plan ▾"); screen.click("All projects");
    expect((page().props.entries as DirectoryEntry[]).map((entry) => entry.id)).toEqual(["one", "two"]);
    expect(screen.elements().filter((element) => element.props.tab === true)[0]?.props.icon).toBe("List");
  });
  it("reaches every destination on compact layouts through four tabs and More", () => {
    const screen = mount(true);
    const primary = screen.elements().filter((element) => element.props.tab === true);
    expect(primary.map((element) => element.props.label)).toEqual(["Sessions", "Overview", "Plan", "Board", "More"]);
    expect(primary.every((element) => element.props.stacked === true)).toBe(true);
    for (const tab of DESTINATIONS) {
      if (!["Sessions", "Overview", "Plan", "Board"].includes(tab)) screen.click("More");
      screen.click(tab);
      expect(hostNavigation(screen.props.host.id).route.tab).toBe(tab);
    }
  });
  it("keeps Graph and Delivery directly accessible on desktop with the remaining pages in More", () => {
    const screen = mount(false);
    expect(screen.elements().filter((element) => element.props.tab === true).map((element) => element.props.label)).toEqual(["Sessions", "Overview", "Plan", "Board", "Graph", "Delivery", "More"]);
    screen.click("Graph"); expect(hostNavigation(screen.props.host.id).route.tab).toBe("Graph");
    screen.click("More"); screen.click("Inbox"); expect(screen.action("More").props.selected).toBe(true);
  });
  it("filters Overview by confirmed context, returns with Back, and clears to All projects", () => {
    const screen = mount(true);
    screen.click("All projects ▾"); screen.click("ASMA-1");
    expect(hostNavigation(screen.props.host.id).route.epicKey).toBe("ASMA-1");
    screen.click("‹ Back"); expect(hostNavigation(screen.props.host.id).route.epicKey).toBeNull();
    screen.click("All projects ▾"); screen.click("ASMA-1"); screen.click("Overview");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ projectId: null, epicKey: "ASMA-1", tab: "Overview" });
    screen.click("ASMA-1 ▾"); screen.click("All projects");
    expect(hostNavigation(screen.props.host.id).route).toMatchObject({ projectId: null, epicKey: null, sourceId: null, tab: "Overview" });
  });
});
