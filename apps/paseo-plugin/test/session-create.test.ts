import { mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult, PaseoWorkspace } from "@getpaseo/client";
import { MutableDaemonConfigSchema } from "@getpaseo/protocol/messages";
import { adamDesks, createSessionCreateService, createSwitchProblem, PERSONA_VERSION, type CreateClient } from "../server/session-create";
import { adhocTitle, deskName, isAdamWorkspace, normalizeTopic, topicProblem, type CreateRequest } from "../shared/session-create";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
type NativeCreate = ReturnType<CreateClient["workspaces"]["ref"]>["agents"]["create"];
function agent(id: string, patch: Partial<Agent> = {}): Agent {
  return { id, provider: "claude", model: "opus", effectiveThinkingOptionId: "high", cwd: "/fixtures/wks-adam", workspaceId: "wks-adam",
    title: "Fixture session", status: "idle", lastError: null, createdAt: "2026-10-22", updatedAt: "2026-10-22", lastUserMessageAt: null,
    capabilities: { supportsStreaming: true, supportsSessionPersistence: true, supportsDynamicModes: false, supportsMcpServers: true, supportsReasoningStream: true, supportsToolInvocations: true },
    currentModeId: "default", availableModes: [], pendingPermissions: [], persistence: null, labels: {},
    requiresAttention: false, attentionReason: null, attentionTimestamp: null, archivedAt: null, ...patch } as Agent;
}
function workspace(id: string, project: string, name: string, patch: Partial<PaseoWorkspace> = {}): PaseoWorkspace {
  return { id, projectId: `project-${id}`, projectDisplayName: project, projectRootPath: `/fixtures/${id}`, projectKind: "directory", workspaceKind: "local_checkout",
    name, title: null, workspaceDirectory: `/fixtures/${id}`, archivingAt: null, status: "done", statusEnteredAt: null, activityAt: null, scripts: [], ...patch };
}
const PROJECT: PaseoAgentListResult["entries"][number]["project"] = { projectKey: "fixture", projectName: "DESK • ADAM",
  checkout: { cwd: "/fixtures/wks-adam", isGit: false, worktreeRoot: null, currentBranch: null, remoteUrl: null, isPaseoOwnedWorktree: false, mainRepoRoot: null } };
const WORKSPACES = [
  workspace("wks-pr", "DESK • PR REVIEW", "adam"),
  workspace("wks-adam", "DESK • ADAM", "ADAM"),
  workspace("wks-repo", "kontor", "ADAM"),
  workspace("wks-dev", "DESK • ADAM", "DEV"),
  workspace("wks-closing", "DESK • OPS", "ADAM", { archivingAt: "2026-10-22" } as Partial<PaseoWorkspace>),
];
const SNAPSHOT = { requestId: "fixture", generatedAt: "2026-10-22", entries: [
  { provider: "claude", label: "Claude", enabled: true, status: "ready", defaultModeId: "default",
    models: [{ provider: "claude", id: "opus", label: "Opus", isDefault: true, defaultThinkingOptionId: "high", thinkingOptions: [{ id: "high", label: "High" }, { id: "max", label: "Max" }] },
      { provider: "claude", id: "legacy", label: "Legacy", isSelectable: false }],
    modes: [{ id: "default", label: "Default", colorTier: "safe" }, { id: "bypassPermissions", label: "Bypass", colorTier: "dangerous" }] },
  { provider: "gemini", label: "Gemini", enabled: true, status: "error", error: "not signed in", models: [] },
  { provider: "codex", label: "Codex", enabled: false, status: "ready", models: [{ provider: "codex", id: "gpt", label: "GPT" }] },
] };

const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

