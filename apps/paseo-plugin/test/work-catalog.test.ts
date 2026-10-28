import { chmod, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readConfig } from "../server/config";
import {
  loadWorkCatalog,
  observeWorkCatalog,
  readWorkCatalogSnapshot,
  workCatalogSnapshotPath,
  writeWorkCatalogSnapshot,
  workCatalogReader,
  WORK_CATALOG_MAX_PAGES,
  WORK_CATALOG_NATIVE_MAX_PAGES,
  WORK_CATALOG_OUTPUT_LIMIT,
  WORK_CATALOG_SNAPSHOT_FILE,
  WORK_CATALOG_SNAPSHOT_LIMIT,
} from "../server/work-catalog";

const temporary: string[] = [];
afterAll(() => Promise.all(temporary.map((directory) => rm(directory, { recursive: true, force: true }))));
// The observer budget is wall-clock. These tests do not exercise it, so keep
// its timer from deciding a pass or fail under host load.
beforeEach(() => vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] }));
afterEach(() => vi.useRealTimers());

const AS_OF = "2026-10-25T06:14:01Z";
const HOST_ID = "srv_host_a";
const RUNTIME = "paseo:ws://127.0.0.1:6767/ws";
/** A fixed fixture size, independent of the mutable output-limit constant. */
const FRAME_OVER_LIMIT = 8 * 1024 * 1024 + 1_024;

const nativeProject = {
  runtime_instance: RUNTIME, project_id: "project-1", display_name: "Project One", custom_name: null,
  project_key: "project-1", root_path: "/work", project_kind: "git", state: "active", first_seen: AS_OF, last_seen: AS_OF,
};
const nativeWorkspace = {
  runtime_instance: RUNTIME, workspace_id: "workspace-1", project_id: "project-1", name: "Workspace One",
  title: null, directory: "/work", kind: "checkout", archiving_at: null, worktree_slug: null,
  project_display_name: "Project One", project_custom_name: null, project_root_path: "/work",
  project_kind: "git", state: "active", first_seen: AS_OF, last_seen: AS_OF,
};
const nativeSession = (id: string, overrides: Record<string, unknown> = {}) => ({
  runtime_instance: RUNTIME, session_id: id, workspace_id: "workspace-1", project_id: "project-1",
  project_name: "Project One", workspace_name: "Workspace One", title: `Session ${id}`,
  provider: "claude", model: "sonnet", status: "idle", archived_at: null, attention_reason: null,
  cwd: "/work", created_at: AS_OF, updated_at: AS_OF, last_user_message_at: null,
  parent_session_id: null, checkout: null, state: "active", first_seen: AS_OF, last_seen: AS_OF,
  ...overrides,
});
const nativeStatus = (overrides: Record<string, unknown> = {}) => ({
  schema_version: 1, generation: 4, attempted_generation: 5, as_of: AS_OF,
  collection: {
    state: "complete", include_archived: true,
    coverage: [{
      runtime_instance: RUNTIME, client_id: "plane-census", server_id: HOST_ID, server_version: "0.11.4",
      include_archived: true, state: "scanned", projects: 1, workspaces: 1, sessions: 3, archived_sessions: 1, error: null,
    }],
  },
  ...overrides,
});
const nativePage = (overrides: Record<string, unknown> = {}) => ({
  schema_version: 1, generation: 4, as_of: AS_OF, source: "database", archive_scope: true,
  projects: [nativeProject], workspaces: [nativeWorkspace],
  sessions: [nativeSession("session-a"), nativeSession("session-b", { archived_at: AS_OF, attention_reason: "permission" })],
  truncated: false,
  ...overrides,
});

const coverage = (subject: string, detail: string) => ({ subject, state: "complete", detail });
const source = (name: "plan" | "jira" | "native", overrides: Record<string, unknown> = {}) => ({
  source: name,
  state: "complete",
  generation: 4,
  attempted_generation: 5,
  as_of: AS_OF,
  error: null,
  retry_at: null,
  coverage: [coverage(name === "plan" ? "collection" : name === "jira" ? "scope" : "archive coverage", `${name} read`)],
  conflicts: name === "jira" ? [{ kind: "moved_key", subject: "ASMA-1", message: "ASMA-1 moved to ASMA-2", observed_at: AS_OF }] : [],
  pending_verification: name === "plan" ? 3 : 0,
  ...overrides,
});

