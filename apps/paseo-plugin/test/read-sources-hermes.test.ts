import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it, vi } from "vitest";
import * as readSources from "../shared/read-sources";
import * as teamsSnapshot from "../shared/teams-source-snapshot";
import * as teamsBindings from "../shared/teams-source-bindings";
import { deferredReact, loadHermesScoped } from "./hermes-scoping";
import type { useReadSources as Hook } from "../client/use-read-sources";
import { readConfiguredSource, readConfiguredSummary } from "../server/read-sources";
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

/** QA1-PR477 race harness: mutable summaries and a full read resolved by the test. */
function raceHarness(initial: SourceRead) {
  let current = initial;
  const pending: Array<(value: SourceRead) => void> = [];
  const read = vi.fn((_input: { sourceId: string }) => new Promise<SourceRead>((resolve) => pending.push(resolve)));
  const { react, render } = deferredReact();
  const modules: Record<string, unknown> = {
    react,
    "@tanstack/react-query": { useQuery: () => ({ data: { sources: [current.source], reads: { plan: current }, diagnostics: [] },
      isError: false, isFetching: false, error: null, refetch: async () => ({}) }) },
    "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => contract.name === "sources.read" ? read : async () => ({}) },
    "../shared/read-sources": readSources, "../shared/teams-source-snapshot": teamsSnapshot, "../shared/teams-source-bindings": teamsBindings,
  };
  const { useReadSources } = loadHermesScoped<{ useReadSources: typeof Hook }>("../client/use-read-sources.ts", modules);
  return { draw: () => render(() => useReadSources("fixture-host", "online")), set: (value: SourceRead) => { current = value; }, read, pending };
}
const raceSettle = async () => { for (let n = 0; n < 4; n++) await new Promise((resolve) => setTimeout(resolve, 0)); };
const raceSummary = (revision: string, path = planSource.path): SourceRead => ({ ...summary(planSource, revision), source: { ...planSource, path } });
const raceFull = (revision: string, path = planSource.path): SourceRead => ({ ...raceSummary(revision, path), content: `# ${revision}` });

describe("QA1-PR477 exact source demand races", () => {
  it("does not cache a file changed after summaries under its old revision", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qa1-477-revision-"));
    try {
      const path = join(directory, "plan.md"), configured = { ...planSource, path };
      await writeFile(path, "# Revision A");
      const first = await readConfiguredSummary(configured);
      const harness = raceHarness(first);
      harness.draw().demand(["plan"]); harness.draw(); await raceSettle();
      await writeFile(path, "# Revision B");
      const later = await readConfiguredSource(configured);
      expect(later.revision).not.toBe(first.revision);
      harness.pending[0]!(later); await raceSettle();
      expect(harness.draw().reads.plan!.content).toBeNull();
    } finally { await rm(directory, { recursive: true, force: true }); }
  });
  it("control: a matching full read overlays its summary", async () => {
    const harness = raceHarness(raceSummary("a"));
    harness.draw().demand(["plan"]); harness.draw(); await raceSettle();
    harness.pending[0]!(raceFull("a")); await raceSettle();
    expect(harness.draw().reads.plan!.content).toBe("# a");
  });
  it("does not label a different returned revision as the requested revision", async () => {
    const harness = raceHarness(raceSummary("a"));
    harness.draw().demand(["plan"]); harness.draw(); await raceSettle();
    harness.pending[0]!(raceFull("b")); await raceSettle();
    const seen = harness.draw().reads.plan!;
    expect(seen.content).toBeNull(); expect(seen.revision).toBe("a");
  });
  it("does not overlay a replacement path returned under the same source ID", async () => {
    const harness = raceHarness(raceSummary("a"));
    harness.draw().demand(["plan"]); harness.draw(); await raceSettle();
    harness.pending[0]!(raceFull("a", "/fixture/replacement.md")); await raceSettle();
    const seen = harness.draw().reads.plan!;
    expect(seen.source.path).toBe(planSource.path); expect(seen.content).toBeNull();
  });
  it("keeps the current revision when an older revision completes late", async () => {
    const harness = raceHarness(raceSummary("a"));
    harness.draw().demand(["plan"]); harness.draw(); await raceSettle();
    harness.set(raceSummary("b")); harness.draw(); await raceSettle();
    expect(harness.read).toHaveBeenCalledTimes(2);
    harness.pending[1]!(raceFull("b")); await raceSettle();
    expect(harness.draw().reads.plan!.content).toBe("# b");
    harness.pending[0]!(raceFull("a")); await raceSettle();
    const seen = harness.draw().reads.plan!;
    expect(seen.content).toBe("# b"); expect(seen.revision).toBe("b");
  });
  it("treats a same-byte replacement path as a distinct demand while the old path is in flight", async () => {
    const harness = raceHarness(raceSummary("a"));
    harness.draw().demand(["plan"]); harness.draw(); await raceSettle();
    harness.set(raceSummary("a", "/fixture/replacement.md")); harness.draw(); await raceSettle();
    expect(harness.read).toHaveBeenCalledTimes(2);
  });
});

