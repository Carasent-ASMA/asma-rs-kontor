import { chmod, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, afterEach, describe, expect, it } from "vitest";
import { readConfig } from "../server/config";
import { kontorStatusReader, observeKontord, STATUS_TTL_MS } from "../server/kontor-status";

const temporary: string[] = [];
afterAll(() => Promise.all(temporary.map((directory) => rm(directory, { recursive: true, force: true }))));
afterEach(() => { delete process.env.KONTOR_AUTH; delete process.env.JIRA_API_TOKEN; });

const STATE_ROOT = "/srv/kontor/state";

/** A kontor-mcp tool result: the document in one text block. */
const toolResult = (document: object, isError = false) => ({ content: [{ type: "text", text: JSON.stringify(document) }], ...(isError ? { isError } : {}) });
const realm = toolResult({ tool: "kontor_realm_get", status: 200, body: { realm_id: "realm-1" } });

/**
 * A disposable stand-in for kontor-mcp that ignores SIGTERM. It logs each start (arguments, environment
 * names, process ID), then answers the handshake and `kontor_realm_get` with `answer` (and leaves, if
 * asked), leaves at once ("exit"), never answers ("silent") or floods its output ("flood").
 */
async function fakeMcp(answer: object | "exit" | "silent" | "flood", leaveAfterAnswer = false) {
  const directory = await mkdtemp(join(tmpdir(), "kontor-mcp-fake-"));
  temporary.push(directory);
  const command = join(directory, "kontor-mcp");
  const log = join(directory, "starts");
  await writeFile(command, `#!/usr/bin/env node
const fs = require("node:fs");
fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify({ args: process.argv.slice(2), env: Object.keys(process.env), pid: process.pid }) + "\\n");
process.on("SIGTERM", () => {});
const answer = ${JSON.stringify(answer)};
if (answer === "exit") { process.stderr.write("kontor-mcp: no realm here\\n"); process.exit(1); }
if (answer === "flood") { const junk = "x".repeat(1023) + "\\n"; const pump = () => { while (process.stdout.write(junk)); process.stdout.once("drain", pump); }; pump(); }
let buffer = "";
process.stdin.setEncoding("utf8").on("data", (chunk) => {
  buffer += chunk;
  for (let newline = buffer.indexOf("\\n"); newline >= 0; newline = buffer.indexOf("\\n")) {
    const message = JSON.parse(buffer.slice(0, newline));
    buffer = buffer.slice(newline + 1);
    if (typeof answer === "string" || message.id === undefined) continue;
    const result = message.method === "initialize"
      ? { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: "fake", version: "0" } }
      : message.params.name === "kontor_realm_get" ? answer : null;
    process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }) + "\\n");
    if (${leaveAfterAnswer} && message.method === "tools/call") process.exit(0);
  }
});
setInterval(() => {}, 60_000);
`);
  await chmod(command, 0o700);
  const starts = async () => (await readFile(log, "utf8").catch(() => "")).split("\n").filter(Boolean)
    .map((line) => JSON.parse(line) as { args: string[]; env: string[]; pid: number });
  return { command, starts };
}

const configFor = (command: string, extra: Record<string, string> = {}) =>
  readConfig({ HOME: "/home/test", KONTOR_PLUGIN_STATE_ROOT: STATE_ROOT, KONTOR_PLUGIN_MCP_COMMAND: command, ...extra });

const alive = (pid: number) => { try { process.kill(pid, 0); return true; } catch { return false; } };
const pause = (ms: number) => new Promise((done) => setTimeout(done, ms));

