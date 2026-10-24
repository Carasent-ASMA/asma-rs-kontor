import { describe, expect, it, vi } from "vitest";
import * as readSources from "../shared/read-sources";
import * as teamsSnapshot from "../shared/teams-source-snapshot";
import * as teamsBindings from "../shared/teams-source-bindings";
import { deferredReact, loadHermesScoped } from "./hermes-scoping";
import type { useReadSources as Hook } from "../client/use-read-sources";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";

const planSource: ReadSourceSummary = { id: "plan", title: "Plan", kind: "plan", path: "/configured/plan.md", epic: "ASMA-1", projectId: null, workstream: null, revisionKind: "working copy" };
const otherSource: ReadSourceSummary = { ...planSource, id: "other", title: "Other", path: "/configured/other.md" };
const jiraSource: ReadSourceSummary = { ...planSource, id: "jira", title: "Jira", kind: "jira", path: "/configured/jira.json" };
const summary = (source: ReadSourceSummary, revision: string): SourceRead => ({ source, status: "ready", revision, readAt: "2026-10-24T10:00:00Z",
  modifiedAt: null, content: null, graph: null, records: null, diagnostics: [] });
const jira: SourceRead = { ...summary(jiraSource, "j"), jiraHistory: { schema: "consumption/data@2", generated: "2026-10-18T10:00:00Z",
  provenance: { path: "/capture.json", sha256: "a".repeat(64), capturedEpics: 0 }, selection: ["ASMA-1"], epics: [] } };
const full = (source: ReadSourceSummary, revision: string): SourceRead => ({ ...summary(source, revision), content: `# ${source.id} at ${revision}` });
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

function harness(data: unknown, read: (input: { sourceId: string }) => Promise<SourceRead>) {
  const { react, render } = deferredReact();
  const modules: Record<string, unknown> = {
    react,
    "@tanstack/react-query": { useQuery: () => ({ data, isError: false, error: null, isFetching: false, refetch: async () => ({}) }) },
    "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => contract.name === "sources.read" ? read : async () => ({}) },
    "../shared/read-sources": readSources, "../shared/teams-source-snapshot": teamsSnapshot, "../shared/teams-source-bindings": teamsBindings,
  };
  const { useReadSources } = loadHermesScoped<{ useReadSources: typeof Hook }>("../client/use-read-sources.ts", modules);
  const draw = () => render(() => useReadSources("host", "online"));
  return { draw };
}

describe("demanded source reads under the iOS (Hermes) scoping of plugin bundles", () => {
  it("reads only demanded exact sources and keys each full read by id and revision", async () => {
    const data = (revision: string) => ({ sources: [planSource, jiraSource], diagnostics: [], reads: { plan: summary(planSource, revision), jira }, teamsSnapshots: {}, teamsErrors: {}, teamsBindingDiagnostics: [] });
    const read = vi.fn(async ({ sourceId }: { sourceId: string }) => full(sourceId === "plan" ? planSource : jiraSource, "a"));
    const { draw } = harness(data("a"), read);
    let hook = draw();
    expect(hook.reads.plan!.content).toBeNull();
    hook.demand(["plan", "jira"]); hook = draw(); await settle(); hook = draw();
    expect(read).toHaveBeenCalledExactlyOnceWith({ sourceId: "plan" });
    expect(hook.reads.plan!.content).toBe("# plan at a");
    // The Jira projection already arrived complete with the summary.
    expect(hook.reads.jira!.jiraHistory?.schema).toBe("consumption/data@2");
    hook.demand(["plan", "jira"]); hook = draw(); await settle();
    expect(read).toHaveBeenCalledTimes(1);
  });
  it("does not overlay an old full read over a new summary revision", async () => {
    let revision = "a";
    const data = () => ({ sources: [planSource], diagnostics: [], reads: { plan: summary(planSource, revision) }, teamsSnapshots: {}, teamsErrors: {}, teamsBindingDiagnostics: [] });
    const pending: Array<(read: SourceRead) => void> = [];
    const read = vi.fn((_input: { sourceId: string }) => new Promise<SourceRead>((resolve) => pending.push(resolve)));
    let model = data();
    const { react, render } = deferredReact();
    const modules: Record<string, unknown> = {
      react,
      "@tanstack/react-query": { useQuery: () => ({ data: model, isError: false, error: null, isFetching: false, refetch: async () => ({}) }) },
      "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => contract.name === "sources.read" ? read : async () => ({}) },
      "../shared/read-sources": readSources, "../shared/teams-source-snapshot": teamsSnapshot, "../shared/teams-source-bindings": teamsBindings,
    };
    const { useReadSources } = loadHermesScoped<{ useReadSources: typeof Hook }>("../client/use-read-sources.ts", modules);
    const draw = () => render(() => useReadSources("host", "online"));
    let hook = draw(); hook.demand(["plan"]); hook = draw(); await settle();
    expect(pending).toHaveLength(1);
    revision = "b"; model = data(); hook = draw(); await settle();
    // Revision b is a new demand; the revision-a full read has not returned yet.
    expect(read).toHaveBeenCalledTimes(2); expect(hook.reads.plan!.content).toBeNull();
    pending[1]!(full(planSource, "b")); await settle(); hook = draw();
    expect(hook.reads.plan!.content).toBe("# plan at b"); expect(hook.reads.plan!.revision).toBe("b");
  });
  it("keeps at most three exact reads in flight", async () => {
    const sources = Array.from({ length: 8 }, (_, index) => ({ ...planSource, id: `plan-${index}`, path: `/configured/${index}.md` }));
    const summaries = Object.fromEntries(sources.map((source) => [source.id, summary(source, "a")]));
    const data = { sources, diagnostics: [], reads: summaries, teamsSnapshots: {}, teamsErrors: {}, teamsBindingDiagnostics: [] };
    let active = 0, peak = 0;
    const releases: Array<() => void> = [];
    const read = vi.fn(({ sourceId }: { sourceId: string }) => new Promise<SourceRead>((resolve) => {
      active += 1; peak = Math.max(peak, active);
      releases.push(() => { active -= 1; resolve(full(sources.find((source) => source.id === sourceId)!, "a")); });
    }));
    const { draw } = harness(data, read);
    let hook = draw(); hook.demand(sources.map((source) => source.id)); hook = draw(); await settle();
    expect(read).toHaveBeenCalledTimes(3); expect(peak).toBe(3);
    releases.shift()!(); await settle();
    expect(read).toHaveBeenCalledTimes(4); expect(peak).toBe(3);
    while (releases.length) { releases.shift()!(); await settle(); }
    hook = draw();
    expect(peak).toBe(3);
    expect(Object.values(hook.reads).filter((entry) => entry.content !== null)).toHaveLength(8);
  });
});
