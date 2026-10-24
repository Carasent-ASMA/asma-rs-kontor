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
