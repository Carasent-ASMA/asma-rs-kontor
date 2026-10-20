import { describe, expect, it } from "vitest";
import { taskSessions } from "../client/session-results";
import { taskTeamView, teamHeadline, teamSteps, type TaskTeamDeclaration } from "../client/task-teams";
import { sessionEntry } from "./session-fixtures";

const declaration = (roles: string[], handoffs: [string, string][] = []): TaskTeamDeclaration => ({
  source: "Fixture", risk: "High-stakes", workflow: "Delivery", team: "Team", roles, handoffs: handoffs.map(([from, to]) => ({ from, to })),
});

describe("declared team steps", () => {
  it("puts a role after every role that hands off to it, with roles that do not hand off to each other in one parallel step", () => {
    expect(teamSteps(declaration(["SA", "SWE", "QA", "AUD"], [["SA", "SWE"], ["SWE", "QA"], ["SWE", "AUD"]]))).toEqual({ steps: [["SA"], ["SWE"], ["QA", "AUD"]], ordered: true });
  });

  it("orders nothing when the source declares no handoff", () => {
    expect(teamSteps(declaration(["SA", "SWE", "QA"]))).toEqual({ steps: [["SA", "SWE", "QA"]], ordered: false });
    expect(teamSteps(declaration([]))).toEqual({ steps: [], ordered: false });
  });

  it("keeps a rework handoff back to an earlier role from moving it, whichever role the source lists first", () => {
    const loop: [string, string][] = [["SA", "SWE"], ["SWE", "QA"], ["QA", "SWE"]];
    expect(teamSteps(declaration(["SA", "SWE", "QA"], loop)).steps).toEqual([["SA"], ["SWE"], ["QA"]]);
    expect(teamSteps(declaration(["QA", "SWE", "SA"], loop)).steps).toEqual([["SA"], ["SWE"], ["QA"]]);
  });

  it("places a role by its longest path, so a shortcut handoff does not pull it forward", () => {
    const path: [string, string][] = [["SA", "SWE"], ["SWE", "QA"], ["QA", "AUD"]];
    for (const handoffs of [[["SA", "AUD"], ...path], [...path, ["SA", "AUD"]]] as [string, string][][]) {
      expect(teamSteps(declaration(["SA", "SWE", "QA", "AUD"], handoffs)).steps).toEqual([["SA"], ["SWE"], ["QA"], ["AUD"]]);
    }
  });

  it("compares role codes as role labels do and ignores handoffs to undeclared roles or to the same role", () => {
    expect(teamSteps(declaration(["sa", "rev-a", "SWE"], [["SA", "REV_A"], ["SA", "OPS"], ["SWE", "swe"]]))).toEqual({ steps: [["SA", "SWE"], ["REV A"]], ordered: true });
    expect(teamSteps(declaration(["SA", "SWE"], [["SA", "OPS"]]))).toEqual({ steps: [["SA", "SWE"]], ordered: false });
  });
});

describe("declared team view", () => {
  const task = { jira: "ASMA-1", kontorId: null, label: "ASMA-1" };
  const links = taskSessions([
    sessionEntry("sa", { labels: { task: "ASMA-1", role: "sa" } }),
    sessionEntry("swe-1", { labels: { task: "ASMA-1", role: "swe" } }),
    sessionEntry("swe-2", { labels: { task: "ASMA-1", "kontor.role": "SWE", "takeover.predecessor": "swe-1" } }),
    sessionEntry("rev", { labels: { task: "ASMA-1", role: "rev_a" } }),
    sessionEntry("loose", { labels: { task: "ASMA-1" } }),
  ], task);

  it("puts each linked session under the role its label names, counts replaced ones as earlier, and keeps the rest outside the team", () => {
    const view = taskTeamView(declaration(["SA", "SWE", "QA", "AUD"], [["SA", "SWE"], ["SWE", "QA"], ["SWE", "AUD"]]), links);
    expect(view.ordered).toBe(true);
    expect(view.steps.map((step) => step.map(({ role, current, earlier }) => [role, current.map((entry) => entry.id), earlier]))).toEqual([
      [["SA", ["sa"], 0]], [["SWE", ["swe-2"], 1]], [["QA", [], 0], ["AUD", [], 0]],
    ]);
    expect(view.outside.map((lane) => [lane.role, lane.current.map((entry) => entry.id)])).toEqual([["REV A", ["rev"]], [null, ["loose"]]]);
  });

  it("leaves every linked session outside a team that declares no roles", () => {
    const view = taskTeamView(declaration([]), links);
    expect(view.steps).toEqual([]);
    expect(view.outside.flatMap((lane) => lane.current.map((entry) => entry.id))).toEqual(["sa", "swe-2", "rev", "loose"]);
  });

  it("names each missing part of the headline instead of leaving a gap", () => {
    expect(teamHeadline(declaration(["SA"]))).toBe("High-stakes · Delivery");
    expect(teamHeadline({ ...declaration(["SA"]), risk: null })).toBe("Risk tier not declared · Delivery");
    expect(teamHeadline({ ...declaration(["SA"]), workflow: null })).toBe("High-stakes · workflow not declared");
  });
});
