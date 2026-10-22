import type { TaskTeamDeclaration } from "../../../paseo-plugin/client/task-teams";

/**
 * SIMULATED team, workflow and risk-tier declarations for the review scenarios
 * that turn them on. No configured source declares these for a task (see the
 * README), so this is a design fixture, not a source format or a Kontor
 * binding: the plugin still places a session under a role only by the
 * session's own role label.
 */
export const SIMULATED_TEAMS_SOURCE = "Simulated team declarations (prototype fixture)";

const TSW_HIGH_STAKES = {
  source: SIMULATED_TEAMS_SOURCE, risk: "High-stakes", workflow: "Scope, build, verify and audit", team: "Task team (high-stakes)",
  roles: ["SA", "SWE", "QA", "AUD"],
  // QA and AUD both start from the build and run in parallel; QA can send work back.
  handoffs: [{ from: "SA", to: "SWE" }, { from: "SWE", to: "QA" }, { from: "SWE", to: "AUD" }, { from: "QA", to: "SWE" }],
} as const satisfies TaskTeamDeclaration;

const TSW_STANDARD = {
  source: SIMULATED_TEAMS_SOURCE, risk: "Standard", workflow: "Build and verify", team: "Task team (standard)",
  roles: ["SWE", "QA"], handoffs: [{ from: "SWE", to: "QA" }],
} as const satisfies TaskTeamDeclaration;

/** By Jira key. Keys left out read as not provided, the production answer. */
export const SIMULATED_TEAMS: Readonly<Record<string, TaskTeamDeclaration>> = {
  // Every role has a session: SA done, SWE running, QA waiting for input beside AUD.
  "ASMA-99103": TSW_HIGH_STAKES,
  // A replaced SWE, a reviewer outside the declared team, and no SA or AUD session.
  "ASMA-99104": TSW_HIGH_STAKES,
  // Partial: a risk tier only, no workflow or team.
  "ASMA-99108": { source: SIMULATED_TEAMS_SOURCE, risk: "Standard", workflow: null, team: null, roles: [], handoffs: [] },
  // Roles without handoffs: nothing is ordered, so nothing is shown in order.
  "ASMA-99107": { source: SIMULATED_TEAMS_SOURCE, risk: "Standard", workflow: "Parallel build and checks", team: "Pair team", roles: ["SWE", "QA"], handoffs: [] },
  // Declared, with no session yet.
  "ASMA-99113": TSW_STANDARD,
};
