import { homedir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import type { VendorUsageRead } from "./codex-usage";

const MAX = Number.MAX_SAFE_INTEGER;
const unknown = (reason: string): VendorUsageRead => ({ input: null, output: null, scope: "provider-cumulative",
  source: `OpenCode cumulative usage unavailable: ${reason}. Historical/complete totals unavailable.` });
const count = (value: unknown): number | null => typeof value === "number" && Number.isSafeInteger(value) && value >= 0 && value <= MAX ? value : null;

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
        const row = database.prepare("SELECT tokens_input, tokens_output, tokens_reasoning, tokens_cache_read, tokens_cache_write FROM session WHERE id = ?")
          .get(nativeHandle) as Record<string, unknown> | undefined;
        if (!row) return unknown("no exact native session row");
        const inputTokens = count(row.tokens_input), outputTokens = count(row.tokens_output), reasoning = count(row.tokens_reasoning);
        const cacheRead = count(row.tokens_cache_read), cacheWrite = count(row.tokens_cache_write);
        if ([inputTokens, outputTokens, reasoning, cacheRead, cacheWrite].some((value) => value === null)) return unknown("invalid session counters");
        const input = inputTokens! + cacheRead! + cacheWrite!, output = outputTokens! + reasoning!;
        if (!Number.isSafeInteger(input) || !Number.isSafeInteger(output)) return unknown("session counters exceed the exact integer range");
        return { input, output, scope: "provider-cumulative",
          source: "OpenCode provider cumulative session row at the exact native session ID; only the exact native session row is read, so child/subagent sessions (parent_id) are excluded; cache read/write are included in input and reasoning is included in output. Historical completeness unverified." };
      } catch { return unknown("source unreadable"); }
      finally { try { database?.close(); } catch { /* already closed */ } }
    },
  };
}
