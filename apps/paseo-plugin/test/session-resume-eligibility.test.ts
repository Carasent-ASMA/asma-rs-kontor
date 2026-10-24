import { mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoAgentListResult } from "@getpaseo/client";
import { createLifecycleService, lifecycleRow, type LifecycleClient } from "../server/session-lifecycle";
import { DEFAULT_RECOVERY_TARGET as target, RESUME_PROMPT } from "../shared/session-lifecycle";

type Agent = PaseoAgentListResult["entries"][number]["agent"];
type Entry = Awaited<ReturnType<LifecycleClient["providers"]["snapshot"]>>["entries"][number];
// Synthetic routes; the review target stays the default codex / gpt-6.1-sol / xhigh, which none of them use.
const routes = {
  "non-default": { provider: "claude", model: "claude-opus-5-5", effectiveThinkingOptionId: "high" },
  "account-suffixed": { provider: "codex-fixture-account", model: "gpt-6.1-sol", effectiveThinkingOptionId: "high" },
  "JSON": { provider: "fixture-harness", model: '["fixture-account","deepseek-flash"]', effectiveThinkingOptionId: null },
  "runtime-suffixed": { provider: "claude", model: "claude-opus-5-5", effectiveThinkingOptionId: "high", runtimeInfo: { provider: "claude", sessionId: null, model: "claude-opus-5-5[1m]" } },
} satisfies Record<string, Partial<Agent>>;
function agent(id: string, patch: Partial<Agent> = {}): Agent {
  return { id, provider: "codex", model: "gpt-6.1-sol", effectiveThinkingOptionId: "xhigh", cwd: "/fixtures/checkout", workspaceId: "workspace-a",
    title: `Fixture ${id}`, status: "error", lastError: "Turn interrupted by transport failure", createdAt: "2001-01-01", updatedAt: "2001-01-01", lastUserMessageAt: null,
    capabilities: { supportsStreaming: true, supportsSessionPersistence: true, supportsDynamicModes: false, supportsMcpServers: true, supportsReasoningStream: true, supportsToolInvocations: true },
    currentModeId: "full-access", availableModes: [], pendingPermissions: [], persistence: { provider: "codex", sessionId: `history-${id}` }, labels: {},
    requiresAttention: true, attentionReason: "error", attentionTimestamp: "2001-01-01", archivedAt: null, ...patch };
}
const ready = (provider: string): Entry => ({ provider, enabled: true, status: "ready" });
const dirs: string[] = [];
afterEach(async () => { for (const path of dirs.splice(0)) await rm(path, { recursive: true, force: true }); });
async function host(initial: Agent[], entries: Entry[]) {
  const root = await mkdtemp(join(tmpdir(), "kontor-resume-test-")); dirs.push(root);
  const records = new Map(initial.map((value) => [value.id, value]));
  const send = vi.fn(async (_id: string, _text: string, _options?: unknown) => {}), archive = vi.fn(async (id: string) => {
    records.set(id, { ...records.get(id)!, status: "closed", archivedAt: "2026-10-24" }); return { archivedAt: "2026-10-24" };
  });
  const snapshot = vi.fn<LifecycleClient["providers"]["snapshot"]>(async () => ({ requestId: "fixture", generatedAt: "now", entries }));
  const client: LifecycleClient = { agents: {
    list: async () => ({ requestId: "fixture", entries: [...records.values()].map((value) => ({ agent: value, project: { projectKey: "fixture", projectName: "Fixture", checkout: { worktreeRoot: null, cwd: "/fixtures/checkout", isGit: false as const, currentBranch: null, remoteUrl: null, isPaseoOwnedWorktree: false as const, mainRepoRoot: null } } })), pageInfo: { hasMore: false, nextCursor: null, prevCursor: null } }),
    ref: (id) => ({ refresh: async () => { const value = records.get(id); return value ? { agent: value, project: null } : null; },
      send: (text, options) => send(id, text, options), archive: () => archive(id),
      timeline: { refetch: async () => ({ entries: [{ type: "user_message", text: "Fictional task context" }], error: null }) } }),
  }, providers: { snapshot } };
  const service = createLifecycleService(client, root);
  return { service, records, send, archive, snapshot, preview: (action: "resume" | "archive") => service.preview({ action, scope: "filtered", agentIds: ["a"], target }) };
}

describe("Resume retries the same session on its own route", () => {
  it.each(Object.entries(routes))("%s route: eligible under the default target, sent once to the same ID, nothing created", async (_name, route) => {
    // Only the session's own provider is ready; the target's provider is absent from this host.
    const h = await host([agent("a", route)], [ready(route.provider)]), review = await h.preview("resume");
    expect(review.rows[0]).toMatchObject({ id: "a", eligible: true, provider: route.provider });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "submitted" });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "submitted" });
    expect(h.send).toHaveBeenCalledTimes(1);
    expect(h.send).toHaveBeenCalledWith("a", RESUME_PROMPT, expect.objectContaining({ messageId: expect.stringMatching(new RegExp(`^kontor-resume-${review.id}-`)) }));
    expect(h.archive).not.toHaveBeenCalled();
    expect([...h.records.keys()]).toEqual(["a"]);
    expect(h.records.get("a")).toMatchObject(route);
  });

  it.each([
    ["unavailable", { provider: "claude", enabled: true, status: "unavailable" }],
    ["disabled", { provider: "claude", enabled: false, status: "ready" }],
  ] as const)("checks the session's own provider, not the target's: %s blocks with nothing sent", async (_name, own) => {
    const h = await host([agent("a", routes["non-default"])], [{ ...ready(target.provider), models: [{ provider: target.provider, id: target.model, label: "Sol", thinkingOptions: [{ id: target.effort, label: "Extra high" }] }] }, own]);
    const review = await h.preview("resume");
    expect(review.rows[0]?.eligible).toBe(true);
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "blocked", message: expect.stringContaining("provider claude is not ready") });
    expect(h.send).not.toHaveBeenCalled();
  });

  it("keeps every other Resume exclusion on a non-default route", () => {
    const cases: Partial<Agent>[] = [{ lastError: "5-hour usage limit reached" }, { providerUnavailable: true }, { lastError: "401 Unauthorized" },
      { status: "idle" }, { status: "running" }, { status: "closed" }, { archivedAt: "2026-10-20" }, { labels: { "takeover.successor": "next" } },
      { attentionReason: "permission" }, { currentModeId: null }, { persistence: null }, { cwd: "" }];
    for (const patch of cases) expect(lifecycleRow(agent("a", { ...routes["account-suffixed"], ...patch }), [], "resume").eligible, JSON.stringify(patch)).toBe(false);
  });
});

describe("Archive is unaffected by the Resume route change", () => {
  it("archives a session on a non-default route without consulting providers or sending", async () => {
    const h = await host([agent("a", { ...routes["account-suffixed"], status: "idle" })], []), review = await h.preview("archive");
    expect(review.rows[0]).toMatchObject({ eligible: true });
    expect(await h.service.execute(review.id, "a")).toMatchObject({ outcome: "archived", runtime: "closed" });
    expect(h.archive).toHaveBeenCalledTimes(1);
    expect(h.send).not.toHaveBeenCalled();
    expect(h.snapshot).not.toHaveBeenCalled();
  });
});
