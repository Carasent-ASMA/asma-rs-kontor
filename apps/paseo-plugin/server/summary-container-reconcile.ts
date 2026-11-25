import { createHash, randomUUID } from "node:crypto";
import { open, readdir, readFile, rename, unlink } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import type { z } from "zod";
import type { PaseoApi } from "@getpaseo/client";
import { summaryHistoryBoundarySchema } from "../shared/session-navigator";
import { allPages } from "./session-create";
import { savedRevisions } from "./session-summaries";
import { archiveExactBytes, summaryJobSchema } from "./stock-summary-generator";

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
export const RECONCILE_USAGE = "Usage: bun scripts/summary-container-reconcile.ts <daemon-port> [summary-state-root], or <daemon-port> <summary-state-root> record-recovery <job-file> retire-legacy-create <evidence> [--apply <before-sha256 from the dry run>]; the port is a decimal integer from 1 to 65535. Only a creating or waiting job with no recovery yet, whose helper does not read back by its exact id, is accepted. The evidence is the operator's reference; it is checked as non-blank, never verified, and is not native evidence. For a job with a placement, it must show that its keyed create is terminal (the installer's receipt triage). A job whose helper reads back takes no recovery and has no supported release: Paseo's readback cannot show that its original work has stopped (AUD-A-R6-01).";
export function reconcileEndpoint(port: string | undefined): { url: string; host: string } {
  const value = /^[1-9][0-9]{0,4}$/.test(port ?? "") ? Number(port) : 0;
  if (value < 1 || value > 65535) throw new Error(RECONCILE_USAGE);
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

const sha256 = (bytes: Buffer | string) => createHash("sha256").update(bytes).digest("hex");

/** The recovery operands that need no host or file: checked by the runner before any client exists, and again here. */
export function recoveryOperands(input: { job?: string; disposition?: string; evidence?: string }) {
  const job = input.job ?? "", evidence = (input.evidence ?? "").trim();
  if (!/^[a-f0-9]{64}\.json$/.test(job)) throw new Error("Name one job file in generation-jobs: 64 hexadecimal characters and .json. Nothing was changed.");
  if (input.disposition !== "retire-legacy-create") throw new Error('The only recovery disposition is "retire-legacy-create". Nothing was changed.');
  if (!evidence) throw new Error("A recovery needs a non-blank evidence reference. Nothing was changed.");
  return { job, disposition: input.disposition, evidence };
}

/**
 * Record an operator's recovery disposition on one held summary job, the installer's supported route instead of a hand
 * edit. It is a dry run unless `applySha256` names the bytes its dry run reviewed; other bytes stop the job unchanged.
 * Only a `creating` or `waiting` job that records no recovery yet and whose helper does not read back by its exact id is
 * accepted: the job the generator holds (legacy, former-key, waiting, or a create the host could not confirm). A job whose
 * helper reads back, `closed` or not, is refused and has no supported release (AUD-A-R6-01). The evidence is the operator's
 * reference: it must be non-blank, and this does not verify it; it is not native evidence. For a job with a placement it
 * must show that its keyed create is terminal; the host receipt behind it is the installer's own triage, not read here.
 * An apply holds `<job>.lock`, created exclusively, from its final read to its replace, so concurrent applies to one job
 * are serialised: inside the lock the job must still be exactly the reviewed bytes, which carry no recovery. The exact
 * bytes are kept once at `superseded/<worker id>.before-recovery.json`; then the record, with every value it had and only
 * `recovery: { disposition, evidence, recordedAt }` added, replaces it atomically. The same disposition and evidence again
 * is a no-op. It never deletes a job, resends, or touches the host beyond reading the helper.
 */
export async function recordSummaryRecovery(paseo: Pick<PaseoApi, "agents">, root: string,
  input: { job: string; disposition: string; evidence: string; applySha256?: string; now?: () => string }) {
  const { evidence } = recoveryOperands(input);
  const jobsDir = join(root, "generation-jobs"), file = join(jobsDir, input.job);
  const raw = await readFile(file);
  const record = JSON.parse(raw.toString("utf8")) as Record<string, unknown>;
  const job = summaryJobSchema.parse(record);
  const plan = { job: input.job, workerId: job.workerId, beforeSha256: sha256(raw), archive: `superseded/${job.workerId}.before-recovery.json` };
  if (job.state !== "creating" && job.state !== "waiting") throw new Error(`Job ${input.job} is ${job.state}; only a held creating or waiting job takes a recovery. Nothing was changed.`);
  if (job.recovery) {
    if (job.recovery.disposition === input.disposition && job.recovery.evidence === evidence) return { ...plan, recovery: job.recovery, applied: false, unchanged: true };
    throw new Error(`Job ${input.job} already records another recovery. It is kept unchanged.`);
  }
  if (await paseo.agents.ref(job.workerId).refresh()) throw new Error(`Summary helper ${job.workerId} reads back on this host, so a recovery does not apply. Paseo's readback cannot show that its original work has stopped (AUD-A-R6-01): the generator reads a correlated result from it or holds the job, and no supported route releases it. Nothing was changed.`);
  const recovery = { disposition: input.disposition, evidence, recordedAt: (input.now ?? (() => new Date().toISOString()))() };
  const next = JSON.stringify({ ...record, recovery });
  if (input.applySha256 === undefined) return { ...plan, recovery, applied: false, unchanged: false };
  if (input.applySha256 !== plan.beforeSha256) throw new Error(`Job ${input.job} is not the bytes its dry run reviewed (now ${plan.beforeSha256}). Nothing was changed; dry-run it again.`);
  const lockFile = `${file}.lock`;
  const lock = await open(lockFile, "wx", 0o600).catch((error: unknown) => {
    if (error instanceof Error && "code" in error && error.code === "EEXIST") throw new Error(`Another recovery apply holds job ${input.job} (${basename(lockFile)}). Nothing was changed; if no apply is running, remove that lock and dry-run again.`);
    throw error;
  });
  try {
    await lock.writeFile(`${process.pid}\n`);
    const now = await readFile(file);
    if (sha256(now) !== input.applySha256) throw new Error(`Job ${input.job} changed while this apply ran (now ${sha256(now)}). Nothing was changed; dry-run it again.`);
    await archiveExactBytes(join(jobsDir, "superseded"), `${job.workerId}.before-recovery.json`, raw);
    const temporary = `${file}.${randomUUID()}.tmp`;
    const handle = await open(temporary, "wx", 0o600);
    try { await handle.writeFile(next); await handle.sync(); } finally { await handle.close(); }
    await rename(temporary, file);
  } finally { await lock.close(); await unlink(lockFile); }
  return { ...plan, recovery, applied: true, unchanged: false, afterSha256: sha256(next) };
}
