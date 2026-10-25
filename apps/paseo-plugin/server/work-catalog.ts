import { spawn } from "node:child_process";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import {
  daemonEntriesViewSchema,
  daemonSourcesViewSchema,
  oldestSourceAsOf,
  pendingAssociations,
  readWorkCatalog,
  workCatalogSnapshotSchema,
  type DaemonEntriesView,
  type DaemonSourcesView,
  type WorkCatalogEntry,
  type WorkCatalogRead,
  type WorkCatalogSnapshot,
  type WorkCatalogSource,
  type WorkCatalogView,
} from "../shared/work-catalog";
import { DEFAULT_CREDENTIAL_TIER, readConfig, type KontorPluginConfig } from "./config";

export const WORK_CATALOG_TTL_MS = 30_000;
export const WORK_CATALOG_BUDGET_MS = 5_000;
/** Everything the child may write, frames included. */
export const WORK_CATALOG_OUTPUT_LIMIT = 8 * 1024 * 1024;
/** The plugin's own snapshot bound; a snapshot is a fallback, not a second catalog. */
export const WORK_CATALOG_SNAPSHOT_LIMIT = 8 * 1024 * 1024;
export const WORK_CATALOG_PAGE_LIMIT = 1_000;
export const WORK_CATALOG_MAX_PAGES = 3;
export const WORK_CATALOG_SNAPSHOT_FILE = "work-catalog-snapshot.json";

const SOURCES_TOOL = "kontor_work_catalog_sources";
const ENTRIES_TOOL = "kontor_work_catalog_entries";

type FailureKind = "unavailable" | "incompatible";

class CatalogReadFailure extends Error {
  readonly kind: FailureKind;

  constructor(kind: FailureKind, message: string) {
    super(message);
    this.kind = kind;
  }
}

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);

