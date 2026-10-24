import { baseSessions, type FixtureSession } from "./sessions";

/** Entirely fictional lifecycle cases. No real IDs, histories or checkouts. */
export function recoverySessions(): FixtureSession[] {
  const seed = baseSessions()[0]!;
  const rows: Array<{ id: string; title: string; status: "error" | "idle" | "running" | "closed"; error?: string; parent?: string; missing?: boolean; unknown?: boolean }> = [
    { id: "retry", title: "Interrupted task · eligible retry", status: "error", error: "Transport interrupted" },
    { id: "quota", title: "Quota exhausted · account change unverified", status: "error", error: "Usage limit reached" },
    { id: "auth", title: "Authentication failed", status: "error", error: "401 Unauthorized" },
    { id: "old", title: "Old but readable session", status: "idle" },
    { id: "missing", title: "Missing native ID · unavailable", status: "idle", missing: true },
    { id: "unknown", title: "Probe failed · keep visible", status: "idle", unknown: true },
    { id: "waiting", title: "Waiting for permission", status: "idle" },
    { id: "parent", title: "Supervisor with protected child", status: "error", error: "Interrupted" },
    { id: "child", title: "Running child · preserve work", status: "running", parent: "parent" },
    { id: "send_failure", title: "Resume acknowledgement lost", status: "error", error: "Interrupted" },
    { id: "archive_failure", title: "Archive close failure", status: "idle" },
    { id: "concurrent", title: "Another recovery owns this session", status: "error", error: "Interrupted" },
    { id: "stopped", title: "Intentionally stopped", status: "closed" },
  ];
  return rows.map((row) => ({ ...seed, agent: { ...seed.agent, id: `agt_recovery_${row.id}`, title: `SWE • ${row.title}`, status: row.status,
    provider: "codex", model: "gpt-6.1-sol", thinkingOptionId: "xhigh", effectiveThinkingOptionId: "xhigh", currentModeId: "full-access",
    lastError: row.error, persistence: { provider: "codex", sessionId: `fictional-history-${row.id}` },
    createdAt: "2001-01-01T00:00:00Z", updatedAt: "2001-01-01T00:00:00Z", requiresAttention: true,
    attentionReason: row.id === "waiting" ? "permission" : row.status === "error" ? "error" : "finished", archivedAt: null,
    parentAgentId: row.parent ? `agt_recovery_${row.parent}` : null,
    labels: { workstream: "Recovery examples", role: "SWE", "fixture.availability": row.missing ? "missing" : row.unknown ? "unknown" : "readable", ...(row.parent ? { "paseo.parent-agent-id": `agt_recovery_${row.parent}` } : {}) } } }));
}
