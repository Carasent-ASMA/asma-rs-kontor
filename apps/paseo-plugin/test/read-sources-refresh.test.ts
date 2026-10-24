import { describe, expect, it, vi } from "vitest";
import { useReadSources, overlaySourceReads, pendingDemandIds, type FullSourceRead } from "../client/use-read-sources";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";

const model = vi.hoisted(() => ({ query: null as null | ((context: { signal: AbortSignal }) => Promise<unknown>), options: null as unknown,
  summaries: vi.fn(), list: vi.fn(), read: vi.fn(), teams: vi.fn() }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: (initial: unknown) => [typeof initial === "function" ? initial() : initial, vi.fn()],
  useRef: (current: unknown) => ({ current }), useEffect: () => {}, useMemo: (factory: () => unknown) => factory() }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (rpc: { name: string }) => rpc.name === "sources.summaries" ? model.summaries
  : rpc.name === "sources.list" ? model.list : rpc.name === "sources.read" ? model.read : model.teams }));
vi.mock("@tanstack/react-query", () => ({ useQuery: (options: { queryFn: typeof model.query }) => { model.query = options.queryFn; model.options = options;
  return { data: undefined, isError: false, error: null, isFetching: false, refetch: vi.fn() }; } }));

const planSource: ReadSourceSummary = { id: "plan", title: "Plan", kind: "plan", path: "/configured/plan.md", epic: "ASMA-1", projectId: null, workstream: null, revisionKind: "working copy" };
const deliverySource: ReadSourceSummary = { id: "delivery", title: "Delivery", kind: "delivery", path: "/configured/delivery.json", epic: null, projectId: null, workstream: null, revisionKind: "working copy" };
const authoringSource: ReadSourceSummary = { id: "authoring", title: "Authoring", kind: "teams", path: "/configured/authoring.yml", epic: null, projectId: null, workstream: null, revisionKind: "working copy" };
const summary = (source: ReadSourceSummary, revision: string): SourceRead => ({ source, status: "ready", revision, readAt: "2026-10-24T10:00:00Z",
  modifiedAt: null, content: null, graph: null, records: null, diagnostics: [] });
const plan = summary(planSource, "a");
const delivery = summary(deliverySource, "b");
const jira: SourceRead = { ...summary({ ...deliverySource, id: "jira", title: "Jira", kind: "jira", path: "/configured/jira.json" }, "c"),
  jiraHistory: { schema: "consumption/data@2", generated: "2026-10-18T10:00:00Z", provenance: { path: "/capture.json", sha256: "a".repeat(64), capturedEpics: 0 }, selection: ["ASMA-1"], epics: [] } };
const fullPlan: FullSourceRead = { hostId: "host", path: planSource.path, kind: "plan", revision: "a", read: { ...plan, content: "# Plan", graph: null, planMetadata: { workstream: "access", epic: "ASMA-1", diagnostics: [] } } };
const registry = { path: null, revision: null, workstreams: {}, desks: {}, diagnostics: [] };
const teamsA: ReadSourceSummary = { ...authoringSource, title: "Catalog A", path: "/configured/a.yml" };
const teamsB: ReadSourceSummary = { ...teamsA, title: "Catalog B", path: "/configured/b.yml" };
const teamsSnapshot = (source: ReadSourceSummary) => ({ source, status: "ready", readAt: "2026-10-24T10:00:00Z" });

