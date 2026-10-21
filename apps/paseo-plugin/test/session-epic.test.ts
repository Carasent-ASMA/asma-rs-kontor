import { describe, expect, it } from "vitest";
import { confirmedEpic, jiraCapture, portfolioRows, withJiraCapture } from "../client/page-data";
import type { DirectoryEntry } from "../client/session-directory";
import { buildOrganizationTree, epicDetail, inEpicScope, organizationScope, sessionEpic, type OrganizationGroup } from "../client/session-organization";
import { sessionProvenance, taskSessions } from "../client/session-results";
import { projectEpicKeys } from "../client/source-scope";
import type { JiraHistory, SourceRead } from "../shared/read-sources";
import { sessionEntry } from "./session-fixtures";

const issue = (key: string) => ({ key, status: "In Progress", cat: "indeterminate" as const });
function capture(generated: string, epics: Record<string, string[]>): JiraHistory {
  return { schema: "consumption/data@2", generated, provenance: { path: "/configured/capture.json", sha256: "b".repeat(64), capturedEpics: Object.keys(epics).length },
    selection: Object.keys(epics), epics: Object.entries(epics).map(([key, children]) => ({ ...issue(key), children: children.map(issue) })) };
}
function read(id: string, history: JiraHistory | null, status: SourceRead["status"] = "ready"): SourceRead {
  return { source: { id, title: id, kind: "jira", path: `/configured/${id}.json`, projectId: null, epic: null, workstream: null, revisionKind: "working copy" },
    status, revision: "c".repeat(64), readAt: "2026-10-21T09:00:00Z", modifiedAt: null, content: null, graph: null, records: null, diagnostics: [], jiraHistory: history };
}
const OBSERVED = "2026-10-20T16:03:00+00:00";
const JIRA = jiraCapture([read("jira", capture(OBSERVED, { "ASMA-8108": ["ASMA-8343", "ASMA-8345"], "ASMA-8371": ["ASMA-8543"], "ASMA-8190": ["ASMA-8203"] }))]);
const linked = (id: string, labels: Record<string, string>, extra: Parameters<typeof sessionEntry>[1] = {}): DirectoryEntry => ({ ...sessionEntry(id, { ...extra, labels }), jira: JIRA });
const epicGroups = (groups: readonly OrganizationGroup[]): OrganizationGroup[] =>
  groups.flatMap((group) => [...(group.kind === "epic" ? [group] : []), ...epicGroups(group.groups)]);
const members = (group: OrganizationGroup): string[] => [...group.sessions.map(({ id }) => id), ...group.groups.flatMap(members)];

