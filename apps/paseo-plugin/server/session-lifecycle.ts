import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, readdir, readFile, rename, unlink, writeFile } from "node:fs/promises";
import { basename, dirname, join } from "node:path";
import { z } from "zod";
import type { PaseoApi, PaseoAgentListResult } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readConfig } from "./config";
import {
  TAKEOVER_LIMIT, RESUME_PROMPT, ARCHIVE_BATCH_LIMIT, archiveOrder, lifecycleReviewSchema, lifecycleRowSchema, lifecycleCapabilities, unresolvedArchive,
  previewLifecycle, executeLifecycleItem, reconcileLifecycleItem, readLifecycleReview, probeSessionAvailability, readSessionDispositions,
  type LifecycleReview, type LifecycleRow, type RecoveryTarget, type SessionAvailability, type UnresolvedRef,
} from "../shared/session-lifecycle";
import {
  RECOVERY_TAIL_ITEMS, observeRefusal, quietAfterRefusal, readProviderRefusal, recoveredAfterRefusal, refusalKind, refusalObservationSchema, refusalState,
  type ProviderRefusal, type ProviderRefusalRead, type RefusalObservation,
} from "../shared/provider-block";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
export type LifecycleClient = {
  agents: { list(options?: Parameters<PaseoApi["agents"]["list"]>[0]): Promise<PaseoAgentListResult>; ref(id: string): Pick<ReturnType<PaseoApi["agents"]["ref"]>, "refresh" | "send" | "archive"> & {
    timeline: { refetch(options?: { direction?: "tail"; limit?: number }): Promise<{ entries: readonly unknown[]; error?: string | null }> };
  } };
  providers: Pick<PaseoApi["providers"], "snapshot">;
};
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);
const hash = (value: string) => createHash("sha256").update(value).digest("hex");
const archiveIntentReferenceSchema = z.object({ agentId: z.string(), action: z.literal("archive"), reviewId: z.string().uuid() });
const fenceSchema = z.object({ reviewId: z.string(), agentId: z.string(), action: z.enum(["resume", "archive"]), phase: z.string(), fingerprint: z.string() });
/** Written only by the pre-runtime-split success check; kept solely to recognise its fences (G3). */
const LEGACY_PREDICATE_MISS = "Archive returned but exact-ID terminal retirement readback is unavailable. Check native outcome; do not retry the request.";
/** Only the old success branch saved this text, and only after archive() resolved and a read showed archivedAt. */
const legacySuccess = (row: LifecycleRow) => row.outcome === "uncertain" && row.message === LEGACY_PREDICATE_MISS && !!row.availability?.archivedAt;
const UNRESOLVED_LIMIT = 100;
// Compare-and-swap: every fence phase write, its paired result save and every release run only while the fence
// still holds the exact bytes this call read or wrote (every field, including phase and the per-execute token), so a
// fence written after that (a successor Resume or Archive) is never overwritten or removed. One step per path at a time.
// ponytail: an in-process queue; one plugin worker owns the root. Cross-process ownership would need a lock directory.
const fenceSteps = new Map<string, Promise<unknown>>();
function queued<T>(path: string, step: () => Promise<T>): Promise<T> {
  const next = (fenceSteps.get(path) ?? Promise.resolve()).then(step);
  fenceSteps.set(path, next.catch(() => {}));
  return next;
}
function whileOwned(path: string, expected: string, step: () => Promise<unknown>): Promise<boolean> {
  return queued(path, async () => {
    const current = await readFile(path, "utf8").catch((error) => { if (error?.code === "ENOENT") return null; throw error; });
    if (current !== expected) return false;
    await step(); return true;
  });
}
const release = (path: string, expected: string) => whileOwned(path, expected, () => unlink(path));
// Refusal memory, write-ahead and per ID: every Resume-path exact read first publishes its attempt, then its outcome, as
// one snapshot with a per-ID sequence. Order is that sequence, never a clock: the record, its held fallback and a complete
// temp are read back by it. This worker's memory is the newest state (one writer per root, as above); on first use it
// adopts the disk.
const memorySchema = z.object({
  seq: z.number().int().nonnegative(),
  /** The open refusal, named by the newest read (its attempt seq) that saw it; the binding keeps the latest completion.
   *  Its kind is `unknown` while the memory is unresolved, and only a later successful provider turn releases it. */
  refusal: refusalObservationSchema.extend({ seq: z.number().int().nonnegative() }).nullable(),
  /** Attempts with no recorded outcome: each one's seq, the snapshot seq it was opened on (`revision`) and its start. */
  open: z.array(z.object({ seq: z.number().int().positive(), revision: z.number().int().nonnegative(), start: z.iso.datetime() }).strict()),
  /** A clean read or a recovery is durably recorded. Absence is never clean. */
  clean: z.boolean(),
  /** The final read of the last recovery: a read that started before it cannot reopen what it released. */
  cleared: z.number().int().nonnegative(),
  /** What the last recovery released, recorded before it took effect. */
  recovered: refusalObservationSchema.nullable(),
});
type RefusalMemory = z.infer<typeof memorySchema>;
type OpenRefusal = NonNullable<RefusalMemory["refusal"]>;
/** One ID's snapshot, consistent with the state it names: an attempt holds an open read, a refusal holds one, a clean
 *  record holds none and no open read, a recovery records what it released and its final read. */
const snapshotSchema = (id: string) => memorySchema.extend({ agentId: z.literal(id), state: z.enum(["attempt", "clean", "refusal", "recovery"]) }).strict()
  .refine(({ state, seq, refusal, open, clean, cleared, recovered }) => (refusal?.seq ?? 0) <= seq && open.every((entry) => entry.seq <= seq) && cleared <= seq
    && (cleared > 0) === !!recovered && (!recovered || clean)
    && (state === "attempt" ? open.length > 0 : state === "refusal" ? !!refusal : !refusal && clean && (state === "clean" ? !open.length : !!recovered)));
type Snapshot = { state: string; memory: RefusalMemory };
function snapshotOf(id: string, bytes: string): Snapshot | null {
  try { const { agentId: _id, state, ...memory } = snapshotSchema(id).parse(JSON.parse(bytes)); return { state, memory }; } catch { return null; }
}
const memories = new Map<string, RefusalMemory>();
// Per refusal path: held records, temps and adopted artifacts the memory already includes. They go only after a durable
// record or hold replaces them.
const covered = new Map<string, Set<string>>();
const sweep = async (files: Set<string>, keep: string | null) => {
  for (const file of [...files]) if (file !== keep) await unlink(file).then(() => files.delete(file), (error) => { if (error?.code === "ENOENT") files.delete(file); });
};
const later = (a: string | undefined, b: string) => a && Date.parse(a) > Date.parse(b) ? a : b;
/** Joins two observations of one ID: `names` decides whose refusal it is; the binding keeps the latest completion. */
const joinRefusal = (base: RefusalObservation | null, next: RefusalObservation, names: boolean): RefusalObservation =>
  ({ ...(names || !base ? next : base), observedAt: later(base?.observedAt, next.observedAt) });