describe("QA1-PR477 current summary policy controls", () => {
  it("does not let an older ready full cache override a current pin-mismatch summary", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qa1-477-pin-"));
    try {
      const path = join(directory, "plan.md");
      await writeFile(path, "# Exact fixture revision A");
      const unpinned = { ...planSource, path };
      const bytes = await readConfiguredSummary(unpinned);
      expect(bytes.status).toBe("ready");
      const approved = { ...unpinned, revisionKind: "approved revision" as const, expectedSha256: bytes.revision! };
      const current = await readConfiguredSummary(approved);
      const loaded = await readConfiguredSource(approved);
      expect(loaded.status).toBe("ready");
      const harness = raceHarness(current);
      harness.draw().demand(["plan"]); harness.draw(); await raceSettle();
      harness.pending[0]!(loaded); await raceSettle();
      expect(harness.draw().reads.plan!.status).toBe("ready");
      const replaced = { ...approved, expectedSha256: "b".repeat(64) };
      const failed = await readConfiguredSummary(replaced);
      expect(failed.status).toBe("unavailable");
      expect(failed.revision).toBe(loaded.revision);
      expect(failed.diagnostics.join(" ")).toContain("Configured revision mismatch");
      harness.set(failed); harness.draw(); await raceSettle();
      const shown = harness.draw().reads.plan!;
      expect(shown.status).toBe("unavailable");
      expect(shown.content).toBeNull();
      expect(shown.diagnostics.join(" ")).toContain("Configured revision mismatch");
      expect(shown.source.expectedSha256).toBe("b".repeat(64));
    } finally { await rm(directory, { recursive: true, force: true }); }
  });
  it("shows a same-policy exact read failure as unavailable, not Reading, without a busy retry", async () => {
    const directory = await mkdtemp(join(tmpdir(), "qa1-477-exact-unavailable-"));
    try {
      const path = join(directory, "plan.md");
      await writeFile(path, "# Readable summary then removed");
      const before = await readConfiguredSummary({ ...planSource, path });
      await rm(path);
      const read = vi.fn(async ({ sourceId }: { sourceId: string }) => readConfiguredSource({ ...planSource, path, id: sourceId }));
      const { react, render } = deferredReact();
      const readerModules: Record<string, unknown> = {
        react,
        "@tanstack/react-query": { useQuery: () => ({ data: { sources: [before.source], reads: { plan: before }, diagnostics: [] },
          isError: false, isFetching: false, error: null, refetch: async () => ({}) }) },
        "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => contract.name === "sources.read" ? read : async () => ({}) },
        "../shared/read-sources": readSources, "../shared/teams-source-snapshot": teamsSnapshot, "../shared/teams-source-bindings": teamsBindings,
      };
      const { useReadSources } = loadHermesScoped<{ useReadSources: typeof Hook }>("../client/use-read-sources.ts", readerModules);
      const draw = () => render(() => useReadSources("fixture-host", "online"));
      draw().demand(["plan"]); draw(); await raceSettle();
      const shown = draw().reads.plan!;
      expect(shown.status).toBe("unavailable");
      expect(shown.content).toBeNull();
      expect(shown.diagnostics.join(" ")).toContain("ENOENT");
      expect(readSources.sourceReadPending(shown)).toBe(false);
      expect(read).toHaveBeenCalledTimes(1);
      for (let index = 0; index < 5; index++) { draw(); await raceSettle(); }
      expect(read).toHaveBeenCalledTimes(1);
    } finally { await rm(directory, { recursive: true, force: true }); }
  });
  it("treats a policy-only approval while an unpinned read is in flight as a new identity", async () => {
    const revision = "a".repeat(64), initial = raceSummary(revision), harness = raceHarness(initial);
    harness.draw().demand(["plan"]); harness.draw(); await raceSettle();
    expect(harness.read).toHaveBeenCalledTimes(1);
    const approved = { ...initial, source: { ...planSource, revisionKind: "approved revision" as const, expectedSha256: revision } };
    harness.set(approved); harness.draw(); await raceSettle();
    expect(harness.read).toHaveBeenCalledTimes(2);
    harness.pending[1]!({ ...summary(approved.source, revision), content: "# approved" }); await raceSettle();
    expect(harness.draw().reads.plan!.source.expectedSha256).toBe(revision);
    harness.pending[0]!({ ...summary(planSource, revision), content: "# old policy" }); await raceSettle();
    const shown = harness.draw().reads.plan!;
    expect(shown.content).not.toBeNull();
    expect(shown.source.expectedSha256).toBe(revision);
    expect(harness.read).toHaveBeenCalledTimes(2);
  });
});
