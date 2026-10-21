import type { FixtureSession } from "../fixtures/sessions";
import { logHostEvent } from "./event-log";

type Target = { provider: string; model: string; effort: string };
type Preview = { action: "resume" | "archive"; scope: "host" | "filtered"; agentIds: string[]; target: Target };
const limitation = "SIMULATED host. Bulk takeover remains open; Resume preserves the same session. No real session, account, file or history is affected.";
const fingerprint = (row: FixtureSession) => JSON.stringify(row.agent);
function lifecycleRow(row: FixtureSession, all: FixtureSession[], input: Preview) {
  const a = row.agent;
  const children = all.filter(({ agent }) => !agent.archivedAt && (agent.parentAgentId ?? agent.labels["paseo.parent-agent-id"]) === a.id).map(({ agent }) => ({ id: agent.id, title: agent.title, status: agent.status, workspaceId: agent.workspaceId }));
  let eligible = false, category: "candidate" | "unknown" | "excluded" = "excluded", reason = "No supported current failure";
  if (a.archivedAt) reason = "Already archived; never revive";
  else if (input.action === "archive" && children.length) reason = "Native archive would cascade. Children are protected; archive is blocked.";
  else if (a.status === "running") { category = "unknown"; reason = "Running or waiting. Age does not prove a stall. Open the native session first."; }
  else if (input.action === "archive") { eligible = true; category = "candidate"; reason = "Native archive, preserving history/files. No undo."; }
  else if (a.status === "closed") reason = "Intentionally stopped/closed; not an automatic failure";
  else if (a.attentionReason === "permission") reason = "Legitimate permission/input wait";
  else if (a.id.endsWith("concurrent")) reason = "Another recovery owns this session; reconcile its existing receipt.";
  else if (/limit|unauthorized/i.test(a.lastError ?? "")) { category = "candidate"; reason = "Quota/auth condition has no validated changed route; Resume is not a quota fix."; }
  else if (a.status === "error" && a.provider === input.target.provider && a.model === input.target.model && a.effectiveThinkingOptionId === input.target.effort) { eligible = true; category = "candidate"; reason = `Current native error: ${a.lastError}. Retry same session; task completion is unverified.`; }
  else { category = "unknown"; reason = "Unknown recovery eligibility. No failure inferred from idle/old timestamps; target changes need takeover."; }
  return { id: a.id, title: a.title, workspaceId: a.workspaceId, cwd: a.cwd, provider: a.provider, model: a.model, effort: a.effectiveThinkingOptionId ?? null, mode: a.currentModeId ?? null,
    nativeStatus: a.status, fingerprint: fingerprint(row), category, eligible, reason, children, outcome: "pending" as "pending" | "intent" | "submitted" | "archived" | "blocked" | "uncertain", message: null as string | null };
}
type Review = { id: string; action: Preview["action"]; scope: Preview["scope"]; target: Target; rows: ReturnType<typeof lifecycleRow>[]; createdAt: string; limitation: string };

/** Fixture-only RPC adapter; schema validation remains the real SDK's job in the export. */
export function fixtureLifecycle(get: () => FixtureSession[], update: (id: string, patch: Partial<FixtureSession["agent"]>, note: string) => void) {
  const reviews = new Map<string, Review>(), locked = new Set<string>(); let latest: string | null = null;
  return async function invoke(method: string, input: unknown): Promise<unknown> {
    if (method === "sessions.lifecycle-capabilities") return { limitation, providers: [{ id: "codex", status: "ready", models: [{ id: "gpt-6.1-sol", efforts: ["low", "high", "xhigh"] }] }, { id: "claude", status: "ready", models: [{ id: "fixture-opus", efforts: ["high"] }] }] };
    if (method === "sessions.availability") {
      const ids = (input as { agentIds: string[] }).agentIds;
      return Object.fromEntries(ids.map((id) => { const row = get().find(({ agent }) => agent.id === id); const state = row?.agent.labels["fixture.availability"] ?? "readable";
        return [id, { state, reason: state === "missing" ? "SIMULATED exact native ID missing" : state === "unknown" ? "SIMULATED probe failure; not proof of death" : "SIMULATED readable record; provider execution unverified", checkedAt: new Date().toISOString(), revision: row?.agent.updatedAt ?? "missing" }]; }));
    }
    if (method === "sessions.lifecycle-preview") {
      const request = input as Preview, all = get();
      const review: Review = { id: crypto.randomUUID(), action: request.action, scope: request.scope, target: request.target, createdAt: new Date().toISOString(), limitation,
        rows: all.filter(({ agent }) => request.scope === "host" ? !agent.archivedAt : request.agentIds.includes(agent.id)).map((row) => lifecycleRow(row, all, request)) };
      reviews.set(review.id, review); latest = review.id; logHostEvent("simulated.review", `${review.id}: ${request.action}, ${request.scope}`); return review;
    }
    if (method === "sessions.lifecycle-review") return structuredClone(reviews.get((input as { reviewId?: string }).reviewId ?? latest ?? "") ?? null);
    if (method === "sessions.lifecycle-execute-item") {
      const { reviewId, agentId } = input as { reviewId: string; agentId: string };
      const review = reviews.get(reviewId), row = review?.rows.find((entry) => entry.id === agentId);
      if (!review || !row) throw new Error("Exact ID not reviewed");
      if (row.outcome !== "pending") return row;
      const current = get().find(({ agent }) => agent.id === agentId);
      if (!row.eligible || !current || fingerprint(current) !== row.fingerprint || locked.has(agentId)) { row.outcome = "blocked"; row.message = "State changed, unsupported or another recovery owns this fixture. Nothing dispatched."; return row; }
      locked.add(agentId); row.outcome = "intent"; await new Promise((resolve) => setTimeout(resolve, 500));
      if (agentId.endsWith("send_failure") || (review.action === "archive" && agentId.endsWith("archive_failure"))) { row.outcome = "uncertain"; row.message = "SIMULATED lost acknowledgement/close failure. Held for reconciliation; no automatic retry."; }
      else if (review.action === "archive") { update(agentId, { status: "closed", archivedAt: new Date().toISOString() }, "SIMULATED native archive; history/files retained"); row.outcome = "archived"; row.message = "SIMULATED native archive verified; history and files retained."; }
      else { update(agentId, { status: "running", requiresAttention: false, attentionReason: null }, "SIMULATED same-session resume accepted"); row.outcome = "submitted"; row.message = "SIMULATED prompt accepted; task completion unverified."; }
      logHostEvent("simulated.lifecycle", `${agentId}: ${row.outcome}`); return structuredClone(row);
    }
    throw new Error(`Unsupported fixture lifecycle RPC ${method}`);
  };
}
