import { chmod, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, afterEach, describe, expect, it } from "vitest";
import { readConfig } from "../server/config";
import { kontorStatusReader, probeKontorStatus, STATUS_TTL_MS } from "../server/kontor-status";

const temporary: string[] = [];
afterAll(() => Promise.all(temporary.map((directory) => rm(directory, { recursive: true, force: true }))));
afterEach(() => { delete process.env.KONTOR_AUTH; delete process.env.JIRA_API_TOKEN; });

const STATE_ROOT = "/srv/kontor/state";

/** A kontor-mcp tool result: the document in one text block. */
const toolResult = (document: object, isError = false) => ({ content: [{ type: "text", text: JSON.stringify(document) }], ...(isError ? { isError } : {}) });

/**
 * A disposable stand-in for kontor-mcp. It logs each start (arguments and environment names), then
 * answers the handshake and `kontor_realm_get` with `answer` (then leaves, if asked), leaves at once ("exit") or
 * never answers ("silent").
 */
async function fakeMcp(answer: object | "exit" | "silent", leaveAfterAnswer = false) {
  const directory = await mkdtemp(join(tmpdir(), "kontor-mcp-fake-"));
  temporary.push(directory);
  const command = join(directory, "kontor-mcp");
  const log = join(directory, "starts");
  await writeFile(command, `#!/usr/bin/env node
const fs = require("node:fs");
fs.appendFileSync(${JSON.stringify(log)}, JSON.stringify({ args: process.argv.slice(2), env: Object.keys(process.env) }) + "\\n");
const answer = ${JSON.stringify(answer)};
if (answer === "exit") { process.stderr.write("kontor-mcp: no realm here\\n"); process.exit(1); }
let buffer = "";
process.stdin.setEncoding("utf8").on("data", (chunk) => {
  buffer += chunk;
  for (let newline = buffer.indexOf("\\n"); newline >= 0; newline = buffer.indexOf("\\n")) {
    const message = JSON.parse(buffer.slice(0, newline));
    buffer = buffer.slice(newline + 1);
    if (answer === "silent" || message.id === undefined) continue;
    const result = message.method === "initialize"
      ? { protocolVersion: message.params.protocolVersion, capabilities: { tools: {} }, serverInfo: { name: "fake", version: "0" } }
      : message.params.name === "kontor_realm_get" ? answer : null;
    process.stdout.write(JSON.stringify({ jsonrpc: "2.0", id: message.id, result }) + "\\n");
    if (${leaveAfterAnswer} && message.method === "tools/call") process.exit(0);
  }
});
`);
  await chmod(command, 0o700);
  const starts = async () => (await readFile(log, "utf8").catch(() => "")).split("\n").filter(Boolean)
    .map((line) => JSON.parse(line) as { args: string[]; env: string[] });
  return { command, starts };
}

const configFor = (command: string, extra: Record<string, string> = {}) =>
  readConfig({ HOME: "/home/test", KONTOR_PLUGIN_STATE_ROOT: STATE_ROOT, KONTOR_PLUGIN_MCP_COMMAND: command, ...extra });

const none = { realmId: null, realmLabel: null, httpStatus: null };

