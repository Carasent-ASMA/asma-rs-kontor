import { createHash, randomBytes, randomUUID, timingSafeEqual } from "node:crypto";
import { mkdir, open, readdir, readFile, rename, unlink, writeFile } from "node:fs/promises";
import { basename, dirname, join, resolve } from "node:path";
import { isDeepStrictEqual } from "node:util";
import { z } from "zod";
import type { PaseoApi, PaseoAgentListResult } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import type { SummaryPin, SummaryRevision } from "../shared/session-navigator";
import { readConfig, type KontorPluginConfig } from "./config";
import { parent, seatBinding } from "./labels";
import { within } from "./placement";
import { sourceCatalog } from "./read-sources";
import { canonicalBindingProblem, placement, promotionBindingFor, transferPromotion } from "./session-canonical";
import { bootstrapPrompt, createSessionCreateService, createStateRoot, createSwitchFile, effectValidityProblem, expectedPlacementProblem, readbackDifferences, reissuePersona, type SeatCreateSpec } from "./session-create";
import { createSessionSummaryService, summaryStateRoot } from "./session-summaries";
import {
  BINDING_KEY, configuredAccountCatalogFromFleet, explicitTakeoverRoute, explicitTakeoverRouteSchema, joinSessionLocalRefusalToRouteHealth, resolveTakeoverRoute,
  routeEvidenceDigest, routeHealthFromProviderSnapshot, sessionLocalRefusalForTakeover, sourceFeatureValues, takeoverAdapter, takeoverPinnedMatches,
  type ConfiguredAccountCatalog, type ExplicitTakeoverRoute, type ProviderRouteSnapshot, type ResolverRun, type RouteHealth, type TakeoverAdapter, type TakeoverRoute,
} from "./session-takeover";
import { asTakeoverClient } from "./session-navigator";
import { createTeamsSourceAdapter } from "./teams-source-adapter";
import { createPaseoPublicCli, type PaseoPublicCli } from "./paseo-cli";
import { createCompleteScheduleReader, type CompleteScheduleReader } from "./paseo-host";
import { CREATE_REQUEST_LABEL, TITLE_AUTO_LABEL, type CreateOutcome } from "../shared/session-create";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import {
  TAKEOVER_LIMIT, RESUME_REMOVED, ARCHIVE_BATCH_LIMIT, archiveOrder, lifecycleReviewSchema, lifecycleRowSchema, unresolvedArchive,
  previewLifecycle, executeLifecycleItem, reconcileLifecycleItem, readLifecycleReview, probeSessionAvailability, readSessionDispositions,
  TAKEOVER_EXTERNAL_BOUNDARIES, TAKEOVER_LIMITS, TAKEOVER_PROMPT, applyTakeover, checkTakeover, reviewTakeover, takeoverOutcomeSchema, takeoverReviewSchema,
  type LifecycleReview, type LifecycleRow, type RecoveryTarget, type SessionAvailability, type TakeoverOutcome, type TakeoverReview, type TakeoverReviewInput, type UnresolvedRef,
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
const fenceSchema = z.object({ reviewId: z.string(), agentId: z.string(), action: z.enum(["resume", "archive", "takeover"]), phase: z.string(), fingerprint: z.string() });
/** Written only by the pre-runtime-split success check; kept solely to recognise its fences (G3). */
const LEGACY_PREDICATE_MISS = "Archive returned but exact-ID terminal retirement readback is unavailable. Check native outcome; do not retry the request.";
/** Only the old success branch saved this text, and only after archive() resolved and a read showed archivedAt. */
const legacySuccess = (row: LifecycleRow) => row.outcome === "uncertain" && row.message === LEGACY_PREDICATE_MISS && !!row.availability?.archivedAt;
const UNRESOLVED_LIMIT = 100;
// Compare-and-swap: every fence phase write, its paired result save and every release run only while the fence
// still holds the exact bytes this call read or wrote (every field, including phase and the per-execute token), so a
// fence written after that (a successor Archive or Take over) is never overwritten or removed. One step per path at a time.
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
// Refusal memory, write-ahead and per ID: every fenced exact read first publishes its attempt, then its outcome, as
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
const identity = ({ observedAt: _at, seq: _seq, ...rest }: RefusalObservation & { seq?: number }) => JSON.stringify(rest);
/** Whether `memory` accounts for everything an older snapshot holds. Each read that snapshot left open is still open,
 *  named the open refusal or was the last recovery's final read. Its refusal is still open, named by the same read with
 *  the same evidence or by a later read, and bound no earlier; or a recovery whose final read started after that refusal
 *  was named released that same refusal (its kind, route and evidence), bound no earlier. */
const covers = (memory: RefusalMemory, older: RefusalMemory) => {
  const held = memory.refusal, refusal = older.refusal, recovered = memory.recovered;
  return older.open.every((entry) => memory.open.some((open) => open.seq === entry.seq) || held?.seq === entry.seq || memory.cleared === entry.seq)
    && (!refusal || (held ? (held.seq > refusal.seq || (held.seq === refusal.seq && identity(held) === identity(refusal))) && bound(held, refusal)
      : !!recovered && memory.cleared > refusal.seq && identity(recovered) === identity(refusal) && bound(recovered, refusal)));
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
/** One archive review row. Resume was removed from Kontor; its receipts stay readable but are never classified again. */
export function lifecycleRow(agent: Agent, inventory: Agent[]): LifecycleRow {
  const children = childrenOf(agent, inventory).map((child) => ({ id: child.id, title: child.title ?? child.id, status: child.status, workspaceId: child.workspaceId ?? null }));
  const row: LifecycleRow = { id: agent.id, title: agent.title ?? agent.id, workspaceId: agent.workspaceId ?? null, cwd: agent.cwd,
    provider: agent.provider, model: agent.runtimeInfo?.model ?? agent.model ?? null,
    effort: agent.effectiveThinkingOptionId ?? agent.runtimeInfo?.thinkingOptionId ?? null,
    mode: agent.currentModeId ?? agent.runtimeInfo?.modeId ?? null, nativeStatus: agent.status,
    fingerprint: archiveRevision(agent), category: "excluded", eligible: false, reason: "", children, outcome: "pending", message: null };
  if (agent.archivedAt != null) row.reason = "Already archived. This action never revives an archived session.";
  else if (children.length) row.reason = "Archive blocked: native archive can cascade to these children. Preserve or finish them through the owning workflow first.";
  else { row.category = "candidate"; row.eligible = true; row.reason = "Archive stops this session and its provider tools, closes its runtime, and keeps durable history and files. No undo is exposed by this SDK."; }
  return row;
}

// Archive needs only the active children (childrenOf skips archived ones), so its walk never reads archived history.
async function inventory(client: { agents: Pick<LifecycleClient["agents"], "list"> }): Promise<Agent[]> {
  const agents = new Map<string, Agent>(), cursors = new Set<string>();
  let cursor: string | undefined;
  for (let page = 0; page < 1_000; page++) {
    const result = await client.agents.list({ filter: { includeArchived: false }, page: { limit: 200, ...(cursor ? { cursor } : {}) } });
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
  async function preview(input: { action: "archive"; scope: LifecycleReview["scope"]; agentIds: string[]; target: RecoveryTarget }): Promise<LifecycleReview> {
    // Resume was removed with its toolbar: a request naming it is refused before any read or write.
    if (input.action !== "archive") throw new Error(RESUME_REMOVED);
    if (input.scope !== "filtered" || input.agentIds.length < 1 || input.agentIds.length > ARCHIVE_BATCH_LIMIT) throw new Error(`Archive requires 1–${ARCHIVE_BATCH_LIMIT} explicitly selected session IDs in filtered scope.`);
    const all = await inventory(client), ids = new Set(input.agentIds);
    const rows: LifecycleRow[] = [];
    for (const id of ids) {
      const observed = await observe(id), listed = all.find((agent) => agent.id === id);
      const record = observed.agent ?? listed;
      // No record: the exact read either confirmed absence ("missing") or failed ("unknown"); only absence reads as missing.
      const row: LifecycleRow = record ? lifecycleRow(record, all) : {
        id, title: id, workspaceId: null, cwd: "", provider: "", model: null, effort: null, mode: null,
        nativeStatus: observed.availability.state, fingerprint: "missing", category: "unknown", eligible: false, reason: "", children: [], outcome: "pending", message: null,
      };
      row.availability = observed.availability;
      if (record?.archivedAt == null && row.children.length && row.children.every((child) => ids.has(child.id))) {
        row.category = "candidate"; row.eligible = true;
        row.reason = "All active managed children are selected. They must archive successfully before this parent; native preflight still blocks any remaining child.";
      }
      if (!observed.agent) { row.eligible = observed.availability.state === "missing"; row.category = "unknown"; row.reason = observed.availability.reason; }
      if (archivedMembership(observed.agent)) row.eligible = true;
      rows.push(row);
    }
    const review: LifecycleReview = { id: randomUUID(), createdAt: new Date().toISOString(), action: input.action, scope: input.scope, target: input.target, rows: archiveOrder(rows), limitation: TAKEOVER_LIMIT };
    await save(review); return review;
  }
  /** An exact read. A provider-refusal read is fenced: its attempt is durable before the read and its outcome is recorded
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
    } catch (error) { throw new Error(`Provider refusal evidence for this session cannot be read (${errorText(error)}). That evidence is left as it is, and no refusal reads as cleared.`); }
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
  /** Makes one fenced exact read's attempt durable before the read is made. */
  async function begin(id: string): Promise<number> {
    const path = refusalPath(id);
    return queued(path, async () => {
      await ready();
      const memory = await adopt(id, path), seq = memory.seq + 1;
      const next = { ...memory, seq, open: [...memory.open, { seq, revision: memory.seq, start: new Date().toISOString() }] };
      memories.set(path, { ...memory, seq });
      const problem = await publish(id, path, next, "attempt");
      if (problem) throw new Error(`Refusal memory for this session cannot be written (${errorText(problem)}), so the exact read was not made.`);
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
  const unrecorded = (read: RefusalRead) => read.seen && read.problem ? new Error(`The refusal this read saw could not be recorded (${errorText(read.problem)}). This worker keeps it, and the read's attempt on disk keeps its refusal unknown after a restart.`) : null;
  /** What a fenced exact read of this ID leaves: a refusal it saw (current), else the open refusal (history); with no
   *  clean stored, never "no refusal". Expiry, a failed read or a cleared lastError never release a refusal; only
   *  positive recovery after its observation does, when `recover` allows the bounded timeline read (G7). */
  async function refusalFor(id: string, observed: RefusalRead, recover = false): Promise<ProviderRefusal | null> {
    const { agent, seen, memory } = observed;
    if (!memory) throw observed.fence ?? new Error("This exact read was not fenced, so its refusal is unknown.");
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
    // An earlier Resume receipt is history: it is never executed or replayed, and nothing is read or written for it.
    if (review.action !== "archive") throw new Error(RESUME_REMOVED);
    if (row.outcome !== "pending") return unresolvedArchive(row) ? reconcile(reviewId, agentId) : row;
    if (!row.eligible) { const blocked = { ...row, outcome: "blocked" as const, message: row.reason }; await saveRow(reviewId, blocked); return blocked; }
    if (row.children.some((child) => {
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
      const observed = await observe(agentId); row.availability = observed.availability;
      if (!observed.agent) throw new Error(observed.availability.reason);
      const found = { agent: observed.agent };
      alreadyArchived = archivedMembership(found.agent);
      if (found.agent.archivedAt != null) throw new Error(alreadyArchived ? "Already archived on this host. No archive request was sent." : "Archive is recorded but runtime retirement is not confirmed. Check the existing native outcome; no duplicate request was sent.");
      if (archiveRevision(found.agent) !== row.fingerprint) throw new Error("Session changed or disappeared while review was open. Nothing dispatched; create a fresh review.");
      const all = await inventory(client);
      const current = lifecycleRow(found.agent, all);
      if (!current.eligible || current.children.length) throw new Error(current.reason);
      // Final read; nothing slow runs between the child walk and this read.
      const latest = await observe(agentId); row.availability = latest.availability;
      if (!latest.agent) throw new Error(latest.availability.reason);
      const final = { agent: latest.agent };
      alreadyArchived = archivedMembership(final.agent);
      if (final.agent.archivedAt != null) throw new Error(alreadyArchived ? "Already archived on this host. No archive request was sent." : "Archive is recorded but runtime retirement is not confirmed. No duplicate request was sent.");
      if (archiveRevision(final.agent) !== row.fingerprint) throw new Error("Session changed during preflight. Nothing dispatched; review again.");
      const finalDecision = lifecycleRow(final.agent, all);
      if (!finalDecision.eligible) throw new Error(finalDecision.reason);
      row.outcome = "intent"; row.message = "Durable intent saved. An interrupted dispatch must be reconciled before retry.";
      const intent = JSON.stringify({ reviewId, agentId, action: review.action, phase: "intent", fingerprint: row.fingerprint, token });
      if (!await whileOwned(lockPath, ours, async () => { await saveRow(reviewId, row); await writeFile(lockPath, intent, { mode: 0o600 }); }))
        throw new Error("Another action took this session's fence before dispatch. Nothing was sent; review again.");
      ours = intent;
      dispatchStarted = true;
      {
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
      row.outcome = dispatchStarted ? "uncertain" : row.availability?.state === "missing" ? "absent" : alreadyArchived ? "already_archived" : "blocked";
      row.message = errorText(error);
      if (row.outcome === "absent") {
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
    if (review?.action === "resume") throw new Error(RESUME_REMOVED);
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
  return { preview, read, execute, reconcile, probe, dispositions, providerRefusal };
}

// ASMA-8641 Take over, native/direct adapter. One operation per confirmed review: it shares this root and the per-ID
// intent fence with Archive, retires the predecessor, and creates its one successor through the #500 core.
export type TakeoverClient = {
  agents: { list: LifecycleClient["agents"]["list"]; ref(id: string): Pick<ReturnType<PaseoApi["agents"]["ref"]>, "refresh" | "archive" | "send" | "timeline" | "detach"> };
  workspaces: { ref(id: string): Pick<ReturnType<PaseoApi["workspaces"]["ref"]>, "refresh"> };
  /** Optional bounded wake read; without it, empty child inventory does not imply wakes clear. */
  schedules?: { forAgent(agentId: string): Promise<{ entries: unknown[] }> };
};
export type ProfileChoice = { bindingKey: string; chain: string };
/** Read from the selected owner, never supplied by the review caller or inferred from labels. */
const continuitySchema = z.object({
  agentId: z.string().min(1), requestId: z.string().uuid(), revision: z.string().min(1), observedAt: z.iso.datetime(),
  surface: z.enum(["direct", "governed", "unknown"]),
  wakes: z.enum(["clear", "preserved", "unknown"]),
  /** Why a live source's scheduled wakes are not settled; never schedule contents or cadences. */
  wakeGap: z.string().nullable().optional(),
  /** The daemon identity that produced a complete scheduled-wake inventory, when one was required. */
  wakeHost: z.string().min(1).nullable().optional(),
  owner: z.object({ agentId: z.string().min(1), revision: z.string().min(1), bindingRevision: z.string().min(1) }).strict().nullable(),
}).strict();
export type TakeoverContinuity = z.infer<typeof continuitySchema>;
export type TakeoverDeps = {
  config: Pick<KontorPluginConfig, "cliCommand" | "stateRoot" | "placementRoots">;
  create: Pick<ReturnType<typeof createSessionCreateService>, "createSeat" | "check" | "preflight" | "record">;
  /** The provider-independent saved-summary service; it captures context without prompting the source. */
  summaries: Pick<ReturnType<typeof createSessionSummaryService>, "save">;
  /** The host provider snapshot session creation reads; explicit model choices are validated against it. */
  snapshot: (input: { cwd: string }) => Promise<ProviderRouteSnapshot>;
  /** Durable promotion records, shared with the New-session writer. */
  promotionRoot: string;
  /** One operator configured-source read per assess: profile choices and account-alias catalog share one revision. */
  operatorFleetDiscovery: () => Promise<{ choices: ProfileChoice[]; catalog: ConfiguredAccountCatalog }>;
  /** The host's route evidence for one checkout and permission mode; only a qualified positive source may call a route available. */
  evidence: (input: { mode: string | null; cwd: string; catalog: ConfiguredAccountCatalog }) => RouteHealth | Promise<RouteHealth>;
  /** A qualified owner/wake read. Missing support is unknown and blocks retirement. */
  continuity?: (agentId: string, requestId: string) => Promise<TakeoverContinuity | null>;
  /** The server-configured public CLI stop; absent means no running stop is attempted. */
  publicCli?: PaseoPublicCli;
  /** The supported public host connector's complete schedule inventory; absent means live wakes stay unknown. */
  completeSchedules?: CompleteScheduleReader;
  run?: ResolverRun;
  /** Pause between the bounded exact reads that look for a closed runtime after archive. */
  settleMs?: number;
};
/** Everything a confirmation pins; apply recomputes it and refuses any difference. */
type Pinned = {
  predecessor: string; owner: TakeoverAdapter; ownerRevision: string | null; created: string | null; bindingKey: string;
  route: Pick<TakeoverRoute, "provenance" | "selected" | "considered" | "validity">; evidence: string; continuity: TakeoverContinuity; spec: SeatCreateSpec;
  /** The source is already archived: its history is read, its runtime is never revived or prompted. */
  retiredSource: boolean;
  /** Managed children the retirement preserves through supported detach, never archive. */
  children: string[];
  /** The saved final summary revision delivered as the successor's bootstrap; null only for a running review preview. */
  summary: SummaryPin | null;
  /** Auto follows the configured chain; explicit uses the operator's selected host route. */
  selection: "auto" | "explicit";
  /** The source was active at review; apply stops it, verifies the same identity, then takes the final summary. */
  running: boolean;
  /** Stable native identity (id/title/cwd/workspace/owner/creation time), not a mutable status revision. */
  sourceIdentity: string;
  /**
   * Fingerprint of the normalized source settings actually inherited; null when the explicit route replaces them.
   * Empty inherited settings have a non-null fingerprint, so a later setting appearing is drift too.
   */
  sourceSettings: string | null;
};
const SUPERVISOR_LIMIT = "The successor keeps its supervisor as a label; the creation core sets no native parent link.";
const CHILD_LIMIT = "Managed children are detached and continue independently; this host exposes no operation that re-parents them to the successor.";
const UNOBSERVED_LIMIT = "No host source of positive route health is qualified yet, so every route reads as unobserved.";
export const RUNNING_STOP_GAP = "This session is running, and no server-configured public Paseo CLI stop adapter is available. Nothing was retired; configure the public CLI on the plugin server or wait for the turn to end, then take over.";
export const RUNNING_STOP_DISCLOSURE = "This session is running. Apply stops the exact source through the server-configured public Paseo CLI, reads it back on the same host, and only then captures the final handoff summary and archives it. If the stop is unconfirmed or its title, checkout, workspace or owner changes, nothing is archived or created.";
const RUNNING_SUMMARY_PREVIEW = "Preview only: this source is still running. The final handoff summary is captured after the approved stop and before retirement; this preview is not a final summary.";
export const SCHEDULE_TRANSFER_GAP = `The source has an ongoing source-linked scheduled duty. It is readable through ${TAKEOVER_EXTERNAL_BOUNDARIES.schedule.publicRead}, but the public schedule surface provides none of: ${TAKEOVER_EXTERNAL_BOUNDARIES.schedule.missing.join("; ")}; retirement is blocked rather than silently dropping the wake.`;
export const GOVERNED_ADAPTER_GAP = `This is a Kontor-governed seat. Its existing authority writer is the only creator of a successor (${TAKEOVER_EXTERNAL_BOUNDARIES.governed.writers.join(" or ")}), and neither installed writer accepts ${TAKEOVER_EXTERNAL_BOUNDARIES.governed.missing.join(" or ")} that a takeover must place before task effects. No native fallback is performed and nothing is retired here.`;
const CWD_DRIFT = "This session runs outside its workspace directory, and a successor would start in the workspace directory, so it is not taken over.";
/** Scope, title, supervisor and orchestration labels a successor keeps; anything else is not carried. */
const CARRIED_LABELS = ["role", "epic", "task", "ticket", TITLE_AUTO_LABEL, "orchestration", "paseo.parent-agent-id"];
export const TAKEOVER_PREDECESSOR_LABEL = "takeover.predecessor";
const takeoverRecordSchema = z.object({ review: takeoverReviewSchema, requestId: z.string().uuid(), chosen: z.string().nullable(), chosenRoute: z.unknown().nullable().optional(), pinned: z.unknown(), nonce: z.string().nullable() }).strict();
type TakeoverRecord = z.infer<typeof takeoverRecordSchema>;
/** One durable source->successor record: a second takeover of the same source never creates another successor. */
const successorMarkerSchema = z.object({
  sourceAgentId: z.string().min(1), reviewId: z.string().uuid(), requestId: z.string().uuid(),
  successorAgentId: z.string().min(1), successorTitle: z.string().min(1),
  state: z.enum(["created", "succeeded"]), updatedAt: z.iso.datetime(),
}).strict();
type SuccessorMarker = z.infer<typeof successorMarkerSchema>;
const associationSchema = z.object({ agentId: z.string(), bindingKey: z.string().regex(BINDING_KEY), reviewId: z.string() }).strict();
const seal = (nonce: string, record: Pick<TakeoverReview, "reviewId" | "revision" | "agentId">, pinned: unknown) => hash(JSON.stringify([nonce, record.reviewId, record.revision, record.agentId, pinned]));
const sameDigest = (left: string, right: string) => left.length === right.length && timingSafeEqual(Buffer.from(left), Buffer.from(right));
const releaseMessage = (reviewId: string) => `kontor-takeover-${reviewId}`;
const settledTakeover = (result: TakeoverOutcome) => result.state === "refused" || result.state === "succeeded";
const takeoverRevision = (agent: Agent) => hash(JSON.stringify([lifecycleRevision(agent), agent.title, agent.createdAt]));
/** Stable native identity a running turn may not keep pinned: status and turn fields are deliberately absent. */
const sourceIdentity = (agent: Agent) => hash(JSON.stringify([agent.id, agent.title, agent.cwd, agent.workspaceId, parent(agent), agent.createdAt, takeoverAdapter(agent)]));
/** The normalized listed settings actually inherited from the source; key order never changes this fingerprint. */
const sourceSettingsFingerprint = (read: ReturnType<typeof sourceFeatureValues>): string =>
  hash(JSON.stringify(Object.entries(read.values ?? {}).sort(([left], [right]) => left < right ? -1 : left > right ? 1 : 0)));
/** Null when the expected settings still match; `null` expected means an explicit replacement owns them instead. */
function sourceSettingsDrift(expected: string | null | undefined, agent: Agent): string | null {
  if (expected == null) return null;
  const read = sourceFeatureValues(agent);
  if (read.problem) return read.problem;
  return sourceSettingsFingerprint(read) === expected ? null
    : "This session's listed model settings changed during its continuity checks. Review it again.";
}
function freshContinuity(receipt: TakeoverContinuity): boolean {
  const age = Date.now() - Date.parse(receipt.observedAt);
  return age >= 0 && age <= 60_000;
}
const takeoverEvidenceSchema = z.object({
  agentId: z.string().min(1), requestId: z.string().uuid(), observedAt: z.iso.datetime(), continuity: continuitySchema,
  sessionRefusal: refusalObservationSchema.nullable(),
}).strict();
const takeoverEvidencePath = (root: string, agentId: string, requestId: string) => join(root, `takeover-evidence-${hash(`${agentId}:${requestId}`)}.json`);
function orchestratedOwnerBindingRevision(owner: Agent): string {
  const labels = owner.labels ?? {};
  return hash(JSON.stringify([labels.orchestration ?? "", seatBinding(labels).seat ?? "", parent(owner) ?? ""]));
}
type WakeRead = { wakes: "clear" | "unknown"; gap?: string; hostId?: string };
/** A live source needs the host's source-scoped reader or a complete, host-qualified public host inventory. */
async function readDirectWakeState(client: TakeoverClient, completeSchedules: CompleteScheduleReader | undefined, agentId: string): Promise<WakeRead> {
  if (client.schedules?.forAgent) {
    try {
      const page = await client.schedules.forAgent(agentId);
      if (!Array.isArray(page?.entries)) return { wakes: "unknown", gap: "The host schedule reader returned no entry array, so no source-linked wake coverage is proven." };
      return page.entries.length
        ? { wakes: "unknown", gap: "The source still has source-linked scheduled wakes; no schedule preservation operation is qualified, so retirement is blocked." }
        : { wakes: "clear" };
    } catch (error) { return { wakes: "unknown", gap: `The host schedule reader failed: ${errorText(error)}. No source-linked wake coverage is proven.` }; }
  }
  if (!completeSchedules) return { wakes: "unknown", gap: "No qualified complete scheduled-wake reader is available on this host: schedules.forAgent is absent and no public host connector is configured." };
  const read = await completeSchedules().catch((error: unknown) => ({ complete: false as const, gap: `The complete schedule reader failed: ${errorText(error)}.` }));
  if (!read.complete) return { wakes: "unknown", gap: read.gap };
  if (typeof read.hostId !== "string" || !read.hostId.trim()) return { wakes: "unknown", gap: "The complete schedule reader named no exact daemon host identity." };
  const malformed = read.schedules.some((schedule) => !schedule || typeof schedule.status !== "string" || !schedule.target
    || ((schedule.target.type === "agent" || schedule.target.type === "self")
      && (typeof schedule.target.agentId !== "string" || !schedule.target.agentId.trim())));
  if (malformed) return { wakes: "unknown", gap: "The complete schedule inventory contains an empty, whitespace or missing target identity, so it is not complete." };
  // Only ongoing duties are wakes; an unrelated or completed schedule must not block another source.
  const ongoing = read.schedules.find((schedule) => (schedule.target.type === "agent" || schedule.target.type === "self")
    && schedule.target.agentId === agentId && schedule.status !== "completed");
  return ongoing
    ? { wakes: "unknown", gap: SCHEDULE_TRANSFER_GAP, hostId: read.hostId }
    : { wakes: "clear", hostId: read.hostId };
}

/** Bounded direct continuity: session-local refusal is persisted; wakes clear only after a qualified schedule read. */
export async function readTakeoverContinuity(client: TakeoverClient, root: string, agentId: string, requestId: string,
  readers: { completeSchedules?: CompleteScheduleReader } = {}): Promise<TakeoverContinuity | null> {
  let agent: Agent | null | undefined;
  try { const found = await client.agents.ref(agentId).refresh(); agent = !found ? null : found.agent.id === agentId ? found.agent : undefined; }
  catch { return null; }
  if (!agent) return null;
  const observedAt = new Date().toISOString(), adapter = takeoverAdapter(agent);
  const mode = agent.currentModeId ?? agent.runtimeInfo?.modeId ?? null;
  const sessionRefusal = sessionLocalRefusalForTakeover(agent, mode, observedAt);
  let continuity: TakeoverContinuity;
  if (adapter.adapter === "governed") {
    continuity = { agentId, requestId, revision: hash(JSON.stringify([lifecycleRevision(agent), "governed"])), observedAt, surface: "governed", wakes: "unknown", owner: null };
  } else if (adapter.adapter === "refused") return null;
  else {
    // An archived, inactive source is history only: it is never prompted or revived by this operation, so no schedule
    // read is required to settle its wakes. A live source always needs a qualified wake read; managed children are
    // preserved independently and are never evidence that schedules are clear.
    const read: WakeRead = agent.archivedAt != null ? { wakes: "clear" } : await readDirectWakeState(client, readers.completeSchedules, agentId);
    let owner: TakeoverContinuity["owner"] = null;
    if (adapter.basis === "orchestrated") {
      const supervisorId = adapter.owner;
      let supervisor: Agent | null | undefined;
      try { const found = await client.agents.ref(supervisorId).refresh(); supervisor = !found ? null : found.agent.id === supervisorId ? found.agent : undefined; }
      catch { return null; }
      if (!supervisor || supervisor.archivedAt != null) return null;
      owner = { agentId: supervisor.id, revision: lifecycleRevision(supervisor), bindingRevision: orchestratedOwnerBindingRevision(supervisor) };
    }
    continuity = {
      agentId, requestId,
      revision: hash(JSON.stringify([lifecycleRevision(agent), adapter.basis, sessionRefusal?.kind ?? "clean", owner?.revision ?? null, read.wakes, read.hostId ?? null])),
      observedAt, surface: "direct", wakes: read.wakes, ...(read.gap ? { wakeGap: read.gap } : {}), ...(read.hostId ? { wakeHost: read.hostId } : {}), owner,
    };
  }
  await writeDurable(takeoverEvidencePath(root, agentId, requestId), takeoverEvidenceSchema.parse({ agentId, requestId, observedAt, continuity, sessionRefusal }));
  return continuity;
}
async function writeDurable(file: string, value: unknown) {
  await mkdir(dirname(file), { recursive: true, mode: 0o700 });
  const temporary = `${file}.${randomUUID()}.tmp`, output = await open(temporary, "wx", 0o600);
  try { await output.writeFile(JSON.stringify(value)); await output.sync(); } finally { await output.close(); }
  await rename(temporary, file);
}
async function readDurable<T>(file: string, schema: z.ZodType<T>): Promise<T | null> {
  try { return schema.parse(JSON.parse(await readFile(file, "utf8"))); }
  catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return null; throw error; }
}
/**
 * What the session's own state forbids, whatever chain or route it would take. Role, provider and archive membership are
 * deliberately absent: every logical seat uses the same operation. Running is reported separately as the missing host
 * stop primitive, and an archived source is replaced from its history without being revived.
 */
function takeoverRefusal(agent: Agent): string | null {
  const labels = agent.labels ?? {};
  if (agent.pendingPermissions.length || agent.attentionReason === "permission") return "Waiting for permission or input. Resolve that in the session first.";
  if (labels["takeover.successor"] || labels["takeover.replaced-by"] || labels["recovery.successor"]) return "Already replaced. Follow the recorded successor.";
  if (!agent.title?.trim()) return "This session has no title to carry over.";
  if (!agent.workspaceId) return "This session has no workspace on this host.";
  if (agent.archivedAt != null && active(agent)) return "The host reports this session archived while it is still executing. Its retirement is contradictory, so no successor is created.";
  return null;
}

/**
 * Review, apply and Check for one session's takeover. Review is reads plus the one fixed resolver read; it issues an
 * opaque digest over everything it pins. Apply takes the review ID, revision and that digest, recomputes the review,
 * refuses any difference, and only then records its intent under the session's fence, archives, and creates the one
 * successor request. An unconfirmed effect is settled by Check, which only reads and never resends.
 */
export function createTakeoverService(client: TakeoverClient, root: string, deps: TakeoverDeps) {
  const file = (prefix: string, id: string) => { if (!/^[a-f0-9-]{36}$/.test(id)) throw new Error("Invalid review ID"); return join(root, `${prefix}-${id}.json`); };
  const intentPath = (agentId: string) => join(root, `intent-${hash(agentId)}.json`);
  const associationPath = (agentId: string) => join(root, `association-${hash(agentId)}.json`);
  const successorMarkerPath = (agentId: string) => join(root, `takeover-successor-${hash(agentId)}.json`);
  const readRecord = (reviewId: string) => readDurable(file("takeover", reviewId), takeoverRecordSchema);
  const readOperation = (reviewId: string) => readDurable(file("takeover-operation", reviewId), takeoverOutcomeSchema);
  const stageOf = (state: TakeoverOutcome["state"], phase: TakeoverOutcome["phase"]): NonNullable<TakeoverOutcome["stage"]> =>
    state === "succeeded" ? "complete"
      : state === "uncertain" ? "uncertain"
      : state === "partial" || state === "refused" ? "failed"
      : phase === "preflight" || phase === "preserve" ? "preparing"
      : phase === "intent" || phase === "stop" || phase === "archive" ? "retiring"
      : phase === "create" || phase === "verify" ? "creating"
      : "delivering";
  const outcome = (record: TakeoverRecord, state: TakeoverOutcome["state"], phase: TakeoverOutcome["phase"], message: string, successor: TakeoverOutcome["successor"], next: TakeoverOutcome["next"]): TakeoverOutcome =>
    ({ reviewId: record.review.reviewId, revision: record.review.revision, agentId: record.review.agentId, state, phase, stage: stageOf(state, phase), message, successor, next, checkedAt: new Date().toISOString() });
  /** Reads the source's durable successor marker; a corrupt or unreadable marker is not permission to duplicate. */
  async function readSuccessorMarker(agentId: string): Promise<{ marker: SuccessorMarker | null } | { problem: string }> {
    try { return { marker: await readDurable(successorMarkerPath(agentId), successorMarkerSchema) }; }
    catch (error) { return { problem: `This session's successor record could not be read: ${errorText(error)}.` }; }
  }
  /** Every native session, archived included, whose own label names this source as its predecessor. */
  async function labelledSuccessors(agentId: string): Promise<Agent[]> {
    const found: Agent[] = [], cursors = new Set<string>();
    let cursor: string | undefined;
    for (let page = 0; page < 1_000; page++) {
      const result = await client.agents.list({ filter: { labels: { [TAKEOVER_PREDECESSOR_LABEL]: agentId }, includeArchived: true }, page: { limit: 200, ...(cursor ? { cursor } : {}) } });
      for (const { agent } of result.entries) if (agent.labels?.[TAKEOVER_PREDECESSOR_LABEL] === agentId) found.push(agent);
      if (!result.pageInfo.hasMore) return found;
      const next = result.pageInfo.nextCursor;
      if (!next || cursors.has(next)) throw new Error("The successor-label inventory is incomplete; no takeover is allowed.");
      cursors.add(next); cursor = next;
    }
    throw new Error("The successor-label inventory exceeded its bounded page walk; no takeover is allowed.");
  }
  /** Records this operation's one successor before any continuation; a different recorded successor is a conflict. */
  async function recordSuccessor(record: TakeoverRecord, spec: SeatCreateSpec, id: string, state: SuccessorMarker["state"]): Promise<{ problem: "unreadable" | "conflict"; message: string } | null> {
    const sourceAgentId = record.review.agentId, current = await readSuccessorMarker(sourceAgentId);
    if ("problem" in current) return { problem: "unreadable", message: current.problem };
    const next: SuccessorMarker = { sourceAgentId, reviewId: record.review.reviewId, requestId: spec.requestId, successorAgentId: id, successorTitle: spec.title, state, updatedAt: new Date().toISOString() };
    if (current.marker && (current.marker.sourceAgentId !== sourceAgentId || current.marker.reviewId !== record.review.reviewId
      || current.marker.requestId !== spec.requestId || current.marker.successorAgentId !== id))
      return { problem: "conflict", message: `This session already records successor ${current.marker.successorAgentId} (“${current.marker.successorTitle}”) from operation ${current.marker.reviewId}. No other successor is created.` };
    if (current.marker && ({ created: 0, succeeded: 1 } as const)[current.marker.state] >= ({ created: 0, succeeded: 1 } as const)[state]) return null;
    try { await writeDurable(successorMarkerPath(sourceAgentId), next); }
    catch (error) { return { problem: "unreadable", message: `This session's successor record could not be written: ${errorText(error)}.` }; }
    return null;
  }
  /** The record, null when the host confirms no such session, undefined when the read failed or named another. */
  async function exact(id: string): Promise<Agent | null | undefined> {
    try { const found = await client.agents.ref(id).refresh(); return !found ? null : found.agent.id === id ? found.agent : undefined; } catch { return undefined; }
  }

  type Assessment = { state: TakeoverReview["state"]; message: string; agent: Agent | null; owner: TakeoverAdapter | null; association: TakeoverReview["association"]; route: TakeoverRoute | null; pinned: Pinned | null; limits: string[] };
  /** Every check a takeover passes before its intent, from exact reads; `pinned` only when all of them pass. */
  async function assess(agentId: string, chosen: string | null, requestId: string, chosenRoute: ExplicitTakeoverRoute | null): Promise<Assessment> {
    let association: TakeoverReview["association"] = { bindingKey: null, source: null, choices: [] }, limits = TAKEOVER_LIMITS;
    let agent: Agent | null = null, owner: TakeoverAdapter | null = null, route: TakeoverRoute | null = null;
    const answer = (state: TakeoverReview["state"], message: string, pinned: Pinned | null = null): Assessment => ({ state, message, agent, owner, association, route, pinned, limits });
    const first = await exact(agentId);
    if (first === undefined) return answer("unknown", "This session could not be read on this host.");
    if (!first) return answer("refused", "This session is not on this host.");
    agent = first;
    // Read the durable successor record before any summary, route or effect. A corrupt record is unknown, never a
    // reason to create another successor.
    const markerRead = await readSuccessorMarker(agentId);
    if ("problem" in markerRead) return answer("unknown", markerRead.problem);
    if (markerRead.marker) return answer("refused", `This session already has a recorded successor ${markerRead.marker.successorAgentId} (“${markerRead.marker.successorTitle}”) from operation ${markerRead.marker.reviewId}. Take over does not create another successor; follow that session or its Check.`);
    // Fallback for a process that died after creation and before the marker: the successor's own lineage label is exact.
    let labelled: Agent[];
    try { labelled = await labelledSuccessors(agentId); }
    catch (error) { return answer("unknown", `This session's successor-label check failed: ${errorText(error)}. Nothing is taken over.`); }
    if (labelled.length) {
      const [successor] = labelled;
      return answer("refused", `This session already has a recorded successor ${successor!.id} (“${successor!.title ?? successor!.id}”) whose ${TAKEOVER_PREDECESSOR_LABEL} label names it. Take over does not create another successor; follow that session or its Check.`);
    }
    const promotion = await promotionBindingFor(deps.promotionRoot, agentId, first.labels?.epic ?? null);
    if (promotion.state === "blocked") return answer("unknown", promotion.message);
    if (promotion.state === "settled") {
      const ownerProblem = await canonicalBindingProblem(client, deps.promotionRoot, promotion.record.epic, agentId);
      if (ownerProblem) return answer("unknown", `The settled promotion binding does not match its native owner: ${ownerProblem}`);
    }
    const direct = owner = takeoverAdapter(first);
    if (direct.adapter === "governed") return answer("refused", GOVERNED_ADAPTER_GAP);
    if (direct.adapter === "refused") return answer("refused", direct.reason);
    const refusal = takeoverRefusal(first);
    if (refusal) return answer("refused", refusal);
    // Running is eligible only through the server-owned stop adapter; it is not a role or lifecycle-state prohibition.
    if (active(first) && !deps.publicCli) return answer("refused", RUNNING_STOP_GAP);
    if (active(first)) limits = [...limits, RUNNING_STOP_DISCLOSURE];
    // A governed seat whose labels were removed still sits in a Kontor placement; no kontord read is made to ask.
    if ([deps.config.stateRoot, ...deps.config.placementRoots].some((placement) => within(resolve(first.cwd), resolve(placement))))
      return answer("refused", "This session runs in a Kontor placement without its seat labels, so governed ownership cannot be ruled out. Nothing is taken over here.");
    let workspace;
    try { workspace = await client.workspaces.ref(first.workspaceId!).refresh(); } catch (error) { return answer("unknown", `Its workspace could not be read: ${errorText(error)}.`); }
    if (!workspace || workspace.id !== first.workspaceId || workspace.archivingAt || !workspace.workspaceDirectory) return answer("refused", "Its exact workspace is no longer available on this host.");
    if (workspace.workspaceDirectory !== first.cwd) return answer("refused", CWD_DRIFT);
    const sourceSettings = sourceFeatureValues(first);
    let children: Agent[];
    try { children = childrenOf(first, await inventory(client)); }
    catch (error) { return answer("unknown", errorText(error)); }
    if (children.length) limits = [...limits, CHILD_LIMIT];
    let ownerRevision: string | null = null, created: string | null = null;
    if (direct.basis === "orchestrated") {
      const supervisor = await exact(direct.owner);
      if (supervisor === undefined) return answer("unknown", "Its owning session could not be read on this host.");
      if (!supervisor || supervisor.archivedAt != null) return answer("refused", "Its owning session is not active on this host, so its supervisor cannot be kept.");
      ownerRevision = lifecycleRevision(supervisor);
    }
    if (direct.createRequest) {
      let record;
      try { record = await deps.create.record(direct.createRequest); } catch (error) { return answer("unknown", `Its creation record could not be read: ${errorText(error)}.`); }
      if (record?.request.requestId !== direct.createRequest || record.outcome?.requestId !== direct.createRequest || record.outcome.agentId !== agentId || record.outcome.state !== "verified") return answer("refused", "Its creation record on this host does not name this session as verified, so its provenance cannot be verified.");
      if (readbackDifferences(first, record.request).length) return answer("refused", "This session no longer matches its verified creation record. Its owner must reconcile that before takeover.");
      created = hash(JSON.stringify(record));
    }
    // Auto follows the recorded/chosen configured chain. An explicit model is validated against the host snapshot.
    let recorded: string | null = null, choices: ProfileChoice[] | null = null, fleetCatalog: ConfiguredAccountCatalog | null = null;
    if (chosenRoute) {
      try { recorded = (await readDurable(associationPath(agentId), associationSchema))?.bindingKey ?? null; } catch { recorded = null; }
      association = { bindingKey: recorded, source: recorded ? "recorded" : null, choices: [] };
    } else {
      try { recorded = (await readDurable(associationPath(agentId), associationSchema))?.bindingKey ?? null; } catch (error) { return answer("unknown", `Its recorded chain could not be read: ${errorText(error)}.`); }
      try {
        const discovery = await deps.operatorFleetDiscovery();
        choices = discovery.choices;
        fleetCatalog = discovery.catalog;
      } catch (error) {
        if (!recorded || chosen) return answer("unknown", `The configured profiles could not be read: ${errorText(error)}.`);
      }
      association = { bindingKey: recorded, source: recorded ? "recorded" : null, choices: choices ?? [] };
      if (chosen) {
        if (!choices!.some((choice) => choice.bindingKey === chosen)) return answer("refused", "That profile is not one this host offers.");
        association = { ...association, bindingKey: chosen, source: "chosen" };
      } else if (!recorded) {
        return choices!.length ? answer("choose", "Choose the configured profile whose chain takes this session over, or choose a model for this takeover. A title, role or model is never used to infer a chain.")
          : answer("refused", "No configured profile is offered on this host and none is recorded for this session, so no chain can take it over. Choose a model instead.");
      }
    }
    const mode = first.currentModeId ?? first.runtimeInfo?.modeId ?? null;
    if (!mode) return answer("unknown", "This session's permission mode cannot be read, so a successor's permission boundary cannot be confirmed.");
    const routeEvidenceAt = new Date().toISOString();
    const sessionRefusal = sessionLocalRefusalForTakeover(first, mode, routeEvidenceAt);
    let health: RouteHealth;
    let evidence: string;
    let selection: "auto" | "explicit" = "auto";
    let featureValues: ExplicitTakeoverRoute["featureValues"];
    let inheritedSettings: string | null = null;
    if (chosenRoute) {
      selection = "explicit";
      let snapshot: ProviderRouteSnapshot;
      try { snapshot = await deps.snapshot({ cwd: first.cwd }); } catch (error) { return answer("unknown", `The host provider snapshot could not be read: ${errorText(error)}. Nothing is chosen.`); }
      const accounts = snapshot.entries.map((entry) => entry.provider);
      const catalog = configuredAccountCatalogFromFleet([{ accounts }], `${snapshot.requestId}@${snapshot.generatedAt}`);
      if (!catalog) return answer("unknown", "The host provider snapshot is unqualified: it names no providers or a duplicate provider. Nothing is chosen.");
      health = joinSessionLocalRefusalToRouteHealth(routeHealthFromProviderSnapshot(snapshot, mode, catalog), sessionRefusal, mode);
      const explicit = explicitTakeoverRoute(health, chosenRoute, mode);
      if ("problem" in explicit) return answer("refused", explicit.problem);
      route = explicit.route;
      if (explicit.featureValues) {
        featureValues = explicit.featureValues;
      } else {
        if (sourceSettings.problem) return answer("unknown", sourceSettings.problem);
        featureValues = sourceSettings.values ?? undefined;
        inheritedSettings = sourceSettingsFingerprint(sourceSettings);
      }
      evidence = route.digest!;
    } else {
      if (sourceSettings.problem) return answer("unknown", sourceSettings.problem);
      featureValues = sourceSettings.values ?? undefined;
      inheritedSettings = sourceSettingsFingerprint(sourceSettings);
      if (!fleetCatalog) return answer("unknown", "The configured account catalog could not be read: the operator fleet source names no verifiable configured account aliases.");
      try { health = await Promise.resolve(deps.evidence({ mode, cwd: first.cwd, catalog: fleetCatalog })); }
      catch (error) { return answer("unknown", `Provider route evidence could not be read: ${errorText(error)}.`); }
      health = joinSessionLocalRefusalToRouteHealth(health, sessionRefusal, mode);
      if (!health.observations.length) limits = [...limits, UNOBSERVED_LIMIT];
      route = await resolveTakeoverRoute(deps.config, direct, { agentId, bindingKey: association.bindingKey }, health, deps.run);
      if (route.state !== "selected" || !route.selected || !route.provenance || !route.validity) return answer(route.state === "selected" ? "unknown" : route.state, route.message);
      evidence = routeEvidenceDigest(health, sessionRefusal, fleetCatalog.revision);
    }
    if (route.state !== "selected" || !route.selected || !route.provenance || !route.validity) return answer(route.state === "selected" ? "unknown" : route.state, route.message);
    const selected = route.selected, labels = first.labels ?? {};
    let persona: { text: string; supported: boolean; version: string } | null = null;
    if (labels.persona !== undefined) {
      const issued = reissuePersona(labels, selected.account, { desk: workspace.projectCustomName ?? workspace.projectDisplayName ?? "", workspace: (workspace.title ?? workspace.name ?? "").trim(), workspaceId: workspace.id });
      if ("refused" in issued) return answer("refused", issued.refused);
      if (!issued.supported) return answer("refused", "This route only sends the role card as a bootstrap message. Instruction delivery and acceptance are not qualified for that profile, so this session cannot be taken over on it.");
      persona = issued;
    }
    if (direct.basis === "orchestrated") limits = [...limits, SUPERVISOR_LIMIT];
    // A running source cannot produce a quiescent final summary. Review pins route/context and a truthful preview;
    // confirmed apply stops it, then captures and saves the final handoff before any archive or creation.
    const runningAtRead = active(first);
    let saved: SummaryRevision | null = null;
    if (!runningAtRead) {
      try { saved = await deps.summaries.save(agentId); } catch (error) { return answer("unknown", `Its handoff summary could not be generated: ${errorText(error)}. The capture is retained and this operation is not started.`); }
      if (!saved.text.trim()) return answer("refused", "The saved handoff summary is empty, so a successor would start without its context.");
    }
    let continuity: TakeoverContinuity;
    try {
      const receipt = continuitySchema.safeParse(await deps.continuity?.(agentId, requestId));
      if (!receipt.success) return answer("unknown", "No qualified owner and wake-continuity receipt can be read on this host. Nothing is retired.");
      continuity = receipt.data;
    } catch (error) { return answer("unknown", `Its continuity receipt could not be read: ${errorText(error)}. Nothing is retired.`); }
    if (continuity.surface === "governed") return answer("refused", GOVERNED_ADAPTER_GAP);
    if (continuity.surface !== "direct") return answer("unknown", "Its continuity source cannot confirm direct ownership. Nothing is retired.");
    // Managed children are preserved by detach; they are never evidence that a live source's schedules are clear.
    if (continuity.wakes !== "clear") return answer("unknown", continuity.wakeGap ?? "Its wake-continuity receipt is not settled. Nothing is retired.");
    if (continuity.agentId !== agentId || continuity.requestId !== requestId || !freshContinuity(continuity))
      return answer("unknown", "Its wake-continuity receipt is stale or names another session or successor request. Nothing is retired.");
    if (direct.basis === "orchestrated" ? continuity.owner?.agentId !== direct.owner || continuity.owner.revision !== ownerRevision : continuity.owner !== null)
      return answer("refused", "Its continuity receipt does not confirm its current owner and binding revision. Nothing is retired.");
    const carried = Object.fromEntries(CARRIED_LABELS.flatMap((key) => labels[key] === undefined ? [] : [[key, labels[key]!]]));
    const recovered = promotion.state === "settled" ? promotion.record.epic : null;
    const bindingContext = recovered ? `\n\nKontor recovered canonical epic binding: ${recovered}. Treat this successor as the epic's canonical LSA; the promoted native origin is retained.` : "";
    const summaryPin: SummaryPin | null = saved ? { summaryId: saved.summaryId, revision: saved.revision } : null;
    const spec: SeatCreateSpec = {
      requestId, title: first.title!, provider: selected.account, model: selected.model, effort: selected.effort, modeId: mode,
      ...(featureValues && Object.keys(featureValues).length ? { featureValues } : {}),
      labels: { ...carried, ...(recovered ? { epic: recovered } : {}), ...(parent(first) ? { "paseo.parent-agent-id": parent(first)! } : {}), ...(persona ? { persona: persona.version } : {}), [CREATE_REQUEST_LABEL]: requestId, [TAKEOVER_PREDECESSOR_LABEL]: agentId },
      workspaceId: workspace.id, anchorWorkspaceId: workspace.id, workspaceName: (workspace.title ?? workspace.name ?? "").trim(), cwd: first.cwd,
      expectedPlacement: { workspaceId: workspace.id, cwd: first.cwd },
      effectValidity: { route: route.validity, continuity: { observedAt: continuity.observedAt, validUntil: new Date(Date.parse(continuity.observedAt) + 60_000).toISOString() } },
      createWorkspace: null, revalidate: null,
      // A plain session stays plain: no persona, no system prompt.
      persona: persona ? { text: persona.text, supported: persona.supported } : { text: "", supported: false },
      bootstrap: bootstrapPrompt(persona?.text ?? "", saved?.text ?? RUNNING_SUMMARY_PREVIEW, persona?.supported ?? true) + bindingContext,
    };
    let expired: string | null;
    const checked = await deps.create.preflight(spec, agentId);
    if ((expired = effectValidityProblem(spec))) return answer("unknown", expired);
    if ("problem" in checked) return answer("refused", checked.problem);
    if (checked.cwd !== first.cwd) return answer("refused", CWD_DRIFT);
    try {
      const now = childrenOf(first, await inventory(client));
      if ((expired = effectValidityProblem(spec))) return answer("unknown", expired);
      if (!isDeepStrictEqual(now.map((child) => child.id).sort(), children.map((child) => child.id).sort()))
        return answer("refused", "Managed children changed during preflight. Nothing is retired; review again.");
      if (direct.basis === "orchestrated") {
        const supervisor = await exact(direct.owner);
        if ((expired = effectValidityProblem(spec))) return answer("unknown", expired);
        if (!supervisor || supervisor.archivedAt != null || lifecycleRevision(supervisor) !== ownerRevision) return answer("unknown", "Its owner changed during preflight. Nothing is retired.");
      }
    } catch (error) { return answer("unknown", `The final continuity check failed: ${errorText(error)}. Nothing is retired.`); }
    // Reads above (a history read can load the provider runtime) may move the record; pin the state they leave.
    const last = await exact(agentId);
    if ((expired = effectValidityProblem(spec))) return answer("unknown", expired);
    if (!last) return answer("unknown", "This session could not be read again after its checks.");
    const settingsDrift = sourceSettingsDrift(inheritedSettings, last);
    if (settingsDrift) return answer("unknown", settingsDrift);
    const again = takeoverRefusal(last);
    if (again) return answer("refused", again);
    const running = runningAtRead || active(last);
    if (running && !deps.publicCli) return answer("refused", RUNNING_STOP_GAP);
    if (running && !limits.includes(RUNNING_STOP_DISCLOSURE)) limits = [...limits, RUNNING_STOP_DISCLOSURE];
    if (sourceIdentity(last) !== sourceIdentity(first)
      || (!active(first) && !running && takeoverRevision(last) !== takeoverRevision(first)))
      return answer("unknown", "This session's title, checkout, workspace or owner changed during its continuity checks. Review it again; nothing is retired.");
    return answer("ready", selection === "explicit"
      ? route.message
      : `Takes over on ${selected.account}/${selected.model}, the first route in chain ${route.provenance.chain} with fresh positive evidence.`, {
      predecessor: takeoverRevision(last), owner: direct, ownerRevision, created, bindingKey: selection === "explicit" ? "explicit" : association.bindingKey!,
      route: { provenance: route.provenance, selected: route.selected, considered: route.considered, validity: route.validity },
      evidence, continuity, spec, retiredSource: first.archivedAt != null, children: children.map((child) => child.id).sort(), summary: summaryPin, selection,
      running, sourceIdentity: sourceIdentity(last), sourceSettings: inheritedSettings,
    });
  }

  async function review(input: TakeoverReviewInput): Promise<TakeoverReview> {
    const prior = input.reviewId ? await readRecord(input.reviewId) : null;
    if (input.reviewId && (!prior || prior.review.agentId !== input.agentId || prior.review.revision !== input.revision)) throw new Error("This is not the current review of this session on this host. Review it again.");
    if (prior && await readOperation(prior.review.reviewId)) throw new Error("This review was already confirmed. Check its outcome instead.");
    // One durable operation/request ID: the review, its fence and the one createSeat request share this identity.
    const reviewId = prior?.review.reviewId ?? randomUUID(), revision = (prior?.review.revision ?? 0) + 1, requestId = reviewId;
    const chosen = input.bindingKey ?? (input.route !== undefined ? null : prior?.chosen ?? null);
    const priorRoute = explicitTakeoverRouteSchema.safeParse(prior?.chosenRoute ?? null);
    // An explicit null in this request asks for Auto; an omitted route keeps the review's earlier choice.
    const chosenRoute = input.route === null ? null : input.route ?? (priorRoute.success ? priorRoute.data : null);
    const found = await assess(input.agentId, chosen, requestId, chosenRoute), agent = found.agent, spec = found.pinned?.spec ?? null;
    const nonce = found.pinned ? randomBytes(32).toString("hex") : null;
    const view: TakeoverReview = {
      reviewId, revision, agentId: input.agentId, checkedAt: new Date().toISOString(), state: found.state, message: found.message,
      predecessor: agent && { title: agent.title ?? agent.id, workspaceId: agent.workspaceId ?? null, cwd: agent.cwd, provider: agent.provider, model: agent.runtimeInfo?.model ?? agent.model ?? null,
        effort: agent.effectiveThinkingOptionId ?? agent.runtimeInfo?.thinkingOptionId ?? null, mode: agent.currentModeId ?? agent.runtimeInfo?.modeId ?? null, status: agent.status },
      ownership: found.owner?.adapter === "direct" ? found.owner.basis : found.owner?.adapter === "governed" ? "governed" : null,
      association: found.association,
      route: found.route?.provenance ? { ...found.route.provenance, selected: found.route.selected, considered: found.route.considered } : null,
      successor: spec && { requestId, title: spec.title, workspaceId: spec.workspaceId!, cwd: spec.cwd, provider: spec.provider, model: spec.model, effort: spec.effort, mode: spec.modeId, persona: spec.labels.persona ?? null },
      limits: found.limits,
      digest: null,
    };
    if (nonce) view.digest = seal(nonce, view, found.pinned);
    await writeDurable(file("takeover", reviewId), { review: view, requestId, chosen, chosenRoute, pinned: found.pinned, nonce });
    return view;
  }

  /** Null once an exact read shows the predecessor retired. An already archived source is history only: never revived. */
  async function retirement(record: TakeoverRecord, agentId: string, reply: unknown, retiredSource = false): Promise<TakeoverOutcome | null> {
    if (retiredSource) {
      const after = await exact(agentId);
      if (after && archivedMembership(after) && !after.pendingPermissions.length) return null;
      if (after && archivedMembership(after)) return outcome(record, "partial", "archive", "The source is archived while an interaction still waits. No successor was created; resolve it in the host first.", null, "check");
      if (after?.archivedAt != null) return outcome(record, "partial", "archive", "The host reports the source archived while it is still executing. Its retirement is contradictory, so no successor was created.", null, "check");
      if (after) return outcome(record, "uncertain", "archive", "The source no longer reads as archived. Its stored history is not used and no successor was created; nothing was revived.", null, "check");
      return outcome(record, "uncertain", "archive", "The archived source cannot be read back. Check reads it again; nothing is resent.", null, "check");
    }
    let after: Agent | null | undefined;
    for (let attempt = 0; attempt < 3; attempt++) {
      if (attempt) await new Promise((wake) => setTimeout(wake, deps.settleMs ?? 250));
      after = await exact(agentId);
      if (after && archivedMembership(after) && after.status === "closed" && !active(after) && !after.pendingPermissions.length) return null;
    }
    if (after && archivedMembership(after)) return outcome(record, "partial", "archive", "Archived, but its runtime is not confirmed stopped, so no successor was created. Check reads it again.", null, "check");
    return outcome(record, "uncertain", "archive", `The archive is not confirmed${reply ? ` (${errorText(reply)})` : ""}. Check reads it again; nothing is resent.`, null, "check");
  }
  /** The predecessor must still be retired before any successor is created or its continuation is released. */
  function predecessorConflict(agent: Agent | null | undefined): string | null {
    if (!agent) return "The predecessor could not be read. Its retirement is not proven.";
    if (!archivedMembership(agent)) return "The predecessor is not archived any more; it may have been revived. No successor task is released.";
    if (active(agent)) return "The predecessor is executing after retirement. No successor task is released.";
    if (agent.pendingPermissions.length) return "The predecessor is waiting for input after retirement. No successor task is released.";
    return null;
  }
  /** What the successor's readback leaves unverified; null when its route, checkout, supervisor, persona and summary match. */
  async function verify(spec: SeatCreateSpec, id: string, predecessor: string, beforeRelease = true): Promise<string | null> {
    if (id === predecessor) return "it reads back as the predecessor";
    const agent = await exact(id);
    if (!agent) return "it could not be read back";
    if (agent.archivedAt != null || (beforeRelease && (active(agent) || agent.pendingPermissions.length))) return "it is archived, executing or waiting for input before task release";
    if (readbackDifferences(agent, { ...spec, workspaceId: spec.workspaceId!, at: "" }).length) return "its title, route, permission mode or creation labels differ from the reviewed request";
    if (agent.cwd !== spec.cwd || agent.workspaceId !== spec.workspaceId) return "its checkout differs from the predecessor's";
    if (parent(agent) !== (spec.labels["paseo.parent-agent-id"] ?? null)) return "its supervisor differs from the predecessor's";
    if ((agent.labels?.persona ?? null) !== (spec.labels.persona ?? null)) return "its persona revision differs";
    const page = await client.agents.ref(id).timeline.refetch({ direction: "tail", limit: 20 }).catch(() => null);
    if (page && "error" in page && page.error) return "its handoff summary read failed";
    return page?.entries.some((entry) => entry.item.type === "user_message" && entry.item.text === spec.bootstrap) ? null : "its handoff summary does not read back";
  }
  /** Null when the successor is created and verified; otherwise the outcome that stops before its task is released. */
  async function afterCreate(record: TakeoverRecord, spec: SeatCreateSpec, created: CreateOutcome): Promise<TakeoverOutcome | null> {
    const named = { requestId: spec.requestId, agentId: created.agentId, title: spec.title };
    // Record the one successor identity before any verification, promotion or continuation work.
    if (created.agentId) {
      const recorded = await recordSuccessor(record, spec, created.agentId, "created");
      if (recorded) return outcome(record, recorded.problem === "conflict" ? "partial" : "uncertain", "create",
        `${recorded.message} Its task was not released and no other successor is created.`, named, recorded.problem === "conflict" ? "none" : "check");
    }
    if (created.state === "refused") return outcome(record, "partial", "create", `The session was retired, but its successor was refused: ${created.message}`, named, "none");
    if (created.state === "unknown") return outcome(record, "uncertain", "create", `The session was retired; its successor's creation is not confirmed. ${created.message}`, named, "check");
    if (created.state === "differences") return outcome(record, "partial", "create", `Successor ${created.agentId} was created with differences, so its task was not released. ${created.differences.join(" ")}`, named, "none");
    if (!created.agentId) return outcome(record, "uncertain", "create", "The creation result names no exact successor. Check reads the same request; nothing is resent.", named, "check");
    const problem = await verify(spec, created.agentId, record.review.agentId);
    if (problem) return outcome(record, "partial", "verify", `Successor ${created.agentId} was created, but ${problem}, so its task was not released.`, named, "none");
    const promotion = await promotionBindingFor(deps.promotionRoot, record.review.agentId, spec.labels.epic ?? null);
    if (promotion.state === "blocked") return outcome(record, "partial", "verify", `Successor ${created.agentId} was created, but ${promotion.message} Its task was not released.`, named, "none");
    if (promotion.state === "none") return null;
    const successor = await placement(client, created.agentId).catch(() => null);
    if (!successor) return outcome(record, "partial", "verify", `Successor ${created.agentId} was created, but its exact placement could not be read back, so its task was not released.`, named, "none");
    const transfer = await transferPromotion(deps.promotionRoot, record.review.agentId, { agentId: created.agentId, native: successor.native,
      persona: successor.agent.labels?.persona ?? "" }, promotion.record.epic);
    return transfer ? outcome(record, "partial", "verify", `Successor ${created.agentId} was created, but ${transfer} Its task was not released.`, named, "none") : null;
  }
  /** True when this exact operation's continuation prompt is visible on the successor's timeline. */
  async function observedRelease(id: string, wanted: string): Promise<boolean> {
    const page = await client.agents.ref(id).timeline.refetch({ direction: "tail", limit: 50 }).catch(() => null);
    return !!page && !("error" in page && page.error) && page.entries.some(({ item }) => item.type === "user_message" && (item.messageId === wanted || item.clientMessageId === wanted));
  }
  /**
   * Delivers this operation's one continuation with its stable message ID. A visible message is never sent again; an
   * absent one is sent once and then observed. Check may call this after a lost create or release reply, but it never
   * creates or archives anything. The release intent is durable before the send.
   */
  async function deliver(record: TakeoverRecord, spec: SeatCreateSpec, id: string, observeAfterError = true): Promise<TakeoverOutcome> {
    const wanted = releaseMessage(record.review.reviewId), named = { requestId: spec.requestId, agentId: id, title: spec.title };
    if (await observedRelease(id, wanted)) return succeeded(record, spec, id);
    // Shared delivery boundary: the exact predecessor must still be retired immediately before the initial send, and
    // before any Check-driven retry. A revived or running source retains the created successor but receives no task.
    const conflict = predecessorConflict(await exact(record.review.agentId));
    if (conflict) return outcome(record, "partial", "release", `${conflict} Successor ${id} is retained, but its task was not released and no other successor is created.`, named, "none");
    const sent = await client.agents.ref(id).send(TAKEOVER_PROMPT, { messageId: wanted }).then(() => null, (error: unknown) => error ?? "rejected");
    if (sent && !observeAfterError) return outcome(record, "uncertain", "release", `Successor “${spec.title}” (${id}) is verified, but its task release reply is not confirmed (${errorText(sent)}). Check reads it; nothing else is created and the same message ID is used if it must be sent.`, named, "check");
    for (let attempt = 0; attempt < 3; attempt++) {
      if (attempt) await new Promise((wake) => setTimeout(wake, deps.settleMs ?? 250));
      if (await observedRelease(id, wanted)) return succeeded(record, spec, id);
    }
    return outcome(record, "uncertain", "release", sent
      ? `Successor “${spec.title}” (${id}) is verified, but its task release reply is not confirmed (${errorText(sent)}). Check reads it; nothing else is created and the same message ID is used if it must be sent.`
      : `Successor “${spec.title}” (${id}) is verified and its task prompt was accepted, but no delivery is observed on its timeline yet. Queued delivery is not continuation. Check reads it; nothing else is created.`, named, "check");
  }
  async function succeeded(record: TakeoverRecord, spec: SeatCreateSpec, id: string): Promise<TakeoverOutcome> {
    const named = { requestId: spec.requestId, agentId: id, title: spec.title };
    // The durable source->successor record settles before the operation does; a conflict is never overwritten.
    const recorded = await recordSuccessor(record, spec, id, "succeeded");
    if (recorded) return outcome(record, recorded.problem === "conflict" ? "partial" : "uncertain", "release",
      `${recorded.message} The continuation is observed, but the operation is not settled as this successor.`, named, recorded.problem === "conflict" ? "none" : "check");
    // An Auto successor's chain is recorded for its exact ID; an explicit choice is this operation only.
    if ((record.pinned as Pinned).selection === "auto")
      await writeDurable(associationPath(id), { agentId: id, bindingKey: (record.pinned as Pinned).bindingKey, reviewId: record.review.reviewId }).catch(() => {});
    return outcome(record, "succeeded", "done", `Taken over by “${spec.title}” (${id}) on ${spec.provider}/${spec.model}.`, named, "none");
  }

  /** Stable source, children, owner, route and request match across a stop; summary and continuity are deliberately refreshed. */
  function stoppedSourceMatches(reviewed: Pinned, current: Pinned): boolean {
    if (reviewed.sourceIdentity !== current.sourceIdentity || !isDeepStrictEqual(reviewed.children, current.children)) return false;
    // The running compare normalizes the mutable status revision; the schedule host must still be the exact pinned host.
    if ((reviewed.continuity.wakeHost ?? null) !== (current.continuity.wakeHost ?? null)) return false;
    const normalized = {
      ...reviewed, predecessor: current.predecessor,
      continuity: { ...reviewed.continuity, revision: current.continuity.revision },
      spec: { ...reviewed.spec, bootstrap: current.spec.bootstrap },
    };
    return takeoverPinnedMatches(normalized, current);
  }
  /** Stops the one exact source, reads it back on the same host, and takes the final summary/continuity before archive. */
  async function stopAndRefresh(record: TakeoverRecord, reviewed: Pinned): Promise<{ record: TakeoverRecord; pinned: Pinned; spec: SeatCreateSpec } | TakeoverOutcome> {
    const agentId = record.review.agentId, named = { requestId: reviewed.spec.requestId, agentId: null, title: reviewed.spec.title };
    if (!deps.publicCli) return outcome(record, "refused", "stop", `${RUNNING_STOP_GAP} Nothing was archived or created.`, named, "review");
    const stopped = await deps.publicCli.stop(agentId);
    let after: Agent | null | undefined;
    for (let attempt = 0; attempt < 3; attempt++) {
      if (attempt) await new Promise((wake) => setTimeout(wake, deps.settleMs ?? 250));
      after = await exact(agentId);
      if (after && !active(after)) break;
    }
    if (!after) return outcome(record, "uncertain", "stop", "The stop was sent, but the same source cannot be read back on this host. Nothing was archived or created; Check reads it again.", named, "check");
    if (after.archivedAt != null) return outcome(record, "refused", "stop", "The source was archived while its stop was being checked. No successor was created; review again.", named, "review");
    if (active(after)) return outcome(record, "refused", "stop",
      `${stopped.ok ? "The public Paseo CLI returned, but the source is still running" : stopped.message} Nothing was archived or created. Review it again after the turn ends.`, named, "review");
    const stoppedSettings = sourceSettingsDrift(reviewed.sourceSettings, after);
    if (stoppedSettings) return outcome(record, "refused", "stop",
      `${stoppedSettings} Nothing was archived or created; review again.`, named, "review");
    if (sourceIdentity(after) !== reviewed.sourceIdentity) return outcome(record, "refused", "stop",
      "The stopped source's title, checkout, workspace or owner changed. Nothing was archived or created; review again.", named, "review");
    const priorRoute = explicitTakeoverRouteSchema.safeParse(record.chosenRoute ?? null);
    let now: Assessment;
    try { now = await assess(agentId, record.chosen, record.requestId, priorRoute.success ? priorRoute.data : null); }
    catch (error) { return outcome(record, "uncertain", "stop", `The stopped source's final review failed: ${errorText(error)}. Nothing was archived or created; Check reads it again.`, named, "check"); }
    if (!now.pinned) return outcome(record, now.state === "unknown" ? "uncertain" : "refused", "stop",
      `${now.message} Nothing was archived or created.`, named, now.state === "unknown" ? "check" : "review");
    if (now.pinned.running || !stoppedSourceMatches(reviewed, now.pinned)) return outcome(record, "refused", "stop",
      "The stopped source's title, children, owner, chain or route no longer matches the review. Nothing was archived or created; review again.", named, "review");
    const final = { ...record, pinned: now.pinned };
    try { await writeDurable(file("takeover-final", record.review.reviewId), final); }
    catch (error) { return outcome(record, "uncertain", "stop", `The stopped source's final handoff could not be recorded: ${errorText(error)}. Nothing was archived or created; Check reads it again.`, named, "check"); }
    return { record: final, pinned: now.pinned, spec: now.pinned.spec };
  }
  async function apply(input: { reviewId: string; revision: number; digest: string }): Promise<TakeoverOutcome> {
    const original = await readRecord(input.reviewId);
    if (!original) throw new Error("This review is not on this host. Review the session again.");
    const agentId = original.review.agentId;
    let record: TakeoverRecord = original, pinned = original.pinned as Pinned;
    // Refusals before the fence is held are not recorded, so a wrong confirmation cannot spend the review.
    const refuse = (message: string) => outcome(record, "refused", "preflight", `${message} Nothing was done.`, null, "review");
    if (input.revision !== original.review.revision) return refuse("A newer revision of this review exists; confirm that one.");
    if (!original.nonce || !original.review.digest || !sameDigest(input.digest, original.review.digest) || seal(original.nonce, original.review, pinned) !== original.review.digest) return refuse("This confirmation does not match a ready review.");
    // A repeated valid confirmation answers from its operation and never dispatches again.
    const done = await readOperation(input.reviewId);
    if (done) return done;
    // A stopped running source has a refreshed final pin; the reviewed fingerprint still owns the fence.
    const reviewedPinned = pinned;
    let final: TakeoverRecord | null = null;
    try { final = await readDurable(file("takeover-final", input.reviewId), takeoverRecordSchema); }
    catch { return refuse("The refreshed final takeover record is unreadable; nothing was done. Review again."); }
    if (final) {
      const refreshed = final.pinned as Pinned | null;
      if (!refreshed) return refuse("The refreshed final takeover record is unreadable; nothing was done. Review again.");
      record = final; pinned = refreshed;
    }
    const lockPath = intentPath(agentId);
    let lock;
    try { lock = await open(lockPath, "wx", 0o600); }
    catch (error) { if (!(error instanceof Error && "code" in error && error.code === "EEXIST")) throw error; return refuse("Another action holds this session."); }
    const token = randomUUID(), fence = (phase: string) => JSON.stringify({ reviewId: record.review.reviewId, agentId, action: "takeover", phase, fingerprint: reviewedPinned.predecessor, token });
    let ours = fence("preflight");
    await lock.writeFile(ours); await lock.sync(); await lock.close();
    /** Records the operation while this call still holds the fence; a settled outcome releases it, any other keeps it. */
    const write = async (result: TakeoverOutcome) => {
      const next = fence(result.phase);
      const kept = await whileOwned(lockPath, ours, async () => {
        await writeDurable(file("takeover-operation", record.review.reviewId), result);
        if (settledTakeover(result)) await unlink(lockPath); else await writeFile(lockPath, next, { mode: 0o600 });
      });
      if (kept) ours = next;
      return kept;
    };
    const finish = async (result: TakeoverOutcome) => await write(result) ? result : { ...result, message: `${result.message} This session's fence changed meanwhile, so this result was not recorded.` };
    let spec = pinned.spec;
    const named = { requestId: spec.requestId, agentId: null, title: spec.title };
    try {
      const priorRoute = explicitTakeoverRouteSchema.safeParse(record.chosenRoute ?? null);
      const now = await assess(agentId, record.chosen, record.requestId, priorRoute.success ? priorRoute.data : null);
      const sameContext = now.pinned && now.pinned.running === pinned.running
        && (pinned.running
          ? stoppedSourceMatches(pinned, now.pinned)
          : isDeepStrictEqual(now.pinned.children, pinned.children) && now.pinned.retiredSource === pinned.retiredSource
            && now.pinned.selection === pinned.selection && isDeepStrictEqual(now.pinned.summary, pinned.summary)
            && takeoverPinnedMatches(pinned, now.pinned));
      if (!now.pinned || !sameContext)
        return finish(outcome(record, "refused", "preflight", `${now.pinned ? "This session, its children, its owner, its chain or its route evidence changed since review." : now.message} Nothing was done; review again.`, null, "review"));
      spec = { ...spec, effectValidity: now.pinned.spec.effectValidity };
      if (!await write(outcome(record, "uncertain", "intent", "Takeover intent recorded before any effect. If nothing follows, Check reads the outcome; nothing is resent.", named, "check")))
        return refuse("Another action took this session's fence first.");
      const latest = await exact(agentId);
      if (!latest) return finish(outcome(record, "refused", "intent", "This session changed during preflight. Nothing was done; review again.", null, "review"));
      const latestSettings = sourceSettingsDrift(pinned.sourceSettings, latest);
      if (latestSettings) return finish(outcome(record, "refused", "intent", `${latestSettings} Nothing was done; review again.`, null, "review"));
      if (pinned.running ? sourceIdentity(latest) !== pinned.sourceIdentity : takeoverRevision(latest) !== pinned.predecessor)
        return finish(outcome(record, "refused", "intent", "This session changed during preflight. Nothing was done; review again.", null, "review"));
      const placement = await expectedPlacementProblem(client, spec);
      if (placement) return finish(outcome(record, "refused", "preflight", placement, null, "review"));
      // No await between this freshness check and archive dispatch: preflight and intent persistence consume time.
      const expired = effectValidityProblem(spec);
      if (expired) return finish(outcome(record, "refused", "preflight", expired, null, "review"));
    } catch (error) { return finish(outcome(record, "refused", "preflight", `Preflight failed: ${errorText(error)}. Nothing was done.`, null, "review")); }
    let phase: TakeoverOutcome["phase"] = "intent";
    try {
      if (pinned.running) {
        phase = "stop";
        if (!await write(outcome(record, "uncertain", "stop", "The running source's stop is in progress. If nothing follows, Check reads it; nothing is archived or created.", named, "check")))
          return outcome(record, "uncertain", "stop", "This session's fence changed, so the running source was not stopped and nothing was archived or created. Check reads the outcome.", named, "check");
        const refreshed = await stopAndRefresh(record, pinned);
        if ("state" in refreshed) return finish(refreshed);
        record = refreshed.record; pinned = refreshed.pinned; spec = refreshed.spec;
        const stoppedPlacement = await expectedPlacementProblem(client, spec);
        if (stoppedPlacement) return finish(outcome(record, "refused", "stop", `${stoppedPlacement} Nothing was archived or created; review again.`, null, "review"));
        const stoppedExpired = effectValidityProblem(spec);
        if (stoppedExpired) return finish(outcome(record, "refused", "stop", `${stoppedExpired} Nothing was archived or created; review again.`, null, "review"));
      }
      // A running review's preview never becomes the handoff; the stopped source must have a saved final summary.
      if (!pinned.summary) return finish(outcome(record, "refused", phase === "stop" ? "stop" : "preflight",
        "No final handoff summary is pinned, so nothing was archived or created. Review the stopped source again.", null, "review"));
      if (pinned.children.length) {
        phase = "preserve";
        if (!await write(outcome(record, "uncertain", "preserve", "Managed children are being detached before retirement. If nothing follows, Check reads them; nothing is archived.", named, "check")))
          return outcome(record, "uncertain", "preserve", "This session's fence changed, so no child was detached and nothing was archived. Check reads the outcome.", named, "check");
        const current = childrenOf((await exact(agentId))!, await inventory(client));
        if (!isDeepStrictEqual(current.map((child) => child.id).sort(), pinned.children.slice().sort()))
          return finish(outcome(record, "refused", "preserve", "Managed children changed before preservation. Nothing was archived; review again.", null, "review"));
        for (const childId of pinned.children) {
          await client.agents.ref(childId).detach();
          const after = await exact(childId);
          if (!after || parent(after) !== null) return finish(outcome(record, "uncertain", "preserve", `Child ${childId} could not be verified as detached from the predecessor. It was not archived and no successor was created. Check reads it; nothing is resent.`, named, "check"));
        }
      }
      const beforeArchive = await exact(agentId);
      if (!beforeArchive) return finish(outcome(record, "refused", "archive", "The source could not be read immediately before archive. Nothing was archived or created; review again.", null, "review"));
      const archiveSettings = sourceSettingsDrift(pinned.sourceSettings, beforeArchive);
      if (archiveSettings) return finish(outcome(record, "refused", "archive", `${archiveSettings} Nothing was archived or created; review again.`, null, "review"));
      phase = "archive";
      if (pinned.retiredSource) {
        if (!await write(outcome(record, "uncertain", "archive", "The source is already archived. Its history is used without reviving or prompting its runtime; creation follows if nothing interrupts.", named, "check")))
          return outcome(record, "uncertain", "archive", "This session's fence changed before creation. No successor was created. Check reads the outcome.", named, "check");
        const unretired = await retirement(record, agentId, null, true);
        if (unretired) return finish(unretired);
      } else {
        const reply = await client.agents.ref(agentId).archive().then(() => null, (error: unknown) => error ?? "rejected");
        const unretired = await retirement(record, agentId, reply);
        if (unretired) return finish(unretired);
      }
      phase = "create";
      const revived = predecessorConflict(await exact(agentId));
      if (revived) return finish(outcome(record, "partial", "archive", `${revived} The successor was not created and no continuation was sent.`, null, "none"));
      // Re-check once more inside the creation core, immediately before its durable intent.
      spec = { ...spec, revalidate: async () => predecessorConflict(await exact(agentId)) };
      if (!await write(outcome(record, "uncertain", "create", "Retired. The successor's creation request is recorded; if nothing follows, Check reads it.", named, "check")))
        return outcome(record, "uncertain", "create", "Retired, but this session's fence changed, so no successor was created. Check reads the outcome.", named, "check");
      const expiredBeforeCreate = effectValidityProblem(spec);
      if (expiredBeforeCreate) return finish(outcome(record, "partial", "create", expiredBeforeCreate, named, "none"));
      const sourceSummary = pinned.summary;
      const created = await deps.create.createSeat(spec.requestId, async () => ({ spec, sourceSummary }));
      const stopped = await afterCreate(record, spec, created);
      if (stopped) return finish(stopped);
      const id = created.agentId!, verified = { ...named, agentId: id };
      // The initial release also re-checks retirement after creation: a source revived to running must not receive it.
      const revivedAfterCreate = predecessorConflict(await exact(agentId));
      if (revivedAfterCreate) return finish(outcome(record, "partial", "verify", `${revivedAfterCreate} The verified successor is not released and no other successor is created.`, verified, "none"));
      phase = "release";
      if (!await write(outcome(record, "uncertain", "release", `Successor “${spec.title}” (${id}) is verified; its task release is being sent.`, verified, "check")))
        return outcome(record, "partial", "verify", `Successor “${spec.title}” (${id}) is verified, but this session's fence changed, so its task was not released.`, verified, "none");
      const expiredBeforeRelease = effectValidityProblem(spec);
      if (expiredBeforeRelease) return finish(outcome(record, "partial", "verify", expiredBeforeRelease, verified, "none"));
      return finish(await deliver(record, spec, id, false));
    } catch (error) {
      return finish(outcome(record, "uncertain", phase, `The takeover stopped with its effect unconfirmed: ${errorText(error)}. Check reads it; nothing is resent.`, named, "check"));
    }
  }

  /** Settles an unconfirmed step by exact reads and the creation core's Check. It never archives or creates; it may send the one continuation with this operation's stable message ID and observe it. */
  async function check(reviewId: string): Promise<TakeoverOutcome> {
    const original = await readRecord(reviewId), stored = await readOperation(reviewId);
    if (!original || !stored) throw new Error("No confirmed takeover of this review is on this host.");
    if (stored.next !== "check") return stored;
    const reviewedPinned = original.pinned as Pinned;
    let final: TakeoverRecord | null = null;
    try { final = await readDurable(file("takeover-final", reviewId), takeoverRecordSchema); }
    catch { return { ...stored, state: "uncertain", next: "check", checkedAt: new Date().toISOString(),
      message: "This takeover's refreshed final record cannot be read. No continuation was sent and no successor was created; Check reads it again." }; }
    const record = final ?? original, pinned = record.pinned as Pinned, spec = pinned.spec, agentId = record.review.agentId;
    // Check performs effects only as this operation: the exact per-session fence must still name this review, agent,
    // action and predecessor revision. A missing or foreign fence is read as unconfirmed and nothing is sent or created.
    const lockPath = intentPath(agentId), bytes = await readFile(lockPath, "utf8").catch(() => null);
    let fence: z.infer<typeof fenceSchema> | null = null;
    try { fence = bytes ? fenceSchema.parse(JSON.parse(bytes)) : null; } catch { /* Not this operation's fence. */ }
    const owned = !!bytes && fence?.reviewId === reviewId && fence.agentId === agentId && fence.action === "takeover" && fence.fingerprint === reviewedPinned.predecessor;
    if (!owned) return { ...stored, state: "uncertain", next: "check", checkedAt: new Date().toISOString(),
      message: "This takeover's operation fence is missing or belongs to another action. No continuation was sent and no successor was created. Check reads it again." };
    const releaseIntent = (id: string) => outcome(record, "uncertain", "release",
      `Successor “${spec.title}” (${id}) is verified; its one task-release intent is recorded. If nothing follows, Check reads it; nothing else is created.`,
      { requestId: spec.requestId, agentId: id, title: spec.title }, "check");
    const writeReleaseIntent = (id: string) => whileOwned(lockPath, bytes!, () => writeDurable(file("takeover-operation", reviewId), releaseIntent(id)));
    let result: TakeoverOutcome;
    if (stored.phase === "stop") {
      const source = await exact(agentId);
      if (!source) result = outcome(record, "refused", "stop", "The stopped source cannot be read on this host, so no retirement or successor is performed. Review the source again.", null, "review");
      else if (source.archivedAt != null) result = outcome(record, "partial", "stop", "The source is archived, but no successor was created. Review the archived source again if you want a successor.", null, "none");
      else if (active(source)) result = outcome(record, "refused", "stop", "The source is still running after the recorded stop attempt. Nothing was archived or created; review it again when the turn ends.", null, "review");
      else {
        const stoppedSettings = sourceSettingsDrift(reviewedPinned.sourceSettings, source);
        if (stoppedSettings) result = outcome(record, "refused", "stop", `${stoppedSettings} Nothing was archived or created; review again.`, null, "review");
        else if (sourceIdentity(source) !== reviewedPinned.sourceIdentity) result = outcome(record, "refused", "stop", "The stopped source's title, checkout, workspace or owner changed. Nothing was archived or created; review again.", null, "review");
        else result = outcome(record, "refused", "stop", `${final ? "The stopped source's final handoff is recorded" : "The stop is complete"}, but retirement and successor creation were not performed. Nothing was archived or created; review again to continue.`, null, "review");
      }
    } else if (stored.phase === "preserve") {
      const source = await exact(agentId);
      if (!source) result = { ...stored, checkedAt: new Date().toISOString(), message: "The predecessor cannot be read; child preservation remains unsettled. Check reads it again." };
      else if (source.archivedAt != null) result = await retirement(record, agentId, null, true) ?? outcome(record, "partial", "archive", "The session was retired; no successor was created.", null, "none");
      else {
        const children = childrenOf(source, await inventory(client));
        const attached = children.filter((child) => pinned.children.includes(child.id));
        result = attached.length || children.length !== pinned.children.length
          ? { ...stored, checkedAt: new Date().toISOString(), message: "Child preservation is not settled: an expected child is still attached or the child set changed. Check reads it again; nothing is archived or resent." }
          : outcome(record, "refused", "preserve", "Every managed child is detached and the predecessor is not archived. Nothing was retired; review again to continue.", null, "review");
      }
    } else if (stored.phase === "intent" || stored.phase === "archive") {
      // Classify the observed archive first; settings drift may block further work but must not erase an
      // already observed or uncertain retirement. Check remains read-only here.
      const retired = await retirement(record, agentId, null, pinned.retiredSource);
      const source = await exact(agentId);
      const settings = source ? sourceSettingsDrift(pinned.sourceSettings, source) : null;
      if (!retired) result = outcome(record, "partial", "archive",
        `The session was retired; no successor was created.${settings ? ` ${settings}` : ""}`, null, "none");
      else result = settings ? { ...retired, checkedAt: new Date().toISOString(), message: `${retired.message} ${settings}` } : retired;
    } else if (stored.phase === "create") {
      const created = await deps.create.check(spec.requestId);
      const stopped = await afterCreate(record, spec, created);
      const revived = predecessorConflict(await exact(agentId));
      result = stopped ?? (revived
        ? outcome(record, "partial", "verify", `${revived} The verified successor is not released and no other successor is created.`, { requestId: spec.requestId, agentId: created.agentId, title: spec.title }, "none")
        : created.agentId
          ? await writeReleaseIntent(created.agentId)
            ? await deliver(record, spec, created.agentId)
            : { ...stored, state: "uncertain", next: "check", checkedAt: new Date().toISOString(), message: "This takeover's fence changed before its release intent. No continuation was sent and no successor was created. Check reads it again." }
          : outcome(record, "uncertain", "create", "The creation result names no exact successor. Check reads the same request; nothing is created again.", { requestId: spec.requestId, agentId: null, title: spec.title }, "check"));
    } else {
      const id = stored.successor?.agentId ?? "";
      const problem = id ? await verify(spec, id, agentId, false) : "no exact successor is named";
      const revived = predecessorConflict(await exact(agentId));
      result = revived
        ? outcome(record, "partial", "release", `${revived} The successor is not released and no other successor is created.`, stored.successor, "none")
        : problem
          ? outcome(record, "partial", "release", `The recorded successor is not verified: ${problem}. Its task was not released and no other successor is created.`, stored.successor, "none")
          : await writeReleaseIntent(id)
            ? await deliver(record, spec, id)
            : { ...stored, state: "uncertain", next: "check", checkedAt: new Date().toISOString(), message: "This takeover's fence changed before its release intent. No continuation was sent and no other successor is created. Check reads it again." };
    }
    // Saved under this operation's own fence while it still holds the session; a settled result releases it.
    const kept = await whileOwned(lockPath, bytes!, async () => {
      await writeDurable(file("takeover-operation", reviewId), result);
      if (settledTakeover(result)) await unlink(lockPath); else await writeFile(lockPath, JSON.stringify({ ...JSON.parse(bytes!), phase: result.phase }), { mode: 0o600 });
    });
    return kept ? result : { ...result, message: `${result.message} This session's fence changed during the read, so this result was not recorded.` };
  }
  // Review revisions, confirmations and Check share one queue across service instances in this plugin worker.
  return {
    review: (input: TakeoverReviewInput) => queued(file("takeover", input.reviewId ?? randomUUID()), () => review(input)),
    apply: (input: { reviewId: string; revision: number; digest: string }) => queued(file("takeover", input.reviewId), () => apply(input)),
    check: (reviewId: string) => queued(file("takeover", reviewId), () => check(reviewId)),
  };
}

/** One configured-source operator read: profile choices and account-alias catalog share the same snapshot revision. */
export async function readOperatorFleetDiscovery(): Promise<{ choices: ProfileChoice[]; catalog: ConfiguredAccountCatalog }> {
  const catalog = sourceCatalog(), snapshot = await createTeamsSourceAdapter(catalog, teamsSourceBindings(catalog.sources).formats).read({ sourceId: "operator" });
  if (snapshot.data?.kind !== "fleet") throw new Error(`the operator fleet source is ${snapshot.status}`);
  const choices = snapshot.data.bindings.filter((binding) => binding.chainResolved && BINDING_KEY.test(binding.reference)).slice(0, 200)
    .map((binding) => ({ bindingKey: binding.reference, chain: binding.chainName }));
  const built = configuredAccountCatalogFromFleet(snapshot.data.domains, snapshot.sha256 ?? snapshot.readAt);
  if (!built) throw new Error("The operator fleet source names no verifiable configured account aliases.");
  return { choices, catalog: built };
}
export function contributeSessionLifecycle(server: PluginServerContext) {
  const root = join(dirname(readConfig().paseoWorktreesRoot), "plugin-state", "kontor", "session-lifecycle-v1");
  server.handle(previewLifecycle, (input, { paseo }) => createLifecycleService(paseo, root).preview(input));
  server.handle(readLifecycleReview, (input, { paseo }) => createLifecycleService(paseo, root).read(input.reviewId));
  server.handle(executeLifecycleItem, (input, { paseo }) => createLifecycleService(paseo, root).execute(input.reviewId, input.agentId));
  server.handle(reconcileLifecycleItem, (input, { paseo }) => createLifecycleService(paseo, root).reconcile(input.reviewId, input.agentId));
  server.handle(probeSessionAvailability, (input, { paseo }) => createLifecycleService(paseo, root).probe(input.agentIds));
  server.handle(readSessionDispositions, (input, { paseo }) => createLifecycleService(paseo, root).dispositions(input.agentIds));
  server.handle(readProviderRefusal, (input, { paseo }) => createLifecycleService(paseo, root).providerRefusal(input.agentId));
  const takeover = (paseo: PaseoApi) => {
    const client = asTakeoverClient(paseo), config = readConfig();
    const publicCli = createPaseoPublicCli(config), completeSchedules = createCompleteScheduleReader(config);
    return createTakeoverService(client, root, {
      config, create: createSessionCreateService(paseo, createStateRoot(), createSwitchFile()), promotionRoot: createStateRoot(), operatorFleetDiscovery: readOperatorFleetDiscovery,
      summaries: createSessionSummaryService(paseo, summaryStateRoot()),
      snapshot: (input) => paseo.providers.snapshot({ cwd: input.cwd }),
      evidence: async ({ mode, cwd, catalog }) => routeHealthFromProviderSnapshot(await paseo.providers.snapshot({ cwd }), mode, catalog),
      publicCli, completeSchedules,
      continuity: (agentId, requestId) => readTakeoverContinuity(client, root, agentId, requestId, { completeSchedules }),
    });
  };
  server.handle(reviewTakeover, (input, { paseo }) => takeover(paseo).review(input));
  server.handle(applyTakeover, (input, { paseo }) => takeover(paseo).apply(input));
  server.handle(checkTakeover, (input, { paseo }) => takeover(paseo).check(input.reviewId));
}
