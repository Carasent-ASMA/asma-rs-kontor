import type { FixtureSession } from "../fixtures/sessions";
import { logHostEvent } from "./event-log";
import { ARCHIVE_BATCH_LIMIT, archiveOrder, RESUME_REMOVED, unresolvedArchive, type LifecycleRow, type LifecycleReview, type SessionAvailability } from "../../../paseo-plugin/shared/session-lifecycle";

type Target = { provider: string; model: string; effort: string };
type Preview = { action: "archive"; scope: "host" | "filtered"; agentIds: string[]; target: Target };
const limitation = "SIMULATED host. Archive only; no real session, account, file or history is affected.";
const parent = (a: FixtureSession["agent"]) => a.parentAgentId ?? a.labels["paseo.parent-agent-id"] ?? null;
const fingerprint = (row: FixtureSession) => JSON.stringify([row.agent.id, row.agent.createdAt, row.agent.provider, row.agent.cwd, row.agent.workspaceId, row.agent.persistence, parent(row.agent)]);
const archived = (a: FixtureSession["agent"]) => !!a.archivedAt && !a.activeTurn && a.status !== "running" && a.status !== "initializing";
function lifecycleRow(row: FixtureSession, all: FixtureSession[], input: Preview): LifecycleRow {
  const a = row.agent;
  const children = all.filter(({ agent }) => !agent.archivedAt && parent(agent) === a.id).map(({ agent }) => ({ id: agent.id, title: agent.title, status: agent.status, workspaceId: agent.workspaceId }));
  let eligible = false, category: "candidate" | "unknown" | "excluded" = "excluded", reason: string;
  if (a.archivedAt) reason = "Already archived; never revive";
  else if (children.some(child => !input.agentIds.includes(child.id))) reason = "Native archive would cascade. Children are protected; archive is blocked.";
  else { eligible = true; category = "candidate"; reason = "SIMULATED force-close and archive, preserving history/files. No undo."; }
  return { id: a.id, title: a.title, workspaceId: a.workspaceId, cwd: a.cwd, provider: a.provider, model: a.model, effort: a.effectiveThinkingOptionId ?? null, mode: a.currentModeId ?? null,
    nativeStatus: a.status, fingerprint: fingerprint(row), category, eligible, reason, children, outcome: "pending", message: null };
}
type Review = LifecycleReview;

