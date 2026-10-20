import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { mkdir, mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import { discoverPlanSources } from "../server/plan-discovery";

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
  it("uses validated canonical bindings, excludes policy examples/untracked plans, and preserves ambiguous sources", async () => {
    const root = await checkout({ "plans/a.md": plan("ASMA-1"), "plans/b.md": plan("ASMA-1"), "plans/unlinked.md": plan(null), "plans/ASMA-2.md": "# ASMA-2\nNo graph", ".agents/skills/example.md": plan("ASMA-3") });
    await writeFile(join(root, "untracked.md"), plan("ASMA-4"));
    const report = await discoverPlanSources(root, exporter);
    expect(report.catalog.map((source) => source.epic)).toEqual(["ASMA-1", "ASMA-1"]);
    expect(report.conflicts).toEqual(["ASMA-1"]);
    expect(report.coverage.map((row) => row.status)).toEqual(["eligible", "eligible", "unlinked"]);
    expect(new Set(report.catalog.map((source) => source.id)).size).toBe(2);
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
    expect(missing.catalog).toEqual([]);
    expect(missing.coverage.every((row) => row.status === "invalid")).toBe(true);
  });
});
