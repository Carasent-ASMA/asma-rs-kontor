import type { ReadSourceSummary } from "../../../paseo-plugin/shared/read-sources";
import type { TeamsSourceSnapshot } from "../../../paseo-plugin/shared/teams-source-snapshot";
import type { TaskTeamReader } from "../../../paseo-plugin/client/task-teams";
import { taskTeamReader as configuredTaskTeamReader } from "../../../paseo-plugin/client/task-team-source";
import { SIMULATED_TEAMS } from "../fixtures/teams";
import { scenario } from "./runtime";

/**
 * Stand-in for the plugin's `./task-team-source`. It answers with the production
 * reader, except in scenarios that turn on simulated declarations, where the
 * fixture keys answer with a simulated declaration instead.
 */
export function taskTeamReader(sources: readonly ReadSourceSummary[], teams: Readonly<Record<string, TeamsSourceSnapshot>>): TaskTeamReader {
  const configured = configuredTaskTeamReader(sources, teams);
  if (!scenario.simulatedTeams) return configured;
  return (task) => {
    const declaration = task.jira ? SIMULATED_TEAMS[task.jira] : undefined;
    return declaration ? { state: "declared", declaration } : configured(task);
  };
}
