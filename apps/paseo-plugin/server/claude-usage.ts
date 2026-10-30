import { createHash, randomUUID } from "node:crypto";
import { constants } from "node:fs";
import { lstat, mkdir, open, opendir, realpath, rename, rm } from "node:fs/promises";
import { join, relative, resolve, sep } from "node:path";
import { z } from "zod";
import type { VendorUsageRead } from "./codex-usage";

const MAX = Number.MAX_SAFE_INTEGER, DIGEST = /^[a-f0-9]{64}$/, MAX_SEEN = 10_000;
const count = z.number().int().nonnegative().max(MAX);
const totalsSchema = z.object({ input: count, output: count }).strict();
const seenSchema = z.record(z.string().regex(DIGEST), totalsSchema).refine((value) => Object.keys(value).length <= MAX_SEEN);
const stateSchema = z.object({ version: z.literal(1), file: z.string(), inode: z.string(), size: count, modified: z.number().nonnegative(), offset: count,
  totals: totalsSchema, seen: seenSchema, problem: z.enum(["invalid message identity", "invalid usage record", "usage index bound", "malformed usage record", "record exceeds the source bound"]).nullable() }).strict();
type State = z.infer<typeof stateSchema>;
const unknown = (reason: string): VendorUsageRead => ({ input: null, output: null, scope: "provider-cumulative",
  source: `Claude cumulative usage unavailable: ${reason}. Historical/complete totals unavailable.` });
const optionalCount = (value: unknown): number | null => value === undefined ? 0 : count.safeParse(value).success ? value as number : null;
const safeHandle = (value: string) => /^[A-Za-z0-9][A-Za-z0-9._-]{0,255}$/.test(value);

/** Only assistant records outside sidechains contribute; a repeated message ID replaces its earlier usage. */
export function consumeClaudeLine(state: State, line: string): void {
  if (state.problem) return;
  try {
    const row = JSON.parse(line);
    if (row?.type !== "assistant" || row?.isSidechain) return;
    const message = row?.message, usage = message?.usage;
    if (!usage || typeof usage !== "object") return;
    if (typeof message?.id !== "string" || !message.id || message.id.length > 1024) { state.problem = "invalid message identity"; return; }
    const inputTokens = count.safeParse(usage.input_tokens), outputTokens = count.safeParse(usage.output_tokens);
    const cacheRead = optionalCount(usage.cache_read_input_tokens), cacheCreation = optionalCount(usage.cache_creation_input_tokens);
    if (!inputTokens.success || !outputTokens.success || cacheRead === null || cacheCreation === null) { state.problem = "invalid usage record"; return; }
    const input = inputTokens.data + cacheRead + cacheCreation, output = outputTokens.data;
    if (!Number.isSafeInteger(input) || !Number.isSafeInteger(output)) { state.problem = "invalid usage record"; return; }
    const key = createHash("sha256").update(message.id).digest("hex"), previous = state.seen[key];
    if (!previous && Object.keys(state.seen).length >= MAX_SEEN) { state.problem = "usage index bound"; return; }
    const next = { input: state.totals.input - (previous?.input ?? 0) + input, output: state.totals.output - (previous?.output ?? 0) + output };
    if (!Number.isSafeInteger(next.input) || !Number.isSafeInteger(next.output) || next.input < 0 || next.output < 0) { state.problem = "invalid usage record"; return; }
    state.seen[key] = { input, output }; state.totals = next;
  } catch { state.problem = "malformed usage record"; }
}

