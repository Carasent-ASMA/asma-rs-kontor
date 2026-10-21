import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { discoverPlanCatalog, discoverPlanSources } from "../server/plan-discovery";

const roots: string[] = [];
afterEach(async () => { await Promise.all(roots.splice(0).map((root) => rm(root, { recursive: true, force: true }))); });
const plan = (epic: string | null) => `# Example\n\n\`\`\`plan-graph\n${epic ?? "unlinked"}\n\`\`\`\n`;
async function checkout(files: Record<string, string>) {
  const root = await mkdtemp(join(tmpdir(), "kontor-discovery-")); roots.push(root);
  execFileSync("git", ["init", "-q", root]);
  for (const [relative, content] of Object.entries(files)) { const path = join(root, relative); await mkdir(dirname(path), { recursive: true }); await writeFile(path, content); }
  execFileSync("git", ["-C", root, "add", "."]);
  execFileSync("git", ["-C", root, "-c", "user.name=Test", "-c", "user.email=test@example.com", "-c", "commit.gpgsign=false", "commit", "-qm", "fixture"]);
  return root;
}
async function exporter(path: string) {
  const content = await readFile(path, "utf8");
  if (content.includes("invalid")) throw new Error("Canonical graph invalid");
  return JSON.stringify({ schema_version: 1, source: path, source_sha256: createHash("sha256").update(content).digest("hex"), graph_sha256: "graph",
    epic: { jira: /ASMA-\d+/.exec(content)?.[0] ?? null, kontor_id: null }, nodes: [], edges: [], waves: [], external_fences: [] });
}
describe("offline plan-source discovery", () => {
  it.each([
    ["plain scalar", "---\nworkstream: qnr-v2\n---\n", "qnr-v2"],
    ["quoted scalar and comment", "---\nworkstream: 'ad-voca' # explicit\n---\n", "ad-voca"],
    ["BOM and CRLF", '\uFEFF---\r\nworkstream: "kontor"\r\n---\r\n', "kontor"],
    ["multiline goal", "---\ngoal: >-\n  workstream: unrelated\nworkstream: pr-lifecycle\n---\n", "pr-lifecycle"],
    ["body only", "# Workstream kontor\nworkstream: kontor\n", null],
    ["later frontmatter", "# Example\n---\nworkstream: kontor\n---\n", null],
    ["nested field", "---\ntask:\n  workstream: kontor\n---\n", null],
    ["missing field", "---\ntitle: Kontor\n---\n", null],
    ["null field", "---\nworkstream: null\n---\n", null],
    ["duplicate field", "---\nworkstream: kontor\nworkstream: qnr-v2\n---\n", null],
    ["alias", "---\nother: &stream kontor\nworkstream: *stream\n---\n", null],
    ["merge", "---\nother: &fields {workstream: kontor}\n<<: *fields\n---\n", null],
    ["non-scalar", "---\nworkstream: [kontor]\n---\n", null],
    ["tagged value", "---\nworkstream: !custom kontor\n---\n", null],
    ["invalid YAML", "---\nworkstream: \"kontor\n---\n", null],
    ["unterminated header", "---\nworkstream: kontor\n", null],
  ])("keeps only explicit top-level workstream metadata: %s", async (_name, header, expected) => {
    const root = await checkout({ "plans/a.md": header + plan("ASMA-1") });
    const report = await discoverPlanSources(root, exporter);
    expect(report.catalog).toHaveLength(1);
    expect(report.catalog[0]).toMatchObject({ epic: "ASMA-1", projectId: null, workstream: expected });
    expect(report.coverage[0]?.status).toBe("eligible");
  });
  it("refreshes explicit metadata from the same working-copy content as the canonical graph", async () => {
    const root = await checkout({ "plans/a.md": "---\nworkstream: kontor\n---\n" + plan("ASMA-1") });
    expect((await discoverPlanSources(root, exporter)).catalog[0]?.workstream).toBe("kontor");
    await writeFile(join(root, "plans/a.md"), plan("ASMA-1"));
    expect((await discoverPlanSources(root, exporter)).catalog[0]?.workstream).toBeNull();
  });
  it("uses validated canonical bindings, excludes policy examples/untracked plans, and preserves ambiguous sources", async () => {
    const root = await checkout({ "plans/a.md": plan("ASMA-1"), "plans/b.md": plan("ASMA-1"), "plans/unlinked.md": plan(null), "plans/ASMA-2.md": "# ASMA-2\nNo graph", ".agents/skills/example.md": plan("ASMA-3") });
    await writeFile(join(root, "untracked.md"), plan("ASMA-4"));
    const report = await discoverPlanSources(root, exporter);
    expect(report.catalog.map((source) => source.epic)).toEqual([null, "ASMA-1", "ASMA-1", null]);
    expect(report.conflicts).toEqual(["ASMA-1"]);
    expect(report.coverage.map((row) => row.status)).toEqual(["unlinked", "eligible", "eligible", "unlinked"]);
    expect(new Set(report.catalog.map((source) => source.id)).size).toBe(4);
    expect(report.catalog.every((source) => source.projectId === null)).toBe(true);
    expect((await discoverPlanSources(root, exporter)).catalog).toEqual(report.catalog);
  });
  it("reports parse failures, edits, missing files and fresh working-copy hashes without substituting another plan", async () => {
    const root = await checkout({ "plans/a.md": plan("ASMA-1"), "plans/b.md": plan("ASMA-2") + "invalid" });
    const initial = await discoverPlanSources(root, exporter);
    expect(initial.coverage[1]).toMatchObject({ status: "invalid", tasks: null });
    const original = initial.coverage[0]!.revision;
    await writeFile(join(root, "plans/a.md"), plan("ASMA-1") + "edit");
    expect((await discoverPlanSources(root, exporter)).coverage[0]!.revision).not.toBe(original);
    await rm(join(root, "plans/a.md"));
    const missing = await discoverPlanSources(root, exporter);
    expect(missing.catalog.map((source) => source.epic)).toEqual([null]);
    expect(missing.coverage.every((row) => row.status === "invalid")).toBe(true);
  });
});

