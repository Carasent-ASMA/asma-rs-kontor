import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ Text: "Text", View: "View" }));
import { NativeFreshnessPill, nativeFreshnessLabel } from "../client/native-freshness";
import { contextPlanSources, contextHeading, DESTINATIONS, hostNavigation, INITIAL_ROUTE, resolveContextPlan, navigate, goBack, tabScope, sessionScopeKey, readRouteView, writeRouteView, refreshSourceRevision } from "../client/navigation-state";
import { patchSessionView, readSessionView } from "../client/session-view-store";
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
  it("retains selection through every destination and restores query, offset and session filters through clear/Back without crossing hosts", () => {
    const host = "all-destination-header", other = "other-destination-header";
    const chosen = { projectId: "project", epicKey: "ASMA-8539", sourceId: source.id, sourceRevision: "revision-a", originAgentId: "agent" };
    const route = navigate(host, { tab: "Sessions", ...chosen });
    const key = sessionScopeKey(host, route);
    patchSessionView(key, { query: "retained", status: "error", includeArchived: true, scrollOffset: 180 });
    for (const tab of DESTINATIONS) {
      const next = navigate(host, { tab, ...tabScope(host, tab) });
      expect(next).toEqual({ tab, ...chosen });
    }
    const beforeClear = hostNavigation(host).route;
    writeRouteView(host, beforeClear, { query: "page query", offset: 240 });
    navigate(host, { projectId: null, epicKey: null, sourceId: null, originAgentId: null });
    expect(hostNavigation(host).route.sourceRevision).toBeNull();
    expect(goBack(host)).toEqual(beforeClear);
    expect(readRouteView(host)).toEqual({ query: "page query", offset: 240 });
    expect(readSessionView(key)).toMatchObject({ query: "retained", status: "error", includeArchived: true, scrollOffset: 180 });
    expect(hostNavigation(other).route).toEqual(INITIAL_ROUTE);
    expect(readSessionView(sessionScopeKey(other, route))).toMatchObject({ query: "", includeArchived: false });
  });
  it("shares Sessions state for same-epic plans, keeps exact page state separate and reports a removed source", () => {
    const host = "same-epic-header";
    const first = navigate(host, { tab: "Sessions", projectId: "project", epicKey: "ASMA-8539", sourceId: "one", sourceRevision: "one-a" });
    const key = sessionScopeKey(host, first);
    patchSessionView(key, { query: "same rows", status: "running", includeArchived: true, scrollOffset: 160 });
    writeRouteView(host, first, { query: "first source page", offset: 75 });
    const second = navigate(host, { sourceId: "two", sourceRevision: "two-a" });
    expect(sessionScopeKey(host, second)).toBe(key);
    expect(readSessionView(key)).toMatchObject({ query: "same rows", status: "running", includeArchived: true, scrollOffset: 160 });
    expect(readRouteView(host, second)).toEqual({ query: "", offset: 0 });
    expect(contextHeading(second, [], {}, null, "Project")).toMatchObject({ title: "Source unavailable", plan: null });
    const revised = refreshSourceRevision(host, "two-b");
    expect(sessionScopeKey(host, revised)).toBe(key);
    expect(readSessionView(key).query).toBe("same rows");
    expect(goBack(host)).toEqual(first);
    expect(readRouteView(host)).toEqual({ query: "first source page", offset: 75 });
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
  it("retains the selected host attribution with a stale inventory on an online host", () => {
    const pill = NativeFreshnessPill({ hostStatus: "online", hostLabel: "Chosen host", colors: {} as Parameters<typeof NativeFreshnessPill>[0]["colors"], state: { status: "ready", live: false, entries: [], lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false } });
    expect(pill.props.accessibilityLabel).toContain("Native sessions from Chosen host: Stale");
    expect(pill.props.accessibilityLabel).toContain("Source freshness is reported separately");
    expect(pill.props.children.props.children).toEqual(["Native · ", "Stale"]);
  });
  it("labels Live as native sessions only and keeps source freshness separate", () => {
    const pill = NativeFreshnessPill({ hostStatus: "online", colors: {} as Parameters<typeof NativeFreshnessPill>[0]["colors"], state: { status: "ready", live: true, entries: [], lastUpdatedAt: 1, error: null, staleReason: null, pages: 1, includeArchived: false } });
    expect(pill.props.accessibilityLabel).toContain("Native sessions: Live");
    expect(pill.props.accessibilityLabel).toContain("Source freshness is reported separately");
    expect(pill.props.children.props.children).toEqual(["Native · ", "Live"]);
  });
  it("labels a truncated catalog as Catalog partial and a last-known snapshot as Snapshot", () => {
    const base = { status: "stale" as const, live: false, entries: [], lastUpdatedAt: Date.parse("2026-10-19T14:00:00Z"), error: null, staleReason: "bounded", pages: 1, includeArchived: false };
    const colors = {} as Parameters<typeof NativeFreshnessPill>[0]["colors"];
    const truncated = NativeFreshnessPill({ state: { ...base, source: "catalog", inventoryComplete: false }, hostStatus: "online", colors });
    expect(truncated.props.children.props.children).toEqual(["Native · ", "Catalog partial"]);
    expect(truncated.props.accessibilityLabel).toContain("Last partial shared catalog inventory read 2026-10-19T14:00:00.000Z");
    const snapshot = NativeFreshnessPill({ state: { ...base, source: "catalog-snapshot", inventoryComplete: true }, hostStatus: "online", colors });
    expect(snapshot.props.children.props.children).toEqual(["Native · ", "Snapshot"]);
    expect(snapshot.props.accessibilityLabel).toContain("Last-known shared catalog snapshot read 2026-10-19T14:00:00.000Z");
  });
  it("keeps partial and unknown inventory evidence distinct from a complete live read", () => {
    const state = { status: "stale" as const, live: false, entries: [], lastUpdatedAt: 1, inventoryComplete: false, error: "Later page failed", staleReason: "Partial read", pages: 1, includeArchived: false };
    const pill = NativeFreshnessPill({ state, hostStatus: "online", colors: {} as Parameters<typeof NativeFreshnessPill>[0]["colors"] });
    expect(pill.props.children.props.children).toEqual(["Native · ", "Stale"]);
    expect(pill.props.accessibilityLabel).toContain("Last partial inventory read 1970-01-01T00:00:00.001Z");
    const unknown = NativeFreshnessPill({ state: { ...state, status: "loading", lastUpdatedAt: null }, hostStatus: "online", colors: {} as Parameters<typeof NativeFreshnessPill>[0]["colors"] });
    expect(unknown.props.accessibilityLabel).toContain("Inventory not read yet · time Unknown");
    expect(unknown.props.children.props.children).toEqual(["Native · ", "Reading"]);
  });
});