const entry = (id: string, sourceId: string | null, overrides: Record<string, unknown> = {}) => ({
  logical_plan_id: id,
  kind: "plan",
  title: `Plan ${id}`,
  state: "active",
  aliases: [],
  associations: sourceId === null ? [] : [{
    relation: "workstream", value: "kontor", kind: "plan_declaration", source_id: sourceId,
    verified: false, dangling: false, note: null,
  }],
  occurrences: [{
    occurrence_id: `occ-${id}`, repository_id: "repo-1", repository_root: "/repo",
    relative_path: `_docs/plans/${id}.md`, content_kind: "tracked", revision_kind: "working_copy",
    content: { revision_id: "a".repeat(64), byte_length: 10, modified_at: AS_OF, title: `Plan ${id}`, epic: "ASMA-1", workstream: "kontor" },
    source_ids: sourceId === null ? [] : [sourceId], expected_sha256: null, pin_matches: null,
    state: "active", first_seen: AS_OF, last_seen: AS_OF,
  }],
  diagnostics: [], conflicts: [],
  first_seen: AS_OF, last_seen: AS_OF,
  ...overrides,
});

const firstPage = {
  schema_version: 1, generation: 4, as_of: AS_OF, source: "database",
  entries: [entry("plan-a", "source-a"), entry("plan-b", null)],
  truncated: true, next_cursor: "v1:4:2:abcdef0123456789",
};
const secondPage = {
  schema_version: 1, generation: 4, as_of: AS_OF, source: "database",
  entries: [entry("plan-c", "source-c")], truncated: false,
};

type FakeOptions = {
  sources?: unknown;
  pages?: Record<string, unknown>;
  nativeStatus?: unknown;
  nativePages?: Record<string, unknown>;
  refuseSources?: boolean;
  refuseNative?: boolean;
  silent?: boolean;
  staleCursorOnce?: boolean;
  unterminatedNative?: boolean;
};

