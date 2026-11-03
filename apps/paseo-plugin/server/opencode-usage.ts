import { homedir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import type { VendorUsageRead } from "./codex-usage";

const MAX = Number.MAX_SAFE_INTEGER;
const unknown = (reason: string): VendorUsageRead => ({ input: null, output: null, scope: "provider-cumulative",
  source: `OpenCode cumulative usage unavailable: ${reason}. Historical/complete totals unavailable.` });
const count = (value: unknown): number | null => typeof value === "number" && Number.isSafeInteger(value) && value >= 0 && value <= MAX ? value : null;
const TOKENS = ["id", "tokens_input", "tokens_output", "tokens_reasoning", "tokens_cache_read", "tokens_cache_write"];

export type OpenCodeLayout = "v1" | "v2";
/** The tables that mark each layout, the same for every reader so that all of them choose the same layout. */
const OPENCODE_MARKERS = { v2: ["session_v2", "session_message", "session_part"], v1: ["session", "message", "part"] } as const;
/**
 * The OpenCode layout from the schema alone, never per row or from the version. 2.x writes `session_v2` and leaves the
 * 1.x `session` rows frozen, so v2 wins whenever any of its marker tables exists. The chosen layout serves only when it
 * carries every column the caller reads (table → columns); otherwise the caller gets a gap, never the other layout.
 */
export function openCodeLayout(database: DatabaseSync, reads: Record<OpenCodeLayout, Record<string, string[]>>): OpenCodeLayout | { gap: string } {
  const columns = new Map<string, Set<string>>();
  for (const row of database.prepare("SELECT m.name AS tbl, p.name AS col FROM sqlite_master m, pragma_table_info(m.name) p WHERE m.type = 'table'").all() as { tbl: string; col: string }[])
    columns.set(row.tbl, (columns.get(row.tbl) ?? new Set()).add(row.col));
  const layout = (["v2", "v1"] as const).find((candidate) => OPENCODE_MARKERS[candidate].some((table) => columns.has(table)));
  if (!layout) return { gap: "OpenCode database has neither the v1 nor the v2 session layout" };
  const missing = Object.entries(reads[layout]).flatMap(([table, read]) => read.filter((column) => !columns.get(table)?.has(column)).map((column) => `${table}.${column}`));
  return missing.length ? { gap: `OpenCode ${layout} layout lacks ${missing.join(", ")}` } : layout;
}

/** `$XDG_DATA_HOME/opencode/opencode.db`, or the XDG default under the operator home. */
export function openCodeDatabasePath(env: NodeJS.ProcessEnv = process.env): string {
  const dataHome = env.XDG_DATA_HOME?.trim();
  return join(dataHome || join(homedir(), ".local", "share"), "opencode", "opencode.db");
}

/** One exact OpenCode session row; the vendor's own counters already include cache and reasoning. */
export function createOpenCodeUsageReader(databasePath: string) {
  return {
    async read(nativeHandle: string): Promise<VendorUsageRead> {
      if (!nativeHandle) return unknown("invalid native session identity");
      let database: DatabaseSync | null = null;
      try {
        database = new DatabaseSync(databasePath, { readOnly: true });
        const layout = openCodeLayout(database, { v2: { session_v2: TOKENS }, v1: { session: TOKENS } });
        if (typeof layout !== "string") return unknown(layout.gap);
        const table = layout === "v2" ? "session_v2" : "session";
        const row = database.prepare(`SELECT tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write FROM ${table} WHERE id = ?`)
          .get(nativeHandle) as Record<string, unknown> | undefined;
        if (!row) return unknown("no exact native session row");
        const inputTokens = count(row.tokens_input), outputTokens = count(row.tokens_output), reasoning = count(row.tokens_reasoning);
        const cacheRead = count(row.tokens_cache_read), cacheWrite = count(row.tokens_cache_write);
        if ([inputTokens, outputTokens, reasoning, cacheRead, cacheWrite].some((value) => value === null)) return unknown("invalid session counters");
        const input = inputTokens! + cacheRead! + cacheWrite!, output = outputTokens! + reasoning!;
        if (!Number.isSafeInteger(input) || !Number.isSafeInteger(output)) return unknown("session counters exceed the exact integer range");
        return { input, output, scope: "provider-cumulative",
          source: `OpenCode provider cumulative session row at the exact native session ID, read from ${table} (the ${layout} layout); only the exact native session row is read, so child/subagent sessions (parent_id) are excluded; cache read/write are included in input and reasoning is included in output. Historical completeness unverified.` };
      } catch { return unknown("source unreadable"); }
      finally { try { database?.close(); } catch { /* already closed */ } }
    },
  };
}
