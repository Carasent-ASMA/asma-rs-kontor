import { chmod, mkdir, mkdtemp, readFile, readdir, rm, symlink, writeFile } from "node:fs/promises";
import { randomUUID } from "node:crypto";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult, PaseoWorkspace } from "@getpaseo/client";
import { MutableDaemonConfigSchema } from "@getpaseo/protocol/messages";
import { SCOPE_LABEL_RULE, createProviders, createSessionCreateService, createSwitchProblem, deskProjects, desksOf, personaFor, PERSONA_VERSION, sourceSummary, type CreateClient, type JiraSearch } from "../server/session-create";
import { deskIssueProblem, deskKind, issueDesk, issueTopic, leadTitle, leadWorkspaceName, normalizeTopic, topicProblem, type CreateRequest, type JiraIssue } from "../shared/session-create";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
type NativeCreate = ReturnType<CreateClient["workspaces"]["ref"]>["agents"]["create"];
type NativeWorkspaceCreate = CreateClient["workspaces"]["create"];
function agent(id: string, patch: Partial<Agent> = {}): Agent {
  return { id, provider: "claude", model: "opus", effectiveThinkingOptionId: "high", cwd: "/fixtures/wks-adam-anchor", workspaceId: "wks-adam-lsa",
    title: "Fixture session", status: "idle", lastError: null, createdAt: "2026-10-22", updatedAt: "2026-10-22", lastUserMessageAt: null,
    capabilities: { supportsStreaming: true, supportsSessionPersistence: true, supportsDynamicModes: false, supportsMcpServers: true, supportsReasoningStream: true, supportsToolInvocations: true },
    currentModeId: "default", availableModes: [], pendingPermissions: [], persistence: null, labels: {},
    requiresAttention: false, attentionReason: null, attentionTimestamp: null, archivedAt: null, ...patch } as Agent;
}
function workspace(id: string, projectId: string, project: string, name: string, patch: Partial<PaseoWorkspace> = {}): PaseoWorkspace {
  return { id, projectId, projectDisplayName: project, projectRootPath: `/fixtures/${id}`, projectKind: "directory", workspaceKind: "local_checkout",
    name, title: null, workspaceDirectory: `/fixtures/${id}`, archivingAt: null, status: "done", statusEnteredAt: null, activityAt: null, scripts: [], ...patch };
}
const WORKSPACES = [
  workspace("wks-adam-anchor", "prj-adam", "DESK • ADAM", "ADAM"),
  workspace("wks-adam-lsa", "prj-adam", "DESK • ADAM", "LSA"),
  workspace("wks-adam-tpm", "prj-adam", "DESK • ADAM", "TPM"),
  workspace("wks-tickets-tsw", "prj-tickets", "DESK • TICKETS", "TSW • ASMA-101 • Replace"),
  workspace("wks-tickets-lsa", "prj-tickets", "DESK • TICKETS", "LSA"),
  workspace("wks-tickets-tpm", "prj-tickets", "DESK • TICKETS", "TPM"),
  workspace("wks-tickets-adhoc", "prj-tickets", "DESK • TICKETS", "ADHOC"),
  workspace("wks-pr-anchor", "prj-pr", "DESK • PR REVIEW", "ADAM"),
  workspace("wks-pr-lsa", "prj-pr", "DESK • PR REVIEW", "LSA"),
  workspace("wks-repo", "prj-repo", "kontor", "ADAM"),
  workspace("wks-closing", "prj-ops", "DESK • OPS", "ADAM", { archivingAt: "2026-10-22" }),
];
const PROJECT: PaseoAgentListResult["entries"][number]["project"] = { projectKey: "fixture", projectName: "DESK • ADAM",
  checkout: { cwd: "/fixtures/wks-adam-anchor", isGit: false, worktreeRoot: null, currentBranch: null, remoteUrl: null, isPaseoOwnedWorktree: false, mainRepoRoot: null } };
const SNAPSHOT: Awaited<ReturnType<CreateClient["providers"]["snapshot"]>> = { requestId: "fixture", generatedAt: "2026-10-22", entries: [
  { provider: "claude", label: "Claude", enabled: true, status: "ready", defaultModeId: "default",
    models: [{ provider: "claude", id: "opus", label: "Opus", isDefault: true, defaultThinkingOptionId: "high", thinkingOptions: [{ id: "high", label: "High" }, { id: "max", label: "Max" }] },
      { provider: "claude", id: "legacy", label: "Legacy", isSelectable: false }],
    modes: [{ id: "default", label: "Default", colorTier: "safe" }, { id: "bypassPermissions", label: "Bypass", colorTier: "dangerous" }] },
  { provider: "opencode", label: "OpenCode", enabled: true, status: "ready", models: [{ provider: "opencode", id: "big", label: "Big" }] },
  { provider: "gemini", label: "Gemini", enabled: true, status: "error", error: "not signed in", models: [] },
  { provider: "codex", label: "Codex", enabled: false, status: "ready", models: [{ provider: "codex", id: "gpt", label: "GPT" }] },
] };