const plain = ({ seq: _seq, ...observation }: OpenRefusal): RefusalObservation => observation;
const unknownRefusal = (observedAt: string, message: string): RefusalObservation => ({ kind: "unknown", failureAt: "unknown", observedAt, route: { provider: "unknown", model: null }, message });
const NO_CLEAN = "No clean read of this session is stored, so a native error that is not a classified provider refusal is unresolved.";
const UNRESOLVED = "An earlier read of this session left no recorded outcome, or a stored refusal record is partial, corrupt or foreign.";
const PENDING = "Another exact read of this session has not recorded its outcome yet.";
/** An open read later than the last release: until it records its outcome, the memory is not clean. */
const pending = (memory: RefusalMemory, after = memory.cleared) => memory.open.some((entry) => entry.seq > after);
const bound = (next: { observedAt: string }, prior: { observedAt: string }) => Date.parse(next.observedAt) >= Date.parse(prior.observedAt);
const identity = ({ observedAt: _at, seq: _seq, ...rest }: OpenRefusal) => JSON.stringify(rest);
/** Whether `memory` accounts for everything an older snapshot holds. Each read that snapshot left open is still open,
 *  named the open refusal or was the last recovery's final read. Its refusal is still open, named by the same read with
 *  the same evidence or by a later read, and bound no earlier; or a recovery whose final read started after that refusal
 *  was named released it, bound no earlier. */
const covers = (memory: RefusalMemory, older: RefusalMemory) => {
  const held = memory.refusal, refusal = older.refusal, recovered = memory.recovered;
  return older.open.every((entry) => memory.open.some((open) => open.seq === entry.seq) || held?.seq === entry.seq || memory.cleared === entry.seq)
    && (!refusal || (held ? (held.seq > refusal.seq || (held.seq === refusal.seq && identity(held) === identity(refusal))) && bound(held, refusal)
      : !!recovered && memory.cleared > refusal.seq && bound(recovered, refusal)));
};
/** An unresolved memory: an open refusal loses its kind, counts as named by read `seq` and binds no earlier than `at`. */
const unresolved = (memory: RefusalMemory, at: string, seq: number): OpenRefusal => memory.refusal
  ? { ...memory.refusal, kind: "unknown", seq: Math.max(memory.refusal.seq, seq), observedAt: later(memory.refusal.observedAt, at) } : { ...unknownRefusal(at, UNRESOLVED), seq };
type RefusalRead = { agent?: Agent; availability: SessionAvailability; attempt: number; seen: RefusalObservation | null; memory?: RefusalMemory; problem?: Error | null; fence?: Error };
export function lifecycleRevision(agent: Agent): string {
  return hash(JSON.stringify([agent.id, agent.status, agent.archivedAt, agent.updatedAt, agent.provider, agent.model,
    agent.cwd, agent.workspaceId, agent.currentModeId, agent.effectiveThinkingOptionId, agent.activeTurn,
    agent.pendingPermissions, agent.lastError, agent.labels, agent.persistence, agent.providerUnavailable,
    agent.runtimeInfo, agent.capabilities.supportsSessionPersistence, agent.attentionReason, parent(agent)]));
}
function parent(agent: Agent) { return ("parentAgentId" in agent && typeof agent.parentAgentId === "string" ? agent.parentAgentId : null) ?? agent.labels?.["paseo.parent-agent-id"] ?? null; }
// Archive deliberately stops current activity. Token/status/permission changes must not
// make a running session impossible to stop; identity and placement must still match.
function archiveRevision(agent: Agent): string {
  return hash(JSON.stringify([agent.id, agent.createdAt, agent.provider, agent.cwd, agent.workspaceId, agent.persistence, parent(agent)]));
}
// Paseo 0.12 persists archivedAt before closing the runtime and stores an archived running record as idle,
// so "closed" is runtime evidence, never the membership test.
function active(agent?: Agent): boolean {
  return !!agent && (!!agent.activeTurn || agent.status === "running" || agent.status === "initializing");
}
function archivedMembership(agent?: Agent): boolean {
  return !!agent?.archivedAt && !active(agent);
}
function childrenOf(agent: Agent, inventory: Agent[]): Agent[] {
  // Block every managed child, even one the native cascade would detach today.
  return inventory.filter((child) => child.archivedAt == null && parent(child) === agent.id);
}
// Resume is a same-ID retry on the session's own route, so no review target takes part in eligibility. A refusal an
// earlier exact read saw keeps Resume closed until positive recovery, whatever this read shows.
export function lifecycleRow(agent: Agent, inventory: Agent[], action: LifecycleReview["action"], refusal: ProviderRefusal | null = null): LifecycleRow {
  const children = childrenOf(agent, inventory).map((child) => ({ id: child.id, title: child.title ?? child.id, status: child.status, workspaceId: child.workspaceId ?? null }));
  const row: LifecycleRow = { id: agent.id, title: agent.title ?? agent.id, workspaceId: agent.workspaceId ?? null, cwd: agent.cwd,
    provider: agent.provider, model: agent.runtimeInfo?.model ?? agent.model ?? null,
    effort: agent.effectiveThinkingOptionId ?? agent.runtimeInfo?.thinkingOptionId ?? null,
    mode: agent.currentModeId ?? agent.runtimeInfo?.modeId ?? null, nativeStatus: agent.status,
    fingerprint: action === "archive" ? archiveRevision(agent) : lifecycleRevision(agent), category: "excluded", eligible: false, reason: "", children, outcome: "pending", message: null };
  const labels = agent.labels ?? {};
  if (agent.archivedAt != null) row.reason = "Already archived. This action never revives an archived session.";
  else if (action === "archive" && children.length) row.reason = "Archive blocked: native archive can cascade to these children. Preserve or finish them through the owning workflow first.";
  else if (action === "archive") { row.category = "candidate"; row.eligible = true; row.reason = "Archive stops this session and its provider tools, closes its runtime, and keeps durable history and files. No undo is exposed by this SDK."; }
  else if (labels["takeover.successor"] || labels["takeover.replaced-by"] || labels["recovery.successor"]) row.reason = "Already replaced. Follow the recorded successor; do not resume this predecessor.";
  else if (labels["task.status"] === "completed" || labels["task.status"] === "stopped" || agent.status === "closed") row.reason = "Closed, completed or intentionally stopped. No automatic failure is inferred.";
  else if (agent.pendingPermissions.length || agent.attentionReason === "permission") row.reason = "Waiting for permission/input. Resolve that wait in the native session.";
  else if (agent.activeTurn || agent.status === "running" || agent.status === "initializing") { row.category = "unknown"; row.reason = "Execution may still be progressing or waiting on a dependency. No supported stalled-execution proof; age is not evidence."; }
  else if (agent.status !== "error") { row.category = "unknown"; row.reason = "No current native failure. Idle, old timestamps and earlier error notices do not prove a stuck task."; }
  else if (refusalKind(agent)) { row.category = "candidate"; row.reason = "Quota/auth/provider restriction: no validated changed account or condition. Open native Accounts/session to resolve it, then review again. Resume alone is not a takeover or a quota fix."; }
  else if (refusal && refusal.recovery !== "confirmed") { row.category = "candidate"; row.reason = `Last provider refusal (${refusal.kind}) has no confirmed recovery: no later turn is shown to have completed. Resolve it in the native session or Accounts, then read it again.`; }
  else if (!agent.capabilities.supportsSessionPersistence || !agent.persistence?.sessionId || !row.mode || !agent.cwd) { row.category = "candidate"; row.reason = "Missing native history/persistence, permission mode or checkout evidence. This screen cannot fabricate a recovery brief."; }
  else { row.category = "candidate"; row.eligible = true; row.reason = `Current native error${agent.lastError ? `: ${agent.lastError}` : ""}. Retry in the same session with existing context and permissions; dispatch is not completion.`; }
  return row;
}

