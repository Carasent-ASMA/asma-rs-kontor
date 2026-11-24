import { createHash } from "node:crypto";
import { readdir, readFile } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import type { z } from "zod";
import type { PaseoApi } from "@getpaseo/client";
import { summaryHistoryBoundarySchema } from "../shared/session-navigator";
import { allPages } from "./session-create";
import { savedRevisions } from "./session-summaries";
import { summaryJobSchema } from "./stock-summary-generator";

type Job = z.infer<typeof summaryJobSchema>;
const HELPER_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

export interface SummaryContainerRow {
  workspaceId: string;
  projectId: string;
  directory: string;
  projectRoot: string;
  helperId: string;
  job: string | null;
  jobState: Job["state"] | "unknown";
  cleaned: boolean | null;
  summarySaved: boolean;
  occupants: Array<{ agentId: string; own: boolean; archived: boolean }> | null;
  terminals: number | null;
  holds: string[];
  closeout: string | null;
}

const same = (a: object | undefined, b: object, keys: string[]) => !!a && keys.every(key => (a as Record<string, unknown>)[key] === (b as Record<string, unknown>)[key]);

/**
 * Whether the job's own result is saved: a revision of the capture's exact source with the complete captured history
 * boundary (epoch, sequence range, input hash, provider session), the capture's cutoff and the job's sections. The
 * capture must be this job's, named by its key. A matching input hash alone is not proof, and any read doubt is false.
 */
async function savedFor(root: string, jobsDir: string, jobFile: string, job: Job): Promise<boolean> {
  try {
    const capture = JSON.parse(await readFile(join(jobsDir, jobFile.replace(/\.json$/, ".capture.json")), "utf8"));
    const source = capture?.sourceAgentId, sections = job.sections;
    if (typeof source !== "string" || typeof capture.cutoff !== "string" || !sections) return false;
    if (`${createHash("sha256").update(JSON.stringify([source, capture.historyBoundary])).digest("hex")}.json` !== jobFile) return false;
    const boundary = summaryHistoryBoundarySchema.parse(capture.historyBoundary);
    return (await savedRevisions(root, source)).some(revision => revision.sourceAgentId === source && revision.cutoff === capture.cutoff
      && same(revision.historyBoundary, boundary, Object.keys(summaryHistoryBoundarySchema.shape)) && same(revision.sections, sections, Object.keys(sections)));
  } catch { return false; }
}

/** The runner's only accepted endpoint: a decimal port in 1..65535 on loopback, normalised for the URL and `--host`. */
export function reconcileEndpoint(port: string | undefined): { url: string; host: string } {
  const value = /^[1-9][0-9]{0,4}$/.test(port ?? "") ? Number(port) : 0;
  if (value < 1 || value > 65535) throw new Error("Usage: bun scripts/summary-container-reconcile.ts <daemon-port> [summary-state-root]; the port is a decimal integer from 1 to 65535.");
  return { url: `ws://127.0.0.1:${value}/ws`, host: `127.0.0.1:${value}` };
}

/**
 * Read-only dry run over the legacy per-job summary containers: active workspaces whose directory is
 * `<root>/generation-jobs/<helper id>`. Each row carries its proof, and `closeout` names the supported command only when
 * every proof holds: a complete job with cleaned=true, a saved revision of its capture, the helper's own readback bound
 * to this exact workspace and project and archived there, no other occupant and no terminal. A directory match only
 * finds occupants; it never proves ownership, so a sibling workspace on the same directory is held. A job recording a
 * designated placement (`owned: false`) and waiting, creating, failed and unknown jobs are never closed out, and
 * nothing here runs a command; the closeout is a separate gated step.
 */
