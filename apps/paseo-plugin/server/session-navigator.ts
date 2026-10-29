import { CODEX_USAGE_LIMITS } from "./codex-usage";
import type { PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { TAKEOVER_LIMITS } from "../shared/session-lifecycle";
import { readNavigatorCapabilities, type NavigatorCapabilities } from "../shared/session-navigator";
import { summaryOnlyNavigatorCapabilities } from "./session-summaries";
import type { TakeoverClient } from "./session-lifecycle";

/** Optional bounded wake read when the installed SDK exposes it; otherwise absent. */
export function paseoWakeScheduleReader(paseo: PaseoApi): TakeoverClient["schedules"] {
  const host = paseo as { schedules?: { forAgent?(agentId: string): Promise<{ entries: unknown[] }> } };
  if (typeof host.schedules?.forAgent !== "function") return undefined;
  return { forAgent: (agentId: string) => host.schedules!.forAgent!(agentId) };
}

export function asTakeoverClient(paseo: PaseoApi): TakeoverClient {
  return { agents: paseo.agents, workspaces: paseo.workspaces, schedules: paseoWakeScheduleReader(paseo) };
}

const GOVERNED_CAPABILITY_LIMIT = "A governed seat is handed to its existing authority writer; until that writer accepts the handoff summary before its successor's task effects, the plugin reports the exact missing governed input and retires nothing.";
/** Answers what this installed plugin can drive on the host; unsupported owners stay explicit. */
export function readInstalledNavigatorCapabilities(paseo: PaseoApi): NavigatorCapabilities {
  const installed = summaryOnlyNavigatorCapabilities(paseo);
  return {
    ...installed,
    usageTotals: { supported: true, limits: CODEX_USAGE_LIMITS },
    takeover: installed.summaries.supported ? { supported: true, limits: [...TAKEOVER_LIMITS, GOVERNED_CAPABILITY_LIMIT] }
      : installed.summaries,
    takeoverBulk: installed.summaries.supported ? { supported: true, limits: ["Each selected seat keeps its own review, confirmation, receipt and successor; the seats run one at a time."] }
      : installed.summaries,
    takeoverRoute: installed.summaries.supported ? { supported: true, limits: ["A picked model is this one takeover's replacement. It is validated against the host's live provider list; the session's current permission mode is kept, and no read-only or other mode is invented.", "Auto follows the session's existing configured chain."] }
      : installed.summaries,
  };
}

export function contributeSessionNavigator(server: PluginServerContext) {
  server.handle(readNavigatorCapabilities, (_input, { paseo }) => readInstalledNavigatorCapabilities(paseo));
}
