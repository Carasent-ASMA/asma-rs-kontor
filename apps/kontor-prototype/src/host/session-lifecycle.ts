import type { FixtureSession } from "../fixtures/sessions";
import { logHostEvent } from "./event-log";
import { unresolvedArchive, type LifecycleRow, type LifecycleReview, type SessionAvailability } from "../../../paseo-plugin/shared/session-lifecycle";

type Target = { provider: string; model: string; effort: string };
type Preview = { action: "resume" | "archive"; scope: "host" | "filtered"; agentIds: string[]; target: Target };
const limitation = "SIMULATED host. Bulk takeover remains open; Resume preserves the same session. No real session, account, file or history is affected.";
const fingerprint = (row: FixtureSession) => JSON.stringify(row.agent);
function lifecycleRow(row: FixtureSession, all: FixtureSession[], input: Preview): LifecycleRow {
  const a = row.agent;
  const children = all.filter(({ agent }) => !agent.archivedAt && (agent.parentAgentId ?? agent.labels["paseo.parent-agent-id"]) === a.id).map(({ agent }) => ({ id: agent.id, title: agent.title, status: agent.status, workspaceId: agent.workspaceId }));
  let eligible = false, category: "candidate" | "unknown" | "excluded" = "excluded", reason = "No supported current failure";
  if (a.archivedAt) reason = "Already archived; never revive";
  else if (input.action === "archive" && children.length) reason = "Native archive would cascade. Children are protected; archive is blocked.";
  else if (input.action === "archive") { eligible = true; category = "candidate"; reason = "SIMULATED force-close and archive, preserving history/files. No undo."; }
  else if (a.status === "running") { category = "unknown"; reason = "Running or waiting. Age does not prove a stall. Open the native session first."; }
  else if (a.status === "closed") reason = "Intentionally stopped/closed; not an automatic failure";
  else if (a.attentionReason === "permission") reason = "Legitimate permission/input wait";
  else if (a.id.endsWith("concurrent")) reason = "Another recovery owns this session; reconcile its existing receipt.";
  else if (/limit|unauthorized/i.test(a.lastError ?? "")) { category = "candidate"; reason = "Quota/auth condition has no validated changed route; Resume is not a quota fix."; }
  else if (a.status === "error" && a.provider === input.target.provider && a.model === input.target.model && a.effectiveThinkingOptionId === input.target.effort) { eligible = true; category = "candidate"; reason = `Current native error: ${a.lastError}. Retry same session; task completion is unverified.`; }
  else { category = "unknown"; reason = "Unknown recovery eligibility. No failure inferred from idle/old timestamps; target changes need takeover."; }
  return { id: a.id, title: a.title, workspaceId: a.workspaceId, cwd: a.cwd, provider: a.provider, model: a.model, effort: a.effectiveThinkingOptionId ?? null, mode: a.currentModeId ?? null,
    nativeStatus: a.status, fingerprint: fingerprint(row), category, eligible, reason, children, outcome: "pending" as "pending" | "intent" | "submitted" | "archived" | "blocked" | "uncertain", message: null as string | null };
}
type Review = LifecycleReview;

