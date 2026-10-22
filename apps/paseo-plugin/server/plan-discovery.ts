import { execFile } from "node:child_process";
import { createHash } from "node:crypto";
import { realpath } from "node:fs/promises";
import { join } from "node:path";
import { promisify } from "node:util";
import { GRAPH_FENCE, planMetadata } from "./plan-metadata";
import type { ReadSourceSummary } from "../shared/read-sources";
import { boundedRead, readConfiguredSource, sourceCatalog, type ExportPlan } from "./read-sources";

const execute = promisify(execFile);
type PlanCoverage = { path: string; id: string; epic: string | null; revision: string | null; tasks: number | null; boundTasks: number | null;
  status: "eligible" | "unlinked" | "invalid"; diagnostics: string[] };

/** Offline discovery in one operator-selected checkout. Titles and paths never supply epic identity. */
export async function discoverPlanSources(checkout: string, exporter?: ExportPlan) {
  const root = await realpath(checkout);
  const git = async (args: string[]) => (await execute("git", ["-C", root, ...args], { maxBuffer: 4 * 1024 * 1024, timeout: 15_000 })).stdout;
  const head = (await git(["rev-parse", "HEAD"])).trim();
  const paths = (await git(["ls-files", "-z", "--", "*.md"])).split("\0").filter((path) => path && !path.split("/").some((part) => ["skills", "node_modules"].includes(part))).sort();
  const repository = await realpath((await git(["rev-parse", "--git-common-dir"])).trim().startsWith("/") ? (await git(["rev-parse", "--git-common-dir"])).trim() : join(root, (await git(["rev-parse", "--git-common-dir"])).trim()));
  const catalog: ReadSourceSummary[] = [];
  const coverage: PlanCoverage[] = [];
  for (const relative of paths) {
    if (!/^(?:_docs|plan|plans)\//.test(relative) || /(?:^|\/)(?:audits?|reports?|history|evidence|reference)\//.test(relative) || /(?:adoption-record|(?:^|-)router)\.md$/i.test(relative)) continue;
    if (/(^|\/)(?:README|index)\.md$/i.test(relative)) continue;
    const primaryPlan = /(^|\/)(?:plan|plans)\//.test(relative) || /(?:^|[-_])plan(?:[-_.]|$)/i.test(relative.split("/").pop() ?? "");
    const path = join(root, relative);
    let content: string;
    try { content = await boundedRead(path); }
    catch (error) {
      // Missing documentation unrelated to a committed plan is outside this report.
      const committed = await git(["show", `HEAD:${relative}`]);
      if (!GRAPH_FENCE.test(committed) && !primaryPlan) continue;
      coverage.push({ path: relative, id: "", epic: null, revision: null, tasks: null, boundTasks: null, status: "invalid", diagnostics: [error instanceof Error ? error.message : String(error)] });
      continue;
    }
    if (!GRAPH_FENCE.test(content) && !primaryPlan) continue;
    const id = `plan-${createHash("sha256").update(relative).digest("hex").slice(0, 24)}`;
    const title = /^#\s+(.+)$/m.exec(content)?.[1]?.trim() || relative;
    const source: ReadSourceSummary = { id, title, kind: "plan", path, workstream: null, epic: null, projectId: null, revisionKind: "working copy", logicalPlanId: createHash("sha256").update(`${repository}:${relative}`).digest("hex") };
    const read = await readConfiguredSource(source, exporter);
    const metadata = read.planMetadata ?? planMetadata(read.content ?? "");
    const epic = read.graph?.epic.jira ?? metadata.epic;
    const eligible = epic !== null && /^ASMA-[1-9]\d*$/.test(epic);
    coverage.push({ path: relative, id, epic, revision: read.revision, tasks: read.graph?.nodes.length ?? null,
      boundTasks: read.graph?.nodes.filter((node) => node.jira !== null).length ?? null,
      status: read.status !== "ready" ? "invalid" : eligible ? "eligible" : "unlinked", diagnostics: read.diagnostics });
    if (read.content !== null) catalog.push({ ...source, epic, workstream: metadata.workstream });
  }
  const validated = sourceCatalog(JSON.stringify(catalog));
  if (validated.diagnostics.length) throw new Error(validated.diagnostics.join(" "));
  const conflicts = [...new Set(catalog.map((source) => source.epic).filter((epic): epic is string => epic !== null))].filter((epic) => catalog.filter((source) => source.epic === epic).length > 1);
  return { root, head, catalog: validated.sources, coverage, conflicts,
    diagnostics: conflicts.map((epic) => `${epic}: multiple canonical plans; retain explicit source choice.`) };
}

/** Explicit roots only. Same repository/path/revision copies collapse; differing revisions remain choices. */
export async function discoverPlanCatalog(checkouts: readonly string[], existing: readonly ReadSourceSummary[] = [], exporter?: ExportPlan) {
  if (!checkouts.length || checkouts.length > 16) throw new Error("Select between 1 and 16 documentation checkouts");
  const reports = [];
  for (const root of [...new Set(checkouts)]) reports.push(await discoverPlanSources(root, exporter));
  const catalog = [...existing];
  const seen = new Map<string, ReadSourceSummary>();
  const diagnostics = reports.flatMap((report) => report.diagnostics);
  for (const report of reports) for (const candidate of report.catalog) {
    const revision = report.coverage.find((row) => row.id === candidate.id)!.revision;
    const identity = `${candidate.logicalPlanId}:${revision}`;
    const match = catalog.findIndex((source) => source.path === candidate.path && !source.expectedSha256);
    if (match >= 0) {
      const previous = catalog[match]!;
      if (previous.workstream && candidate.workstream && previous.workstream !== candidate.workstream) diagnostics.push(`${previous.id}: retained explicit catalog workstream; declaration conflicts.`);
      if (previous.epic && candidate.epic && previous.epic !== candidate.epic) diagnostics.push(`${previous.id}: retained explicit catalog epic; declaration conflicts.`);
      catalog[match] = { ...candidate, id: previous.id, projectId: previous.projectId, revisionKind: previous.revisionKind,
        workstream: previous.workstream ?? candidate.workstream, epic: previous.epic ?? candidate.epic };
      seen.set(identity, catalog[match]!); continue;
    }
    if (seen.has(identity) || catalog.some((source) => (source.logicalPlanId === candidate.logicalPlanId || source.path === candidate.path) && source.expectedSha256 === revision)) continue;
    const id = catalog.some((source) => source.id === candidate.id) ? `${candidate.id}-${revision!.slice(0, 12)}` : candidate.id;
    const next = { ...candidate, id };
    catalog.push(next); seen.set(identity, next);
  }
  const validated = sourceCatalog(JSON.stringify(catalog));
  if (validated.diagnostics.length) throw new Error(validated.diagnostics.join(" "));
  return { catalog: validated.sources, reports, diagnostics };
}