function parse(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

/**
 * kontor-mcp's tool result for one exact tool: either the daemon envelope
 * ({tool, status, body}) or the observer's own refusal ({tool, code,
 * dispatched: false}). Another tool's document is not an answer.
 */
function envelope(result: unknown, tool: string): unknown {
  if (!isRecord(result)) throw new CatalogReadFailure("incompatible", "the observer answered an unknown tool result");
  const { isError = false, content } = result;
  const block: unknown = Array.isArray(content) && content.length === 1 ? content[0] : undefined;
  if (typeof isError !== "boolean" || !isRecord(block) || block.type !== "text" || typeof block.text !== "string") {
    throw new CatalogReadFailure("incompatible", `the observer did not answer ${tool} with one text document`);
  }
  const document = parse(block.text);
  if (!isRecord(document) || document.tool !== tool) {
    throw new CatalogReadFailure("incompatible", `the observer answered something other than ${tool}`);
  }
  const { status, body } = document;
  const answered = typeof status === "number" && Number.isInteger(status);
  const success = answered && status >= 200 && status < 300;
  if (isError) {
    if (document.dispatched === false) {
      if (document.code === "unavailable") {
        throw new CatalogReadFailure("unavailable", typeof document.rule === "string" ? document.rule : "the observer could not reach the daemon");
      }
      if (document.code === "not_found") {
        throw new CatalogReadFailure("incompatible", `the installed observer does not serve ${tool}; upgrade kontor-mcp`);
      }
      throw new CatalogReadFailure("incompatible", typeof document.rule === "string" ? document.rule : `the observer refused ${tool}`);
    }
    throw new CatalogReadFailure("incompatible", `the daemon refused ${tool}${answered ? ` with status ${status}` : ""}`);
  }
  if (!success || !isRecord(body)) {
    throw new CatalogReadFailure("incompatible", `${tool} did not return a successful catalog document`);
  }
  return body;
}

type Observation = {
  sources: DaemonSourcesView;
  entries: DaemonEntriesView["entries"];
  entriesTruncated: boolean;
};

/**
 * Ask the configured kontor-mcp for the composite source status and the entry
 * pages behind it, over stdio, with the arguments the plugin's observer
 * credential already uses.
 *
 * The child gets only PATH and HOME. It reads its own credential from the state
 * root; the plugin never opens, names or forwards it. The process is killed
 * after one answer, the output limit, the budget or `signal`.
 */
export function observeWorkCatalog(config: KontorPluginConfig, budgetMs = WORK_CATALOG_BUDGET_MS, signal?: AbortSignal): Promise<Observation> {
  if (!config.mcpEnabled) {
    return Promise.reject(new CatalogReadFailure("unavailable", "the plugin's observer MCP surface is switched off"));
  }
  return new Promise((settle, refuse) => {
    const args = ["--state-root", config.stateRoot, "--credential-tier", DEFAULT_CREDENTIAL_TIER, ...(config.serveProfile ? ["--serve-profile", config.serveProfile] : [])];
    const child = spawn(config.mcpCommand, args, {
      env: { PATH: process.env.PATH ?? "", HOME: process.env.HOME ?? "" },
      stdio: ["pipe", "pipe", "ignore"],
      signal,
    });
    let done = false;
    // initialize (1), sources (2), then one entry page per request (3, 4, ...).
    let awaiting = 1;
    let page = 0;
    let cursor: string | null = null;
    let sources: DaemonSourcesView | null = null;
    let entries: DaemonEntriesView["entries"] = [];
    let received = 0;
    let output = Buffer.alloc(0);
    const finish = (failure?: unknown, observation?: Observation) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      child.kill("SIGKILL");
      if (failure !== undefined) refuse(failure);
      else if (observation !== undefined) settle(observation);
      else refuse(new CatalogReadFailure("unavailable", "the observer ended without an answer"));
    };
    const timer = setTimeout(() => finish(new CatalogReadFailure("unavailable", "the observer did not answer within the plugin budget")), budgetMs);
    const send = (message: object) => child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", ...message })}\n`);
    child.on("error", () => finish(new CatalogReadFailure("unavailable", "the observer could not be started")));
    child.on("close", () => finish(new CatalogReadFailure("unavailable", "the observer ended without an answer")));
    child.stdin.on("error", () => {});
    const sendEntries = () => {
      page += 1;
      const id = 2 + page;
      awaiting = id;
      send({
        id,
        method: "tools/call",
        params: {
          name: ENTRIES_TOOL,
          arguments: { limit: WORK_CATALOG_PAGE_LIMIT, ...(cursor === null ? {} : { cursor }) },
        },
      });
    };
    const take = (message: unknown) => {
      if (isRecord(message) && message.jsonrpc === "2.0" && typeof message.method === "string") return;
      if (!isRecord(message) || message.jsonrpc !== "2.0" || message.id !== awaiting || "error" in message || !isRecord(message.result)) {
        return finish(new CatalogReadFailure("incompatible", "the observer answered a frame the plugin did not ask for"));
      }
      try {
        if (awaiting === 1) {
          if (typeof message.result.protocolVersion !== "string") {
            return finish(new CatalogReadFailure("incompatible", "the observer did not complete the MCP handshake"));
          }
          send({ method: "notifications/initialized" });
          awaiting = 2;
          send({ id: 2, method: "tools/call", params: { name: SOURCES_TOOL, arguments: {} } });
          return;
        }
        if (awaiting === 2) {
          sources = daemonSourcesViewSchema.parse(envelope(message.result, SOURCES_TOOL));
          sendEntries();
          return;
        }
        const view = daemonEntriesViewSchema.parse(envelope(message.result, ENTRIES_TOOL));
        entries = [...entries, ...view.entries];
        cursor = view.next_cursor ?? null;
        if (cursor !== null && page < WORK_CATALOG_MAX_PAGES) {
          sendEntries();
          return;
        }
        if (sources === null) {
          return finish(new CatalogReadFailure("incompatible", "the observer answered entries without a source status"));
        }
        finish(undefined, {
          sources,
          entries,
          entriesTruncated: cursor !== null || view.truncated && view.next_cursor === null && page >= WORK_CATALOG_MAX_PAGES,
        });
      } catch (error) {
        finish(error instanceof CatalogReadFailure
          ? error
          : new CatalogReadFailure("incompatible", `the observer's document is not a catalog answer: ${error instanceof Error ? error.message : String(error)}`));
      }
    };
    const read = (chunk: Buffer) => {
      received += chunk.byteLength;
      if (received > WORK_CATALOG_OUTPUT_LIMIT) {
        return finish(new CatalogReadFailure("incompatible", "the observer's answer exceeded the plugin's bounded read"));
      }
      output = Buffer.concat([output, chunk]);
      for (let newline = output.indexOf(10); newline >= 0 && !done; newline = output.indexOf(10)) {
        const line = output.subarray(0, newline).toString("utf8");
        output = output.subarray(newline + 1);
        take(parse(line));
      }
    };
    child.stdout.on("data", (chunk: Buffer) => {
      try {
        read(chunk);
      } catch {
        finish(new CatalogReadFailure("incompatible", "the observer's answer could not be read"));
      }
    });
    send({ id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "kontor-paseo-plugin", version: "0.1.0" } } });
  });
}

