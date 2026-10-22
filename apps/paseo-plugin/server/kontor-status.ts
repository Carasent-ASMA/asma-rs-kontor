import { spawn } from "node:child_process";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readKontorStatus, type KontorStatus } from "../shared/kontor-status";
import { DEFAULT_CREDENTIAL_TIER, readConfig, type KontorPluginConfig } from "./config";

export const STATUS_TTL_MS = 30_000;
export const STATUS_BUDGET_MS = 5_000;
const MAX_OUTPUT = 1 << 20;

type Outcome = Omit<KontorStatus, "checkedAt">;

const outcome = (status: KontorStatus["status"], extra: Partial<Outcome> = {}): Outcome =>
  ({ status, realmId: null, realmLabel: null, httpStatus: null, ...extra });

/** The tool result carries kontor-mcp's document in its one text block. */
function classify(result: unknown): Outcome {
  const { isError, content } = (result ?? {}) as { isError?: boolean; content?: { text?: string }[] };
  let document: { dispatched?: unknown; code?: unknown; status?: unknown; body?: { realm_id?: unknown; display_label?: unknown } };
  try {
    document = JSON.parse(content?.[0]?.text ?? "");
  } catch {
    return outcome("unknown");
  }
  if (isError) {
    // Nothing answered on the realm's endpoint; kontor-mcp dispatched nothing.
    if (document.dispatched === false && document.code === "unavailable") return outcome("not-answering");
    return typeof document.status === "number" ? outcome("refused", { httpStatus: document.status }) : outcome("unknown");
  }
  const body = document.body;
  return typeof body?.realm_id === "string"
    ? outcome("ready", { realmId: body.realm_id, realmLabel: typeof body.display_label === "string" ? body.display_label : null })
    : outcome("unknown");
}

/**
 * Ask the configured `kontor-mcp` for `kontor_realm_get` at the fixed observer tier, over stdio.
 *
 * Only an authenticated answer to that read is `ready`; a completed handshake alone is not readiness.
 * `kontor-mcp` reads its own credential from the state root; the plugin never opens, names or forwards it.
 * The child gets only PATH and HOME, its standard error is never read, and it is stopped after one answer
 * or the budget, or when `signal` aborts. Never rejects.
 */
export function probeKontorStatus(config: KontorPluginConfig, budgetMs = STATUS_BUDGET_MS, signal?: AbortSignal): Promise<Outcome> {
  if (!config.mcpEnabled) return Promise.resolve(outcome("off"));
  return new Promise((settle) => {
    // The arguments agent.create composes for kontor-observer: the configured state root, the fixed tier and
    // the configured serve profile, if any.
    const args = ["--state-root", config.stateRoot, "--credential-tier", DEFAULT_CREDENTIAL_TIER, ...(config.serveProfile ? ["--serve-profile", config.serveProfile] : [])];
    const child = spawn(config.mcpCommand, args, {
      env: { PATH: process.env.PATH ?? "", HOME: process.env.HOME ?? "" },
      stdio: ["pipe", "pipe", "ignore"],
      signal,
    });
    let done = false;
    let served = false;
    let output = "";
    const finish = (result: Outcome) => {
      if (done) return;
      done = true;
      clearTimeout(timer);
      child.kill();
      settle(result);
    };
    const timer = setTimeout(() => finish(outcome("unknown")), budgetMs);
    const send = (message: object) => child.stdin.write(`${JSON.stringify({ jsonrpc: "2.0", ...message })}\n`);
    child.on("error", (error: NodeJS.ErrnoException) => finish(outcome(error.code === "ENOENT" ? "mcp-missing" : "unknown")));
    // kontor-mcp resolves the realm before it serves, so leaving before the handshake means no realm here.
    // "close" comes after its output is drained, so an answer it wrote before leaving is still read.
    child.on("close", () => finish(outcome(served ? "unknown" : "no-realm")));
    // A child that leaves early closes the pipe; its exit is what reports it.
    child.stdin.on("error", () => {});
    child.stdout.setEncoding("utf8").on("data", (chunk: string) => {
      output += chunk;
      if (output.length > MAX_OUTPUT) return finish(outcome("unknown"));
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
          served = true;
          send({ method: "notifications/initialized" });
          send({ id: 2, method: "tools/call", params: { name: "kontor_realm_get", arguments: {} } });
        } else if (message.id === 2) {
          finish(message.error ? outcome("unknown") : classify(message.result));
        }
      }
    });
    send({ id: 1, method: "initialize", params: { protocolVersion: "2025-03-26", capabilities: {}, clientInfo: { name: "kontor-paseo-plugin", version: "0.1.0" } } });
  });
}

/** At most one probe per 30 seconds; callers inside the window, concurrent ones included, share it. */
export function kontorStatusReader(config: () => KontorPluginConfig = readConfig, now = Date.now, signal?: AbortSignal): () => Promise<KontorStatus> {
  let cached: { at: number; read: Promise<KontorStatus> } | null = null;
  return () => {
    const at = now();
    if (!cached || at - cached.at >= STATUS_TTL_MS) {
      cached = { at, read: probeKontorStatus(config(), STATUS_BUDGET_MS, signal).then((result) => ({ ...result, checkedAt: new Date(now()).toISOString() })) };
    }
    return cached.read;
  };
}

/** Independent of the floor. The returned disposer stops a probe still in flight when the worker stops. */
export function contributeKontorStatus(server: Pick<PluginServerContext, "handle">): () => void {
  const stop = new AbortController();
  server.handle(readKontorStatus, kontorStatusReader(readConfig, Date.now, stop.signal));
  return () => stop.abort();
}
