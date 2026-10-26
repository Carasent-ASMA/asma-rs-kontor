import type { PaseoApi } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readNavigatorCapabilities, UNSUPPORTED, type NavigatorCapabilities } from "../shared/session-navigator";
import { TAKEOVER_LIMITS, type TakeoverClient } from "./session-lifecycle";

const WAKE_SCHEDULE_API_GAP =
  "The host SDK does not expose schedules.forAgent; wake continuity stays unknown and executable takeover remains gated until the host adds that bounded read.";

const TAKEOVER_ROUTE_LIMITS = [
  "One operator fleet profile may be chosen per review when the seat has no recorded chain.",
  "Arbitrary provider/model override for one takeover is not installed on this host.",
];

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
  const takeoverLimits = paseoWakeScheduleReader(paseo) ? TAKEOVER_LIMITS : [...TAKEOVER_LIMITS, WAKE_SCHEDULE_API_GAP];
  return {
    usageTotals: UNSUPPORTED.usageTotals,
    summaries: UNSUPPORTED.summaries,
    draftAttach: UNSUPPORTED.draftAttach,
    takeover: { supported: true, limits: takeoverLimits },
    takeoverBulk: UNSUPPORTED.takeoverBulk,
    takeoverRoute: { supported: true, limits: TAKEOVER_ROUTE_LIMITS },
  };
}

export function contributeSessionNavigator(server: PluginServerContext) {
  server.handle(readNavigatorCapabilities, (_input, { paseo }) => readInstalledNavigatorCapabilities(paseo));
}