function projectSources(sources: DaemonSourcesView): WorkCatalogSource[] {
  const byName = new Map(sources.sources.map((source) => [source.source, source]));
  if (!byName.has("plan") || !byName.has("jira") || !byName.has("native")) {
    throw new CatalogReadFailure("incompatible", "the source status does not name plan, Jira and native exactly once");
  }
  return ["plan", "jira", "native"].map((name) => {
    const source = byName.get(name as "plan" | "jira" | "native")!;
    return {
      source: source.source,
      state: source.state,
      generation: source.generation,
      attemptedGeneration: source.attempted_generation,
      asOf: source.as_of,
      error: source.error,
      retryAt: source.retry_at,
      coverage: source.coverage.map((line) => ({ subject: line.subject, state: line.state, detail: line.detail })),
      conflicts: source.conflicts.map((conflict) => ({ kind: conflict.kind, subject: conflict.subject, message: conflict.message, observedAt: conflict.observed_at })),
      pendingVerification: source.pending_verification,
    };
  });
}

function projectEntry(entry: DaemonEntriesView["entries"][number]): WorkCatalogEntry {
  const unique = (values: readonly string[]) => [...new Set(values)].sort();
  return {
    logicalPlanId: entry.logical_plan_id,
    kind: entry.kind,
    title: entry.title,
    state: entry.state,
    sourceIds: unique(entry.occurrences.flatMap((occurrence) => occurrence.source_ids)),
    epic: entry.occurrences.find((occurrence) => occurrence.content.epic !== null)?.content.epic ?? null,
    workstream: entry.occurrences.find((occurrence) => occurrence.content.workstream !== null)?.content.workstream ?? null,
    repositoryIds: unique(entry.occurrences.map((occurrence) => occurrence.repository_id)),
    relativePaths: unique(entry.occurrences.map((occurrence) => occurrence.relative_path)),
    conflicts: entry.conflicts.map((conflict) => ({ kind: conflict.kind, message: conflict.message, observedAt: conflict.observed_at })),
    pendingVerification: pendingAssociations(entry.associations),
    lastSeen: entry.last_seen,
  };
}

export function projectWorkCatalog(sources: DaemonSourcesView, entries: DaemonEntriesView["entries"], entriesTruncated: boolean): WorkCatalogView {
  const unique = new Map<string, WorkCatalogEntry>();
  for (const entry of entries) {
    const projected = projectEntry(entry);
    unique.set(projected.logicalPlanId, projected);
  }
  return {
    schemaVersion: 1,
    sources: projectSources(sources),
    entries: [...unique.values()].sort((left, right) => left.logicalPlanId.localeCompare(right.logicalPlanId)),
    entriesTruncated,
  };
}

export function workCatalogSnapshotPath(config: KontorPluginConfig): string {
  return join(dirname(config.paseoWorktreesRoot), "plugin-config", "kontor", WORK_CATALOG_SNAPSHOT_FILE);
}

/**
 * Read the plugin's own versioned last-known snapshot. Anything larger, older
 * or malformed is refused rather than trusted; the direct readers remain.
 */
