import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ Text: "Text", View: "View" }));
import { NativeFreshnessPill, nativeFreshnessLabel } from "../client/native-freshness";
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
    expect(nativeFreshnessLabel({ hostStatus: "online", status: "ready", live: false })).toBe("Stale");
    expect(nativeFreshnessLabel({ hostStatus: "online", status: "loading", live: false })).toBe("Reading");
    expect(nativeFreshnessLabel({ hostStatus: "offline", status: "ready", live: true })).toBe("Host offline");
    expect(nativeFreshnessLabel({ hostStatus: "online", status: "error", live: false })).toBe("Unavailable");
    expect(nativeFreshnessLabel({ hostStatus: "online", status: "ready", live: true })).toBe("Live");
  });
  it.each([
    { hostStatus: "connecting", status: "ready", live: true, label: "Reconnecting" },
    { hostStatus: "idle", status: "ready", live: true, label: "Host idle" },
    { hostStatus: "offline", status: "ready", live: true, label: "Host offline" },
    { hostStatus: "error", status: "ready", live: true, label: "Host error" },
    { hostStatus: "online", status: "idle", live: false, label: "Reading" },
    { hostStatus: "online", status: "loading", live: true, label: "Reading" },
    { hostStatus: "online", status: "stale", live: true, label: "Stale" },
    { hostStatus: "online", status: "error", live: true, label: "Unavailable" },
    { hostStatus: "online", status: "ready", live: false, label: "Stale" },
    { hostStatus: "online", status: "ready", live: true, label: "Live" },
  ] as const)("distinguishes SDK $hostStatus and inventory $status/$live as $label", ({ label, ...input }) => {
    expect(nativeFreshnessLabel(input)).toBe(label);
  });
  it("labels Live as native sessions only and keeps source freshness separate", () => {
    const pill = NativeFreshnessPill({ hostStatus: "online", colors: {} as Parameters<typeof NativeFreshnessPill>[0]["colors"], state: { status: "ready", live: true, entries: [], lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false } });
    expect(pill.props.accessibilityLabel).toContain("Native sessions: Live");
    expect(pill.props.accessibilityLabel).toContain("Source freshness is reported separately");
    expect(pill.props.children.props.children).toEqual(["Native · ", "Live"]);
  });
});
