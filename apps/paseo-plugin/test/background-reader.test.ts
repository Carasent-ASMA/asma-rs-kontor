import { createHash } from "node:crypto";
import { appendFile, mkdir, mkdtemp, readdir, readFile, rm, truncate, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { DatabaseSync } from "node:sqlite";
import { afterEach, describe, expect, it } from "vitest";
import type { PaseoAgent } from "@getpaseo/client";
import { CODEX_BACKGROUND_GAP, createBackgroundReader, type BackgroundReaderDeps } from "../server/background-reader";
import { readPreparedBackground } from "../server/takeover-brief";
import { verifiedVendorRoute } from "../server/vendor-usage";

const SOURCE = "agent-1", HANDLE = "0a1b2c3d-native", REVISION = "source-rev-1";
const hash = (bytes: string | Buffer) => createHash("sha256").update(bytes).digest("hex");
const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

async function fixture(provider = "claude", base = provider) {
  const root = await mkdtemp(join(tmpdir(), "kontor-background-")); dirs.push(root);
  const projects = join(root, "projects"), stateRoot = join(root, "state"), database = join(root, "opencode.db");
  const agent = { id: SOURCE, provider, persistence: { provider: base, sessionId: HANDLE, nativeHandle: HANDLE } } as PaseoAgent;
  let clock = Date.parse("2026-10-30T12:00:00Z");
  const deps: BackgroundReaderDeps = { exact: async () => agent, revision: () => REVISION, stateRoot, now: () => clock += 1000,
    route: async (name) => name === provider ? { base: base as "claude", provider: name, source: base === "opencode" ? database : projects } : null };
  const read = (reader = createBackgroundReader(deps)) => reader({ sourceId: SOURCE, sourceRevision: REVISION, requestId: "11111111-1111-4111-8111-111111111111" });
  return { root, projects, stateRoot, database, agent, deps, read };
}

// Claude transcript records, in the shapes Claude Code writes them.
const launch = (id: string, name: string, background = true, extra: Record<string, unknown> = {}) =>
  ({ type: "assistant", message: { role: "assistant", content: [{ type: "tool_use", id, name, input: { command: "sleep 9", ...(background ? { run_in_background: true } : {}) } }] }, ...extra });
const result = (id: string, toolUseResult: Record<string, unknown>) =>
  ({ type: "user", message: { role: "user", content: [{ type: "tool_result", tool_use_id: id, content: "started" }] }, toolUseResult });
const notice = (fields: Record<string, string>, asBlock = false, origin = true) => {
  const text = `<task-notification>\n${Object.entries(fields).map(([key, value]) => `<${key}>${value}</${key}>`).join("\n")}\n</task-notification>`;
  return { type: "user", ...(origin ? { origin: { kind: "task-notification" } } : {}), message: { role: "user", content: asBlock ? [{ type: "text", text }] : text } };
};
// An async Agent that finished, then a SendMessage resume of it.
const AGENT = "adaf34b20dc8e9128";
const resumed = [launch("toolu_agent", "Agent"), result("toolu_agent", { isAsync: true, status: "async_launched", agentId: AGENT }),
  notice({ "task-id": AGENT, status: "completed" }), launch("toolu_msg", "SendMessage", false), result("toolu_msg", { success: true, message: "Message sent" }),
  launch("toolu_send", "SendMessage", false), result("toolu_send", { success: true, resumedAgentId: AGENT })];
const say = (text: string) => ({ type: "assistant", message: { role: "assistant", content: [{ type: "text", text }] } });
async function transcript(projects: string, rows: unknown[], tail = "", project = "-tmp-project", handle = HANDLE) {
  await mkdir(join(projects, project), { recursive: true });
  const path = join(projects, project, `${handle}.jsonl`);
  await writeFile(path, rows.map((row) => `${JSON.stringify(row)}\n`).join("") + tail);
  return path;
}

describe("Claude provider-native background", () => {
  it("C1 completes an empty positive read with a content-addressed snapshot", async () => {
    const f = await fixture(); await transcript(f.projects, [say("no background work")]);
    const read = await f.read(); if (!read.complete) throw new Error(read.gap);
    expect(read.accounting.workers).toEqual([]);
    const bytes = await readFile(join(f.stateRoot, `${read.accounting.revision}.json`));
    expect(hash(bytes)).toBe(read.accounting.revision);
    expect(JSON.parse(bytes.toString())).toEqual({ version: 1, provider: "claude", source: f.projects, handle: HANDLE, workers: [] });
  });
  it("C2 finishes a background Bash by its tool-use id, and the lifecycle accepts the checkpoint", async () => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_bash", "Bash"), result("toolu_bash", { backgroundTaskId: "bash1" }),
      notice({ "task-id": "bash1", "tool-use-id": "toolu_bash", status: "completed", summary: "done" })]);
    const reader = createBackgroundReader({ ...f.deps, now: Date.now });
    const read = await readPreparedBackground(reader, { sourceId: SOURCE, sourceRevision: REVISION, requestId: "11111111-1111-4111-8111-111111111111" });
    expect(read.workers).toEqual([{ id: "toolu_bash", state: "finished", checkpoint: { path: join(f.stateRoot, `${read.revision}.json`), sha256: read.revision } }]);
  });
  it("C3 finishes a Monitor through its task id only", async () => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_mon", "Monitor", false), result("toolu_mon", { taskId: "bmon1", timeoutMs: 1000 }),
      notice({ "task-id": "bmon1", status: "killed" }, true)]);
    const read = await f.read(); if (!read.complete) throw new Error(read.gap);
    expect(read.accounting.workers.map((worker) => worker.id)).toEqual(["toolu_mon"]);
  });
  it("C4 refuses an unmarked background Agent by id and writes no snapshot", async () => {
    const f = await fixture(); await transcript(f.projects, [launch("toolu_agent", "Agent"), result("toolu_agent", { backgroundTaskId: "a1" })]);
    const read = await f.read();
    expect(read).toEqual({ complete: false, gap: expect.stringContaining("1 provider-native background launches have no terminal record (first 20 ids: toolu_agent)") });
    await expect(readdir(f.stateRoot)).rejects.toThrow();
  });
  it("C5 does not treat a status-less Monitor event or expiry as terminal", async () => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_mon", "Monitor", false), result("toolu_mon", { taskId: "bmon1" }),
      notice({ "task-id": "bmon1", summary: "Monitor event", event: "[Monitor expired after 20m]" })]);
    expect((await f.read()).complete).toBe(false);
  });
  it("C6 ignores sidechain launches", async () => {
    const f = await fixture(); await transcript(f.projects, [launch("toolu_side", "Bash", true, { isSidechain: true }), say("main chain")]);
    const read = await f.read(); if (!read.complete) throw new Error(read.gap);
    expect(read.accounting.workers).toEqual([]);
  });
  it("C7 refuses a malformed complete line", async () => {
    const f = await fixture(); await mkdir(join(f.projects, "p"), { recursive: true });
    await writeFile(join(f.projects, "p", `${HANDLE}.jsonl`), `${JSON.stringify(say("a"))}\n{"type":\n${JSON.stringify(say("b"))}\n`);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("malformed record at line 2") });
  });
  it("C7 skips only a trailing partial line", async () => {
    const f = await fixture(); await transcript(f.projects, [say("a")], '{"type":"assistant","message":{"content":[{"type":"tool_use","id":"toolu_x","name":"Bash","input":{"run_in_');
    expect((await f.read()).complete).toBe(true);
  });
  it.each([0, 2])("C8 refuses %i transcripts for the handle", async (count) => {
    const f = await fixture(); await mkdir(f.projects, { recursive: true });
    for (let index = 0; index < count; index++) await transcript(f.projects, [say("a")], "", `-project-${index}`);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining(`found ${count}`) });
  });
  it("C9 refuses a transcript over the size bound", async () => {
    const f = await fixture(), path = await transcript(f.projects, [say("a")]);
    await truncate(path, 128 * 1024 * 1024 + 1);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("not a bounded regular file") });
  });
  it("C10 finishes an async Agent through a padded agentId task id only", async () => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_agent", "Agent"), result("toolu_agent", { isAsync: true, status: "async_launched", agentId: "adaf34b20dc8e9128" }),
      notice({ "task-id": "  adaf34b20dc8e9128\n", status: "completed", summary: "Agent done" })]);
    const read = await f.read(); if (!read.complete) throw new Error(read.gap);
    expect(read.accounting.workers.map((worker) => worker.id)).toEqual(["toolu_agent"]);
  });
  it.each([{ isAsync: true }, { status: "async_launched" }])("C11 refuses an unflagged async Agent (%o) with no notification", async (async) => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_agent", "Agent", false), result("toolu_agent", { ...async, agentId: "a0123456789abcdef" })]);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("1 provider-native background launches have no terminal record (first 20 ids: toolu_agent)") });
  });
  it("C12 refuses an Agent or Task use with no result yet, not a finished synchronous one", async () => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_sync", "Agent", false), result("toolu_sync", { status: "completed", agentId: "a1111111111111111" }), launch("toolu_pending", "Task", false)]);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("1 provider-native background launches have no terminal record (first 20 ids: toolu_pending)") });
  });
  it("C13 refuses an unflagged Bash that Claude moved to the background", async () => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_bash", "Bash", false), result("toolu_bash", { backgroundTaskId: "bauto1", timedOutAfterMs: 600000 })]);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("1 provider-native background launches have no terminal record (first 20 ids: toolu_bash)") });
  });
  it("C13 finishes that Bash through its tool-use id", async () => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_bash", "Bash", false), result("toolu_bash", { backgroundTaskId: "bauto1", timedOutAfterMs: 600000 }),
      notice({ "tool-use-id": "toolu_bash", status: "completed" })]);
    const read = await f.read(); if (!read.complete) throw new Error(read.gap);
    expect(read.accounting.workers.map((worker) => worker.id)).toEqual(["toolu_bash"]);
  });
  it("C14 keeps a SendMessage resume open after the agent's first run finished", async () => {
    const f = await fixture(); await transcript(f.projects, resumed);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("1 provider-native background launches have no terminal record (first 20 ids: toolu_send)") });
  });
  it("C14 finishes the resume through a later task-only notice", async () => {
    const f = await fixture(); await transcript(f.projects, [...resumed, notice({ "task-id": AGENT, status: "failed" })]);
    const read = await f.read(); if (!read.complete) throw new Error(read.gap);
    expect(JSON.parse(await readFile(join(f.stateRoot, `${read.accounting.revision}.json`), "utf8")).workers).toEqual([
      { id: "toolu_agent", kind: "Agent", status: "completed", taskId: AGENT }, { id: "toolu_send", kind: "SendMessage", status: "failed", taskId: AGENT }]);
  });
  it("C14 refuses a resume whose resumed and revived ids differ", async () => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_send", "SendMessage", false), result("toolu_send", { resumedAgentId: AGENT, revivedAgentId: "a0000000000000000" })]);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("resumed two different agents") });
  });
  it("C15 ignores an api-request-blob that replays the notice", async () => {
    const f = await fixture();
    await transcript(f.projects, [...resumed, { ...notice({ "task-id": AGENT, status: "completed" }), type: "api-request-blob", sessionId: HANDLE, hash: "h" }]);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("(first 20 ids: toolu_send)") });
  });
  it("C16 does not close a launch from a user line that only quotes a notice", async () => {
    const f = await fixture();
    await transcript(f.projects, [launch("toolu_bash", "Bash"), result("toolu_bash", { backgroundTaskId: "bash1" }),
      notice({ "tool-use-id": "toolu_bash", status: "completed" }, false, false)]);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("(first 20 ids: toolu_bash)") });
  });
  it("C17 closes only the latest open launch for a task-only notice", async () => {
    const f = await fixture();
    await transcript(f.projects, [...resumed.filter((row) => row !== resumed[2]), notice({ "task-id": AGENT, status: "completed" })]);
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("1 provider-native background launches have no terminal record (first 20 ids: toolu_agent)") });
  });
});

