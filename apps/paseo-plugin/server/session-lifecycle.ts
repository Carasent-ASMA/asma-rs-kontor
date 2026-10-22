import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, readFile, rename, unlink, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";
import { z } from "zod";
import type { PaseoApi, PaseoAgentListResult } from "@getpaseo/client";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { readConfig } from "./config";
import {
  TAKEOVER_LIMIT, RESUME_PROMPT, lifecycleReviewSchema, lifecycleRowSchema, lifecycleCapabilities,
  previewLifecycle, executeLifecycleItem, reconcileLifecycleItem, readLifecycleReview, probeSessionAvailability, readSessionDispositions,
  type LifecycleReview, type LifecycleRow, type RecoveryTarget, type SessionAvailability,
} from "../shared/session-lifecycle";

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
function terminalArchive(agent?: Agent): boolean {
  return !!agent?.archivedAt && agent.status === "closed" && !agent.activeTurn;
}
function childrenOf(agent: Agent, inventory: Agent[]): Agent[] {
  // Block every managed child, even one the native cascade would detach today.
  return inventory.filter((child) => child.archivedAt == null && parent(child) === agent.id);
}
export function lifecycleRow(agent: Agent, inventory: Agent[], action: LifecycleReview["action"], target: RecoveryTarget): LifecycleRow {
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
  else if (agent.providerUnavailable || /quota|usage.?limit|rate.?limit|exhaust|unauthori[sz]ed|authenticat|\b401\b|\b403\b|billing|credit|api.?key|credential|token.expired|permission.denied/i.test(agent.lastError ?? "")) { row.category = "candidate"; row.reason = "Quota/auth/provider restriction: no validated changed account or condition. Open native Accounts/session to resolve it, then review again. Resume alone is not a takeover or a quota fix."; }
  else if (agent.provider !== target.provider || row.model !== target.model || row.effort !== target.effort) { row.category = "candidate"; row.reason = "Target differs from this session. Resume preserves its existing provider/model/effort. Provider change requires the still-open takeover workflow."; }
  else if (!agent.capabilities.supportsSessionPersistence || !agent.persistence?.sessionId || !row.mode || !agent.cwd) { row.category = "candidate"; row.reason = "Missing native history/persistence, permission mode or checkout evidence. This screen cannot fabricate a recovery brief."; }
  else { row.category = "candidate"; row.eligible = true; row.reason = `Current native error${agent.lastError ? `: ${agent.lastError}` : ""}. Retry in the same session with existing context and permissions; dispatch is not completion.`; }
  return row;
}

