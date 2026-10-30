import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { mkdir, open } from "node:fs/promises";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import type { PaseoAgent } from "@getpaseo/client";
import { claudeTranscriptPaths } from "./claude-usage";
import type { BackgroundReader } from "./takeover-brief";
import { nativeIdentity, type VerifiedRoute } from "./vendor-usage";

export type BackgroundReaderDeps = {
  /** The exact source record, or null when the host has none. */
  exact: (id: string) => Promise<PaseoAgent | null>;
  /** The lifecycle revision the review pinned; passed in so this module never imports the lifecycle. */
  revision: (agent: PaseoAgent) => string;
  route: (provider: string) => Promise<VerifiedRoute | null>;
  /** Immutable content-addressed snapshots, one per distinct accounting. */
  stateRoot: string;
  now?: () => number;
};
type Worker = { id: string; kind: string; status: string; taskId?: string };
type Read = { workers: Worker[] } | { gap: string };

export const CODEX_BACKGROUND_GAP = "Codex provider-native background accounting is not qualified; nothing is retired. Use ordinary Take over, which makes no background-transfer claim.";
const TRANSCRIPT_LIMIT = 128 * 1024 * 1024, OPENCODE_DEPTH = 8, OPENCODE_ROWS = 500;
const TERMINAL = new Set(["completed", "failed", "killed", "stopped"]);
const NOTIFICATION = /<task-notification>([\s\S]*?)<\/task-notification>/g;
const tag = (body: string, name: string) => new RegExp(`<${name}>([^<]*)</${name}>`).exec(body)?.[1]?.trim() || null;
const byId = (left: Worker, right: Worker) => left.id < right.id ? -1 : left.id > right.id ? 1 : 0;

/**
 * Provider-native work owned by the source runtime that has no Paseo ID: Claude main-chain background launches and
 * OpenCode child sessions. A complete result comes only from a positive full read; it never prompts, stops or labels.
 */
export function createBackgroundReader(deps: BackgroundReaderDeps): BackgroundReader {
  const now = deps.now ?? Date.now;
  return async ({ sourceId, sourceRevision, requestId }) => {
    try {
      const agent = await deps.exact(sourceId);
      if (!agent || agent.id !== sourceId) return { complete: false, gap: "The exact source session could not be read; nothing is retired." };
      if (deps.revision(agent) !== sourceRevision) return { complete: false, gap: "The source changed since review; nothing is retired." };
      const route = await deps.route(agent.provider);
      if (!route) return { complete: false, gap: "This provider route is not a verified Claude or OpenCode source; nothing is retired." };
      if (route.base === "codex") return { complete: false, gap: CODEX_BACKGROUND_GAP };
      const handle = nativeIdentity(agent, route);
      if (!handle) return { complete: false, gap: "The exact native session identity could not be verified; nothing is retired." };
      const read = route.base === "claude" ? await readClaude(route.source, handle) : readOpenCode(route.source, handle);
      if ("gap" in read) return { complete: false, gap: read.gap };
      const bytes = JSON.stringify({ version: 1, provider: route.base, source: route.source, handle, workers: read.workers.sort(byId) });
      const revision = createHash("sha256").update(bytes).digest("hex"), path = join(deps.stateRoot, `${revision}.json`);
      await writeOnce(deps.stateRoot, path, bytes);
      return { complete: true, accounting: { sourceId, sourceRevision, requestId, observedAt: new Date(now()).toISOString(), revision,
        workers: read.workers.map((worker) => ({ id: worker.id, state: "finished" as const, checkpoint: { path, sha256: revision } })) } };
    } catch (error) {
      return { complete: false, gap: `Provider-native background read failed: ${error instanceof Error ? error.message : String(error)}` };
    }
  };
}

async function writeOnce(root: string, path: string, bytes: string) {
  await mkdir(root, { recursive: true, mode: 0o700 });
  try {
    const file = await open(path, constants.O_WRONLY | constants.O_CREAT | constants.O_EXCL | constants.O_NOFOLLOW, 0o600);
    try { await file.writeFile(bytes); await file.sync(); } finally { await file.close(); }
  } catch (error) {
    if (!(error && typeof error === "object" && "code" in error && error.code === "EEXIST")) throw error;
    const file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW);
    try { if ((await file.readFile()).toString() !== bytes) throw new Error("the background snapshot differs from its content address"); }
    finally { await file.close(); }
  }
}