describe("OpenCode provider-native background", () => {
  function seed(path: string, setup: (db: { session(id: string, parent?: string | null): void; message(session: string, data: unknown, at?: number): void; part(session: string, data: unknown): void }) => void) {
    const db = new DatabaseSync(path);
    db.exec("CREATE TABLE session (id text PRIMARY KEY, parent_id text); CREATE TABLE message (id text PRIMARY KEY, session_id text, time_created integer, data text); CREATE TABLE part (id text PRIMARY KEY, message_id text, session_id text, data text);");
    let next = 0;
    db.exec("BEGIN");
    setup({
      session: (id, parent = null) => { db.prepare("INSERT INTO session VALUES (?, ?)").run(id, parent); },
      message: (session, data, at = ++next) => { db.prepare("INSERT INTO message VALUES (?, ?, ?, ?)").run(`msg_${++next}`, session, at, JSON.stringify(data)); },
      part: (session, data) => { db.prepare("INSERT INTO part VALUES (?, ?, ?, ?)").run(`prt_${++next}`, "msg", session, JSON.stringify(data)); },
    });
    db.exec("COMMIT"); db.close();
  }
  const done = { role: "assistant", time: { created: 1, completed: 2 } };
  it("O1 completes a source without children", async () => {
    const f = await fixture("opencode"); seed(f.database, (db) => db.session(HANDLE));
    const read = await f.read(); if (!read.complete) throw new Error(read.gap);
    expect(read.accounting.workers).toEqual([]);
  });
  it("O2 finishes a child and its grandchild", async () => {
    const f = await fixture("opencode");
    seed(f.database, (db) => { db.session(HANDLE); db.session("ses_child", HANDLE); db.session("ses_grand", "ses_child");
      db.part(HANDLE, { type: "tool", tool: "task", state: { status: "completed" } }); db.message("ses_child", done); db.message("ses_grand", done); });
    const read = await f.read(); if (!read.complete) throw new Error(read.gap);
    expect(read.accounting.workers.map((worker) => [worker.id, worker.state])).toEqual([["ses_child", "finished"], ["ses_grand", "finished"]]);
  });
  it("O3 refuses a child with a running tool", async () => {
    const f = await fixture("opencode");
    seed(f.database, (db) => { db.session(HANDLE); db.session("ses_child", HANDLE); db.message("ses_child", done); db.part("ses_child", { type: "tool", tool: "bash", state: { status: "running" } }); });
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("ses_child") });
  });
  it("O4 refuses a child whose latest assistant message has not completed", async () => {
    const f = await fixture("opencode");
    seed(f.database, (db) => { db.session(HANDLE); db.session("ses_child", HANDLE); db.message("ses_child", done, 1); db.message("ses_child", { role: "assistant", time: { created: 3 } }, 5); });
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("ses_child") });
  });
  it("O5 refuses a running source task", async () => {
    const f = await fixture("opencode"); seed(f.database, (db) => { db.session(HANDLE); db.part(HANDLE, { type: "tool", tool: "task", state: { status: "running" } }); });
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("task still pending or running") });
  });
  it("O6 refuses more than 500 descendants", async () => {
    const f = await fixture("opencode");
    seed(f.database, (db) => { db.session(HANDLE); for (let index = 0; index < 501; index++) { db.session(`ses_${index}`, HANDLE); db.message(`ses_${index}`, done); } });
    expect(await f.read()).toEqual({ complete: false, gap: expect.stringContaining("more than 500") });
  });
});

