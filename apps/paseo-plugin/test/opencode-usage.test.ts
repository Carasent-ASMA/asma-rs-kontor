import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { DatabaseSync } from "node:sqlite";
import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { usageTotalsSchema } from "../shared/session-navigator";
import { createOpenCodeUsageReader, openCodeDatabasePath } from "../server/opencode-usage";
import { OPENCODE_V1_SCHEMA, OPENCODE_V2_SCHEMA } from "./fixtures/opencode-schema";

let root: string, database: string;
const ID = "ses_0123456789abcdef";
beforeEach(async () => {
  root = await mkdtemp(join(tmpdir(), "kontor-opencode-usage-"));
  database = join(root, "opencode.db");
});
afterEach(async () => { await rm(root, { recursive: true, force: true }); });

type Cell = string | number | null;
function write(rows: { id: string; input?: Cell; output?: Cell; reasoning?: Cell; cacheRead?: Cell; cacheWrite?: Cell }[]) {
  const db = new DatabaseSync(database);
  try {
    db.exec("DROP TABLE IF EXISTS session");
    db.exec("CREATE TABLE session (id TEXT PRIMARY KEY, tokens_input INTEGER, tokens_output INTEGER, tokens_reasoning INTEGER, tokens_cache_read INTEGER, tokens_cache_write INTEGER)");
    const insert = db.prepare("INSERT INTO session (id, tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write) VALUES (?, ?, ?, ?, ?, ?)");
    for (const row of rows) insert.run(row.id, row.input === undefined ? 0 : row.input, row.output === undefined ? 0 : row.output,
      row.reasoning === undefined ? 0 : row.reasoning, row.cacheRead === undefined ? 0 : row.cacheRead, row.cacheWrite === undefined ? 0 : row.cacheWrite);
  } finally { db.close(); }
}

describe("OpenCode session usage", () => {
  it("resolves the XDG data-home database and its operator default", () => {
    expect(openCodeDatabasePath({ XDG_DATA_HOME: "/tmp/fixture-xdg" })).toBe("/tmp/fixture-xdg/opencode/opencode.db");
    expect(openCodeDatabasePath({})).toMatch(/\/opencode\/opencode\.db$/);
  });

  it("maps one exact session row: input includes cache read/write and output includes reasoning", async () => {
    write([
      { id: ID, input: 10, output: 5, reasoning: 2, cacheRead: 3, cacheWrite: 4 },
      { id: "ses_other", input: 999, output: 999, reasoning: 999, cacheRead: 999, cacheWrite: 999 },
    ]);
    const result = await createOpenCodeUsageReader(database).read(ID);
    expect(result).toMatchObject({ input: 17, output: 7, scope: "provider-cumulative" });
    expect(usageTotalsSchema.parse(result)).toEqual(result);
    expect(result.source).toContain("OpenCode provider cumulative session row");
    expect(result.source).not.toContain("1.19.3");
    expect(result.source).toContain("child/subagent sessions (parent_id) are excluded");
    expect(result.source).toContain("cache read/write are included in input");
    expect(result.source).toContain("read from session (the v1 layout)");
  });

  it("returns unknown for a missing row, missing database or invalid counters", async () => {
    write([{ id: ID }]);
    expect(await createOpenCodeUsageReader(database).read("ses_missing")).toMatchObject({ input: null, output: null, source: expect.stringContaining("no exact native session row") });
    expect(await createOpenCodeUsageReader(join(root, "missing.db")).read(ID)).toMatchObject({ input: null, output: null, source: expect.stringContaining("source unreadable") });
    for (const row of [
      { id: ID, input: "ten" },
      { id: ID, output: -1 },
      { id: ID, reasoning: null },
      { id: ID, cacheRead: 1.5 },
      { id: ID, input: 1, cacheWrite: Number.MAX_SAFE_INTEGER },
    ]) {
      write([row]);
      expect(await createOpenCodeUsageReader(database).read(ID)).toMatchObject({ input: null, output: null, source: expect.stringContaining("OpenCode cumulative usage unavailable") });
    }
  });

  it("does not fall back to another session or to latest-turn telemetry", async () => {
    write([{ id: "ses_other", input: 100, output: 8 }]);
    const result = await createOpenCodeUsageReader(database).read(ID);
    expect(result).toEqual({ input: null, output: null, scope: "provider-cumulative", source: expect.stringContaining("no exact native session row") });
  });
});

// Rows go into the captured OpenCode DDL; foreign keys are off so a fixture names only the rows its case needs.
function captured(schema: string, rows: { table: "session" | "session_v2"; id: string; parent?: string; tokens: number[] }[]) {
  const db = new DatabaseSync(database, { enableForeignKeyConstraints: false });
  try {
    db.exec(schema);
    for (const { table, id, parent = null, tokens } of rows) db.prepare(`INSERT INTO ${table} (id, project_id, parent_id, slug, directory, title, version, time_created, time_updated,
      tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write) VALUES (?, 'prj', ?, 's', '/w', 't', '0', 1, 1, ?, ?, ?, ?, ?)`).run(id, parent, ...tokens);
  } finally { db.close(); }
}

