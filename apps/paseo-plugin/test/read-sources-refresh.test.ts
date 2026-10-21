import { describe, expect, it, vi } from "vitest";
import { useReadSources } from "../client/use-read-sources";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
const model = vi.hoisted(() => ({ query: null as null | ((context: { signal: AbortSignal }) => Promise<unknown>), list: vi.fn(), read: vi.fn() }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (rpc: { name: string }) => rpc.name === "sources.list" ? model.list : model.read }));
vi.mock("@tanstack/react-query", () => ({ useQuery: (options: { queryFn: typeof model.query }) => { model.query = options.queryFn; return { data: undefined, isError: false, error: null, isFetching: false, refetch: vi.fn() }; } }));
const source: ReadSourceSummary = { id: "plan", title: "Plan", kind: "plan", path: "/configured/plan.md", epic: "ASMA-1", projectId: null, workstream: null, revisionKind: "working copy" };
const read: SourceRead = { source, status: "partial", revision: "a", readAt: "now", modifiedAt: null, content: "# Plan", graph: null, records: null, diagnostics: ["Invalid graph"] };
async function refresh() { useReadSources("host", "online"); return model.query!({ signal: new AbortController().signal }); }
describe("source refresh snapshots", () => {
  it("replaces reads after source deletion and preserves parse failure as a per-source result", async () => {
    model.list.mockReset().mockResolvedValue({ sources: [source], diagnostics: [] }); model.read.mockReset().mockResolvedValue(read);
    expect(await refresh()).toMatchObject({ reads: { plan: { status: "partial", diagnostics: ["Invalid graph"] } } });
    model.list.mockResolvedValue({ sources: [], diagnostics: [] });
    expect(await refresh()).toMatchObject({ sources: [], reads: {} });
    model.list.mockResolvedValue({ sources: [], diagnostics: ["Invalid source catalog"] });
    expect(await refresh()).toMatchObject({ reads: {}, diagnostics: ["Invalid source catalog"] });
  });
  it("rejects a catalog edited between listing and reading rather than mixing source identities", async () => {
    model.list.mockReset().mockResolvedValueOnce({ sources: [source], diagnostics: [] }).mockResolvedValue({ sources: [{ ...source, path: "/replacement/plan.md" }], diagnostics: [] });
    model.read.mockReset().mockResolvedValue({ ...read, source: { ...source, path: "/replacement/plan.md" } });
    await expect(refresh()).rejects.toThrow("Source catalog changed during refresh");
  });
});
