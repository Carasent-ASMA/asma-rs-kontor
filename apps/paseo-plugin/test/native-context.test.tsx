import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ Text: "Text", View: "View" }));
import { nativeFreshnessLabel } from "../client/native-freshness";
import { contextPlanSources, INITIAL_ROUTE, resolveContextPlan, navigate, goBack } from "../client/navigation-state";
import type { ReadSourceSummary } from "../shared/read-sources";
const source: ReadSourceSummary = { id: "exact-source", title: "Different title", kind: "plan", path: "/configured/plan.md", projectId: null, epic: "ASMA-8539", workstream: null, revisionKind: "working copy" };
describe("explicit epic context selection", () => {
  it.each(["Plan", "Graph", "Board"] as const)("selects one explicit matching source for epic → %s and preserves Back", (tab) => {
    const route = { ...INITIAL_ROUTE, tab, epicKey: "ASMA-8539" };
    const next = resolveContextPlan(route, [source], {});
    expect(next.sourceId).toBe(source.id);
    navigate(`context-${tab}`, { tab: "Overview", epicKey: "ASMA-8539" }); navigate(`context-${tab}`, next);
    expect(goBack(`context-${tab}`).tab).toBe("Overview");
  });
  it("retains an explicitly chosen source across multiple candidates and leaves ambiguity to the picker", () => {
    const plans = [source, { ...source, id: "second" }];
    const route = { ...INITIAL_ROUTE, tab: "Graph" as const, epicKey: "ASMA-8539" };
    expect(resolveContextPlan(route, plans, {}).sourceId).toBeNull();
    expect(contextPlanSources(route, plans, {}).map((value) => value.id)).toEqual(["exact-source", "second"]);
    expect(resolveContextPlan({ ...route, sourceId: "second" }, plans, {}).sourceId).toBe("second");
  });
  it("never infers title links or crosses a project/conversation association boundary", () => {
    const route = { ...INITIAL_ROUTE, tab: "Plan" as const, epicKey: "ASMA-8539" };
    expect(resolveContextPlan(route, [{ ...source, epic: null, title: "ASMA-8539" }], {}).sourceId).toBeNull();
    expect(resolveContextPlan({ ...route, projectId: "other" }, [source], {}).sourceId).toBeNull();
    expect(resolveContextPlan(route, [source], {}, []).sourceId).toBeNull();
    expect(resolveContextPlan({ ...route, epicKey: null }, [source], {}).sourceId).toBeNull();
  });
});
describe("shared native freshness truth", () => {
  it("requires a reconciled live directory, rather than just an online socket", () => {
    expect(nativeFreshnessLabel({ hostOnline: true, status: "ready", live: false })).toBe("Stale");
    expect(nativeFreshnessLabel({ hostOnline: true, status: "loading", live: false })).toBe("Reading");
    expect(nativeFreshnessLabel({ hostOnline: false, status: "ready", live: true })).toBe("Host offline");
    expect(nativeFreshnessLabel({ hostOnline: true, status: "error", live: false })).toBe("Unavailable");
    expect(nativeFreshnessLabel({ hostOnline: true, status: "ready", live: true })).toBe("Live");
  });
});
