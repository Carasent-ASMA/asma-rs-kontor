import { readFileSync } from "node:fs";
import { URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { jiraHistorySchema, type CanonicalGraph, type JiraHistory, type SourceRead } from "../shared/read-sources";
const model = vi.hoisted(() => ({ evidence: false }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (factory: () => unknown) => factory(), useRef: (current: unknown) => ({ current }), useState: () => [model.evidence, (value: boolean) => { model.evidence = value; }] }));
vi.mock("react-native", () => ({ Text: "Text", View: "View", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ FlatList: "FlatList", ScrollView: "ScrollView", Icon: "Icon" }));
import { GraphPage, captureDay, capturedJiraChildren, type BoardItem } from "../client/graph-page";
import { PlanGraph } from "../client/plan-graph/graph";
import { SourceDetails } from "../client/source-details";
import { Action, Disclosure } from "../client/page-controls";
import { TaskSessionsChooser, TaskSessionsControl, type BoardSessions } from "../client/board-sessions";
import { MenuItem } from "../client/anchored-menu";
import { emptyDirectoryState, type DirectoryEntry } from "../client/session-directory";
import { taskSessions } from "../client/session-results";
import type { TaskTeamDeclaration, TaskTeamReader } from "../client/task-teams";
import { sessionEntry } from "./session-fixtures";
beforeEach(() => { model.evidence = false; });

type Element = ReactElement<Record<string, unknown>>;
const graph: CanonicalGraph = JSON.parse(readFileSync(new URL("./fixtures/plan-graph-asma-8371.json", import.meta.url), "utf8"));
const history = jiraHistorySchema.parse(JSON.parse(readFileSync(new URL("./fixtures/jira-history.json", import.meta.url), "utf8")));
const children = history.epics.find((epic) => epic.key === "ASMA-8371")!.children;
const plan: SourceRead = { source: { id: "plan", title: "Plan", kind: "plan", path: graph.source, workstream: null, epic: graph.epic.jira, projectId: null, revisionKind: "working copy" }, status: "ready", revision: graph.source_sha256, readAt: "2026-10-19T00:00:00Z", modifiedAt: null, content: "# Plan", graph, records: null, diagnostics: [] };
const jira: SourceRead = { ...plan, source: { ...plan.source, id: "jira", title: "Historical Jira", kind: "jira", epic: null, path: "/configured/history.json" }, revision: history.provenance.sha256, content: null, graph: null, jiraHistory: history };
const colors = { foreground: "fg", foregroundMuted: "muted", accent: "accent", accentForeground: "accent-fg", surface0: "s0", surface1: "s1", surface2: "s2", border: "border", statusDanger: "danger", statusSuccess: "success", statusWarning: "warning" };

/** Local row components render inline; shared/SDK components stay leaves. */
const ROWS = new Set(["BoardRow", "CompactBoardRow"]);
const isRow = (node: Element) => typeof node.type === "function" && ROWS.has(node.type.name);
const expand = (node: Element) => (node.type as (props: Record<string, unknown>) => Element)(node.props);
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (isRow(node)) return [node, ...elements(expand(node))];
  return [node, ...elements(node.props.children as ReactNode)];
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (isRow(node)) return text(expand(node));
  return text(node.props.children as ReactNode);
}
function mount(options: { selectedId?: string | null; reads?: SourceRead[]; epicKey?: string | null; compact?: boolean; mode?: "Board" | "Graph"; initialOffset?: number; onBoard?: () => void; page?: typeof GraphPage; sessions?: BoardSessions; taskTeams?: TaskTeamReader } = {}) {
  const reads = options.reads ?? [jira];
  const onPlan = vi.fn(); const onIssue = vi.fn(); const onChooseSource = vi.fn(); const onScroll = vi.fn();
  const tree = (options.page ?? GraphPage)({ mode: options.mode ?? "Board", hostId: "board-host", theme: { colors } as PluginTheme, compact: options.compact ?? true,
    sources: reads.map((read) => read.source), selectedId: options.selectedId ?? null, reads: Object.fromEntries(reads.map((read) => [read.source.id, read])),
    available: true, error: null, epicKey: options.epicKey, onSelect: vi.fn(), onPlan, onIssue, onChooseSource, onScroll, initialOffset: options.initialOffset, onBoard: options.onBoard, sessions: options.sessions, taskTeams: options.taskTeams }) as Element;
  // Board wraps its list in a View that also holds the linked-session chooser.
  const list = elements(tree).find((element) => element.type === "FlatList");
  const data = (list?.props.data ?? []) as BoardItem[];
  const row = (item: BoardItem) => (list!.props.renderItem as (info: { item: BoardItem; index: number }) => Element)({ item, index: data.indexOf(item) });
  const header = list?.props.ListHeaderComponent as ReactNode;
  return { tree, list, data, row, header, onPlan, onIssue, onChooseSource, onScroll };
}
const actions = (node: ReactNode) => elements(node).filter((element) => element.type === Action);
const jiraRows = (data: BoardItem[]) => data.flatMap((item) => item.kind === "jira" ? [item] : []);
const taskRows = (data: BoardItem[]) => data.flatMap((item) => item.kind === "task" ? [item] : []);
/** The compact row's card, holding the text column, the issue action beside it and any linked sessions below. */
const card = (row: Element) => expand(row);
/** The compact row's text column: the row button (plan task) or the grouped row (captured Jira); no other button sits inside it. */
const outer = (row: Element) => elements(card(row)).find((element) => element.props.accessible === true)!;
const label = (row: Element) => String(outer(row).props.accessibilityLabel);
/** The row's full description: the compact row button, or the wide row's detail column. */
const rowLabel = (row: Element) => String(elements(row).find((element) => element.props.accessible === true)!.props.accessibilityLabel);
const issueButtons = (row: Element) => elements(row).filter((element) => element.type === "Pressable" && String(element.props.accessibilityLabel).startsWith("Open issue"));
/** Text of a chain, drawing its inline parts (role steps and glyph runs) the way React would. */
const INLINE = new Set(["ChainText", "TeamChainText", "Glyphs", "TeamBlock"]);
function inlineText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(inlineText).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function" && INLINE.has(node.type.name)) return inlineText(expand(node));
  return inlineText(node.props.children as ReactNode);
}
const metaOf = (row: Element) => text(elements(row).find((element) => element.type === "Text" && element.props.numberOfLines === 1 && element.props.style && (element.props.style as { flex?: number }).flex === 1));
const day = captureDay(history.generated);

