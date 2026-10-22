import type { ReadSourceSummary, SourceRead } from "../../../paseo-plugin/shared/read-sources";

/**
 * Fictional configured sources: two plans with canonical graphs and one dated
 * Jira capture. Hashes are fixed placeholders, not digests of real files.
 */
const hex = (seed: string) => seed.repeat(64).slice(0, 64);
const READ_AT = "2026-10-20T08:58:00.000Z";

type Node = { id: string; title: string; phase: string; jira: string | null; kontor_id: string | null; depends_on: string[]; wave: number };

function plan(id: string, title: string, epic: string, path: string, revision: string, nodes: Node[]): SourceRead {
  const source: ReadSourceSummary = { id, title, kind: "plan", path, workstream: "Kontor", epic, projectId: "proj_kontor_demo", revisionKind: "approved revision", expectedSha256: null };
  const waves = [...new Set(nodes.map((node) => node.wave))].sort().map((wave) => ({ wave, tasks: nodes.filter((node) => node.wave === wave).map((node) => node.id) }));
  const rows = nodes.map((node) => `| ${node.id} | ${node.title} | ${node.jira ?? "—"} | ${node.phase} | ${node.depends_on.join(", ") || "—"} |`);
  const content = [
    `# ${title}`, "", `Epic: ${epic}. Fictional plan for the Kontor review prototype.`, "",
    "## Tasks", "", "| Task | Title | Jira | Phase | After |", "| --- | --- | --- | --- | --- |", ...rows, "",
    "## Notes", "", "Roles are not assigned in this plan. Session labels are the only task link the prototype reads.",
  ].join("\n");
  return {
    source, status: "ready", revision, readAt: READ_AT, modifiedAt: "2026-10-19T16:20:00.000Z", content,
    graph: {
      schema_version: 1, source: path, source_sha256: revision, graph_sha256: hex(revision.slice(0, 1) === "a" ? "b" : "a"),
      epic: { jira: epic, kontor_id: null }, nodes,
      edges: nodes.flatMap((node) => node.depends_on.map((from) => ({ from, to: node.id }))), waves, external_fences: [],
    },
    records: null, diagnostics: [],
  };
}

export const PLAN_99101 = plan("plan-99101", "Sessions organisation", "ASMA-99101", "docs/plans/asma-99101.md", hex("a1"), [
  { id: "KON-1", title: "Flat filtered results", phase: "Build", jira: "ASMA-99103", kontor_id: null, depends_on: [], wave: 0 },
  { id: "KON-2", title: "Board team chain", phase: "Build", jira: "ASMA-99104", kontor_id: null, depends_on: [], wave: 0 },
  { id: "KON-3", title: "Restore tree on clear", phase: "Build", jira: "ASMA-99105", kontor_id: null, depends_on: ["KON-1"], wave: 1 },
  { id: "KON-4", title: "Retire legacy check", phase: "Cleanup", jira: "ASMA-99106", kontor_id: null, depends_on: [], wave: 0 },
  { id: "KON-5", title: "Rollout notes", phase: "Release", jira: "ASMA-99108", kontor_id: null, depends_on: ["KON-2", "KON-3"], wave: 2 },
  { id: "KON-6", title: "Native device pass", phase: "Verify", jira: null, kontor_id: null, depends_on: ["KON-2"], wave: 1 },
]);

export const PLAN_99102 = plan("plan-99102", "Board teams", "ASMA-99102", "docs/plans/asma-99102.md", hex("c2"), [
  { id: "KTB-1", title: "Parallel verification", phase: "Build", jira: "ASMA-99107", kontor_id: null, depends_on: [], wave: 0 },
  { id: "KTB-2", title: "Chooser polish", phase: "Build", jira: "ASMA-99113", kontor_id: null, depends_on: ["KTB-1"], wave: 1 },
]);

export const JIRA_CAPTURE: SourceRead = {
  source: { id: "jira-capture", title: "Jira capture (fixture)", kind: "jira", path: "fixtures/jira-capture.json", workstream: null, epic: null, projectId: null, revisionKind: "approved revision", expectedSha256: null },
  status: "ready", revision: hex("d3"), readAt: READ_AT, modifiedAt: "2026-10-19T18:00:00.000Z", content: null, graph: null, records: null, diagnostics: [],
  jiraHistory: {
    schema: "consumption/data@2", generated: "2026-10-19T18:00:00.000Z",
    provenance: { path: "fixtures/jira-capture.json", sha256: hex("e4"), capturedEpics: 3 },
    selection: ["ASMA-99101", "ASMA-99102", "ASMA-99201"],
    epics: [
      { key: "ASMA-99101", status: "In Progress", cat: "indeterminate", children: [
        { key: "ASMA-99103", status: "In Progress", cat: "indeterminate" },
        { key: "ASMA-99104", status: "In Review", cat: "indeterminate" },
        { key: "ASMA-99105", status: "Done", cat: "done" },
        { key: "ASMA-99106", status: "Won't Do", cat: "done" },
        { key: "ASMA-99108", status: "To Do", cat: "new" },
        { key: "ASMA-99114", status: "To Do", cat: "new" },
      ] },
      { key: "ASMA-99102", status: "In Progress", cat: "indeterminate", children: [
        { key: "ASMA-99107", status: "In Progress", cat: "indeterminate" },
        { key: "ASMA-99113", status: "To Do", cat: "new" },
      ] },
      { key: "ASMA-99201", status: "In Progress", cat: "indeterminate", children: [
        { key: "ASMA-99202", status: "In Progress", cat: "indeterminate" },
      ] },
    ],
  },
};

export const SOURCE_READS: readonly SourceRead[] = [PLAN_99101, PLAN_99102, JIRA_CAPTURE];
