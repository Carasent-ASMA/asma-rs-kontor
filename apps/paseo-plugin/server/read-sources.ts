import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { open, realpath, stat } from "node:fs/promises";
import { dirname, isAbsolute } from "node:path";
import { promisify } from "node:util";
import { z } from "zod";
import type { PluginServerContext, PluginSettings } from "@getpaseo/plugin/server";
import { canonicalGraphSchema, listReadSources, readPlanAssociations, readSource, recordsSchema,
  sourceSummarySchema, type ReadSourceSummary, type SourceRead } from "../shared/read-sources";
import type { organizationSettingsSchema } from "../shared/organization";

const MAX_BYTES = 2 * 1024 * 1024;
const execute = promisify(execFile);
const entrySchema = sourceSummarySchema.extend({
  id: z.string().regex(/^[a-zA-Z0-9_-]{1,128}$/), title: z.string().min(1),
  workstream: z.string().nullable().default(null), epic: z.string().regex(/^ASMA-\d+$/).nullable().default(null),
  projectId: z.string().nullable().default(null), revisionKind: z.enum(["saved draft", "working copy", "approved revision"]).default("working copy"),
  path: z.string().refine(isAbsolute, "Source path must be absolute"),
}).strict();
export type SourceCatalog = { sources: ReadSourceSummary[]; diagnostics: string[] };
export function sourceCatalog(raw = process.env.KONTOR_PLUGIN_READ_SOURCES): SourceCatalog {
  if (!raw?.trim()) return { sources: [], diagnostics: ["No read sources configured on this host."] };
  try {
    const sources = z.array(entrySchema).max(64).parse(JSON.parse(raw));
    if (new Set(sources.map((source) => source.id)).size !== sources.length) throw new Error("Duplicate source IDs");
    return { sources, diagnostics: [] };
  } catch (error) {
    return { sources: [], diagnostics: [`Invalid KONTOR_PLUGIN_READ_SOURCES: ${error instanceof Error ? error.message : String(error)}`] };
  }
}
const sha256 = (data: string) => createHash("sha256").update(data).digest("hex");
async function boundedRead(path: string): Promise<string> {
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
const exportPlan: ExportPlan = async (path) => {
  const result = await execute("asma", ["plan", "export", path], {
    cwd: dirname(path), timeout: 15_000, maxBuffer: MAX_BYTES, encoding: "utf8",
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
    result.modifiedAt = (await stat(path)).mtime.toISOString();
    if (source.kind !== "plan") {
      const raw: unknown = JSON.parse(content);
      if (typeof raw !== "object" || raw === null || !(source.kind in raw)) throw new Error(`Expected a ${source.kind} records document`);
      const records = recordsSchema.parse(raw);
      result.records = recordsSchema.parse({ [source.kind]: records[source.kind] });
      result.status = "ready";
      return result;
    }
    result.content = content;
    result.status = "partial";
    const graph = canonicalGraphSchema.parse(JSON.parse(await exporter(path)));
    const exportedPath = isAbsolute(graph.source) ? graph.source : `${dirname(path)}/${graph.source}`;
    if (await realpath(exportedPath) !== path) throw new Error("Canonical export names a different source");
    if (graph.source_sha256 !== revision || sha256(await boundedRead(path)) !== revision || await realpath(source.path) !== path) {
      throw new Error("Source changed during canonical export; retry this exact source");
    }
    if (source.epic !== null && graph.epic.jira !== source.epic) throw new Error("Configured epic differs from the canonical source epic");
    result.graph = graph;
    result.status = "ready";
  } catch (error) {
    result.diagnostics.push(`${source.id} (${source.path}): ${error instanceof Error ? error.message : String(error)}`);
  }
  return result;
}
export function contributeReadSources(server: PluginServerContext, settings: PluginSettings<typeof organizationSettingsSchema>): () => void {
  const catalog = sourceCatalog();
  let current = settings.read();
  const stop = settings.subscribe((state) => { current = Promise.resolve(state); });
  server.handle(listReadSources, () => catalog);
  server.handle(readSource, ({ sourceId }) => {
    const source = catalog.sources.find((entry) => entry.id === sourceId);
    if (!source) throw new Error("Source ID is not in the configured host catalog");
    return readConfiguredSource(source);
  });
  server.handle(readPlanAssociations, async () => {
    const state = await current;
    return { status: state.status, associations: state.status === "ready" ? state.values.planAssociations : {},
      sources: catalog.sources.filter((entry) => entry.kind === "plan"), error: state.status === "invalid" ? state.error : null };
  });
  return () => { void stop(); };
}