const epic: JiraIssue = { id: "100", key: "ASMA-100", summary: "Deliver the desk layout", created: "2026-10-01T10:00:00.000+0000", type: "Epic", epic: true, status: "In Development", statusCategory: "In Progress", open: true, parent: null };
const underActive: JiraIssue = { id: "101", key: "ASMA-101", summary: "Replace New LSA with a role-aware New session", created: "2026-10-02T10:00:00.000+0000", type: "User Story", epic: false, status: "In Development", statusCategory: "In Progress", open: true,
  parent: { key: "ASMA-100", summary: "Deliver the desk layout", statusCategory: "In Progress", open: true } };
const underDone: JiraIssue = { ...underActive, id: "102", key: "ASMA-102", created: "2026-10-03T10:00:00.000+0000", parent: { ...underActive.parent!, statusCategory: "Done", open: false } };
const standalone: JiraIssue = { ...underActive, id: "103", key: "ASMA-103", created: "2026-10-04T10:00:00.000+0000", parent: null };
const closedEpic: JiraIssue = { ...epic, id: "104", key: "ASMA-104", created: "2026-10-05T10:00:00.000+0000", status: "Done", statusCategory: "Done", open: false };
const ISSUES = [epic, underActive, underDone, standalone, closedEpic];

const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });

async function setup({ switchText = null as string | null, answerMs = 5_000, existing = [] as Agent[], offline = false, timelineText = "Handoff: the plan is approved." as string | null,
  workspaceProblem = null as string | null, failWorkspaceCreate = false } = {}) {
  const root = await mkdtemp(join(tmpdir(), "kontor-create-test-")); dirs.push(root);
  const switchFile = join(root, "create.json"), stateRoot = join(root, "state");
  if (switchText !== null) await writeFile(switchFile, switchText);
  const agents = new Map(existing.map((value) => [value.id, value]));
  const live = [...WORKSPACES];
  const intents: unknown[] = [];
  let readbackPatch: Partial<Agent> = {}, dispatchWorkspace = "";
  const create = vi.fn<NativeCreate>(async (input) => {
    // Intent precedes effect: the request is on disk, unsettled, when the host is asked.
    intents.push(JSON.parse(await readFile(join(stateRoot, `${input.idempotencyKey}.json`), "utf8")));
    const placed = live.find((entry) => entry.id === dispatchWorkspace)!;
    const [provider, model] = String(input.config.provider).split("/");
    const value = agent("agent-new", { title: input.title, labels: input.labels, provider, model, cwd: placed.workspaceDirectory!, workspaceId: placed.id,
      currentModeId: input.config.modeId, effectiveThinkingOptionId: input.config.thinkingOptionId, ...readbackPatch });
    agents.set(value.id, value);
    return { id: value.id } as Awaited<ReturnType<NativeCreate>>;
  });
  const workspacesCreate = vi.fn<NativeWorkspaceCreate>(async (input) => {
    if (failWorkspaceCreate) throw new Error("host refused workspace creation");
    const source = input.source as { kind: "directory"; path: string; projectId?: string };
    const owner = live.find((entry) => entry.projectId === source.projectId);
    const created = workspace("wks-created", source.projectId!, owner?.projectDisplayName ?? "DESK • ADAM", String(input.title),
      { workspaceDirectory: source.path, title: workspaceProblem ?? String(input.title) });
    live.push(created);
    return { id: created.id, refresh: async () => live.find((entry) => entry.id === created.id) ?? null } as Awaited<ReturnType<NativeWorkspaceCreate>>;
  });
  const list = vi.fn<CreateClient["agents"]["list"]>(async (options) => ({ requestId: "fixture", pageInfo: { hasMore: false, nextCursor: null, prevCursor: null },
    entries: [...agents.values()].filter((value) => Object.entries(options?.filter?.labels ?? {}).every(([key, label]) => value.labels?.[key] === label)).map((value) => ({ agent: value, project: PROJECT })) }));
  const snapshot = vi.fn<CreateClient["providers"]["snapshot"]>(async () => SNAPSHOT);
  const timeline = vi.fn(async () => ({ entries: timelineText === null ? [] : [{ item: { type: "assistant_message", text: timelineText } }] }));
  const jira = vi.fn<JiraSearch>(async ({ query, issueClass, openOnly }) => {
    if (offline) return { issues: [], hasMore: false, nextPageToken: null, failure: "Jira is unreachable" };
    const needle = query.trim().toLowerCase(), exactKey = /^asma-[1-9]\d*$/.test(needle);
    const eligible = (issue: JiraIssue) => issue.epic || !issue.parent?.open;
    const classMatches = (issue: JiraIssue) => exactKey || issueClass === null || issueClass === "eligible"
      ? (exactKey || eligible(issue))
      : issueClass === "epic" ? issue.epic : !issue.epic && eligible(issue);
    return { issues: ISSUES.filter((issue) => (!needle || issue.key.toLowerCase().startsWith(needle) || issue.summary.toLowerCase().includes(needle))
      && classMatches(issue) && (!openOnly || issue.open)), hasMore: false, nextPageToken: null, failure: null };
  });
  const client = {
    workspaces: {
      list: vi.fn(async () => ({ requestId: "fixture", entries: live, pageInfo: { hasMore: false, nextCursor: null, prevCursor: null } })),
      ref: (id: string) => ({ refresh: async () => live.find((entry) => entry.id === id) ?? null, agents: { create: (input: Parameters<NativeCreate>[0]) => { dispatchWorkspace = id; return create(input); } } }),
      create: workspacesCreate,
    },
    agents: { list, ref: (id: string) => ({ refresh: async () => { const value = agents.get(id); return value ? { agent: value, project: PROJECT } : null; }, timeline: { refetch: timeline } }) },
    providers: { snapshot },
    config: { get: vi.fn(async () => ({ requestId: "fixture", config: MutableDaemonConfigSchema.parse({ mcp: { injectIntoAgents: false }, agentProfiles: [
      { id: "profile-lsa", name: "lsa", provider: "claude", model: "opus", modeId: "default", thinkingOptionId: "max" },
      { id: "profile-tpm", name: "tpm", provider: "opencode", model: "big" }] }) })) },
  } as unknown as CreateClient;
  const service = createSessionCreateService(client, stateRoot, switchFile, answerMs, jira);
  return { service, client, create, workspacesCreate, list, snapshot, agents, intents, stateRoot, switchFile, timeline, jira, live,
    setReadback: (patch: Partial<Agent>) => { readbackPatch = patch; }, setOffline: (value: boolean) => { offline = value; } };
}
const request = (patch: Partial<CreateRequest> = {}): CreateRequest => ({ requestId: randomUUID(), role: "LSA", start: "ad-hoc", projectId: "prj-adam", topic: " Route  review ", provider: "claude", model: "opus", effort: "max", modeId: "default", ...patch });