// Archive needs only the active children (childrenOf skips archived ones), so its walk never reads archived history.
async function inventory(client: LifecycleClient, action: LifecycleReview["action"]): Promise<Agent[]> {
  const agents = new Map<string, Agent>(), cursors = new Set<string>();
  let cursor: string | undefined;
  for (let page = 0; page < 1_000; page++) {
    const result = await client.agents.list({ filter: { includeArchived: action !== "archive" }, page: { limit: 200, ...(cursor ? { cursor } : {}) } });
    for (const { agent } of result.entries) agents.set(agent.id, agent);
    if (!result.pageInfo.hasMore) return [...agents.values()];
    const next = result.pageInfo.nextCursor;
    if (!next || cursors.has(next)) throw new Error("Incomplete native inventory; no lifecycle action is allowed.");
    cursors.add(next); cursor = next;
  }
  throw new Error("Native inventory exceeded the bounded page walk; no lifecycle action is allowed.");
}

/** Host-local durable intents. An interrupted/unknown dispatch is held, never replayed. */
export function createLifecycleService(client: LifecycleClient, root: string) {
  const ready = () => mkdir(root, { recursive: true, mode: 0o700 });
  const reviewPath = (id: string) => join(root, `${id}.json`);
  const resultPath = (reviewId: string, agentId: string) => join(root, `result-${reviewId}-${hash(agentId)}.json`);
  const removedPath = (agentId: string) => join(root, `local-absence-${hash(agentId)}.json`);
  const refusalPath = (agentId: string) => join(root, `refusal-${hash(agentId)}.json`);
  async function saveRow(reviewId: string, row: LifecycleRow) {
    const file = resultPath(reviewId, row.id), temporary = `${file}.${randomUUID()}.tmp`;
    const output = await open(temporary, "wx", 0o600);
    try { await output.writeFile(JSON.stringify(row)); await output.sync(); } finally { await output.close(); }
    await rename(temporary, file);
  }
  async function save(review: LifecycleReview) {
    await ready();
    const file = reviewPath(review.id), temporary = `${file}.${randomUUID()}.tmp`;
    const output = await open(temporary, "wx", 0o600);
    try { await output.writeFile(JSON.stringify(review)); await output.sync(); } finally { await output.close(); }
    await rename(temporary, file);
    const pointer = join(root, `latest-${randomUUID()}.tmp`);
    await writeFile(pointer, JSON.stringify({ id: review.id }), { mode: 0o600 }); await rename(pointer, join(root, "latest.json"));
  }
  async function read(id?: string): Promise<LifecycleReview | null> {
    await ready();
    if (!id) {
      let latest: { id: string } | undefined;
      try { latest = JSON.parse(await readFile(join(root, "latest.json"), "utf8")); }
      catch (error) { if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) throw error; }
      // The latest pointer moves with every review, so recovery comes from the durable fences instead.
      const unresolved = await unresolvedArchives();
      if (!latest) {
        if (unresolved.length) throw new Error(`${unresolved.length} archive fence(s) are not confirmed, but no review pointer exists. No fence was changed.`);
        return null;
      }
      const review = await read(latest.id);
      return review && { ...review, unresolved };
    }
    if (!/^[a-f0-9-]{36}$/.test(id)) throw new Error("Invalid review ID");
    const review = lifecycleReviewSchema.parse(JSON.parse(await readFile(reviewPath(id), "utf8")));
    review.rows = await Promise.all(review.rows.map(async (row) => {
      try { return lifecycleRowSchema.parse(JSON.parse(await readFile(resultPath(id, row.id), "utf8"))); }
      catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return row; throw error; }
    }));
    return review;
  }
  /** Metadata-only: local fences and their owner's merged result. A listed ref is a hint, never authority. */
  async function unresolvedArchives(): Promise<UnresolvedRef[]> {
    const refs: UnresolvedRef[] = [];
    let problems = 0;
    for (const name of (await readdir(root)).filter((entry) => /^intent-[a-f0-9]{64}\.json$/.test(entry)).sort()) {
      try {
        const lock = fenceSchema.parse(JSON.parse(await readFile(join(root, name), "utf8")));
        if (name !== `intent-${hash(lock.agentId)}.json` || !z.string().uuid().safeParse(lock.reviewId).success) { problems++; continue; }
        // Resume fences are not archive recovery work.
        if (lock.action !== "archive") continue;
        if (!["preflight", "intent", "uncertain", "unconfirmed", "archived", "blocked", "absent", "already_archived"].includes(lock.phase)) { problems++; continue; }
        // The owner's merged row decides for every phase, so an unreadable owner never reads as settled. Execute saves
        // that row before each fence write after preflight; only an in-flight preflight still has a pending row.
        const owner = await read(lock.reviewId), row = owner?.rows.find((entry) => entry.id === lock.agentId);
        if (!owner || owner.id !== lock.reviewId || owner.action !== "archive" || !row || row.fingerprint !== lock.fingerprint || (row.outcome === "pending" && lock.phase !== "preflight")) { problems++; continue; }
        if (unresolvedArchive(row)) refs.push({ reviewId: lock.reviewId, agentId: lock.agentId });
      } catch {
        // A fence released between the directory read and this read is no longer recovery work.
        if (await readFile(join(root, name)).then(() => true, (error) => error?.code !== "ENOENT")) problems++;
      }
    }
    // Every unvalidated fence still blocks its ID in execute, so it must never read as "none".
    if (problems) throw new Error(`Archive checks could not be fully read: ${problems} fence(s) could not be validated. No fence was changed.`);
    if (refs.length > UNRESOLVED_LIMIT) throw new Error(`More than ${UNRESOLVED_LIMIT} archives are not confirmed (${refs.length} found). The list cannot be shown exactly; no fence was changed.`);
    return refs;
  }
  async function capabilities() {
    const snapshot = await client.providers.snapshot();
    return { providers: snapshot.entries.map((entry) => ({ id: entry.provider, status: entry.status,
      models: (entry.models ?? []).filter((model) => model.isSelectable !== false).map((model) => ({ id: model.id, efforts: (model.thinkingOptions ?? []).map((option) => option.id) })) })), limitation: TAKEOVER_LIMIT };
  }
  async function preview(input: { action: LifecycleReview["action"]; scope: LifecycleReview["scope"]; agentIds: string[]; target: RecoveryTarget }): Promise<LifecycleReview> {
    if (input.action === "archive" && (input.scope !== "filtered" || input.agentIds.length < 1 || input.agentIds.length > ARCHIVE_BATCH_LIMIT)) throw new Error(`Archive requires 1–${ARCHIVE_BATCH_LIMIT} explicitly selected session IDs in filtered scope.`);
    const all = await inventory(client, input.action), ids = new Set(input.agentIds);
    const selectedIds = input.scope === "host" ? all.filter((agent) => agent.archivedAt == null).map((agent) => agent.id) : [...ids];
    const rows: LifecycleRow[] = [];
    for (const id of selectedIds) {
      const observed = await observe(id, input.action === "resume"), listed = all.find((agent) => agent.id === id);
      const record = observed.agent ?? listed;
      // Unreadable refusal evidence closes only this row's Resume, never the whole review.
      let refusal: ProviderRefusal | null = null, refusalProblem: string | null = null;
      if (input.action === "resume") try { refusal = await refusalFor(id, observed); } catch (error) { refusalProblem = errorText(error); }
      // No record: the exact read either confirmed absence ("missing") or failed ("unknown"); only absence reads as missing.
      const row: LifecycleRow = record ? lifecycleRow(record, all, input.action, refusal) : {
        id, title: id, workspaceId: null, cwd: "", provider: "", model: null, effort: null, mode: null,
        nativeStatus: observed.availability.state, fingerprint: "missing", category: "unknown", eligible: false, reason: "", children: [], outcome: "pending", message: null,
      };
      row.availability = observed.availability;
      if (input.action === "archive" && record?.archivedAt == null && row.children.length && row.children.every((child) => ids.has(child.id))) {
        row.category = "candidate"; row.eligible = true;
        row.reason = "All active managed children are selected. They must archive successfully before this parent; native preflight still blocks any remaining child.";
      }
      if (!observed.agent) { row.eligible = input.action === "archive" && observed.availability.state === "missing"; row.category = "unknown"; row.reason = observed.availability.reason; }
      if (input.action === "archive" && archivedMembership(observed.agent)) row.eligible = true;
      if (refusalProblem && row.eligible) { row.eligible = false; row.reason = refusalProblem; }
      rows.push(row);
    }
    const review: LifecycleReview = { id: randomUUID(), createdAt: new Date().toISOString(), action: input.action, scope: input.scope, target: input.target, rows: input.action === "archive" ? archiveOrder(rows) : rows, limitation: TAKEOVER_LIMIT };
    await save(review); return review;
  }
  /** An exact read. A Resume-path read is fenced: its attempt is durable before the read and its outcome is recorded
   *  after it. When the attempt cannot be published, the read is not made. */
  async function observe(id: string, fenced = false): Promise<RefusalRead> {
    if (!fenced) return { ...await readExact(id), attempt: 0, seen: null };
    let attempt: number;
    try { attempt = await begin(id); }
    catch (error) {
      const fence = error instanceof Error ? error : new Error(String(error));
      return { availability: { agentId: id, state: "unknown", reason: fence.message, checkedAt: new Date().toISOString(), revision: "unknown" }, attempt: 0, seen: null, fence };
    }
    const read = await readExact(id), seenAt = new Date().toISOString();
    // Bound when the exact read returned, never at its start: a slow read must not bind recovery before its record.
    const seen = read.agent ? observeRefusal(read.agent, seenAt) : null;
    return { ...read, attempt, seen, ...await settle(id, attempt, read.agent, seen, seenAt) };
  }
  async function readExact(id: string): Promise<{ agent?: Agent; availability: SessionAvailability }> {
    const checkedAt = new Date().toISOString();
    try {
      const found = await client.agents.ref(id).refresh();
      if (!found) return { availability: { agentId: id, state: "missing", reason: "No native record returned for this exact ID on this host. Nothing to archive; check availability to retry the read.", checkedAt, revision: "missing" } };
      if (found.agent.id !== id) return { availability: { agentId: id, state: "unknown", reason: "Availability unknown: the host reply identified a different session. This is not evidence that the requested session is missing.", checkedAt, revision: "unknown" } };
      return { agent: found.agent, availability: { agentId: id, state: "readable", reason: "Native record found. Provider history and ability to continue have not been verified.", checkedAt,
        revision: found.agent.updatedAt, nativeStatus: found.agent.status, archivedAt: found.agent.archivedAt ?? null,
        providerHistory: found.agent.persistence?.sessionId ? "unverified" : "not-reported" } };
    } catch (error) { return { availability: { agentId: id, state: "unknown", reason: `Native read failed: ${errorText(error)}. Absence is not confirmed.`, checkedAt, revision: "unknown" } }; }
  }
  /** This ID's refusal memory, in the path's queue. On first use it adopts the valid snapshot with the highest seq among
   *  the record, a held record and complete temps. An attempt that snapshot left open, a partial, corrupt, inconsistent
   *  or foreign artifact, a different snapshot of the same seq, a snapshot this worker did not write and does not
   *  outrank, or an older snapshot the memory does not cover leaves the memory unresolved: a refusal of
   *  unknown kind bound to now, so neither a stale kind, a cleared error nor a turn before this read can release it.
   *  Evidence that cannot be read is an error. */
  async function adopt(id: string, path: string): Promise<RefusalMemory> {
    const mine = covered.get(path) ?? new Set<string>(), known = memories.get(path), name = basename(path), adopted: string[] = [], found: Snapshot[] = [];
    let unsure = false;
    try {
      const names = await readdir(root).catch((error) => { if (error?.code === "ENOENT") return [] as string[]; throw error; });
      for (const entry of names.filter((value) => value === name || value.startsWith(`${name}.`))) {
        const file = join(root, entry);
        if (mine.has(file) || (known && file === path)) continue;
        const bytes = await readFile(file, "utf8").catch((error) => { if (error?.code === "ENOENT") return null; throw error; });
        if (bytes === null) continue;
        if (file !== path) adopted.push(file);
        const snapshot = snapshotOf(id, bytes);
        if (snapshot) found.push(snapshot); else unsure = true;
      }
    } catch (error) { throw new Error(`Provider refusal evidence for this session cannot be read (${errorText(error)}). Resume stays closed; that evidence is left as it is.`); }
    const top = found.reduce<Snapshot | null>((best, next) => !best || next.memory.seq > best.memory.seq ? next : best, null);
    const base = known ?? top?.memory ?? { seq: 0, refusal: null, open: [], clean: false, cleared: 0, recovered: null };
    for (const other of found) {
      if (known ? other.memory.seq >= known.seq : other.memory.seq === base.seq && JSON.stringify(other) !== JSON.stringify(top)) unsure = true;
      else if (!covers(base, other.memory)) unsure = true;
    }
    let memory = { ...base, seq: Math.max(base.seq, ...found.map((other) => other.memory.seq)) };
    if (!known && memory.open.length) { unsure = true; memory = { ...memory, open: [] }; }
    if (unsure) {
      const now = new Date().toISOString();
      memory = { ...memory, refusal: unresolved(memory, now, memory.seq) };
    }
    for (const file of adopted) mine.add(file);
    covered.set(path, mine); memories.set(path, memory);
    return memory;
  }
  /** Publishes one snapshot: the record, else its held fallback; a complete temp that reached neither is still read
   *  back after a restart. Returns an error only when none of them is durable. What it covers goes after that. */
  async function publish(id: string, path: string, memory: RefusalMemory, state: string): Promise<Error | null> {
    const bytes = JSON.stringify({ agentId: id, state, ...memory }), temporary = `${path}.${randomUUID()}.tmp`, held = `${path}.held`, mine = covered.get(path)!;
    mine.add(temporary);
    let wrote = false;
    try {
      await writeFile(temporary, bytes, { mode: 0o600 }); wrote = true;
      await rename(temporary, path); mine.delete(temporary);
      await sweep(mine, null); return null;
    } catch (error) {
      mine.add(held);
      if (await writeFile(held, bytes, { mode: 0o600 }).then(() => true, () => false)) { await sweep(mine, held); return null; }
      return wrote ? null : error instanceof Error ? error : new Error(String(error));
    }
  }
  /** Makes one Resume-path read's attempt durable before the read is made. */
  async function begin(id: string): Promise<number> {
    const path = refusalPath(id);
    return queued(path, async () => {
      await ready();
      const memory = await adopt(id, path), seq = memory.seq + 1;
      const next = { ...memory, seq, open: [...memory.open, { seq, revision: memory.seq, start: new Date().toISOString() }] };
      memories.set(path, { ...memory, seq });
      const problem = await publish(id, path, next, "attempt");
      if (problem) throw new Error(`Refusal memory for this session cannot be written (${errorText(problem)}), so the exact read was not made. Resume stays closed.`);
      memories.set(path, next);
      return seq;
    });
  }
  /** Records one fenced read's outcome, in the path's queue. A read that returned no record of this ID has no outcome:
   *  like an attempt left open across a restart, it leaves the memory unresolved. A refusal names the memory unless a newer read did, and
   *  is dropped only when a recovery whose final read started after it released it; otherwise it moves the binding later.
   *  A read without one keeps an open refusal. With no clean stored, a native error without a classified refusal is
   *  unresolved until a later successful provider turn, and only a read without a native error records the first clean,
   *  which counts once it is durable. */
  async function settle(id: string, attempt: number, agent: Agent | undefined, seen: RefusalObservation | null, seenAt: string): Promise<{ memory: RefusalMemory; problem: Error | null }> {
    const path = refusalPath(id);
    return queued(path, async () => {
      const memory = memories.get(path)!, open = memory.open.filter((entry) => entry.seq !== attempt);
      let { refusal, clean } = memory;
      if (!agent) { if (attempt > memory.cleared) refusal = unresolved(memory, seenAt, attempt); }
      else if (seen && attempt > memory.cleared) {
        const names = !refusal || attempt > refusal.seq;
        refusal = { ...joinRefusal(refusal, seen, names), seq: refusal && !names ? refusal.seq : attempt };
      } else if (!seen && !refusal && !clean) {
        if (agent.status === "error" || agent.lastError) refusal = { ...unknownRefusal(seenAt, NO_CLEAN), seq: attempt };
        else clean = true;
      }
      const next = { ...memory, seq: memory.seq + 1, open, refusal, clean };
      const problem = await publish(id, path, next, refusal ? "refusal" : clean && !open.length ? "clean" : "attempt");
      const kept = problem && clean && !memory.clean ? { ...next, clean: false } : next;
      memories.set(path, kept);
      return { memory: kept, problem };
    });
  }
  /** Releases the refusal `found` held, only if nothing changed it since and no later read is still open, and only once
   *  the release is durable. */
  async function commit(id: string, found: RefusalMemory, attempt: number): Promise<boolean> {
    const path = refusalPath(id);
    return queued(path, async () => {
      const now = memories.get(path)!;
      if (!now.refusal || pending(now, attempt) || JSON.stringify([now.refusal, now.cleared]) !== JSON.stringify([found.refusal, found.cleared])) return false;
      const next = { ...now, seq: now.seq + 1, refusal: null, clean: true, cleared: attempt, recovered: plain(now.refusal) };
      memories.set(path, { ...now, seq: next.seq });
      if (await publish(id, path, next, "recovery")) return false;
      memories.set(path, next); return true;
    });
  }
  const unrecorded = (read: RefusalRead) => read.seen && read.problem ? new Error(`The refusal this read saw could not be recorded (${errorText(read.problem)}). This worker keeps it, and the read's attempt on disk keeps Resume closed after a restart.`) : null;
  /** What a fenced exact read of this ID leaves: a refusal it saw (current), else the open refusal (history); with no
   *  clean stored, never "no refusal". Expiry, a failed read or a cleared lastError never release a refusal; only
   *  positive recovery after its observation does, when `recover` allows the bounded timeline read (G7). */
  async function refusalFor(id: string, observed: RefusalRead, recover = false): Promise<ProviderRefusal | null> {
    const { agent, seen, memory } = observed;
    if (!memory) throw observed.fence ?? new Error("This exact read was not fenced. Resume stays closed.");
    const problem = unrecorded(observed);
    if (problem) throw problem;
    if (seen) return refusalState(seen, { seen: true, readable: true, error: agent?.status === "error", recovered: false });
    if (!memory.refusal) return memory.clean && !pending(memory) ? null
      : refusalState(unknownRefusal(observed.availability.checkedAt, memory.clean ? PENDING : NO_CLEAN), { seen: false, readable: !!agent, error: false, recovered: false });
    let stored = memory.refusal, recovered = false;
    if (recover && agent && quietAfterRefusal(agent)) {
      // A timeline read can load the provider runtime, so only the explicit refusal read runs it. Unavailable is not recovery.
      const history = await client.agents.ref(id).timeline.refetch({ direction: "tail", limit: RECOVERY_TAIL_ITEMS }).catch(() => null);
      if (history && !history.error && recoveredAfterRefusal(history.entries, stored)) {
        // The session may have moved during the timeline read: only a later fenced read that is still quiet confirms, and
        // a refusal that read sees is new evidence that moves the stored binding and is reported, never cleared.
        const after = await observe(id, true), again = unrecorded(after);
        if (again) throw again;
        if (after.seen) stored = after.memory?.refusal ?? stored;
        else recovered = !!after.agent && !!after.memory && quietAfterRefusal(after.agent) && await commit(id, memory, after.attempt);
      }
    }
    return refusalState(plain(stored), { seen: false, readable: !!agent, error: false, recovered });
  }
  async function providerRefusal(id: string): Promise<ProviderRefusalRead> {
    const observed = await observe(id, true);
    return { availability: observed.availability, refusal: await refusalFor(id, observed, true) };
  }
  async function probe(ids: string[]): Promise<Record<string, SessionAvailability>> {
    const result: Record<string, SessionAvailability> = {};
    // Metadata reads only: no provider history load, send, create or archive.
    for (const id of new Set(ids)) result[id] = (await observe(id)).availability;
    return result;
  }
  async function dispositions(ids: string[]): Promise<Record<string, SessionAvailability>> {
    const result: Record<string, SessionAvailability> = {};
    for (const id of new Set(ids)) {
      let receipt: { agentId: string };
      try { receipt = JSON.parse(await readFile(removedPath(id), "utf8")); }
      catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") continue; throw error; }
      if (receipt.agentId !== id) continue;
      // A persisted local dismissal is not proof of ongoing absence. Recheck only
      // these operator-selected IDs. Unknown reads stay visible; readable IDs return.
      const observed = await observe(id);
      result[id] = { ...observed.availability, localRemoval: observed.availability.state === "missing" };
    }
    return result;
  }
  async function execute(reviewId: string, agentId: string): Promise<LifecycleRow> {
    const review = await read(reviewId);
    const row = review?.rows.find((entry) => entry.id === agentId);
    if (!review || !row) throw new Error("Exact session ID is not in this review.");
    if (row.outcome !== "pending") return review.action === "archive" && unresolvedArchive(row) ? reconcile(reviewId, agentId) : row;
    if (!row.eligible) { const blocked = { ...row, outcome: "blocked" as const, message: row.reason }; await saveRow(reviewId, blocked); return blocked; }
    if (review.action === "archive" && row.children.some((child) => {
      const result = review.rows.find((entry) => entry.id === child.id);
      return !result || !(result.outcome === "already_archived" || result.outcome === "archived" && result.runtime !== "unconfirmed");
    })) {
      const blocked = { ...row, outcome: "blocked" as const, message: "Selected children have not all archived with confirmed outcomes. No parent archive was sent." };
      await saveRow(reviewId, blocked); return blocked;
    }
    // A per-session lock covers competing reviews/actions and survives worker reload/crash.
    const lockPath = join(root, `intent-${hash(agentId)}.json`);
    let lock;
    try { lock = await open(lockPath, "wx", 0o600); }
    catch (error) {
      if (!(error instanceof Error) || !("code" in error) || error.code !== "EEXIST") throw error;
      // EEXIST is the ownership decision. The owner may still be publishing or
      // rewriting metadata, or may release the lock before this optional read.
      let reconcileReviewId: string | undefined;
      try {
        const prior = archiveIntentReferenceSchema.safeParse(JSON.parse(await readFile(lockPath, "utf8")));
        if (prior.success && prior.data.agentId === agentId) reconcileReviewId = prior.data.reviewId;
      } catch { /* Optional metadata cannot grant ownership or release the fence. */ }
      return { ...row, outcome: "blocked", ...(reconcileReviewId ? { reconcileReviewId } : {}),
        message: reconcileReviewId
          ? "An earlier archive owns this session. Check its native outcome; no second dispatch was made."
          : "An earlier action blocked this request. Its archive reconciliation reference is unavailable; no second dispatch was made. No retry or lock removal was attempted." };
    }
    // The token makes each fence write unique, so a release never matches another execute's identical fence.
    const token = randomUUID(), preflight = JSON.stringify({ reviewId, agentId, action: review.action, phase: "preflight", fingerprint: row.fingerprint, token });
    await lock.writeFile(preflight); await lock.sync(); await lock.close();
    let ours = preflight;
    let dispatchStarted = false, alreadyArchived = false;
    try {
      // Another click may have read pending before the first owner saved its result and released.
      const settled = (await read(reviewId))?.rows.find((entry) => entry.id === agentId);
      if (settled && settled.outcome !== "pending") { await release(lockPath, ours); return settled; }
      const doneFile = join(root, `done-${hash(`${agentId}:${row.fingerprint}`)}.json`);
      try { await readFile(doneFile, "utf8"); throw new Error("This exact session revision already has a dispatched action. Reconcile its receipt before another action."); }
      catch (error) { if (!(error instanceof Error) || !("code" in error) || error.code !== "ENOENT") throw error; }
      const agent = client.agents.ref(agentId);
      const observed = await observe(agentId, review.action === "resume"); row.availability = observed.availability;
      if (!observed.agent) throw new Error(observed.availability.reason);
      const found = { agent: observed.agent };
      alreadyArchived = archivedMembership(found.agent);
      if (found.agent.archivedAt != null) throw new Error(alreadyArchived ? "Already archived on this host. No archive request was sent." : "Archive is recorded but runtime retirement is not confirmed. Check the existing native outcome; no duplicate request was sent.");
      if ((review.action === "archive" ? archiveRevision(found.agent) : lifecycleRevision(found.agent)) !== row.fingerprint) throw new Error("Session changed or disappeared while review was open. Nothing dispatched; create a fresh review.");
      const all = await inventory(client, review.action);
      const current = lifecycleRow(found.agent, all, review.action, review.action === "resume" ? await refusalFor(agentId, observed) : null);
      if (!current.eligible || (review.action === "archive" && current.children.length)) throw new Error(current.reason);
      if (review.action === "resume") {
        // The session's own provider, never the review target.
        const snapshot = await client.providers.snapshot({ cwd: found.agent.cwd });
        if (!snapshot.entries.some((entry) => entry.provider === found.agent.provider && entry.enabled && entry.status === "ready"))
          throw new Error(`This session's provider ${found.agent.provider} is not ready on this host. Nothing was sent.`);
        // This is user-authorized execution preflight, never an automatic availability probe:
        // native timeline fetch can load the provider runtime. Context stays in that same session.
        const history = await agent.timeline.refetch({ direction: "tail", limit: 1 });
        if (history.error || !history.entries.length) throw new Error(`Native history is unavailable; no continuation prompt sent. ${history.error ?? "No retained conversation entries."}`);
      }
      // Final read after all potentially slow preflight calls (Resume only; nothing slow runs between Archive's
      // child walk and this read). Never prompt an archived predecessor.
      const latest = await observe(agentId, review.action === "resume"); row.availability = latest.availability;
      if (!latest.agent) throw new Error(latest.availability.reason);
      const final = { agent: latest.agent };
      alreadyArchived = archivedMembership(final.agent);
      if (final.agent.archivedAt != null) throw new Error(alreadyArchived ? "Already archived on this host. No archive request was sent." : "Archive is recorded but runtime retirement is not confirmed. No duplicate request was sent.");
      if ((review.action === "archive" ? archiveRevision(final.agent) : lifecycleRevision(final.agent)) !== row.fingerprint) throw new Error("Session changed during preflight. Nothing dispatched; review again.");
      const finalDecision = lifecycleRow(final.agent, all, review.action, review.action === "resume" ? await refusalFor(agentId, latest) : null);
      if (!finalDecision.eligible) throw new Error(finalDecision.reason);
      row.outcome = "intent"; row.message = "Durable intent saved. An interrupted dispatch must be reconciled before retry.";
      const intent = JSON.stringify({ reviewId, agentId, action: review.action, phase: "intent", fingerprint: row.fingerprint, token });
      if (!await whileOwned(lockPath, ours, async () => { await saveRow(reviewId, row); await writeFile(lockPath, intent, { mode: 0o600 }); }))
        throw new Error("Another action took this session's fence before dispatch. Nothing was sent; review again.");
      ours = intent; dispatchStarted = true;
      if (review.action === "resume") {
        await agent.send(RESUME_PROMPT, { messageId: `kontor-resume-${review.id}-${hash(agentId).slice(0, 12)}`, activeTurnBehavior: "steer" });
        row.outcome = "submitted"; row.message = "Resume prompt accepted in the same session. Completion and resolution of the original failure are not verified.";
      } else {
        // A rejected reply can follow a persisted archive, so read before classifying. A rejection is untyped here:
        // a lost acknowledgement looks the same as a close error, so neither is reported as a proven close failure.
        const reply = await agent.archive().then((receipt) => ({ receipt }), (error: unknown) => ({ error }));
        if ("receipt" in reply && "agentId" in reply.receipt && reply.receipt.agentId !== agentId) throw new Error("Archive acknowledgement identified another session. Check native outcome; no retry was made.");
        const observedAfter = await observe(agentId); row.availability = observedAfter.availability;
        const after = observedAfter.agent;
        if ("error" in reply && !archivedMembership(after)) throw new Error(active(after) ? "Archive not confirmed: the session still reports activity." : `Archive not confirmed: ${errorText(reply.error)}`);
        if (!archivedMembership(after)) throw new Error("Archive returned, but an exact-ID read does not show this session archived and inactive. Check again; do not retry the request.");
        row.nativeStatus = after!.status; row.outcome = "archived";
        row.runtime = "error" in reply ? "unconfirmed" : after!.status === "closed" ? "closed" : "not-observed";
        row.message = "error" in reply ? `Archived. The archive reply did not confirm completion (${errorText(reply.error)}), so stopping its runtime is not confirmed.`
          : row.runtime === "closed" ? "Archived. An exact-ID read shows its runtime closed. Durable history and files remain with the host."
          : "Archived. The host did not report its runtime as closed. Durable history and files remain with the host.";
      }
    } catch (error) {
      row.outcome = dispatchStarted ? "uncertain" : row.availability?.state === "missing" ? "absent" : review.action === "archive" && alreadyArchived ? "already_archived" : "blocked";
      row.message = errorText(error);
      if (review.action === "archive" && row.outcome === "absent") {
        await writeFile(removedPath(agentId), JSON.stringify({ agentId, reviewId, observedAt: row.availability!.checkedAt }), { mode: 0o600 });
        row.availability = { ...row.availability!, localRemoval: true };
        row.message = "No native record exists for this exact ID on this host. A local stale-row removal was recorded; no daemon archive was performed.";
      }
    }
    const written = JSON.stringify({ reviewId, agentId, action: review.action, phase: row.runtime === "unconfirmed" ? "unconfirmed" : row.outcome, fingerprint: row.fingerprint, message: row.message, token });
    const done = async () => { if (dispatchStarted) await writeFile(join(root, `done-${hash(`${agentId}:${row.fingerprint}`)}.json`), JSON.stringify(row), { mode: 0o600 }); };
    // Unknown effects and an unconfirmed runtime retain the session lock. A fresh review may retry a known preflight block.
    if (await whileOwned(lockPath, ours, async () => {
      await saveRow(reviewId, row); await done();
      if (unresolvedArchive(row)) await writeFile(lockPath, written, { mode: 0o600 }); else await unlink(lockPath);
    })) return row;
    // A Check settled and released this fence while the request ran, and a successor may hold the path now: that saved
    // result and fence stand. This call reports what it observed, unsaved; only the dispatch record is added.
    await done();
    return { ...row, message: `${row.message ?? ""} This session's fence now belongs to another action, so this result was not recorded.`.trim() };
  }
  /** The row execute recorded when its dispatch ended; null unless it parses and names this exact ID, revision and a
   *  readable exact-ID read. Read only: a fault or any mismatch is no evidence, so the fence stays. */
  async function dispatchReceipt(agentId: string, fingerprint: string): Promise<LifecycleRow | null> {
    try {
      const receipt = lifecycleRowSchema.safeParse(JSON.parse(await readFile(join(root, `done-${hash(`${agentId}:${fingerprint}`)}.json`), "utf8")));
      return receipt.success && receipt.data.id === agentId && receipt.data.fingerprint === fingerprint
        && receipt.data.availability?.agentId === agentId && receipt.data.availability.state === "readable" ? receipt.data : null;
    } catch { return null; }
  }
  /** Settles a dispatched archive by an exact-ID read. Never resends; releases only its own fence (G3). */
  async function reconcile(reviewId: string, agentId: string): Promise<LifecycleRow> {
    const review = await read(reviewId), row = review?.rows.find((entry) => entry.id === agentId);
    if (!review || !row || review.action !== "archive") throw new Error("Exact archive ID is not in this review.");
    const lockPath = join(root, `intent-${hash(agentId)}.json`);
    let fence: unknown, bytes = "";
    try { bytes = await readFile(lockPath, "utf8"); fence = JSON.parse(bytes); }
    catch (error) { if (!(error instanceof Error && "code" in error && error.code === "ENOENT")) fence = "unreadable"; }
    const lock = fenceSchema.safeParse(fence);
    const owned = lock.success && lock.data.reviewId === reviewId && lock.data.agentId === agentId && lock.data.action === "archive" && lock.data.fingerprint === row.fingerprint;
    if (row.outcome === "pending") {
      if (fence === undefined) return row;
      if (!lock.success || lock.data.reviewId !== reviewId || lock.data.agentId !== agentId || lock.data.action !== "archive") throw new Error("This review does not own the pending archive intent.");
    } else if (!unresolvedArchive(row)) {
      // A saved settled row whose release was interrupted: execute releases exactly this owned fence for such a row.
      // A preflight fence belongs to an execute that is releasing it itself.
      if (owned && lock.data.phase !== "preflight") await release(lockPath, bytes);
      return row;
    }
    // An earlier Check could replace that saved text. The receipt written with this legacy fence (one per exact ID and
    // revision, never rewritten) keeps it; a pending row's fence belongs to an execute that has not dispatched.
    const legacy = legacySuccess(row) || (owned && row.outcome !== "pending" && lock.success && lock.data.phase === "uncertain"
      && !!(await dispatchReceipt(agentId, row.fingerprint).then((receipt) => receipt && legacySuccess(receipt))));
    const observed = await observe(agentId), after = observed.agent;
    const closed = archivedMembership(after) && after!.status === "closed";
    const settled = closed || (legacy && owned && archivedMembership(after));
    const foreign = fence !== undefined && !owned ? " This session's archive fence does not belong to this review, so it was not released." : "";
    const result: LifecycleRow = { ...row, availability: observed.availability, nativeStatus: after?.status ?? row.nativeStatus,
      ...(settled ? { outcome: "archived" as const, runtime: closed ? "closed" as const : "not-observed" as const,
        message: (closed ? "Archived. An exact-ID read shows its runtime closed. No additional archive request was sent." : "Archived. The archive had returned; the host did not report its runtime as closed. No additional archive request was sent.") + foreign }
      : archivedMembership(after) ? { outcome: "archived" as const, runtime: "unconfirmed" as const,
        message: "Archived, but an exact-ID read does not show its runtime closed, so stopping it is still not confirmed. The fence is kept; no additional archive request was sent." + foreign }
      : { outcome: "uncertain" as const, runtime: undefined, message: "Archive outcome remains uncertain. The original no-replay fence is preserved. " + observed.availability.reason + foreign }) };
    if (!owned) { await saveRow(reviewId, result); return result; }
    // Save before releasing, both only while the fence still holds the bytes read above: an execute finishing, or a
    // successor, may have changed it during the read; then the saved result and that fence stand, and this call reports
    // its read unsaved. A failed save keeps
    // the fence and the unresolved row, so the lookup still lists it; an interrupted release leaves an owned fence
    // behind a settled row, which the next Check releases. A pending row's fence belongs to an execute in preflight.
    if (await whileOwned(lockPath, bytes, async () => { await saveRow(reviewId, result); if (settled && row.outcome !== "pending") await unlink(lockPath); })) return result;
    return { ...result, message: `${result.message ?? ""} This session's fence changed during the read, so this result was not recorded.`.trim() };
  }
  return { capabilities, preview, read, execute, reconcile, probe, dispositions, providerRefusal };
}

export function contributeSessionLifecycle(server: PluginServerContext) {
  const root = join(dirname(readConfig().paseoWorktreesRoot), "plugin-state", "kontor", "session-lifecycle-v1");
  server.handle(lifecycleCapabilities, (_input, { paseo }) => createLifecycleService(paseo, root).capabilities());
  server.handle(previewLifecycle, (input, { paseo }) => createLifecycleService(paseo, root).preview(input));
  server.handle(readLifecycleReview, (input, { paseo }) => createLifecycleService(paseo, root).read(input.reviewId));
  server.handle(executeLifecycleItem, (input, { paseo }) => createLifecycleService(paseo, root).execute(input.reviewId, input.agentId));
  server.handle(reconcileLifecycleItem, (input, { paseo }) => createLifecycleService(paseo, root).reconcile(input.reviewId, input.agentId));
  server.handle(probeSessionAvailability, (input, { paseo }) => createLifecycleService(paseo, root).probe(input.agentIds));
  server.handle(readSessionDispositions, (input, { paseo }) => createLifecycleService(paseo, root).dispositions(input.agentIds));
  server.handle(readProviderRefusal, (input, { paseo }) => createLifecycleService(paseo, root).providerRefusal(input.agentId));
}
