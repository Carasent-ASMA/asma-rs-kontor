import { CODEX_USAGE_LIMITS } from "./codex-usage";
import type { PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readNavigatorCapabilities, UNSUPPORTED, type NavigatorCapabilities } from "../shared/session-navigator";
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

/** Answers what this installed plugin can drive on the host; unsupported owners stay explicit. */
export function readInstalledNavigatorCapabilities(paseo: PaseoApi): NavigatorCapabilities {
  return {
    ...summaryOnlyNavigatorCapabilities(paseo),
    usageTotals: { supported: true, limits: CODEX_USAGE_LIMITS },
    takeover: UNSUPPORTED.takeover,
    takeoverBulk: UNSUPPORTED.takeoverBulk,
    takeoverRoute: UNSUPPORTED.takeoverRoute,
  };
}

export function contributeSessionNavigator(server: PluginServerContext) {
  server.handle(readNavigatorCapabilities, (_input, { paseo }) => readInstalledNavigatorCapabilities(paseo));
}