describe("Board without a valid canonical graph (BOARD-01)", () => {
  it("lists every captured Jira child of the context epic with status, category, capture time and source, and no title", () => {
    const { tree, list, data, row, header } = mount({ epicKey: "ASMA-8371" });
    expect(list).toBeDefined();
    expect(elements(tree).some((element) => element.type === PlanGraph)).toBe(false);
    expect(jiraRows(data).map((item) => item.record.key)).toEqual(children.map((child) => child.key));
    expect(data[0]).toMatchObject({ kind: "section", title: `Captured Jira tasks · ${children.length}` });
    expect((data[0] as { note: string }).note).toContain("Plan declaration and dependencies: unavailable");
    const first = row(jiraRows(data)[0]!);
    expect(text(first)).toContain(`${children[0]!.key} · title not captured`);
    expect(text(first)).not.toContain("projection");
    expect(label(first)).toContain(`${children[0]!.key}: title unavailable in the captured Jira projection.`);
    expect(metaOf(first)).toBe(`${children[0]!.status} (${children[0]!.cat}) · captured ${day}`);
    expect(label(first)).toContain(`${children[0]!.status} (${children[0]!.cat}), captured ${history.generated} from Historical Jira`);
    expect(label(first)).toContain("Plan declaration and dependencies unavailable");
    // Compact moves the notes and the truth statement into Source details and the meta label.
    expect(text(header)).not.toContain("No plan selected.");
    expect(elements(header).find((element) => element.props.testID === "board-meta")!.props.accessibilityLabel).toContain("Current completion and readiness: Unknown");
    model.evidence = true;
    const opened = text(mount({ epicKey: "ASMA-8371" }).header);
    expect(opened).toContain("No plan selected. Plan declarations and dependencies are unavailable.");
    expect(opened).toContain("Current completion and readiness: Unknown");
    expect(opened).toContain("Plan declaration and dependencies: unavailable without a valid canonical plan graph.");
    model.evidence = false;
    // Wide shows the same short row and header; the exact capture, source, declaration and notes move with it.
    const wide = mount({ epicKey: "ASMA-8371", compact: false });
    const wideRow = wide.row(jiraRows(wide.data)[0]!);
    expect(text(wideRow)).toContain(`${children[0]!.status} (${children[0]!.cat}) · captured ${day}`);
    expect(text(wideRow)).not.toContain(history.generated);
    expect(text(wideRow)).not.toContain("Plan declaration and dependencies unavailable");
    expect(rowLabel(wideRow)).toContain(`${children[0]!.status} (${children[0]!.cat}), captured ${history.generated} from Historical Jira`);
    expect(rowLabel(wideRow)).toContain("Plan declaration and dependencies unavailable");
    expect(text(wide.header)).not.toContain("No plan selected.");
    expect(text(wide.header)).not.toContain("Current completion and readiness");
    expect(elements(wide.header).find((element) => element.props.testID === "board-meta")!.props.accessibilityLabel).toContain("Current completion and readiness: Unknown");
    model.evidence = true;
    const wideNotes = text(elements(mount({ epicKey: "ASMA-8371", compact: false }).header).find((element) => element.props.testID === "board-source-notes"));
    expect(wideNotes).toContain("No plan selected. Plan declarations and dependencies are unavailable.");
    expect(wideNotes).toContain("Jira states are dated historical captures. Current completion and readiness: Unknown.");
  });

  it.each([true, false])("never invents a title: no plan title appears on captured rows, even for keys a plan declares (compact %s)", (compact) => {
    const { data, row } = mount({ selectedId: "plan", reads: [{ ...plan, revision: "f".repeat(64) }, jira], compact });
    expect(jiraRows(data).map((item) => item.record.key)).toEqual(expect.arrayContaining(graph.nodes.map((node) => node.jira)));
    const rendered = jiraRows(data).map((item) => `${text(row(item))}\n${rowLabel(row(item))}`).join("\n");
    expect(graph.nodes.filter((node) => rendered.includes(node.title)).map((node) => node.id)).toEqual([]);
    expect(jiraRows(data).every((item) => text(row(item)).includes(`${item.record.key} · title not captured`))).toBe(true);
    expect(jiraRows(data).every((item) => rowLabel(row(item)).includes("title unavailable in the captured Jira projection"))).toBe(true);
  });

  it("offers exactly one Open issue action per captured row, bound to the exact key, and no plan action", () => {
    const { data, row, onIssue, onPlan } = mount({ epicKey: "ASMA-8371" });
    const target = row(jiraRows(data)[3]!);
    const key = jiraRows(data)[3]!.record.key;
    expect(issueButtons(target).map((button) => button.props.accessibilityLabel)).toEqual([`Open issue ${key}`]);
    expect(outer(target).type).toBe("View");
    expect(outer(target).props.onPress).toBeUndefined();
    (issueButtons(target)[0]!.props.onPress as () => void)();
    expect(outer(target).props.accessibilityActions).toEqual([{ name: "openIssue", label: `Open issue ${key}` }]);
    (outer(target).props.onAccessibilityAction as (event: object) => void)({ nativeEvent: { actionName: "openIssue" } });
    (outer(target).props.onAccessibilityAction as (event: object) => void)({ nativeEvent: { actionName: "activate" } });
    expect(onIssue.mock.calls).toEqual([[key], [key]]); expect(onPlan).not.toHaveBeenCalled();
    const wide = mount({ epicKey: "ASMA-8371", compact: false });
    const buttons = actions(wide.row(jiraRows(wide.data)[3]!));
    expect(buttons.map((button) => [button.props.label, button.props.accessibilityLabel])).toEqual([["Open issue", `Open issue ${key}`]]);
  });

  it("falls back to captured records when the selected plan's graph is from another revision", () => {
    const stale: SourceRead = { ...plan, revision: "f".repeat(64) };
    const { data, header, row } = mount({ selectedId: "plan", reads: [stale, jira] });
    expect(jiraRows(data)).toHaveLength(children.length);
    expect(data.some((item) => item.kind === "task")).toBe(false);
    expect(actions(header).map((button) => button.props.label)).toEqual(["Open source plan", "Source details"]);
    expect(text(header)).toContain("no plan graph");
    expect(text(mount({ selectedId: "plan", reads: [stale, jira], compact: false }).header)).not.toContain("No valid exact-source canonical graph available");
    model.evidence = true;
    expect(text(mount({ selectedId: "plan", reads: [stale, jira] }).header)).toContain("No valid exact-source canonical graph available");
    expect(text(mount({ selectedId: "plan", reads: [stale, jira], compact: false }).header)).toContain("No valid exact-source canonical graph available");
    expect(text(row(jiraRows(data)[0]!))).not.toContain(graph.nodes[0]!.title);
  });

  it.each([true, false])("shows an honest empty state with Choose plan and no fabricated rows when there is no epic and no plan (compact %s)", (compact) => {
    const { data, header, onChooseSource } = mount({ epicKey: null, compact });
    expect(data).toEqual([]);
    expect(text(header)).toContain("No plan or epic in this context.");
    const choose = actions(header).find((button) => button.props.label === "Choose plan")!;
    (choose.props.onPress as () => void)(); expect(onChooseSource).toHaveBeenCalledTimes(1);
    expect(capturedJiraChildren(null, [jira]).records).toEqual([]);
  });

  it.each([true, false])("says when the epic or its children were not captured instead of implying an empty epic (compact %s)", (compact) => {
    const missing = mount({ epicKey: "ASMA-1", compact });
    expect(text(missing.header)).toContain("ASMA-1 is not in the captured Jira projection. This is not evidence that the epic has no tasks.");
    const empty: JiraHistory = { ...history, epics: history.epics.map((epic) => epic.key === "ASMA-8371" ? { ...epic, children: [] } : epic) };
    const none = mount({ epicKey: "ASMA-8371", reads: [{ ...jira, jiraHistory: empty }], compact });
    expect(none.data).toEqual([]);
    expect(text(none.header)).toContain("No child task records were captured for ASMA-8371.");
    const partial = mount({ epicKey: "ASMA-8371", reads: [{ ...jira, status: "partial" }], compact });
    expect(partial.data).toEqual([]);
    expect(text(partial.header)).toContain("Historical Jira source unavailable or partial");
    if (compact) {
      expect(text(missing.header)).toContain("Jira not captured");
      expect(text(none.header)).toContain("no Jira children captured");
    }
  });

  it("lists disagreeing captures from two sources side by side instead of choosing one", () => {
    const later: SourceRead = { ...jira, source: { ...jira.source, id: "jira-later", title: "Later capture" },
      jiraHistory: { ...history, generated: "2026-10-19T08:00+00:00", epics: history.epics.map((epic) => epic.key === "ASMA-8371" ? { ...epic, children: epic.children.map((child, index) => index === 0 ? { ...child, status: "In Development", cat: "indeterminate" as const } : child) } : epic) } };
    const { data, row } = mount({ epicKey: "ASMA-8371", reads: [jira, later] });
    const first = jiraRows(data)[0]!;
    expect(first.record.observations.map((value) => [value.status, value.observedAt, value.sourceTitle])).toEqual([
      [children[0]!.status, history.generated, "Historical Jira"], ["In Development", "2026-10-19T08:00+00:00", "Later capture"]]);
    expect(metaOf(row(first))).toBe(`Captures disagree: ${children[0]!.status} (${children[0]!.cat}) ${day} / In Development (indeterminate) 19 Oct`);
    expect(label(row(first))).toContain("In Development (indeterminate), captured 2026-10-19T08:00+00:00 from Later capture");
    expect(elements(row(first)).find((element) => String(element.props.testID).endsWith("-tone"))!.props.style).toMatchObject({ backgroundColor: "muted" });
    // Agreeing captures from both sources keep one status with both capture days.
    expect(metaOf(row(jiraRows(data)[1]!))).toBe(`${children[1]!.status} (${children[1]!.cat}) · captured ${day} / 19 Oct`);
    const wide = mount({ epicKey: "ASMA-8371", reads: [jira, later], compact: false });
    expect(text(wide.row(jiraRows(wide.data)[0]!))).toContain(`Captures disagree: ${children[0]!.status} (${children[0]!.cat}) ${day} / In Development (indeterminate) 19 Oct`);
    expect(rowLabel(wide.row(jiraRows(wide.data)[0]!))).toContain("In Development (indeterminate), captured 2026-10-19T08:00+00:00 from Later capture");
    expect(jiraRows(data)).toHaveLength(children.length);
  });

  it("restores the fallback list offset once and then reports user scrolling", () => {
    const { list, onScroll } = mount({ epicKey: "ASMA-8371", initialOffset: 640 });
    const scrollToOffset = vi.fn(); (list!.props.ref as { current: unknown }).current = { scrollToOffset };
    (list!.props.onScroll as (event: object) => void)({ nativeEvent: { contentOffset: { y: 0 } } });
    expect(onScroll).not.toHaveBeenCalled();
    (list!.props.onContentSizeChange as (width: number, height: number) => void)(400, 3000);
    expect(scrollToOffset).toHaveBeenCalledWith({ offset: 640, animated: false });
    (list!.props.onScroll as (event: object) => void)({ nativeEvent: { contentOffset: { y: 120 } } });
    (list!.props.onContentSizeChange as (width: number, height: number) => void)(400, 3600);
    expect(onScroll.mock.calls).toEqual([[120]]); expect(scrollToOffset).toHaveBeenCalledTimes(1);
  });
});

