import { baseSessions, deskSessions, timeSessions, type FixtureSession } from "./sessions";
import { modelUsageSessions } from "./model-usage";
import { lifecycleSessions } from "./lifecycle";

/** One combined fictional inventory, using production plugin logic without copying it. */
export function integrationSessions(): FixtureSession[] {
  const usage = modelUsageSessions();
  const times = timeSessions().map(value => value.agent.id === "agt_time_recent" ? {
    ...value, agent: { ...value.agent, provider: usage[0]!.agent.provider, model: usage[0]!.agent.model,
      runtimeInfo: usage[0]!.agent.runtimeInfo, lastUsage: usage[0]!.agent.lastUsage },
  } : value);
  return [...times, ...usage, ...deskSessions(), ...lifecycleSessions(), ...baseSessions()];
}

const SYNTHETIC_SOURCE = "SYNTHETIC totals for review; not read from any host";
/** SYNTHETIC whole-session token totals for three integration sessions, one with its output side not reported. */
export const SYNTHETIC_TOTALS = {
  agt_model_report_0: { input: 124_800, output: 9_600, source: SYNTHETIC_SOURCE },
  agt_swe_99103: { input: 1_200_000, output: 84_600, source: SYNTHETIC_SOURCE },
  agt_qa_99103: { input: 31_200, output: null, source: SYNTHETIC_SOURCE },
} as const;
