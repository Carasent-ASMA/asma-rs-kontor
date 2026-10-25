import { execFileSync } from "node:child_process";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { confirmedEpic, epicIdentitySource, jiraCapture } from "../client/page-data";
import type { DirectoryEntry } from "../client/session-directory";
import { buildOrganizationTree, EPIC_LABELS, organizationScope, sessionEpic, TASK_JIRA_LABELS, type OrganizationGroup } from "../client/session-organization";
import { sessionProvenance } from "../client/session-results";
import { epicIdentitySchema, jiraHistorySchema, sourceReadPending, type EpicIdentity, type JiraHistory, type SourceRead } from "../shared/read-sources";
import { readConfiguredSource, readConfiguredSummary } from "../server/read-sources";
import { sessionEntry } from "./session-fixtures";

// The collector's own synthetic run: ASMA-10 epic > ASMA-11 story > ASMA-12 sub-task; ASMA-20 epic > ASMA-41, which
// Jira returns for ASMA-40; ASMA-30 a task with no epic; ASMA-50, whose parent ASMA-51 is not returned; ASMA-404 not returned.
const exported = epicIdentitySchema.parse(JSON.parse(execFileSync("python3", [join(__dirname, "../scripts/export-epic-identity.py"), "--self-check"], { encoding: "utf8" })));
const AS_OF = exported.as_of;
const source = { id: "identity", title: "Jira identity", kind: "jira" as const, path: "/configured/identity.json", projectId: null, epic: null, workstream: null, revisionKind: "working copy" as const };
const read = (document: { epicIdentity: EpicIdentity } | { jiraHistory: JiraHistory }, id = "identity"): SourceRead =>
  ({ source: { ...source, id }, status: "ready", revision: "a".repeat(64), readAt: AS_OF, modifiedAt: null, content: null, graph: null, records: null, diagnostics: [], ...document });
const OCT24 = "2026-10-24T16:07+00:00";
const dated = jiraHistorySchema.parse({ schema: "consumption/data@2", generated: OCT24, provenance: { path: "/configured/capture.json", sha256: "b".repeat(64), capturedEpics: 2 },
  selection: ["ASMA-99", "ASMA-8108"], epics: [{ key: "ASMA-99", status: "Done", cat: "done", children: [{ key: "ASMA-12", status: "Done", cat: "done" }, { key: "ASMA-404", status: "Done", cat: "done" }] },
    { key: "ASMA-8108", status: "Done", cat: "done", children: [{ key: "ASMA-8343", status: "Done", cat: "done" }] }] });
const at = (id: string, labels: Record<string, string>, reads: readonly SourceRead[] = [read({ epicIdentity: exported })]): DirectoryEntry =>
  ({ ...sessionEntry(id, { labels, projectKey: "prj", projectName: "desk", workspaceName: "w" }), jira: jiraCapture(reads) });
const epicGroups = (groups: readonly OrganizationGroup[]): OrganizationGroup[] => groups.flatMap((group) => [...(group.kind === "epic" ? [group] : []), ...epicGroups(group.groups)]);

