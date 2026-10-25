import { spawn } from "node:child_process";
import { mkdir, readFile, rename, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import {
  daemonEntriesViewSchema,
  daemonNativeInventorySchema,
  daemonNativeStatusSchema,
  daemonSourcesViewSchema,
  oldestSourceAsOf,
  pendingAssociations,
  readWorkCatalog,
  workCatalogSnapshotSchema,
  workCatalogSnapshotV2Schema,
  type DaemonEntriesView,
  type DaemonNativeInventory,
  type DaemonSourcesView,
  type WorkCatalogEntry,
  type WorkCatalogNativeRead,
  type WorkCatalogNativeView,
  type WorkCatalogRead,
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
const NATIVE_STATUS_TOOL = "kontor_work_catalog_native_status";
const NATIVE_INVENTORY_TOOL = "kontor_work_catalog_native_inventory";

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

type NativeObservation = {
  mode: "catalog" | "unavailable";
  runtimeInstance: string | null;
  view: WorkCatalogNativeView | null;
  error: string | null;
};

type Observation = {
  sources: DaemonSourcesView;
  entries: DaemonEntriesView["entries"];
  entriesTruncated: boolean;
  native: NativeObservation;
};

type JsonRpcPending = {
  resolve: (value: unknown) => void;
  reject: (reason: unknown) => void;
};

type JsonRpcSession = {
  request(method: string, params: object): Promise<unknown>;
  notify(method: string, params?: object): void;
  close(): void;
};

/**
 * One bounded JSON-RPC session over the observer process. Every response is
 * matched to its request id: another tool's document, a late frame or an
 * unexpected notification is not an answer. The child gets only PATH and HOME,
 * reads its own credential, and is killed after close, the budget, the output
 * bound or `signal`.
 */
function startObserver(config: KontorPluginConfig, budgetMs: number, signal?: AbortSignal): JsonRpcSession {
  const args = ["--state-root", config.stateRoot, "--credential-tier", DEFAULT_CREDENTIAL_TIER, ...(config.serveProfile ? ["--serve-profile", config.serveProfile] : [])];
  const child = spawn(config.mcpCommand, args, {
    env: { PATH: process.env.PATH ?? "", HOME: process.env.HOME ?? "" },
    stdio: ["pipe", "pipe", "ignore"],
    signal,
  });
  let closed = false;
  let nextId = 1;
  let received = 0;
  let output = Buffer.alloc(0);
  let timer: ReturnType<typeof setTimeout> | null = null;
  const pending = new Map<number, JsonRpcPending>();

  const fail = (error: unknown) => {
    if (closed) return;
    closed = true;
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
    child.kill("SIGKILL");
    for (const request of pending.values()) request.reject(error);
    pending.clear();
  };
  timer = setTimeout(() => fail(new CatalogReadFailure("unavailable", "the observer did not answer within the plugin budget")), budgetMs);
  child.on("error", () => fail(new CatalogReadFailure("unavailable", "the observer could not be started")));
  child.on("close", () => fail(new CatalogReadFailure("unavailable", "the observer ended without an answer")));
  child.stdin.on("error", () => {});
  const send = (message: object) => {
    if (closed) return;
    child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", ...message })}\n`);
  };
  const take = (message: unknown) => {
    if (isRecord(message) && message.jsonrpc === "2.0" && typeof message.method === "string" && message.id === undefined) return;
    if (!isRecord(message) || message.jsonrpc !== "2.0" || typeof message.id !== "number") {
      return fail(new CatalogReadFailure("incompatible", "the observer answered a frame the plugin did not ask for"));
    }
    const request = pending.get(message.id);
    if (!request) {
      return fail(new CatalogReadFailure("incompatible", "the observer answered a frame the plugin did not ask for"));
    }
    pending.delete(message.id);
    if ("error" in message) {
      const detail = isRecord(message.error) && typeof message.error.message === "string" ? message.error.message : "the observer refused the request";
      return request.reject(new CatalogReadFailure("incompatible", detail));
    }
    if (!isRecord(message.result)) {
      return request.reject(new CatalogReadFailure("incompatible", "the observer returned no result document"));
    }
    request.resolve(message.result);
  };
  const read = (chunk: Buffer) => {
    received += chunk.byteLength;
    if (received > WORK_CATALOG_OUTPUT_LIMIT) {
      return fail(new CatalogReadFailure("incompatible", "the observer's answer exceeded the plugin's bounded read"));
    }
    output = Buffer.concat([output, chunk]);
    for (let newline = output.indexOf(10); newline >= 0 && !closed; newline = output.indexOf(10)) {
      const line = output.subarray(0, newline).toString("utf8");
      output = output.subarray(newline + 1);
      take(parse(line));
    }
  };
  child.stdout.on("data", (chunk: Buffer) => {
    try {
      read(chunk);
    } catch {
      fail(new CatalogReadFailure("incompatible", "the observer's answer could not be read"));
    }
  });
  return {
    request(method, params) {
      if (closed) return Promise.reject(new CatalogReadFailure("unavailable", "the observer session is closed"));
      const id = nextId;
      nextId += 1;
      return new Promise((resolve, reject) => {
        pending.set(id, { resolve, reject });
        send({ id, method, params });
      });
    },
    notify(method, params) {
      send({ method, ...(params === undefined ? {} : { params }) });
    },
    close() {
      if (closed) return;
      closed = true;
      if (timer !== null) {
        clearTimeout(timer);
        timer = null;
      }
      child.kill("SIGKILL");
      pending.clear();
    },
  };
}

async function callTool(session: JsonRpcSession, tool: string, args: object): Promise<unknown> {
  return envelope(await session.request("tools/call", { name: tool, arguments: args }), tool);
}

async function observeEntries(session: JsonRpcSession): Promise<{ entries: DaemonEntriesView["entries"]; truncated: boolean }> {
  let cursor: string | null = null;
  let page = 0;
  let generation: number | null = null;
  let entries: DaemonEntriesView["entries"] = [];
  for (;;) {
    const view = daemonEntriesViewSchema.parse(await callTool(session, ENTRIES_TOOL, {
      limit: WORK_CATALOG_PAGE_LIMIT,
      ...(cursor === null ? {} : { cursor }),
    }));
    if (generation === null) generation = view.generation;
    else if (generation !== view.generation) throw new CatalogReadFailure("incompatible", "entry pages span two catalog generations");
    page += 1;
    entries = [...entries, ...view.entries];
    const next = view.next_cursor ?? null;
    if (next !== null && page < WORK_CATALOG_MAX_PAGES) {
      cursor = next;
      continue;
    }
    return { entries, truncated: next !== null || (view.truncated && next === null && page >= WORK_CATALOG_MAX_PAGES) };
  }
}

function nativeUnavailable(error: string): NativeObservation {
  return { mode: "unavailable", runtimeInstance: null, view: null, error };
}

function projectNativeInventory(pages: readonly DaemonNativeInventory[], runtimeInstance: string): WorkCatalogNativeView {
  const first = pages[0];
  if (!first) throw new CatalogReadFailure("incompatible", "the native inventory returned no pages");
  const generation = first.generation;
  const asOf = first.as_of;
  const source = first.source;
  const archiveScope = first.archive_scope;
  const projects = new Map<string, WorkCatalogNativeView["projects"][number]>();
  const workspaces = new Map<string, WorkCatalogNativeView["workspaces"][number]>();
  const sessions = new Map<string, WorkCatalogNativeView["sessions"][number]>();
  let truncated = false;
  for (const [index, page] of pages.entries()) {
    if (page.generation !== generation || page.as_of !== asOf || page.source !== source || page.archive_scope !== archiveScope) {
      throw new CatalogReadFailure("incompatible", "native inventory pages span two generations");
    }
    for (const project of page.projects) {
      if (project.runtime_instance !== runtimeInstance) throw new CatalogReadFailure("incompatible", "the native inventory mixed runtime instances");
      if (project.state === "active") projects.set(project.project_id, {
        projectId: project.project_id,
        displayName: project.display_name,
        customName: project.custom_name,
        projectKey: project.project_key,
        rootPath: project.root_path,
        projectKind: project.project_kind,
      });
    }
    for (const workspace of page.workspaces) {
      if (workspace.runtime_instance !== runtimeInstance) throw new CatalogReadFailure("incompatible", "the native inventory mixed runtime instances");
      if (workspace.state === "active") workspaces.set(workspace.workspace_id, {
        workspaceId: workspace.workspace_id,
        projectId: workspace.project_id,
        name: workspace.name,
        title: workspace.title,
        directory: workspace.directory,
        kind: workspace.kind,
        archivingAt: workspace.archiving_at,
        worktreeSlug: workspace.worktree_slug,
        projectDisplayName: workspace.project_display_name,
        projectCustomName: workspace.project_custom_name,
        projectRootPath: workspace.project_root_path,
        projectKind: workspace.project_kind,
      });
    }
    for (const session of page.sessions) {
      if (session.runtime_instance !== runtimeInstance) throw new CatalogReadFailure("incompatible", "the native inventory mixed runtime instances");
      if (session.state === "active") sessions.set(session.session_id, {
        sessionId: session.session_id,
        workspaceId: session.workspace_id,
        projectId: session.project_id,
        projectName: session.project_name,
        workspaceName: session.workspace_name,
        title: session.title,
        provider: session.provider,
        model: session.model,
        status: session.status,
        archivedAt: session.archived_at,
        attentionReason: session.attention_reason,
        cwd: session.cwd,
        createdAt: session.created_at,
        updatedAt: session.updated_at,
        lastUserMessageAt: session.last_user_message_at,
        parentSessionId: session.parent_session_id,
        checkout: session.checkout === null ? null : {
          cwd: session.checkout.cwd,
          branch: session.checkout.branch,
          worktreeRoot: session.checkout.worktree_root,
          isGit: session.checkout.is_git,
          isPaseoOwnedWorktree: session.checkout.is_paseo_owned_worktree,
        },
      });
    }
    if (index === pages.length - 1) {
      const next = page.next_cursor ?? null;
      truncated = next !== null || (page.truncated && next === null);
    }
  }
  return {
    runtimeInstance,
    generation,
    asOf,
    source,
    archiveScope,
    truncated,
    projects: [...projects.values()],
    workspaces: [...workspaces.values()],
    sessions: [...sessions.values()],
  };
}

/** The native inventory is optional: its failure must not hide the plan catalog. */
async function observeNative(session: JsonRpcSession, hostId: string): Promise<NativeObservation> {
  try {
    const status = daemonNativeStatusSchema.parse(await callTool(session, NATIVE_STATUS_TOOL, {}));
    const matches = status.collection.coverage.filter((runtime) => runtime.server_id === hostId);
    if (matches.length !== 1) {
      return nativeUnavailable(matches.length === 0
        ? "No catalog native runtime reports this Paseo host's exact server id; the direct session directory stays active."
        : "More than one catalog native runtime reports this Paseo host's exact server id; the catalog inventory is ambiguous.");
    }
    if (status.generation === 0 || status.as_of === null) {
      return nativeUnavailable("The catalog native census has no complete generation yet; the direct session directory stays active.");
    }
    const runtimeInstance = matches[0]!.runtime_instance;
    const pages: DaemonNativeInventory[] = [];
    let cursor: string | null = null;
    for (let page = 0; page < WORK_CATALOG_MAX_PAGES; page += 1) {
      const view = daemonNativeInventorySchema.parse(await callTool(session, NATIVE_INVENTORY_TOOL, {
        instance: runtimeInstance,
        include_removed: false,
        limit: WORK_CATALOG_PAGE_LIMIT,
        ...(cursor === null ? {} : { cursor }),
      }));
      pages.push(view);
      const next = view.next_cursor ?? null;
      if (next === null || page === WORK_CATALOG_MAX_PAGES - 1) break;
      cursor = next;
    }
    return {
      mode: "catalog",
      runtimeInstance,
      view: projectNativeInventory(pages, runtimeInstance),
      error: null,
    };
  } catch (error) {
    return nativeUnavailable(error instanceof CatalogReadFailure ? error.message : error instanceof Error ? error.message : String(error));
  }
}

/**
 * Ask the configured kontor-mcp for the composite source status, the plan entry
 * pages, and the selected Paseo host's native inventory, over stdio.
 *
 * The child gets only PATH and HOME. It reads its own credential from the state
 * root; the plugin never opens, names or forwards it. The process is killed
 * after one answer, the output limit, the budget or `signal`.
 */
export async function observeWorkCatalog(config: KontorPluginConfig, hostId: string, budgetMs = WORK_CATALOG_BUDGET_MS, signal?: AbortSignal): Promise<Observation> {
  if (!config.mcpEnabled) {
    throw new CatalogReadFailure("unavailable", "the plugin's observer MCP surface is switched off");
  }
  const session = startObserver(config, budgetMs, signal);
  try {
    const initialized = await session.request("initialize", { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "kontor-paseo-plugin", version: "0.1.0" } });
    if (!isRecord(initialized) || typeof initialized.protocolVersion !== "string") {
      throw new CatalogReadFailure("incompatible", "the observer did not complete the MCP handshake");
    }
    session.notify("notifications/initialized");
    const sources = daemonSourcesViewSchema.parse(await callTool(session, SOURCES_TOOL, {}));
    // Read the plan catalog before the optional native inventory: a slow or
    // missing native tool must not delay or hide the plan catalog answer.
    const entries = await observeEntries(session);
    const native = await observeNative(session, hostId);
    return { sources, entries: entries.entries, entriesTruncated: entries.truncated, native };
  } finally {
    session.close();
  }
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

export type WorkCatalogSnapshotEntry = {
  /** 1 is the legacy plan-only snapshot; 2 adds the host and native inventory. */
  readonly version: 1 | 2;
  readonly savedAt: string;
  readonly sourceAsOf: string | null;
  /** Null only for a legacy v1 snapshot. */
  readonly hostId: string | null;
  readonly runtimeInstance: string | null;
  readonly nativeAsOf: string | null;
  readonly native: WorkCatalogNativeView | null;
  readonly view: WorkCatalogView;
};

/**
 * Read the plugin's own versioned last-known snapshot. A v2 snapshot is bound
 * to its host and is refused for another host; a v1 snapshot remains readable
 * for the plan view and never claims a native inventory. Anything larger or
 * malformed is refused rather than trusted.
 */
export async function readWorkCatalogSnapshot(path: string, hostId?: string): Promise<WorkCatalogSnapshotEntry | null> {
  let bytes: Buffer;
  try {
    bytes = await readFile(path);
  } catch {
    return null;
  }
  if (bytes.byteLength > WORK_CATALOG_SNAPSHOT_LIMIT) return null;
  let raw: unknown;
  try {
    raw = JSON.parse(bytes.toString("utf8"));
  } catch {
    return null;
  }
  const v2 = workCatalogSnapshotV2Schema.safeParse(raw);
  if (v2.success) {
    if (hostId !== undefined && v2.data.hostId !== hostId) return null;
    return {
      version: 2,
      savedAt: v2.data.savedAt,
      sourceAsOf: v2.data.sourceAsOf,
      hostId: v2.data.hostId,
      runtimeInstance: v2.data.runtimeInstance,
      nativeAsOf: v2.data.nativeAsOf,
      native: v2.data.native,
      view: v2.data.view,
    };
  }
  const v1 = workCatalogSnapshotSchema.safeParse(raw);
  if (v1.success) {
    return {
      version: 1,
      savedAt: v1.data.savedAt,
      sourceAsOf: v1.data.sourceAsOf,
      hostId: null,
      runtimeInstance: null,
      nativeAsOf: null,
      native: null,
      view: v1.data.view,
    };
  }
  return null;
}

export type WorkCatalogSnapshotWrite = {
  readonly hostId: string;
  readonly view: WorkCatalogView;
  readonly native: WorkCatalogNativeView | null;
  readonly savedAt: string;
};

/**
 * Write snapshot v2 atomically. A slower older plan or native read cannot
 * replace a newer last-known one, and a native failure preserves the existing
 * native inventory for the same host rather than erasing it. A different host
 * replaces the file rather than mixing hosts.
 */
export async function writeWorkCatalogSnapshot(path: string, input: WorkCatalogSnapshotWrite): Promise<WorkCatalogSnapshotEntry | null> {
  const existing = await readWorkCatalogSnapshot(path);
  const sameHost = existing?.version === 2 && existing.hostId === input.hostId ? existing : null;
  const sourceAsOf = oldestSourceAsOf(input.view);
  const keepPlan = sameHost !== null && sameHost.sourceAsOf !== null
    && (sourceAsOf === null || sameHost.sourceAsOf > sourceAsOf);
  const nativeAsOf = input.native?.asOf ?? null;
  const keepNative = sameHost !== null && sameHost.native !== null
    && (input.native === null || (sameHost.nativeAsOf !== null && (nativeAsOf === null || sameHost.nativeAsOf > nativeAsOf)));
  const view = keepPlan ? sameHost.view : input.view;
  const effectiveSourceAsOf = keepPlan ? sameHost.sourceAsOf : sourceAsOf;
  const native = keepNative ? sameHost.native : input.native;
  const effectiveNativeAsOf = keepNative ? sameHost.nativeAsOf : nativeAsOf;
  const snapshot = {
    schema: "kontor/plugin-work-catalog@2" as const,
    version: 2 as const,
    savedAt: input.savedAt,
    sourceAsOf: effectiveSourceAsOf,
    hostId: input.hostId,
    runtimeInstance: native?.runtimeInstance ?? null,
    nativeAsOf: effectiveNativeAsOf,
    native,
    view,
  };
  const bytes = Buffer.from(JSON.stringify(snapshot), "utf8");
  if (bytes.byteLength > WORK_CATALOG_SNAPSHOT_LIMIT) return null;
  const temporary = `${path}.${process.pid}.${Date.now()}.tmp`;
  try {
    await mkdir(dirname(path), { recursive: true });
    await writeFile(temporary, bytes, { mode: 0o600 });
    await rename(temporary, path);
    return {
      version: 2,
      savedAt: snapshot.savedAt,
      sourceAsOf: snapshot.sourceAsOf,
      hostId: snapshot.hostId,
      runtimeInstance: snapshot.runtimeInstance,
      nativeAsOf: snapshot.nativeAsOf,
      native: snapshot.native,
      view: snapshot.view,
    };
  } catch {
    return null;
  }
}

function nativeRead(input: {
  mode: WorkCatalogNativeRead["mode"];
  hostId: string;
  runtimeInstance: string | null;
  checkedAt: string;
  view: WorkCatalogNativeView | null;
  error: string | null;
  snapshotSavedAt: string | null;
}): WorkCatalogNativeRead {
  return input;
}

/**
 * Prefer the live catalog through the observer boundary, then fall back to the
 * plugin's own snapshot. `checkedAt` is when the plugin asked; every displayed
 * freshness instant is a source's own completion instant. Native inventory is
 * independent: a missing native runtime never hides a usable plan catalog.
 */
export async function loadWorkCatalog(
  config: KontorPluginConfig = readConfig(),
  options: {
    hostId: string;
    budgetMs?: number;
    signal?: AbortSignal;
    now?: () => number;
    snapshotPath?: string;
    observe?: (config: KontorPluginConfig, hostId: string, budgetMs: number, signal?: AbortSignal) => Promise<Observation>;
  },
): Promise<WorkCatalogRead> {
  const now = options.now ?? Date.now;
  const checkedAt = new Date(now()).toISOString();
  const snapshotPath = options.snapshotPath ?? workCatalogSnapshotPath(config);
  try {
    const observation = await (options.observe ?? observeWorkCatalog)(config, options.hostId, options.budgetMs ?? WORK_CATALOG_BUDGET_MS, options.signal);
    const view = projectWorkCatalog(observation.sources, observation.entries, observation.entriesTruncated);
    const sourceAsOf = oldestSourceAsOf(view);
    const written = await writeWorkCatalogSnapshot(snapshotPath, {
      hostId: options.hostId,
      view,
      native: observation.native.view,
      savedAt: checkedAt,
    });
    const kept = written !== null && written.sourceAsOf === sourceAsOf;
    const native = observation.native.view !== null
      ? nativeRead({ mode: "catalog", hostId: options.hostId, runtimeInstance: observation.native.runtimeInstance, checkedAt, view: observation.native.view, error: observation.native.error, snapshotSavedAt: written?.native === null ? null : written?.savedAt ?? null })
      : written?.native !== null && written?.native !== undefined
        ? nativeRead({ mode: "snapshot", hostId: options.hostId, runtimeInstance: written.runtimeInstance, checkedAt, view: written.native, error: observation.native.error, snapshotSavedAt: written.savedAt })
        : nativeRead({ mode: "unavailable", hostId: options.hostId, runtimeInstance: null, checkedAt, view: null, error: observation.native.error, snapshotSavedAt: null });
    return {
      mode: "catalog",
      checkedAt,
      sourceAsOf,
      view,
      error: null,
      snapshot: kept ? { savedAt: written.savedAt, sourceAsOf: written.sourceAsOf } : null,
      native,
    };
  } catch (error) {
    const failure = error instanceof CatalogReadFailure ? error : new CatalogReadFailure("unavailable", error instanceof Error ? error.message : String(error));
    const snapshot = await readWorkCatalogSnapshot(snapshotPath, options.hostId);
    if (snapshot !== null) {
      return {
        mode: "snapshot",
        checkedAt,
        sourceAsOf: snapshot.sourceAsOf,
        view: snapshot.view,
        error: failure.message,
        snapshot: { savedAt: snapshot.savedAt, sourceAsOf: snapshot.sourceAsOf },
        native: snapshot.native !== null
          ? nativeRead({ mode: "snapshot", hostId: options.hostId, runtimeInstance: snapshot.runtimeInstance, checkedAt, view: snapshot.native, error: failure.message, snapshotSavedAt: snapshot.savedAt })
          : nativeRead({ mode: "unavailable", hostId: options.hostId, runtimeInstance: null, checkedAt, view: null, error: `${failure.message} No native snapshot for this host is stored.`, snapshotSavedAt: null }),
      };
    }
    return {
      mode: "unavailable",
      checkedAt,
      sourceAsOf: null,
      view: null,
      error: failure.message,
      snapshot: null,
      native: nativeRead({ mode: "unavailable", hostId: options.hostId, runtimeInstance: null, checkedAt, view: null, error: failure.message, snapshotSavedAt: null }),
    };
  }
}

/** Register the catalog read whatever the floor; it only observes. The disposer aborts a read still in flight. */
export function contributeWorkCatalog(server: Pick<PluginServerContext, "handle">): () => void {
  const stop = new AbortController();
  server.handle(readWorkCatalog, (input) => loadWorkCatalog(readConfig(), { hostId: input.hostId, signal: stop.signal }));
  return () => stop.abort();
}
