import { createHash } from "node:crypto";
import { mkdir, mkdtemp, realpath, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PluginServerContext, PluginSettings } from "@getpaseo/plugin/server";
import { contributeReadSources, readConfiguredSource, sourceCatalog } from "../server/read-sources";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationSettingsSchema } from "../shared/organization";
import { listReadSources, readSource, readPlanAssociations, type ReadSourceSummary } from "../shared/read-sources";

const catalogFailure = vi.hoisted(() => ({ denyOpen: false }));
vi.mock("node:fs", async (original) => {
  const actual = await original<typeof import("node:fs")>();
  return { ...actual, openSync: (...args: Parameters<typeof actual.openSync>) => {
    if (catalogFailure.denyOpen) throw Object.assign(new Error("permission denied"), { code: "EACCES" });
    return actual.openSync(...args);
  } };
});

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
  it("preserves explicit catalog workstreams through validation and exact-source reads", async () => {
    const { source, graph } = await fixture();
    const catalog = sourceCatalog(JSON.stringify([{ ...source, workstream: "ad-voca" }]));
    expect(catalog.diagnostics).toEqual([]);
    const result = await readConfiguredSource(catalog.sources[0]!, async () => JSON.stringify(graph));
    expect(result.status).toBe("ready");
    expect(result.source.workstream).toBe("ad-voca");
    expect(source.workstream).toBeNull();
  });
  it("fails closed for relative paths, duplicate IDs, unknown fields and malformed catalog", () => {
    expect(sourceCatalog("[]").sources).toEqual([]);
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

describe("operator catalog file on plugin load/reload", () => {
  let home: string;
  let file: string;
  const entry = { id: "operator-plan", title: "Operator plan", kind: "plan", path: "/operator/plan.md" };
  beforeEach(async () => {
    home = await mkdtemp(join(tmpdir(), "kontor-catalog-")); temporary.push(home);
    file = join(home, "plugin-config", "kontor", "read-sources.json");
    await mkdir(dirname(file), { recursive: true });
    vi.stubEnv("PASEO_HOME", home); vi.stubEnv("KONTOR_PLUGIN_READ_SOURCES", undefined);
  });
  afterEach(() => { catalogFailure.denyOpen = false; vi.unstubAllEnvs(); });
  it("keeps an absent file an honest no-sources state", () => {
    expect(sourceCatalog()).toEqual({ sources: [], diagnostics: ["No read sources configured on this host."] });
  });
  it("uses the same schema for a host-home file without reading its declared source contents", async () => {
    const raw = JSON.stringify([entry]); await writeFile(file, raw);
    expect(sourceCatalog()).toEqual(sourceCatalog(raw));
    expect(sourceCatalog().sources[0]?.path).toBe("/operator/plan.md");
  });
  it("preserves nonblank environment precedence, including empty and invalid explicit catalogs", async () => {
    await writeFile(file, JSON.stringify([entry]));
    for (const raw of [JSON.stringify([{ ...entry, id: "environment" }]), "[]", "invalid JSON"]) {
      vi.stubEnv("KONTOR_PLUGIN_READ_SOURCES", raw);
      expect(sourceCatalog()).toEqual(sourceCatalog(raw));
      expect(sourceCatalog().sources.some((source) => source.id === entry.id)).toBe(false);
    }
    vi.stubEnv("KONTOR_PLUGIN_READ_SOURCES", "  "); expect(sourceCatalog().sources[0]?.id).toBe(entry.id);
  });
  it("loads a fresh snapshot on the next plugin load, without altering the prior snapshot", async () => {
    await writeFile(file, JSON.stringify([entry])); const previous = sourceCatalog();
    await writeFile(file, JSON.stringify([{ ...entry, id: "replacement" }]));
    expect(sourceCatalog().sources[0]?.id).toBe("replacement");
    expect(previous.sources[0]?.id).toBe(entry.id);
  });
  it("refreshes list/read/association RPCs without a worker reload and revokes removed or invalid entries", async () => {
    const { source } = await fixture();
    const server = { handle: vi.fn() };
    const settings: PluginSettings<typeof organizationSettingsSchema> = { read: async () => ({ status: "ready", revision: "1", values: DEFAULT_ORGANIZATION_SETTINGS }), subscribe: () => () => {} };
    const stop = contributeReadSources(server, settings);
    const handler = (name: string) => server.handle.mock.calls.find(([rpc]) => rpc.name === name)![1];
    expect(handler(listReadSources.name)({}).sources).toEqual([]);
    await writeFile(file, JSON.stringify([{ ...source, kind: "delivery" }]));
    await writeFile(source.path, JSON.stringify({ delivery: [] }));
    expect(handler(listReadSources.name)({}).sources).toHaveLength(1);
    expect(await handler(readSource.name)({ sourceId: source.id })).toMatchObject({ status: "ready", records: { delivery: [] } });
    await writeFile(file, JSON.stringify([source]));
    expect(await handler(readPlanAssociations.name)({})).toMatchObject({ sources: [source] });
    await writeFile(file, "invalid JSON");
    expect(handler(listReadSources.name)({}).diagnostics.join()).toContain("Invalid source catalog");
    expect(() => handler(readSource.name)({ sourceId: source.id })).toThrow("not in the configured host catalog");
    await rm(file);
    expect(handler(listReadSources.name)({}).diagnostics.join()).toContain("No read sources configured");
    stop();
  });
  it("uses the existing default Paseo home when PASEO_HOME is absent", async () => {
    vi.stubEnv("PASEO_HOME", undefined); vi.stubEnv("HOME", home);
    const defaultFile = join(home, ".paseo", "plugin-config", "kontor", "read-sources.json");
    await mkdir(dirname(defaultFile), { recursive: true }); await writeFile(defaultFile, JSON.stringify([entry]));
    expect(sourceCatalog().sources[0]?.id).toBe(entry.id);
  });
  it.each([
    "not JSON", JSON.stringify([{ ...entry, path: "relative.md" }]),
    JSON.stringify([{ ...entry, credential: "not allowed" }]), JSON.stringify([entry, entry]),
    JSON.stringify([{ ...entry, revisionKind: "approved revision" }]),
    JSON.stringify(Array.from({ length: 65 }, (_, index) => ({ ...entry, id: `source-${index}` }))),
  ])("reports invalid file entries with the file origin (%#)", async (raw) => {
    await writeFile(file, raw); const result = sourceCatalog();
    expect(result.sources).toEqual([]); expect(result.diagnostics.join()).toContain(`Invalid source catalog ${file}:`);
    expect(sourceCatalog(raw).sources).toEqual([]);
  });
  it("accepts a 2 MiB catalog and diagnoses oversized or non-regular files", async () => {
    await writeFile(file, "[]" + " ".repeat(2 * 1024 * 1024 - 2)); expect(sourceCatalog()).toEqual({ sources: [], diagnostics: [] });
    await writeFile(file, "x".repeat(2 * 1024 * 1024 + 1)); expect(sourceCatalog().diagnostics.join()).toContain("regular file of at most 2 MiB");
    await rm(file); await mkdir(file); expect(sourceCatalog().diagnostics.join()).toContain("regular file of at most 2 MiB");
  });
  it("reports unreadable file errors instead of claiming no configuration", () => {
    catalogFailure.denyOpen = true; const result = sourceCatalog();
    expect(result.sources).toEqual([]); expect(result.diagnostics.join()).toContain(`Cannot read source catalog ${file}: permission denied`);
  });
});
