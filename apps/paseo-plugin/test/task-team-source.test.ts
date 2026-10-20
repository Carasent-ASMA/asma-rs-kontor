import { fileURLToPath, URL } from "node:url";
import { describe, expect, it } from "vitest";
import { sourceCatalog } from "../server/read-sources";
import { createTeamsSourceAdapter } from "../server/teams-source-adapter";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import { taskTeamReader } from "../client/task-team-source";

const task = { jira: "ASMA-1", kontorId: "K-1", label: "ASMA-1" };
const catalog = sourceCatalog(JSON.stringify([
  ["authoring", "authoring-fleet.yml"], ["templates", "mvp-team-pack.json"],
].map(([id, filename]) => ({ id, title: id, kind: "teams", path: fileURLToPath(new URL(`./fixtures/teams-source/${filename}`, import.meta.url)) }))));
const summary = (id: string, kind: "plan" | "jira" | "decisions" | "delivery") => ({ ...catalog.sources[0]!, id, title: id, kind });

describe("production task team reader", () => {
  it("declares no team for any task and says what each configured kind of source does hold, naming the configured templates", async () => {
    const adapter = createTeamsSourceAdapter(catalog, teamsSourceBindings(catalog.sources).formats);
    const snapshots = Object.fromEntries(await Promise.all(catalog.sources.map(async (source) => [source.id, await adapter.read({ sourceId: source.id })])));
    const read = taskTeamReader([summary("plan", "plan"), ...catalog.sources, summary("jira", "jira"), summary("decisions", "decisions"), summary("delivery", "delivery")], snapshots)(task);
    expect(read).toEqual({ state: "not-provided", coverage: "No configured source assigns a team, workflow or risk tier to a task (6 configured sources). Plans declare tasks and dependencies; Teams configuration declares team templates (Plan, build and verify; Research spike), model bindings and roster seats, none of them for a task; Jira history records status; decision records hold decisions; delivery records hold pull requests, checks and releases." });
  });

  it("gives every task the same answer, whatever the task", () => {
    const read = taskTeamReader([summary("plan", "plan")], {});
    const answer = { state: "not-provided", coverage: "No configured source assigns a team, workflow or risk tier to a task (1 configured source). Plans declare tasks and dependencies." };
    expect([read(task), read({ jira: "ASMA-2", kontorId: null, label: "ASMA-2" }), read({ jira: null, kontorId: "K-9", label: "K-9" })]).toEqual([answer, answer, answer]);
  });

  it("says so when no source is configured", () => {
    expect(taskTeamReader([], {})(task)).toEqual({ state: "not-provided", coverage: "No source is configured, so no team, workflow or risk tier is declared for any task." });
  });

  it("names Teams configuration generally while its templates are unread", () => {
    expect(taskTeamReader(catalog.sources, {})(task)).toMatchObject({ coverage: "No configured source assigns a team, workflow or risk tier to a task (2 configured sources). Teams configuration declares team templates, model bindings and roster seats." });
  });
});