it.each(["plans/declared.md", "_docs/domain/implementation-plan.md"])("retains unavailable coverage for an admitted graphless plan: %s", async (relative) => {
  const root = await checkout({ [relative]: "---\nworkstream: reporting\nepic: ASMA-9\n---\n# Declared plan", "plans/README.md": "# Plan directory", "_docs/unrelated.md": "# Documentation" });
  expect((await discoverPlanSources(root, exporter)).catalog).toHaveLength(1);
  await Promise.all([relative, "plans/README.md", "_docs/unrelated.md"].map((path) => rm(join(root, path))));
  const missing = await discoverPlanSources(root, exporter);
  expect(missing.catalog).toEqual([]);
  expect(missing.coverage).toHaveLength(1);
  expect(missing.coverage[0]).toMatchObject({ path: relative, status: "invalid", revision: null, tasks: null, boundTasks: null });
  expect(missing.coverage[0]!.diagnostics.join(" ")).toContain("ENOENT");
  expect(missing.coverage[0]!.diagnostics.join(" ")).toContain(relative);
  expect((await discoverPlanCatalog([root], [], exporter)).reports[0]!.coverage).toEqual(missing.coverage);
});

it("discovers graphless declarations and deduplicates worktree copies while retaining differing revisions and pins", async () => {
  const root = await checkout({ "plans/plain.md": "---\nworkstream: reporting\nepic: ASMA-9\n---\n# No graph or ESW" });
  const first = await discoverPlanSources(root, exporter);
  expect(first.catalog[0]).toMatchObject({ workstream: "reporting", epic: "ASMA-9" });
  const copy = root + "-copy"; roots.push(copy);
  execFileSync("git", ["-C", root, "worktree", "add", "--detach", copy], { stdio: "ignore" });
  expect((await discoverPlanCatalog([root, copy], [], exporter)).catalog).toHaveLength(1);
  await writeFile(join(copy, "plans/plain.md"), "---\nworkstream: access\nepic: ASMA-9\n---\n# Different revision");
  expect((await discoverPlanCatalog([root, copy], [], exporter)).catalog).toHaveLength(2);
  const retained = join(root, "retained.md");
  await writeFile(retained, await readFile(join(root, "plans/plain.md")));
  const pinned = { ...first.catalog[0]!, path: retained, expectedSha256: first.coverage[0]!.revision!, revisionKind: "approved revision" as const };
  expect((await discoverPlanCatalog([root], [pinned], exporter)).catalog).toEqual([pinned]);
});

it("appends new plans while retaining existing source IDs, order, explicit mappings and historical pins", async () => {
  const root = await checkout({ "plans/current.md": "---\nworkstream: reporting\n---\n# Current", "plans/new.md": "# Unknown workstream and epic" });
  const discovered = await discoverPlanSources(root, exporter);
  const current = { ...discovered.catalog[0]!, id: "retained-id", projectId: "explicit-project", workstream: "operator-mapping" };
  const historical = { ...current, id: "historical", path: "/retained/historical.md", expectedSha256: "a".repeat(64), revisionKind: "saved draft" as const };
  const report = await discoverPlanCatalog([root], [historical, current], exporter);
  expect(report.catalog.map(source => source.id)).toEqual(["historical", "retained-id", discovered.catalog[1]!.id]);
  expect(report.catalog[0]).toEqual(historical);
  expect(report.catalog[1]).toMatchObject({ id: current.id, projectId: "explicit-project", workstream: "operator-mapping" });
  expect(report.catalog[2]).toMatchObject({ workstream: null, epic: null, projectId: null });
  expect(report.diagnostics.join()).toContain("retained explicit catalog workstream");
});