async function setup({ switchText = '{"create":"on"}' as string | null, answerMs = 5_000, existing = [] as Agent[] } = {}) {
  const root = await mkdtemp(join(tmpdir(), "kontor-create-test-")); dirs.push(root);
  const switchFile = join(root, "create.json"), stateRoot = join(root, "state");
  if (switchText !== null) await writeFile(switchFile, switchText);
  const agents = new Map(existing.map((value) => [value.id, value]));
  const intents: unknown[] = [];
  let readbackPatch: Partial<Agent> = {};
  const create = vi.fn<NativeCreate>(async (input) => {
    // Intent precedes effect: the request is on disk, unsettled, when the host is asked.
    intents.push(JSON.parse(await readFile(join(stateRoot, `${input.idempotencyKey}.json`), "utf8")));
    const value = agent("agent-new", { title: input.title, labels: input.labels, currentModeId: input.config.modeId, effectiveThinkingOptionId: input.config.thinkingOptionId, ...readbackPatch });
    agents.set(value.id, value);
    return { id: value.id } as Awaited<ReturnType<NativeCreate>>;
  });
  const list = vi.fn<CreateClient["agents"]["list"]>(async (options) => ({ requestId: "fixture", pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
    entries: [...agents.values()].filter((value) => Object.entries(options?.filter?.labels ?? {}).every(([key, label]) => value.labels?.[key] === label)).map((value) => ({ agent: value, project: PROJECT })) }));
  const snapshot = vi.fn(async () => SNAPSHOT);
  const client = {
    workspaces: {
      list: vi.fn(async () => ({ requestId: "fixture", entries: WORKSPACES, pageInfo: { hasMore: false, nextCursor: null, prevCursor: null } })),
      ref: (id: string) => ({ refresh: async () => WORKSPACES.find((entry) => entry.id === id) ?? null, agents: { create } }),
    },
    agents: { list, ref: (id: string) => ({ refresh: async () => { const value = agents.get(id); return value ? { agent: value, project: PROJECT } : null; } }) },
    providers: { snapshot },
    config: { get: vi.fn(async () => ({ requestId: "fixture", config: MutableDaemonConfigSchema.parse({ mcp: { injectIntoAgents: false }, agentProfiles: [{ id: "profile-1", name: "lsa", provider: "claude", model: "opus", modeId: "default", thinkingOptionId: "max" }] }) })) },
  } as CreateClient;
  const service = createSessionCreateService(client, stateRoot, switchFile, answerMs);
  return { service, client, create, list, snapshot, agents, intents, stateRoot, setReadback: (patch: Partial<Agent>) => { readbackPatch = patch; } };
}
const request = (patch: Partial<CreateRequest> = {}): CreateRequest => ({ requestId: randomUUID(), topic: "  Route  review ", workspaceId: "wks-adam", provider: "claude", model: "opus", effort: "max", modeId: "default", ...patch });

describe("ad-hoc LSA title grammar", () => {
  it("normalizes a topic and refuses separators, control characters and bad lengths", () => {
    expect(normalizeTopic("  Route \n  review ")).toBe("Route review");
    expect(normalizeTopic("Café")).toBe("Café");
    expect(topicProblem("Route review")).toBeNull();
    for (const bad of ["A • B", "A · B", "Tab\u0007bell", "ab", "x".repeat(49)]) expect(topicProblem(bad)).toEqual(expect.any(String));
    expect(adhocTitle("Route review")).toBe("LSA • ADHOC • Route review");
  });

  it("reads a desk from its project name and an ADAM workspace in any case", () => {
    expect(deskName("DESK • PR REVIEW")).toBe("PR REVIEW");
    expect(deskName("kontor")).toBeNull();
    expect(isAdamWorkspace(" adam ")).toBe(true);
    expect(isAdamWorkspace("ADAM-2")).toBe(false);
    expect(adamDesks(WORKSPACES).map((desk) => desk.workspaceId)).toEqual(["wks-adam", "wks-pr"]);
  });
});

describe("create switch", () => {
  it.each([
    ['{"create":"on"}', null],
    [null, "off on this host"],
    ['{"create":"off"}', "switched off"],
    ['{"create":true}', "not valid"],
    ['{"create":"on","extra":1}', "not valid"],
    ['{"create":"on","create":"on"}', "not valid"],
    ['{"floor":"on"}', "not valid"],
    ["not json", "not valid"],
  ])("reads %s as %s", async (text, expected) => {
    const root = await mkdtemp(join(tmpdir(), "kontor-create-switch-")); dirs.push(root);
    const file = join(root, "create.json");
    if (text !== null) await writeFile(file, text);
    const problem = createSwitchProblem(file);
    if (expected === null) expect(problem).toBeNull(); else expect(problem).toContain(expected);
  });
});

