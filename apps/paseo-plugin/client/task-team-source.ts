import type { ReadSourceSummary } from "../shared/read-sources";
import type { TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import type { TaskTeamReader } from "./task-teams";

const KIND_COVERAGE: Readonly<Record<ReadSourceSummary["kind"], string>> = {
  plan: "plans declare tasks and dependencies",
  jira: "Jira history records status",
  decisions: "decision records hold decisions",
  delivery: "delivery records hold pull requests, checks and releases",
  teams: "Teams configuration declares team templates, model bindings and roster seats",
};

/**
 * Board's read of a task's team, workflow and risk tier. No configured source
 * format assigns them to a task: the plan export carries no such field, Teams
 * configuration is not per task, and Jira history holds status only. So every
 * task reads "not provided", with the configured sources. Labels are not
 * read for this: an unenforced label is not a declaration.
 */
export function taskTeamReader(sources: readonly ReadSourceSummary[], teams: Readonly<Record<string, TeamsSourceSnapshot>>): TaskTeamReader {
  const kinds = [...new Set(sources.map((source) => source.kind))];
  const templates = [...new Set(Object.values(teams).flatMap((snapshot) => snapshot.data?.kind === "templates" ? snapshot.data.templates.map((template) => template.name) : []))];
  const parts = kinds.map((kind) => kind === "teams" && templates.length
    ? `Teams configuration declares team templates (${templates.join("; ")}), model bindings and roster seats, none of them for a task`
    : KIND_COVERAGE[kind]);
  const coverage = sources.length
    ? `No configured source assigns a team, workflow or risk tier to a task (${sources.length} configured source${sources.length === 1 ? "" : "s"}). ${parts.join("; ").replace(/^./, (first) => first.toLocaleUpperCase())}.`
    : "No source is configured, so no team, workflow or risk tier is declared for any task.";
  return () => ({ state: "not-provided", coverage });
}
