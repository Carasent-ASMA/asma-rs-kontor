import { readFileSync } from "node:fs";
import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { URL } from "node:url";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { jiraHistorySchema, type CanonicalGraph, type SourceRead } from "../shared/read-sources";
import { projectJiraHistory } from "../server/jira-history";
import { readConfiguredSource } from "../server/read-sources";
import { historicalJiraIssue, historicalPlanProgress } from "../client/page-data";
const history = jiraHistorySchema.parse(JSON.parse(readFileSync(new URL("./fixtures/jira-history.json", import.meta.url), "utf8")));
const read: SourceRead = { source: { id: "jira", title: "Historical Jira", kind: "jira", path: "/configured/projection.json", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }, status: "ready", revision: "a".repeat(64), readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: null, graph: null, records: null, diagnostics: [], jiraHistory: history };
const graph: CanonicalGraph = { schema_version: 1, source: "/plan.md", source_sha256: "a".repeat(64), graph_sha256: "b".repeat(64), epic: { jira: "ASMA-8539", kontor_id: null }, nodes: [8540, 8541, 8551, 8542, 8545, 8546, 8547, 8548, 8549, 8550].map((key, i) => ({ id: `task-${i}`, title: "Title is never used for status matching", jira: `ASMA-${key}`, kontor_id: null, phase: "Phase", wave: 0, depends_on: [] })), edges: [], waves: [], external_fences: [] };
describe("bounded historical Jira evidence", () => {
  it("projects only allowed fields for explicit epic keys with original time and provenance", () => {
    const projected = projectJiraHistory({ schema: history.schema, generated: history.generated, epics: history.epics, facts: [{ credential: "do-not-copy" }], seats: ["do-not-copy"], account: "do-not-copy" }, ["ASMA-8539"], history.provenance);
    expect(projected.epics.map((epic) => epic.key)).toEqual(["ASMA-8539"]);
    expect(projected.epics[0]?.children).toHaveLength(3);
    expect(projected.generated).toBe("2026-10-18T17:00+00:00");
    expect(JSON.stringify(projected)).not.toContain("do-not-copy");
    expect(Object.keys(projected.epics[0]!.children[0]!)).toEqual(["key", "status", "cat"]);
  });
  it("shows three matched tasks out of ten, with seven Unknown, rather than a denominator of three", () => {
    expect(history.epics.map((epic) => [epic.key, epic.children.length]).sort()).toEqual([["ASMA-8190", 17], ["ASMA-8371", 41], ["ASMA-8539", 3]]);
    expect(historicalPlanProgress(graph, [read])).toMatchObject({ total: 10, known: 3, done: 0, unknown: 7 });
    expect(historicalJiraIssue("ASMA-8539", null, [read])?.issue.status).toBe("Groomed");
    expect(historicalJiraIssue("ASMA-8541", "ASMA-8539", [read])?.issue.status).toBe("READY FOR DEVELOPMENT");
  });
  it("does not infer from titles, near keys, wrong parents, missing observations or conflicts", () => {
    for (const key of ["asma-8540", "ASMA-85400", "8540", "Title is never used for status matching"]) expect(historicalJiraIssue(key, "ASMA-8539", [read])).toBeNull();
    expect(historicalJiraIssue("ASMA-8540", "ASMA-8371", [read])).toBeNull();
    expect(historicalPlanProgress(graph, [{ ...read, status: "unavailable" }]).known).toBe(0);
    const changed = structuredClone(read); changed.jiraHistory!.epics.find((epic) => epic.key === "ASMA-8539")!.children[0]!.status = "Conflicting capture";
    expect(historicalJiraIssue("ASMA-8540", "ASMA-8539", [read, changed])).toBeNull();
    expect(historicalPlanProgress({ ...graph, epic: { jira: null, kontor_id: null } }, [read]).known).toBe(0);
  });
  it("rejects unsupported, invalid or ambiguous records rather than inventing status", () => {
    expect(() => jiraHistorySchema.parse({ ...history, schema: "something-else" })).toThrow();
    expect(() => jiraHistorySchema.parse({ ...history, generated: "today" })).toThrow();
    expect(() => jiraHistorySchema.parse({ ...history, epics: [...history.epics, history.epics[0]] })).toThrow();
  });
  it("reads a configured bounded projection without changing its observation time or leaking raw content", async () => {
    const directory = await mkdtemp(join(tmpdir(), "kontor-jira-history-"));
    try {
      const path = join(directory, "projection.json"); await writeFile(path, JSON.stringify(history));
      const result = await readConfiguredSource({ ...read.source, path });
      expect(result.status).toBe("ready"); expect(result.jiraHistory?.generated).toBe(history.generated);
      expect(result.content).toBeNull(); expect(result.records).toBeNull();
      await writeFile(path, JSON.stringify({ ...history, facts: ["unapproved payload"] }));
      expect((await readConfiguredSource({ ...read.source, path })).status).toBe("unavailable");
    } finally { await rm(directory, { recursive: true, force: true }); }
  });
});
