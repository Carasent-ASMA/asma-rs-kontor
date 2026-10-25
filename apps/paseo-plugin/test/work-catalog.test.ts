import { chmod, mkdir, mkdtemp, readFile, rm, stat, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { readConfig } from "../server/config";
import {
  loadWorkCatalog,
  observeWorkCatalog,
  readWorkCatalogSnapshot,
  workCatalogSnapshotPath,
  writeWorkCatalogSnapshot,
  WORK_CATALOG_MAX_PAGES,
  WORK_CATALOG_SNAPSHOT_FILE,
} from "../server/work-catalog";

const temporary: string[] = [];
afterAll(() => Promise.all(temporary.map((directory) => rm(directory, { recursive: true, force: true }))));

const AS_OF = "2026-10-25T06:14:01Z";

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
  refuseSources?: boolean;
  silent?: boolean;
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
const refuseSources = ${options.refuseSources === true};
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
      process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result: envelope(name, pages[key]) }) + "\\n");
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
    const read = await loadWorkCatalog(config, { now: () => Date.parse(AS_OF) });

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

    const snapshot = JSON.parse(await readFile(join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE), "utf8"));
    expect(snapshot).toMatchObject({ schema: "kontor/plugin-work-catalog@1", version: 1, sourceAsOf: AS_OF });
    expect(snapshot.view.schemaVersion).toBe(1);

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
    const live = await loadWorkCatalog(config, { now: () => Date.parse(AS_OF) });
    expect(live.mode).toBe("catalog");

    const later = Date.parse("2026-10-25T09:00:00Z");
    const fallback = await loadWorkCatalog(configFor(join(paseoHome, "missing-kontor-mcp"), paseoHome), {
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
    await loadWorkCatalog(config, { now: () => Date.parse(AS_OF) });

    const old = await fakeMcp({ refuseSources: true });
    const read = await loadWorkCatalog(configFor(old.command, paseoHome), {
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
    const observation = await observeWorkCatalog(configFor(fake.command, paseoHome), 2_000);
    expect(observation.entries.map((value) => value.logical_plan_id)).toEqual(["plan-a", "plan-b", "plan-c"]);
    expect(observation.entriesTruncated).toBe(false);
    expect(WORK_CATALOG_MAX_PAGES).toBe(3);
    const starts = await fake.starts();
    expect(starts).toHaveLength(1);
  });

  it("leaves every operator-owned source file byte-identical through a fallback", async () => {
    const fake = await fakeMcp();
    const { paseoHome, configDirectory } = await sandbox();
    const sources = JSON.stringify([{ id: "approved", kind: "plan", title: "Approved", path: "/configured/plan.md", workstream: "kontor",
      revisionKind: "approved revision", expectedSha256: "a".repeat(64), logicalPlanId: "explicit-plan" }]);
    const sourcePath = join(configDirectory, "read-sources.json");
    await writeFile(sourcePath, sources);
    const before = { bytes: await readFile(sourcePath, "utf8"), stat: await stat(sourcePath) };

    const config = configFor(fake.command, paseoHome, { KONTOR_PLUGIN_READ_SOURCES: sources });
    await loadWorkCatalog(config, { now: () => Date.parse(AS_OF) });
    await loadWorkCatalog(configFor(join(paseoHome, "missing-kontor-mcp"), paseoHome, { KONTOR_PLUGIN_READ_SOURCES: sources }), { now: () => Date.parse("2026-10-25T09:00:00Z") });

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
    const first = await writeWorkCatalogSnapshot(path, newer, "2026-10-25T08:01:00Z");
    expect(first?.sourceAsOf).toBe("2026-10-25T08:00:00Z");
    const second = await writeWorkCatalogSnapshot(path, older, "2026-10-25T08:02:00Z");
    expect(second?.sourceAsOf).toBe("2026-10-25T08:00:00Z");
    expect((await readWorkCatalogSnapshot(path))?.sourceAsOf).toBe("2026-10-25T08:00:00Z");
    expect(workCatalogSnapshotPath(configFor("kontor-mcp", paseoHome))).toBe(path);
  });

  it("keeps the snapshot path inside the plugin configuration directory", async () => {
    const { paseoHome, configDirectory } = await sandbox();
    expect(workCatalogSnapshotPath(configFor("kontor-mcp", paseoHome))).toBe(join(configDirectory, WORK_CATALOG_SNAPSHOT_FILE));
  });
});
