import { describe, expect, it } from "vitest";
import { confirmedEpic, jiraCapture, portfolioRows, withJiraCapture } from "../client/page-data";
import type { DirectoryEntry } from "../client/session-directory";
import { organizationIcon } from "../client/desk-icon";
import { associationLabel, buildOrganizationTree, epicDetail, filterOrganizationTree, inEpicScope, organizationScope, sessionEpic, visibleSessions, type OrganizationGroup } from "../client/session-organization";
import { epicScopeChoices, matchesEpicScope } from "../client/session-scope";
import { relateSplitProjects } from "../client/split-project-rows";
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
      expect(sessionEpic(entry, {})).toEqual({ epic: "ASMA-8108", note: `from the Jira capture of ${OBSERVED} for ASMA-8343`, association: "epic" });
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
    expect(sessionEpic(sessionEntry("asma-epic", { labels: { "asma.epic": "ASMA-8278", "asma.task": "ASMA-8300" } }), {})).toEqual({ epic: "ASMA-8278", note: "from the asma.epic label", association: "epic" });
    expect(sessionEpic(linked("both", { "asma.epic": "ASMA-8108", "asma.task": "ASMA-8343" }), {}).note).toBe(`from the asma.epic label and the Jira capture of ${OBSERVED} for ASMA-8108 and the Jira capture of ${OBSERVED} for ASMA-8343`);
    expect(taskSessions([linked("board", { "asma.task": "asma-8343 " })], { jira: "ASMA-8343", kontorId: null }).map(({ via }) => via)).toEqual(["asma.task"]);
  });

  it("says No epic linked, with the reason, when nothing settles the epic", () => {
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
      expect(sessionEpic(entry, {})).toEqual({ epic: null, note, association: "unlinked" });
      expect(epicDetail(sessionEpic(entry, {}))).toBe(`Unknown, ${note}`);
      expect(confirmedEpic(entry)).toBeNull();
      expect(epicGroups(buildOrganizationTree({ entries: [entry] })).map((group) => group.label)).toEqual(["No epic linked"]);
    }
    // A capture that is partial or unavailable is not read at all.
    for (const status of ["partial", "unavailable"] as const) expect(jiraCapture([read("jira", capture(OBSERVED, { "ASMA-8108": ["ASMA-8343"] }), status)]).epics.size).toBe(0);
    expect(sessionEpic(linked("trimmed", { task: " asma-8343 " }), {}).epic).toBe("ASMA-8108");
  });

  it("keeps the epic unknown on every surface when its sources disagree", () => {
    const labels = linked("labels", { "jira.epic": "ASMA-8399", "origin.epic": "ASMA-8371" });
    expect(sessionEpic(labels, {})).toEqual({ epic: null, note: "sources disagree: the jira.epic label says ASMA-8399; the origin.epic label says ASMA-8371", association: "conflict" });
    expect([organizationScope(labels, {}).epic, confirmedEpic(labels)]).toEqual(["Unassigned", null]);
    // A label the capture files under another epic: the label names a task, or the capture is stale.
    const filed = linked("filed", { epic: "ASMA-8543" });
    expect(sessionEpic(filed, {})).toEqual({ epic: null, note: `sources disagree: the epic label says ASMA-8543; the Jira capture of ${OBSERVED} for ASMA-8543 says ASMA-8371`, association: "conflict" });
    const later = "2026-10-21T08:00:00Z";
    const split = jiraCapture([read("one", capture(OBSERVED, { "ASMA-8108": ["ASMA-8343"] })), read("two", capture(later, { "ASMA-8501": ["ASMA-8343"] }))]);
    expect(split.epics.get("ASMA-8343")).toBeNull();
    expect(sessionEpic({ ...sessionEntry("split", { labels: { task: "ASMA-8343" } }), jira: split }, {}))
      .toEqual({ epic: null, note: `sources disagree: the Jira captures of ${OBSERVED} / ${later} disagree on ASMA-8343`, association: "conflict" });
    // A display mapping is the operator's own placement and wins.
    expect(sessionEpic(filed, { "session:filed": { epic: "ASMA-8371" } })).toEqual({ epic: "ASMA-8371", note: "from a display mapping", association: "epic" });
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
    expect(sessionEpic(root, {})).toEqual({ epic: "ASMA-8101", note: `from the jira.epic label and the Jira capture of ${OBSERVED} for ASMA-8101`, association: "epic" });
    expect(sessionEpic(at("child", "8101", { "asma.task": "ASMA-8107" }), {}).epic).toBe("ASMA-8101");
    expect(sessionEpic(at("op", "7869", { "jira.issue": "ASMA-7869" }), {}).epic).toBe("ASMA-7869");
    expect(sessionEpic(at("cli", "8108", { "asma.task": "ASMA-8343" }), {}).epic).toBe("ASMA-8108");
    const bare = at("bare", "8108", {});
    const tree = buildOrganizationTree({ entries: [root, at("op", "7869", { "jira.epic": "ASMA-7869" }), bare],
      workspaces: [{ id: "wks_done-a", name: "TSW • done a", projectKey: "prj_8101", projectName: "ESW • 8101" }, { id: "wks_done-b", name: "TSW • done b", projectKey: "prj_8101", projectName: "ESW • 8101" }] });
    expect(epicGroups(tree).map((group) => [group.label, group.context, group.epic, members(group), group.detail])).toEqual([
      ["ASMA-7869", "ESW • 7869", "ASMA-7869", ["op"], null],
      ["ASMA-8101", "ESW • 8101", "ASMA-8101", ["root"], null],
      // Workspaces alone cannot say why, so the group does; sessions say it in their details.
      ["No epic linked", "ESW • 8101", null, [], "2 workspaces without sessions"],
      ["No epic linked", "ESW • 8108", null, ["bare"], null],
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

// The reported ASMA-8639 seats: one carries its own epic and task labels, six carry only a parent link. Synthetic
// IDs; the 21 Oct capture predates ASMA-8639, the 24 Oct one has it. Titles, parents and projects never settle an epic.
describe("what a session's own metadata settles", () => {
  const OCT21 = "2026-10-21T08:00:00+00:00", OCT24 = "2026-10-24T16:07:00+00:00";
  const stale = jiraCapture([read("jira", capture(OCT21, { "ASMA-8108": ["ASMA-8343"] }))]);
  const fresh = jiraCapture([read("jira", capture(OCT24, { "ASMA-8639": ["ASMA-8640", "ASMA-8641"] }))]);
  const seat = (id: string, project: string, title: string, labels: Record<string, string> = {}, jira = stale): DirectoryEntry =>
    ({ ...sessionEntry(id, { title, projectKey: `prj_${project}`, projectName: project, workspaceId: `wks_${project}`, workspaceName: `TSW • ${project}`,
      labels: { "paseo.parent-agent-id": "parent-tpm", ...labels } }), jira });
  const roster = (filled: (task: string) => Record<string, string>, jira = stale) => [
    seat("aud-8640", "asma-8640", "AUD • ASMA-8640 • Audit the projection", { role: "AUD", epic: "ASMA-8639", task: "ASMA-8640" }, jira),
    ...["SWE", "QA"].map((role) => seat(`${role}-8640`, "asma-8640", `${role} • ASMA-8640 • Build the projection`, filled("ASMA-8640"), jira)),
    ...["SWE", "QA", "AUD", "SA"].map((role) => seat(`${role}-8641`, "asma-8641", `${role} • ASMA-8641 • Takeover contract`, filled("ASMA-8641"), jira)),
  ];
  const rows = (entries: readonly DirectoryEntry[], byWorkstream = false) => {
    const tree = relateSplitProjects(buildOrganizationTree({ entries, byWorkstream }));
    // The whole inventory stays reachable: no session is hidden to tidy a row.
    expect(visibleSessions(tree).map(({ id }) => id).sort()).toEqual(entries.map(({ id }) => id).sort());
    return tree.map((top) => [top.label, top.detail, epicGroups(top.groups).map((group) => [group.label, group.context ?? null, members(group).length, organizationIcon(group)])]);
  };

  it("leaves parent-only seats unlinked, counts one epic once, and keeps the project off the label", () => {
    const entries = roster(() => ({}));
    for (const entry of entries.slice(1)) expect(sessionEpic(entry, {})).toEqual({ epic: null, note: "no epic or task label", association: "unlinked" });
    expect(rows(entries)).toEqual([["Other work", "1 epic", [
      ["ASMA-8639", "asma-8640", 1, "Flag"], ["No epic linked", "asma-8640", 2, "CircleDashed"], ["No epic linked", "asma-8641", 4, "CircleDashed"]]]]);
    // QA and AUD review checkouts registered as their own projects: the project names a row's context, never its title.
    const reviews = [seat("qa-8645", "asma-8645-qa", "QA • ASMA-8645 • keychain review"), seat("aud-8645", "asma-8645-aud", "AUD • ASMA-8645 • keychain review")];
    expect(rows(reviews)[0]![2]).toEqual([["No epic linked", "asma-8645-aud", 1, "CircleDashed"], ["No epic linked", "asma-8645-qa", 1, "CircleDashed"]]);
  });

  it("files filled task labels through a capture that covers them, and says when one does not", () => {
    const task = (key: string) => ({ task: key });
    // The 21 Oct capture does not cover ASMA-8640: dated evidence that is silent, not a claim of no epic.
    expect(sessionEpic(roster(task)[1]!, {})).toEqual({ epic: null, note: `ASMA-8640 is not in the Jira capture of ${OCT21}`, association: "unlinked" });
    const filed = roster(task, fresh);
    expect(sessionEpic(filed[1]!, {})).toEqual({ epic: "ASMA-8639", note: `from the Jira capture of ${OCT24} for ASMA-8640`, association: "epic" });
    // One epic in two projects is one epic in the header; under a workstream filter it is one row.
    expect(rows(filed)).toEqual([["Other work", "1 epic", [["ASMA-8639", "asma-8640", 3, "Flag"], ["ASMA-8639", "asma-8641", 4, "Flag"]]]]);
    expect(rows(filed, true)).toEqual([["Other work", "1 epic", [["ASMA-8639", null, 7, "Flag"]]]]);
  });

  it("tells ad hoc, verified ticket, conflicting and unlinked sessions apart on every surface", () => {
    const created = { "create.request": "request-1" };
    const at = (id: string, labels: Record<string, string>, title = `LSA • ADHOC • ${id}`) => seat(id, "desk", title, labels);
    const cases: [DirectoryEntry, string, string][] = [
      [at("epic", { epic: "ASMA-1", persona: "kontor-lsa-adhoc@0123456789ab", ...created }), "epic", "ASMA-1"],
      [at("adhoc", { persona: "kontor-lsa-adhoc@0123456789ab", ...created }), "adhoc", "Ad hoc"],
      [at("bootstrap", { persona: "kontor-lsa-adhoc+bootstrap@ffffffffffff", ...created }), "adhoc", "Ad hoc"],
      [at("ticket", { ticket: "asma-9", persona: "kontor-lsa-bound@0123456789ab", ...created }), "ticket", "Ticket ASMA-9"],
      // A conflict never falls through to ad hoc.
      [at("conflict", { epic: "ASMA-1", "jira.epic": "ASMA-2", persona: "kontor-lsa-adhoc@0123456789ab", ...created }), "conflict", "Epic sources disagree"],
      // Without Kontor's creation record, neither the persona nor a ticket label settles anything; nor does a bound persona.
      [at("copied-persona", { persona: "kontor-lsa-adhoc@0123456789ab" }), "unlinked", "No epic linked"],
      [at("hand-ticket", { ticket: "ASMA-9" }), "unlinked", "No epic linked"],
      [at("bound", { persona: "kontor-lsa-bound@0123456789ab", ...created }), "unlinked", "No epic linked"],
      [at("titled", {}, "SWE • ASMA-8640 • Build the projection"), "unlinked", "No epic linked"],
    ];
    for (const [entry, association, label] of cases) {
      expect([entry.id, sessionEpic(entry, {}).association, associationLabel(sessionEpic(entry, {}))]).toEqual([entry.id, association, label]);
      expect(sessionProvenance(entry, {}).parts[1]).toBe(label);
    }
    expect(sessionEpic(cases[6]![0], {}).note).toBe("no epic or task label; the ticket label ASMA-9 has no Kontor creation record");
    const entries = cases.map(([entry]) => entry);
    expect(rows(entries)).toEqual([["Other work", "1 epic", [["ASMA-1", null, 1, "Flag"], ["Ad hoc", null, 2, "Sparkles"], ["Epic sources disagree", null, 1, "CircleAlert"],
      ["No epic linked", null, 4, "CircleDashed"], ["Ticket ASMA-9", null, 1, "Ticket"]]]]);
    // The epic filter reads the same resolver: one known epic; every other kind of work is without a linked epic.
    expect(epicScopeChoices(entries, {}).map(({ label }) => label)).toEqual(["ASMA-1", "No epic linked"]);
    expect(entries.filter((entry) => matchesEpicScope(entry, { kind: "unknown" }, {})).length).toBe(8);
    // Details name the same association the row does; only an unsettled epic reads Unknown.
    expect(cases.map(([entry]) => epicDetail(sessionEpic(entry, {})))).toEqual([
      "ASMA-1, from the epic label", "Ad hoc, created by Kontor as ad hoc work", "Ad hoc, created by Kontor as ad hoc work",
      "Ticket ASMA-9, a standalone ticket, checked in Jira when Kontor created the session",
      "Unknown, sources disagree: the epic label says ASMA-1; the jira.epic label says ASMA-2",
      "Unknown, no epic or task label", "Unknown, no epic or task label; the ticket label ASMA-9 has no Kontor creation record",
      "Unknown, no epic or task label", "Unknown, no epic or task label"]);
  });

  it("keeps a declared task unlinked, with the reason, when no source files it, whatever intent Kontor recorded", () => {
    const labels = { task: "ASMA-8640", persona: "kontor-lsa-adhoc@abc", "create.request": "request-1" };
    const unavailable = jiraCapture([read("jira", capture(OCT24, { "ASMA-8639": ["ASMA-8640"] }), "unavailable")]);
    expect(sessionEpic(seat("unavailable", "desk", "LSA • ADHOC • x", labels, unavailable), {})).toEqual({ epic: null, note: "no Jira capture read for ASMA-8640", association: "unlinked" });
    expect(sessionEpic(seat("missing", "desk", "LSA • ADHOC • x", labels), {})).toEqual({ epic: null, note: `ASMA-8640 is not in the Jira capture of ${OCT21}`, association: "unlinked" });
    // The ticket Kontor checked is not the declared task; its creation record is not denied.
    expect(sessionEpic(seat("ticket", "desk", "x", { ...labels, ticket: "ASMA-9" }), {})).toEqual({ epic: null, association: "unlinked",
      note: `ASMA-8640 is not in the Jira capture of ${OCT21}; the ticket ASMA-9 is not the declared task` });
    expect(sessionEpic(seat("filed", "desk", "x", labels, fresh), {}).epic).toBe("ASMA-8639");
    // A verified ticket that also names itself as the task is still that ticket.
    expect(sessionEpic(seat("self", "desk", "x", { ticket: "ASMA-9", task: "asma-9", "create.request": "request-1" }), {}).association).toBe("ticket");
  });

  it("counts only the epics a filter leaves visible", () => {
    const two = jiraCapture([read("jira", capture(OCT24, { "ASMA-8633": ["ASMA-8637"], "ASMA-8639": ["ASMA-8640"] }))]);
    const entries = [seat("a", "asma-8637", "SWE • a", { task: "ASMA-8637" }, two), seat("b", "asma-8640", "SWE • b", { task: "ASMA-8640" }, two)];
    const tree = buildOrganizationTree({ entries });
    expect(tree[0]!.detail).toBe("2 epics");
    const filtered = filterOrganizationTree(tree, (entry) => entry.id === "a");
    expect([filtered[0]!.detail, epicGroups(filtered[0]!.groups).map(({ epic }) => epic)]).toEqual(["1 epic", ["ASMA-8633"]]);
    expect(filterOrganizationTree(tree, (entry) => entry.id === "nothing")).toEqual([]);
  });
});