describe("Board with a valid canonical graph", () => {
  it("keeps canonical task rows and adds captured children no task declares as a separate exact-key group", () => {
    const extra = { key: "ASMA-83720", status: "To Do", cat: "new" as const };
    const withExtra: SourceRead = { ...jira, jiraHistory: { ...history, epics: history.epics.map((epic) => epic.key === "ASMA-8371" ? { ...epic, children: [...epic.children, extra] } : epic) } };
    const { data, row } = mount({ selectedId: "plan", reads: [plan, withExtra], epicKey: "ASMA-1" });
    const declared = new Set(graph.nodes.map((node) => node.jira));
    const undeclared = [...children.map((child) => child.key).filter((key) => !declared.has(key)), extra.key];
    expect(data.map((item) => item.kind)).toEqual(["section", ...graph.nodes.map(() => "task"), "section", ...undeclared.map(() => "jira")]);
    expect(data.flatMap((item) => item.kind === "task" ? [item.node.id] : [])).toEqual(graph.nodes.map((node) => node.id));
    expect(jiraRows(data).map((item) => item.record.key)).toEqual(undeclared);
    const section = data[graph.nodes.length + 1] as { title: string; note: string };
    expect(section.title).toBe(`Captured Jira tasks not declared in this plan · ${undeclared.length}`);
    expect(section.note).toContain("Children of ASMA-8371");
    const last = row(jiraRows(data).at(-1)!);
    expect(text(last)).toContain("ASMA-83720 · title not captured");
    expect(metaOf(last)).toBe(`To Do (new) · captured ${day}`);
    expect(label(last)).toContain("Not declared in this plan revision");
    const task = row(data[1]!);
    expect(text(task)).toContain(`${graph.nodes[0]!.jira} · ${graph.nodes[0]!.title}`);
    expect(metaOf(task)).toBe(`TASK-001 · Closed (done) · captured ${day} · Phase 1`);
    expect(metaOf(row(data[3]!))).toBe(`TASK-003 · Closed (done) · captured ${day} · Phase 2 · after TASK-001`);
    expect(label(task)).toContain(`Closed (done), captured ${history.generated} from Historical Jira`);
    const wide = mount({ selectedId: "plan", reads: [plan, withExtra], epicKey: "ASMA-1", compact: false });
    expect(text(wide.row(jiraRows(wide.data).at(-1)!))).not.toContain("Not declared in this plan revision");
    expect(rowLabel(wide.row(jiraRows(wide.data).at(-1)!))).toContain("Not declared in this plan revision");
    const wideTask = wide.row(wide.data[1]!);
    expect(text(wideTask)).toContain(`TASK-001 · Closed (done) · captured ${day} · Phase 1`);
    expect(text(wideTask)).not.toContain("depth");
    expect(text(wideTask)).not.toContain(history.generated);
    expect(rowLabel(wideTask)).toContain(`Closed (done), captured ${history.generated} from Historical Jira`);
    expect(rowLabel(wideTask)).toContain("Phase 1, dependency depth 0. No prerequisites declared.");
  });

  it("opens each task in the plan from the whole compact row with its exact canonical task id, never a Jira key or title", () => {
    const { data, row, onPlan, onIssue } = mount({ selectedId: "plan", reads: [plan, jira] });
    for (const item of taskRows(data)) {
      const button = outer(row(item));
      expect(button.type).toBe("Pressable");
      expect(button.props.accessibilityRole).toBe("button");
      expect(String(button.props.accessibilityLabel).startsWith(`Open plan task ${item.node.id}: ${item.node.title}. `)).toBe(true);
      (button.props.onPress as () => void)();
    }
    expect(onPlan.mock.calls).toEqual(graph.nodes.map((node) => ["plan", node.id]));
    expect(onIssue).not.toHaveBeenCalled();
    const first = row(data[1]!);
    expect(issueButtons(first)).toHaveLength(1);
    (issueButtons(first)[0]!.props.onPress as () => void)();
    (outer(first).props.onAccessibilityAction as (event: object) => void)({ nativeEvent: { actionName: "openIssue" } });
    expect(onIssue.mock.calls).toEqual([[graph.nodes[0]!.jira], [graph.nodes[0]!.jira]]);
    expect(onPlan).toHaveBeenCalledTimes(graph.nodes.length);
    expect(label(first)).toContain("Phase 1, dependency depth 0. No prerequisites declared.");
    expect(label(row(data[3]!))).toContain("Phase 2, dependency depth 1. After TASK-001.");
    const wide = mount({ selectedId: "plan", reads: [plan, jira], compact: false });
    for (const item of taskRows(wide.data)) (actions(wide.row(item)).find((button) => button.props.accessibilityLabel === `Open plan task ${item.node.id}`)!.props.onPress as () => void)();
    expect(wide.onPlan.mock.calls).toEqual(graph.nodes.map((node) => ["plan", node.id]));
  });

  it("keeps Unknown visible and the status dot muted without a key or a consistent capture", () => {
    const tone = (rendered: Element) => (elements(rendered).find((element) => String(element.props.testID).endsWith("-tone"))!.props.style as { backgroundColor: string }).backgroundColor;
    const unbound: CanonicalGraph = { ...graph, nodes: graph.nodes.map((node, index) => index === 0 ? { ...node, jira: null } : node) };
    const noKey = mount({ selectedId: "plan", reads: [{ ...plan, graph: unbound }, jira] });
    const first = noKey.row(noKey.data[1]!);
    expect(metaOf(first)).toBe("TASK-001 · Unknown · no Jira key · Phase 1");
    expect(label(first)).toContain("No Jira binding declared, status Unknown");
    expect(issueButtons(first)).toEqual([]);
    expect(outer(first).props.accessibilityActions).toEqual([]);
    expect(tone(first)).toBe("muted");
    const partial = mount({ selectedId: "plan", reads: [plan, { ...jira, status: "partial" }] });
    const noCapture = partial.row(partial.data[1]!);
    expect(metaOf(noCapture)).toBe("TASK-001 · Unknown · no consistent capture · Phase 1");
    expect(label(noCapture)).toContain(`Jira ${graph.nodes[0]!.jira}: no consistent captured record, status Unknown`);
    expect(tone(noCapture)).toBe("muted");
    expect(issueButtons(noCapture)).toHaveLength(1);
  });

  it("colours the status dot from the captured Jira category only", () => {
    const { data, row } = mount({ selectedId: "plan", reads: [plan, jira] });
    const tone = (item: BoardItem) => (elements(row(item)).find((element) => String(element.props.testID).endsWith("-tone"))!.props.style as { backgroundColor: string }).backgroundColor;
    expect(tone(data[1]!)).toBe("success");
    expect(tone(taskRows(data).find((item) => item.node.id === "TASK-022")!)).toBe("muted");
    const byCat = (cat: string) => jiraRows(data).find((item) => item.record.observations[0]!.cat === cat)!;
    expect(tone(byCat("indeterminate"))).toBe("warning");
    expect(tone(byCat("new"))).toBe("muted");
    expect(tone(byCat("done"))).toBe("success");
  });
});

