import { baseSessions, type FixtureAgent, type FixtureSession } from "./sessions";

/** Fictional native states, rendered by the production status derivation and markers. */
export function statusMarkerSessions(): FixtureSession[] {
  const base = baseSessions()[0]!;
  const patches: Partial<FixtureAgent>[] = [
    { status: "running" }, { status: "idle" }, { status: "initializing" }, { status: "closed" },
    { status: "idle", requiresAttention: true, attentionReason: "permission", pendingPermissions: [{}] },
    { status: "error", requiresAttention: true, attentionReason: "error" },
    { status: "idle", requiresAttention: true, attentionReason: "finished" },
    { status: "running", requiresAttention: true, attentionReason: "finished" },
    { status: "closed", requiresAttention: true, attentionReason: "error" },
    { status: "closed", archivedAt: "2026-10-20T10:00:00Z", attentionReason: "error" },
  ];
  return patches.map((patch, index) => ({ ...base, agent: { ...base.agent,
    id: `agt_marker_${index}`, title: `Marker sample ${String.fromCharCode(65 + index)}`,
    status: "idle", requiresAttention: false, attentionReason: null, pendingPermissions: [], archivedAt: null,
    labels: { ...base.agent.labels, role: "SWE" }, ...patch,
  } }));
}