describe("create options", () => {
  it("offers desk ADAM workspaces, providers with reasons, and the host's LSA profile", async () => {
    const options = await (await setup()).service.options();
    expect(options).toMatchObject({ enabled: true, disabledReason: null, pending: null, notes: [],
      profile: { name: "lsa", provider: "claude", model: "opus", modeId: "default", effort: "max" } });
    expect(options.desks).toEqual([
      { workspaceId: "wks-adam", projectId: "project-wks-adam", desk: "ADAM", project: "DESK • ADAM" },
      { workspaceId: "wks-pr", projectId: "project-wks-pr", desk: "PR REVIEW", project: "DESK • PR REVIEW" },
    ]);
    const [claude, gemini, codex] = options.providers;
    expect(claude).toMatchObject({ available: true, persona: "supported", defaultModel: "opus", defaultMode: "default",
      models: [{ id: "opus", efforts: [{ id: "high" }, { id: "max" }], defaultEffort: "high" }],
      modes: [{ id: "default", dangerous: false }, { id: "bypassPermissions", dangerous: true }] });
    expect(gemini).toMatchObject({ available: false, persona: "unverified", note: "Provider error: not signed in." });
    expect(codex).toMatchObject({ available: false, persona: "supported", note: "Disabled on this host." });
  });

  it("is off without the operator's switch, and notes an unreadable profile list", async () => {
    const s = await setup({ switchText: null });
    vi.mocked(s.client.config.get).mockRejectedValueOnce(new Error("config unavailable"));
    expect(await s.service.options()).toMatchObject({ enabled: false, disabledReason: expect.stringContaining("off on this host"), profile: null, notes: ["Agent profiles unavailable: config unavailable."] });
  });
});

