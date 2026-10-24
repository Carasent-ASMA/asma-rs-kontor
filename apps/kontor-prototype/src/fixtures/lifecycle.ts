import { baseSessions, type FixtureSession } from "./sessions";

/** Fictional closed histories, including ten without an active workspace-list entry. */
export function lifecycleSessions(): FixtureSession[] {
  const seed = baseSessions()[0]!;
  const updatedAt = new Date(Date.now() - 30 * 60_000).toISOString();
  const make = (id: string, status: "closed" | "error", reason: "error" | "finished", archivedAt: string | null = null): FixtureSession => ({
    ...seed, agent: { ...seed.agent, id, title: `SWE • Lifecycle ${id.replace("agt_lifecycle_", "")}`, status,
      updatedAt, requiresAttention: true, attentionReason: reason, archivedAt,
      labels: { workstream: "Lifecycle examples", epic: "ASMA-99101", role: "SWE" } },
  });
  const histories = Array.from({ length: 13 }, (_, index) => {
    const value = make(`agt_lifecycle_closed_${index}`, "closed", "error");
    if (index < 10) { value.agent.workspaceId = `wks_lifecycle_archived_${index}`; value.project = { ...seed.project!, workspaceName: "Archived workspace (fictional)" }; }
    return value;
  });
  return [...histories, make("agt_lifecycle_current", "error", "error"),
    make("agt_lifecycle_archived", "closed", "error", updatedAt), make("agt_lifecycle_finished", "closed", "finished")];
}