describe("source summaries and demanded full reads", () => {
  it("requests summaries once, keeps Teams adapter IDs on their snapshots and never reads every source", async () => {
    const sources = [planSource, deliverySource, authoringSource];
    model.summaries.mockReset().mockResolvedValue({ sources, diagnostics: [], registry,
      reads: { plan, delivery, authoring: summary(authoringSource, "d") } });
    model.list.mockReset().mockResolvedValue({ sources, diagnostics: [], registry });
    model.teams.mockReset().mockResolvedValue(teamsSnapshot(authoringSource));
    model.read.mockReset();
    useReadSources("host", "online");
    const result = await model.query!({ signal: new AbortController().signal }) as Record<string, unknown>;
    expect(model.summaries).toHaveBeenCalledExactlyOnceWith({});
    expect(model.read).not.toHaveBeenCalled();
    expect(model.options).toMatchObject({ queryKey: ["kontor-read-source-summaries", "host"], enabled: true, refetchInterval: 30_000, retry: false, staleTime: 0 });
    expect(Object.keys(result.teamsSnapshots as object)).toEqual(["authoring"]);
    expect(result.reads).toEqual({ plan, delivery, authoring: summary(authoringSource, "d") });
    useReadSources("other", "offline");
    expect(model.options).toMatchObject({ queryKey: ["kontor-read-source-summaries", "other"], enabled: false, refetchInterval: false });
  });
  it("keeps a Teams adapter failure isolated from summary reads", async () => {
    model.summaries.mockReset().mockResolvedValue({ sources: [authoringSource], diagnostics: ["catalog note"], reads: { authoring: summary(authoringSource, "d") } });
    model.list.mockReset().mockResolvedValue({ sources: [authoringSource], diagnostics: ["catalog note"] });
    model.teams.mockReset().mockRejectedValue(new Error("authoring unavailable"));
    useReadSources("host", "online");
    const result = await model.query!({ signal: new AbortController().signal }) as Record<string, unknown>;
    expect(result.teamsSnapshots).toEqual({}); expect(result.teamsErrors).toEqual({ authoring: "authoring unavailable" });
    expect(Object.keys(result.reads as object)).toEqual(["authoring"]); expect(result.diagnostics).toEqual(["catalog note"]);
  });
  it("AUD P2-03 rejects a catalog changed between summaries and the final identity read", async () => {
    model.summaries.mockReset().mockResolvedValue({ sources: [teamsA], diagnostics: [], reads: { authoring: summary(teamsA, "a") } });
    model.teams.mockReset().mockResolvedValue(teamsSnapshot(teamsB));
    model.list.mockReset().mockResolvedValue({ sources: [teamsB], diagnostics: [] });
    useReadSources("host", "online");
    await expect(model.query!({ signal: new AbortController().signal })).rejects.toThrow("Source catalog changed during refresh");
  });
  it("AUD P2-03 does not publish a snapshot read through a replacement path under an unchanged catalog", async () => {
    model.summaries.mockReset().mockResolvedValue({ sources: [teamsA], diagnostics: [], reads: { authoring: summary(teamsA, "a") } });
    model.teams.mockReset().mockResolvedValue(teamsSnapshot(teamsB));
    model.list.mockReset().mockResolvedValue({ sources: [teamsA], diagnostics: [] });
    useReadSources("host", "online");
    const result = await model.query!({ signal: new AbortController().signal }) as Record<string, unknown>;
    expect(result.teamsSnapshots).toEqual({});
    expect(result.teamsErrors).toEqual({ authoring: "Teams source identity changed during read; refresh sources." });
    expect((result.sources as ReadSourceSummary[])[0]!.path).toBe(teamsA.path);
  });
  it("overlays a full read only at its own revision and skips Teams adapter IDs", () => {
    expect(overlaySourceReads({ plan, delivery }, {}, "host", new Set())).toEqual({ plan, delivery });
    expect(overlaySourceReads({ plan, delivery }, { plan: fullPlan }, "host", new Set())).toEqual({ plan: fullPlan.read, delivery });
    const stale = { ...fullPlan, revision: "b" };
    expect(overlaySourceReads({ plan }, { plan: stale }, "host", new Set()).plan).toBe(plan);
    const replaced = { ...fullPlan, path: "/replacement/plan.md" };
    expect(overlaySourceReads({ plan }, { plan: replaced }, "host", new Set()).plan).toBe(plan);
    expect(overlaySourceReads({ plan }, { plan: fullPlan }, "other", new Set()).plan).toBe(plan);
    expect(overlaySourceReads({ plan, authoring: summary(authoringSource, "d") }, {}, "host", new Set(["authoring"]))).toEqual({ plan });
  });
  it("demands only summary-present exact reads at the current revision", () => {
    expect(pendingDemandIds(["plan", "delivery", "jira", "authoring"], { plan, delivery, jira, authoring: summary(authoringSource, "d") }, {}, "host", new Set(["authoring"]))).toEqual(["plan", "delivery"]);
    expect(pendingDemandIds(["plan", "delivery"], { plan, delivery }, { plan: fullPlan }, "host", new Set())).toEqual(["delivery"]);
    const revisionB = { ...plan, revision: "b" };
    expect(pendingDemandIds(["plan"], { plan: revisionB }, { plan: fullPlan }, "host", new Set())).toEqual(["plan"]);
    const unavailable: SourceRead = { ...plan, status: "unavailable" };
    expect(pendingDemandIds(["plan"], { plan: unavailable }, {}, "host", new Set())).toEqual([]);
  });
});