describe("kontord status through the observer kontor-mcp", () => {
  it("reads the realm at the observer tier, giving the child only PATH and HOME", async () => {
    process.env.KONTOR_AUTH = "planted";
    process.env.JIRA_API_TOKEN = "planted";
    const fake = await fakeMcp(toolResult({ tool: "kontor_realm_get", status: 200, body: { realm_id: "realm-1", display_label: "Example realm" } }));
    expect(await probeKontorStatus(configFor(fake.command))).toEqual({ status: "ready", realmId: "realm-1", realmLabel: "Example realm", httpStatus: null });
    const starts = await fake.starts();
    expect(starts).toHaveLength(1);
    const start = starts[0]!;
    expect(start.args).toEqual(["--state-root", STATE_ROOT, "--credential-tier", "observer"]);
    expect(start.env).not.toContain("KONTOR_AUTH");
    expect(start.env).not.toContain("JIRA_API_TOKEN");
    expect(start.env).toEqual(expect.arrayContaining(["PATH", "HOME"]));
    await probeKontorStatus(configFor(fake.command, { KONTOR_PLUGIN_SERVE_PROFILE: "observer-narrow" }));
    expect((await fake.starts())[1]?.args).toEqual(["--state-root", STATE_ROOT, "--credential-tier", "observer", "--serve-profile", "observer-narrow"]);
  });

  it("reads an answer written just before kontor-mcp leaves", async () => {
    const fake = await fakeMcp(toolResult({ tool: "kontor_realm_get", status: 200, body: { realm_id: "realm-1" } }), true);
    expect(await probeKontorStatus(configFor(fake.command))).toEqual({ status: "ready", ...none, realmId: "realm-1" });
  });

  it("tells a realm nobody answers for from one that refused the observer read", async () => {
    const down = await fakeMcp(toolResult({ tool: "kontor_realm_get", code: "unavailable", dispatched: false }, true));
    expect(await probeKontorStatus(configFor(down.command))).toEqual({ status: "not-answering", ...none });
    const refused = await fakeMcp(toolResult({ tool: "kontor_realm_get", status: 401, body: { code: "unauthenticated" } }, true));
    expect(await probeKontorStatus(configFor(refused.command))).toEqual({ status: "refused", ...none, httpStatus: 401 });
  });

  it("reports no realm when kontor-mcp leaves before serving, and a missing command as such", async () => {
    const leaves = await fakeMcp("exit");
    expect(await probeKontorStatus(configFor(leaves.command))).toEqual({ status: "no-realm", ...none });
    const directory = await mkdtemp(join(tmpdir(), "kontor-mcp-none-"));
    temporary.push(directory);
    expect(await probeKontorStatus(configFor(join(directory, "kontor-mcp")))).toEqual({ status: "mcp-missing", ...none });
  });

  it("never reads silence or an unexpected answer as absent or healthy", async () => {
    const silent = await fakeMcp("silent");
    expect(await probeKontorStatus(configFor(silent.command), 300)).toEqual({ status: "unknown", ...none });
    const odd = await fakeMcp(toolResult({ tool: "kontor_realm_get", status: 200, body: {} }));
    expect(await probeKontorStatus(configFor(odd.command))).toEqual({ status: "unknown", ...none });
  });

  it("stops a probe in flight when the worker stops", async () => {
    const silent = await fakeMcp("silent");
    const stop = new AbortController();
    const started = Date.now();
    const probe = probeKontorStatus(configFor(silent.command), 10_000, stop.signal);
    setTimeout(() => stop.abort(), 100);
    expect(await probe).toEqual({ status: "unknown", ...none });
    expect(Date.now() - started).toBeLessThan(5_000);
  });

  it("asks nothing when the operator turned the Kontor MCP off", async () => {
    const fake = await fakeMcp("exit");
    expect(await probeKontorStatus(configFor(fake.command, { KONTOR_PLUGIN_MCP: "off" }))).toEqual({ status: "off", ...none });
    expect(await fake.starts()).toEqual([]);
  });

  it("starts at most one probe per window and shares it with concurrent callers", async () => {
    const fake = await fakeMcp(toolResult({ tool: "kontor_realm_get", status: 200, body: { realm_id: "realm-1" } }));
    let clock = 1_000_000;
    const read = kontorStatusReader(() => configFor(fake.command), () => clock);
    const [first, concurrent] = await Promise.all([read(), read()]);
    clock += STATUS_TTL_MS - 1;
    expect(await read()).toBe(first);
    expect(concurrent).toBe(first);
    expect(first).toMatchObject({ status: "ready", realmId: "realm-1", checkedAt: new Date(1_000_000).toISOString() });
    expect(await fake.starts()).toHaveLength(1);
    clock += 1;
    await read();
    expect(await fake.starts()).toHaveLength(2);
  });
});
