import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { open, realpath } from "node:fs/promises";
import { dirname, isAbsolute, join } from "node:path";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { z } from "zod";
import { readConfig } from "./config";
import { exportTokenomics, readTokenomics, tokenomicsDataSchema, tokenomicsLiveSchema, tokenomicsQuerySchema, type TokenomicsData, type TokenomicsLive, type TokenomicsQuery, type TokenomicsView } from "../shared/tokenomics";
import { buildTokenomics, paginateTokenomics, pivotRows, tokenomicsCsv, tokenomicsOptions, usageModel } from "../shared/tokenomics-model";

export const REPORT_MAX_BYTES = 32 * 1024 * 1024;
export const LIVE_MAX_BYTES = 16 * 1024 * 1024;
const CONFIG_MAX_BYTES = 8192;
const STALE_MS = 48 * 3600_000;

/** File descriptors are non-blocking, regular, bounded, and checked again after reading. */
export async function readReportFile(path: string, limit: number): Promise<string> {
  const file = await open(path, constants.O_RDONLY | constants.O_NONBLOCK | constants.O_NOFOLLOW);
  try {
    const before = await file.stat();
    if (!before.isFile() || before.size > limit) throw new Error("Report must be a regular file within its byte limit");
    const bytes = Buffer.alloc(Math.min(before.size + 1, limit + 1)); let length = 0;
    while (length < bytes.length) { const result = await file.read(bytes, length, bytes.length - length, null); if (!result.bytesRead) break; length += result.bytesRead; }
    const after = await file.stat();
    if (length > limit || after.size !== before.size || after.mtimeMs !== before.mtimeMs || length !== before.size) throw new Error("Report changed while reading; retry");
    return bytes.subarray(0, length).toString("utf8");
  } finally { await file.close(); }
}

