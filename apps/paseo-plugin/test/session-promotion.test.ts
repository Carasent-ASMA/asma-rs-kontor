import { createHash, randomUUID } from "node:crypto";
import { mkdtemp, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult, PaseoWorkspace } from "@getpaseo/client";
import { createPromotionService, type PromotionClient } from "../server/session-promotion";
import { PERSONA_VERSION, reissuePersona } from "../server/session-create";
import { TITLE_AUTO_LABEL } from "../shared/session-create";
import type { JiraSearch } from "../server/session-create";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

const sourceTitle = "LSA • ADHOC • Route review";
const summary = "Scope: keep the ADHOC session.\nArtifacts: design revision 7.\nVerified: source reviewed.\nUnverified: installed readback.\nNext action: await operator.";
const summarySha = createHash("sha256").update(summary).digest("hex");
const summaryRevision = "artifact-rev-7";
const ack = `HANDOFF-ACK ASMA-100 ${summaryRevision}`;

function agent(id: string, patch: Partial<Agent> = {}): Agent {
  return { id, provider: "claude", model: "opus", effectiveThinkingOptionId: "high", cwd: "/fixtures/wks-adhoc", workspaceId: "wks-adhoc",
    title: sourceTitle, status: "idle", lastError: null, createdAt: "2026-10-28", updatedAt: "2026-10-28", lastUserMessageAt: null,
    capabilities: { supportsStreaming: true, supportsSessionPersistence: true, supportsDynamicModes: false, supportsMcpServers: true, supportsReasoningStream: true, supportsToolInvocations: true },
    currentModeId: "default", availableModes: [], pendingPermissions: [], persistence: null,
    labels: { role: "LSA", [TITLE_AUTO_LABEL]: sourceTitle, persona: PERSONA_VERSION }, requiresAttention: false,
    attentionReason: null, attentionTimestamp: null, archivedAt: null, ...patch } as Agent;
}
function workspace(id: string, name: string, patch: Partial<PaseoWorkspace> = {}): PaseoWorkspace {
  return { id, projectId: "prj-adam", projectDisplayName: "DESK • ADAM", projectRootPath: "/fixtures", projectKind: "directory",
    workspaceKind: "local_checkout", name, title: null, workspaceDirectory: `/fixtures/${id}`, archivingAt: null, status: "done",
    statusEnteredAt: null, activityAt: null, scripts: [], ...patch };
}

async function setup({ source = agent("agent-adhoc"), others = [] as Agent[], runText = ack, sendError = null as Error | null, timeline = summary as string | null } = {}) {
  const root = await mkdtemp(join(tmpdir(), "kontor-promotion-")); dirs.push(root);
  const switchFile = join(root, "create.json");
  const agents = new Map([source, ...others].map((value) => [value.id, value]));
  const homes = new Map<string, PaseoWorkspace>([
    ["wks-adhoc", workspace("wks-adhoc", "ADHOC")],
    ["wks-lsa", workspace("wks-lsa", "LSA")],
  ]);
  for (const value of agents.values()) if (value.workspaceId && !homes.has(value.workspaceId)) homes.set(value.workspaceId, workspace(value.workspaceId, "LSA"));
  const send = vi.fn(async (_text: string) => { if (sendError) throw sendError; });
  const run = vi.fn(async (_text: string, _options?: { timeoutMs?: number }) => ({ status: "idle", lastMessage: runText, permission: null }));
  const timelineRefetch = vi.fn(async () => ({ entries: timeline === null ? [] : [{ item: { type: "assistant_message", text: timeline } }] }));
  const list = vi.fn(async (options?: { filter?: { labels?: Record<string, string>; includeArchived?: boolean } }) => ({
    requestId: "fixture", pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
    entries: [...agents.values()].filter((value) => (options?.filter?.includeArchived !== false || !value.archivedAt)
      && Object.entries(options?.filter?.labels ?? {}).every(([key, label]) => value.labels?.[key] === label)).map((value) => ({ agent: value, project: null })),
  }));
  const client = {
    agents: { list, ref: (id: string) => ({
      refresh: async () => { const value = agents.get(id); return value ? { agent: value, project: null } : null; },
      timeline: { refetch: timelineRefetch },
      send,
      run,
    }) },
    workspaces: { ref: (id: string) => ({ refresh: async () => homes.get(id) ?? null }) },
  } as unknown as PromotionClient;
  const jira = vi.fn<JiraSearch>(async ({ query }) => ({ issues: [{ id: "100", key: query, summary: "Epic", created: "2026-10-01", type: "Epic", epic: true,
    status: "In Development", statusCategory: "In Progress", open: true, parent: null }], hasMore: false, nextPageToken: null, failure: null }));
  const service = createPromotionService(client, root, switchFile, jira);
  return { service, send, run, agents, homes, root, switchFile, jira, list, timelineRefetch };
}

