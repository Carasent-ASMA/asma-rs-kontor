import { describe, expect, it } from "vitest";
import { jiraHistorySchema, type JiraHistory, type ReadSourceSummary, type SourceRead, type WorkstreamRegistry } from "../shared/read-sources";
import { projectJiraHistory } from "../server/jira-history";
import { jiraCapture } from "../client/page-data";
import { capturedWorkstreams, planFallbackNote, planWorkstreamMappings, planWorkstreamOptions } from "../client/plan-workstreams";
import { buildOrganizationTree, organizationScope, visibleSessions } from "../client/session-organization";
import { sessionEntry } from "./session-fixtures";

// The live shapes of 2026-10-24: Jira labels ASMA-7675 ws:qnr-v2 and ASMA-8633/8639 ws:kontor; the ASMA-7675 plan declares no workstream.
const generated = "2026-10-24T16:07+00:00";
const history: JiraHistory = { schema: "consumption/data@2", generated, provenance: { path: "/captures/data.json", sha256: "c".repeat(64), capturedEpics: 171 },
  selection: ["ASMA-7675", "ASMA-8633", "ASMA-8639", "ASMA-8701"],
  epics: [
    { key: "ASMA-7675", ws: "qnr-v2", status: "In Development", cat: "indeterminate", children: [{ key: "ASMA-8350", status: "In Development", cat: "indeterminate" }] },
    { key: "ASMA-8633", ws: "kontor", status: "In Development", cat: "indeterminate", children: [{ key: "ASMA-8634", status: "In Review", cat: "indeterminate" }] },
    { key: "ASMA-8639", ws: "kontor", status: "In Development", cat: "indeterminate", children: [] },
    { key: "ASMA-8701", status: "Groomed", cat: "new", children: [] },
  ] };
const jiraSource: ReadSourceSummary = { id: "jira", title: "Historical Jira", kind: "jira", path: "/captures/projection.json", workstream: null, epic: null, projectId: null, revisionKind: "working copy" };
const jiraRead: SourceRead = { source: jiraSource, status: "ready", revision: "d".repeat(64), readAt: "now", modifiedAt: null, content: null, graph: null, records: null, diagnostics: [], jiraHistory: history };
const qnrPlan: ReadSourceSummary = { id: "qnr", title: "QNR v2 Nonprod Delivery (ASMA-7675)", kind: "plan", path: "/docs/qnr.md", workstream: null, epic: "ASMA-7675", projectId: null, revisionKind: "working copy" };
const qnrRead = (workstream: string | null): SourceRead => ({ source: qnrPlan, status: "ready", revision: "e".repeat(64), readAt: "now", modifiedAt: null, content: null, graph: null, records: null, diagnostics: [], planMetadata: { workstream, epic: null, diagnostics: [] } });
const registry: WorkstreamRegistry = { path: "/registry.json", revision: "f".repeat(64),
  workstreams: { kontor: { name: "Kontor" }, "qnr-v2": { name: "QNR v2" }, access: { name: "Access & login" } },
  desks: { "pr-review-desk": { name: "PR Review Desk" }, "adam-adhoc": { name: "Adam • Ad-hoc", match: "fallback", paseo_project: "DESK • ADAM" }, qsw: { name: "QSW Desk", match: "first", paseo_project: "DESK • QSW" } },
  diagnostics: [] };
const jira = jiraCapture([jiraRead]);
const adam = { projectKey: "prj_adam", projectName: "DESK • ADAM", workspaceId: "wks_adam", workspaceName: "ADAM" };
const entries = [
  // ASMA-7675: committee seats with an epic label, an LSA with jira.epic, and one LSA nothing labels.
  sessionEntry("seat-a", { projectKey: "prj_qnr", projectName: "ESW • ASMA-7675 • QNR-P1", workspaceId: "wks_qnr", labels: { epic: "ASMA-7675" } }),
  sessionEntry("lsa-8350", { projectKey: "prj_qnr", projectName: "ESW • ASMA-7675 • QNR-P1", workspaceId: "wks_8350", labels: { role: "lsa", "jira.epic": "ASMA-7675" } }),
  sessionEntry("lsa-qnr-bare", { title: "LSA • ASMA-7675 • QNR-P1", projectKey: "prj_qnr", projectName: "ESW • ASMA-7675 • QNR-P1", workspaceId: "wks_qnr" }),
  // ASMA-8633: a task-only child, the LSA and TPM in DESK • ADAM (only the TPM labels a workstream).
  sessionEntry("qa-8634", { projectKey: "prj_8634", projectName: "ESW • ASMA-8634", workspaceId: "wks_8634", labels: { role: "qa", task: "ASMA-8634" } }),
  sessionEntry("lsa-8633", { ...adam, labels: { role: "LSA", epic: "ASMA-8633" } }),
  sessionEntry("tpm-8633", { ...adam, labels: { role: "tpm", epic: "ASMA-8633", workstream: "kontor" } }),
  // ASMA-8639: LSA and TPM in DESK • ADAM, neither labels a workstream.
  sessionEntry("lsa-8639", { ...adam, labels: { role: "LSA", epic: "ASMA-8639" } }),
  sessionEntry("tpm-8639", { ...adam, labels: { role: "TPM", epic: "ASMA-8639" } }),
  // Precedence and the desks that keep their sessions.
  sessionEntry("pr-desk", { projectKey: "prj_review", projectName: "DESK • PR REVIEW", workspaceId: "wks_review", labels: { epic: "ASMA-8633" } }),
  sessionEntry("qsw-desk", { projectKey: "prj_qsw", projectName: "DESK • QSW", workspaceId: "wks_qsw", labels: { epic: "ASMA-8633" } }),
  sessionEntry("labelled", { workspaceId: "wks_l", labels: { epic: "ASMA-7675", workstream: "tokenomics" } }),
  sessionEntry("mapped", { workspaceId: "wks_m", labels: { epic: "ASMA-7675" } }),
  sessionEntry("archived", { workspaceId: "wks_x", archivedAt: "2026-10-20T00:00:00.000Z", labels: { epic: "ASMA-8639" } }),
  sessionEntry("adam-adhoc", { ...adam, labels: { role: "LSA" } }),
  sessionEntry("unlabelled-epic", { workspaceId: "wks_u", labels: { epic: "ASMA-8701" } }),
].map((entry) => ({ ...entry, jira }));
const saved = { "session:mapped": { workstream: "custom" } };
const reads = { jira: jiraRead, qnr: qnrRead(null) };
const scope = (mappings: ReturnType<typeof planWorkstreamMappings>) => Object.fromEntries(entries.map((entry) => [entry.id, organizationScope(entry, mappings).workstream]));

