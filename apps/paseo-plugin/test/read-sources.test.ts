import { createHash } from "node:crypto";
import { mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PluginServerContext, PluginSettings } from "@getpaseo/plugin/server";
import { contributeReadSources, readConfiguredSource, sourceCatalog } from "../server/read-sources";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationSettingsSchema } from "../shared/organization";
import { readPlanAssociations, type ReadSourceSummary } from "../shared/read-sources";

const temporary: string[] = [];
afterEach(async () => { await Promise.all(temporary.splice(0).map((path) => rm(path, { recursive: true, force: true }))); });
async function fixture(content = "# Plan\n\nNo dependencies are inferred from this prose.") {
  const directory = await mkdtemp(join(tmpdir(), "kontor-source-")); temporary.push(directory);
  const path = join(directory, "plan.md"); await writeFile(path, content);
  const source: ReadSourceSummary = { id: "draft", title: "Draft", kind: "plan", path, workstream: null, epic: null, projectId: null, revisionKind: "saved draft" };
  const graph = { schema_version: 1, source: path, source_sha256: createHash("sha256").update(content).digest("hex"), graph_sha256: "graph", epic: { jira: null, kontor_id: null }, nodes: [], edges: [], waves: [], external_fences: [] };
  return { source, graph, content };
}
describe("configured read sources", () => {
  it("fails closed for relative paths, duplicate IDs, unknown fields and malformed catalog", () => {
    expect(sourceCatalog().sources).toEqual([]);
    for (const raw of ["no json", JSON.stringify([{ id: "x", title: "x", kind: "plan", path: "relative.md" }]), JSON.stringify([{ id: "x", title: "x", kind: "plan", path: "/tmp/p", credential: "forbidden" }])]) expect(sourceCatalog(raw).sources).toEqual([]);
    const item = { id: "x", title: "x", kind: "plan", path: "/tmp/p" };
    expect(sourceCatalog(JSON.stringify([item, item])).diagnostics.join()).toContain("Duplicate source IDs");
  });
  it("returns the exact source with a fresh matching canonical export and draft provenance", async () => {
    const { source, graph, content } = await fixture();
    const exporter = vi.fn(async () => JSON.stringify(graph));
    const result = await readConfiguredSource(source, exporter);
    expect(exporter).toHaveBeenCalledWith(await realpath(source.path));
    expect(result.status).toBe("ready"); expect(result.content).toBe(content); expect(result.revision).toBe(graph.source_sha256);
    expect(result.graph?.nodes).toEqual([]); expect(result.source.revisionKind).toBe("saved draft");
  });
  it("refuses a stale graph, another source, unsupported exports, and source changes during export", async () => {
    const { source, graph } = await fixture();
    const second = await fixture("# Different plan");
    for (const exported of [{ ...graph, source_sha256: "a".repeat(64) }, { ...graph, source: second.source.path }, { ...graph, schema_version: 2 }]) {
      const result = await readConfiguredSource(source, async () => JSON.stringify(exported));
      expect(result.graph).toBeNull(); expect(result.status).toBe("partial"); expect(result.diagnostics.join()).toContain(source.path);
    }
    const changed = await readConfiguredSource(source, async () => { await writeFile(source.path, "# Changed"); return JSON.stringify(graph); });
    expect(changed.graph).toBeNull(); expect(changed.diagnostics.join()).toContain("changed during");
  });
  it("reports source-specific exporter errors without legacy Markdown extraction", async () => {
    const { source } = await fixture("```mermaid\ngraph TD\nA-->B\n```\nTASK-1 -> TASK-2");
    const result = await readConfiguredSource(source, async () => { throw new Error("canonical dependency cycle A → B → A"); });
    expect(result.status).toBe("partial"); expect(result.graph).toBeNull(); expect(result.diagnostics.join()).toContain("dependency cycle");
  });
  it("reports deleted/oversized sources and never starts an export for them", async () => {
    const { source } = await fixture("x".repeat(2 * 1024 * 1024 + 1));
    const exporter = vi.fn(async () => "{}");
    expect((await readConfiguredSource(source, exporter)).status).toBe("unavailable");
    await rm(source.path); expect((await readConfiguredSource(source, exporter)).status).toBe("unavailable");
    expect(exporter).not.toHaveBeenCalled();
  });
  it("does not present a changed working copy as an approved revision", async () => {
    const { source } = await fixture();
    expect(sourceCatalog(JSON.stringify([{ ...source, revisionKind: "approved revision" }])).sources).toEqual([]);
    const result = await readConfiguredSource({ ...source, revisionKind: "approved revision", expectedSha256: "f".repeat(64) });
    expect(result.status).toBe("unavailable"); expect(result.content).toBeNull(); expect(result.graph).toBeNull();
    expect(result.diagnostics.join()).toContain("Configured revision mismatch");
  });
  it("reads actual configured team chains without presenting an observed assignment", async () => {
    const { source } = await fixture(JSON.stringify({ teams: [{ id: "review", title: "Review", roles: [{ role: "AUD", chain: ["provider/model/high"] }] }] }));
    const result = await readConfiguredSource({ ...source, kind: "teams" });
    expect(result.records?.teams[0]?.roles[0]?.chain).toEqual(["provider/model/high"]);
    expect(result.graph).toBeNull(); expect(result.status).toBe("ready");
  });
  it("reads host settings and subscribes to successful revisions for explicit associations", async () => {
    const handlers = new Map<string, (input: object) => unknown>();
    let listener: ((value: Awaited<ReturnType<PluginSettings<typeof organizationSettingsSchema>["read"]>>) => unknown) | undefined;
    const stop = vi.fn();
    const settings: PluginSettings<typeof organizationSettingsSchema> = { read: async () => ({ status: "ready", revision: "1", values: DEFAULT_ORGANIZATION_SETTINGS }), subscribe: (next) => { listener = next; return stop; } };
    const server = { handle: (contract: { name: string }, handler: (input: object) => unknown) => handlers.set(contract.name, handler) } as unknown as PluginServerContext;
    const cleanup = contributeReadSources(server, settings);
    listener?.({ status: "ready", revision: "2", values: { ...DEFAULT_ORGANIZATION_SETTINGS, planAssociations: { native: ["draft"] } } });
    expect(await handlers.get(readPlanAssociations.name)?.({})).toMatchObject({ status: "ready", associations: { native: ["draft"] } });
    listener?.({ status: "invalid", revision: "3", error: "invalid stored preferences" });
    expect(await handlers.get(readPlanAssociations.name)?.({})).toMatchObject({ status: "invalid", associations: {} });
    cleanup(); expect(stop).toHaveBeenCalledOnce();
  });
});