describe("desk and title grammar", () => {
  it("normalizes a topic and refuses separators, control characters and bad lengths", () => {
    expect(normalizeTopic("  Route \n  review ")).toBe("Route review");
    expect(normalizeTopic("Café")).toBe("Café");
    expect(topicProblem("Route review")).toBeNull();
    for (const bad of ["A • B", "A · B", "Tab\u0007bell", "ab", "x".repeat(49)]) expect(topicProblem(bad)).toEqual(expect.any(String));
    expect(issueTopic("Replace New LSA with a role-aware New session and From Jira ticket")).toBe("Replace New LSA with a role-aware New session an");
  });

  it("builds role-aware titles and chooses the lead workspace by role and start", () => {
    expect(leadTitle("LSA", null, "Route review")).toBe("LSA • ADHOC • Route review");
    expect(leadTitle("LSA", "ASMA-100", "Route review")).toBe("LSA • ASMA-100 • Route review");
    expect(leadTitle("TPM", "ASMA-100", "Route review")).toBe("TPM • ASMA-100 • Route review");
    expect(leadWorkspaceName("LSA", "ad-hoc")).toBe("ADHOC");
    expect(leadWorkspaceName("LSA", "from-jira")).toBe("LSA");
    expect(leadWorkspaceName("TPM", "from-jira")).toBe("TPM");
    expect(["ADAM", "TICKETS", "PR REVIEW", "OPS"].map(deskKind)).toEqual(["adam", "tickets", "pr-review", "other"]);
  });

  it("reads every desk project with its lead workspaces, ADAM first, skipping non-desks and archiving projects", () => {
    const desks = desksOf(WORKSPACES);
    expect(desks.map((desk) => [desk.projectId, desk.kind, desk.anchorWorkspaceId])).toEqual([["prj-adam", "adam", "wks-adam-anchor"], ["prj-pr", "pr-review", "wks-pr-anchor"], ["prj-tickets", "tickets", "wks-tickets-lsa"]]);
    expect(deskProjects(WORKSPACES)[0]).toMatchObject({ project: "DESK • ADAM", cwd: "/fixtures/wks-adam-anchor",
      lead: { ADHOC: null, LSA: "wks-adam-lsa", TPM: "wks-adam-tpm" } });
    expect(deskProjects(WORKSPACES)[2]!.lead).toEqual({ ADHOC: "wks-tickets-adhoc", LSA: "wks-tickets-lsa", TPM: "wks-tickets-tpm" });
  });

  it("anchors a desk on its ADAM or lead checkout, never on a ticket TSW or a project root", () => {
    const tickets = desksOf([workspace("wks-tsw", "prj-t", "DESK • TICKETS", "TSW • ASMA-1 • One"), workspace("wks-lsa", "prj-t", "DESK • TICKETS", "LSA")])[0]!;
    expect(tickets).toMatchObject({ cwd: "/fixtures/wks-lsa", anchorWorkspaceId: "wks-lsa" });
    expect(desksOf([workspace("wks-tsw", "prj-new", "DESK • NEW", "TSW • ASMA-1 • One",
      { projectRootPath: "/fixtures/desk-root", workspaceDirectory: "/fixtures/tsw" })])).toEqual([]);
  });

  it("refuses a desk with no checkout instead of creating in its non-git project root", async () => {
    const s = await setup();
    s.live.push(workspace("wks-empty-tsw", "prj-empty", "DESK • TICKETS", "TSW • ASMA-9 • One"));
    const refused = await s.service.create(request({ projectId: "prj-empty" }));
    expect(refused).toMatchObject({ state: "refused", message: expect.stringContaining("no checkout; create the lead workspaces through adoption first") });
    expect(s.intents).toEqual([]);
    expect(s.create).not.toHaveBeenCalled();
    expect(s.workspacesCreate).not.toHaveBeenCalled();
  });

  it("gates a From Jira issue by desk kind, open status and an active parent epic", () => {
    const adam = { project: "DESK • ADAM", kind: "adam" as const }, tickets = { project: "DESK • TICKETS", kind: "tickets" as const };
    expect(deskIssueProblem(adam, epic)).toBeNull();
    expect(deskIssueProblem(adam, standalone)).toContain("not an epic");
    expect(deskIssueProblem(tickets, standalone)).toBeNull();
    expect(deskIssueProblem(tickets, underDone)).toBeNull();
    expect(deskIssueProblem(tickets, epic)).toContain("is an epic");
    expect(deskIssueProblem(tickets, underActive)).toBe("ASMA-101 is under the active epic ASMA-100 (In Progress), whose ESW delivers it. Start from ASMA-100 instead.");
    expect(deskIssueProblem(adam, closedEpic)).toContain("ASMA-104 is Done");
  });

  it("derives the desk from the issue, never from the desk the dialog opened in", () => {
    const desks = deskProjects(WORKSPACES);
    expect(issueDesk(desks, epic)).toMatchObject({ desk: { projectId: "prj-adam" }, problem: null });
    expect(issueDesk(desks, standalone)).toMatchObject({ desk: { projectId: "prj-tickets" }, problem: null });
    expect(issueDesk(desks, underDone)).toMatchObject({ desk: { projectId: "prj-tickets" }, problem: null });
    expect(issueDesk(desks, underActive)).toEqual({ desk: null, problem: expect.stringContaining("Start from ASMA-100 instead.") });
    expect(issueDesk(desks, closedEpic)).toEqual({ desk: null, problem: expect.stringContaining("is Done") });
    expect(issueDesk(desks.filter((desk) => desk.kind !== "tickets"), standalone))
      .toEqual({ desk: null, problem: "ASMA-103 starts on DESK • TICKETS, which has no checkout on this host. Create its lead workspaces through adoption first." });
  });

  it("selects one versioned persona per role and start", () => {
    expect([personaFor("LSA", "ad-hoc"), personaFor("LSA", "from-jira"), personaFor("TPM", "from-jira")]).toEqual(["lsa-ad-hoc", "lsa-bound", "tpm-bound"]);
    expect(PERSONA_VERSION).toMatch(/^kontor-lsa-adhoc@[a-f0-9]{12}$/);
  });
});

