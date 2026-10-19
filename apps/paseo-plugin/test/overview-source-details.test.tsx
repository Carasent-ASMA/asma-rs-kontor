import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { jiraHistorySchema, type CanonicalGraph, type SourceRead } from "../shared/read-sources";
const model = vi.hoisted(() => ({ open: false }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: () => [model.open, (value: boolean) => { model.open = value; }], useRef: (current: unknown) => ({ current }), useMemo: (factory: () => unknown) => factory() }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", Text: "Text", View: "View", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { SourceDetails } from "../client/source-details";
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
  return [node, ...elements(node.props.children as ReactNode)];
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return text((node.type as (props: any) => ReactNode)(node.props));
  return text(node.props.children as ReactNode);
}
function overview(reads: Record<string, SourceRead>) {
  const props: Props = { tab: "Overview", contextLabel: "Selected plan", colors: {} as Props["colors"], entries: [sessionEntry("idle", { status: "idle", labels: { epic: "ASMA-8539" } })], directory: { entries: [], status: "ready", live: false, lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false }, sources: Object.values(reads).map((value) => value.source), reads, sourceId: "plan", sourceError: null, query: "", setQuery: vi.fn(), onSource: vi.fn(), onOpen: vi.fn(), onTab: vi.fn(), onEpicTab: vi.fn(), initialOffset: 0, onScroll: vi.fn(), initialDocumentOffset: 0, onDocumentScroll: vi.fn() };
  const outer = ReadOnlyPage(props);
  const child = elements(outer).find((element) => element.props.tab === "Overview")!;
  return (child.type as (props: Props) => ReactNode)(child.props as Props);
}
beforeEach(() => { model.open = false; });
describe("compact Overview source evidence", () => {
  it("keeps source state and original observation visible, expanding technical details only on request", () => {
    const render = () => SourceDetails({ read, colors: {} as Props["colors"] });
    const closed = render(); const visible = text(closed);
    expect(visible).toContain("ready"); expect(visible).toContain("Historical Jira snapshot");
    expect(visible).toContain("Observed 2026-10-18T17:00+00:00 · not live");
    for (const value of [read.source.path, read.revision!, history.provenance.path, history.provenance.sha256, read.readAt]) expect(visible).not.toContain(value);
    const action = elements(closed).find((element) => element.props.label === "Source details")!;
    (action.props.onPress as () => void)();
    const opened = text(render());
    for (const value of [read.source.path, read.revision!, history.provenance.path, history.provenance.sha256, read.readAt]) expect(opened).toContain(value);
    expect(opened).toContain("Neither count establishes complete inventory");
  });
  it("marks a missing historical source unavailable without substituting the read time for observation time", () => {
    const visible = text(SourceDetails({ read: { ...read, status: "unavailable", jiraHistory: null, diagnostics: ["/unavailable/source.json"] }, colors: {} as Props["colors"] }));
    expect(visible).toContain("unavailable"); expect(visible).toContain("Observed Unknown · not live");
    expect(visible).not.toContain(read.readAt); expect(visible).not.toContain("/unavailable/source.json");
  });
  it("shows the historical workflow and 3/10 coverage without presenting current completion", () => {
    const tree = overview({ plan, jira: read }); const visible = text(tree);
    expect(visible).toContain("Jira workflow: Groomed · historical, observed 2026-10-18T17:00+00:00");
    expect(visible).toContain("Observed done 0/10 · other observed 3/10 · Unknown 7/10 · plan coverage 3/10");
    expect(visible).toContain("Current completion: Unknown");
    expect(visible).toContain("kontord is not enabled"); expect(visible).not.toContain("kontord not connected");
    expect(elements(tree).some((element) => element.props.label === "Open plan")).toBe(true);
    expect(elements(tree).some((element) => element.props.label === "Sessions")).toBe(true);
    expect(elements(tree).filter((element) => String(element.props.label).startsWith("Open plan:")).length).toBe(0);
  });
  it("retains native content when Jira is unavailable and never treats idle as task completion", () => {
    const visible = text(overview({ plan }));
    expect(visible).toContain("0 running"); expect(visible).toContain("1 idle"); expect(visible).toContain("0 closed");
    expect(visible).toContain("plan coverage 0/10 · 10 tasks Unknown");
    expect(visible).toContain("Jira workflow: Unknown");
    expect(visible).toContain("Historical Jira: not connected to a configured status source");
  });
});
