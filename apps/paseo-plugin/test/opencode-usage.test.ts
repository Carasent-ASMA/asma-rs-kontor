import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { DatabaseSync } from "node:sqlite";
import { mkdtemp, rm } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { usageTotalsSchema } from "../shared/session-navigator";
import { createOpenCodeUsageReader, openCodeDatabasePath } from "../server/opencode-usage";

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