export async function reconcileSummaryContainers(paseo: PaseoApi, root: string, host: string | null = null) {
  const jobsDir = join(root, "generation-jobs");
  const jobs: Array<{ file: string; job: Job }> = [];
  for (const file of (await readdir(jobsDir).catch(() => [] as string[])).filter(name => name.endsWith(".json") && !name.endsWith(".capture.json")).sort()) {
    // An unreadable record names no helper, so its container stays unknown and held.
    try { jobs.push({ file, job: summaryJobSchema.parse(JSON.parse(await readFile(join(jobsDir, file), "utf8"))) }); } catch { /* held as unknown */ }
  }
  const workspaces = await allPages(cursor => paseo.workspaces.list({ includeBackground: true, page: { limit: 200, ...(cursor ? { cursor } : {}) } }));
  const rows: SummaryContainerRow[] = [];
  for (const workspace of workspaces) {
    const directory = workspace.workspaceDirectory;
    if (!directory || dirname(directory) !== jobsDir) continue;
    const helperId = basename(directory), holds: string[] = [];
    const owner = jobs.find(({ job }) => job.workerId === helperId || job.previousWorkers.includes(helperId));
    const job = owner?.job ?? null;
    if (!HELPER_ID.test(helperId)) holds.push("The directory is not a helper id.");
    if (!job) holds.push("No job record names this helper (unknown).");
    else {
      if (job.state !== "complete" || !job.sections) holds.push(`The job is ${job.state}; only a complete job is closed out.`);
      if (job.cleaned !== true) holds.push("The job has no confirmed helper archive.");
    }
    const summarySaved = owner ? await savedFor(root, jobsDir, owner.file, owner.job) : false;
    if (job && !summarySaved) holds.push("No saved summary revision matches the job's capture.");
    if (job?.placement) holds.push("The job records a designated workspace it does not own (owned: false).");
    else if (job) {
      let bound = false;
      try {
        const read = await paseo.agents.ref(helperId).refresh();
        bound = read?.agent.id === helperId && read.agent.workspaceId === workspace.id && read.agent.cwd === directory && read.project?.projectKey === workspace.projectId;
      } catch { /* unproven: held below */ }
      if (!bound) holds.push("The helper's own readback does not bind this workspace and project.");
    }
    let occupants: SummaryContainerRow["occupants"] = null;
    try {
      const listed = await allPages(cursor => paseo.agents.list({ filter: { projectKeys: [workspace.projectId], includeArchived: true, includeBackground: true }, page: { limit: 200, ...(cursor ? { cursor } : {}) } }));
      occupants = listed.map(entry => entry.agent).filter(agent => agent.workspaceId === workspace.id || agent.cwd === directory)
        .map(agent => ({ agentId: agent.id, own: agent.id === helperId && agent.workspaceId === workspace.id, archived: !!agent.archivedAt }));
    } catch { holds.push("Its occupants could not be read."); }
    if (occupants) {
      if (!occupants.some(occupant => occupant.own)) holds.push("Its helper does not resolve in this container.");
      for (const occupant of occupants) {
        if (!occupant.own) holds.push(`Foreign occupant ${occupant.agentId}.`);
        else if (!occupant.archived) holds.push(`Helper ${occupant.agentId} is not archived.`);
      }
    }
    let terminals: number | null = null;
    try { terminals = (await paseo.workspaces.ref(workspace.id).terminals.list()).entries.length; } catch { holds.push("Its terminals could not be read."); }
    if (terminals) holds.push(`${terminals} terminal(s) open.`);
    rows.push({ workspaceId: workspace.id, projectId: workspace.projectId, directory, projectRoot: workspace.projectRootPath, helperId,
      job: owner?.file ?? null, jobState: job?.state ?? "unknown", cleaned: job?.cleaned ?? null, summarySaved, occupants, terminals, holds,
      closeout: holds.length ? null : `paseo workspace archive ${workspace.id} --json${host ? ` --host ${host}` : ""}` });
  }
  return {
    dryRun: true as const, executed: false as const, root, containers: rows.length,
    closeout: rows.filter(row => row.closeout).length, held: rows.filter(row => !row.closeout).length,
    projectResidual: "Archiving a container leaves its per-job project. The only project removal command, `paseo project delete`, deletes and is not proposed.",
    rows,
  };
}
