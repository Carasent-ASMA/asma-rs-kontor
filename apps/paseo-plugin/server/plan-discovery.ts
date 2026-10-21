import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { realpath } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";
import { isMap, parseDocument } from "yaml";
import type { ReadSourceSummary } from "../shared/read-sources";
import { boundedRead, readConfiguredSource, sourceCatalog, type ExportPlan } from "./read-sources";

const execute = promisify(execFile);
const GRAPH_FENCE = /^ {0,3}(?:`{3,}|~{3,})plan-graph\s*$/m;
type PlanCoverage = { path: string; id: string; epic: string | null; revision: string | null; tasks: number | null; boundTasks: number | null;
  status: "eligible" | "unlinked" | "invalid"; diagnostics: string[] };

function planWorkstream(content: string): { workstream: string | null; diagnostics: string[] } {
  const unknown = (reason: string) => ({ workstream: null, diagnostics: [`${reason}; plan workstream remains unknown.`] });
  const opening = /^\uFEFF?---[ \t]*\r?\n/.exec(content);
  if (!opening) return { workstream: null, diagnostics: [] };
  const rest = content.slice(opening[0].length);
  const closing = /^(?:---|\.\.\.)[ \t]*\r?$/m.exec(rest);
  if (!closing) return unknown("Unterminated frontmatter");
  const header = parseDocument(rest.slice(0, closing.index));
  if (header.errors.length || header.warnings.length || !isMap(header.contents)) return unknown("Unsupported or invalid frontmatter");
  // Read only the explicit top-level scalar, without resolving aliases or merge keys.
  const value = header.get("workstream");
  if (value === undefined || value === null) return { workstream: null, diagnostics: [] };
  if (typeof value !== "string" || !/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(value)) return unknown("Unsupported workstream declaration");
  return { workstream: value, diagnostics: [] };
}

/** Offline discovery in one operator-selected checkout. Titles and paths never supply epic identity. */
export async function discoverPlanSources(checkout: string, exporter?: ExportPlan) {
  const root = await realpath(checkout);
  const git = async (args: string[]) => (await execute("git", ["-C", root, ...args], { maxBuffer: 4 * 1024 * 1024, timeout: 15_000 })).stdout;
  const head = (await git(["rev-parse", "HEAD"])).trim();
  const paths = (await git(["ls-files", "-z", "--", "*.md"])).split("\0").filter((path) => path && !path.split("/").some((part) => ["skills", "node_modules"].includes(part))).sort();
  const catalog: ReadSourceSummary[] = [];
  const coverage: PlanCoverage[] = [];
  for (const relative of paths) {
    const path = join(root, relative);
    let content: string;
    try { content = await boundedRead(path); }
    catch (error) {
      // Missing documentation unrelated to a committed plan is outside this report.
      const committed = await git(["show", `HEAD:${relative}`]);
      if (!GRAPH_FENCE.test(committed)) continue;
      coverage.push({ path: relative, id: "", epic: null, revision: null, tasks: null, boundTasks: null, status: "invalid", diagnostics: [error instanceof Error ? error.message : String(error)] });
      continue;
    }
    if (!GRAPH_FENCE.test(content)) continue;
    const id = `plan-${createHash("sha256").update(relative).digest("hex").slice(0, 24)}`;
    const title = /^#\s+(.+)$/m.exec(content)?.[1]?.trim() || relative;
    const source: ReadSourceSummary = { id, title, kind: "plan", path, workstream: null, epic: null, projectId: null, revisionKind: "working copy" };
    const read = await readConfiguredSource(source, exporter);
    const metadata = planWorkstream(read.content ?? "");
    const epic = read.graph?.epic.jira ?? null;
    const eligible = epic !== null && /^ASMA-[1-9]\d*$/.test(epic);
    coverage.push({ path: relative, id, epic, revision: read.revision, tasks: read.graph?.nodes.length ?? null,
      boundTasks: read.graph?.nodes.filter((node) => node.jira !== null).length ?? null,
      status: read.status !== "ready" ? "invalid" : eligible ? "eligible" : "unlinked", diagnostics: [...read.diagnostics, ...metadata.diagnostics] });
    if (eligible && read.status === "ready") catalog.push({ ...source, epic, workstream: metadata.workstream });
  }
  const validated = sourceCatalog(JSON.stringify(catalog));
  if (validated.diagnostics.length) throw new Error(validated.diagnostics.join(" "));
  const conflicts = [...new Set(catalog.map((source) => source.epic))].filter((epic) => catalog.filter((source) => source.epic === epic).length > 1);
  return { root, head, catalog: validated.sources, coverage, conflicts,
    diagnostics: conflicts.map((epic) => `${epic}: multiple canonical plans; retain explicit source choice.`) };
}