describe("create", () => {
  it("dispatches one idempotent create with the exact title, labels and config, then verifies the readback", async () => {
    const s = await setup(), input = request();
    const outcome = await s.service.create(input);
    const title = "LSA • ADHOC • Route review";
    expect(s.create).toHaveBeenCalledTimes(1);
    const sent = s.create.mock.calls[0]![0];
    expect(sent).toEqual({ idempotencyKey: input.requestId, title,
      labels: { role: "LSA", "title.auto": title, persona: PERSONA_VERSION, "create.request": input.requestId },
      config: { provider: "claude/opus", modeId: "default", thinkingOptionId: "max", systemPrompt: expect.stringContaining(`Your Paseo title is "${title}"`) } });
    expect(Object.keys(sent.labels!).some((key) => key.startsWith("kontor.") || key.includes("orchestration"))).toBe(false);
    expect(sent.config.systemPrompt).toContain("desk DESK • ADAM, workspace ADAM (wks-adam)");
    expect(sent).not.toHaveProperty("initialPrompt");
    expect(s.intents).toEqual([expect.objectContaining({ outcome: null, request: expect.objectContaining({ requestId: input.requestId, title }) })]);
    expect(outcome).toMatchObject({ state: "verified", agentId: "agent-new", title, persona: "supported", differences: [] });
    expect(JSON.parse(await readFile(join(s.stateRoot, `${input.requestId}.json`), "utf8")).outcome).toEqual(outcome);
  });

  it("answers a repeated request from its record without a second dispatch", async () => {
    const s = await setup(), input = request();
    const first = await s.service.create(input);
    expect(await s.service.create(input)).toEqual(first);
    expect(s.create).toHaveBeenCalledTimes(1);
  });

  it("reports readback differences instead of claiming a match", async () => {
    const s = await setup();
    s.setReadback({ model: "sonnet", workspaceId: "wks-elsewhere" });
    const outcome = await s.service.create(request());
    expect(outcome).toMatchObject({ state: "differences", agentId: "agent-new" });
    expect(outcome.differences).toEqual(["Placed outside the chosen ADAM workspace.", "Model reads sonnet."]);
  });

  it("reports a working directory mismatch even when the workspace ID matches", async () => {
    const s = await setup();
    s.setReadback({ cwd: "/fixtures/elsewhere" });
    expect(await s.service.create(request())).toMatchObject({ state: "differences", differences: ["Working directory differs from the chosen ADAM workspace."] });
  });

  it.each([
    ["the switch is off", { switchText: '{"create":"off"}' }, {}, "switched off"],
    ["the topic forges a segment", {}, { topic: "A • B" }, "cannot contain"],
    ["the workspace is not a desk ADAM", {}, { workspaceId: "wks-repo" }, "no longer a desk's ADAM workspace"],
    ["the provider is unavailable", {}, { provider: "gemini", model: "x", effort: null, modeId: null }, "not signed in"],
    ["the model is not selectable", {}, { model: "legacy" }, "not selectable"],
    ["the effort is not offered", {}, { effort: "turbo" }, "Effort turbo"],
    ["the mode is not offered", {}, { modeId: "plan" }, "Mode plan"],
    ["the title names a live session there", { existing: [agent("agent-old", { title: "LSA • ADHOC • Route review" })] }, {}, "already names a session"],
  ])("refuses without dispatch or record when %s", async (_label, options, patch, message) => {
    const s = await setup(options);
    const outcome = await s.service.create(request(patch));
    expect(outcome).toMatchObject({ state: "refused", agentId: null, message: expect.stringContaining(message) });
    expect(outcome.message).toContain("Nothing was created.");
    expect(s.create).not.toHaveBeenCalled();
    await expect(readdir(s.stateRoot)).rejects.toThrow();
  });

  it("allows the same title once the earlier session is archived", async () => {
    const s = await setup({ existing: [agent("agent-old", { title: "LSA • ADHOC • Route review", archivedAt: "2026-10-21" })] });
    expect((await s.service.create(request())).state).toBe("verified");
  });

  it("refuses when the host cannot be read before dispatch", async () => {
    const s = await setup();
    s.snapshot.mockRejectedValueOnce(new Error("socket closed"));
    expect(await s.service.create(request())).toMatchObject({ state: "refused", message: expect.stringContaining("socket closed") });
    expect(s.create).not.toHaveBeenCalled();
  });

  it("keeps a failed dispatch unknown when Check finds no correlated session", async () => {
    const s = await setup(), first = request();
    s.create.mockRejectedValueOnce(new Error("transport closed"));
    expect(await s.service.create(first)).toMatchObject({ state: "unknown", agentId: null, message: expect.stringContaining("Do not retry") });
    expect((await s.service.options()).pending).toMatchObject({ requestId: first.requestId, state: "unknown" });
    expect(await s.service.create(request({ topic: "Another topic" }))).toMatchObject({ state: "refused", message: expect.stringContaining("Check it first") });
    expect(await s.service.check(first.requestId)).toMatchObject({ state: "unknown", agentId: null });
    expect(s.list).toHaveBeenLastCalledWith({ filter: { labels: { "create.request": first.requestId }, includeArchived: true }, page: { limit: 10 } });
    expect((await s.service.options()).pending).toMatchObject({ requestId: first.requestId, state: "unknown" });
    expect((await s.service.create(request({ topic: "Another topic" }))).state).toBe("refused");
    expect(s.create).toHaveBeenCalledTimes(1);
  });

  it("reports unknown when the host does not answer in time, and Check finds the late session by its request label", async () => {
    const s = await setup({ answerMs: 20 }), input = request();
    let finish: (() => void) | undefined;
    s.create.mockImplementationOnce(async (sent) => {
      await new Promise<void>((resolve) => { finish = resolve; });
      const value = agent("agent-late", { title: sent.title, labels: sent.labels, effectiveThinkingOptionId: "max" });
      s.agents.set(value.id, value);
      return { id: value.id } as Awaited<ReturnType<NativeCreate>>;
    });
    expect(await s.service.create(input)).toMatchObject({ state: "unknown", message: expect.stringContaining("no answer within 0 s") });
    expect(await s.service.check(input.requestId)).toMatchObject({ state: "unknown" });
    expect((await s.service.create(request({ topic: "Another topic" }))).state).toBe("refused");
    finish!();
    await vi.waitFor(() => expect(s.agents.has("agent-late")).toBe(true));
    expect(await s.service.check(input.requestId)).toMatchObject({ state: "verified", agentId: "agent-late" });
    expect(s.create).toHaveBeenCalledTimes(1);
  });

  it("keeps an unreadable host read on Check unknown", async () => {
    const s = await setup(), input = request();
    s.create.mockRejectedValueOnce(new Error("transport closed"));
    await s.service.create(input);
    s.list.mockRejectedValueOnce(new Error("host restarting"));
    expect(await s.service.check(input.requestId)).toMatchObject({ state: "unknown", message: expect.stringContaining("host restarting") });
  });

  it("keeps a pending uppercase request ID fenced across service instances", async () => {
    const s = await setup(), input = request({ requestId: randomUUID().toUpperCase() });
    s.create.mockRejectedValueOnce(new Error("transport closed"));
    await s.service.create(input);
    expect((await s.service.options()).pending).toMatchObject({ requestId: input.requestId });
    expect(await s.service.create(request({ topic: "Another topic" }))).toMatchObject({ state: "refused" });
    expect(s.create).toHaveBeenCalledTimes(1);
  });

  it("does not pick one of several sessions carrying the same request label", async () => {
    const s = await setup(), input = request();
    s.create.mockRejectedValueOnce(new Error("transport closed"));
    await s.service.create(input);
    for (const id of ["agent-a", "agent-b"]) s.agents.set(id, agent(id, { labels: { "create.request": input.requestId } }));
    expect(await s.service.check(input.requestId)).toMatchObject({ state: "unknown", agentId: null, message: expect.stringContaining("Multiple sessions") });
    expect(s.create).toHaveBeenCalledTimes(1);
  });

  it("refuses Check for a request this host never recorded", async () => {
    await expect((await setup()).service.check(randomUUID())).rejects.toThrow("no record of that creation request");
  });

  it("walks every Check page and keeps an incomplete host read unknown", async () => {
    const s = await setup(), input = request();
    s.create.mockRejectedValueOnce(new Error("transport closed"));
    await s.service.create(input);
    s.agents.set("agent-later-page", agent("agent-later-page", { title: "LSA • ADHOC • Route review", effectiveThinkingOptionId: "max",
      labels: { role: "LSA", "title.auto": "LSA • ADHOC • Route review", persona: PERSONA_VERSION, "create.request": input.requestId } }));
    s.list.mockResolvedValueOnce({ requestId: "fixture", pageInfo: { hasMore: true, nextCursor: null, prevCursor: null }, entries: [] });
    expect(await s.service.check(input.requestId)).toMatchObject({ state: "unknown" });
    s.list.mockResolvedValueOnce({ requestId: "fixture", pageInfo: { hasMore: true, nextCursor: "page-2", prevCursor: null }, entries: [] });
    expect(await s.service.check(input.requestId)).toMatchObject({ state: "verified", agentId: "agent-later-page" });
    expect(s.list).toHaveBeenLastCalledWith({ filter: { labels: { "create.request": input.requestId }, includeArchived: true }, page: { limit: 10, cursor: "page-2" } });
  });

  it("blocks every new create while a creation record cannot be read", async () => {
    const s = await setup();
    await s.service.create(request());
    await writeFile(join(s.stateRoot, `${randomUUID()}.json`), "{partial");
    expect(await s.service.create(request({ topic: "Another topic" }))).toMatchObject({ state: "refused", message: expect.stringContaining("cannot be read") });
    expect(await s.service.options()).toMatchObject({ enabled: false, disabledReason: expect.stringContaining("cannot be read") });
    expect(s.create).toHaveBeenCalledTimes(1);
  });
});