/** Fixture-only RPC adapter; schema validation remains the real SDK's job in the export. */
export function fixtureLifecycle(get: () => FixtureSession[], update: (id: string, patch: Partial<FixtureSession["agent"]>, note: string) => void) {
  const reviews = new Map<string, Review>(), locked = new Map<string, string>(), removed = new Set<string>(); let latest: string | null = null;
  function availability(id: string): SessionAvailability {
    const row = get().find(({ agent }) => agent.id === id), a = row?.agent;
    const declared = a?.labels["fixture.availability"];
    const state = declared === "unknown" || declared === "missing" ? declared : a ? "readable" : "missing";
    return { agentId: id, state, reason: state === "missing" ? "SIMULATED no native record for this exact ID. Nothing to archive." : state === "unknown" ? "SIMULATED 401/timeout read failed; absence not confirmed" : "SIMULATED native record found; provider history unverified", checkedAt: new Date().toISOString(), revision: a?.updatedAt ?? "missing", ...(state === "readable" ? { nativeStatus: a?.status, archivedAt: a?.archivedAt ?? null, providerHistory: a?.persistence?.sessionId ? "unverified" : "not-reported" } : {}) };
  }
  return async function invoke(method: string, input: unknown): Promise<unknown> {
    if (method === "sessions.availability" || method === "sessions.lifecycle-dispositions") {
      const ids = (input as { agentIds: string[] }).agentIds;
      if (ids.length > 100) throw new Error("Metadata read exceeds 100 IDs");
      return Object.fromEntries(ids.filter(id => method !== "sessions.lifecycle-dispositions" || removed.has(id)).map(id => [id, { ...availability(id), ...(removed.has(id) ? { localRemoval: availability(id).state === "missing" } : {}) }]));
    }
    if (method === "sessions.lifecycle-preview") {
      const request = input as Preview, all = get();
      if (request.action !== "archive") throw new Error(RESUME_REMOVED);
      if ((request.scope !== "filtered" || request.agentIds.length < 1 || request.agentIds.length > ARCHIVE_BATCH_LIMIT))
        throw new Error(`Archive requires 1–${ARCHIVE_BATCH_LIMIT} explicitly selected session IDs in filtered scope.`);
      const review: Review = { id: crypto.randomUUID(), action: request.action, scope: request.scope, target: request.target, createdAt: new Date().toISOString(), limitation,
        rows: all.filter(({ agent }) => request.scope === "host" ? !agent.archivedAt : request.agentIds.includes(agent.id)).map((row) => {
          const result = lifecycleRow(row, all, request), read = availability(row.agent.id);
          return { ...result, availability: read, ...(read.state !== "readable" ? { eligible: false, reason: read.reason, category: "unknown" as const } : {}) };
        }) };
      review.rows = archiveOrder(review.rows);
      reviews.set(review.id, review); latest = review.id; logHostEvent("simulated.review", `${review.id}: ${request.action}, ${request.scope}`); return structuredClone(review);
    }
    if (method === "sessions.lifecycle-review") {
      const { reviewId } = input as { reviewId?: string };
      const review = reviews.get(reviewId ?? latest ?? "");
      if (!review || reviewId) return review ? structuredClone(review) : null;
      const unresolved = [...locked].flatMap(([agentId, owner]) => {
        const saved = reviews.get(owner);
        return saved?.action === "archive" && saved.rows.some(row => row.id === agentId && unresolvedArchive(row)) ? [{ reviewId: owner, agentId }] : [];
      });
      if (unresolved.length > 100) throw new Error(`More than 100 archives are not confirmed (${unresolved.length} found)`);
      return structuredClone({ ...review, unresolved });
    }
    if (method === "sessions.lifecycle-reconcile-item") {
      const { reviewId, agentId } = input as { reviewId: string; agentId: string };
      const row = reviews.get(reviewId)?.rows.find(entry => entry.id === agentId);
      if (!row) throw new Error("Exact ID not reviewed");
      if (reviews.get(reviewId)?.action !== "archive") throw new Error(RESUME_REMOVED);
      row.availability = availability(agentId);
      const current = get().find(entry => entry.agent.id === agentId);
      const owns = locked.get(agentId) === reviewId;
      if (row.availability.state === "readable" && current && archived(current.agent)) {
        row.outcome = "archived";
        row.runtime = current.agent.status === "closed" ? "closed" : owns ? "unconfirmed" : "not-observed";
        row.message = "SIMULATED exact-ID read confirms archive membership; runtime retirement is a separate fact.";
        if (owns && row.runtime === "closed") locked.delete(agentId);
      } else if (row.availability.state === "missing") {
        row.outcome = "absent"; row.availability.localRemoval = true; removed.add(agentId);
        row.message = "SIMULATED exact-ID read confirms absence.";
        if (owns) locked.delete(agentId);
      }
      return structuredClone(row);
    }
    if (method === "sessions.lifecycle-execute-item") {
      const { reviewId, agentId } = input as { reviewId: string; agentId: string };
      const review = reviews.get(reviewId), row = review?.rows.find((entry) => entry.id === agentId);
      if (!review || !row) throw new Error("Exact ID not reviewed");
      if (row.outcome !== "pending") return structuredClone(row);
      const owner = locked.get(agentId);
      if (owner) return { ...structuredClone(row), outcome: "blocked", reconcileReviewId: owner, message: "An earlier fixture action owns this session; no second dispatch." };
      const current = get().find(({ agent }) => agent.id === agentId);
      const read = availability(agentId); Object.assign(row, { availability: read });
      if (read.state === "missing") { Object.assign(row, { outcome: "absent", message: "SIMULATED native record disappeared. Nothing dispatched." }); return structuredClone(row); }
      if (read.state === "unknown") { row.outcome = "blocked"; row.message = read.reason; return structuredClone(row); }
      if (read.archivedAt) { Object.assign(row, { outcome: "already_archived", message: "SIMULATED already archived. No request sent." }); return structuredClone(row); }
      const remainingChildren = current ? get().filter(entry => !entry.agent.archivedAt && parent(entry.agent) === agentId) : [];
      const unfinishedChildren = row.children.some(child => {
        const result = review.rows.find(entry => entry.id === child.id);
        return !result || !(result.outcome === "already_archived" || result.outcome === "archived" && result.runtime !== "unconfirmed");
      });
      if (!row.eligible || !current || fingerprint(current) !== row.fingerprint || remainingChildren.length || unfinishedChildren) {
        row.outcome = "blocked"; row.message = "State changed, unsupported or a managed child remains. Nothing dispatched."; return structuredClone(row);
      }
      locked.set(agentId, reviewId); row.outcome = "intent"; await new Promise((resolve) => setTimeout(resolve, 500));
      if (agentId.endsWith("archive_failure")) {
        update(agentId, { status: "idle", archivedAt: new Date().toISOString() }, "SIMULATED persisted membership with rejected close reply");
        row.outcome = "archived"; row.runtime = "unconfirmed"; row.message = "SIMULATED archive recorded; runtime retirement is not confirmed. Fence retained.";
      } else if (agentId.endsWith("send_failure")) { row.outcome = "uncertain"; row.message = "SIMULATED lost acknowledgement. Held for reconciliation; no automatic retry."; }
      else { update(agentId, { status: "closed", archivedAt: new Date().toISOString() }, "SIMULATED native archive; history/files retained"); row.outcome = "archived"; row.runtime = "closed"; row.message = "SIMULATED native archive verified; history and files retained."; }
      Object.assign(row, { availability: availability(agentId) });
      if (!unresolvedArchive(row)) locked.delete(agentId);
      logHostEvent("simulated.lifecycle", `${agentId}: ${row.outcome}`); return structuredClone(row);
    }
    throw new Error(`Unsupported fixture lifecycle RPC ${method}`);
  };
}