describe("kontord observation through the observer kontor-mcp", () => {
  it("reads the realm with the arguments agent.create composes, giving the child only PATH and HOME", async () => {
    process.env.KONTOR_AUTH = "planted";
    process.env.JIRA_API_TOKEN = "planted";
    const fake = await fakeMcp(realm);
    expect(await observeKontord(configFor(fake.command))).toBe("ready");
    expect(await observeKontord(configFor(fake.command, { KONTOR_PLUGIN_SERVE_PROFILE: "observer-narrow" }))).toBe("ready");
    const [plain, profiled] = await fake.starts();
    expect(plain?.args).toEqual(["--state-root", STATE_ROOT, "--credential-tier", "observer"]);
    expect(profiled?.args).toEqual(["--state-root", STATE_ROOT, "--credential-tier", "observer", "--serve-profile", "observer-narrow"]);
    expect(plain?.env).not.toContain("KONTOR_AUTH");
    expect(plain?.env).not.toContain("JIRA_API_TOKEN");
    expect(plain?.env).toEqual(expect.arrayContaining(["PATH", "HOME"]));
  });

  it("reads an answer written just before kontor-mcp leaves", async () => {
    expect(await observeKontord(configFor((await fakeMcp(realm, true)).command))).toBe("ready");
  });

  it("tells an unavailable realm from one that refused the observer read", async () => {
    const down = await fakeMcp(toolResult({ tool: "kontor_realm_get", code: "unavailable", dispatched: false }, true));
    expect(await observeKontord(configFor(down.command))).toBe("unavailable");
    const refused = await fakeMcp(toolResult({ tool: "kontor_realm_get", status: 401, body: { code: "unauthenticated" } }, true));
    expect(await observeKontord(configFor(refused.command))).toBe("refused");
  });

  it("reads a missing realm, credential or command, silence and an odd answer as unknown, never as absent", async () => {
    expect(await observeKontord(configFor((await fakeMcp("exit")).command))).toBe("unknown");
    const directory = await mkdtemp(join(tmpdir(), "kontor-mcp-none-"));
    temporary.push(directory);
    expect(await observeKontord(configFor(join(directory, "kontor-mcp")))).toBe("unknown");
    expect(await observeKontord(configFor((await fakeMcp("silent")).command), 300)).toBe("unknown");
    expect(await observeKontord(configFor((await fakeMcp(toolResult({ tool: "kontor_realm_get", status: 200, body: {} }))).command))).toBe("unknown");
  });

  it("kills the child at the budget even when it ignores SIGTERM, and stops reading at the output limit", async () => {
    const stubborn = await fakeMcp("silent");
    expect(await observeKontord(configFor(stubborn.command), 1_500)).toBe("unknown");
    await pause(200);
    expect(alive((await stubborn.starts())[0]!.pid)).toBe(false);
    const flood = await fakeMcp("flood");
    const started = Date.now();
    expect(await observeKontord(configFor(flood.command), 10_000)).toBe("unknown");
    expect(Date.now() - started).toBeLessThan(5_000);
    await pause(200);
    expect(alive((await flood.starts())[0]!.pid)).toBe(false);
  });

  it("stops a probe in flight when the worker stops", async () => {
    const silent = await fakeMcp("silent");
    const stop = new AbortController();
    const started = Date.now();
    const probe = observeKontord(configFor(silent.command), 10_000, stop.signal);
    setTimeout(() => stop.abort(), 100);
    expect(await probe).toBe("unknown");
    expect(Date.now() - started).toBeLessThan(5_000);
  });
});

describe("kontor.status", () => {
  it("reports composition apart from the observation, and asks nothing with the MCP off", async () => {
    const fake = await fakeMcp(realm);
    const at = () => 0;
    expect(await kontorStatusReader(true, () => configFor(fake.command), at)()).toEqual({ observation: "ready", composition: "composed", checkedAt: new Date(0).toISOString() });
    expect(await kontorStatusReader(false, () => configFor(fake.command), at)()).toMatchObject({ observation: "ready", composition: "floor-off" });
    expect(await fake.starts()).toHaveLength(2);
    expect(await kontorStatusReader(true, () => configFor(fake.command, { KONTOR_PLUGIN_MCP: "off" }), at)()).toMatchObject({ observation: "unknown", composition: "mcp-off" });
    expect(await kontorStatusReader(false, () => configFor(fake.command, { KONTOR_PLUGIN_MCP: "off" }), at)()).toMatchObject({ observation: "unknown", composition: "floor-off" });
    expect(await fake.starts()).toHaveLength(2);
  });

  it("starts at most one probe per window for one configuration, failures included", async () => {
    const fake = await fakeMcp("exit");
    let clock = 1_000_000;
    let stateRoot = STATE_ROOT;
    const read = kontorStatusReader(true, () => configFor(fake.command, { KONTOR_PLUGIN_STATE_ROOT: stateRoot }), () => clock);
    const [first, concurrent] = await Promise.all([read(), read()]);
    clock += STATUS_TTL_MS - 1;
    expect(await read()).toBe(first);
    expect(concurrent).toBe(first);
    expect(first).toEqual({ observation: "unknown", composition: "composed", checkedAt: new Date(1_000_000).toISOString() });
    expect(await fake.starts()).toHaveLength(1);
    clock += 1;
    await read();
    expect(await fake.starts()).toHaveLength(2);
    stateRoot = "/srv/kontor/other";
    await read();
    expect((await fake.starts()).map((start) => start.args[1])).toEqual([STATE_ROOT, STATE_ROOT, "/srv/kontor/other"]);
  });
});