/** Every `<project>/<handle>.jsonl` under the projects root, keyed by native handle, as root-relative paths; symlinks are skipped. */
async function transcriptIndex(projectsRoot: string): Promise<Map<string, string[]>> {
  const found = new Map<string, string[]>(); let directories = 0, entries = 0;
  let root;
  try { root = await opendir(projectsRoot); }
  catch (error) { if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") return found; throw error; }
  for await (const project of root) {
    if (++directories > 4096) throw new Error("index bound");
    if (project.isSymbolicLink() || !project.isDirectory()) continue;
    let directory;
    try { directory = await opendir(join(projectsRoot, project.name)); } catch { continue; }
    for await (const item of directory) {
      if (++entries > 100_000) throw new Error("index bound");
      if (item.isSymbolicLink() || !item.isFile() || !item.name.endsWith(".jsonl")) continue;
      const handle = item.name.slice(0, -6);
      if (safeHandle(handle)) found.set(handle, [...(found.get(handle) ?? []), join(project.name, item.name)]);
    }
  }
  return found;
}

/** Absolute paths of every transcript for one exact native handle; callers decide what anything but one match means. */
export async function claudeTranscriptPaths(projectsRoot: string, handle: string): Promise<string[]> {
  if (!safeHandle(handle)) return [];
  return ((await transcriptIndex(projectsRoot)).get(handle) ?? []).map((path) => join(projectsRoot, path));
}

/** One exact native Claude JSONL, read incrementally and cached as numeric totals plus hashed message IDs. */
export function createClaudeUsageReader(projectsRoot: string, cacheRoot: string, now = Date.now) {
  let indexedAt = -Infinity, paths = new Map<string, string[]>(), indexing: Promise<void> | null = null;
  const tails = new Map<string, Promise<unknown>>();
  const namespace = createHash("sha256").update(resolve(projectsRoot)).digest("hex");
  async function index() {
    if (now() - indexedAt < 5_000) return;
    if (indexing) return indexing;
    indexing = (async () => { paths = await transcriptIndex(projectsRoot); indexedAt = now(); })();
    try { await indexing; } finally { indexing = null; }
  }
  const cacheFile = (handle: string) => join(cacheRoot, `${createHash("sha256").update(`${namespace}\0${handle}`).digest("hex")}.json`);
  async function cached(handle: string): Promise<State | null> {
    let file;
    try { file = await open(cacheFile(handle), constants.O_RDONLY | constants.O_NOFOLLOW); }
    catch (error) { if (error && typeof error === "object" && "code" in error && error.code === "ENOENT") return null; throw error; }
    try {
      const buffer = Buffer.alloc(4 * 1024 * 1024 + 1), result = await file.read(buffer, 0, buffer.length, 0);
      if (result.bytesRead > 4 * 1024 * 1024) throw new Error("cache bound");
      return stateSchema.parse(JSON.parse(buffer.subarray(0, result.bytesRead).toString("utf8")));
    } finally { await file.close(); }
  }
  async function save(state: State) {
    await mkdir(cacheRoot, { recursive: true, mode: 0o700 });
    const path = cacheFile(state.file.split(sep).at(-1)!.slice(0, -6)), temporary = `${path}.${randomUUID()}.tmp`;
    try {
      const file = await open(temporary, "wx", 0o600);
      try { await file.writeFile(JSON.stringify(stateSchema.parse(state))); await file.sync(); } finally { await file.close(); }
      await rename(temporary, path);
    } finally { await rm(temporary, { force: true }); }
  }
  async function read(handle: string, budget: { bytes: number }): Promise<VendorUsageRead> {
    if (!safeHandle(handle)) return unknown("invalid native session identity");
    try {
      await index();
      const matches = paths.get(handle) ?? [];
      if (matches.length !== 1) return unknown(matches.length ? "ambiguous exact source" : "no exact source in the configured namespace");
      const path = join(projectsRoot, matches[0]!);
      const root = await realpath(projectsRoot), target = await realpath(path), location = relative(root, target);
      if (location.startsWith(`..${sep}`) || location === ".." || resolve(root, location) !== target || (await lstat(path)).isSymbolicLink()) return unknown("source outside the allowed namespace");
      const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
      try {
        const stat = await file.stat(), inode = `${stat.dev}:${stat.ino}`;
        if (!stat.isFile()) return unknown("source is not a regular file");
        const old = await cached(handle);
        const state: State = old && old.file === matches[0] && old.inode === inode && old.offset <= stat.size && !(old.size === stat.size && old.modified !== stat.mtimeMs) ? old
          : { version: 1, file: matches[0]!, inode, size: stat.size, modified: stat.mtimeMs, offset: 0, totals: { input: 0, output: 0 }, seen: {}, problem: null };
        let position = state.offset, pending = Buffer.alloc(0);
        const end = stat.size;
        while (position < end && budget.bytes > 0 && !state.problem) {
          const size = Math.min(64 * 1024, end - position, budget.bytes); budget.bytes -= size;
          const buffer = Buffer.alloc(size), { bytesRead } = await file.read(buffer, 0, size, position);
          if (!bytesRead) break;
          position += bytesRead; pending = Buffer.concat([pending, buffer.subarray(0, bytesRead)]);
          let newline;
          while ((newline = pending.indexOf(10)) !== -1) {
            if (newline > 16 * 1024 * 1024) { state.problem = "record exceeds the source bound"; break; }
            consumeClaudeLine(state, pending.subarray(0, newline).toString("utf8"));
            state.offset += newline + 1; pending = pending.subarray(newline + 1);
            if (state.problem) break;
          }
          if (pending.length > 16 * 1024 * 1024) state.problem = "record exceeds the source bound";
        }
        const after = await file.stat(), current = await lstat(path);
        if (`${after.dev}:${after.ino}` !== inode || after.size < end || (after.size === end && after.mtimeMs !== stat.mtimeMs) || `${current.dev}:${current.ino}` !== inode) return unknown("source changed during read");
        state.size = end; state.modified = stat.mtimeMs;
        await save(state);
        if (state.problem) return unknown(state.problem);
        if (position === end && state.offset < end) return unknown("awaiting source record completion");
        if (state.offset < end) return unknown("incomplete bounded source read");
        if (!Object.keys(state.seen).length) return unknown("no assistant usage");
        return { input: state.totals.input, output: state.totals.output, scope: "provider-cumulative",
          source: "Claude Code cumulative assistant-message totals from one exact native JSONL at this host's configured/default projects root; input includes cache reads and cache creation, output is output_tokens; sidechains and subagents are excluded. Historical completeness unverified." };
      } finally { await file.close(); }
    } catch { return unknown("source or numeric cache unreadable"); }
  }
  return {
    read(handle: string, budget = { bytes: 32 * 1024 * 1024 }) {
      const previous = tails.get(handle) ?? Promise.resolve();
      const result = previous.catch(() => undefined).then(() => read(handle, budget)); tails.set(handle, result);
      return result.finally(() => { if (tails.get(handle) === result) tails.delete(handle); });
    },
  };
}
