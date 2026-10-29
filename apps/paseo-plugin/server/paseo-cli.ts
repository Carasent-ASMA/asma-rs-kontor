import { execFile } from "node:child_process";
import type { KontorPluginConfig } from "./config";

/** The result of this server's one bounded public-CLI stop attempt; the source readback is authoritative. */
export type PaseoStopResult = { ok: true } | { ok: false; unavailable: boolean; message: string };
/** A source-linked wake read: only a qualified no-wake proof is `clear`; every other outcome names its gap. */
export type PaseoWakeRead = { wakes: "clear" | "unknown"; gap?: string };
export type PaseoPublicCli = {
  /** Stops one exact agent ID; never a prefix, path or caller-supplied argv. */
  stop(agentId: string): Promise<PaseoStopResult>;
  /** Reads whether this exact source has a scheduled wake, without copying schedules into a record. */
  wakeState(agentId: string): Promise<PaseoWakeRead>;
};

const STOP_BUDGET_MS = 15_000;
const READ_BUDGET_MS = 10_000;
const OUTPUT_LIMIT = 1_000_000;
const SCHEDULE_LIMIT = 200;
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

function listRows(output: string): Record<string, unknown>[] | null {
  let parsed: unknown;
  try { parsed = JSON.parse(output); } catch { return null; }
  return Array.isArray(parsed) && parsed.every((row) => row !== null && typeof row === "object" && !Array.isArray(row)) ? parsed as Record<string, unknown>[] : null;
}

/** The public list displays only a target kind and a prefix; existing-target kinds are inspected for an exact ID. */
function targetKind(row: Record<string, unknown>): "new-agent" | "existing" | "unknown" {
  const target = row.target;
  if (typeof target === "string") return target.startsWith("new-agent:") ? "new-agent" : /^(?:agent|self):/.test(target) ? "existing" : "unknown";
  if (target && typeof target === "object" && !Array.isArray(target)) {
    const type = (target as Record<string, unknown>).type;
    if (type === "new-agent") return "new-agent";
    if (type === "agent" || type === "self") return "existing";
  }
  return "unknown";
}

function inspectedTarget(output: string): { type: string; agentId: string } | null {
  let parsed: unknown;
  try { parsed = JSON.parse(output); } catch { return null; }
  const target = parsed && typeof parsed === "object" && !Array.isArray(parsed) ? (parsed as Record<string, unknown>).target : null;
  if (!target || typeof target !== "object" || Array.isArray(target)) return null;
  const value = target as Record<string, unknown>;
  return (value.type === "agent" || value.type === "self") && typeof value.agentId === "string" ? { type: value.type, agentId: value.agentId } : null;
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
    async wakeState(agentId) {
      const id = exactId(agentId);
      if (!id) return { wakes: "unknown", gap: "The source ID is not a single exact agent ID, so scheduled wakes were not read." };
      let rows: Record<string, unknown>[] | null;
      try { rows = listRows(await run(config.paseoCommand, ["schedule", "ls", "--json", ...daemonArgs(config)])); }
      catch (error) { return { wakes: "unknown", gap: `The public Paseo CLI schedule read failed: ${errorText(error)}.` }; }
      if (!rows) return { wakes: "unknown", gap: "The public Paseo CLI schedule list was not a JSON array, so no wake coverage is proven." };
      const kinds = rows.map(targetKind);
      if (kinds.some((kind) => kind === "unknown")) return { wakes: "unknown", gap: "The public Paseo CLI schedule list contains an unrecognized target, so no wake coverage is proven." };
      // The installed CLI filters this list to new-agent schedules. Seeing an existing-target row first proves the
      // list can expose heartbeats and existing-agent schedules; otherwise absence of a source-linked wake is unproven.
      if (!kinds.includes("existing")) return { wakes: "unknown", gap: "The installed public CLI schedule list returns only new-agent schedules; it does not list heartbeat or existing-agent schedules, so no source-linked wake can be proven absent." };
      if (rows.length > SCHEDULE_LIMIT) return { wakes: "unknown", gap: "The public Paseo CLI schedule list exceeds the bounded inspection limit, so no wake coverage is proven." };
      for (const [index, kind] of kinds.entries()) {
        if (kind !== "existing") continue;
        const scheduleId = rows[index]!.id;
        if (typeof scheduleId !== "string" || !exactId(scheduleId)) return { wakes: "unknown", gap: "The public Paseo CLI schedule list has no exact schedule ID, so no wake coverage is proven." };
        let target: { type: string; agentId: string } | null;
        try { target = inspectedTarget(await run(config.paseoCommand, ["schedule", "inspect", scheduleId, "--json", ...daemonArgs(config)])); }
        catch (error) { return { wakes: "unknown", gap: `The public Paseo CLI schedule inspect failed: ${errorText(error)}.` }; }
        if (!target) return { wakes: "unknown", gap: "The public Paseo CLI schedule inspect returned no exact existing-agent target, so no wake coverage is proven." };
        if (target.agentId === id) return { wakes: "unknown", gap: "The source has a source-linked scheduled wake; no schedule preservation operation is qualified, so retirement is blocked." };
      }
      return { wakes: "clear" };
    },
  };
}