describe("session epic resolution", () => {
  it("files ASMA-8343 under ASMA-8108 from the Jira capture on every surface", () => {
    for (const label of ["asma.task", "task", "jira.issue"]) {
      const entry = linked(`swe-${label}`, { [label]: "ASMA-8343" }, { projectKey: "prj", workspaceName: "w" });
      expect(sessionEpic(entry, {})).toEqual({ epic: "ASMA-8108", note: `from the Jira capture of ${OBSERVED} for ASMA-8343` });
      expect(organizationScope(entry, {}).epic).toBe("ASMA-8108");
      expect(inEpicScope(entry, {}, "ASMA-8108")).toBe(true);
      expect(confirmedEpic(entry)).toBe("ASMA-8108");
      expect(projectEpicKeys("prj", [entry])).toEqual(["ASMA-8108"]);
      expect(sessionProvenance(entry, {}).parts).toEqual(["Other work", "ASMA-8108", "w"]);
      const group = epicGroups(buildOrganizationTree({ entries: [entry] }))[0]!;
      expect([group.epic, members(group)]).toEqual(["ASMA-8108", [entry.id]]);
      expect(portfolioRows([entry], [], false).map((row) => [row.epicKey, row.sessions.total])).toEqual([["ASMA-8108", 1]]);
    }
    // The epic's own key and an asma.epic label settle it too; the label needs no capture.
    expect(sessionEpic(linked("epic-task", { task: "ASMA-8108" }), {}).epic).toBe("ASMA-8108");
    expect(sessionEpic(sessionEntry("asma-epic", { labels: { "asma.epic": "ASMA-8278", "asma.task": "ASMA-8300" } }), {})).toEqual({ epic: "ASMA-8278", note: "from the asma.epic label" });
    expect(sessionEpic(linked("both", { "asma.epic": "ASMA-8108", "asma.task": "ASMA-8343" }), {}).note).toBe(`from the asma.epic label and the Jira capture of ${OBSERVED} for ASMA-8108 and the Jira capture of ${OBSERVED} for ASMA-8343`);
    expect(taskSessions([linked("board", { "asma.task": "asma-8343 " })], { jira: "ASMA-8343", kontorId: null }).map(({ via }) => via)).toEqual(["asma.task"]);
  });

  it("says Epic unknown, with the reason, when nothing settles the epic", () => {
    const cases: [DirectoryEntry, string][] = [
      // Truly unparented in Jira, a subtask, or another project: the capture cannot show any of them has no epic.
      [linked("unparented", { task: "ASMA-7182" }), `ASMA-7182 is not in the Jira capture of ${OBSERVED}`],
      [linked("subtask", { "jira.issue": "ASMA-9001" }), `ASMA-9001 is not in the Jira capture of ${OBSERVED}`],
      [linked("other-project", { task: "EX-100" }), `EX-100 is not in the Jira capture of ${OBSERVED}`],
      [linked("bounds", { task: "ASMA-83430" }), `ASMA-83430 is not in the Jira capture of ${OBSERVED}`],
      [sessionEntry("no-capture", { labels: { "asma.task": "ASMA-8343" } }), "no Jira capture read for ASMA-8343"],
      [linked("bare", {}, { title: "SWE • ASMA-8343", projectName: "ESW • ASMA-8108", workspaceName: "ASMA-8343 work" }), "no epic or task label"],
      [linked("unrecognised", { "work.epic": "ASMA-8108", scope: "ASMA-8343" }), "no epic or task label"],
    ];
    for (const [entry, note] of cases) {
      expect(sessionEpic(entry, {})).toEqual({ epic: null, note });
      expect(epicDetail(sessionEpic(entry, {}))).toBe(`Unknown, ${note}`);
      expect(confirmedEpic(entry)).toBeNull();
      expect(epicGroups(buildOrganizationTree({ entries: [entry] })).map((group) => group.label)).toEqual(["Epic unknown"]);
    }
    // A capture that is partial or unavailable is not read at all.
    for (const status of ["partial", "unavailable"] as const) expect(jiraCapture([read("jira", capture(OBSERVED, { "ASMA-8108": ["ASMA-8343"] }), status)]).epics.size).toBe(0);
    expect(sessionEpic(linked("trimmed", { task: " asma-8343 " }), {}).epic).toBe("ASMA-8108");
  });

  it("keeps the epic unknown on every surface when its sources disagree", () => {
    const labels = linked("labels", { "jira.epic": "ASMA-8399", "origin.epic": "ASMA-8371" });
    expect(sessionEpic(labels, {})).toEqual({ epic: null, note: "sources disagree: the jira.epic label says ASMA-8399; the origin.epic label says ASMA-8371" });
    expect([organizationScope(labels, {}).epic, confirmedEpic(labels)]).toEqual(["Unassigned", null]);
    // A label the capture files under another epic: the label names a task, or the capture is stale.
    const filed = linked("filed", { epic: "ASMA-8543" });
    expect(sessionEpic(filed, {})).toEqual({ epic: null, note: `sources disagree: the epic label says ASMA-8543; the Jira capture of ${OBSERVED} for ASMA-8543 says ASMA-8371` });
    const later = "2026-10-21T08:00:00Z";
    const split = jiraCapture([read("one", capture(OBSERVED, { "ASMA-8108": ["ASMA-8343"] })), read("two", capture(later, { "ASMA-8501": ["ASMA-8343"] }))]);
    expect(split.epics.get("ASMA-8343")).toBeNull();
    expect(sessionEpic({ ...sessionEntry("split", { labels: { task: "ASMA-8343" } }), jira: split }, {}))
      .toEqual({ epic: null, note: `sources disagree: the Jira captures of ${OBSERVED} / ${later} disagree on ASMA-8343` });
    // A display mapping is the operator's own placement and wins.
    expect(sessionEpic(filed, { "session:filed": { epic: "ASMA-8371" } })).toEqual({ epic: "ASMA-8371", note: "from a display mapping" });
  });

  it("shows a stale capture that agrees with the label with its observation time, since it cannot see a later move", () => {
    // Jira moved ASMA-8203 to another epic after this capture; both sources still agree, so the date is the only cue.
    const stale = linked("stale", { "jira.epic": "ASMA-8190", "asma.task": "ASMA-8203" });
    expect(epicDetail(sessionEpic(stale, {}))).toBe(`ASMA-8190, from the jira.epic label and the Jira capture of ${OBSERVED} for ASMA-8190 and the Jira capture of ${OBSERVED} for ASMA-8203`);
  });

  it("files an epic under itself and keeps a project's sessionless workspaces apart, saying why", () => {
    const roots = jiraCapture([read("jira", capture(OBSERVED, { "ASMA-8101": ["ASMA-8107"], "ASMA-7869": ["ASMA-8015"], "ASMA-8108": ["ASMA-8343"] }))]);
    const at = (id: string, project: string, labels: Record<string, string>): DirectoryEntry =>
      ({ ...sessionEntry(id, { projectKey: `prj_${project}`, projectName: `ESW • ${project}`, workspaceId: `wks_${id}`, workspaceName: `TSW • ${id}`, labels }), jira: roots });
    // Epic as root: the epic's own key, by label or as the named issue; a child task by its parent.
    const root = at("root", "8101", { "jira.epic": "ASMA-8101", "jira.issue": "ASMA-8101" });
    expect(sessionEpic(root, {})).toEqual({ epic: "ASMA-8101", note: `from the jira.epic label and the Jira capture of ${OBSERVED} for ASMA-8101` });
    expect(sessionEpic(at("child", "8101", { "asma.task": "ASMA-8107" }), {}).epic).toBe("ASMA-8101");
    expect(sessionEpic(at("op", "7869", { "jira.issue": "ASMA-7869" }), {}).epic).toBe("ASMA-7869");
    expect(sessionEpic(at("cli", "8108", { "asma.task": "ASMA-8343" }), {}).epic).toBe("ASMA-8108");
    const bare = at("bare", "8108", {});
    const tree = buildOrganizationTree({ entries: [root, at("op", "7869", { "jira.epic": "ASMA-7869" }), bare],
      workspaces: [{ id: "wks_done-a", name: "TSW • done a", projectKey: "prj_8101", projectName: "ESW • 8101" }, { id: "wks_done-b", name: "TSW • done b", projectKey: "prj_8101", projectName: "ESW • 8101" }] });
    expect(epicGroups(tree).map((group) => [group.label, group.epic, members(group), group.detail])).toEqual([
      ["ASMA-7869 · ESW • 7869", "ASMA-7869", ["op"], null],
      ["ASMA-8101 · ESW • 8101", "ASMA-8101", ["root"], null],
      // Workspaces alone cannot say why, so the group does; sessions say it in their details.
      ["Epic unknown · ESW • 8101", null, [], "2 workspaces without sessions"],
      ["Epic unknown · ESW • 8108", null, ["bare"], null],
    ]);
    const workspaces = (group: OrganizationGroup): string[] => [...(group.kind === "workspace" ? [group.label] : []), ...group.groups.flatMap(workspaces)];
    expect(workspaces(epicGroups(tree)[2]!)).toEqual(["TSW • done a", "TSW • done b"]);
  });

  it("attaches one capture per update without changing an unchanged entry's identity", () => {
    expect(jiraCapture([])).toBe(jiraCapture([read("jira", null)]));
    const entries = [sessionEntry("a"), sessionEntry("b")];
    const cache = new WeakMap<DirectoryEntry, DirectoryEntry>();
    const first = withJiraCapture(entries, JIRA, cache);
    expect(first.map((entry) => entry.jira)).toEqual([JIRA, JIRA]);
    expect(entries.map((entry) => entry.jira)).toEqual([undefined, undefined]);
    const next = withJiraCapture([entries[0]!, sessionEntry("c")], JIRA, cache);
    expect(next[0]).toBe(first[0]);
    expect(next[1]).not.toBe(first[1]);
  });
});
