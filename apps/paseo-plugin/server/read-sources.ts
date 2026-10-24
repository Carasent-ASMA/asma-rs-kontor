import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { constants, closeSync, fstatSync, openSync, readSync } from "node:fs";
import { open, realpath, stat } from "node:fs/promises";
import { dirname, isAbsolute, join } from "node:path";
import { promisify } from "node:util";
import { z } from "zod";
import { parseDocument } from "yaml";
import type { PluginServerContext, PluginSettings } from "@getpaseo/plugin/server";
import { canonicalGraphSchema, jiraHistorySchema, listReadSources, readPlanAssociations, readSource, readSourceSummaries, recordsSchema,
  sourceSummarySchema, workstreamRegistrySchema, type ReadSourceSummary, type SourceRead, type SourceSummaryRead, type WorkstreamRegistry } from "../shared/read-sources";
import { GRAPH_FENCE, planMetadata } from "./plan-metadata";
import type { organizationSettingsSchema } from "../shared/organization";
import { readConfig } from "./config";

const MAX_BYTES = 2 * 1024 * 1024;
const execute = promisify(execFile);
const entrySchema = sourceSummarySchema.extend({
  id: z.string().regex(/^[a-zA-Z0-9_-]{1,128}$/), title: z.string().min(1),
  workstream: z.string().nullable().default(null), epic: z.string().regex(/^ASMA-\d+$/).nullable().default(null),
  projectId: z.string().nullable().default(null), revisionKind: z.enum(["saved draft", "working copy", "approved revision"]).default("working copy"),
  path: z.string().refine(isAbsolute, "Source path must be absolute"),
}).strict().refine((entry) => entry.revisionKind !== "approved revision" || entry.expectedSha256 != null, "An approved revision requires an expected SHA-256");
export type SourceCatalog = { sources: ReadSourceSummary[]; diagnostics: string[] };
function parseSourceCatalog(raw: string, origin: string): SourceCatalog {
  try {
    const sources = z.array(entrySchema).max(512).parse(JSON.parse(raw));
    if (new Set(sources.map((source) => source.id)).size !== sources.length) throw new Error("Duplicate source IDs");
    return { sources, diagnostics: [] };
  } catch (error) {
    return { sources: [], diagnostics: [`Invalid ${origin}: ${error instanceof Error ? error.message : String(error)}`] };
  }
}
/** Read one operator-owned catalog snapshot, never from client settings. */
export function sourceCatalog(raw = process.env.KONTOR_PLUGIN_READ_SOURCES): SourceCatalog {
  if (raw?.trim()) return parseSourceCatalog(raw, "KONTOR_PLUGIN_READ_SOURCES");
  const file = join(dirname(readConfig().paseoWorktreesRoot), "plugin-config", "kontor", "read-sources.json");
  let descriptor: number | undefined;
  try {
    // Non-blocking open prevents a misconfigured FIFO from blocking plugin reload.
    descriptor = openSync(file, constants.O_RDONLY | constants.O_NONBLOCK);
    const info = fstatSync(descriptor);
    if (!info.isFile() || info.size > MAX_BYTES) throw new Error("Catalog must be a regular file of at most 2 MiB");
    const buffer = Buffer.alloc(MAX_BYTES + 1);
    let length = 0;
    while (length < buffer.length) {
      const count = readSync(descriptor, buffer, length, buffer.length - length, null);
      if (!count) break;
      length += count;
    }
    if (length > MAX_BYTES) throw new Error("Catalog exceeds 2 MiB");
    return parseSourceCatalog(buffer.subarray(0, length).toString("utf8"), `source catalog ${file}`);
  } catch (error) {
    if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") {
      return { sources: [], diagnostics: ["No read sources configured on this host."] };
    }
    return { sources: [], diagnostics: [`Cannot read source catalog ${file}: ${error instanceof Error ? error.message : String(error)}`] };
  } finally {
    if (descriptor !== undefined) closeSync(descriptor);
  }
}
const sha256 = (data: string) => createHash("sha256").update(data).digest("hex");
export async function boundedRead(path: string): Promise<string> {
  const handle = await open(path, "r");
  try {
    const info = await handle.stat();
    if (!info.isFile() || info.size > MAX_BYTES) throw new Error("Source must be a regular file of at most 2 MiB");
    const buffer = Buffer.alloc(MAX_BYTES + 1);
    const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
    if (bytesRead > MAX_BYTES) throw new Error("Source exceeds 2 MiB");
    return buffer.subarray(0, bytesRead).toString("utf8");
  } finally { await handle.close(); }
}
export type ExportPlan = (path: string) => Promise<string>;
/** One bounded authoritative file, refreshed with the catalog; no filesystem discovery in the UI. */
export async function readWorkstreamRegistry(workspace = process.env.KONTOR_PLUGIN_ASMA_WORKSPACE?.trim() || undefined): Promise<WorkstreamRegistry> {
  const result: WorkstreamRegistry = { path: null, revision: null, workstreams: {}, desks: {}, diagnostics: [] };
  try {
    if (workspace === undefined) {
      // Operator-owned, like the source catalog; clients cannot supply filesystem roots.
      const file = join(dirname(readConfig().paseoWorktreesRoot), "plugin-config", "kontor", "registry.json");
      if (!isAbsolute(file)) throw new Error("Registry configuration path must be absolute");
      const raw = await boundedRead(file);
      const parsed: unknown = JSON.parse(raw);
      if (parseDocument(raw).errors.length) throw new Error("Registry configuration must not repeat keys");
      const config = z.object({ workspaceRoot: z.string().refine(isAbsolute, "Workspace root must be absolute") }).strict()
        .parse(parsed);
      workspace = config.workspaceRoot;
    }
    if (!workspace || !isAbsolute(workspace)) throw new Error("Configure an absolute workspace root in KONTOR_PLUGIN_ASMA_WORKSPACE or plugin-config/kontor/registry.json");
    result.path = join(workspace, "config/orchestration/workstreams.json");
    const content = await boundedRead(result.path);
    const raw = z.object({ schema_version: z.literal(1), workstreams: workstreamRegistrySchema.shape.workstreams, desks: workstreamRegistrySchema.shape.desks }).parse(JSON.parse(content));
    return { ...result, workstreams: raw.workstreams, desks: raw.desks, revision: sha256(content) };
  } catch (error) { result.diagnostics.push(`Workstream registry unavailable: ${error instanceof Error ? error.message : String(error)}`); return result; }
}
const exportPlan: ExportPlan = async (path) => {
  const workspace = process.env.KONTOR_PLUGIN_ASMA_WORKSPACE?.trim();
  if (workspace && !isAbsolute(workspace)) throw new Error("KONTOR_PLUGIN_ASMA_WORKSPACE must be an absolute host-side path");
  const result = await execute("asma", ["plan", "export", path], {
    // A documentation checkout need not contain the installed plan validator.
    // Use the host's configured ASMA workspace, never a client-selected cwd.
    cwd: workspace || undefined, timeout: 15_000, maxBuffer: MAX_BYTES, encoding: "utf8",
  });
  return result.stdout;
};
/** A fresh export of the exact catalog file; no cached graph or Markdown inference. */
export async function readConfiguredSource(source: ReadSourceSummary, exporter: ExportPlan = exportPlan): Promise<SourceRead> {
  const result: SourceRead = { source, status: "unavailable", revision: null, readAt: new Date().toISOString(),
    modifiedAt: null, content: null, graph: null, records: null, diagnostics: [] };
  try {
    // Resolving symlinks is diagnostic/binding only; settings can never supply this path.
    const path = await realpath(source.path);
    const content = await boundedRead(path);
    const revision = sha256(content);
    result.revision = revision;
    if (source.expectedSha256 && source.expectedSha256 !== revision) throw new Error(`Configured revision mismatch: expected ${source.expectedSha256}, read ${revision}`);
    result.modifiedAt = (await stat(path)).mtime.toISOString();
    if (source.kind === "jira") {
      const history = jiraHistorySchema.parse(JSON.parse(content));
      if (source.epic !== null && !history.epics.some((epic) => epic.key === source.epic)) throw new Error("Configured epic is absent from historical Jira projection");
      result.jiraHistory = history;
      result.status = "ready";
      return result;
    }
    if (source.kind !== "plan") {
      const raw: unknown = JSON.parse(content);
      if (typeof raw !== "object" || raw === null || !(source.kind in raw)) throw new Error(`Expected a ${source.kind} records document`);
      const records = recordsSchema.parse(raw);
      result.records = recordsSchema.parse({ [source.kind]: records[source.kind] });
      result.status = "ready";
      return result;
    }
    result.content = content;
    result.planMetadata = planMetadata(content);
    result.diagnostics.push(...result.planMetadata.diagnostics);
    if (source.workstream !== null && result.planMetadata.workstream !== null && source.workstream !== result.planMetadata.workstream) {
      result.diagnostics.push("Catalog and frontmatter workstreams conflict; automatic workstream association is disabled.");
      result.planMetadata.workstream = null;
    }
    result.status = "partial";
    if (!GRAPH_FENCE.test(content)) {
      if (source.epic && result.planMetadata.epic && source.epic !== result.planMetadata.epic) throw new Error("Configured epic differs from frontmatter epic");
      result.status = "ready";
      result.diagnostics.push("No canonical task graph declared; Markdown and explicit metadata only.");
      return result;
    }
    const graph = canonicalGraphSchema.parse(JSON.parse(await exporter(path)));
    const exportedPath = isAbsolute(graph.source) ? graph.source : `${dirname(path)}/${graph.source}`;
    if (await realpath(exportedPath) !== path) throw new Error("Canonical export names a different source");
    if (graph.source_sha256 !== revision || sha256(await boundedRead(path)) !== revision || await realpath(source.path) !== path) {
      throw new Error("Source changed during canonical export; retry this exact source");
    }
    if (source.epic !== null && graph.epic.jira !== source.epic) throw new Error("Configured epic differs from the canonical source epic");
    if (result.planMetadata.epic !== null && result.planMetadata.epic !== graph.epic.jira) throw new Error("Frontmatter epic differs from the canonical source epic");
    result.graph = graph;
    result.status = "ready";
  } catch (error) {
    result.diagnostics.push(`${source.id} (${source.path}): ${error instanceof Error ? error.message : String(error)}`);
    if (error && typeof error === "object" && "stdout" in error && typeof error.stdout === "string" && error.stdout.trim()) result.diagnostics.push(error.stdout.trim().slice(0, 4096));
  }
  return result;
}
/** One bounded metadata read: no Markdown content, no graph export and no JSON records; the Jira projection is complete. */
export async function readConfiguredSummary(source: ReadSourceSummary): Promise<SourceSummaryRead> {
  const result: SourceSummaryRead = { source, status: "unavailable", revision: null, readAt: new Date().toISOString(),
    modifiedAt: null, content: null, graph: null, records: null, diagnostics: [] };
  try {
    // Resolving symlinks is diagnostic/binding only; settings can never supply this path.
    const path = await realpath(source.path);
    const content = await boundedRead(path);
    const revision = sha256(content);
    result.revision = revision;
    if (source.expectedSha256 && source.expectedSha256 !== revision) throw new Error(`Configured revision mismatch: expected ${source.expectedSha256}, read ${revision}`);
    result.modifiedAt = (await stat(path)).mtime.toISOString();
    if (source.kind === "jira") {
      const history = jiraHistorySchema.parse(JSON.parse(content));
      if (source.epic !== null && !history.epics.some((epic) => epic.key === source.epic)) throw new Error("Configured epic is absent from historical Jira projection");
      result.jiraHistory = history;
      result.status = "ready";
      return result;
    }
    if (source.kind !== "plan") {
      const raw: unknown = JSON.parse(content);
      if (typeof raw !== "object" || raw === null || !(source.kind in raw)) throw new Error(`Expected a ${source.kind} records document`);
      // Validate the record document without returning it; the page reads it on demand.
      recordsSchema.parse(raw);
      result.status = "ready";
      return result;
    }
    result.planMetadata = planMetadata(content);
    result.diagnostics.push(...result.planMetadata.diagnostics);
    if (source.workstream !== null && result.planMetadata.workstream !== null && source.workstream !== result.planMetadata.workstream) {
      result.diagnostics.push("Catalog and frontmatter workstreams conflict; automatic workstream association is disabled.");
      result.planMetadata.workstream = null;
    }
    if (source.epic !== null && result.planMetadata.epic !== null && source.epic !== result.planMetadata.epic) throw new Error("Configured epic differs from frontmatter epic");
    if (!GRAPH_FENCE.test(content)) result.diagnostics.push("No canonical task graph declared; Markdown and explicit metadata only.");
    result.status = "ready";
  } catch (error) {
    result.diagnostics.push(`${source.id} (${source.path}): ${error instanceof Error ? error.message : String(error)}`);
  }
  return result;
}
export function contributeReadSources(server: Pick<PluginServerContext, "handle">, settings: PluginSettings<typeof organizationSettingsSchema>, catalog: SourceCatalog | (() => SourceCatalog) = sourceCatalog): () => void {
  const currentCatalog = () => typeof catalog === "function" ? catalog() : catalog;
  let latest: Awaited<ReturnType<typeof settings.read>> | null = null;
  let readError: string | null = null;
  let stopped = false;
  const firstRead = settings.read().then((state) => { if (latest === null && !stopped) latest = state; }, (error: unknown) => { readError = error instanceof Error ? error.message : String(error); });
  const stop = settings.subscribe((state) => { latest = state; readError = null; });
  server.handle(listReadSources, async () => ({ ...currentCatalog(), registry: await readWorkstreamRegistry() }));
  server.handle(readSourceSummaries, async () => {
    const sources = currentCatalog();
    const reads: Record<string, SourceSummaryRead> = {};
    const registry = readWorkstreamRegistry();
    let cursor = 0;
    // Bounded parallel file reads; each source is read exactly once and never exported.
    async function worker() {
      while (cursor < sources.sources.length) {
        const source = sources.sources[cursor++];
        if (!source) return;
        reads[source.id] = await readConfiguredSummary(source);
      }
    }
    await Promise.all(Array.from({ length: Math.min(8, sources.sources.length) }, () => worker()));
    return { ...sources, registry: await registry, reads };
  });
  server.handle(readSource, ({ sourceId }) => {
    const source = currentCatalog().sources.find((entry) => entry.id === sourceId);
    if (!source) throw new Error("Source ID is not in the configured host catalog");
    return readConfiguredSource(source);
  });
  server.handle(readPlanAssociations, async () => {
    await firstRead;
    const state = latest;
    return { status: state?.status ?? ("unavailable" as const), associations: state?.status === "ready" ? state.values.planAssociations : {},
      sources: currentCatalog().sources.filter((entry) => entry.kind === "plan"), error: state?.status === "invalid" ? state.error : readError };
  });
  return () => { stopped = true; void stop(); };
}
