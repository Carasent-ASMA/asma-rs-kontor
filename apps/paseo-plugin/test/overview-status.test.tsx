import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { describe, expect, it, vi } from "vitest";
import { jiraHistorySchema, type CanonicalGraph, type SourceRead } from "../shared/read-sources";
vi.mock("react-native", () => ({ View: "View", Text: "Text" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { NativeSummaryIndicators, PlanTaskStatus } from "../client/overview-status";
import { nativeSummary } from "../client/page-data";
import { sessionEntry } from "./session-fixtures";
type Element = ReactElement<Record<string, unknown>>;
const colors = { foreground: "black", foregroundMuted: "gray", statusSuccess: "green", statusWarning: "amber", statusDanger: "red", surface2: "neutral", accent: "blue", border: "border" } as Parameters<typeof PlanTaskStatus>[0]["colors"];
const history = jiraHistorySchema.parse(JSON.parse(readFileSync(new URL("./fixtures/jira-history.json", import.meta.url), "utf8")));
const read: SourceRead = { source: { id: "jira", title: "Historical Jira", kind: "jira", path: "/configured/projection.json", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }, status: "ready", revision: "a".repeat(64), readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: null, graph: null, records: null, diagnostics: [], jiraHistory: history };
const graph: CanonicalGraph = { schema_version: 1, source: "/plan.md", source_sha256: "b".repeat(64), graph_sha256: "c".repeat(64), epic: { jira: "ASMA-8539", kontor_id: null }, nodes: [8540, 8541, 8551, 8542, 8545, 8546, 8547, 8548, 8549, 8550].map((key) => ({ id: `task-${key}`, title: "Task", jira: `ASMA-${key}`, kontor_id: null, phase: "Phase", wave: 0, depends_on: [] })), edges: [], waves: [], external_fences: [] };
const plan: SourceRead = { ...read, source: { ...read.source, id: "plan", title: "Selected plan", path: graph.source, kind: "plan", epic: "ASMA-8539" }, revision: graph.source_sha256, jiraHistory: null, graph };
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return elements((node.type as (props: any) => ReactNode)(node.props));
  return [node, ...elements(node.props.children as ReactNode)];
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return text((node.type as (props: any) => ReactNode)(node.props));
  return text(node.props.children as ReactNode);
}
function capturedDone(count: number) {
  const copy = structuredClone(read);
  const epic = copy.jiraHistory!.epics.find((value) => value.key === "ASMA-8539")!;
  epic.children = graph.nodes.slice(0, count).map((node) => ({ key: node.jira!, status: "Closed", cat: "done" }));
  return copy;
}
function render(reads: SourceRead[], selected = plan) {
  const tree = PlanTaskStatus({ plan: selected, reads, colors });
  const nodes = elements(tree);
  return { visible: text(tree), graphic: nodes.find((node) => node.props.accessibilityRole === "image")!, segments: nodes.filter((node) => String(node.props.testID).startsWith("task-status-")) };
}
const widths = (view: ReturnType<typeof render>) => Object.fromEntries(view.segments.map((node) => [node.props.testID, (node.props.style as { width: string }).width]));
describe("RO-03 visual historical task status", () => {
  it("renders 3 observed done of 10 authored as 30% done and 70% Unknown, never full delivery", () => {
    const view = render([capturedDone(3)]);
    expect(widths(view)).toEqual({ "task-status-observed-done": "30%", "task-status-unknown": "70%" });
    expect(view.visible).toContain("Observed done 3/10 · other observed 0/10 · Unknown 7/10 · plan coverage 3/10");
    expect(view.visible).toContain(history.generated); expect(view.visible).toContain("not live"); expect(view.visible).toContain("current completion Unknown");
    expect(view.visible).not.toContain("100%"); expect(view.visible).not.toContain("delivery");
    expect(view.graphic.props.accessible).toBe(true);
    expect(view.graphic.props.accessibilityValue).toEqual({ text: expect.stringContaining("Observed done 3/10") });
    expect(view.graphic.props.accessibilityValue).not.toHaveProperty("now");
    for (const hidden of [read.source.path, read.readAt, read.revision!, graph.source_sha256, history.provenance.path, history.provenance.sha256]) expect(view.visible).not.toContain(hidden);
  });
  it("keeps other observed tasks separate and uses the authored denominator for every segment", () => {
    const capture = capturedDone(3); capture.jiraHistory!.epics.find((value) => value.key === "ASMA-8539")!.children[1]!.cat = "indeterminate";
    capture.jiraHistory!.epics.find((value) => value.key === "ASMA-8539")!.children[2]!.cat = "new";
    const view = render([capture]);
    expect(widths(view)).toEqual({ "task-status-observed-done": "10%", "task-status-other-observed": "20%", "task-status-unknown": "70%" });
    expect(view.visible).toContain("Observed done 1/10 · other observed 2/10 · Unknown 7/10 · plan coverage 3/10");
    expect((view.segments[2]!.props.style as { borderStyle: string }).borderStyle).toBe("dashed");
  });
  it.each(["missing", "unavailable", "conflicting", "wrong parent"])("renders %s evidence as all Unknown rather than zero-complete", (kind) => {
    const conflict = capturedDone(3); const wrongParent = structuredClone(conflict); wrongParent.jiraHistory!.epics.find((value) => value.key === "ASMA-8539")!.key = "ASMA-9999";
    const contradicting = structuredClone(conflict); contradicting.jiraHistory!.epics.find((value) => value.key === "ASMA-8539")!.children.forEach((child) => { child.status = "Contradicting observation"; child.cat = "new"; });
    const reads = kind === "missing" ? [] : kind === "unavailable" ? [{ ...read, status: "unavailable" as const }] : kind === "wrong parent" ? [wrongParent] : [conflict, contradicting];
    const view = render(reads);
    expect(widths(view)).toEqual({ "task-status-unknown": "100%" });
    expect(view.visible).toContain("Task status Unknown / Not captured · plan coverage 0/10 · 10 tasks Unknown");
    expect(view.visible).not.toContain("Observed done 0"); expect(view.visible).not.toContain("complete 0");
    expect((view.segments[0]!.props.style as { backgroundColor: string }).backgroundColor).toBe("neutral");
    if (kind === "missing" || kind === "unavailable") expect(view.visible).toContain("Historical capture Not captured");
  });
  it.each(["missing graph", "unavailable graph", "empty plan"])("keeps %s neutral with no false denominator or NaN", (kind) => {
    const selected = kind === "missing graph" ? { ...plan, graph: null } : kind === "unavailable graph" ? { ...plan, status: "unavailable" as const } : { ...plan, graph: { ...graph, nodes: [] } };
    const view = render([read], selected);
    expect(view.visible).toContain("Task status Unknown / Not captured"); expect(view.visible).toContain("coverage Not captured");
    expect(widths(view)).toEqual({ "task-status-unknown": "100%" });
    expect(JSON.stringify(view.graphic.props)).not.toMatch(/NaN|Infinity/);
    expect(view.visible).not.toContain("0/0");
  });
  it("keeps even fully observed done states historical, with current completion Unknown", () => {
    const view = render([capturedDone(10)]);
    expect(widths(view)).toEqual({ "task-status-observed-done": "100%" });
    expect(view.visible).toContain("Observed done 10/10"); expect(view.visible).toContain("plan coverage 10/10");
    expect(view.visible).toContain("historical task status"); expect(view.visible).toContain("current completion Unknown");
  });
  it("does not double-count matching sources and retains both historical capture times", () => {
    const capture = capturedDone(3); const second = structuredClone(capture); second.jiraHistory!.generated = "2026-10-17T12:00:00Z";
    const view = render([capture, second]);
    expect(widths(view)).toEqual({ "task-status-observed-done": "30%", "task-status-unknown": "70%" });
    expect(view.visible).toContain(history.generated); expect(view.visible).toContain(second.jiraHistory!.generated);
  });
});
describe("compact native summary indicators", () => {
  it("labels native runtime/attention and needs-input independently without implying task completion or governance", () => {
    const counts = nativeSummary([sessionEntry("running", { status: "running" }), sessionEntry("idle", { status: "idle" }), sessionEntry("closed", { status: "closed" }), sessionEntry("permission", { status: "idle", attentionReason: "permission", requiresAttention: false }), sessionEntry("error", { status: "error", requiresAttention: true })]);
    const tree = NativeSummaryIndicators({ counts, colors }); const nodes = elements(tree);
    const labels = nodes.filter((node) => node.props.accessible).map((node) => node.props.accessibilityLabel);
    expect(labels).toEqual(["Native snapshot: 5 sessions", "Native snapshot: 1 running", "Native snapshot: 2 idle", "Native snapshot: 1 closed", "Native snapshot: 1 native attention", "Native snapshot: 1 needs input", "Native snapshot: 1 native errors"]);
    expect(nodes.filter((node) => node.props.accessibilityRole === "text")).toHaveLength(7);
    expect(tree.props.style.flexWrap).toBe("wrap");
    expect(text(tree)).not.toMatch(/done|delivery|decision|governance|Live/);
  });
});