/** Fixture-only RPC adapter; schema validation remains the real SDK's job in the export. */
export function fixtureLifecycle(get: () => FixtureSession[], update: (id: string, patch: Partial<FixtureSession["agent"]>, note: string) => void) {
  const reviews = new Map<string, Review>(), locked = new Set<string>(); let latest: string | null = null;
  function availability(id: string): SessionAvailability {
    const row = get().find(({ agent }) => agent.id === id), a = row?.agent;
    const declared = a?.labels["fixture.availability"];
    const state = declared === "unknown" || declared === "missing" ? declared : a ? "readable" : "missing";
    return { agentId: id, state, reason: state === "missing" ? "SIMULATED no native record for this exact ID. Nothing to archive." : state === "unknown" ? "SIMULATED 401/timeout read failed; absence not confirmed" : "SIMULATED native record found; provider history unverified", checkedAt: new Date().toISOString(), revision: a?.updatedAt ?? "missing", ...(state === "readable" ? { nativeStatus: a?.status, archivedAt: a?.archivedAt ?? null, providerHistory: a?.persistence?.sessionId ? "unverified" : "not-reported" } : {}) };
  }
  return async function invoke(method: string, input: unknown): Promise<unknown> {
    if (method === "sessions.lifecycle-capabilities") return { limitation, providers: [{ id: "codex", status: "ready", models: [{ id: "gpt-6.1-sol", efforts: ["low", "high", "xhigh"] }] }, { id: "claude", status: "ready", models: [{ id: "fixture-opus", efforts: ["high"] }] }] };
    if (method === "sessions.availability" || method === "sessions.lifecycle-dispositions") {
      const ids = (input as { agentIds: string[] }).agentIds;
      return Object.fromEntries(ids.map((id) => [id, availability(id)]));
    }
    if (method === "sessions.lifecycle-preview") {
      const request = input as Preview, all = get();
      const review: Review = { id: crypto.randomUUID(), action: request.action, scope: request.scope, target: request.target, createdAt: new Date().toISOString(), limitation,
        rows: all.filter(({ agent }) => request.scope === "host" ? !agent.archivedAt : request.agentIds.includes(agent.id)).map((row) => {
          const result = lifecycleRow(row, all, request), read = availability(row.agent.id);
          return { ...result, availability: read, ...(read.state !== "readable" ? { eligible: false, reason: read.reason, category: "unknown" as const } : {}) };
        }) };
      reviews.set(review.id, review); latest = review.id; logHostEvent("simulated.review", `${review.id}: ${request.action}, ${request.scope}`); return review;
    }
    if (method === "sessions.lifecycle-review") {
      const review = reviews.get((input as { reviewId?: string }).reviewId ?? latest ?? "");
      const unresolved = [...reviews.values()].flatMap(value => value.action === "archive" ? value.rows.filter(unresolvedArchive).map(row => ({ reviewId: value.id, agentId: row.id })) : []);
      return review ? structuredClone({ ...review, unresolved }) : null;
    }
    if (method === "sessions.lifecycle-reconcile-item") {
      const { reviewId, agentId } = input as { reviewId: string; agentId: string };
      const row = reviews.get(reviewId)?.rows.find(entry => entry.id === agentId);
      if (!row) throw new Error("Exact ID not reviewed");
      row.availability = availability(agentId);
      if (row.availability.state === "readable" && row.availability.archivedAt) {
        row.outcome = "archived"; row.runtime = row.availability.nativeStatus === "closed" ? "closed" : "not-observed";
        row.message = "SIMULATED exact-ID read confirms archive membership."; locked.delete(agentId);
      } else if (row.availability.state === "missing") {
        row.outcome = "absent"; row.availability.localRemoval = true;
        row.message = "SIMULATED exact-ID read confirms absence."; locked.delete(agentId);
      }
      return structuredClone(row);
    }
    if (method === "sessions.lifecycle-execute-item") {
      const { reviewId, agentId } = input as { reviewId: string; agentId: string };
      const review = reviews.get(reviewId), row = review?.rows.find((entry) => entry.id === agentId);
      if (!review || !row) throw new Error("Exact ID not reviewed");
      if (row.outcome !== "pending") return row;
      const current = get().find(({ agent }) => agent.id === agentId);
      const read = availability(agentId); Object.assign(row, { availability: read });
      if (read.state === "missing") { Object.assign(row, { outcome: "absent", message: "SIMULATED native record disappeared. Nothing dispatched." }); return structuredClone(row); }
      if (read.state === "unknown") { row.outcome = "blocked"; row.message = read.reason; return structuredClone(row); }
      if (read.archivedAt) { Object.assign(row, { outcome: "already_archived", message: "SIMULATED already archived. No request sent." }); return structuredClone(row); }
      if (!row.eligible || !current || fingerprint(current) !== row.fingerprint || locked.has(agentId)) { row.outcome = "blocked"; row.message = "State changed, unsupported or another recovery owns this fixture. Nothing dispatched."; return row; }
      locked.add(agentId); row.outcome = "intent"; await new Promise((resolve) => setTimeout(resolve, 500));
      if (agentId.endsWith("send_failure") || (review.action === "archive" && agentId.endsWith("archive_failure"))) { row.outcome = "uncertain"; row.message = "SIMULATED lost acknowledgement/close failure. Held for reconciliation; no automatic retry."; }
      else if (review.action === "archive") { update(agentId, { status: "closed", archivedAt: new Date().toISOString() }, "SIMULATED native archive; history/files retained"); row.outcome = "archived"; row.runtime = "closed"; row.message = "SIMULATED native archive verified; history and files retained."; }
      else { update(agentId, { status: "running", requiresAttention: false, attentionReason: null }, "SIMULATED same-session resume accepted"); row.outcome = "submitted"; row.message = "SIMULATED prompt accepted; task completion unverified."; }
      Object.assign(row, { availability: availability(agentId) });
      if (row.outcome !== "uncertain") locked.delete(agentId);
      logHostEvent("simulated.lifecycle", `${agentId}: ${row.outcome}`); return structuredClone(row);
    }
    throw new Error(`Unsupported fixture lifecycle RPC ${method}`);
  };
}
