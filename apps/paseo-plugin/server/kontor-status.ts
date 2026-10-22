import { spawn } from "node:child_process";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readKontorStatus, type KontorStatus } from "../shared/kontor-status";
import { DEFAULT_CREDENTIAL_TIER, readConfig, type KontorPluginConfig } from "./config";

export const STATUS_TTL_MS = 30_000;
export const STATUS_BUDGET_MS = 5_000;
/** Everything the child may write, frames included; the one realm document is far smaller. */
export const STATUS_OUTPUT_LIMIT = 64 * 1024;

type Observation = KontorStatus["observation"];

const TOOL = "kontor_realm_get";

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === "object" && value !== null && !Array.isArray(value);

function parse(text: string): unknown {
  try {
    return JSON.parse(text);
  } catch {
    return undefined;
  }
}

/**
 * kontor-mcp's tool result: one text block holding either the daemon's envelope ({tool, status, body}, an
 * error exactly when the status is not 2xx) or its own refusal ({tool, code, dispatched: false}). Anything
 * else, another tool's document included, is unknown.
 */
function classify(result: Record<string, unknown>): Observation {
  const { isError = false, content } = result;
  const block: unknown = Array.isArray(content) && content.length === 1 ? content[0] : undefined;
  if (typeof isError !== "boolean" || !isRecord(block) || block.type !== "text" || typeof block.text !== "string") return "unknown";
  const document = parse(block.text);
  if (!isRecord(document) || document.tool !== TOOL) return "unknown";
  const { status, body } = document;
  const answered = typeof status === "number" && Number.isInteger(status);
  const success = answered && status >= 200 && status < 300;
  if (isError) {
    // kontor-mcp reached nothing that answered, so nothing was dispatched.
    if (document.dispatched === false && document.code === "unavailable") return "unavailable";
    return answered && !success ? "refused" : "unknown";
  }
  return success && isRecord(body) && typeof body.realm_id === "string" && body.realm_id !== "" ? "ready" : "unknown";
}

/**
 * Ask the configured `kontor-mcp` for `kontor_realm_get` at the fixed observer tier, over stdio, with the
 * arguments agent.create composes for kontor-observer and no fallback to other arguments.
 *
 * `kontor-mcp` reads its own credential from the state root; the plugin never opens, names or forwards it.
 * The child gets only PATH and HOME, its standard error is never read, and it is killed after one answer,
 * the output limit, the budget or `signal`. Never rejects.
 */
export function observeKontord(config: KontorPluginConfig, budgetMs = STATUS_BUDGET_MS, signal?: AbortSignal): Promise<Observation> {
  if (!config.mcpEnabled) return Promise.resolve("unknown");
  return new Promise((settle) => {
    const args = ["--state-root", config.stateRoot, "--credential-tier", DEFAULT_CREDENTIAL_TIER, ...(config.serveProfile ? ["--serve-profile", config.serveProfile] : [])];
    const child = spawn(config.mcpCommand, args, {
      env: { PATH: process.env.PATH ?? "", HOME: process.env.HOME ?? "" },
      stdio: ["pipe", "pipe", "ignore"],
      signal,
    });
    let done = false;
    // The one request whose response may come next: initialize (1), then kontor_realm_get (2).
    let awaiting = 1;
    let received = 0;
    let output = Buffer.alloc(0);
    const finish = (observation: Observation) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      child.kill("SIGKILL");
      settle(observation);
    };
    const timer = setTimeout(() => finish("unknown"), budgetMs);
    const send = (message: object) => child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", ...message })}\n`);
    // A missing command, credential or realm all end here; none of them proves there is no realm.
    child.on("error", () => finish("unknown"));
    // "close" comes after the child's output is drained, so an answer written before it left is still read.
    child.on("close", () => finish("unknown"));
    // A child that leaves early closes the pipe; its close is what reports it.
    child.stdin.on("error", () => {});
    /** One frame. Its own notifications and requests are skipped; any other frame but the awaited response ends the probe. */
    const take = (message: unknown) => {
      if (isRecord(message) && message.jsonrpc === "2.0" && typeof message.method === "string") return;
      if (!isRecord(message) || message.jsonrpc !== "2.0" || message.id !== awaiting || "error" in message || !isRecord(message.result)) return finish("unknown");
      if (awaiting === 2) return finish(classify(message.result));
      if (typeof message.result.protocolVersion !== "string") return finish("unknown");
      awaiting = 2;
      send({ method: "notifications/initialized" });
      send({ id: 2, method: "tools/call", params: { name: TOOL, arguments: {} } });
    };
    const read = (chunk: Buffer) => {
      // Bytes as the child wrote them, counted before anything is decoded or kept.
      received += chunk.byteLength;
      if (received > STATUS_OUTPUT_LIMIT) return finish("unknown");
      output = Buffer.concat([output, chunk]);
      // A newline byte never occurs inside a UTF-8 character, so each line decodes whole.
      for (let newline = output.indexOf(10); newline >= 0 && !done; newline = output.indexOf(10)) {
        const line = output.subarray(0, newline).toString("utf8");
        output = output.subarray(newline + 1);
        take(parse(line));
      }
    };
    // Nothing the child writes may throw out of this handler: it runs in the shared plugin worker.
    child.stdout.on("data", (chunk: Buffer) => {
      try {
        read(chunk);
      } catch {
        finish("unknown");
      }
    });
    send({ id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "kontor-paseo-plugin", version: "0.1.0" } } });
  });
}

/**
 * At most one probe per 30 seconds for one configuration; callers inside the window, concurrent ones and
 * failures included, share it. A changed command, state root, profile or switch starts a new one.
 */
export function kontorStatusReader(floorEnabled: boolean, config: () => KontorPluginConfig = readConfig, now = Date.now, signal?: AbortSignal): () => Promise<KontorStatus> {
  let cached: { key: string; at: number; read: Promise<KontorStatus> } | null = null;
  return () => {
    const current = config();
    const key = JSON.stringify([current.mcpEnabled, current.mcpCommand, current.stateRoot, current.serveProfile]);
    const at = now();
    if (!cached || cached.key !== key || at - cached.at >= STATUS_TTL_MS) {
      const composition = !floorEnabled ? "floor-off" : current.mcpEnabled ? "composed" : "mcp-off";
      cached = { key, at, read: observeKontord(current, STATUS_BUDGET_MS, signal).then((observation) => ({ observation, composition, checkedAt: new Date(now()).toISOString() })) };
    }
    return cached.read;
  };
}

/** Registered whatever the floor; it only reports it. The disposer kills a probe still in flight. */
export function contributeKontorStatus(server: Pick<PluginServerContext, "handle">, floorEnabled: boolean): () => void {
  const stop = new AbortController();
  server.handle(readKontorStatus, kontorStatusReader(floorEnabled, readConfig, Date.now, stop.signal));
  return () => stop.abort();
}
