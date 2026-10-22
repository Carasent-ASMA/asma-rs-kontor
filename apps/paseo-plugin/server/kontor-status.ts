import { spawn } from "node:child_process";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readKontorStatus, type KontorStatus } from "../shared/kontor-status";
import { DEFAULT_CREDENTIAL_TIER, readConfig, type KontorPluginConfig } from "./config";

export const STATUS_TTL_MS = 30_000;
export const STATUS_BUDGET_MS = 5_000;
/** Everything the child may write, frames included; the one realm document is far smaller. */
export const STATUS_OUTPUT_LIMIT = 64 * 1024;

type Observation = KontorStatus["observation"];

/** The tool result carries kontor-mcp's document in its one text block. */
function classify(result: unknown): Observation {
  const { isError, content } = (result ?? {}) as { isError?: boolean; content?: { text?: string }[] };
  let document: { dispatched?: unknown; code?: unknown; status?: unknown; body?: { realm_id?: unknown } };
  try {
    document = JSON.parse(content?.[0]?.text ?? "");
  } catch {
    return "unknown";
  }
  if (isError) {
    // kontor-mcp dispatched the read and nothing usable answered.
    if (document.dispatched === false && document.code === "unavailable") return "unavailable";
    return typeof document.status === "number" ? "refused" : "unknown";
  }
  return typeof document.body?.realm_id === "string" ? "ready" : "unknown";
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
    let received = 0;
    let output = "";
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
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => {
      received += chunk.length;
      if (received > STATUS_OUTPUT_LIMIT) return finish("unknown");
      output += chunk;
      for (let newline = output.indexOf("\n"); newline >= 0; newline = output.indexOf("\n")) {
        const line = output.slice(0, newline);
        output = output.slice(newline + 1);
        let message: { id?: unknown; result?: unknown; error?: unknown };
        try {
          message = JSON.parse(line);
        } catch {
          continue;
        }
        if (message.id === 1) {
          send({ method: "notifications/initialized" });
          send({ id: 2, method: "tools/call", params: { name: "kontor_realm_get", arguments: {} } });
        } else if (message.id === 2) {
          finish(message.error ? "unknown" : classify(message.result));
        }
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
