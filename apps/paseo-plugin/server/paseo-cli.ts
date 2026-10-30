import { execFile } from "node:child_process";
import type { KontorPluginConfig } from "./config";

/** The result of this server's one bounded public-CLI stop attempt; the source readback is authoritative. */
export type PaseoStopResult = { ok: true } | { ok: false; unavailable: boolean; message: string };
export type PaseoPublicCli = {
  /** Stops one exact agent ID; never a prefix, path or caller-supplied argv. */
  stop(agentId: string): Promise<PaseoStopResult>;
  /** Archived display metadata only; retirement must already be verified. */
  rename?(agentId: string, title: string): Promise<PaseoStopResult>;
};

const STOP_BUDGET_MS = 15_000;
const OUTPUT_LIMIT = 1_000_000;
/** An ID, never a prefix or a path. Paseo IDs are UUIDs; this also accepts older native ID forms safely. */
const EXACT_ID = /^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$/;
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

type Run = (command: string, args: string[]) => Promise<string>;
const runPublicCli: Run = (command, args) => new Promise((resolve, reject) => {
  execFile(command, args, {
    env: { PATH: process.env.PATH ?? "", HOME: process.env.HOME ?? "" },
    timeout: STOP_BUDGET_MS, killSignal: "SIGKILL", maxBuffer: OUTPUT_LIMIT, encoding: "utf8",
  }, (error, stdout) => error ? reject(error) : resolve(stdout));
});

function daemonArgs(config: Pick<KontorPluginConfig, "paseoHome" | "paseoHost">): string[] {
  return [...(config.paseoHome ? ["--home", config.paseoHome] : []), ...(config.paseoHost ? ["--host", config.paseoHost] : [])];
}

function exactId(agentId: string): string | null {
  return EXACT_ID.test(agentId) && !agentId.startsWith("-") ? agentId : null;
}

/**
 * The installed public CLI is an explicit server-configured executable. It receives one fixed argument vector, the
 * exact source ID, and only the configured daemon home/host. Tests inject `run`; production never shells out.
 */
export function createPaseoPublicCli(
  config: Pick<KontorPluginConfig, "paseoCommand" | "paseoHome" | "paseoHost">,
  run: Run = runPublicCli,
): PaseoPublicCli {
  return {
    async rename(agentId, title) {
      const id = exactId(agentId);
      if (!id || !title.trim() || title.length > 2000 || /[\r\n\0]/.test(title)) return { ok: false, unavailable: false, message: "Invalid exact rename target/title; nothing was sent." };
      try {
        await run(config.paseoCommand, ["agent", "update", id, "--name", title, "--json", ...daemonArgs(config)]);
        return { ok: true };
      } catch { return { ok: false, unavailable: false, message: "Archived title update is unconfirmed; rename_pending." }; }
    },
    async stop(agentId) {
      const id = exactId(agentId);
      if (!id) return { ok: false, unavailable: false, message: "The source ID is not a single exact agent ID, so the public CLI was not run." };
      try {
        await run(config.paseoCommand, ["agent", "stop", id, "--json", ...daemonArgs(config)]);
        return { ok: true };
      } catch (error) {
        const unavailable = !!error && typeof error === "object" && "code" in error && error.code === "ENOENT";
        return { ok: false, unavailable, message: unavailable
          ? `The configured public Paseo CLI ${config.paseoCommand} could not be found.`
          : `The public Paseo CLI stop failed: ${errorText(error)}.` };
      }
    },
  };
}
