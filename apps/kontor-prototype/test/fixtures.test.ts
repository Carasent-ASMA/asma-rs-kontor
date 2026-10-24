import { describe, expect, it, vi } from "vitest";
import { lifecycleReviewSchema, lifecycleRowSchema, DEFAULT_RECOVERY_TARGET, availabilitySchema } from "../../paseo-plugin/shared/session-lifecycle";
import { listReadSources, readSource, readSourceSummaries, readPlanAssociations } from "../../paseo-plugin/shared/read-sources";
import { readResponsibility } from "../../paseo-plugin/shared/responsibility";
import { readKontorStatus } from "../../paseo-plugin/shared/kontor-status";
import { readCreateOptions, executeCreate, checkCreate } from "../../paseo-plugin/shared/session-create";
import { callPluginRpc } from "paseo-sdk/host";
import { recoverySessions } from "../src/fixtures/recovery";
import { fixtureLifecycle } from "../src/host/session-lifecycle";
import { hostEvents } from "../src/host/event-log";

describe("current plugin contracts", () => {
  it("serves the source catalog, registry, profiles and Kontor status through the real SDK", async () => {
    vi.stubGlobal("window", { location: { search: "?scenario=integrated-review" } });
    const { invokeRpc } = await import("../src/host/runtime");
    const catalog = await callPluginRpc(listReadSources, invokeRpc, {});
    expect(catalog.registry?.workstreams.kontor.name).toBe("Kontor");
    const summaries = await callPluginRpc(readSourceSummaries, invokeRpc, {});
    expect(summaries.sources).toEqual(catalog.sources);
    expect(summaries.registry).toEqual(catalog.registry);
    expect(Object.keys(summaries.reads)).toHaveLength(3);
    expect(hostEvents().filter(event => event.kind === "sources.read")).toHaveLength(0);
    for (const read of Object.values(summaries.reads)) {
      expect([read.content, read.graph, read.records]).toEqual([null, null, null]);
    }
    for (const source of catalog.sources) {
      const read = await callPluginRpc(readSource, invokeRpc, { sourceId: source.id });
      expect(read.status).toBe("ready");
      if (source.kind === "plan") expect(read.graph).not.toBeNull();
      expect(summaries.reads[source.id]?.planMetadata).toEqual(read.planMetadata);
      expect(summaries.reads[source.id]?.jiraHistory).toEqual(read.jiraHistory);
    }
    expect((await callPluginRpc(readPlanAssociations, invokeRpc, {})).status).toBe("ready");
    expect((await callPluginRpc(readResponsibility, invokeRpc, {})).projection?.people).toHaveLength(2);
    expect((await callPluginRpc(readKontorStatus, invokeRpc, {})).observation).toBe("ready");
    expect((await callPluginRpc(readCreateOptions, invokeRpc, {})).enabled).toBe(true);
    const request = { requestId: crypto.randomUUID(), topic: "Fictional review", workspaceId: "wks_proof_adam", provider: "codex", model: "gpt-6.1-sol", effort: "xhigh", modeId: "fixture-mode" };
    const first = await callPluginRpc(executeCreate, invokeRpc, request);
    expect(first.state).toBe("verified");
    expect(first.agentId).toMatch(/^agt_fixture_create_/);
    expect(await callPluginRpc(executeCreate, invokeRpc, request)).toEqual(first);
    expect(await callPluginRpc(checkCreate, invokeRpc, { requestId: request.requestId })).toEqual(first);
    const refused = await callPluginRpc(executeCreate, invokeRpc, { ...request, requestId: crypto.randomUUID(), workspaceId: "not-a-fixture" });
    expect(refused.state).toBe("refused");
    expect(refused.agentId).toBeNull();
  });

  it("keeps unknown and protected children visible, archives a running fixture and retains its history", async () => {
    const rows = recoverySessions();
    const invoke = fixtureLifecycle(() => rows, (id, patch) => Object.assign(rows.find(row => row.agent.id === id)!.agent, patch));
    const preview = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", {
      action: "archive", scope: "filtered", agentIds: ["agt_recovery_unknown", "agt_recovery_parent", "agt_recovery_child"], target: DEFAULT_RECOVERY_TARGET,
    }));
    expect(preview.rows.find(row => row.id === "agt_recovery_unknown")?.eligible).toBe(false);
    expect(preview.rows.find(row => row.id === "agt_recovery_parent")?.children[0]?.id).toBe("agt_recovery_child");
    expect(preview.rows.find(row => row.id === "agt_recovery_parent")?.eligible).toBe(true);
    const input = { reviewId: preview.id, agentId: "agt_recovery_child" };
    const archived = lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", input));
    expect(archived.outcome).toBe("archived");
    expect(archived.runtime).toBe("closed");
    expect(rows.find(row => row.agent.id === input.agentId)?.agent.persistence?.sessionId).toBe("fictional-history-child");
    expect(await invoke("sessions.lifecycle-execute-item", input)).toEqual(archived);
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-reconcile-item", input)).outcome).toBe("archived");
    expect(await invoke("sessions.lifecycle-dispositions", { agentIds: [input.agentId] })).toEqual({});
    const dispositions = await invoke("sessions.availability", { agentIds: [input.agentId] }) as Record<string, unknown>;
    expect(availabilitySchema.parse(dispositions[input.agentId]).archivedAt).toBeTruthy();
  });

  it("retains an archived/unconfirmed owner across success, Resume and a fresh read without redispatch", async () => {
    const rows = recoverySessions();
    const update = vi.fn((id, patch) => Object.assign(rows.find(row => row.agent.id === id)!.agent, patch));
    const invoke = fixtureLifecycle(() => rows, update);
    const request = { action: "archive", scope: "filtered", agentIds: ["agt_recovery_archive_failure"], target: DEFAULT_RECOVERY_TARGET };
    const review = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", request));
    const input = { reviewId: review.id, agentId: request.agentIds[0] };
    const failed = lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", input));
    expect([failed.outcome, failed.runtime]).toEqual(["archived", "unconfirmed"]);
    const newer = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", request));
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", { ...input, reviewId: newer.id })).reconcileReviewId).toBe(review.id);
    for (const [action, agentId] of [["archive", "agt_recovery_old"], ["resume", "agt_recovery_retry"]] as const) {
      const later = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", { ...request, action, agentIds: [agentId] }));
      await invoke("sessions.lifecycle-execute-item", { reviewId: later.id, agentId });
    }
    expect(lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-review", {})).unresolved).toEqual([input]);
    expect(lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-review", { reviewId: review.id })).unresolved).toBeUndefined();
    const before = update.mock.calls.length;
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-reconcile-item", input)).runtime).toBe("unconfirmed");
    expect(update).toHaveBeenCalledTimes(before);
    rows.find(row => row.agent.id === input.agentId)!.agent.status = "closed";
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-reconcile-item", input)).runtime).toBe("closed");
    expect(lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-review", {})).unresolved).toEqual([]);
    expect(update).toHaveBeenCalledTimes(before);
  });

  it("requires selected children to finish before a parent and never changes an omitted child", async () => {
    const rows = recoverySessions(), update = vi.fn((id, patch) => Object.assign(rows.find(row => row.agent.id === id)!.agent, patch));
    const invoke = fixtureLifecycle(() => rows, update);
    const request = { action: "archive", scope: "filtered", agentIds: ["agt_recovery_parent"], target: DEFAULT_RECOVERY_TARGET };
    const alone = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", request));
    await invoke("sessions.lifecycle-execute-item", { reviewId: alone.id, agentId: request.agentIds[0] });
    expect(update).not.toHaveBeenCalled();
    const batch = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", { ...request, agentIds: [...request.agentIds, "agt_recovery_child"] }));
    expect(batch.rows.map(row => row.id)).toEqual(["agt_recovery_child", "agt_recovery_parent"]);
    for (const row of batch.rows) {
      expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", { reviewId: batch.id, agentId: row.id })).outcome).toBe("archived");
    }
    expect(update.mock.calls.map(([id]) => id)).toEqual(["agt_recovery_child", "agt_recovery_parent"]);
  });

  it("refuses host-wide or oversized archive and changed placement without an effect", async () => {
    const rows = recoverySessions(), update = vi.fn(), invoke = fixtureLifecycle(() => rows, update);
    const request = { action: "archive", scope: "filtered", agentIds: ["agt_recovery_old"], target: DEFAULT_RECOVERY_TARGET };
    await expect(invoke("sessions.lifecycle-preview", { ...request, scope: "host" })).rejects.toThrow("explicitly selected");
    await expect(invoke("sessions.lifecycle-preview", { ...request, agentIds: Array.from({ length: 51 }, (_, i) => `fixture_${i}`) })).rejects.toThrow("explicitly selected");
    const review = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", request));
    rows.find(row => row.agent.id === request.agentIds[0])!.agent.workspaceId = "changed-workspace";
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", { reviewId: review.id, agentId: request.agentIds[0] })).outcome).toBe("blocked");
    expect(update).not.toHaveBeenCalled();
  });

  it("keeps a parent blocked when its selected child has archived but its close reply is unconfirmed", async () => {
    const rows = recoverySessions(), update = vi.fn((id, patch) => Object.assign(rows.find(row => row.agent.id === id)!.agent, patch));
    rows.find(row => row.agent.id === "agt_recovery_child")!.agent.archivedAt = "2001-01-02T00:00:00Z";
    rows.find(row => row.agent.id === "agt_recovery_archive_failure")!.agent.parentAgentId = "agt_recovery_parent";
    const invoke = fixtureLifecycle(() => rows, update);
    const review = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", { action: "archive", scope: "filtered", agentIds: ["agt_recovery_parent", "agt_recovery_archive_failure"], target: DEFAULT_RECOVERY_TARGET }));
    const outcomes = [];
    for (const row of review.rows) outcomes.push(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", { reviewId: review.id, agentId: row.id })));
    expect(outcomes.map(row => [row.outcome, row.runtime])).toEqual([["archived", "unconfirmed"], ["blocked", undefined]]);
    expect(update.mock.calls.map(([id]) => id)).toEqual(["agt_recovery_archive_failure"]);
  });

  it("retains a legacy uncertain archive and reads it without resending", async () => {
    const rows = recoverySessions(), update = vi.fn(), invoke = fixtureLifecycle(() => rows, update);
    const review = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", { action: "archive", scope: "filtered", agentIds: ["agt_recovery_send_failure"], target: DEFAULT_RECOVERY_TARGET }));
    const ref = { reviewId: review.id, agentId: "agt_recovery_send_failure" };
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", ref)).outcome).toBe("uncertain");
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-reconcile-item", ref)).outcome).toBe("uncertain");
    expect(lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-review", {})).unresolved).toEqual([ref]);
    expect(update).not.toHaveBeenCalled();
  });
});