describe("OpenCode v2 session usage", () => {
  const read = (id = ID) => createOpenCodeUsageReader(database).read(id);
  it("reads the exact session_v2 row of a v2-only database, without its child sessions", async () => {
    captured(OPENCODE_V2_SCHEMA, [{ table: "session_v2", id: ID, tokens: [10, 5, 2, 3, 4] }, { table: "session_v2", id: "ses_child", parent: ID, tokens: [999, 999, 999, 999, 999] }]);
    const result = await read();
    expect(result).toMatchObject({ input: 17, output: 7, scope: "provider-cumulative" });
    expect(usageTotalsSchema.parse(result)).toEqual(result);
    expect(result.source).toContain("read from session_v2 (the v2 layout)");
    expect(result.source).toContain("child/subagent sessions (parent_id) are excluded");
  });
  it("reads only v2 when both layouts are present, never a frozen v1 row", async () => {
    captured(OPENCODE_V1_SCHEMA + OPENCODE_V2_SCHEMA, [{ table: "session", id: ID, tokens: [1, 1, 1, 1, 1] }, { table: "session", id: "ses_frozen", tokens: [100, 8, 0, 0, 0] },
      { table: "session_v2", id: ID, tokens: [10, 5, 2, 3, 4] }]);
    expect(await read()).toMatchObject({ input: 17, output: 7, source: expect.stringContaining("read from session_v2") });
    expect(await read("ses_frozen")).toEqual({ input: null, output: null, scope: "provider-cumulative", source: expect.stringContaining("no exact native session row") });
  });
  it.each([["session_message and session_part", "DROP TABLE session_v2;"], ["session_part alone", "DROP TABLE session_v2; DROP TABLE session_message;"],
    ["session_message alone", "DROP TABLE session_v2; DROP TABLE session_part;"]])(
    "refuses a database whose v2 markers are %s, never reading the frozen v1 row", async (_, drop) => {
    captured(`${OPENCODE_V1_SCHEMA}${OPENCODE_V2_SCHEMA}\n${drop}`, [{ table: "session", id: ID, tokens: [10, 5, 2, 3, 4] }]);
    expect(await read()).toEqual({ input: null, output: null, scope: "provider-cumulative", source: expect.stringContaining("OpenCode v2 layout lacks session_v2.id") });
  });
  it("reads session_v2 when it is the only v2 marker, never the frozen v1 row", async () => {
    captured(`${OPENCODE_V1_SCHEMA}${OPENCODE_V2_SCHEMA}\nDROP TABLE session_message; DROP TABLE session_part;`,
      [{ table: "session", id: ID, tokens: [100, 8, 0, 0, 0] }, { table: "session_v2", id: ID, tokens: [10, 5, 2, 3, 4] }]);
    expect(await read()).toMatchObject({ input: 17, output: 7, source: expect.stringContaining("read from session_v2 (the v2 layout)") });
  });
  it.each([["message", "DROP TABLE session; DROP TABLE part;"], ["part", "DROP TABLE session; DROP TABLE message;"]])(
    "reports the v1 layout gap, not neither, when %s is the only v1 marker", async (_, drop) => {
    captured(`${OPENCODE_V1_SCHEMA}\n${drop}`, []);
    expect(await read()).toEqual({ input: null, output: null, scope: "provider-cumulative", source: expect.stringContaining("OpenCode v1 layout lacks session.id") });
  });
  it("reads the exact session row of a v1-only database on the captured v1 DDL", async () => {
    captured(OPENCODE_V1_SCHEMA, [{ table: "session", id: ID, tokens: [10, 5, 2, 3, 4] }, { table: "session", id: "ses_child", parent: ID, tokens: [999, 999, 999, 999, 999] }]);
    expect(await read()).toMatchObject({ input: 17, output: 7, source: expect.stringContaining("read from session (the v1 layout)") });
  });
  it.each([["a v2-only", OPENCODE_V2_SCHEMA], ["a both-layout", OPENCODE_V1_SCHEMA + OPENCODE_V2_SCHEMA]])("refuses %s database whose session_v2 lacks a counter it reads", async (_, schema) => {
    captured(`${schema}\nALTER TABLE session_v2 RENAME COLUMN tokens_cache_write TO tokens_cache_write_renamed;`, []);
    expect(await read()).toEqual({ input: null, output: null, scope: "provider-cumulative", source: expect.stringContaining("OpenCode v2 layout lacks session_v2.tokens_cache_write") });
  });
  it("refuses a database with neither layout", async () => {
    const db = new DatabaseSync(database); db.exec("CREATE TABLE unrelated (id text)"); db.close();
    expect(await read()).toEqual({ input: null, output: null, scope: "provider-cumulative", source: expect.stringContaining("neither the v1 nor the v2 session layout") });
  });
});
