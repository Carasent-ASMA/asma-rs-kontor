import { describe, expect, it } from "vitest";
import { buildOrganizationTree } from "../client/session-organization";
import {
  chainLabel,
  chainSummary,
  labelledRole,
  matchesResultSearch,
  roleLanes,
  sessionAncestors,
  sessionProvenance,
  taskSessions,
  teamRuns,
} from "../client/session-results";
import { sessionEntry } from "./session-fixtures";

describe("filtered result provenance", () => {
  it("names workstream, epic and workspace from labels and mappings, never from the title", () => {
    const labelled = sessionEntry("a", { title: "Desk • ASMA-1 in title only", workspaceName: "tsw-1", labels: { workstream: "Kontor", epic: "ASMA-9" } });
    expect(sessionProvenance(labelled, {})).toEqual({ parts: ["Kontor", "ASMA-9", "tsw-1"], desk: false, workspace: "tsw-1" });
    const mapped = sessionProvenance(labelled, { "session:a": { workstream: "Mapped stream", epic: "Mapped epic" } });
    expect(mapped.parts).toEqual(["Mapped stream", "Mapped epic", "tsw-1"]);
  });

  it("says when the workstream or epic is missing instead of leaving a gap", () => {
    expect(sessionProvenance(sessionEntry("bare", { workspaceName: "w" }), {}).parts).toEqual(["Other work", "Unassigned", "w"]);
  });

  it("names a desk and its workspace without an epic", () => {
    const desk = sessionProvenance(sessionEntry("d", { workspaceName: "desk-review", labels: { desk: "PR Review", epic: "ASMA-9" } }), {});
    expect(desk).toEqual({ parts: ["PR Review desk", "desk-review"], desk: true, workspace: "desk-review" });
  });

  it("never passes a working directory off as a workspace", () => {
    const loose = sessionEntry("w", { workspaceId: null, workspaceName: null, cwd: "/work/scratch" });
    expect(sessionProvenance(loose, {}).workspace).toBe("No workspace · /work/scratch");
    expect(sessionProvenance(sessionEntry("x", { workspaceId: null, workspaceName: null, cwd: "" }), {}).workspace).toBe("No workspace");
  });

  it("matches every search token against the search text or the placement labels", () => {
    const entry = sessionEntry("m", { title: "Parser", workspaceName: "tsw", labels: { workstream: "Kontor" } });
    expect(matchesResultSearch(entry, [], {})).toBe(true);
    expect(matchesResultSearch(entry, ["parser", "kontor"], {})).toBe(true);
    expect(matchesResultSearch(entry, ["mapped"], { "session:m": { workstream: "Mapped stream" } })).toBe(true);
    expect(matchesResultSearch(entry, ["parser", "absent"], {})).toBe(false);
  });

  it("finds the groups holding a session from the root down, or null when the tree does not hold it", () => {
    const entries = [sessionEntry("s", { workspaceName: "tsw", labels: { workstream: "Kontor", epic: "ASMA-9" } })];
    const tree = buildOrganizationTree({ entries });
    const path = sessionAncestors(tree, "s")!;
    expect(path.length).toBeGreaterThan(1);
    expect(path[0]).toBe(tree[0]!.key);
    expect(sessionAncestors(tree, "missing")).toBeNull();
  });
});

describe("task to session joins", () => {
  const sessions = [
    sessionEntry("jira", { labels: { "jira.issue": "asma-7" } }),
    sessionEntry("task", { labels: { task: "ASMA-7" } }),
    sessionEntry("ticket", { labels: { "kontor.ticket": "T-7" } }),
    sessionEntry("titled", { title: "SWE • ASMA-7 in the title", labels: { epic: "ASMA-7" } }),
    sessionEntry("prefix", { labels: { "jira.issue": "ASMA-70" } }),
  ];

  it("joins only on an exact Jira key label or Kontor ticket label", () => {
    expect(taskSessions(sessions, { jira: "ASMA-7", kontorId: "T-7" }).map((link) => [link.entry.id, link.via]))
      .toEqual([["jira", "jira.issue"], ["task", "task"], ["ticket", "kontor.ticket"]]);
    expect(taskSessions(sessions, { jira: null, kontorId: "T-7" }).map((link) => link.entry.id)).toEqual(["ticket"]);
    expect(taskSessions(sessions, { jira: null, kontorId: null })).toEqual([]);
  });

  it("reads a role from a role label only", () => {
    expect(labelledRole(sessionEntry("r", { labels: { "kontor.role": "rev_a" } }))).toBe("REV A");
    expect(labelledRole(sessionEntry("r", { labels: { role: "qa" } }))).toBe("QA");
    expect(labelledRole(sessionEntry("r", { title: "SWE • work", labels: {} }))).toBeNull();
  });
});

describe("role lanes", () => {
  const link = (entry: ReturnType<typeof sessionEntry>) => ({ entry, via: "jira.issue" });

  it("keeps parallel sessions of one role apart and puts unlabelled sessions last", () => {
    const lanes = roleLanes([
      link(sessionEntry("free", { labels: {} })),
      link(sessionEntry("qa-1", { status: "running", labels: { role: "QA" } })),
      link(sessionEntry("swe", { status: "running", labels: { role: "SWE" } })),
      link(sessionEntry("qa-2", { status: "idle", labels: { role: "QA" } })),
    ]);
    expect(lanes.map((lane) => [lane.role, lane.current.map((entry) => entry.id)])).toEqual([["SWE", ["swe"]], ["QA", ["qa-1", "qa-2"]], [null, ["free"]]]);
    expect(chainSummary(lanes)).toBe("SWE: Running; QA: Running and Idle; Role not labelled: Idle");
  });

  it("moves a replaced or archived session to earlier and never infers completion", () => {
    const lanes = roleLanes([
      link(sessionEntry("first", { status: "error", labels: { role: "SWE" } })),
      link(sessionEntry("second", { status: "running", labels: { role: "SWE", "takeover.predecessor": "first" } })),
      link(sessionEntry("old", { status: "closed", archivedAt: "2026-10-01T00:00:00Z", labels: { role: "QA" } })),
    ]);
    expect(lanes.map((lane) => [lane.role, lane.current.map((entry) => entry.id), lane.earlier.map((entry) => entry.id)]))
      .toEqual([["SWE", ["second"], ["first"]], ["QA", [], ["old"]]]);
    expect(chainSummary(lanes)).toBe("SWE: Running");
  });

  it("keeps an error cue apart from the runtime status", () => {
    expect(chainLabel(sessionEntry("e", { status: "error" }))).toBe("Error");
    expect(chainLabel(sessionEntry("e", { status: "idle", attentionReason: "error" }))).toBe("Error reported, idle now");
    expect(chainLabel(sessionEntry("e", { status: "closed", archivedAt: "2026-10-01T00:00:00Z" }))).toBe("Archived");
  });

  it("lists distinct Kontor team run labels", () => {
    expect(teamRuns([
      link(sessionEntry("a", { labels: { "kontor.team_run": "run-2" } })),
      link(sessionEntry("b", { labels: { "kontor.team_run": "run-1" } })),
      link(sessionEntry("c", { labels: { "kontor.team_run": "run-2" } })),
      link(sessionEntry("d")),
    ])).toEqual(["run-1", "run-2"]);
  });
});