describe("current Jira identity export", () => {
  it("reads every label the resolver reads, and the reader accepts what the collector writes", () => {
    expect(exported.provenance.label_keys).toEqual([...EPIC_LABELS, ...TASK_JIRA_LABELS, "ticket"]);
    expect(exported.issues.map(({ key, via }) => [key, via])).toEqual([["ASMA-10", "scope"], ["ASMA-11", "parent"], ["ASMA-12", "declared"], ["ASMA-20", "declared"], ["ASMA-30", "declared"], ["ASMA-41", "declared"], ["ASMA-50", "declared"]]);
    expect(exported.coverage).toMatchObject({ declared: 6, found: 5, missing: ["ASMA-51", "ASMA-404"], out_of_scope: ["EX-1"], unresolved: ["ASMA-50"] });
    expect(epicIdentitySource([exported])?.unanswered.get("ASMA-50")).toBe("the parent of ASMA-50 was not read");
  });

  it("refuses an ambiguous, dangling or extended document, and reads a host with more sessions than issues", () => {
    expect(epicIdentitySchema.parse({ ...exported, provenance: { ...exported.provenance, sessions: { read: 5598, unreadable: 0 } } }).provenance.sessions.read).toBe(5598);
    expect(() => epicIdentitySchema.parse({ ...exported, issues: [...exported.issues, exported.issues[0]] })).toThrow(/duplicate/);
    expect(() => epicIdentitySchema.parse({ ...exported, conflicts: [{ ...exported.conflicts[0]!, current: "ASMA-77" }] })).toThrow(/lacks/);
    expect(() => epicIdentitySchema.parse({ ...exported, assignee: "not read" })).toThrow();
    expect(() => epicIdentitySchema.parse({ ...exported, issues: [{ ...exported.issues[0]!, summary: undefined }] })).toThrow();
  });

  it("is read as a jira source, and a configured epic must be one it observed as an epic", async () => {
    const directory = await mkdtemp(join(tmpdir(), "kontor-epic-identity-"));
    try {
      const path = join(directory, "identity.json");
      await writeFile(path, JSON.stringify(exported));
      for (const reader of [readConfiguredSource, readConfiguredSummary]) {
        const result = await reader({ ...source, path, epic: "ASMA-10" });
        expect([result.status, result.epicIdentity?.order, result.jiraHistory]).toEqual(["ready", exported.order, undefined]);
        expect(sourceReadPending(result)).toBe(false);
        const story = await reader({ ...source, path, epic: "ASMA-11" });
        expect([story.status, story.diagnostics[0]]).toEqual(["unavailable", expect.stringContaining("not an epic in the Jira identity export")]);
      }
    } finally { await rm(directory, { recursive: true, force: true }); }
  });

  it("files a declared key under its current epic on every surface, before any dated capture", () => {
    const entry = at("subtask", { task: "ASMA-12" }, [read({ epicIdentity: exported }), read({ jiraHistory: dated }, "dated")]);
    // The 24 Oct capture filed ASMA-12 under ASMA-99; the current identity supersedes it for that key, without a conflict.
    expect(sessionEpic(entry, {})).toEqual({ epic: "ASMA-10", note: `from Jira as observed ${AS_OF} for ASMA-12`, association: "epic" });
    expect([organizationScope(entry, {}).epic, confirmedEpic(entry), sessionProvenance(entry, {}).parts[1]]).toEqual(["ASMA-10", "ASMA-10", "ASMA-10"]);
    expect(epicGroups(buildOrganizationTree({ entries: [entry] })).map(({ epic }) => epic)).toEqual(["ASMA-10"]);
    // A key the identity does not cover falls back to the dated capture, which says when it was observed.
    expect(sessionEpic(at("old", { task: "ASMA-8343" }, [read({ epicIdentity: exported }), read({ jiraHistory: dated }, "dated")]), {}).note).toBe(`from the Jira capture of ${OCT24} for ASMA-8343`);
    // Jira returns ASMA-40 as ASMA-41; the epic label and the identity agree.
    expect(sessionEpic(at("moved", { epic: "ASMA-20", task: "ASMA-40" }), {})).toEqual({ epic: "ASMA-20", note: `from the epic label and Jira as observed ${AS_OF} for ASMA-20 and Jira as observed ${AS_OF} for ASMA-40 (now ASMA-41)`, association: "epic" });
  });

  it("keeps a task with no epic, an unreturned key and an uncovered key unlinked, and says which", () => {
    const created = { persona: "kontor-lsa-adhoc@0123456789ab", "create.request": "request-1" };
    expect(sessionEpic(at("standalone", { task: "ASMA-30", ...created }), {})).toEqual({ epic: null, note: `ASMA-30 (Task) has no epic in Jira as observed ${AS_OF}`, association: "unlinked" });
    expect(sessionEpic(at("gone", { task: "ASMA-404" }), {}).note).toBe(`Jira did not return ASMA-404 as of ${AS_OF}`);
    // A key the identity asked for and Jira did not return never falls back to a dated capture; that capture stays apart.
    expect(sessionEpic(at("gone-dated", { task: "ASMA-404" }, [read({ epicIdentity: exported }), read({ jiraHistory: dated }, "dated")]), {}))
      .toEqual({ epic: null, note: `Jira did not return ASMA-404 as of ${AS_OF}; the Jira capture of ${OCT24} filed it under ASMA-99`, association: "unlinked" });
    // An explicit epic label stays the assignment when Jira could not answer; the note says it was not verified.
    expect(sessionEpic(at("label-unverified", { epic: "ASMA-404" }), {})).toEqual({ epic: "ASMA-404", note: `from the epic label; not verified: Jira did not return ASMA-404 as of ${AS_OF}`, association: "epic" });
    expect(sessionEpic(at("label-task-unverified", { epic: "ASMA-20", task: "ASMA-404" }), {}).note).toBe(`from the epic label and Jira as observed ${AS_OF} for ASMA-20; not verified: Jira did not return ASMA-404 as of ${AS_OF}`);
    expect(sessionEpic(at("later", { "jira.issue": "ASMA-999" }), {}).note).toBe(`ASMA-999 is not in the Jira identity read of ${AS_OF}`);
    expect(sessionEpic(at("both", { "jira.issue": "ASMA-999" }, [read({ epicIdentity: exported }), read({ jiraHistory: dated }, "dated")]), {}).note)
      .toBe(`ASMA-999 is not in the Jira identity read of ${AS_OF} or the Jira capture of ${OCT24}`);
    // An epic label naming an issue Jira shows with no epic above it is a disagreement, never an epic.
    expect(sessionEpic(at("not-an-epic", { epic: "ASMA-30" }), {})).toEqual({ epic: null, note: `sources disagree: the epic label says ASMA-30; Jira as observed ${AS_OF} for ASMA-30 says no epic`, association: "conflict" });
    expect(sessionEpic(at("story-as-epic", { epic: "ASMA-11" }), {}).association).toBe("conflict");
    // A verified ticket Jira shows standalone stays that ticket; one Jira now files under an epic is that epic's.
    expect(sessionEpic(at("ticket", { ticket: "ASMA-30", task: "ASMA-30", "create.request": "request-1" }), {}))
      .toEqual({ epic: null, note: `a standalone ticket, checked in Jira when Kontor created the session; ASMA-30 (Task) has no epic in Jira as observed ${AS_OF}`, association: "ticket", ticket: "ASMA-30" });
    expect(sessionEpic(at("filed-ticket", { ticket: "ASMA-12", "create.request": "request-1" }), {})).toEqual({ epic: "ASMA-10", note: `from Jira as observed ${AS_OF} for ASMA-12`, association: "epic" });
    expect(sessionEpic(at("gone-ticket", { ticket: "ASMA-404", "create.request": "request-1" }), {}).note).toBe(`a standalone ticket, checked in Jira when Kontor created the session; Jira did not return ASMA-404 as of ${AS_OF}`);
  });

  it("reads an epic key Jira now returns under another as that one issue, keeping the old key in the note", () => {
    const moved = structuredClone(exported);
    moved.conflicts.push({ kind: "moved_key", key: "ASMA-1", current: "ASMA-20", observed_at: AS_OF });
    const entry = at("moved-epic", { epic: "asma-1", task: "ASMA-40" }, [read({ epicIdentity: moved })]);
    expect(sessionEpic(entry, {})).toEqual({ epic: "ASMA-20", association: "epic",
      note: `from the epic label ASMA-1 and Jira as observed ${AS_OF} for ASMA-1 (now ASMA-20) and Jira as observed ${AS_OF} for ASMA-40 (now ASMA-41)` });
    expect(confirmedEpic(entry)).toBe("ASMA-20");
    // The seam keeps what the catalog will serve: the immutable ID, parent, update and observation times, provenance and coverage.
    const source = epicIdentitySource([moved])!;
    expect(source.issues.get("ASMA-40")).toEqual({ id: "100041", key: "ASMA-41", type: "Story", parent: "ASMA-20", epic: "ASMA-20", updated: "2026-01-01T08:00:00Z", observedAt: expect.any(String) });
    expect(source.issues.get("ASMA-1")?.id).toBe(source.issues.get("ASMA-20")?.id);
    expect([source.order, source.provenance, source.coverage]).toEqual([exported.order, { realm: "tracker.example", scope: exported.scope.jql, collector: exported.provenance.collector }, exported.coverage]);
  });

  it("uses the newest export only and says when a parent was not observed", () => {
    const older = structuredClone(exported);
    older.order -= 1;
    older.issues.find(({ key }) => key === "ASMA-11")!.parent = "ASMA-20";
    expect(epicIdentitySource([exported, older])?.issues.get("ASMA-12")?.epic).toBe("ASMA-10");
    expect(epicIdentitySource([older])?.issues.get("ASMA-12")?.epic).toBe("ASMA-20");
    const orphan = structuredClone(exported);
    orphan.issues = orphan.issues.filter(({ key }) => key !== "ASMA-11");
    expect(epicIdentitySource([orphan])?.unanswered.get("ASMA-12")).toBe("the parent of ASMA-12 was not read");
    expect(sessionEpic(at("orphan", { task: "ASMA-12" }, [read({ epicIdentity: orphan })]), {}).note).toBe(`the parent of ASMA-12 was not read as of ${AS_OF}`);
    // A moved key whose current issue lacks its parent is unanswered too, never left to a dated capture.
    const movedOrphan = structuredClone(orphan);
    movedOrphan.conflicts.push({ kind: "moved_key", key: "ASMA-2", current: "ASMA-12", observed_at: AS_OF });
    expect(epicIdentitySource([movedOrphan])?.unanswered.get("ASMA-2")).toBe("the parent of ASMA-12 was not read");
    expect(epicIdentitySource([])).toBeUndefined();
  });
});