describe("epic workstreams from the Jira capture's ws: labels", () => {
  it("projects ws on epics only, keeps reading captures without it, and rejects a malformed key", () => {
    const raw = { schema: "consumption/data@2", generated, facts: ["dropped"], epics: [
      { key: "ASMA-7675", ws: "qnr-v2", summary: "dropped", type: "Epic", status: "In Development", cat: "indeterminate", children: [{ key: "ASMA-8350", summary: "dropped", status: "Open", cat: "new", resolved: "" }] },
      ...Array.from({ length: 70 }, (_, index) => ({ key: `ASMA-${9000 + index}`, ws: "kontor", status: "Open", cat: "new", children: [] }))] };
    const keys = raw.epics.map((epic) => epic.key);
    const projected = projectJiraHistory(raw, keys, { path: "/captures/data.json", sha256: "c".repeat(64) });
    expect(projected.epics).toHaveLength(71);
    expect(projected.epics[0]).toEqual({ key: "ASMA-7675", ws: "qnr-v2", status: "In Development", cat: "indeterminate", children: [{ key: "ASMA-8350", status: "Open", cat: "new" }] });
    expect(JSON.stringify(projected)).not.toContain("dropped");
    expect(jiraHistorySchema.parse({ ...history, epics: history.epics.map(({ ws: _ws, ...epic }) => epic) }).epics[0]).not.toHaveProperty("ws");
    for (const ws of ["QNR v2", "ws:qnr-v2", ""]) expect(() => jiraHistorySchema.parse({ ...history, epics: [{ ...history.epics[0]!, ws }] })).toThrow();
    expect(() => jiraHistorySchema.parse({ ...history, epics: [{ ...history.epics[0]!, children: [{ key: "ASMA-8350", ws: "kontor", status: "Open", cat: "new" }] }] })).toThrow();
  });

  it("indexes each epic's label across ready captures; disagreement is unknown, silence is absent", () => {
    const other = { ...jiraRead, source: { ...jiraSource, id: "older" }, jiraHistory: { ...history, epics: [{ ...history.epics[0]!, ws: "reporting" }, { ...history.epics[2]!, ws: undefined }] } };
    const index = capturedWorkstreams([jiraRead, other, { ...jiraRead, status: "unavailable", jiraHistory: { ...history, epics: [{ ...history.epics[1]!, ws: "access" }] } }]);
    expect(Object.fromEntries(index)).toEqual({ "ASMA-7675": null, "ASMA-8633": "kontor", "ASMA-8639": "kontor" });
  });

  it("files QNR v2 and the newest Kontor epics under their workstreams after mappings and labels, and leaves unknowns unlinked", () => {
    expect(scope(planWorkstreamMappings([qnrPlan], reads, entries, [], saved, {}, registry))).toEqual({
      "seat-a": "qnr-v2", "lsa-8350": "qnr-v2", "lsa-qnr-bare": "Other work",
      "qa-8634": "kontor", "lsa-8633": "kontor", "tpm-8633": "kontor", "lsa-8639": "kontor", "tpm-8639": "kontor",
      "pr-desk": "pr-review", "qsw-desk": "qsw", labelled: "tokenomics", mapped: "custom", archived: "kontor", "adam-adhoc": "adam", "unlabelled-epic": "Other work",
    });
  });

  it("keeps every native desk first until the registry calls it a fallback desk", () => {
    const linked = scope(planWorkstreamMappings([qnrPlan], reads, entries, [], saved, {}));
    expect([linked["lsa-8633"], linked["tpm-8633"], linked["lsa-8639"], linked["tpm-8639"], linked["pr-desk"]]).toEqual(["adam", "kontor", "adam", "adam", "pr-review"]);
    const sameName = { ...registry, desks: { ...registry.desks, "adam-adhoc": { name: "Adam", match: "fallback" as const } } };
    expect(scope(planWorkstreamMappings([qnrPlan], reads, entries, [], saved, {}, sameName))["lsa-8633"]).toBe("adam");
  });

  it("links nothing where a plan or another capture disagrees, and explicit plan links stay plan-only", () => {
    const seat = [entries[0]!];
    expect(planWorkstreamMappings([qnrPlan], { ...reads, qnr: qnrRead("qnr-v2") }, seat, [], {}, {})).toEqual({ "session:seat-a": { workstream: "qnr-v2" } });
    expect(planWorkstreamMappings([qnrPlan], { ...reads, qnr: qnrRead("reporting") }, seat, [], {}, {})).toEqual({});
    expect(planWorkstreamMappings([qnrPlan], { ...reads, qnr: qnrRead("reporting") }, seat, [], {}, { "seat-a": ["qnr"] })).toEqual({ "session:seat-a": { workstream: "reporting" } });
    const disagreeing = { ...jiraRead, source: { ...jiraSource, id: "other" }, jiraHistory: { ...history, epics: [{ ...history.epics[0]!, ws: "reporting" }] } };
    expect(planWorkstreamMappings([qnrPlan], { ...reads, other: disagreeing }, seat, [], {}, {})).toEqual({});
    expect(planWorkstreamMappings([], {}, seat, [{ id: "wks_e", name: "Empty" }], { "workspace:wks_e": { epic: "ASMA-8639" } }, {})["workspace:wks_e"]).toEqual({ epic: "ASMA-8639" });
    expect(planWorkstreamMappings([], { jira: jiraRead }, [], [{ id: "wks_e", name: "Empty" }], { "workspace:wks_e": { epic: "ASMA-8639" } }, {})["workspace:wks_e"]).toEqual({ epic: "ASMA-8639", workstream: "kontor" });
  });

  it("offers the registered workstreams with honest counts, the tree agrees, and desk sessions stay under their desk", () => {
    const mappings = planWorkstreamMappings([qnrPlan], reads, entries, [], saved, {}, registry);
    const options = planWorkstreamOptions([qnrPlan, jiraSource], reads, registry, entries, mappings);
    const facts = Object.fromEntries(options.map((option) => [option.key, [option.sessions, option.registered, option.shownUnder]]));
    expect(facts["qnr-v2"]).toEqual([2, true, {}]);
    expect(facts.kontor).toEqual([6, true, { "DESK • ADAM": 4 }]);
    expect(facts.access).toEqual([0, true, {}]);
    // My work offers choices with sessions; the zero-session registered choice is what All work adds.
    expect(options.filter((option) => option.sessions === 0).map((option) => option.key)).toEqual(["access"]);
    const tree = buildOrganizationTree({ entries, displayMappings: mappings });
    const top = Object.fromEntries(tree.map((group) => [group.label, visibleSessions([group]).map((entry) => entry.id).sort()]));
    expect(top["qnr-v2"]).toEqual(["seat-a", "lsa-8350"].sort());
    expect(top["Other work"]).toEqual(["lsa-qnr-bare", "unlabelled-epic"]);
    expect(Object.values(top).flat().sort()).toEqual(entries.map((entry) => entry.id).sort());
    // Filing follows the epic; the tree still shows each native desk's sessions under that desk.
    expect(top["DESK • ADAM"]).toEqual(["adam-adhoc", "lsa-8633", "lsa-8639", "tpm-8633", "tpm-8639"]);
    expect([top["DESK • PR REVIEW"], top["DESK • QSW"], top.kontor]).toEqual([["pr-desk"], ["qsw-desk"], ["archived", "qa-8634"]]);
  });

  it("says why a session stays unlinked, naming the capture and its repair", () => {
    const sources = [qnrPlan, jiraSource];
    expect(planFallbackNote(sources, reads, null, null)).toBe("No known ASMA epic, so no plan or Jira ws: label applies.");
    expect(planFallbackNote(sources, reads, "ASMA-8701", null)).toBe(`No configured plan declares ASMA-8701. ASMA-8701 has no ws: label in the Jira capture of ${generated}.`);
    expect(planFallbackNote(sources, reads, "ASMA-9999", null)).toBe(`No configured plan declares ASMA-9999. ASMA-9999 is not in the Jira capture of ${generated}.`);
    expect(planFallbackNote(sources, { ...reads, qnr: qnrRead("reporting") }, "ASMA-7675", null))
      .toBe(`Of 1 plan for ASMA-7675, 1 declares reporting, so none applies. The Jira capture of ${generated} labels ASMA-7675 ws:qnr-v2, which a plan contradicts.`);
    expect(planFallbackNote([qnrPlan], { qnr: qnrRead(null) }, null, null)).toBe("No known ASMA epic, so no plan applies.");
  });
});