export function parseLiveAssignment(raw: string): TokenomicsLive {
  // Accept only the collector's literal JSON assignment, never JavaScript expressions or another statement.
  const match = /^\s*window\.ASMA_LIVE\s*=\s*(\{[\s\S]*\})\s*;?\s*$/.exec(raw);
  if (!match) throw new Error("Live report is not a JSON assignment");
  return tokenomicsLiveSchema.parse(JSON.parse(match[1]!));
}
const configSchema = z.object({ root: z.string().refine(isAbsolute) }).strict();
async function configuredRoot(): Promise<string | null> {
  if (process.env.KONTOR_PLUGIN_TOKENOMICS_ROOT !== undefined) return configSchema.parse({ root: process.env.KONTOR_PLUGIN_TOKENOMICS_ROOT }).root;
  const path = join(dirname(readConfig().paseoWorktreesRoot), "plugin-config", "kontor", "tokenomics.json");
  try { return configSchema.parse(JSON.parse(await readReportFile(path, CONFIG_MAX_BYTES))).root; }
  catch (error) { if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") return null; throw error; }
}
export type ReportSnapshot = { data: TokenomicsData | null; live: TokenomicsLive; revision: string | null; diagnostics: string[]; readAt: string };
export async function readTokenomicsSnapshot(root: string | null, now = new Date()): Promise<ReportSnapshot> {
  const result: ReportSnapshot = { data: null, live: tokenomicsLiveSchema.parse({}), revision: null, diagnostics: [], readAt: now.toISOString() };
  if (!root) { result.diagnostics.push("Tokenomics source is not configured on this host."); return result; }
  try {
    if (!isAbsolute(root)) throw new Error("Expected an absolute operator-owned root");
    const directory = await realpath(root), raw = await readReportFile(join(directory, "data.json"), REPORT_MAX_BYTES);
    const parsed = tokenomicsDataSchema.safeParse(JSON.parse(raw));
    if (!parsed.success) { result.diagnostics.push("Report does not match consumption/data@2; no totals can be trusted."); return result; }
    result.data = parsed.data;
    const hash = createHash("sha256").update(raw);
    try {
      const liveRaw = await readReportFile(join(directory, "_stats", "live.js"), LIVE_MAX_BYTES);
      result.live = parseLiveAssignment(liveRaw); hash.update(liveRaw);
    } catch { result.diagnostics.push("Private live report is missing, malformed, changed during read, or outside its limit. Code Review and Delivery are unavailable."); }
    result.revision = hash.digest("hex");
    const age = now.getTime() - Date.parse(result.data.generated);
    if (age > STALE_MS) result.diagnostics.push("Tokenomics data is stale (more than 48 hours since generation).");
    if (age < -300_000) result.diagnostics.push("Report generation time is in the future; check the host clock.");
    for (const [label, time] of [["Code Review", result.live.review?.asOf], ["Delivery", result.live.delivery?.asOf]] as const) {
      if (time && (!Number.isFinite(Date.parse(time)) || now.getTime() - Date.parse(time) > STALE_MS)) result.diagnostics.push(`${label} live data is stale or has an unknown observation time.`);
    }
    if (!result.live.review) result.diagnostics.push("Code Review has no collected PR desk state.");
    if (!result.live.delivery) result.diagnostics.push("Delivery has no collected snapshot.");
    if (result.live.delivery && (!result.live.delivery.jira || result.live.delivery.errors.length)) result.diagnostics.push("Delivery coverage is partial; see its collection diagnostics.");
  } catch { result.data = null; result.diagnostics.push("Tokenomics report is unreadable, malformed, changed during read, or outside its limit. Refresh after the collector finishes."); }
  return result;
}

export function tokenomicsView(snapshot: ReportSnapshot, input: TokenomicsQuery): TokenomicsView {
  const q = tokenomicsQuerySchema.parse(input), d = snapshot.data;
  const empty: TokenomicsView = { status: "unavailable", readAt: snapshot.readAt, generated: null, revision: snapshot.revision, diagnostics: snapshot.diagnostics, query: q, options: {}, tiles: [], tables: [], notes: [] };
  if (!d) return empty;
  if (q.from && q.to && q.from > q.to) return { ...empty, diagnostics: [...snapshot.diagnostics, "Custom period start must not be after its end."] };
  if ((q.rev && !d.revs.some((r) => r.id === q.rev)) || (q.compare && !d.revs.some((r) => r.id === q.compare))) return { ...empty, diagnostics: [...snapshot.diagnostics, "Selected revision is no longer present; choose an available check."] };
  const model = usageModel(d, q), result = buildTokenomics(d, snapshot.live, q);
  const options = tokenomicsOptions(d, model);
  if (q.page === "pivot") options.pivotNodes = pivotRows(model, q).filter((r) => r.hasChildren).map((r) => ({ value: r.key, label: r.cells.slice(0, q.groups.length).filter(Boolean).join(" → ") }));
  return { ...empty, status: snapshot.diagnostics.length ? "partial" : "ready", generated: d.generated,
    options, tiles: result.tiles, tables: paginateTokenomics(result, q), notes: result.notes };
}

export function contributeTokenomics(server: PluginServerContext): void {
  const read = async () => { try { return await readTokenomicsSnapshot(await configuredRoot()); } catch { return { data: null, live: tokenomicsLiveSchema.parse({}), revision: null, diagnostics: ["Tokenomics host configuration is invalid or unreadable."], readAt: new Date().toISOString() }; } };
  server.handle(readTokenomics, async (input) => tokenomicsView(await read(), input));
  server.handle(exportTokenomics, async (input) => {
    const snapshot = await read(), view = tokenomicsView(snapshot, input);
    if (!snapshot.data || !snapshot.revision || view.status === "unavailable") throw new Error("Tokenomics export is unavailable; refresh the source status.");
    if ((input.page.startsWith("cr-") && !snapshot.live.review) || (input.page.startsWith("dv-") && !snapshot.live.delivery)) throw new Error("The selected live analytics source is unavailable; no export can be produced.");
    // Exports carry every matching row, independently of the native display page size.
    const q = tokenomicsQuerySchema.parse(input), result = buildTokenomics(snapshot.data, snapshot.live, q);
    const tables = result.tables.map((t) => ({ ...t, rows: t.rows.filter((r) => !q.search || [...r.cells, ...r.detail].join(" ").toLowerCase().includes(q.search.trim().toLowerCase())) }));
    const content = tokenomicsCsv({ ...result, tables });
    if (Buffer.byteLength(content) > LIVE_MAX_BYTES) throw new Error("Export exceeds the bounded response limit; narrow its filters.");
    return { filename: `tokenomics-${q.page}.csv`, content, revision: snapshot.revision };
  });
}