export async function readWorkCatalogSnapshot(path: string): Promise<WorkCatalogSnapshot | null> {
  let bytes: Buffer;
  try {
    bytes = await readFile(path);
  } catch {
    return null;
  }
  if (bytes.byteLength > WORK_CATALOG_SNAPSHOT_LIMIT) return null;
  try {
    return workCatalogSnapshotSchema.parse(JSON.parse(bytes.toString("utf8")));
  } catch {
    return null;
  }
}

/** Write the snapshot atomically. Best effort: a failed write never hides a live catalog. */
export async function writeWorkCatalogSnapshot(path: string, view: WorkCatalogView, savedAt: string): Promise<WorkCatalogSnapshot | null> {
  const snapshot: WorkCatalogSnapshot = {
    schema: "kontor/plugin-work-catalog@1",
    version: 1,
    savedAt,
    sourceAsOf: oldestSourceAsOf(view),
    view,
  };
  const bytes = Buffer.from(JSON.stringify(snapshot), "utf8");
  if (bytes.byteLength > WORK_CATALOG_SNAPSHOT_LIMIT) return null;
  // A slower, older read must not replace a newer last-known snapshot. Equal
  // source instants are the same observation and may be rewritten.
  const existing = await readWorkCatalogSnapshot(path);
  if (existing !== null && existing.sourceAsOf !== null
    && (snapshot.sourceAsOf === null || existing.sourceAsOf > snapshot.sourceAsOf)) {
    return existing;
  }
  const temporary = `${path}.${process.pid}.${Date.now()}.tmp`;
  try {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(temporary, bytes, { mode: 0o600 });
    await rename(temporary, path);
    return snapshot;
  } catch {
    return null;
  }
}

/**
 * Prefer the live catalog through the observer boundary, then fall back to the
 * plugin's own snapshot. `checkedAt` is when the plugin asked; every displayed
 * freshness instant is a source's own completion instant.
 */
export async function loadWorkCatalog(
  config: KontorPluginConfig = readConfig(),
  options: {
    budgetMs?: number;
    signal?: AbortSignal;
    now?: () => number;
    snapshotPath?: string;
    observe?: (config: KontorPluginConfig, budgetMs: number, signal?: AbortSignal) => Promise<Observation>;
  } = {},
): Promise<WorkCatalogRead> {
  const now = options.now ?? Date.now;
  const checkedAt = new Date(now()).toISOString();
  const snapshotPath = options.snapshotPath ?? workCatalogSnapshotPath(config);
  try {
    const observation = await (options.observe ?? observeWorkCatalog)(config, options.budgetMs ?? WORK_CATALOG_BUDGET_MS, options.signal);
    const view = projectWorkCatalog(observation.sources, observation.entries, observation.entriesTruncated);
    const sourceAsOf = oldestSourceAsOf(view);
    const written = await writeWorkCatalogSnapshot(snapshotPath, view, checkedAt);
    const kept = written !== null && written.sourceAsOf === sourceAsOf;
    return {
      mode: "catalog",
      checkedAt,
      sourceAsOf,
      view,
      error: null,
      snapshot: kept ? { savedAt: written.savedAt, sourceAsOf: written.sourceAsOf } : null,
    };
  } catch (error) {
    const failure = error instanceof CatalogReadFailure ? error : new CatalogReadFailure("unavailable", error instanceof Error ? error.message : String(error));
    const snapshot = await readWorkCatalogSnapshot(snapshotPath);
    if (snapshot !== null) {
      return {
        mode: "snapshot",
        checkedAt,
        sourceAsOf: snapshot.sourceAsOf,
        view: snapshot.view,
        error: failure.message,
        snapshot: { savedAt: snapshot.savedAt, sourceAsOf: snapshot.sourceAsOf },
      };
    }
    return { mode: "unavailable", checkedAt, sourceAsOf: null, view: null, error: failure.message, snapshot: null };
  }
}

/** Register the catalog read whatever the floor; it only observes. The disposer aborts a read still in flight. */
export function contributeWorkCatalog(server: Pick<PluginServerContext, "handle">): () => void {
  const stop = new AbortController();
  server.handle(readWorkCatalog, () => loadWorkCatalog(readConfig(), { signal: stop.signal }));
  return () => stop.abort();
}