describe("source, route and revision gates", () => {
  it("G1 refuses Codex with the exact disclosure", async () => {
    const f = await fixture("codex");
    expect(await f.read()).toEqual({ complete: false, gap: CODEX_BACKGROUND_GAP });
    expect(CODEX_BACKGROUND_GAP).toBe("Codex provider-native background accounting is not qualified; nothing is retired. Use ordinary Take over, which makes no background-transfer claim.");
  });
  it("G2 refuses an alias the host does not verify", async () => {
    const f = await fixture("claude-alt", "claude"); await transcript(f.projects, [say("a")]);
    const paseo = { agents: {}, providers: { snapshot: async () => ({ entries: [{ provider: "claude-alt", source: "custom" }] }) }, config: { get: async () => ({ config: { providers: { "claude-alt": {} } } }) } };
    const read = await f.read(createBackgroundReader({ ...f.deps, route: (provider) => verifiedVendorRoute(paseo as never, provider) }));
    expect(read).toEqual({ complete: false, gap: expect.stringContaining("not a verified Claude or OpenCode source") });
  });
  it("G3 refuses a source revision other than the reviewed one", async () => {
    const f = await fixture(); await transcript(f.projects, [say("a")]);
    expect(await f.read(createBackgroundReader({ ...f.deps, revision: () => "source-rev-2" }))).toEqual({ complete: false, gap: expect.stringContaining("changed since review") });
  });
  it("G4 refuses when the exact source is not on the host", async () => {
    const f = await fixture(); await transcript(f.projects, [say("a")]);
    expect((await f.read(createBackgroundReader({ ...f.deps, exact: async () => null }))).complete).toBe(false);
  });
});