async function fakeMcp(options: FakeOptions = {}) {
  const directory = await mkdtemp(join(tmpdir(), "kontor-work-catalog-"));
  temporary.push(directory);
  const command = join(directory, "kontor-mcp");
  const log = join(directory, "starts");
  const sources = options.sources ?? { schema_version: 1, sources: [source("plan"), source("jira"), source("native")] };
  const pages = options.pages ?? { first: firstPage, "v1:4:2:abcdef0123456789": secondPage };
  await writeFile(command, `#!/usr/bin/env node
const fs = require("node:fs");
fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify({ args: process.argv.slice(2), env: Object.keys(process.env) }) + "\\n");
const sources = ${JSON.stringify(sources)};
const pages = ${JSON.stringify(pages)};
const nativeStatus = ${JSON.stringify(options.nativeStatus ?? nativeStatus())};
const nativePages = ${JSON.stringify(options.nativePages ?? { first: nativePage() })};
const refuseSources = ${options.refuseSources === true};
const refuseNative = ${options.refuseNative === true};
const staleCursorOnce = ${options.staleCursorOnce === true};
const unterminatedNative = ${options.unterminatedNative === true};
let staleSent = false;
const text = (document, isError) => ({ content: [{ type: "text", text: JSON.stringify(document) }], ...(isError ? { isError: true } : {}) });
const envelope = (tool, body) => text({ tool, status: 200, body });
let buffer = "";
process.stdin.setEncoding("utf8").on("data", (chunk) => {
  if (${options.silent === true}) return;
  buffer += chunk;
  for (let newline = buffer.indexOf("\\n"); newline >= 0; newline = buffer.indexOf("\\n")) {
    const message = JSON.parse(buffer.slice(0, newline));
    buffer = buffer.slice(newline + 1);
    if (message.id === undefined) continue;
    if (message.method === "initialize") {
      process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result: { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: "fake", version: "0" } } }) + "\\n");
      continue;
    }
    const name = message.params.name;
    if (name === "kontor_work_catalog_sources") {
      const result = refuseSources
        ? text({ tool: name, code: "not_found", rule: "no such tool", dispatched: false }, true)
        : envelope(name, sources);
      process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }) + "\\n");
      continue;
    }
    if (name === "kontor_work_catalog_entries") {
      const key = message.params.arguments.cursor ?? "first";
      if (staleCursorOnce && key !== "first" && !staleSent) {
        staleSent = true;
        process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result: text({ tool: name, status: 410, body: { code: "stale_cursor" } }, true) }) + "\\n");
        continue;
      }
      process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result: envelope(name, pages[key]) }) + "\\n");
      continue;
    }
    if (name === "kontor_work_catalog_native_status") {
      const result = refuseNative
        ? text({ tool: name, code: "not_found", rule: "no such tool", dispatched: false }, true)
        : envelope(name, nativeStatus);
      process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }) + "\\n");
      continue;
    }
    if (name === "kontor_work_catalog_native_inventory") {
      if (unterminatedNative) {
        process.stdout.write("x".repeat(${FRAME_OVER_LIMIT}));
        continue;
      }
      const key = message.params.arguments.cursor ?? "first";
      const result = refuseNative
        ? text({ tool: name, code: "not_found", rule: "no such tool", dispatched: false }, true)
        : envelope(name, nativePages[key]);
      process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }) + "\\n");
    }
  }
});
setInterval(() => {}, 60_000);
`);
  await chmod(command, 0o700);
  return { command, starts: async () => (await readFile(log, "utf8").catch(() => "")).split("\n").filter(Boolean).map((line) => JSON.parse(line) as { args: string[]; env: string[] }) };
}

async function sandbox() {
  const paseoHome = await mkdtemp(join(tmpdir(), "kontor-work-catalog-home-"));
  temporary.push(paseoHome);
  const configDirectory = join(paseoHome, "plugin-config", "kontor");
  await mkdir(configDirectory, { recursive: true });
  return { paseoHome, configDirectory };
}

const configFor = (command: string, paseoHome: string, extra: Record<string, string> = {}) =>
  readConfig({ HOME: "/home/test", PASEO_HOME: paseoHome, KONTOR_PLUGIN_STATE_ROOT: "/srv/kontor/state", KONTOR_PLUGIN_MCP_COMMAND: command, ...extra });