describe("compact Board density (iPhone 390x844)", () => {
  it("renders one title line, one meta line and one row of actions in the compact header", () => {
    const { header } = mount({ selectedId: "plan", reads: [plan, jira] });
    const outside = (node: Element) => node.type === Action;
    const walk = (node: ReactNode): Element[] => {
      if (Array.isArray(node)) return node.flatMap(walk);
      if (!isValidElement<Record<string, unknown>>(node) || outside(node)) return [];
      return [node, ...walk(node.props.children as ReactNode)];
    };
    const texts = walk(header).filter((element) => element.type === "Text" || typeof element.type === "function");
    expect(texts.map((element) => element.props.testID ?? element.props.accessibilityRole)).toEqual(["header", "board-meta"]);
    expect(texts.map((element) => element.props.numberOfLines)).toEqual([1, 1]);
    expect(text(texts[1])).toBe(`${children.length} Jira · ${graph.nodes.length} plan tasks · captured ${day}`);
    expect(actions(header).map((button) => button.props.label)).toEqual(["Open source plan", "Source details"]);
    expect(text(header)).not.toContain("Jira states are dated historical captures");
    model.evidence = true;
    const notes = elements(mount({ selectedId: "plan", reads: [plan, jira] }).header).find((element) => element.props.testID === "board-source-notes")!;
    expect(text(notes)).toContain("Jira states are dated historical captures. Current completion and readiness: Unknown.");
    expect(text(notes)).toContain("Plan tasks · 19: Canonical declarations in this exact plan revision.");
    expect(text(notes)).toContain("Captured Jira tasks not declared in this plan");
  });

  it("keeps the wide header to the same title and meta line, with the plan and epic in the title's label", () => {
    const { header, data, row } = mount({ selectedId: "plan", reads: [plan, jira], compact: false });
    const heading = elements(header).find((element) => element.props.accessibilityRole === "header")!;
    expect(text(heading)).toBe("Board");
    expect(heading.props.accessibilityLabel).toBe(`Board · ${graph.epic.jira} · Plan`);
    expect(text(elements(header).find((element) => element.props.testID === "board-meta"))).toBe(`${children.length} Jira · ${graph.nodes.length} plan tasks · captured ${day}`);
    expect(text(header)).not.toContain("Jira states are dated historical captures");
    const section = row(data[0]!);
    expect(text(section)).toBe((data[0] as { title: string }).title);
    expect(elements(section).find((element) => element.props.accessibilityRole === "header")!.props.accessibilityHint).toBe((data[0] as { note: string }).note);
  });

  it("keeps section rows to one line on compact, with the note as the hint", () => {
    const { data, row } = mount({ selectedId: "plan", reads: [plan, jira] });
    const section = row(data[0]!);
    expect(elements(section).filter((element) => element.type === "Text").map((element) => [element.props.numberOfLines, element.props.accessibilityHint]))
      .toEqual([[1, (data[0] as { note: string }).note]]);
    expect(elements(section).some((element) => typeof element.type === "function")).toBe(false);
  });

  it("budgets each compact task row to a two-line title and one meta line within 64-76pt, with a real 44pt issue target beside the text on touch", () => {
    const { data, row } = mount({ selectedId: "plan", reads: [plan, jira] });
    for (const item of [...taskRows(data), ...jiraRows(data)]) {
      const rendered = row(item);
      expect(actions(rendered)).toEqual([]);
      // Expand once so element identity holds across the queries below.
      const [shellElement, ...tree] = elements(card(rendered));
      const issue = tree.filter((element) => element.type === "Pressable" && String(element.props.accessibilityLabel).startsWith("Open issue"));
      const inIssue = new Set(elements(issue));
      const lines = tree.filter((element) => element.type === "Text" && element.props.numberOfLines !== undefined && !inIssue.has(element));
      expect(lines.map((element) => element.props.numberOfLines)).toEqual([2, 1]);
      const [title, meta] = lines as [Element, Element];
      const shell = shellElement!.props.style as Record<string, number>;
      const column = tree.find((element) => element.props.accessible === true)!;
      const metaRow = tree.find((element) => element.type === "View" && [element.props.children].flat(Infinity).includes(meta))!;
      const textHeight = 2 * (title.props.style as { lineHeight: number }).lineHeight + (column.props.style as { gap: number }).gap + (metaRow.props.style as { minHeight: number }).minHeight;
      if (item.kind === "task") {
        // The row button is a real 44pt target on touch, even when the title takes one line (17 + 2 + 22 = 41pt).
        const oneLine = (title.props.style as { lineHeight: number }).lineHeight + (column.props.style as { gap: number }).gap + (metaRow.props.style as { minHeight: number }).minHeight;
        expect(column.type).toBe("Pressable");
        expect(oneLine).toBeLessThan(44);
        expect((column.props.style as { minHeight: number }).minHeight).toBe(44);
      }
      const target = issue.length ? (issue[0]!.props.style as { minHeight: number }).minHeight : 0;
      const height = 2 * shell.paddingVertical! + 2 * shell.borderWidth! + Math.max(textHeight, target);
      expect(height).toBeGreaterThanOrEqual(64); expect(height).toBeLessThanOrEqual(76);
      // 669pt list viewport under the context row and above the tab bar on the 390x844 test surface.
      expect(Math.floor(669 / (height + shell.marginBottom!))).toBeGreaterThanOrEqual(8);
      if (item.kind === "task" ? item.node.jira : true) {
        expect(issue).toHaveLength(1);
        // A real 44pt target, since the web ignores a hit slop; it fits beside the two text lines, so the row keeps its height.
        const style = issue[0]!.props.style as { minHeight: number; minWidth: number };
        expect([style.minHeight, style.minWidth, issue[0]!.props.hitSlop]).toEqual([44, 44, undefined]);
        expect(target).toBeLessThanOrEqual(textHeight);
        // In its own column beside the text: not in the meta line, not over the title, and never inside the row button.
        expect(elements(metaRow.props.children as ReactNode).includes(issue[0]!)).toBe(false);
        const columns = tree.find((element) => element.type === "View" && (element.props.style as { flexDirection?: string } | undefined)?.flexDirection === "row" && (element.props.children as ReactNode[]).includes(issue[0]!))!;
        const [textColumn] = columns.props.children as Element[];
        expect(textColumn).toBe(column);
        expect(elements(textColumn).includes(title) && elements(textColumn).includes(metaRow)).toBe(true);
        expect(elements(textColumn).includes(issue[0]!)).toBe(false);
      }
    }
    const wide = mount({ selectedId: "plan", reads: [plan, jira], compact: false });
    expect(actions(wide.row(wide.data[1]!)).map((button) => [button.props.label, button.props.small])).toEqual([["Open in plan", true], ["Open issue", true]]);
  });

  it("keeps the dense 22pt issue action level with the meta line, beside the row button, for a mouse or trackpad", async () => {
    vi.resetModules();
    vi.stubGlobal("matchMedia", (query: string) => ({ matches: query !== "(pointer: coarse)" }));
    try {
      const { GraphPage: FinePage } = await import("../client/graph-page");
      const { data, row, onIssue, onPlan } = mount({ selectedId: "plan", reads: [plan, jira], page: FinePage });
      const rendered = row(data[1]!);
      const tree = elements(card(rendered));
      const issue = tree.filter((element) => element.type === "Pressable" && String(element.props.accessibilityLabel).startsWith("Open issue"));
      expect(issue).toHaveLength(1);
      const style = issue[0]!.props.style as { minHeight: number; minWidth: number };
      expect([style.minHeight, style.minWidth, issue[0]!.props.hitSlop]).toEqual([22, 44, undefined]);
      const meta = tree.find((element) => element.type === "Text" && element.props.numberOfLines === 1 && (element.props.style as { flex?: number }).flex === 1)!;
      const metaRow = tree.find((element) => element.type === "View" && [element.props.children].flat(Infinity).includes(meta))!;
      // A sibling of the row button, bottom-aligned so it sits on the meta line's 22pt band; a button never nests in a button.
      const column = tree.find((element) => element.props.accessible === true)!;
      expect(elements(column).includes(issue[0]!)).toBe(false);
      // A mouse keeps the dense row: the row button has no touch minimum.
      expect((column.props.style as { minHeight: number }).minHeight).toBe(0);
      const columns = tree.find((element) => element.type === "View" && (element.props.style as { flexDirection?: string } | undefined)?.flexDirection === "row" && (element.props.children as ReactNode[]).includes(issue[0]!))!;
      expect((columns.props.style as { alignItems: string }).alignItems).toBe("flex-end");
      expect((metaRow.props.style as { minHeight: number }).minHeight).toBe(style.minHeight);
      expect((columns.props.children as Element[])[0]).toBe(column);
      (issue[0]!.props.onPress as () => void)();
      (outer(rendered).props.onPress as () => void)();
      expect([onIssue.mock.calls, onPlan.mock.calls]).toEqual([[[graph.nodes[0]!.jira]], [["plan", graph.nodes[0]!.id]]]);
    } finally {
      vi.unstubAllGlobals(); vi.resetModules();
    }
  });

  it("formats capture days from the capture's own calendar date and leaves unparseable values as captured", () => {
    expect(captureDay("2026-10-18T17:00+00:00")).toBe("18 Oct");
    expect(captureDay("2026-01-05T23:30-08:00")).toBe("5 Jan");
    expect(captureDay("2026-13-05")).toBe("2026-13-05");
    expect(captureDay("yesterday")).toBe("yesterday");
  });
});