/** Main-chain launches are terminal only through a status-bearing task notification; a Monitor event or expiry is not. */
async function readClaude(projectsRoot: string, handle: string): Promise<Read> {
  const paths = await claudeTranscriptPaths(projectsRoot, handle);
  if (paths.length !== 1) return { gap: `Expected exactly one Claude transcript for this session and found ${paths.length}; nothing is retired.` };
  const file = await open(paths[0]!, constants.O_RDONLY | constants.O_NOFOLLOW);
  let bytes: Buffer;
  try {
    const stat = await file.stat();
    if (!stat.isFile() || stat.size > TRANSCRIPT_LIMIT) return { gap: "The Claude transcript is not a bounded regular file; nothing is retired." };
    bytes = await file.readFile();
  } finally { await file.close(); }
  if (bytes.length > TRANSCRIPT_LIMIT) return { gap: "The Claude transcript is not a bounded regular file; nothing is retired." };
  const launches = new Map<string, string>(), tasks = new Map<string, string>(), ends: { toolUse: string | null; task: string | null; status: string }[] = [];
  // The final line without a newline is a write in progress.
  const lines = bytes.toString("utf8").split("\n").slice(0, -1);
  for (const [index, line] of lines.entries()) {
    let row;
    try { row = JSON.parse(line); } catch { return { gap: `The Claude transcript has a malformed record at line ${index + 1}; nothing is retired.` }; }
    if (row?.isSidechain) continue;
    const content = row?.message?.content, texts: string[] = [];
    if (row?.type === "user" && typeof content === "string") texts.push(content);
    for (const block of Array.isArray(content) ? content : []) {
      if (block?.type === "tool_use" && typeof block.id === "string" && (block.name === "Monitor" || block.input?.run_in_background === true)) launches.set(block.id, String(block.name));
      if (block?.type === "tool_result" && typeof block.tool_use_id === "string") {
        const task = row.toolUseResult?.backgroundTaskId ?? row.toolUseResult?.taskId;
        if (typeof task === "string" && task) tasks.set(task, block.tool_use_id);
      }
      if (row?.type === "user" && block?.type === "text" && typeof block.text === "string") texts.push(block.text);
    }
    for (const text of texts) for (const [, body] of text.matchAll(NOTIFICATION)) {
      const status = tag(body!, "status");
      if (status && TERMINAL.has(status)) ends.push({ toolUse: tag(body!, "tool-use-id"), task: tag(body!, "task-id"), status });
    }
  }
  const taskOf = new Map([...tasks].map(([task, toolUse]) => [toolUse, task])), done = new Map<string, string>();
  for (const end of ends) {
    const id = end.toolUse && launches.has(end.toolUse) ? end.toolUse : end.task ? tasks.get(end.task) : undefined;
    if (id && launches.has(id)) done.set(id, end.status);
  }
  const unmarked = [...launches.keys()].filter((id) => !done.has(id)).sort();
  if (unmarked.length) return { gap: `${unmarked.length} provider-native background launches have no terminal record (first 20 ids: ${unmarked.slice(0, 20).join(", ")}). Claude leaves none after a UI stop, a Monitor timeout or teardown. Nothing is retired. Let them finish, stop them in the source, or use ordinary Take over, which makes no background-transfer claim.` };
  return { workers: [...launches].map(([id, kind]) => ({ id, kind, status: done.get(id)!, ...(taskOf.get(id) ? { taskId: taskOf.get(id) } : {}) })) };
}

/** Child sessions through `parent_id`, each settled: no pending/running tool and a completed latest assistant message. */
function readOpenCode(databasePath: string, handle: string): Read {
  const database = new DatabaseSync(databasePath, { readOnly: true });
  try {
    if (!database.prepare("SELECT id FROM session WHERE id = ?").get(handle)) return { gap: "The exact OpenCode session row is missing; nothing is retired." };
    const task = database.prepare("SELECT count(*) AS n FROM part WHERE session_id = ? AND json_extract(data, '$.type') = 'tool' AND json_extract(data, '$.tool') = 'task' AND json_extract(data, '$.state.status') IN ('pending', 'running')").get(handle) as { n: number };
    if (task.n) return { gap: "The source has an OpenCode task still pending or running. Nothing is retired. Let it finish, stop it in the source, or use ordinary Take over, which makes no background-transfer claim." };
    const children = database.prepare("SELECT id FROM session WHERE parent_id = ? ORDER BY id LIMIT ?");
    const descendants: string[] = [];
    let level = [handle];
    for (let depth = 1; level.length; depth++) {
      const next = level.flatMap((id) => (children.all(id, OPENCODE_ROWS + 1) as { id: string }[]).map((row) => row.id));
      if (next.length && depth > OPENCODE_DEPTH) return { gap: `OpenCode child sessions are nested deeper than ${OPENCODE_DEPTH}; nothing is retired.` };
      descendants.push(...next);
      if (descendants.length > OPENCODE_ROWS) return { gap: `The source has more than ${OPENCODE_ROWS} OpenCode child sessions; nothing is retired.` };
      level = next;
    }
    const busy = database.prepare("SELECT count(*) AS n FROM part WHERE session_id = ? AND json_extract(data, '$.type') = 'tool' AND json_extract(data, '$.state.status') IN ('pending', 'running')");
    const latest = database.prepare("SELECT json_extract(data, '$.time.completed') AS completed FROM message WHERE session_id = ? AND json_extract(data, '$.role') = 'assistant' ORDER BY time_created DESC, id DESC LIMIT 1");
    const unsettled = descendants.filter((id) => {
      const completed = (latest.get(id) as { completed: unknown } | undefined)?.completed;
      return (busy.get(id) as { n: number }).n > 0 || typeof completed !== "number" || !Number.isFinite(completed);
    });
    if (unsettled.length) return { gap: `${unsettled.length} OpenCode child sessions are not finished (first 20 ids: ${unsettled.slice(0, 20).join(", ")}). Nothing is retired. Let them finish, stop them in the source, or use ordinary Take over, which makes no background-transfer claim.` };
    return { workers: descendants.map((id) => ({ id, kind: "child-session", status: "completed" })) };
  } finally { database.close(); }
}
