import { readdir, readFile } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import type { z } from "zod";
import type { PaseoApi } from "@getpaseo/client";
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

async function savedFor(root: string, jobsDir: string, jobFile: string): Promise<boolean> {
  try {
    const capture = JSON.parse(await readFile(join(jobsDir, jobFile.replace(/\.json$/, ".capture.json")), "utf8"));
    const hash = capture?.historyBoundary?.snapshotHash;
    return typeof hash === "string" && typeof capture.sourceAgentId === "string"
      && (await savedRevisions(root, capture.sourceAgentId)).some(revision => revision.historyBoundary?.snapshotHash === hash);
  } catch { return false; }
}

/**
 * Read-only dry run over the legacy per-job summary containers: active workspaces whose directory is
 * `<root>/generation-jobs/<helper id>`. Each row carries its proof, and `closeout` names the supported command only when
 * every proof holds: a complete job with cleaned=true, a saved revision of its capture, the helper itself archived in
 * the container, no other occupant and no terminal. Waiting, creating, failed and unknown jobs are never closed out,
 * and nothing here runs a command; the closeout is a separate gated step.
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
    const summarySaved = owner ? await savedFor(root, jobsDir, owner.file) : false;
    if (job && !summarySaved) holds.push("No saved summary revision matches the job's capture.");
    let occupants: SummaryContainerRow["occupants"] = null;
    try {
      const listed = await allPages(cursor => paseo.agents.list({ filter: { projectKeys: [workspace.projectId], includeArchived: true, includeBackground: true }, page: { limit: 200, ...(cursor ? { cursor } : {}) } }));
      occupants = listed.map(entry => entry.agent).filter(agent => agent.workspaceId === workspace.id || agent.cwd === directory)
        .map(agent => ({ agentId: agent.id, own: agent.id === helperId, archived: !!agent.archivedAt }));
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