describe("Board and unavailable Graph disclosures (A11Y-01)", () => {
  it("keeps Source details hidden until its expanded toggle is pressed", () => {
    const closed = mount({ selectedId: "plan", reads: [plan, jira] });
    const toggle = actions(closed.header).find((button) => button.props.label === "Source details")!;
    expect(toggle.props.expanded).toBe(false);
    expect(elements(closed.header).some((element) => element.type === SourceDetails)).toBe(false);
    (toggle.props.onPress as () => void)();
    const opened = mount({ selectedId: "plan", reads: [plan, jira] });
    expect(actions(opened.header).find((button) => button.props.label === "Source details")!.props.expanded).toBe(true);
    expect(elements(opened.header).filter((element) => element.type === SourceDetails).map((element) => (element.props.read as SourceRead).source.id)).toEqual(["plan", "jira"]);
  });

  it("keeps Graph explicitly unavailable without a canonical graph and offers captured tasks on Board", () => {
    const onBoard = vi.fn();
    const { tree } = mount({ mode: "Graph", epicKey: "ASMA-8371", onBoard });
    expect(elements(tree).some((element) => element.type === PlanGraph)).toBe(false);
    expect(text(tree)).toContain("Dependency graph unavailable");
    expect(text(tree)).toContain("Captured Jira tasks remain available on Board.");
    (actions(tree).find((button) => button.props.label === "Inspect captured tasks")!.props.onPress as () => void)();
    expect(onBoard).toHaveBeenCalledTimes(1);
    expect(actions(mount({ mode: "Graph", epicKey: null, onBoard }).tree).some((button) => button.props.label === "Inspect captured tasks")).toBe(false);
  });
});