describe("ADHOC LSA conditional promotion", () => {
  it("promotes the same native session when no epic lead exists and keeps native and logical placement separate", async () => {
    const s = await setup();
    const source = s.agents.get("agent-adhoc")!;
    const requestId = randomUUID();
    const outcome = await s.service.promote({ requestId, agentId: source.id, epic: "ASMA-100" });
    expect(outcome).toMatchObject({ state: "promoted", agentId: source.id, epic: "ASMA-100", canonicalAgentId: source.id,
      native: { workspaceId: "wks-adhoc", workspace: "ADHOC", title: sourceTitle, titleAuto: sourceTitle },
      logical: { workspace: "LSA", node: "epic:ASMA-100" }, acknowledgement: "not-required" });
    expect(outcome.native!.workspace).not.toBe(outcome.logical!.workspace);
    expect(s.send).toHaveBeenCalledExactlyOnceWith(expect.stringContaining("Native origin: ADHOC (wks-adhoc) is retained"));
    expect(s.send.mock.calls[0]![0]).toContain(`Binding revision: ${outcome.logical!.bindingRevision}`);
    expect(s.agents.get(source.id)!.workspaceId).toBe("wks-adhoc");
    expect(s.agents.get(source.id)!.title).toBe(sourceTitle);
    const record = JSON.parse(await readFile(join(s.root, "promotion-v1", `${requestId}.json`), "utf8"));
    expect(record).toMatchObject({ state: "promoted", branch: "promote", native: { workspace: "ADHOC" }, logical: { workspace: "LSA" } });
    const read = await s.service.read(source.id);
    expect(read?.record.state).toBe("promoted");
    expect(read?.native?.workspace).toBe("ADHOC");
  });

  it("hands off to the exact existing epic LSA with a different topic and requires its exact-revision ACK", async () => {
    const canonical = agent("agent-canonical", { workspaceId: "wks-lsa", title: "LSA • ASMA-100 • Other topic",
      labels: { role: "LSA", epic: "ASMA-100", [TITLE_AUTO_LABEL]: "LSA • ASMA-100 • Other topic" } });
    const s = await setup({ others: [canonical] });
    const outcome = await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100", summary: { text: summary, revision: summaryRevision } });
    expect(outcome).toMatchObject({ state: "handoff", canonicalAgentId: canonical.id, summarySha256: summarySha, summaryRevision, acknowledgement: "verified",
      native: { workspace: "ADHOC" }, logical: { workspace: "LSA" } });
    expect(s.run).toHaveBeenCalledExactlyOnceWith(expect.stringContaining(summary), { timeoutMs: 120_000 });
    expect(s.run.mock.calls[0]![0]).toContain(ack);
    expect(s.send).not.toHaveBeenCalled();
    expect(s.agents.get(canonical.id)!.title).toBe("LSA • ASMA-100 • Other topic");
    expect(s.agents.get(canonical.id)!.workspaceId).toBe("wks-lsa");
  });

  it("treats two active leads for one epic as ambiguous and dispatches nothing", async () => {
    const one = agent("lead-one", { workspaceId: "wks-lsa", labels: { role: "LSA", epic: "ASMA-100" } });
    const two = agent("lead-two", { workspaceId: "wks-lsa", labels: { role: "LSA", epic: "ASMA-100" } });
    const s = await setup({ others: [one, two] });
    const outcome = await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100" });
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringContaining("Multiple active canonical LSAs") });
    expect(s.send).not.toHaveBeenCalled();
    expect(s.run).not.toHaveBeenCalled();
  });

  it("does not treat an archived lead as absence and never creates a replacement", async () => {
    const archived = agent("lead-old", { workspaceId: "wks-lsa", archivedAt: "2026-10-20", labels: { role: "LSA", epic: "ASMA-100" } });
    const s = await setup({ others: [archived] });
    const outcome = await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100" });
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringContaining("archived canonical LSA") });
    expect(s.send).not.toHaveBeenCalled();
    expect(s.run).not.toHaveBeenCalled();
  });

  it("refuses a legacy fixed ADHOC persona with an explicit same-ID refresh gap", async () => {
    const legacy = agent("agent-legacy", { labels: { role: "LSA", [TITLE_AUTO_LABEL]: sourceTitle, persona: "kontor-lsa-adhoc@79da7ff9c079" } });
    const s = await setup({ source: legacy });
    const outcome = await s.service.promote({ requestId: randomUUID(), agentId: legacy.id, epic: "ASMA-100" });
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringContaining("legacy fixed ADHOC persona") });
    expect(s.send).not.toHaveBeenCalled();
  });

  it("preserves an operator-renamed title while binding the logical node", async () => {
    const renamed = agent("agent-renamed", { title: "Operator named this review", labels: { role: "LSA", [TITLE_AUTO_LABEL]: sourceTitle, persona: PERSONA_VERSION } });
    const s = await setup({ source: renamed });
    const outcome = await s.service.promote({ requestId: randomUUID(), agentId: renamed.id, epic: "ASMA-100" });
    expect(outcome).toMatchObject({ state: "promoted", native: { title: "Operator named this review", titleAuto: sourceTitle } });
    expect(s.agents.get(renamed.id)!.title).toBe("Operator named this review");
  });

  it("keeps a promoted ADHOC session reissuable by the takeover recovery caller", async () => {
    const s = await setup();
    expect(await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100" })).toMatchObject({ state: "promoted" });
    const promoted = s.agents.get("agent-adhoc")!;
    const issued = reissuePersona(promoted.labels!, "claude", { desk: "DESK • ADAM", workspace: "ADHOC", workspaceId: "wks-adhoc" });
    expect("refused" in issued).toBe(false);
    if (!("refused" in issued)) expect(issued.version).toBe(PERSONA_VERSION);
  });

  it("serializes concurrent requests for the same promotion", async () => {
    const s = await setup();
    const input = { requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100" };
    const [first, second] = await Promise.all([s.service.promote(input), s.service.promote(input)]);
    expect([first.state, second.state]).toEqual(["promoted", "promoted"]);
    expect(s.send).toHaveBeenCalledTimes(1);
  });

  it("refuses a cross-epic rebind before dispatch", async () => {
    const bound = agent("agent-bound", { labels: { role: "LSA", epic: "ASMA-200", [TITLE_AUTO_LABEL]: sourceTitle, persona: PERSONA_VERSION } });
    const s = await setup({ source: bound });
    const outcome = await s.service.promote({ requestId: randomUUID(), agentId: bound.id, epic: "ASMA-100" });
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringContaining("already bound to ASMA-200") });
    expect(s.send).not.toHaveBeenCalled();
  });


  it("does not promote while a From Jira New-session intent for the same epic is unsettled", async () => {
    const s = await setup();
    await writeFile(join(s.root, `${randomUUID()}.json`), JSON.stringify({ request: { requestId: randomUUID(),
      labels: { role: "LSA", epic: "ASMA-100" } }, outcome: null }));
    expect(await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100" }))
      .toMatchObject({ state: "refused", message: expect.stringContaining("not settled") });
    expect(s.send).not.toHaveBeenCalled();
  });
  it("does not resend after an ambiguous context delivery and returns unknown", async () => {
    const s = await setup({ sendError: new Error("ack lost") });
    const input = { requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100" };
    expect(await s.service.promote(input)).toMatchObject({ state: "unknown", message: expect.stringContaining("Do not resend") });
    expect(await s.service.promote(input)).toMatchObject({ state: "unknown" });
    expect(s.send).toHaveBeenCalledTimes(1);
  });

  it("does not release or claim a handoff when the receiver does not ACK the exact revision", async () => {
    const canonical = agent("agent-canonical", { workspaceId: "wks-lsa", labels: { role: "LSA", epic: "ASMA-100", [TITLE_AUTO_LABEL]: "LSA • ASMA-100 • Other topic" } });
    const s = await setup({ others: [canonical], runText: "Handoff received." });
    const outcome = await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100", summary: { text: summary, revision: summaryRevision } });
    expect(outcome).toMatchObject({ state: "unknown", canonicalAgentId: canonical.id, acknowledgement: "unknown" });
    expect(s.run).toHaveBeenCalledTimes(1);
    expect(s.send).not.toHaveBeenCalled();
  });


  it("rejects the same request id when its epic payload changed", async () => {
    const s = await setup();
    const requestId = randomUUID();
    expect(await s.service.promote({ requestId, agentId: "agent-adhoc", epic: "ASMA-100" })).toMatchObject({ state: "promoted" });
    expect(await s.service.promote({ requestId, agentId: "agent-adhoc", epic: "ASMA-200" }))
      .toMatchObject({ state: "refused", message: expect.stringContaining("another session or epic") });
    expect(s.send).toHaveBeenCalledTimes(1);
  });

  it("rejects an existing lead without an explicitly authored summary revision", async () => {
    const canonical = agent("agent-canonical", { workspaceId: "wks-lsa", title: "LSA • ASMA-100 • Other topic",
      labels: { role: "LSA", epic: "ASMA-100", [TITLE_AUTO_LABEL]: "LSA • ASMA-100 • Other topic" } });
    const s = await setup({ others: [canonical] });
    expect(await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100" }))
      .toMatchObject({ state: "refused", message: expect.stringContaining("explicitly authored handoff summary") });
    expect(s.run).not.toHaveBeenCalled();
  });

  it("allows a legacy source to hand off to an existing lead without changing its instructions", async () => {
    const legacy = agent("agent-legacy", { labels: { role: "LSA", [TITLE_AUTO_LABEL]: sourceTitle, persona: "kontor-lsa-adhoc@79da7ff9c079" } });
    const canonical = agent("agent-canonical", { workspaceId: "wks-lsa", title: "LSA • ASMA-100 • Other topic",
      labels: { role: "LSA", epic: "ASMA-100", [TITLE_AUTO_LABEL]: "LSA • ASMA-100 • Other topic" } });
    const s = await setup({ source: legacy, others: [canonical] });
    expect(await s.service.promote({ requestId: randomUUID(), agentId: legacy.id, epic: "ASMA-100", summary: { text: summary, revision: summaryRevision } }))
      .toMatchObject({ state: "handoff", canonicalAgentId: canonical.id });
    expect(s.send).not.toHaveBeenCalled();
  });

  it("refuses when the source role changes during discovery before dispatch", async () => {
    const s = await setup();
    const original = s.list.getMockImplementation()!;
    s.list.mockImplementationOnce(async (options) => {
      const result = await original(options);
      const source = s.agents.get("agent-adhoc")!;
      s.agents.set(source.id, { ...source, labels: { ...source.labels, role: "TPM" } });
      return result;
    });
    expect(await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100" }))
      .toMatchObject({ state: "refused", message: expect.stringContaining("role or epic binding changed") });
    expect(s.send).not.toHaveBeenCalled();
  });

  it("does not certify an ACK from an errored or permission-blocked run", async () => {
    const canonical = agent("agent-canonical", { workspaceId: "wks-lsa", title: "LSA • ASMA-100 • Other topic",
      labels: { role: "LSA", epic: "ASMA-100", [TITLE_AUTO_LABEL]: "LSA • ASMA-100 • Other topic" } });
    const s = await setup({ others: [canonical] });
    s.run.mockResolvedValueOnce({ status: "error", lastMessage: ack, permission: null });
    expect(await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100", summary: { text: summary, revision: summaryRevision } }))
      .toMatchObject({ state: "unknown", acknowledgement: "unknown" });
  });

  it("does not certify an ACK when the receiver drifted before the acknowledgement", async () => {
    const canonical = agent("agent-canonical", { workspaceId: "wks-lsa", title: "LSA • ASMA-100 • Other topic",
      labels: { role: "LSA", epic: "ASMA-100", [TITLE_AUTO_LABEL]: "LSA • ASMA-100 • Other topic" } });
    const s = await setup({ others: [canonical] });
    s.run.mockImplementationOnce(async () => {
      s.agents.set(canonical.id, { ...canonical, labels: { ...canonical.labels, role: "TPM" } });
      return { status: "idle", lastMessage: ack, permission: null };
    });
    expect(await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100", summary: { text: summary, revision: summaryRevision } }))
      .toMatchObject({ state: "unknown", acknowledgement: "unknown" });
  });

  it("refuses a source exact read that returns another agent ID", async () => {
    const s = await setup();
    const source = s.agents.get("agent-adhoc")!;
    s.agents.set(source.id, { ...source, id: "wrong-source" });
    expect(await s.service.promote({ requestId: randomUUID(), agentId: source.id, epic: "ASMA-100" }))
      .toMatchObject({ state: "refused", message: expect.stringContaining("exact native placement") });
    expect(s.send).not.toHaveBeenCalled();
  });

  it("refuses a source exact read whose workspace ID changed", async () => {
    const s = await setup();
    s.homes.set("wks-adhoc", workspace("wrong-workspace", "ADHOC"));
    expect(await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100" }))
      .toMatchObject({ state: "refused", message: expect.stringContaining("exact native placement") });
    expect(s.send).not.toHaveBeenCalled();
  });

  it("refuses a receiver exact read that returns another agent ID", async () => {
    const canonical = agent("agent-canonical", { workspaceId: "wks-lsa", title: "LSA • ASMA-100 • Other topic",
      labels: { role: "LSA", epic: "ASMA-100", [TITLE_AUTO_LABEL]: "LSA • ASMA-100 • Other topic" } });
    const s = await setup({ others: [canonical] });
    s.agents.set(canonical.id, { ...canonical, id: "wrong-canonical" });
    expect(await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100", summary: { text: summary, revision: summaryRevision } }))
      .toMatchObject({ state: "refused" });
    expect(s.run).not.toHaveBeenCalled();
  });

  it("refuses a label-only receiver whose epic label disappears during revalidation", async () => {
    const canonical = agent("agent-canonical", { workspaceId: "wks-lsa", title: "LSA • ASMA-100 • Other topic",
      labels: { role: "LSA", epic: "ASMA-100", [TITLE_AUTO_LABEL]: "LSA • ASMA-100 • Other topic" } });
    const s = await setup({ others: [canonical] });
    const original = s.list.getMockImplementation()!;
    s.list.mockImplementationOnce(async (options) => {
      const result = await original(options);
      const { epic: _epic, ...labels } = canonical.labels ?? {};
      s.agents.set(canonical.id, { ...canonical, labels });
      return result;
    });
    expect(await s.service.promote({ requestId: randomUUID(), agentId: "agent-adhoc", epic: "ASMA-100", summary: { text: summary, revision: summaryRevision } }))
      .toMatchObject({ state: "refused", message: expect.stringContaining("exact epic label") });
    expect(s.run).not.toHaveBeenCalled();
  });
  it("refuses a source that is not an ADHOC LSA and a closed or non-epic key before any effect", async () => {
    const tpm = agent("agent-tpm", { labels: { role: "TPM", [TITLE_AUTO_LABEL]: "TPM • ADHOC • Route", persona: PERSONA_VERSION } });
    const s = await setup({ source: tpm });
    expect(await s.service.promote({ requestId: randomUUID(), agentId: tpm.id, epic: "ASMA-100" }))
      .toMatchObject({ state: "refused", message: expect.stringContaining("Only an ADHOC LSA") });
    expect(s.send).not.toHaveBeenCalled();
    s.jira.mockResolvedValueOnce({ issues: [{ ...(await s.jira({ query: "ASMA-100", issueClass: "epic", openOnly: true })).issues[0]!, open: false, status: "Done", statusCategory: "Done" }], hasMore: false, nextPageToken: null, failure: null });
    const source = agent("agent-adhoc");
    s.agents.set(source.id, source);
    expect(await s.service.promote({ requestId: randomUUID(), agentId: source.id, epic: "ASMA-100" }))
      .toMatchObject({ state: "refused", message: expect.stringContaining("not open") });
  });
});
