import { describe, expect, it } from "vitest";
import { goBack, hostNavigation, linkedPlans, navigate, readRouteView, refreshSourceRevision, sessionScopeKey, writeRouteView } from "../client/navigation-state";
import { inboxEntries, jiraIssueUrl, nativeSummary, portfolioFilter, portfolioRows, searchRecords, recordedDecisions, type PortfolioFilter } from "../client/page-data";
import { jiraHistorySchema } from "../shared/read-sources";
import type { SourceRead } from "../shared/read-sources";
import { reorderedSiblings } from "../client/organization-order";
import { sessionEntry } from "./session-fixtures";
import { patchSessionView, readSessionView } from "../client/session-view-store";

const keys = (rows: ReturnType<typeof portfolioRows>, filter: PortfolioFilter) => rows.filter((row) => portfolioFilter(row, filter)).map((row) => row.id);
describe("read-only navigation and data truth", () => {
  it("filters existing attention observations and keeps Unknown decision coverage distinct from captured zero", () => {
    const entries = [sessionEntry("permission", { labels: { epic: "ASMA-1" }, attentionReason: "permission", requiresAttention: false }), sessionEntry("quiet", { labels: { epic: "ASMA-2" } })];
    const empty: SourceRead = { source: { id: "decisions", title: "Captured decisions", kind: "decisions", path: "/configured/decisions.json", epic: "ASMA-1", projectId: null, workstream: null, revisionKind: "working copy" }, status: "ready", readAt: "now", modifiedAt: null, revision: "hash", content: null, graph: null, records: { teams: [], decisions: [], delivery: [] }, diagnostics: [] };
    expect(keys(portfolioRows(entries, [], true), "review")).toEqual(["ASMA-1"]);
    expect(keys(portfolioRows(entries, [], true), "active")).toEqual([]);
    expect(recordedDecisions([empty])).toMatchObject({ open: 0, complete: true });
    expect(recordedDecisions([])).toMatchObject({ open: null, complete: false });
    // A captured zero leaves the filter; an epic without a decision source stays as Unknown.
    expect(keys(portfolioRows(entries, [empty], true), "decisions")).toEqual(["ASMA-2"]);
    const open = { ...empty, records: { teams: [], delivery: [], decisions: [{ id: "waiting", title: "Recorded open decision", state: "open" as const }] } };
    expect(portfolioRows(entries, [open], true).map((row) => row.decisions)).toEqual([1, null]);
    expect(keys(portfolioRows(entries, [open], true), "decisions")).toEqual(["ASMA-1", "ASMA-2"]);
    expect(portfolioRows(entries, [{ ...empty, status: "unavailable" }], true)[0]!.decisions).toBeNull();
    // Delivery confirmation is never inferred, so every epic remains in the Unknown group.
    expect(keys(portfolioRows(entries, [empty], true), "delivered")).toEqual(["ASMA-1", "ASMA-2"]);
  });
  it("builds portfolio rows from exact plans and dated historical Jira, excluding Won't Do from progress", () => {
    const history = jiraHistorySchema.parse({ schema: "consumption/data@2", generated: "2026-10-18T17:00+00:00", provenance: { path: "/capture.json", sha256: "a".repeat(64), capturedEpics: 3 }, selection: ["ASMA-1", "ASMA-3"],
      epics: [{ key: "ASMA-1", status: "Done", cat: "done", children: [{ key: "ASMA-11", status: "Closed", cat: "done" }, { key: "ASMA-12", status: "In Development", cat: "indeterminate" }, { key: "ASMA-13", status: "Won't Do", cat: "done" }] },
        { key: "ASMA-3", status: "Groomed", cat: "new", children: [] }] });
    const jira: SourceRead = { source: { id: "jira", title: "Historical Jira", kind: "jira", path: "/jira.json", epic: null, projectId: null, workstream: null, revisionKind: "working copy" }, status: "ready", readAt: "now", modifiedAt: null, revision: "hash", content: null, graph: null, records: null, diagnostics: [], jiraHistory: history };
    const plan: SourceRead = { ...jira, source: { ...jira.source, id: "plan", title: "Exact plan", kind: "plan", epic: "ASMA-1" }, content: "# Plan", jiraHistory: null };
    const draft: SourceRead = { ...plan, source: { ...plan.source, id: "draft", title: "Draft without epic", epic: null } };
    const entries = [sessionEntry("run", { status: "running", labels: { epic: "ASMA-1" } }), sessionEntry("loose")];
    const rows = portfolioRows(entries, [jira, plan, draft], true);
    expect(rows.map((row) => [row.id, row.title])).toEqual([["ASMA-1", "ASMA-1 · Exact plan"], ["ASMA-3", "ASMA-3"], ["plan:draft", "Draft without epic"]]);
    expect(rows[0]).toMatchObject({ lifecycle: { status: "Done", done: true, observedAt: "2026-10-18T17:00+00:00" }, tasks: { total: 2, done: 1, excluded: 1 }, sessions: { total: 1, running: 1 } });
    expect(rows[1]).toMatchObject({ lifecycle: { done: false }, tasks: { total: 0, done: 0 }, plans: [] });
    expect(keys(rows, "completed")).toEqual(["ASMA-1"]);
    expect(keys(rows, "active")).toEqual(["ASMA-1"]);
    // A scoped portfolio does not add unrelated epics from the historical selection.
    expect(portfolioRows(entries, [jira, plan], false).map((row) => row.id)).toEqual(["ASMA-1"]);
    expect(portfolioRows([], [jira], false)).toEqual([]);
  });
  it("keeps host-scoped context through Overview and supports explicit All projects clearing", () => {
    navigate("one", { projectId: "project", tab: "Plan", sourceId: "source" });
    navigate("one", { tab: "Overview" });
    expect(hostNavigation("one").route).toMatchObject({ projectId: "project", sourceId: "source", tab: "Overview" });
    navigate("one", { projectId: null, epicKey: null, sourceId: null });
    expect(hostNavigation("one").route.projectId).toBeNull();
    goBack("one");
    expect(goBack("one")).toMatchObject({ projectId: "project", tab: "Plan", sourceId: "source" });
    expect(hostNavigation("two").route.projectId).toBeNull();
  });
  it("uses explicit native conversation/source associations, including drafts without an epic", () => {
    expect(linkedPlans({ native: ["draft", "draft", "deleted"] }, "native", new Set(["draft"]))).toEqual(["draft"]);
    expect(linkedPlans({}, "ASMA-1 in a title", new Set(["draft"]))).toEqual([]);
  });
  it("restores each history entry's query, position and session filters", () => {
    const first = navigate("history", { tab: "Plan", sourceId: "one" });
    writeRouteView("history", first, { query: "first query", offset: 120 });
    const sessionKey = sessionScopeKey("history", first);
    patchSessionView(sessionKey, { status: "attention", shortcut: "Adam", scrollOffset: 500 });
    navigate("history", { sourceId: "two" });
    writeRouteView("history", hostNavigation("history").route, { query: "second query" });
    patchSessionView(sessionKey, { status: "all", shortcut: null, scrollOffset: 0 });
    goBack("history");
    expect(hostNavigation("history").route.sourceId).toBe("one");
    expect(readRouteView("history")).toEqual({ query: "first query", offset: 120 });
    expect(readSessionView(sessionKey)).toMatchObject({ status: "attention", shortcut: "Adam", scrollOffset: 500 });
  });
  it("isolates query and offset by project, source and exact revision", () => {
    const first = navigate("isolation", { tab: "Plan", projectId: "A", sourceId: "plan-A", sourceRevision: "sha-A" });
    writeRouteView("isolation", first, { query: "plan A text", offset: 700 });
    navigate("isolation", { projectId: "B", sourceId: "plan-B", sourceRevision: "sha-B" });
    expect(readRouteView("isolation")).toEqual({ query: "", offset: 0 });
    navigate("isolation", { projectId: "B", sourceId: "plan-A", sourceRevision: "sha-A" });
    expect(readRouteView("isolation")).toEqual({ query: "", offset: 0 });
    navigate("isolation", { projectId: "A", sourceId: "plan-A", sourceRevision: "sha-A" });
    expect(readRouteView("isolation")).toEqual({ query: "plan A text", offset: 700 });
    refreshSourceRevision("isolation", "sha-A-new");
    expect(readRouteView("isolation")).toEqual({ query: "", offset: 0 });
    expect(readRouteView("different-host", first)).toEqual({ query: "", offset: 0 });
  });
  it("keeps idle/attention separate from completion and governance; only structured confirmed keys form rows", () => {
    const entries = [sessionEntry("one", { status: "idle", requiresAttention: true, labels: { epic: "ASMA-1" } }), sessionEntry("two", { title: "ASMA-1", labels: { epic: "unconfirmed" } })];
    expect(nativeSummary(entries)).toMatchObject({ idle: 2, attention: 1 });
    // Sessions without a confirmed epic stay in the inventory count, never as a pseudo-epic row.
    expect(portfolioRows(entries, [], true).map((row) => row.id)).toEqual(["ASMA-1"]);
    expect(keys(portfolioRows(entries, [], true), "completed")).toEqual([]);
    expect(inboxEntries(entries).map((entry) => entry.id)).toEqual(["one"]);
  });
  it("searches available labels/names, never an unprovided transcript", () => {
    const entries = [sessionEntry("one", { title: "SWE • package", labels: { ticket: "ASMA-12" } })];
    expect(searchRecords(entries, [], "ASMA-12").sessions).toHaveLength(1);
    expect(searchRecords(entries, [], "private prompt").sessions).toHaveLength(0);
  });
  it("opens only full confirmed ASMA issue keys through the Jira source link", () => {
    expect(jiraIssueUrl("ASMA-8541")).toBe("https://carasent.atlassian.net/browse/ASMA-8541");
    for (const value of ["8541", "ASMA-0", "ASMA-1/../../admin", "other-1", "ASMA-1 in prose"]) expect(jiraIssueUrl(value)).toBeNull();
  });
  it("reorders siblings without dropping rows, including removed IDs and out-of-range gestures", () => {
    expect(reorderedSiblings(["a", "b", "c"], "b", 50)).toEqual(["a", "c", "b"]);
    expect(reorderedSiblings(["a", "b", "c"], "gone", 1)).toEqual(["a", "b", "c"]);
    expect(reorderedSiblings(["a", "b", "c"], "c", -50)).toEqual(["c", "a", "b"]);
  });

});