describe("Board linked sessions", () => {
  const sessionsOf = (entries: DirectoryEntry[], extra: Partial<BoardSessions> = {}, live = true): BoardSessions => ({
    state: { ...emptyDirectoryState(), entries, status: "ready", live, inventoryComplete: true, lastUpdatedAt: 1 },
    onOpen: vi.fn(), onOpenWorkspace: vi.fn(), onShowInSessions: vi.fn(), ...extra,
  });
  const entries = [
    sessionEntry("swe", { status: "running", workspaceId: "wks_a", workspaceName: "tsw-a", labels: { "jira.issue": graph.nodes[0]!.jira!, role: "SWE" } }),
    sessionEntry("qa", { status: "idle", needsInput: true, workspaceId: "wks_b", workspaceName: "tsw-b", labels: { "jira.issue": graph.nodes[0]!.jira!, role: "QA" } }),
    sessionEntry("one", { labels: { task: graph.nodes[1]!.jira!, role: "SWE" } }),
    sessionEntry("titled", { title: `SWE • ${graph.nodes[2]!.jira}`, labels: { epic: graph.nodes[2]!.jira! } }),
  ];
  const control = (rendered: Element) => elements(rendered).find((element) => element.type === TaskSessionsControl);
  const expandControl = (element: Element) => (TaskSessionsControl as (props: Record<string, unknown>) => Element)(element.props);

  it("joins rows to sessions by exact label only: nothing for 0, Open session for 1, a chooser for several", () => {
    const sessions = sessionsOf(entries);
    const { data, row, header } = mount({ selectedId: "plan", reads: [plan, jira], sessions });
    expect(text(elements(header).find((element) => element.props.testID === "board-sessions-meta"))).toMatch(/^2 of \d+ tasks with sessions$/);
    const [first, second, third] = taskRows(data);
    expect(control(row(third!))).toBeUndefined();
    const single = expandControl(control(row(second!))!);
    expect(String(single.props.accessibilityLabel)).toMatch(/^Open linked session Session one\. 1 linked session\. SWE: Idle\. Roles come from session labels; no handoff order is declared\.$/);
    expect(text(single)).toContain("Open session ›");
    (single.props.onPress as () => void)();
    expect(sessions.onOpen).toHaveBeenCalledWith(entries[2]);
    const several = expandControl(control(row(first!))!);
    expect(String(several.props.accessibilityLabel)).toContain("2 linked sessions. SWE: Running; QA: Needs input.");
    expect(text(several)).toContain("2 sessions ›");
    (several.props.onPress as () => void)();
    expect(sessions.onOpen).toHaveBeenCalledTimes(1);
    // The setter stub stores the chosen task id; the next render draws the chooser for it.
    const reopened = mount({ selectedId: "plan", reads: [plan, jira], sessions });
    const chooser = elements(reopened.tree).find((element) => element.type === TaskSessionsChooser)!;
    expect([chooser.props.taskKey, (chooser.props.links as unknown[]).length]).toEqual([graph.nodes[0]!.jira, 2]);
    expect(elements(mount({ selectedId: "plan", reads: [plan, jira] }).header).some((element) => element.props.testID === "board-sessions-meta")).toBe(false);
  });

  it("says linked sessions are unknown, not absent, when catalog rows carry no labels", () => {
    const base = sessionsOf(entries);
    const sessions: BoardSessions = {
      ...base,
      state: {
        ...base.state,
        source: "catalog",
        labelsComplete: false,
        live: false,
        status: "ready",
        inventoryComplete: true,
      },
    };
    const { data, row, header } = mount({ selectedId: "plan", reads: [plan, jira], sessions });
    const meta = elements(header).find((element) => element.props.testID === "board-sessions-meta")!;
    expect(text(meta)).toContain("Linked sessions unknown");
    expect(text(meta)).not.toContain("No task has a linked session");
    expect(text(header)).not.toContain("No task has a linked session");
    expect(control(row(taskRows(data)[0]!))).toBeUndefined();
  });

  it("adds one chain line to a linked compact row, a real 44pt target on touch, and nothing to an unlinked row", () => {
    const { data, row } = mount({ selectedId: "plan", reads: [plan, jira], sessions: sessionsOf(entries) });
    const linked = card(row(taskRows(data)[0]!));
    const children = [linked.props.children].flat() as Element[];
    expect(children.at(-1)!.type).toBe(TaskSessionsControl);
    const style = (expandControl(children.at(-1)!).props.style as (state: object) => { minHeight: number })({ pressed: false });
    expect(style.minHeight).toBe(44);
    expect(([card(row(taskRows(data)[2]!)).props.children].flat() as unknown[]).at(-1)).toBeNull();
  });

  it("says last known when the host's live session feed is not attached", () => {
    const { header, data, row } = mount({ selectedId: "plan", reads: [plan, jira], sessions: sessionsOf(entries, {}, false) });
    expect(text(elements(header).find((element) => element.props.testID === "board-sessions-meta"))).toMatch(/ · last known$/);
    const chain = expandControl(control(row(taskRows(data)[0]!))!);
    const words = elements(chain).find((element) => typeof element.type === "function" && element.type.name === "ChainText")!;
    expect(inlineText(words)).toMatch(/^Last known {2}SWE ●/);
    expect(String(chain.props.accessibilityLabel)).toContain("2 linked sessions, last known states.");
  });

  it("lists the chooser by workspace with state words, the read's limits and Show in Sessions", () => {
    const sessions = sessionsOf(entries);
    const onClose = vi.fn();
    const task = { jira: graph.nodes[0]!.jira, kontorId: null, label: graph.nodes[0]!.jira! };
    const props = { taskKey: task.label, task, links: taskSessions(entries, task), live: true, archivedIncluded: false, anchor: { current: null }, root: { current: null }, rootSize: { width: 390, height: 700 }, compact: true, colors, sessions, onClose };
    const menu = (TaskSessionsChooser as (props: Record<string, unknown>) => Element)(props);
    // How the links were read sits in a collapsed Source details, not in the chooser's own lines.
    const details = elements(menu).find((element) => element.type === Disclosure)!;
    expect(details.props.label).toBe("Source details");
    expect(text(details.props.children as ReactNode)).toBe(`Linked by an exact jira.issue label equal to ${task.label}. Titles are not read.Archived sessions are not read.`);
    expect(text(menu).replace(text(details.props.children as ReactNode), "")).not.toMatch(/jira\.issue|exact|label/);
    const items = () => elements(menu).filter((element) => element.type === MenuItem);
    expect(items().map((item) => [item.props.label, item.props.detail])).toEqual([
      ["Open workspace", undefined], ["Session swe", "SWE · Running"], ["Open workspace", undefined], ["Session qa", "QA · Needs input"], ["Show in Sessions", undefined],
    ]);
    (items()[0]!.props.onPress as () => void)();
    expect(sessions.onOpenWorkspace).toHaveBeenCalledWith("wks_a");
    (items()[3]!.props.onPress as () => void)();
    expect(sessions.onOpen).toHaveBeenCalledWith(entries[1]);
    (items()[4]!.props.onPress as () => void)();
    expect(sessions.onShowInSessions).toHaveBeenCalledWith(task);
    expect(onClose).toHaveBeenCalledTimes(3);
    const withoutWorkspaces = (TaskSessionsChooser as (props: Record<string, unknown>) => Element)({ ...props, sessions: { ...sessions, onOpenWorkspace: undefined } });
    expect(elements(withoutWorkspaces).filter((element) => element.type === MenuItem).map((item) => item.props.label)).not.toContain("Open workspace");
  });

  it("puts a replaced or archived session under its own heading, apart from the current ones", () => {
    const key = graph.nodes[0]!.jira!;
    const history = [
      sessionEntry("first", { status: "error", labels: { "jira.issue": key, role: "SWE" } }),
      sessionEntry("second", { status: "running", labels: { "jira.issue": key, role: "SWE", "takeover.predecessor": "first" } }),
    ];
    const task = { jira: key, kontorId: null, label: key };
    const menu = (TaskSessionsChooser as (props: Record<string, unknown>) => Element)({ taskKey: key, task, links: taskSessions(history, task), live: true, archivedIncluded: true, anchor: { current: null }, root: { current: null }, rootSize: { width: 1440, height: 900 }, compact: false, colors, sessions: sessionsOf(history), onClose: vi.fn() });
    expect(text(menu)).toContain("Replaced or archived");
    expect(elements(menu).filter((element) => element.type === MenuItem).map((item) => item.props.label)).toEqual(["Open workspace", "Session second", "Session first", "Show in Sessions"]);
  });
});

