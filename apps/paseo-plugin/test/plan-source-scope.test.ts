import { describe, expect, it } from "vitest";
import { INITIAL_ROUTE, contextHeading, contextPlanSources, resolveContextPlan } from "../client/navigation-state";
import { confirmedEpic, portfolioRows } from "../client/page-data";
import { projectEpicKeys, sourceInProject } from "../client/source-scope";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import { sessionEntry } from "./session-fixtures";

const plan: ReadSourceSummary = { id: "plan", title: "Exact plan", kind: "plan", path: "/configured/plan.md", epic: "ASMA-1", projectId: null, workstream: null, revisionKind: "working copy" };
const read: SourceRead = { source: plan, status: "ready", revision: "a".repeat(64), readAt: "now", modifiedAt: null, content: "# Plan", graph: null, records: null, diagnostics: [] };
describe("canonical source scope", () => {
  it("finds the same plan in each native project carrying its exact confirmed epic without attributing siblings", () => {
    const entries = [sessionEntry("one", { projectKey: "a", labels: { epic: "ASMA-1" } }), sessionEntry("two", { projectKey: "b", labels: { "jira.epic": "ASMA-1" } }), sessionEntry("loose", { projectKey: "a", title: "ASMA-1" })];
    for (const projectId of ["a", "b"]) {
      const route = { ...INITIAL_ROUTE, tab: "Plan" as const, projectId, epicKey: "ASMA-1" };
      const epics = projectEpicKeys(projectId, entries);
      expect(contextPlanSources(route, [plan], { plan: read }, null, epics)).toEqual([plan]);
      expect(resolveContextPlan(route, [plan], { plan: read }, null, epics)).toMatchObject({ sourceId: "plan", sourceRevision: read.revision });
      expect(contextHeading(route, [plan], { plan: read }, null, projectId, epics).title).toBe(plan.title);
    }
    expect(portfolioRows(entries, [read], false)[0]!.sessions.total).toBe(2);
    expect(confirmedEpic(entries[2]!)).toBeNull();
    expect(projectEpicKeys("unrelated", entries)).toEqual([]);
    expect(sourceInProject(plan, read, "unrelated", [])).toBe(false);
  });
  it("preserves explicit project/association fences, multiple-plan choice, and conflicting-label unknowns", () => {
    expect(sourceInProject({ ...plan, projectId: "other" }, read, "a", ["ASMA-1"])).toBe(false);
    const route = { ...INITIAL_ROUTE, tab: "Board" as const, projectId: "a", epicKey: "ASMA-1" };
    expect(contextHeading(route, [], {}, null, "a", [], "No read sources configured").title).toBe("Plan sources unavailable");
    expect(contextHeading(route, [], {}, null, "a").title).toBe("No plan linked");
    const second = { ...plan, id: "second" };
    expect(resolveContextPlan(route, [plan, second], { plan: read }, null, ["ASMA-1"]).sourceId).toBeNull();
    expect(contextPlanSources(route, [plan, second], { plan: read }, ["second"], ["ASMA-1"])).toEqual([second]);
    expect(confirmedEpic(sessionEntry("conflict", { labels: { epic: "ASMA-1", "jira.epic": "ASMA-2" } }))).toBeNull();
    expect(projectEpicKeys("a", [sessionEntry("title", { projectKey: "a", projectName: "ASMA-1", title: "ASMA-1" })])).toEqual([]);
    for (const kind of ["delivery", "decisions"] as const) expect(sourceInProject({ ...plan, kind }, undefined, "a", ["ASMA-1"])).toBe(true);
  });
});