describe("review/apply stability", () => {
  const base = [launch("toolu_bash", "Bash"), result("toolu_bash", { backgroundTaskId: "bash1" }), notice({ "tool-use-id": "toolu_bash", status: "failed" })];
  it("S1 keeps the revision when only non-background lines are appended", async () => {
    const f = await fixture(), path = await transcript(f.projects, base);
    const first = await f.read();
    await appendFile(path, [say("more"), launch("toolu_sync", "Bash", false), result("toolu_sync", {})].map((row) => `${JSON.stringify(row)}\n`).join(""));
    const second = await f.read();
    if (!first.complete || !second.complete) throw new Error("incomplete");
    expect(second.accounting.observedAt).not.toBe(first.accounting.observedAt);
    expect(second.accounting.revision).toBe(first.accounting.revision);
  });
  it("S2 changes the revision when a launch is added", async () => {
    const f = await fixture(), path = await transcript(f.projects, base);
    const first = await f.read();
    await appendFile(path, [launch("toolu_more", "Bash"), notice({ "tool-use-id": "toolu_more", status: "completed" })].map((row) => `${JSON.stringify(row)}\n`).join(""));
    const second = await f.read();
    if (!first.complete || !second.complete) throw new Error("incomplete");
    expect(second.accounting.revision).not.toBe(first.accounting.revision);
  });
});