describe("Board declared teams", () => {
  const key = (index: number) => graph.nodes[index]!.jira!;
  const team: TaskTeamDeclaration = { source: "Team fixture", risk: "High-stakes", workflow: "Delivery", team: "Task team", roles: ["SA", "SWE", "QA", "AUD"],
    handoffs: [{ from: "SA", to: "SWE" }, { from: "SWE", to: "QA" }, { from: "SWE", to: "AUD" }, { from: "QA", to: "SWE" }] };
  const coverage = "No configured source assigns a team, workflow or risk tier to a task (2 read).";
  const reader = (declared: Readonly<Record<string, TaskTeamDeclaration>>): TaskTeamReader => (task) => {
    const declaration = task.jira ? declared[task.jira] : undefined;
    return declaration ? { state: "declared", declaration } : { state: "not-provided", coverage };
  };
  const sessionsOf = (entries: DirectoryEntry[]): BoardSessions => ({
    state: { ...emptyDirectoryState(), entries, status: "ready", live: true, inventoryComplete: true, lastUpdatedAt: 1 },
    onOpen: vi.fn(), onOpenWorkspace: vi.fn(), onShowInSessions: vi.fn(),
  });
  const entries = () => [
    sessionEntry("swe", { status: "running", labels: { "jira.issue": key(0), role: "swe" } }),
    sessionEntry("qa", { status: "idle", needsInput: true, labels: { "jira.issue": key(0), "kontor.role": "QA" } }),
    sessionEntry("rev", { status: "idle", labels: { "jira.issue": key(0), role: "rev-a" } }),
    sessionEntry("one", { status: "idle", labels: { task: key(1), role: "SWE" } }),
  ];
  const control = (rendered: Element) => elements(rendered).find((element) => element.type === TaskSessionsControl);
  const expandControl = (element: Element) => (TaskSessionsControl as (props: Record<string, unknown>) => Element)(element.props);
  const rowOf = (data: BoardItem[], index: number) => taskRows(data).find((item) => item.node.jira === key(index))!;
  const chain = (rendered: Element) => inlineText(elements(rendered).find((element) => typeof element.type === "function" && element.type.name === "TeamChainText"));

  it("shows a declared team on its task, with or without sessions: risk and workflow, then roles in declared steps with parallel roles together", () => {
    const declared = { [key(0)]: team, [key(1)]: team, [key(2)]: team };
    const { data, row, header } = mount({ selectedId: "plan", reads: [plan, jira], sessions: sessionsOf(entries()), taskTeams: reader(declared) });
    expect(text(elements(header).find((element) => element.props.testID === "board-sessions-meta"))).toMatch(/^2 of \d+ tasks with sessions · 3 with a declared team$/);
    const [first, third] = [0, 2].map((index) => expandControl(control(row(rowOf(data, index)))!));
    expect(text(elements(first!).find((element) => String(element.props.testID).endsWith("-team")))).toBe("High-stakes · Delivery");
    // A rework handoff (QA back to SWE) does not reorder the steps; a role nobody is labelled with shows a dash.
    expect(chain(first!)).toBe("SA –  →  SWE ●  →  QA ▲ & AUD –  ·  REV A ○");
    expect(String(first!.props.accessibilityLabel)).toBe("Choose a linked session. 3 linked sessions. High-stakes · Delivery. Team Task team. SA: No session, then SWE: Running, then QA and AUD in parallel: QA: Needs input; AUD: No session. Not in the declared team: REV A: Idle. Sessions sit under a role by their role label.");
    expect(chain(third!)).toBe("SA –  →  SWE –  →  QA – & AUD –");
    expect(text(third!)).toContain("0 sessions ›");
  });

  it("opens the chooser for a declared team even with one session, and opens one session directly where no team is declared", () => {
    const sessions = sessionsOf(entries());
    const declaredRow = mount({ selectedId: "plan", reads: [plan, jira], sessions, taskTeams: reader({ [key(1)]: team }) });
    const declared = expandControl(control(declaredRow.row(rowOf(declaredRow.data, 1)))!);
    expect(text(declared)).toContain("1 session ›");
    (declared.props.onPress as () => void)();
    expect(sessions.onOpen).not.toHaveBeenCalled();
    expect(model.evidence).toBe(`task:${graph.nodes[1]!.id}`);
    model.evidence = false;
    const plainRow = mount({ selectedId: "plan", reads: [plan, jira], sessions, taskTeams: reader({}) });
    const plain = expandControl(control(plainRow.row(rowOf(plainRow.data, 1)))!);
    expect(text(plain)).toContain("Open session ›");
    (plain.props.onPress as () => void)();
    expect(sessions.onOpen).toHaveBeenCalledWith(sessions.state.entries[3]);
    expect(control(plainRow.row(rowOf(plainRow.data, 2)))).toBeUndefined();
  });

  it("keeps partial declarations honest: the missing parts are named and roles fall back to labels", () => {
    const partial: TaskTeamDeclaration = { source: "Team fixture", risk: "Standard", workflow: null, team: null, roles: [], handoffs: [] };
    const { data, row, header } = mount({ selectedId: "plan", reads: [plan, jira], sessions: sessionsOf(entries()), taskTeams: reader({ [key(0)]: partial, [key(1)]: { ...partial, roles: ["SWE"] }, [key(2)]: { ...partial, team: "Named team" } }) });
    // A risk tier alone is not a team; a named team or declared roles are.
    expect(text(elements(header).find((element) => element.props.testID === "board-sessions-meta"))).toMatch(/^2 of \d+ tasks with sessions · 2 with a declared team$/);
    const line = expandControl(control(row(rowOf(data, 0)))!);
    expect(text(elements(line).find((element) => String(element.props.testID).endsWith("-team")))).toBe("Standard · workflow not declared");
    expect(inlineText(elements(line).find((element) => typeof element.type === "function" && element.type.name === "ChainText"))).toBe("SWE ●  ·  QA ▲  ·  REV A ○");
  });

  it("puts how sessions and teams were read in Board Source details, not in its meta line", () => {
    model.evidence = true;
    const notes = (taskTeams: TaskTeamReader) => {
      const { header } = mount({ selectedId: "plan", reads: [plan, jira], sessions: sessionsOf(entries()), taskTeams });
      return text(elements(header).find((element) => element.props.testID === "board-source-notes"));
    };
    expect(notes(reader({}))).toContain("Linked sessions: a session links to a task only when its jira.issue, task or asma.task label equals the task's Jira key, or its kontor.ticket label equals its Kontor ticket.");
    expect(notes(reader({}))).toContain(coverage);
    expect(notes(reader({ [key(0)]: team }))).toMatch(/Team, workflow and risk tier: declared for 1 of 41 tasks by Team fixture\.$/);
  });

  it("leads the chooser with the team, or says it is not provided, and names the declaring source only in Source details", () => {
    const sessions = sessionsOf(entries());
    const task = { jira: key(0), kontorId: null, label: key(0) };
    const links = taskSessions(sessions.state.entries, task);
    const props = { taskKey: task.label, task, links, live: true, archivedIncluded: false, anchor: { current: null }, root: { current: null }, rootSize: { width: 390, height: 700 }, compact: true, colors, sessions, onClose: vi.fn() };
    const open = (read: Record<string, unknown>) => (TaskSessionsChooser as (props: Record<string, unknown>) => Element)({ ...props, ...read });
    const declared = open({ team: { state: "declared", declaration: team } });
    const block = elements(declared).find((element) => typeof element.type === "function" && element.type.name === "TeamBlock")!;
    expect(inlineText(block)).toBe("High-stakes · DeliveryTeam Task teamSA –  →  SWE ●  →  QA ▲ & AUD –  ·  REV A ○");
    const details = (menu: Element) => text(elements(menu).find((element) => element.type === Disclosure)!.props.children as ReactNode);
    expect(details(declared)).toContain("Team, workflow and risk tier declared by Team fixture. A session sits under a role by its own role label, which assigns nothing.");
    const missing = open({ team: { state: "not-provided", coverage } });
    expect(inlineText(elements(missing).find((element) => typeof element.type === "function" && element.type.name === "TeamBlock"))).toBe("Team, workflow and risk tier: not provided");
    expect(details(missing)).toContain(coverage);
    const none = open({ links: [], team: { state: "declared", declaration: team } });
    expect(text(none)).toContain(`${key(0)} · no linked session`);
    expect(details(none)).toContain(`No session has a jira.issue, task or kontor.ticket label equal to ${key(0)}.`);
  });
});