describe("the plugin's shared catalog read", () => {
  it("reads sources and entry pages through the observer boundary and writes a versioned snapshot", async () => {
    process.env.KONTOR_AUTH = "planted";
    const fake = await fakeMcp();
    const { paseoHome, configDirectory } = await sandbox();
    const config = configFor(fake.command, paseoHome);
    const read = await loadWorkCatalog(config, { hostId: HOST_ID, now: () => Date.parse(AS_OF) });

    expect(read.mode).toBe("catalog");
    expect(read.checkedAt).toBe(new Date(Date.parse(AS_OF)).toISOString());
    expect(read.sourceAsOf).toBe(AS_OF);
    expect(read.error).toBeNull();
    expect(read.view?.sources.map((value) => value.source)).toEqual(["plan", "jira", "native"]);
    expect(read.view?.sources[0]).toMatchObject({ state: "complete", generation: 4, asOf: AS_OF, pendingVerification: 3 });
    expect(read.view?.sources[1]?.conflicts[0]).toMatchObject({ kind: "moved_key", subject: "ASMA-1" });
    expect(read.view?.entries.map((value) => value.logicalPlanId)).toEqual(["plan-a", "plan-b", "plan-c"]);
    expect(read.view?.entries.find((value) => value.logicalPlanId === "plan-a")).toMatchObject({
      sourceIds: ["source-a"], epic: "ASMA-1", workstream: "kontor", pendingVerification: 1,
    });
    expect(read.view?.entriesTruncated).toBe(false);
    expect(read.snapshot).toEqual({ savedAt: new Date(Date.parse(AS_OF)).toISOString(), sourceAsOf: AS_OF });

    expect(read.native.mode).toBe("catalog");
    expect(read.native.runtimeInstance).toBe(RUNTIME);
    expect(read.native.view?.sessions.map((session) => session.sessionId)).toEqual(["session-a", "session-b"]);

    const snapshot = JSON.parse(await readFile(join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE), "utf8"));
    expect(snapshot).toMatchObject({ schema: "kontor/plugin-work-catalog@2", version: 2, hostId: HOST_ID, sourceAsOf: AS_OF });
    expect(snapshot.view.schemaVersion).toBe(1);
    expect(snapshot.native.runtimeInstance).toBe(RUNTIME);
    expect(snapshot.native.sessions.map((session: { sessionId: string }) => session.sessionId)).toEqual(["session-a", "session-b"]);

    const [start] = await fake.starts();
    expect(start?.args).toEqual(["--state-root", "/srv/kontor/state", "--credential-tier", "observer"]);
    expect(start?.env).not.toContain("KONTOR_AUTH");
    expect(start?.env).toEqual(expect.arrayContaining(["PATH", "HOME"]));
    delete process.env.KONTOR_AUTH;
  });

  it("falls back to the last-known snapshot with its own source as-of, never the check time", async () => {
    const fake = await fakeMcp();
    const { paseoHome, configDirectory } = await sandbox();
    const config = configFor(fake.command, paseoHome);
    const live = await loadWorkCatalog(config, { hostId: HOST_ID, now: () => Date.parse(AS_OF) });
    expect(live.mode).toBe("catalog");

    const later = Date.parse("2026-10-25T09:00:00Z");
    const fallback = await loadWorkCatalog(configFor(join(paseoHome, "missing-kontor-mcp"), paseoHome), {
      hostId: HOST_ID,
      now: () => later,
      snapshotPath: join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE),
    });
    expect(fallback.mode).toBe("snapshot");
    expect(fallback.checkedAt).toBe(new Date(later).toISOString());
    expect(fallback.sourceAsOf).toBe(AS_OF);
    expect(fallback.checkedAt).not.toBe(fallback.sourceAsOf);
    expect(fallback.view).toEqual(live.view);
    expect(fallback.snapshot).toEqual({ savedAt: new Date(Date.parse(AS_OF)).toISOString(), sourceAsOf: AS_OF });
    expect(fallback.error).toContain("could not be started");
  });

  it("reports unavailable, never an empty catalog, when there is no snapshot", async () => {
    const { paseoHome, configDirectory } = await sandbox();
    const read = await loadWorkCatalog(configFor(join(paseoHome, "missing-kontor-mcp"), paseoHome), {
      hostId: HOST_ID,
      now: () => Date.parse(AS_OF),
      snapshotPath: join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE),
    });
    expect(read).toMatchObject({ mode: "unavailable", view: null, snapshot: null, sourceAsOf: null });
    expect(read.error).toContain("could not be started");
  });

  it("treats a missing observer tool as incompatible and keeps the snapshot", async () => {
    const good = await fakeMcp();
    const { paseoHome, configDirectory } = await sandbox();
    const config = configFor(good.command, paseoHome);
    await loadWorkCatalog(config, { hostId: HOST_ID, now: () => Date.parse(AS_OF) });

    const old = await fakeMcp({ refuseSources: true });
    const read = await loadWorkCatalog(configFor(old.command, paseoHome), {
      hostId: HOST_ID,
      now: () => Date.parse("2026-10-25T10:00:00Z"),
      snapshotPath: join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE),
    });
    expect(read.mode).toBe("snapshot");
    expect(read.sourceAsOf).toBe(AS_OF);
    expect(read.error).toContain("upgrade kontor-mcp");
  });

  it("follows entry cursors, bounded, and sends the continuation unchanged", async () => {
    const fake = await fakeMcp();
    const { paseoHome } = await sandbox();
    const observation = await observeWorkCatalog(configFor(fake.command, paseoHome), HOST_ID, 2_000);
    expect(observation.entries.map((value) => value.logical_plan_id)).toEqual(["plan-a", "plan-b", "plan-c"]);
    expect(observation.entriesTruncated).toBe(false);
    expect(WORK_CATALOG_MAX_PAGES).toBe(3);
    const starts = await fake.starts();
    expect(starts).toHaveLength(1);
  });

  it("retries a stale entry cursor once from the first page", async () => {
    const fake = await fakeMcp({ staleCursorOnce: true });
    const { paseoHome } = await sandbox();
    const observation = await observeWorkCatalog(configFor(fake.command, paseoHome), HOST_ID, 2_000);
    expect(observation.entries.map((value) => value.logical_plan_id)).toEqual(["plan-a", "plan-b", "plan-c"]);
    const starts = await fake.starts();
    expect(starts).toHaveLength(1);
  });

  it("shares one catalog read per host for the TTL and keeps hosts separate", async () => {
    const fake = await fakeMcp();
    const { paseoHome } = await sandbox();
    const config = configFor(fake.command, paseoHome);
    let clock = Date.parse(AS_OF);
    const read = workCatalogReader(() => config, () => clock);
    const first = await read(HOST_ID);
    const starts = (await fake.starts()).length;
    const second = await read(HOST_ID);
    expect(second).toBe(first);
    expect((await fake.starts()).length).toBe(starts);
    clock += 29_999;
    expect(await read(HOST_ID)).toBe(first);
    expect((await fake.starts()).length).toBe(starts);
    clock += 1;
    await read(HOST_ID);
    expect((await fake.starts()).length).toBe(starts + 1);
    await read("srv_host_b");
    expect((await fake.starts()).length).toBe(starts + 2);
  });

  it("shares one in-flight read before it settles", async () => {
    const fake = await fakeMcp();
    const { paseoHome } = await sandbox();
    const read = workCatalogReader(() => configFor(fake.command, paseoHome), () => Date.parse(AS_OF));
    const first = read(HOST_ID);
    const second = read(HOST_ID);
    expect(second).toBe(first);
    await Promise.all([first, second]);
    expect(await fake.starts()).toHaveLength(1);
  });

  it("reads the realistic multi-page native census without a cumulative response bound", async () => {
    const long = "x".repeat(1_500);
    const page = (index: number, next: string | null) => ({
      ...nativePage({
        sessions: Array.from({ length: 1_000 }, (_, offset) => nativeSession(`session-${index}-${offset}`, {
          title: long,
          cwd: `/${long}`,
          project_name: long,
        })),
        next_cursor: next,
        truncated: next !== null,
      }),
    });
    const nativePages = {
      first: page(0, "n1"),
      n1: page(1, "n2"),
      n2: page(2, "n3"),
      n3: page(3, "n4"),
      n4: page(4, null),
    };
    const fake = await fakeMcp({ nativePages });
    const { paseoHome } = await sandbox();
    const observation = await observeWorkCatalog(configFor(fake.command, paseoHome), HOST_ID, 10_000);
    expect(observation.native.view?.sessions).toHaveLength(5_000);
    expect(observation.native.view?.truncated).toBe(false);
  });

  it("refuses one observer frame beyond the bounded read and keeps the plan catalog", async () => {
    vi.useRealTimers();
    const huge = nativePage({
      sessions: [nativeSession("huge", { title: "x".repeat(FRAME_OVER_LIMIT) })],
    });
    const fake = await fakeMcp({ nativePages: { first: huge } });
    const { paseoHome } = await sandbox();
    const read = await loadWorkCatalog(configFor(fake.command, paseoHome), {
      hostId: HOST_ID,
      now: () => Date.parse(AS_OF),
    });
    expect(read.mode).toBe("catalog");
    expect(read.view?.entries).toHaveLength(3);
    expect(read.native.mode).toBe("unavailable");
    expect(read.native.error).toContain("bounded read");
  }, 30_000);

  it("refuses an unterminated observer frame beyond the bounded read", async () => {
    vi.useRealTimers();
    const fake = await fakeMcp({ unterminatedNative: true });
    const { paseoHome } = await sandbox();
    const read = await loadWorkCatalog(configFor(fake.command, paseoHome), {
      hostId: HOST_ID,
      now: () => Date.parse(AS_OF),
    });
    expect(read.mode).toBe("catalog");
    expect(read.view?.entries).toHaveLength(3);
    expect(read.native.mode).toBe("unavailable");
    expect(read.native.error).toContain("bounded read");
  }, 30_000);

  it("leaves every operator-owned source file byte-identical through a fallback", async () => {
    const fake = await fakeMcp();
    const { paseoHome, configDirectory } = await sandbox();
    const sources = JSON.stringify([{ id: "approved", kind: "plan", title: "Approved", path: "/configured/plan.md", workstream: "kontor",
      revisionKind: "approved revision", expectedSha256: "a".repeat(64), logicalPlanId: "explicit-plan" }]);
    const sourcePath = join(configDirectory, "read-sources.json");
    await writeFile(sourcePath, sources);
    const before = { bytes: await readFile(sourcePath, "utf8"), stat: await stat(sourcePath) };

    const config = configFor(fake.command, paseoHome, { KONTOR_PLUGIN_READ_SOURCES: sources });
    await loadWorkCatalog(config, { hostId: HOST_ID, now: () => Date.parse(AS_OF) });
    await loadWorkCatalog(configFor(join(paseoHome, "missing-kontor-mcp"), paseoHome, { KONTOR_PLUGIN_READ_SOURCES: sources }), { hostId: HOST_ID, now: () => Date.parse("2026-10-25T09:00:00Z") });

    expect(await readFile(sourcePath, "utf8")).toBe(before.bytes);
    expect((await stat(sourcePath)).mtimeMs).toBe(before.stat.mtimeMs);
    const catalog = JSON.parse(before.bytes);
    expect(catalog[0]).toMatchObject({ id: "approved", expectedSha256: "a".repeat(64), logicalPlanId: "explicit-plan" });
  });

  it("does not replace a newer last-known snapshot with a slower older read", async () => {
    const { paseoHome, configDirectory } = await sandbox();
    const path = join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE);
    const pluginSource = (name: "plan" | "jira" | "native", asOf: string) => ({
      source: name, state: "complete" as const, generation: 4, attemptedGeneration: 5, asOf,
      error: null, retryAt: null, coverage: [], conflicts: [], pendingVerification: 0,
    });
    const newer = {
      schemaVersion: 1 as const,
      sources: [pluginSource("plan", "2026-10-25T08:00:00Z"), pluginSource("jira", "2026-10-25T08:00:00Z"), pluginSource("native", "2026-10-25T08:00:00Z")],
      entries: [],
      entriesTruncated: false,
    };
    const older = {
      ...newer,
      sources: newer.sources.map((item) => ({ ...item, asOf: "2026-10-25T06:14:01Z" })),
    };
    const first = await writeWorkCatalogSnapshot(path, { hostId: HOST_ID, view: newer, native: null, savedAt: "2026-10-25T08:01:00Z" });
    expect(first?.sourceAsOf).toBe("2026-10-25T08:00:00Z");
    const second = await writeWorkCatalogSnapshot(path, { hostId: HOST_ID, view: older, native: null, savedAt: "2026-10-25T08:02:00Z" });
    expect(second?.sourceAsOf).toBe("2026-10-25T08:00:00Z");
    expect(second?.savedAt).toBe("2026-10-25T08:01:00Z");
    expect((await readWorkCatalogSnapshot(path, HOST_ID))?.sourceAsOf).toBe("2026-10-25T08:00:00Z");
    expect(workCatalogSnapshotPath(configFor("kontor-mcp", paseoHome))).toBe(path);
  });

  it("maps the native catalog only by the exact Paseo server id", async () => {
    const mismatched = nativeStatus({
      collection: {
        state: "complete", include_archived: true,
        coverage: [{
          runtime_instance: RUNTIME, client_id: "plane-census", server_id: "srv_other",
          server_version: "0.11.4", include_archived: true, state: "scanned",
          projects: 1, workspaces: 1, sessions: 2, archived_sessions: 1, error: null,
        }],
      },
    });
    const fake = await fakeMcp({ nativeStatus: mismatched });
    const { paseoHome } = await sandbox();
    const read = await loadWorkCatalog(configFor(fake.command, paseoHome), { hostId: HOST_ID, now: () => Date.parse(AS_OF) });
    expect(read.mode).toBe("catalog");
    expect(read.native.mode).toBe("unavailable");
    expect(read.native.view).toBeNull();
    expect(read.native.error).toContain("exact server id");
  });

  it("pages the native inventory by its generation-bound cursor", async () => {
    const first = nativePage({ sessions: [nativeSession("session-a")], truncated: true, next_cursor: "native-2" });
    const second = nativePage({ sessions: [nativeSession("session-c")], projects: [], workspaces: [], next_cursor: null });
    const fake = await fakeMcp({ nativePages: { first, "native-2": second } });
    const { paseoHome } = await sandbox();
    const read = await loadWorkCatalog(configFor(fake.command, paseoHome), { hostId: HOST_ID, now: () => Date.parse(AS_OF) });
    expect(read.native.view?.sessions.map((session) => session.sessionId)).toEqual(["session-a", "session-c"]);
    expect(read.native.view?.truncated).toBe(false);
  });

  it("consumes the 5,654-record production census within the native bound", async () => {
    const count = 5_654;
    const page = (start: number, end: number, next: string | null) => nativePage({
      sessions: Array.from({ length: end - start }, (_, index) => nativeSession(`session-${start + index}`)),
      projects: start === 0 ? [nativeProject] : [],
      workspaces: start === 0 ? [nativeWorkspace] : [],
      truncated: next !== null,
      ...(next === null ? {} : { next_cursor: next }),
    });
    const fake = await fakeMcp({
      nativeStatus: nativeStatus({
        collection: {
          state: "complete", include_archived: true,
          coverage: [{
            runtime_instance: RUNTIME, client_id: "plane-census", server_id: HOST_ID,
            server_version: "0.11.4", include_archived: true, state: "scanned",
            projects: 1, workspaces: 1, sessions: count, archived_sessions: 4_000, error: null,
          }],
        },
      }),
      nativePages: {
        first: page(0, 1_000, "native-2"),
        "native-2": page(1_000, 2_000, "native-3"),
        "native-3": page(2_000, 3_000, "native-4"),
        "native-4": page(3_000, 4_000, "native-5"),
        "native-5": page(4_000, 5_000, "native-6"),
        "native-6": page(5_000, count, null),
      },
    });
    const { paseoHome, configDirectory } = await sandbox();
    const read = await loadWorkCatalog(configFor(fake.command, paseoHome), { hostId: HOST_ID, now: () => Date.parse(AS_OF) });
    expect(WORK_CATALOG_NATIVE_MAX_PAGES).toBe(6);
    expect(read.native.mode).toBe("catalog");
    expect(read.native.view?.sessions).toHaveLength(count);
    expect(read.native.view?.truncated).toBe(false);
    const snapshot = await readWorkCatalogSnapshot(join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE), HOST_ID);
    expect(snapshot?.native?.sessions).toHaveLength(count);
    expect((await stat(join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE))).size).toBeLessThan(WORK_CATALOG_SNAPSHOT_LIMIT);
  }, 20_000);

  it("keeps the plan catalog when the native observer tool is missing", async () => {
    const fake = await fakeMcp({ refuseNative: true });
    const { paseoHome } = await sandbox();
    const read = await loadWorkCatalog(configFor(fake.command, paseoHome), { hostId: HOST_ID, now: () => Date.parse(AS_OF) });
    expect(read.mode).toBe("catalog");
    expect(read.view?.entries).toHaveLength(3);
    expect(read.native.mode).toBe("unavailable");
    expect(read.native.error).toContain("upgrade kontor-mcp");
  });

  it("reads a legacy v1 snapshot for the plan view and never invents native rows", async () => {
    const { paseoHome, configDirectory } = await sandbox();
    const path = join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE);
    const legacyView = {
      schemaVersion: 1 as const,
      sources: [source("plan"), source("jira"), source("native")].map((item, index) => ({
        source: item.source, state: item.state, generation: item.generation, attemptedGeneration: item.attempted_generation,
        asOf: item.as_of, error: item.error, retryAt: item.retry_at, coverage: [], conflicts: [], pendingVerification: index === 0 ? 3 : 0,
      })),
      entries: [],
      entriesTruncated: false,
    };
    await writeFile(path, JSON.stringify({ schema: "kontor/plugin-work-catalog@1", version: 1, savedAt: "2026-10-25T08:00:00Z", sourceAsOf: AS_OF, view: legacyView }));
    const read = await loadWorkCatalog(configFor(join(paseoHome, "missing-kontor-mcp"), paseoHome), {
      hostId: HOST_ID, now: () => Date.parse("2026-10-25T09:00:00Z"), snapshotPath: path,
    });
    expect(read.mode).toBe("snapshot");
    expect(read.view?.schemaVersion).toBe(1);
    expect(read.native.mode).toBe("unavailable");
    expect(read.native.view).toBeNull();
    expect(read.native.error).toContain("No native snapshot for this host");
  });

  it("refuses a snapshot written for another host", async () => {
    const { paseoHome, configDirectory } = await sandbox();
    const path = join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE);
    const live = await loadWorkCatalog(configFor((await fakeMcp()).command, paseoHome), { hostId: HOST_ID, now: () => Date.parse(AS_OF) });
    expect(live.native.view?.sessions).toHaveLength(2);
    expect(await readWorkCatalogSnapshot(path, "srv_other")).toBeNull();
    const other = await loadWorkCatalog(configFor(join(paseoHome, "missing-kontor-mcp"), paseoHome), {
      hostId: "srv_other", now: () => Date.parse("2026-10-25T09:00:00Z"), snapshotPath: path,
    });
    expect(other.mode).toBe("unavailable");
    expect(other.native.mode).toBe("unavailable");
  });

  it("keeps a last-known native snapshot when a later read cannot map the host", async () => {
    const { paseoHome, configDirectory } = await sandbox();
    const path = join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE);
    const fake = await fakeMcp();
    const live = await loadWorkCatalog(configFor(fake.command, paseoHome), { hostId: HOST_ID, now: () => Date.parse(AS_OF) });
    expect(live.native.mode).toBe("catalog");
    const mismatched = nativeStatus({
      collection: {
        state: "complete", include_archived: true,
        coverage: [{
          runtime_instance: RUNTIME, client_id: "plane-census", server_id: "srv_other",
          server_version: "0.11.4", include_archived: true, state: "scanned",
          projects: 1, workspaces: 1, sessions: 2, archived_sessions: 1, error: null,
        }],
      },
    });
    const next = await fakeMcp({ nativeStatus: mismatched });
    const read = await loadWorkCatalog(configFor(next.command, paseoHome), {
      hostId: HOST_ID, now: () => Date.parse("2026-10-25T09:00:00Z"), snapshotPath: path,
    });
    expect(read.mode).toBe("catalog");
    expect(read.native.mode).toBe("snapshot");
    expect(read.native.view?.sessions.map((session) => session.sessionId)).toEqual(["session-a", "session-b"]);
    expect(read.native.error).toContain("exact server id");
  });

  it("keeps the snapshot path inside the plugin configuration directory", async () => {
    const { paseoHome, configDirectory } = await sandbox();
    expect(workCatalogSnapshotPath(configFor("kontor-mcp", paseoHome))).toBe(join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE));
  });
});
