import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { jiraHistorySchema, type CanonicalGraph, type SourceRead } from "../shared/read-sources";
const model = vi.hoisted(() => ({ open: false }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useContext: () => null, useState: () => [model.open, (value: boolean) => { model.open = value; }], useRef: (current: unknown) => ({ current }), useMemo: (factory: () => unknown) => factory() }));
vi.mock("react-native", () => ({ ActivityIndicator: "ActivityIndicator", ScrollView: "ScrollView", Text: "Text", View: "View", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { SourceDetails } from "../client/source-details";
import type { WorkCatalogRead } from "../shared/work-catalog";
import { ReadOnlyPage } from "../client/read-only-pages";
import { sessionEntry } from "./session-fixtures";
type Props = Parameters<typeof ReadOnlyPage>[0];
const history = jiraHistorySchema.parse(JSON.parse(readFileSync(new URL("./fixtures/jira-history.json", import.meta.url), "utf8")));
const read: SourceRead = { source: { id: "jira", title: "Historical Jira", kind: "jira", path: "/configured/projection.json", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }, status: "ready", revision: "a".repeat(64), readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: null, graph: null, records: null, diagnostics: [], jiraHistory: history };
const graph: CanonicalGraph = { schema_version: 1, source: "/plan.md", source_sha256: "b".repeat(64), graph_sha256: "c".repeat(64), epic: { jira: "ASMA-8539", kontor_id: null }, nodes: [8540, 8541, 8551, 8542, 8545, 8546, 8547, 8548, 8549, 8550].map((key) => ({ id: `task-${key}`, title: "Task", jira: `ASMA-${key}`, kontor_id: null, phase: "Phase", wave: 0, depends_on: [] })), edges: [], waves: [], external_fences: [] };
const plan: SourceRead = { ...read, source: { ...read.source, id: "plan", title: "Selected plan", path: graph.source, kind: "plan", epic: "ASMA-8539" }, revision: graph.source_sha256, jiraHistory: null, graph };
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function" && node.type !== ReadOnlyPage) return [node, ...elements((node.type as (props: any) => ReactNode)(node.props))];
  return [node, ...elements(node.props.children as ReactNode)];
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return text((node.type as (props: any) => ReactNode)(node.props));
  return text(node.props.children as ReactNode);
}
function overview(reads: Record<string, SourceRead>, extra: Partial<Props> = {}) {
  const props: Props = { tab: "Overview", contextLabel: "Selected plan", colors: {} as Props["colors"], entries: [sessionEntry("idle", { status: "idle", labels: { epic: "ASMA-8539" } })], directory: { entries: [], status: "ready", live: false, lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false, labelsComplete: true }, sources: Object.values(reads).map((value) => value.source), reads, sourceId: "plan", sourceError: null, query: "", setQuery: vi.fn(), onSource: vi.fn(), onOpen: vi.fn(), onTab: vi.fn(), onEpicTab: vi.fn(), initialOffset: 0, onScroll: vi.fn() };
  Object.assign(props, extra);
  const outer = ReadOnlyPage(props);
  const child = elements(outer).find((element) => element.props.tab === "Overview")!;
  return (child.type as (props: Props) => ReactNode)(child.props as Props);
}
const catalogRead = (nativeTruncated: boolean, entriesTruncated = false, options: { sourceAsOf?: string | null; nativeMode?: "catalog" | "snapshot" | "unavailable" } = {}) => ({
  mode: "catalog",
  checkedAt: "2026-10-25T09:00:00Z",
  sourceAsOf: options.sourceAsOf === undefined ? "2026-10-25T08:15:00Z" : options.sourceAsOf,
  view: { schemaVersion: 1, sources: [], entries: [], entriesTruncated },
  error: null,
  snapshot: null,
  native: {
    mode: options.nativeMode ?? "catalog",
    hostId: "srv_host_a",
    runtimeInstance: options.nativeMode === "unavailable" ? null : "paseo:ws://127.0.0.1:6767/ws",
    checkedAt: "2026-10-25T09:00:00Z",
    view: options.nativeMode === "unavailable" ? null : {
      runtimeInstance: "paseo:ws://127.0.0.1:6767/ws",
      generation: 12,
      asOf: "2026-10-25T08:55:00Z",
      source: options.nativeMode === "snapshot" ? "snapshot" : "database",
      archiveScope: true,
      truncated: nativeTruncated,
      projects: [],
      workspaces: [],
      sessions: [],
    },
    error: options.nativeMode === "unavailable" ? "native unavailable" : null,
    snapshotSavedAt: options.nativeMode === "snapshot" ? "2026-10-20T00:00:00Z" : null,
  },
}) as unknown as WorkCatalogRead;
const meta = (tree: ReactNode) => String(elements(tree).find((node) => node.props.testID === "overview-meta")!.props.accessibilityLabel);
const tile = (tree: ReactNode, label: string) => elements(tree).find((node) => node.props.accessibilityRole === "button" && String(node.props.accessibilityLabel).startsWith(`${label}: `))!;
beforeEach(() => { model.open = false; });
describe("compact Overview source evidence", () => {
  it("presents the six prototype filters, keeps unsupported ones Unknown and stores the choice in the route query", () => {
    const setQuery = vi.fn(); const tree = overview({ plan, jira: read }, { setQuery, scoped: true });
    expect(["Epics", "In progress", "Needs attention", "Needs your decision", "Completed epics", "Delivered"].map((label) => String(tile(tree, label).props.accessibilityLabel)))
      .toEqual(["Epics: 1. All work", "In progress: 0. Epics with running sessions", "Needs attention: 0. Native attention, input or error", "Needs your decision: Not captured. Decision records unavailable", "Completed epics: 0. Recorded epic lifecycle", "Delivered: Unknown. Delivery confirmation not captured"]);
    (tile(tree, "Needs your decision").props.onPress as () => void)(); expect(setQuery).toHaveBeenCalledWith("decisions");
    (tile(tree, "Epics").props.onPress as () => void)(); expect(setQuery).toHaveBeenLastCalledWith("");
    // Tile meanings stay in their labels; an unfiltered list does not repeat the Epics count.
    for (const help of ["All work", "Epics with running sessions", "Recorded epic lifecycle", "Delivery confirmation not captured"]) expect(text(tree)).not.toContain(help);
    expect(text(tree)).not.toMatch(/\d+ of \d+ shown/);
    const review = overview({ plan, jira: read }, { query: "review", scoped: true });
    expect(text(review)).toContain("0 of 1 shown"); expect(text(review)).toContain("No epics in this group.");
    expect(tile(review, "Needs attention").props.accessibilityState).toEqual({ selected: true });
    expect([tile(review, "Needs attention").props["aria-pressed"], tile(tree, "Needs attention").props["aria-pressed"]]).toEqual([true, false]);
    const delivered = text(overview({ plan, jira: read }, { query: "delivered", scoped: true }));
    expect(delivered).not.toMatch(/\d+ of \d+ shown/); expect(delivered).toContain("Recorded pull requests and checks do not establish delivery");
  });
  it("opens the epic's exact plan and views from a compact row with prototype identity, signals and coverage", () => {
    const onEpicTab = vi.fn(); const onSource = vi.fn();
    const tree = overview({ plan, jira: read }, { onEpicTab, onSource, scoped: true });
    const visible = text(tree);
    expect(visible).toContain("ASMA-8539 · Selected plan"); expect(visible).toContain("Epic · Groomed"); expect(visible).toContain("0 / 3 captured tasks done");
    // Visible lines keep non-zero, captured facts and a short capture day; the full statements move into labels.
    expect(visible).toContain("1 session"); expect(visible).not.toContain("0 running"); expect(visible).not.toContain("Captured plan available");
    expect(visible).not.toContain("Not captured · Delivery"); expect(visible).not.toContain("2026-10-18");
    expect(visible).toContain("Jira, captured 18 Oct"); expect(visible).toContain("3 Jira records · full backlog coverage not established");
    const row = elements(tree).find((node) => String(node.props.accessibilityLabel).startsWith("ASMA-8539 · Selected plan. Captured plan available"))!;
    expect(row.props.accessibilityLabel).toContain("1 session · 0 running · 0 native attention · 0 needs input · 0 errors");
    expect(row.props.accessibilityLabel).toContain("Decisions: Not captured · Delivery: Not captured");
    expect(row.props.accessibilityLabel).toContain("3 Jira records · observed 2026-10-18 · full backlog coverage not established");
    expect(elements(tree).some((node) => node.props.accessibilityLabel === "Jira, observed 2026-10-18")).toBe(true);
    (row.props.onPress as () => void)(); expect(onEpicTab).toHaveBeenLastCalledWith("Plan", "ASMA-8539", "plan");
    for (const [label, tab] of [["Tasks", "Board"], ["Graph", "Graph"], ["Plan", "Plan"]] as const) {
      (elements(tree).find((node) => node.props.accessibilityLabel === `${label}: ASMA-8539 · Selected plan`)!.props.onPress as () => void)();
      expect(onEpicTab).toHaveBeenLastCalledWith(tab, "ASMA-8539", "plan");
    }
    (elements(tree).find((node) => node.props.accessibilityLabel === "Sessions: ASMA-8539 · Selected plan")!.props.onPress as () => void)();
    expect(onEpicTab).toHaveBeenLastCalledWith("Sessions", "ASMA-8539", null);
    expect(onSource).not.toHaveBeenCalled();
    expect(visible).not.toMatch(/Current completion|Epic · (Completed|Done)|Delivered: [0-9]/);
  });
  it("keeps source state and original observation visible, expanding technical details only on request", () => {
    const render = () => SourceDetails({ read, colors: {} as Props["colors"] });
    const closed = render(); const visible = text(closed);
    expect(visible).toContain("ready"); expect(visible).toContain("Historical Jira snapshot");
    expect(visible).toContain("Observed 2026-10-18T17:00+00:00 · not live");
    for (const value of [read.source.path, read.revision!, history.provenance.path, history.provenance.sha256, read.readAt]) expect(visible).not.toContain(value);
    const action = elements(closed).find((element) => element.props.label === "Source details")!;
    expect(action.props.expanded).toBe(false);
    (action.props.onPress as () => void)();
    const opened = text(render());
    for (const value of [read.source.path, read.revision!, history.provenance.path, history.provenance.sha256, read.readAt]) expect(opened).toContain(value);
    expect(opened).toContain("Neither count establishes complete inventory");
    expect(elements(render()).find((element) => element.props.label === "Source details")!.props.expanded).toBe(true);
  });
  it("marks a missing historical source unavailable without substituting the read time for observation time", () => {
    const visible = text(SourceDetails({ read: { ...read, status: "unavailable", jiraHistory: null, diagnostics: ["/unavailable/source.json"] }, colors: {} as Props["colors"] }));
    expect(visible).toContain("unavailable"); expect(visible).toContain("Observed Unknown · not live");
    expect(visible).not.toContain(read.readAt); expect(visible).not.toContain("/unavailable/source.json");
  });
  it("puts sources, coverage and the kontord state in one disclosure with expanded semantics", () => {
    const closed = overview({ plan, jira: read }, { scoped: true });
    const toggle = elements(closed).find((node) => node.props.accessibilityRole === "button" && String(node.props.accessibilityLabel).startsWith("Sources and coverage"))!;
    expect(toggle.props.accessibilityLabel).toBe("Sources and coverage · Jira captured 18 Oct, not live");
    expect(text(closed)).toContain("Sources and coverage · Jira captured 18 Oct");
    expect(toggle.props.accessibilityState).toEqual({ expanded: false });
    expect(text(closed)).not.toContain("kontord");
    expect(text(closed)).not.toContain(read.source.path);
    model.open = true;
    const opened = text(overview({ plan, jira: read }, { scoped: true }));
    expect(opened).toContain("Project overview, available without kontord.");
    expect(opened).toContain("Sessions in this scope: 1 session · 0 running");
    expect(opened).toContain("Epic lifecycle, native activity, task status and delivery are separate observations.");
    expect(opened).toContain("Governed scheduler and gates: not shown here. kontord on this host: unknown.");
    expect(text(overview({ plan, jira: read }, { scoped: true, kontord: "kontord on this host: answering." }))).toContain("Governed scheduler and gates: not shown here. kontord on this host: answering.");
    expect(opened).toContain("full backlog coverage is not established");
    expect(opened).toContain("Historical Jira · ready · Historical Jira snapshot");
  });
  it("calls a bounded catalog partial instead of live", () => {
    const cue = (catalog: WorkCatalogRead) => elements(overview({ plan, jira: read }, { scoped: true, catalog }))
      .find((node) => node.props.accessibilityRole === "button" && String(node.props.accessibilityLabel).startsWith("Sources and coverage"))!;
    expect(String(cue(catalogRead(true)).props.accessibilityLabel)).toContain("catalog partial");
    expect(String(cue(catalogRead(false, true)).props.accessibilityLabel)).toContain("catalog partial");
    expect(String(cue(catalogRead(false)).props.accessibilityLabel)).toContain("catalog live");
  });

  it.each([false, undefined])("renders every label-derived Overview count unknown when labels are %s", (labelsComplete) => {
    const catalogRow = sessionEntry("catalog-running", { status: "running", labels: {} });
    const directory = {
      entries: [catalogRow], status: "ready" as const, live: false, lastUpdatedAt: 1, error: null,
      staleReason: null, pages: 1, includeArchived: false, source: "catalog" as const,
      labelsComplete, inventoryComplete: true,
    };
    const global = overview({ plan, jira: read }, { entries: [catalogRow], directory, scoped: false });
    expect(String(tile(global, "In progress").props.accessibilityLabel)).toContain("In progress: Unknown");
    expect(String(tile(global, "Needs attention").props.accessibilityLabel)).toContain("Needs attention: Unknown");
    expect(String(tile(global, "Epics").props.accessibilityLabel)).toContain("Epics: Unknown");
    expect(meta(global)).toContain("Host sessions: 1 session · 1 running");
    expect(text(global)).toContain("epic attribution unknown");
    // The epic row still has recorded Jira facts, but its session link count is unknown.
    expect(elements(global).some((node) => String(node.props.accessibilityLabel).includes("Sessions unknown · shared catalog rows carry no labels"))).toBe(true);

    const scoped = overview({ plan, jira: read }, { entries: [], directory, scoped: true, epicScoped: true });
    expect(meta(scoped)).toContain("Sessions in this scope: unknown");
    expect(meta(scoped)).not.toContain("0 sessions");
    expect(String(tile(scoped, "In progress").props.accessibilityLabel)).toContain("In progress: Unknown");
    const active = overview({ plan, jira: read }, { entries: [], directory, scoped: true, epicScoped: true, query: "active" });
    expect(text(active)).toContain("Session attribution is unknown");
    expect(text(active)).toContain("Session attribution unknown for this filter");
    expect(text(active)).not.toContain("No epics in this group.");
  });

  it("distinguishes live, partial, snapshot and unavailable catalog cues", () => {
    const cue = (catalog: WorkCatalogRead, directory = { entries: [], status: "ready" as const, live: false, lastUpdatedAt: 1, error: null,
      staleReason: null, pages: 1, includeArchived: false, source: "catalog" as const, inventoryComplete: true, labelsComplete: false }) =>
      elements(overview({ plan, jira: read }, { scoped: true, catalog, directory }))
        .find((node) => node.props.accessibilityRole === "button" && String(node.props.accessibilityLabel).startsWith("Sources and coverage"))!;
    expect(String(cue(catalogRead(false)).props.accessibilityLabel)).toContain("catalog live");
    expect(String(cue(catalogRead(false, false, { sourceAsOf: null })).props.accessibilityLabel)).toContain("catalog partial");
    expect(String(cue(catalogRead(false), { entries: [], status: "ready", live: false, lastUpdatedAt: 1, error: null, staleReason: null,
      pages: 1, includeArchived: false, source: "catalog", inventoryComplete: false, labelsComplete: false }).props.accessibilityLabel)).toContain("catalog partial");
    expect(String(cue(catalogRead(false, false, { nativeMode: "snapshot" })).props.accessibilityLabel)).toContain("catalog snapshot");
    expect(String(cue(catalogRead(false, false, { nativeMode: "unavailable" })).props.accessibilityLabel)).toContain("catalog unavailable");
  });

  it("retains native content when Jira is unavailable and never treats idle as task completion", () => {
    const tree = overview({ plan }, { scoped: true }); const visible = text(tree);
    expect(visible).toContain("1 session"); expect(meta(tree)).toContain("Sessions in this scope: 1 session · 0 running");
    expect(visible).toContain("Epic status unknown"); expect(visible).toContain("Progress unavailable");
    expect(visible).toContain("Jira records not captured");
    expect(elements(overview({ plan }, { scoped: true })).some((node) => node.props.accessibilityLabel === "Sources and coverage · Jira not connected")).toBe(true);
  });
  it("lists the historical selection's epics only in an unscoped portfolio and keeps ad-hoc sessions as inventory", () => {
    const entries = [sessionEntry("idle", { status: "idle", labels: { epic: "ASMA-8539" } }), sessionEntry("loose", { status: "running" })];
    const tree = overview({ plan, jira: read }, { entries, contextLabel: undefined });
    const visible = text(tree);
    expect(visible).toContain("All projects"); expect(visible).not.toMatch(/\d+ of \d+ shown/);
    expect(visible).toContain("2 sessions · 1 running · 1 epic unknown");
    expect(meta(tree)).toContain("Host sessions: 2 sessions · 1 running"); expect(meta(tree)).toContain("1 with an unknown epic stay in Sessions.");
    expect(visible).not.toContain("Unassigned / ad-hoc");
    const phoneTree = overview({ plan, jira: read }, { entries, contextLabel: undefined, compact: true }); const phone = text(phoneTree);
    // Scope and the exact read stay in the meta label and Sources and coverage; host status lives only in the top-bar Native pill.
    expect(phone).not.toContain("kontord"); expect(phone).not.toContain("Last inventory read ");
    expect(meta(phoneTree)).toContain("Global overview, available without kontord."); expect(meta(phoneTree)).toContain("Last inventory read ");
    expect(phone).not.toContain("Live"); expect(phone).not.toContain("Not live"); expect(visible).not.toContain("Live host subscription"); expect(visible).not.toContain("Not live");
    expect(phone).toContain("2 sessions · 1 running · 1 epic unknown · inventory completeness Unknown");
    expect(phone).not.toContain("Native sessions come from this host"); expect(phone).not.toContain("Host sessions:");
    expect(visible).toContain("ASMA-8190"); expect(visible).toContain("Plan not captured"); expect(visible).not.toContain("captured tasks remain available");
    expect(visible).toContain("13 / 17 captured tasks done");
    const inspect = elements(tree).find((node) => String(node.props.accessibilityLabel).startsWith("ASMA-8190. Plan not captured"))!;
    const onEpicTab = vi.fn(); const again = overview({ plan, jira: read }, { entries, onEpicTab });
    (elements(again).find((node) => node.props.accessibilityLabel === inspect.props.accessibilityLabel)!.props.onPress as () => void)();
    expect(onEpicTab).toHaveBeenCalledWith("Board", "ASMA-8190", null);
  });
});
