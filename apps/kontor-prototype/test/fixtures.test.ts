import { describe, expect, it, vi } from "vitest";
import { lifecycleReviewSchema, lifecycleRowSchema, DEFAULT_RECOVERY_TARGET, availabilitySchema } from "../../paseo-plugin/shared/session-lifecycle";
import { listReadSources, readSource, readPlanAssociations } from "../../paseo-plugin/shared/read-sources";
import { readResponsibility } from "../../paseo-plugin/shared/responsibility";
import { readKontorStatus } from "../../paseo-plugin/shared/kontor-status";
import { readCreateOptions, executeCreate, checkCreate } from "../../paseo-plugin/shared/session-create";
import { callPluginRpc } from "paseo-sdk/host";
import { recoverySessions } from "../src/fixtures/recovery";
import { fixtureLifecycle } from "../src/host/session-lifecycle";

describe("current plugin contracts", () => {
  it("serves the source catalog, registry, profiles and Kontor status through the real SDK", async () => {
    vi.stubGlobal("window", { location: { search: "?scenario=integrated-review" } });
    const { invokeRpc } = await import("../src/host/runtime");
    const catalog = await callPluginRpc(listReadSources, invokeRpc, {});
    expect(catalog.registry?.workstreams.kontor.name).toBe("Kontor");
    for (const source of catalog.sources) {
      const read = await callPluginRpc(readSource, invokeRpc, { sourceId: source.id });
      expect(read.status).toBe("ready");
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
      action: "archive", scope: "host", agentIds: [], target: DEFAULT_RECOVERY_TARGET,
    }));
    expect(preview.rows.find(row => row.id === "agt_recovery_unknown")?.eligible).toBe(false);
    expect(preview.rows.find(row => row.id === "agt_recovery_parent")?.children[0]?.id).toBe("agt_recovery_child");
    expect(preview.rows.find(row => row.id === "agt_recovery_parent")?.eligible).toBe(false);
    const input = { reviewId: preview.id, agentId: "agt_recovery_child" };
    const archived = lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", input));
    expect(archived.outcome).toBe("archived");
    expect(archived.runtime).toBe("closed");
    expect(rows.find(row => row.agent.id === input.agentId)?.agent.persistence?.sessionId).toBe("fictional-history-child");
    expect(await invoke("sessions.lifecycle-execute-item", input)).toEqual(archived);
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-reconcile-item", input)).outcome).toBe("archived");
    const dispositions = await invoke("sessions.lifecycle-dispositions", { agentIds: [input.agentId] }) as Record<string, unknown>;
    expect(availabilitySchema.parse(dispositions[input.agentId]).archivedAt).toBeTruthy();
  });

  it("retains an uncertain archive across a newer review and refuses an automatic retry", async () => {
    const rows = recoverySessions();
    const update = vi.fn();
    const invoke = fixtureLifecycle(() => rows, update);
    const request = { action: "archive", scope: "filtered", agentIds: ["agt_recovery_archive_failure"], target: DEFAULT_RECOVERY_TARGET };
    const review = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", request));
    const input = { reviewId: review.id, agentId: request.agentIds[0] };
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", input)).outcome).toBe("uncertain");
    const newer = lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-preview", request));
    expect(lifecycleRowSchema.parse(await invoke("sessions.lifecycle-execute-item", { ...input, reviewId: newer.id })).outcome).toBe("blocked");
    expect(lifecycleReviewSchema.parse(await invoke("sessions.lifecycle-review", {})).unresolved).toContainEqual(input);
    expect(update).not.toHaveBeenCalled();
  });
});