async function inventory(client: LifecycleClient): Promise<Agent[]> {
  const agents = new Map<string, Agent>(), cursors = new Set<string>();
  let cursor: string | undefined;
  for (let page = 0; page < 1_000; page++) {
    const result = await client.agents.list({ filter: { includeArchived: true }, page: { limit: 200, ...(cursor ? { cursor } : {}) } });
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
      let latest: { id: string };
      try { latest = JSON.parse(await readFile(join(root, "latest.json"), "utf8")); }
      catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return null; throw error; }
      return read(latest.id);
    }
    if (!/^[a-f0-9-]{36}$/.test(id)) throw new Error("Invalid review ID");
    const review = lifecycleReviewSchema.parse(JSON.parse(await readFile(reviewPath(id), "utf8")));
    review.rows = await Promise.all(review.rows.map(async (row) => {
      try { return lifecycleRowSchema.parse(JSON.parse(await readFile(resultPath(id, row.id), "utf8"))); }
      catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return row; throw error; }
    }));
    return review;
  }
  async function capabilities() {
    const snapshot = await client.providers.snapshot();
    return { providers: snapshot.entries.map((entry) => ({ id: entry.provider, status: entry.status,
      models: (entry.models ?? []).filter((model) => model.isSelectable !== false).map((model) => ({ id: model.id, efforts: (model.thinkingOptions ?? []).map((option) => option.id) })) })), limitation: TAKEOVER_LIMIT };
  }
  async function preview(input: { action: LifecycleReview["action"]; scope: LifecycleReview["scope"]; agentIds: string[]; target: RecoveryTarget }): Promise<LifecycleReview> {
    if (input.action === "archive" && (input.scope !== "filtered" || input.agentIds.length !== 1)) throw new Error("Archive requires one exact session ID.");
    const all = await inventory(client), ids = new Set(input.agentIds);
    const selectedIds = input.scope === "host" ? all.filter((agent) => agent.archivedAt == null).map((agent) => agent.id) : [...ids];
    const rows: LifecycleRow[] = [];
    for (const id of selectedIds) {
      const observed = await observe(id), listed = all.find((agent) => agent.id === id);
      const record = observed.agent ?? listed;
      const row: LifecycleRow = record ? lifecycleRow(record, all, input.action, input.target) : {
        id, title: id, workspaceId: null, cwd: "", provider: "", model: null, effort: null, mode: null,
        nativeStatus: "missing", fingerprint: "missing", category: "unknown", eligible: false, reason: "", children: [], outcome: "pending", message: null,
      };
      row.availability = observed.availability;
      if (!observed.agent) { row.eligible = input.action === "archive" && observed.availability.state === "missing"; row.category = "unknown"; row.reason = observed.availability.reason; }
      if (input.action === "archive" && terminalArchive(observed.agent)) row.eligible = true;
      rows.push(row);
    }
    const review: LifecycleReview = { id: randomUUID(), createdAt: new Date().toISOString(), action: input.action, scope: input.scope, target: input.target, rows, limitation: TAKEOVER_LIMIT };
    await save(review); return review;
  }
  async function observe(id: string): Promise<{ agent?: Agent; availability: SessionAvailability }> {
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
    if (row.outcome !== "pending") return review.action === "archive" && ["intent", "uncertain"].includes(row.outcome) ? reconcile(reviewId, agentId) : row;
    if (!row.eligible) { const blocked = { ...row, outcome: "blocked" as const, message: row.reason }; await saveRow(reviewId, blocked); return blocked; }
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
    await lock.writeFile(JSON.stringify({ reviewId, agentId, action: review.action, phase: "preflight", fingerprint: row.fingerprint })); await lock.sync(); await lock.close();
    let dispatchStarted = false, alreadyArchived = false;
    try {
      // Another click may have read pending before the first owner saved its result and released.
      const settled = (await read(reviewId))?.rows.find((entry) => entry.id === agentId);
      if (settled && settled.outcome !== "pending") { await unlink(lockPath); return settled; }
      const doneFile = join(root, `done-${hash(`${agentId}:${row.fingerprint}`)}.json`);
      try { await readFile(doneFile, "utf8"); throw new Error("This exact session revision already has a dispatched action. Reconcile its receipt before another action."); }
      catch (error) { if (!(error instanceof Error) || !("code" in error) || error.code !== "ENOENT") throw error; }
      const agent = client.agents.ref(agentId);
      const observed = await observe(agentId); row.availability = observed.availability;
      if (!observed.agent) throw new Error(observed.availability.reason);
      const found = { agent: observed.agent };
      alreadyArchived = terminalArchive(found.agent);
      if (found.agent.archivedAt != null) throw new Error(terminalArchive(found.agent) ? "Already archived and closed on this host. No archive request was sent." : "Archive is recorded but runtime retirement is not confirmed. Check the existing native outcome; no duplicate request was sent.");
      if ((review.action === "archive" ? archiveRevision(found.agent) : lifecycleRevision(found.agent)) !== row.fingerprint) throw new Error("Session changed or disappeared while review was open. Nothing dispatched; create a fresh review.");
      const all = await inventory(client);
      const current = lifecycleRow(found.agent, all, review.action, review.target);
      if (!current.eligible || (review.action === "archive" && current.children.length)) throw new Error(current.reason);
      if (review.action === "resume") {
        const snapshot = await client.providers.snapshot({ cwd: found.agent.cwd });
        const provider = snapshot.entries.find((entry) => entry.provider === review.target.provider && entry.enabled && entry.status === "ready");
        const model = provider?.models?.find((entry) => entry.id === review.target.model);
        if (!model || !model.thinkingOptions?.some((option) => option.id === review.target.effort)) throw new Error("Requested provider/model/effort is not currently available on this host.");
        // This is user-authorized execution preflight, never an automatic availability probe:
        // native timeline fetch can load the provider runtime. Context stays in that same session.
        const history = await agent.timeline.refetch({ direction: "tail", limit: 1 });
        if (history.error || !history.entries.length) throw new Error(`Native history is unavailable; no continuation prompt sent. ${history.error ?? "No retained conversation entries."}`);
      }
      // Final read after all potentially slow preflight calls. Never prompt an archived predecessor.
      const finalInventory = review.action === "archive" ? await inventory(client) : all;
      const latest = await observe(agentId); row.availability = latest.availability;
      if (!latest.agent) throw new Error(latest.availability.reason);
      const final = { agent: latest.agent };
      alreadyArchived = terminalArchive(final.agent);
      if (final.agent.archivedAt != null) throw new Error(terminalArchive(final.agent) ? "Already archived and closed on this host. No archive request was sent." : "Archive is recorded but runtime retirement is not confirmed. No duplicate request was sent.");
      if ((review.action === "archive" ? archiveRevision(final.agent) : lifecycleRevision(final.agent)) !== row.fingerprint) throw new Error("Session changed during preflight. Nothing dispatched; review again.");
      const finalDecision = lifecycleRow(final.agent, finalInventory, review.action, review.target);
      if (!finalDecision.eligible) throw new Error(finalDecision.reason);
      row.outcome = "intent"; row.message = "Durable intent saved. An interrupted dispatch must be reconciled before retry."; await saveRow(reviewId, row);
      await writeFile(lockPath, JSON.stringify({ reviewId, agentId, action: review.action, phase: "intent", fingerprint: row.fingerprint }), { mode: 0o600 });
      dispatchStarted = true;
      if (review.action === "resume") {
        await agent.send(RESUME_PROMPT, { messageId: `kontor-resume-${review.id}-${hash(agentId).slice(0, 12)}`, activeTurnBehavior: "steer" });
        row.outcome = "submitted"; row.message = "Resume prompt accepted in the same session. Completion and resolution of the original failure are not verified.";
      } else {
        const receipt = await agent.archive();
        if ("agentId" in receipt && receipt.agentId !== agentId) throw new Error("Archive acknowledgement identified another session. Check native outcome; no retry was made.");
        const observedAfter = await observe(agentId); row.availability = observedAfter.availability;
        const after = observedAfter.agent;
        if (!terminalArchive(after)) throw new Error("Archive returned but exact-ID terminal retirement readback is unavailable. Check native outcome; do not retry the request.");
        row.nativeStatus = after!.status;
        row.outcome = "archived"; row.message = "Native archive succeeded and closed state was verified. Durable history and files remain with the host.";
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
    await saveRow(reviewId, row);
    await writeFile(lockPath, JSON.stringify({ reviewId, agentId, action: review.action, phase: row.outcome, fingerprint: row.fingerprint, message: row.message }), { mode: 0o600 });
    if (dispatchStarted) await writeFile(join(root, `done-${hash(`${agentId}:${row.fingerprint}`)}.json`), JSON.stringify(row), { mode: 0o600 });
    // Unknown effects retain the session lock. A fresh review may retry a known preflight block.
    if (row.outcome !== "uncertain") await unlink(lockPath);
    return row;
  }
  /** Metadata-only settlement of a dispatched archive. Never resend or delete its fence. */
  async function reconcile(reviewId: string, agentId: string): Promise<LifecycleRow> {
    const review = await read(reviewId), row = review?.rows.find((entry) => entry.id === agentId);
    if (!review || !row || review.action !== "archive") throw new Error("Exact archive ID is not in this review.");
    if (row.outcome === "pending") {
      let intent: { reviewId: string; agentId: string; action: string };
      try { intent = JSON.parse(await readFile(join(root, `intent-${hash(agentId)}.json`), "utf8")); }
      catch (error) { if (error instanceof Error && "code" in error && error.code === "ENOENT") return row; throw error; }
      if (intent.reviewId !== reviewId || intent.agentId !== agentId || intent.action !== "archive") throw new Error("This review does not own the pending archive intent.");
    } else if (!["intent", "uncertain"].includes(row.outcome)) return row;
    const observed = await observe(agentId);
    const result: LifecycleRow = { ...row, availability: observed.availability,
      nativeStatus: observed.agent?.status ?? row.nativeStatus,
      outcome: terminalArchive(observed.agent) ? "archived" : "uncertain",
      message: terminalArchive(observed.agent) ? "Native archive and closed runtime verified by an exact-ID read. No additional archive request was sent."
        : "Archive outcome remains uncertain. The original no-replay fence is preserved. " + observed.availability.reason };
    await saveRow(reviewId, result);
    return result;
  }
  return { capabilities, preview, read, execute, reconcile, probe, dispositions };
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
}