describe("create switch", () => {
  it.each([
    ['{"create":"on"}', null],
    [null, null],
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

  it.skipIf(process.getuid?.() === 0)("keeps an unreadable switch disabled", async () => {
    const s = await setup({ switchText: '{"create":"on"}' });
    await chmod(s.switchFile, 0o000);
    expect(createSwitchProblem(s.switchFile)).toContain("not valid");
    expect(await s.service.options()).toMatchObject({ enabled: false });
    expect(await s.service.create(request())).toMatchObject({ state: "refused" });
    expect(s.create).not.toHaveBeenCalled();
  });

  it("keeps a dangling switch symlink disabled", async () => {
    const s = await setup();
    await symlink(join(s.stateRoot, "missing.json"), s.switchFile);
    expect(createSwitchProblem(s.switchFile)).toContain("not valid");
    expect(await s.service.options()).toMatchObject({ enabled: false });
    expect(await s.service.create(request())).toMatchObject({ state: "refused" });
    expect(s.create).not.toHaveBeenCalled();
  });

  it("keeps a directory at the switch path disabled without intent or dispatch", async () => {
    const s = await setup();
    await mkdir(s.switchFile);
    expect(await s.service.options()).toMatchObject({ enabled: false, disabledReason: expect.stringContaining("not valid") });
    expect(await s.service.create(request())).toMatchObject({ state: "refused", message: expect.stringContaining("not valid") });
    expect(s.intents).toEqual([]);
    expect(s.create).not.toHaveBeenCalled();
    await expect(readdir(s.stateRoot)).rejects.toMatchObject({ code: "ENOENT" });
  });

  it.each(["plugin-config", "plugin-config/kontor"])("refuses a dangling %s directory symlink before intent or native reads/dispatch", async (component) => {
    const s = await setup(), root = dirname(s.switchFile), link = join(root, component);
    await mkdir(dirname(link), { recursive: true });
    await symlink(join(root, "missing-directory"), link);
    const service = createSessionCreateService(s.client, s.stateRoot, join(root, "plugin-config", "kontor", "create.json"));
    const workspaceRead = vi.spyOn(s.client.workspaces, "ref"), agentRead = vi.spyOn(s.client.agents, "ref");
    const options = await service.options();
    vi.clearAllMocks();
    const outcome = await service.create(request());
    expect(options).toMatchObject({ enabled: false, disabledReason: expect.stringContaining("not valid") });
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringContaining("not valid") });
    expect(s.intents).toEqual([]);
    for (const call of [s.client.workspaces.list, workspaceRead, s.snapshot, s.client.config.get, s.list, agentRead, s.create]) expect(call).not.toHaveBeenCalled();
    await expect(readdir(s.stateRoot)).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("is available when normal configuration directories are absent", async () => {
    const s = await setup();
    const service = createSessionCreateService(s.client, s.stateRoot, join(dirname(s.switchFile), "plugin-config", "kontor", "create.json"));
    expect(await service.options()).toMatchObject({ enabled: true, disabledReason: null });
    expect(s.create).not.toHaveBeenCalled();
    expect(await service.create(request())).toMatchObject({ state: "verified" });
  });
});

describe("create options", () => {
  it("scopes the catalog to the chosen desk and never falls back from a missing desk", async () => {
    const s = await setup();
    expect(await s.service.options()).toMatchObject({ catalogProjectId: "prj-adam" });
    expect(s.snapshot).toHaveBeenLastCalledWith({ cwd: "/fixtures/wks-adam-anchor" });
    expect(await s.service.options({ projectId: "prj-pr" })).toMatchObject({ catalogProjectId: "prj-pr" });
    expect(s.snapshot).toHaveBeenLastCalledWith({ cwd: "/fixtures/wks-pr-anchor" });
    expect(await s.service.options({ workspaceId: "wks-tickets-lsa" })).toMatchObject({ catalogProjectId: "prj-tickets" });
    s.snapshot.mockClear();
    expect(await s.service.options({ projectId: "missing" })).toMatchObject({ catalogProjectId: null, providers: [] });
    expect(s.snapshot).not.toHaveBeenCalled();
    expect(s.create).not.toHaveBeenCalled();
  });

  it("offers desks, providers with reasons and the role's host profile", async () => {
    const s = await setup();
    const options = await s.service.options();
    expect(options).toMatchObject({ enabled: true, disabledReason: null, pending: null, notes: [], jiraProblem: null, issues: [],
      profile: { name: "lsa", provider: "claude", model: "opus", modeId: "default", effort: "max" } });
    expect(options.desks.map((desk) => desk.project)).toEqual(["DESK • ADAM", "DESK • PR REVIEW", "DESK • TICKETS"]);
    const [claude, opencode, gemini, codex] = options.providers;
    expect(claude).toMatchObject({ available: true, persona: "supported", defaultModel: "opus", defaultMode: "default" });
    expect(opencode).toMatchObject({ available: true, persona: "unverified", defaultModel: "big" });
    expect(gemini).toMatchObject({ available: false, persona: "unverified", note: "Provider error: not signed in." });
    expect(codex).toMatchObject({ available: false, persona: "supported", note: "Disabled on this host." });
    expect((await s.service.options({ role: "TPM" })).profile).toMatchObject({ name: "tpm", provider: "opencode", model: "big" });
  });

  it("lists only open epics on ADAM and only non-epic issues without an active parent on TICKETS", async () => {
    const s = await setup();
    expect((await s.service.options({ projectId: "prj-adam", role: "LSA", start: "from-jira" })).issues.map((issue) => issue.key)).toEqual(["ASMA-100"]);
    const tickets = await s.service.options({ projectId: "prj-tickets", role: "TPM", start: "from-jira" });
    expect(tickets.issues.map((issue) => issue.key)).toEqual(["ASMA-102", "ASMA-103"]);
    expect(s.jira).toHaveBeenCalledWith({ query: "", issueClass: "non-epic", openOnly: true });
    s.jira.mockClear();
    expect((await s.service.options({ projectId: "prj-pr", role: "LSA", start: "from-jira" })).issues).toEqual([]);
    expect(s.jira).not.toHaveBeenCalled();
    s.jira.mockClear();
    await s.service.options({ projectId: "prj-adam", start: "ad-hoc" });
    expect(s.jira).not.toHaveBeenCalled();
  });

  it("passes the selected class before the cap and keeps every eligible type", async () => {
    const s = await setup();
    const options = await s.service.options({ projectId: "prj-tickets", role: "LSA", start: "from-jira", issueClass: "eligible" });
    expect(s.jira).toHaveBeenCalledTimes(1);
    expect(s.jira).toHaveBeenCalledWith({ query: "", issueClass: "eligible", openOnly: true });
    expect(options.issues.map((issue) => issue.key)).toEqual(["ASMA-100", "ASMA-102", "ASMA-103"]);
  });

  it("names an outage instead of silently offering no issue", async () => {
    const s = await setup({ offline: true });
    expect(await s.service.options({ start: "from-jira" })).toMatchObject({ issues: [], jiraProblem: "Jira could not be read: Jira is unreachable" });
  });

  it("is available without the operator's switch, and notes an unreadable profile list", async () => {
    const s = await setup({ switchText: null });
    vi.mocked(s.client.config.get).mockRejectedValueOnce(new Error("config unavailable"));
    expect(await s.service.options()).toMatchObject({ enabled: true, disabledReason: null, profile: null, notes: ["Agent profiles unavailable: config unavailable."] });
  });
});

describe("host provider eligibility", () => {
  it.each([
    [true, "ready", "ready", true],
    [false, "ready", "disabled", false],
    [true, "loading", "loading", false],
    [true, "error", "error", false],
    [true, "unavailable", "unavailable", false],
    [undefined, "ready", "unknown", false],
    [true, undefined, "unknown", false],
  ])("keeps enabled=%s status=%s distinct", (enabled, status, expected, available) => {
    const snapshot = { ...SNAPSHOT, entries: [{ ...SNAPSHOT.entries[0]!, enabled, status }] } as Awaited<ReturnType<CreateClient["providers"]["snapshot"]>>;
    expect(createProviders(snapshot)[0]).toMatchObject({ status: expected, available });
  });

  it("uses the host's selectable model list without inventing credential or account eligibility", () => {
    const [provider] = createProviders(SNAPSHOT as Awaited<ReturnType<CreateClient["providers"]["snapshot"]>>);
    expect(provider!.models.map((model) => model.id)).toEqual(["opus"]);
    expect(provider!.defaultModel).toBe("opus");
  });
});

describe("new session create", () => {
  it("creates an ad-hoc LSA in ADHOC, creating the missing workspace and reading it back before dispatch", async () => {
    const s = await setup(), input = request();
    const outcome = await s.service.create(input);
    const title = "LSA • ADHOC • Route review";
    expect(s.workspacesCreate).toHaveBeenCalledExactlyOnceWith({ source: { kind: "directory", path: "/fixtures/wks-adam-anchor", projectId: "prj-adam" }, title: "ADHOC" });
    expect(s.create).toHaveBeenCalledTimes(1);
    const sent = s.create.mock.calls[0]![0];
    expect(sent).toEqual({ idempotencyKey: input.requestId, title,
      labels: { role: "LSA", "title.auto": title, persona: PERSONA_VERSION, "create.request": input.requestId },
      config: { provider: "claude/opus", modeId: "default", thinkingOptionId: "max", systemPrompt: expect.stringContaining("stays ad hoc") } });
    expect(Object.keys(sent.labels!).some((key) => key.startsWith("kontor.") || key.includes("orchestration"))).toBe(false);
    expect(sent.config.systemPrompt).toContain("desk DESK • ADAM, workspace ADHOC (id assigned at creation)");
    expect(sent.config.systemPrompt).toContain(SCOPE_LABEL_RULE);
    expect(sent).not.toHaveProperty("prompt");
    expect(s.intents).toEqual([expect.objectContaining({ outcome: null, request: expect.objectContaining({ requestId: input.requestId, title, workspaceId: "wks-created" }) })]);
    expect(outcome).toMatchObject({ state: "verified", agentId: "agent-new", title, persona: "supported", differences: [], workspaceId: "wks-created" });
    expect(JSON.parse(await readFile(join(s.stateRoot, `${input.requestId}.json`), "utf8")).outcome).toEqual(outcome);
  });

  it("places a bound LSA from an epic in LSA with the epic label and persona", async () => {
    const s = await setup();
    const outcome = await s.service.create(request({ start: "from-jira", issueKey: "ASMA-100", topic: undefined }));
    const title = "LSA • ASMA-100 • Deliver the desk layout";
    expect(outcome).toMatchObject({ state: "verified", workspaceId: "wks-adam-lsa" });
    const sent = s.create.mock.calls[0]![0];
    expect(sent.title).toBe(title);
    expect(sent.labels).toMatchObject({ role: "LSA", epic: "ASMA-100", "title.auto": title, "create.request": expect.any(String) });
    expect(sent.labels).not.toHaveProperty("ticket");
    expect(sent.config.systemPrompt).toContain("Jira duty:");
    expect(sent.config.systemPrompt).toContain("Wall protocol.");
    expect(sent.config.systemPrompt).toContain("A widen this ticket");
    expect(sent).not.toHaveProperty("prompt");
    expect(s.workspacesCreate).not.toHaveBeenCalled();
  });

  it("keeps the exact key and binding when the From Jira topic is edited", async () => {
    const s = await setup();
    const outcome = await s.service.create(request({ start: "from-jira", issueKey: "ASMA-100", topic: "  Desk   layout follow-up " }));
    const title = "LSA • ASMA-100 • Desk layout follow-up";
    expect(outcome).toMatchObject({ state: "verified", title, workspaceId: "wks-adam-lsa" });
    const sent = s.create.mock.calls[0]![0];
    expect(sent.labels).toMatchObject({ epic: "ASMA-100", "title.auto": title });
    expect(sent.config.systemPrompt).toContain("Scope: epic ASMA-100.");
    expect(sent.config.systemPrompt).toContain("Topic: Desk layout follow-up.");
    for (const topic of ["ASMA-100 • Forged", "ab"]) {
      expect(await s.service.create(request({ start: "from-jira", issueKey: "ASMA-100", topic }))).toMatchObject({ state: "refused" });
    }
    expect(s.create).toHaveBeenCalledTimes(1);
  });

  it("places a bound TPM from an epic in TPM", async () => {
    const s = await setup();
    expect(await s.service.create(request({ role: "TPM", start: "from-jira", issueKey: "ASMA-100", topic: undefined })))
      .toMatchObject({ state: "verified", workspaceId: "wks-adam-tpm", title: "TPM • ASMA-100 • Deliver the desk layout" });
    expect(s.create.mock.calls[0]![0].labels).toMatchObject({ role: "TPM", epic: "ASMA-100" });
  });

  it.each([
    ["a parentless issue", "ASMA-103"],
    ["an issue whose parent epic is Done", "ASMA-102"],
  ])("accepts %s on TICKETS and labels it as a ticket", async (_label, key) => {
    const s = await setup();
    expect(await s.service.create(request({ projectId: "prj-tickets", start: "from-jira", issueKey: key, topic: undefined })))
      .toMatchObject({ state: "verified", workspaceId: "wks-tickets-lsa" });
    expect(s.create.mock.calls[0]![0].labels).toMatchObject({ ticket: key });
    expect(s.create.mock.calls[0]![0].labels).not.toHaveProperty("epic");
  });

  it.each([
    ["an epic on TICKETS", { projectId: "prj-tickets", start: "from-jira", issueKey: "ASMA-100" }, {}, "is an epic"],
    ["a non-epic on ADAM", { start: "from-jira", issueKey: "ASMA-103" }, {}, "not an epic"],
    ["From Jira on PR REVIEW", { projectId: "prj-pr", start: "from-jira", issueKey: "ASMA-100" }, {}, "offers ad hoc sessions only"],
    ["an ad-hoc TPM", { role: "TPM" }, {}, "Ad hoc creates an LSA only"],
    ["an unknown key", { start: "from-jira", issueKey: "ASMA-9999" }, {}, "was not found in Jira"],
    ["a key outside ASMA", { start: "from-jira", issueKey: "OTHER-1" }, {}, "Choose an ASMA issue key"],
    ["a closed issue", { start: "from-jira", issueKey: "ASMA-104" }, {}, "is Done"],
    ["an issue under an active epic", { projectId: "prj-tickets", start: "from-jira", issueKey: "ASMA-101" }, {}, "under the active epic ASMA-100"],
    ["the switch off", {}, { switchText: '{"create":"off"}' }, "switched off"],
    ["the switch invalid", {}, { switchText: "not json" }, "not valid"],
    ["a forged topic", { topic: "A • B" }, {}, "cannot contain"],
    ["the provider unavailable", { provider: "gemini", model: "x", effort: null, modeId: null }, {}, "not signed in"],
    ["the model not selectable", { model: "legacy" }, {}, "not selectable"],
    ["the effort not offered", { effort: "turbo" }, {}, "Effort turbo"],
    ["the mode not offered", { modeId: "plan" }, {}, "Mode plan"],
    ["the title names a live session there", {}, { existing: [agent("agent-old", { title: "LSA • ADHOC • Route review", workspaceId: "wks-adam-adhoc" })] }, "already names a session"],
  ])("refuses %s without dispatch, workspace or intent", async (_label, patch, options, message) => {
    const s = await setup(options);
    if (message === "already names a session") s.live.push(workspace("wks-adam-adhoc", "prj-adam", "DESK • ADAM", "ADHOC"));
    const outcome = await s.service.create(request(patch as Partial<CreateRequest>));
    expect(outcome).toMatchObject({ state: "refused", agentId: null, message: expect.stringContaining(message) });
    expect(outcome.message).toContain("Nothing was created.");
    expect(s.create).not.toHaveBeenCalled();
    expect(s.workspacesCreate).not.toHaveBeenCalled();
    await expect(readdir(s.stateRoot)).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("refuses every Jira read failure without creating anything", async () => {
    const s = await setup({ offline: true });
    expect(await s.service.create(request({ start: "from-jira", issueKey: "ASMA-100" }))).toMatchObject({ state: "refused", message: expect.stringContaining("Jira could not be read") });
    expect(s.create).not.toHaveBeenCalled();
    expect(s.workspacesCreate).not.toHaveBeenCalled();
    await expect(readdir(s.stateRoot)).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("re-reads the issue at create time and refuses a change before intent", async () => {
    const s = await setup();
    const original = s.jira.getMockImplementation()!;
    s.jira.mockImplementationOnce(original).mockImplementationOnce(async () => ({ issues: [{ ...epic, open: false, status: "Done", statusCategory: "Done" }], hasMore: false, nextPageToken: null, failure: null }));
    const outcome = await s.service.create(request({ start: "from-jira", issueKey: "ASMA-100", topic: undefined }));
    expect(outcome).toMatchObject({ state: "refused", message: expect.stringContaining("is Done") });
    expect(s.create).not.toHaveBeenCalled();
    await expect(readdir(s.stateRoot)).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("delivers the persona for an unverified provider as a labelled bootstrap prompt, never as a system prompt", async () => {
    const s = await setup();
    expect(await s.service.create(request({ role: "TPM", start: "from-jira", issueKey: "ASMA-100", provider: "opencode", model: "big", effort: null, modeId: null, topic: undefined })))
      .toMatchObject({ state: "verified", persona: "unverified" });
    const sent = s.create.mock.calls[0]![0];
    expect(sent.config).toEqual({ provider: "opencode/big" });
    expect(sent.prompt).toContain("You are Adam, playing TPM");
    expect(sent.prompt).toContain("Jira duty:");
    expect(sent.prompt).toContain("Wall protocol.");
    expect(sent.prompt).toContain("bootstrap message, not a task");
    expect(sent.labels!.persona).toMatch(/^kontor-tpm-bound\+bootstrap@[a-f0-9]{12}$/);
  });

  it("reads a named source session's summary into the bootstrap before the first task", async () => {
    const s = await setup({ timelineText: "Handoff: ASMA-100 is approved; begin with the core." });
    const input = request({ sourceAgentId: "agent-source" });
    const outcome = await s.service.create(input);
    expect(outcome).toMatchObject({ state: "verified" });
    expect(s.timeline).toHaveBeenCalledWith({ limit: 100, direction: "tail" });
    expect(s.create.mock.calls[0]![0].prompt).toContain("Handoff: ASMA-100 is approved; begin with the core.");
    expect(s.create.mock.calls[0]![0].prompt).toContain("before your first task");
  });

  it.each([null, ""])("refuses a source session with no handoff summary: %s", async (timelineText) => {
    const s = await setup({ timelineText });
    expect(await s.service.create(request({ sourceAgentId: "agent-source" }))).toMatchObject({ state: "refused", message: expect.stringContaining("has no handoff summary") });
    expect(s.create).not.toHaveBeenCalled();
    expect(s.workspacesCreate).not.toHaveBeenCalled();
  });

  it("answers a repeated request from its record without a second dispatch", async () => {
    const s = await setup(), input = request();
    const first = await s.service.create(input);
    expect(await s.service.create(input)).toEqual(first);
    expect(s.create).toHaveBeenCalledTimes(1);
    expect(s.workspacesCreate).toHaveBeenCalledTimes(1);
  });

  it("reports readback differences instead of claiming a match", async () => {
    const s = await setup();
    s.setReadback({ model: "sonnet", workspaceId: "wks-elsewhere" });
    const outcome = await s.service.create(request());
    expect(outcome).toMatchObject({ state: "differences", agentId: "agent-new" });
    expect(outcome.differences).toEqual(["Placed outside the chosen workspace.", "Model reads sonnet."]);
  });

  it("reports a working directory mismatch even when the workspace ID matches", async () => {
    const s = await setup();
    s.setReadback({ cwd: "/fixtures/elsewhere" });
    expect(await s.service.create(request())).toMatchObject({ state: "differences", differences: ["Working directory differs from the chosen workspace."] });
  });

  it("reads the created workspace back and dispatches nothing when its name differs", async () => {
    const s = await setup({ workspaceProblem: "NOT-ADHOC" });
    expect(await s.service.create(request())).toMatchObject({ state: "unknown", agentId: null });
    expect(s.create).not.toHaveBeenCalled();
  });

  it("refuses when the workspace cannot be created and no matching workspace exists", async () => {
    const s = await setup({ failWorkspaceCreate: true });
    expect(await s.service.create(request())).toMatchObject({ state: "refused", message: expect.stringContaining("Nothing was created") });
    expect(s.create).not.toHaveBeenCalled();
  });

  it("allows the same title once the earlier session is archived", async () => {
    const s = await setup({ existing: [agent("agent-old", { title: "LSA • ADHOC • Route review", archivedAt: "2026-10-21" })] });
    s.live.push(workspace("wks-adam-adhoc", "prj-adam", "DESK • ADAM", "ADHOC"));
    expect((await s.service.create(request())).state).toBe("verified");
  });

  it("keeps a failed dispatch unknown when Check finds no correlated session", async () => {
    const s = await setup(), first = request();
    s.create.mockRejectedValueOnce(new Error("transport closed"));
    expect(await s.service.create(first)).toMatchObject({ state: "unknown", agentId: null, message: expect.stringContaining("Do not retry") });
    expect((await s.service.options()).pending).toMatchObject({ requestId: first.requestId, state: "unknown" });
    expect(await s.service.create(request({ topic: "Another topic" }))).toMatchObject({ state: "refused", message: expect.stringContaining("Check it first") });
    expect(await s.service.check(first.requestId)).toMatchObject({ state: "unknown", agentId: null });
    expect(s.list).toHaveBeenLastCalledWith({ filter: { labels: { "create.request": first.requestId }, includeArchived: true }, page: { limit: 10 } });
    expect(s.create).toHaveBeenCalledTimes(1);
  });

  it("reports unknown when the host does not answer in time, and Check finds the late session by its request label", async () => {
    const s = await setup({ answerMs: 20 }), input = request();
    let finish: (() => void) | undefined;
    s.create.mockImplementationOnce(async (sent) => {
      await new Promise<void>((resolve) => { finish = resolve; });
      const value = agent("agent-late", { title: sent.title, labels: sent.labels, effectiveThinkingOptionId: "max", provider: "claude", model: "opus", workspaceId: "wks-created", cwd: "/fixtures/wks-adam-anchor" });
      s.agents.set(value.id, value);
      return { id: value.id } as Awaited<ReturnType<NativeCreate>>;
    });
    expect(await s.service.create(input)).toMatchObject({ state: "unknown", message: expect.stringContaining("no answer within 0 s") });
    expect(await s.service.check(input.requestId)).toMatchObject({ state: "unknown" });
    finish!();
    await vi.waitFor(() => expect(s.agents.has("agent-late")).toBe(true));
    expect(await s.service.check(input.requestId)).toMatchObject({ state: "verified", agentId: "agent-late" });
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

  it("blocks every new create while a creation record cannot be read", async () => {
    const s = await setup();
    await s.service.create(request());
    await writeFile(join(s.stateRoot, `${randomUUID()}.json`), "{partial");
    expect(await s.service.create(request({ topic: "Another topic" }))).toMatchObject({ state: "refused", message: expect.stringContaining("cannot be read") });
    expect(await s.service.options()).toMatchObject({ enabled: false, disabledReason: expect.stringContaining("cannot be read") });
    expect(s.create).toHaveBeenCalledTimes(1);
  });

  it("reads the last assistant message of a source session as its summary", async () => {
    const s = await setup({ timelineText: "  Ship the desk layout.  " });
    expect(await sourceSummary(s.client, "agent-source")).toBe("Ship the desk layout.");
  });
});
