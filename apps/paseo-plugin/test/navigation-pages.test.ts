import { describe, expect, it } from "vitest";
import { goBack, hostNavigation, linkedPlans, navigate, readRouteView, refreshSourceRevision, sessionScopeKey, writeRouteView } from "../client/navigation-state";
import { inboxEntries, jiraIssueUrl, nativeSummary, overviewRows, searchRecords } from "../client/page-data";
import { reorderedSiblings } from "../client/organization-order";
import { sessionEntry } from "./session-fixtures";
import { patchSessionView, readSessionView } from "../client/session-view-store";

describe("read-only navigation and data truth", () => {
  it("keeps per-host context and back routes; Overview is always global", () => {
    navigate("one", { projectId: "project", tab: "Plan", sourceId: "source" });
    navigate("one", { tab: "Overview" });
    expect(hostNavigation("one").route.projectId).toBeNull();
    expect(goBack("one")).toMatchObject({ projectId: "project", tab: "Plan", sourceId: "source" });
    expect(hostNavigation("two").route.projectId).toBeNull();
  });
  it("uses explicit native conversation/source associations, including drafts without an epic", () => {
    expect(linkedPlans({ native: ["draft", "draft", "deleted"] }, "native", new Set(["draft"]))).toEqual(["draft"]);
    expect(linkedPlans({}, "ASMA-1 in a title", new Set(["draft"]))).toEqual([]);
  });
  it("restores each history entry's query, position and session filters", () => {
    const first = navigate("history", { tab: "Plan", sourceId: "one" });
    writeRouteView("history", first, { query: "first query", offset: 120, documentOffset: 900 });
    const sessionKey = sessionScopeKey("history", first);
    patchSessionView(sessionKey, { attentionOnly: true, favoritesOnly: true, scrollOffset: 500 });
    navigate("history", { sourceId: "two" });
    writeRouteView("history", hostNavigation("history").route, { query: "second query" });
    patchSessionView(sessionKey, { attentionOnly: false, favoritesOnly: false, scrollOffset: 0 });
    goBack("history");
    expect(hostNavigation("history").route.sourceId).toBe("one");
    expect(readRouteView("history")).toEqual({ query: "first query", offset: 120, documentOffset: 900 });
    expect(readSessionView(sessionKey)).toMatchObject({ attentionOnly: true, favoritesOnly: true, scrollOffset: 500 });
  });
  it("isolates query and offset by project, source and exact revision", () => {
    const first = navigate("isolation", { tab: "Plan", projectId: "A", sourceId: "plan-A", sourceRevision: "sha-A" });
    writeRouteView("isolation", first, { query: "plan A text", offset: 700, documentOffset: 900 });
    navigate("isolation", { projectId: "B", sourceId: "plan-B", sourceRevision: "sha-B" });
    expect(readRouteView("isolation")).toEqual({ query: "", offset: 0, documentOffset: 0 });
    navigate("isolation", { projectId: "B", sourceId: "plan-A", sourceRevision: "sha-A" });
    expect(readRouteView("isolation")).toEqual({ query: "", offset: 0, documentOffset: 0 });
    navigate("isolation", { projectId: "A", sourceId: "plan-A", sourceRevision: "sha-A" });
    expect(readRouteView("isolation")).toEqual({ query: "plan A text", offset: 700, documentOffset: 900 });
    refreshSourceRevision("isolation", "sha-A-new");
    expect(readRouteView("isolation")).toEqual({ query: "", offset: 0, documentOffset: 0 });
    expect(readRouteView("different-host", first)).toEqual({ query: "", offset: 0, documentOffset: 0 });
  });
  it("keeps idle/attention separate from completion and governance; only structured confirmed keys form rows", () => {
    const entries = [sessionEntry("one", { status: "idle", requiresAttention: true, labels: { epic: "ASMA-1" } }), sessionEntry("two", { title: "ASMA-1", labels: { epic: "unconfirmed" } })];
    expect(nativeSummary(entries)).toMatchObject({ idle: 2, attention: 1 });
    expect(overviewRows(entries, []).map((row) => row.key)).toEqual(["ASMA-1", "Unassigned / ad-hoc"]);
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
