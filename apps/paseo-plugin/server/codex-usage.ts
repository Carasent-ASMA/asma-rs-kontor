import { createHash, randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { lstat, mkdir, open, opendir, realpath, rename, rm } from "node:fs/promises";
import { join, relative, resolve, sep } from "node:path";
import { z } from "zod";

const uuid = z.string().uuid();
const number = z.number().int().nonnegative().max(Number.MAX_SAFE_INTEGER);
const countsSchema = z.object({ input_tokens: number, output_tokens: number, total_tokens: number,
  cached_input_tokens: number.optional(), cache_write_input_tokens: number.optional(), reasoning_output_tokens: number.optional() });
type Counts = z.infer<typeof countsSchema>;
const stateSchema = z.object({ version: z.literal(3), threadId: uuid, file: z.string(), inode: z.string(), size: number, modified: z.number().nonnegative(), offset: number,
  meta: z.boolean(), problem: z.enum(["identity mismatch", "unqualified format version", "fork or inherited scope is unqualified", "invalid identity metadata", "invalid cumulative counters", "inconsistent cumulative counters", "cumulative counter reset", "malformed usage record", "record exceeds the source bound"]).nullable(), counts: countsSchema.nullable(), baseline: countsSchema.nullable(), at: z.string().nullable() }).strict();
type State = z.infer<typeof stateSchema>;
export type CodexUsageRead = { input: number | null; output: number | null; source: string; scope: "provider-cumulative" };
export const CODEX_USAGE_LIMITS = [
  "Codex 0.160.0 local provider cumulative snapshots only, joined by exact native thread ID in this host's configured/default Codex store.",
  "Historical completeness and child-thread inclusion are unverified. Forked/inherited, reset, ambiguous, unsupported and incomplete sources remain unavailable; snapshots are never summed.",
  "Other provider routes and provider homes without a qualified exact source remain unknown. No provider session is started or resumed.",
];
const unknown = (reason: string): CodexUsageRead => ({ input: null, output: null, source: `Codex cumulative usage unavailable: ${reason}. Historical/complete totals unavailable.`, scope: "provider-cumulative" });
const countKeys = ["input_tokens", "output_tokens", "total_tokens", "cached_input_tokens", "cache_write_input_tokens", "reasoning_output_tokens"] as const;

/** Interpret only identity and numeric usage records; never retain transcript text or sum cumulative values. */
export function consumeCodexLine(state: State, line: string): void {
  if (state.problem) return;
  try {
    const row = JSON.parse(line);
    // Revalidate every metadata boundary, including one appended after a cached offset.
    if (!state.meta || row?.type === "session_meta") {
      const meta = row?.payload;
      if (row?.type !== "session_meta" || meta?.id !== state.threadId) { state.problem = "identity mismatch"; return; }
      if (meta.cli_version !== "0.160.0" || meta.originator !== "codex_cli_rs") { state.problem = "unqualified format version"; return; }
      if (meta.session_id !== meta.id || meta.forked_from_id || meta.parent_thread_id || typeof meta.source !== "string") { state.problem = "fork or inherited scope is unqualified"; return; }
      state.meta = true;
      return;
    }
    if (row?.type !== "event_msg" || row?.payload?.type !== "token_count") return;
    const raw = row.payload.info?.total_token_usage;
    if (raw === undefined || raw === null) return;
    const parsed = countsSchema.safeParse(raw);
    if (!parsed.success || !z.string().datetime().safeParse(row.timestamp).success) { state.problem = "invalid cumulative counters"; return; }
    const next = parsed.data;
    if (next.input_tokens + next.output_tokens !== next.total_tokens || (next.cached_input_tokens ?? 0) > next.input_tokens || (next.reasoning_output_tokens ?? 0) > next.output_tokens) { state.problem = "inconsistent cumulative counters"; return; }
    if (state.counts && countKeys.some((key) => state.counts![key] !== undefined && next[key] !== undefined && next[key]! < state.counts![key]!)) { state.problem = "cumulative counter reset"; return; }
    state.counts = next; state.at = row.timestamp;
  } catch { state.problem = state.meta ? "malformed usage record" : "invalid identity metadata"; }
}

/** Bounded, on-demand source index and incremental numeric cache. Provider files are opened read-only. */
export function createCodexUsageReader(providerRoot: string, cacheRoot: string, now = Date.now) {
  let indexedAt = -Infinity, paths = new Map<string, string[]>(), indexing: Promise<void> | null = null;
  const tails = new Map<string, Promise<unknown>>();
  const namespace = createHash("sha256").update(resolve(providerRoot)).digest("hex");
  async function index() {
    if (now() - indexedAt < 5_000) return;
    if (indexing) return indexing;
    indexing = (async () => {
      const found = new Map<string, string[]>(); let directories = 0, entries = 0;
      async function walk(relativePath: string, depth: number) {
        if (++directories > 4096) throw new Error("index bound");
        let directory;
        try { directory = await opendir(join(providerRoot, relativePath)); }
        catch (error) { if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") return; throw error; }
        for await (const item of directory) {
          if (++entries > 100_000) throw new Error("index bound");
          if (item.isSymbolicLink()) continue;
          const path = join(relativePath, item.name);
          if (item.isDirectory() && depth < 3 && /^\d{2,4}$/.test(item.name)) await walk(path, depth + 1);
          if (!item.isFile()) continue;
          const match = /-([0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12})\.jsonl$/i.exec(item.name);
          if (match) found.set(match[1]!, [...(found.get(match[1]!) ?? []), path]);
        }
      }
      await walk("sessions", 0); await walk("archived_sessions", 0);
      paths = found; indexedAt = now();
    })();
    try { await indexing; } finally { indexing = null; }
  }
  const cacheFile = (threadId: string) => join(cacheRoot, `${createHash("sha256").update(`${namespace}\0${threadId}`).digest("hex")}.json`);
  async function cached(threadId: string): Promise<State | null> {
    let file;
    try { file = await open(cacheFile(threadId), constants.O_RDONLY | constants.O_NOFOLLOW); }
    catch (error) { if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") return null; throw error; }
    try {
      const buffer = Buffer.alloc(4097), result = await file.read(buffer, 0, buffer.length, 0);
      if (result.bytesRead > 4096) throw new Error("cache bound");
      const raw = JSON.parse(buffer.subarray(0, result.bytesRead).toString("utf8"));
      // Recompute older caches, including permanent large-record failures, from byte zero.
      if (raw?.version !== 3) return null;
      const value = stateSchema.parse(raw);
      if (value.threadId !== threadId) throw new Error("cache identity");
      return value;
    } finally { await file.close(); }
  }
  async function save(state: State) {
    await mkdir(cacheRoot, { recursive: true, mode: 0o700 });
    const path = cacheFile(state.threadId), temporary = `${path}.${randomUUID()}.tmp`;
    try {
      const file = await open(temporary, "wx", 0o600);
      try { await file.writeFile(JSON.stringify(stateSchema.parse(state))); await file.sync(); } finally { await file.close(); }
      await rename(temporary, path);
    } finally { await rm(temporary, { force: true }); }
  }
  async function read(threadId: string, budget: { bytes: number }): Promise<CodexUsageRead> {
    if (!uuid.safeParse(threadId).success) return unknown("invalid native identity");
    try {
      await index();
      const matches = paths.get(threadId) ?? [];
      if (matches.length !== 1) return unknown(matches.length ? "ambiguous exact source" : "no exact source in the configured namespace");
      const path = join(providerRoot, matches[0]!);
      const root = await realpath(providerRoot), target = await realpath(path), location = relative(root, target);
      if (location.startsWith(`..${sep}`) || location === ".." || resolve(root, location) !== target || (await lstat(path)).isSymbolicLink()) return unknown("source outside the allowed namespace");
      const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
      try {
        const stat = await file.stat(), inode = `${stat.dev}:${stat.ino}`;
        if (!stat.isFile()) return unknown("source is not a regular file");
        const old = await cached(threadId);
        const state: State = old && old.file === matches[0] && old.inode === inode && old.offset <= stat.size && !(old.size === stat.size && old.modified !== stat.mtimeMs) ? old
          : { version: 3, threadId, file: matches[0]!, inode, size: stat.size, modified: stat.mtimeMs, offset: 0, meta: false, problem: old?.problem === "cumulative counter reset" ? old.problem : null, counts: null, baseline: old?.baseline ?? old?.counts ?? null, at: null };
        let position = state.offset, pending = Buffer.alloc(0);
        const end = stat.size;
        // A shared RPC budget bounds all requested sessions; unfinished scans retain only numeric state and offsets.
        while (position < end && budget.bytes > 0 && !state.problem) {
          const size = Math.min(64 * 1024, end - position, budget.bytes); budget.bytes -= size;
          const buffer = Buffer.alloc(size), { bytesRead } = await file.read(buffer, 0, size, position);
          if (!bytesRead) break;
          position += bytesRead; pending = Buffer.concat([pending, buffer.subarray(0, bytesRead)]);
          let newline;
          while ((newline = pending.indexOf(10)) !== -1) {
            if (newline > 16 * 1024 * 1024) { state.problem = "record exceeds the source bound"; break; }
            consumeCodexLine(state, pending.subarray(0, newline).toString("utf8"));
            state.offset += newline + 1; pending = pending.subarray(newline + 1);
          }
          if (pending.length > 16 * 1024 * 1024) state.problem = "record exceeds the source bound";
        }
        const after = await file.stat();
        const current = await lstat(path);
        if (`${after.dev}:${after.ino}` !== inode || after.size < end || (after.size === end && after.mtimeMs !== stat.mtimeMs) || `${current.dev}:${current.ino}` !== inode) return unknown("source changed during read");
        if (state.offset === end && state.baseline && state.counts && countKeys.some((key) => state.baseline![key] !== undefined && state.counts![key] !== undefined && state.counts![key]! < state.baseline![key]!)) state.problem = "cumulative counter reset";
        if (!state.problem && state.offset === end && state.counts) state.baseline = state.counts;
        state.size = end; state.modified = stat.mtimeMs;
        await save(state);
        if (state.problem) return unknown(state.problem);
        if (state.offset < end || !state.meta) return unknown("incomplete bounded source read");
        if (!state.counts) return unknown("no provider cumulative report");
        return { input: state.counts.input_tokens, output: state.counts.output_tokens, scope: "provider-cumulative",
          source: `Codex 0.160.0 provider cumulative snapshot at ${state.at}. Exact native thread; cache reads and reasoning are already included, not added again. Historical completeness and child-thread inclusion unverified.` };
      } finally { await file.close(); }
    } catch { return unknown("source or numeric cache unreadable"); }
  }
  return {
    read(threadId: string, budget = { bytes: 32 * 1024 * 1024 }) {
      const previous = tails.get(threadId) ?? Promise.resolve();
      const result = previous.catch(() => undefined).then(() => read(threadId, budget)); tails.set(threadId, result);
      return result.finally(() => { if (tails.get(threadId) === result) tails.delete(threadId); });
    },
  };
}
