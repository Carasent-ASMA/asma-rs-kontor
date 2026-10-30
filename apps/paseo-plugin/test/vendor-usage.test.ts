import { describe, expect, it, vi } from "vitest";
import { BUILTIN_PROVIDER_IDS } from "@getpaseo/protocol/provider-manifest";
import { createVendorUsageService, codexThreadIdentity, type VendorBase, type VendorUsageRoute } from "../server/vendor-usage";
import { readInstalledNavigatorCapabilities } from "../server/session-navigator";

const THREAD = "00000000-0000-4000-8000-000000000001", NEXT = "00000000-0000-4000-8000-000000000002";
const CLAUDE = "00000000-0000-4000-8000-000000000003", OPENCODE = "ses_0123456789abcdef";
const agent = (id: string, nativeHandle = THREAD, provider = "codex") => ({ id, provider, title: "Same title", persistence: { provider, sessionId: "tree-root-is-not-thread-id", nativeHandle, metadata: { threadId: nativeHandle } } });
const nativeAgent = (id: string, base: VendorBase) => {
  const nativeHandle = base === "codex" ? THREAD : base === "claude" ? CLAUDE : OPENCODE;
  return { id, provider: base, title: "Same title", persistence: { provider: base, sessionId: nativeHandle, nativeHandle, metadata: {} } };
};
const aliasAgent = (id: string, provider: string, base: VendorBase) => ({ ...nativeAgent(id, base), provider });
const found = (a: { id: string }) => ({ agent: a });
const host = (refresh: ReturnType<typeof vi.fn>) => ({ agents: { ref: vi.fn(() => ({ refresh })) } }) as never;
const aliasMetadata = (aliases: string[], env: Record<string, string> = {}) => ({
 providers: { snapshot: vi.fn(async (): Promise<{ entries: { provider: string; source: "custom" | "builtin" }[] }> => ({ entries: aliases.map(provider => ({ provider, source: "custom" })) })) },
 config: { get: vi.fn(async (): Promise<any> => ({ config: { providers: Object.fromEntries(aliases.map(provider => [provider, { extends: "codex", env, secret: "PRIVATE_PROVIDER_CONFIG" }])) } })) },
});
const aliasHost = (refresh: ReturnType<typeof vi.fn>, aliases: string[], env: Record<string, string> = {}) => Object.assign(host(refresh), aliasMetadata(aliases, env));
const value = { input: 100, output: 5, scope: "provider-cumulative" as const, source: "Fixture provider cumulative; history unavailable" };
const readStub = () => vi.fn(async (_route: VendorUsageRoute, _native: string, _budget: object) => value);

describe("vendor-native usage ownership", () => {
  it("uses the exact native thread and public read-only refresh, never the tree root/title/model", async () => {
    const refresh = vi.fn(async () => found(agent("a"))), read = readStub(), service = createVendorUsageService({ read });
    expect(await service(host(refresh), ["a", "a"])).toEqual({ a: value });
    expect(read).toHaveBeenCalledExactlyOnceWith({ base: "codex", source: expect.any(String) }, THREAD, expect.any(Object));
    expect(refresh).toHaveBeenCalledTimes(2);
  });
  it("rejects conflicting identities, wrong native agent and unqualified Claude without inventing numbers", async () => {
    const read = readStub(), service = createVendorUsageService({ read });
    for (const a of [agent("wrong"), { ...nativeAgent("a", "claude"), persistence: { ...nativeAgent("a", "claude").persistence, provider: "codex" } }, { ...agent("a"), persistence: { ...agent("a").persistence, metadata: { threadId: NEXT } } }]) {
      expect(await service(host(vi.fn(async () => found(a))), ["a"])).toMatchObject({ a: { input: null, output: null } });
    }
    expect(read).not.toHaveBeenCalled();
    expect(codexThreadIdentity({ persistence: { provider: "codex", sessionId: THREAD } } as never)).toBeNull();
  });
  it("rejects a provider/thread switch between exact read and reply", async () => {
    const refresh = vi.fn().mockResolvedValueOnce(found(agent("a"))).mockResolvedValueOnce(found(agent("a", NEXT)));
    const result = await createVendorUsageService({ read: async () => value })(host(refresh), ["a"]);
    expect(result.a).toMatchObject({ input: null, output: null, source: expect.stringContaining("changed") });
  });
  it("rejects effective route changes even when stale Codex persistence survives", async () => {
    const before = agent("a"), after = { ...before, provider: "claude" };
    const refresh = vi.fn().mockResolvedValueOnce(found(before)).mockResolvedValueOnce(found(after));
    expect(await createVendorUsageService({ read: async () => value })(host(refresh), ["a"])).toMatchObject({ a: { input: null, output: null } });
    const read = readStub();
    expect(await createVendorUsageService({ read })(host(vi.fn(async () => found(after))), ["a"])).toMatchObject({ a: { input: null, output: null } });
    expect(read).not.toHaveBeenCalled();
  });
  it("preserves stable Codex aliases but refuses an alias/account route switch", async () => {
    const before = { ...agent("a"), provider: "fixture-codex-route-a" }, after = { ...before, provider: "fixture-codex-route-b" };
    expect(await createVendorUsageService({ read: async () => value })(aliasHost(vi.fn(async () => found(before)), [before.provider, after.provider]), ["a"])).toEqual({ a: value });
    const refresh = vi.fn().mockResolvedValueOnce(found(before)).mockResolvedValueOnce(found(after));
    expect(await createVendorUsageService({ read: async () => value })(aliasHost(refresh, [before.provider, after.provider]), ["a"])).toMatchObject({ a: { input: null, output: null } });
  });
  it("keeps failures unknown and shares a finite budget across the batch", async () => {
    const budgets: object[] = [], read = vi.fn(async (_route: VendorUsageRoute, _native: string, budget: object) => { budgets.push(budget); return value; });
    const paseo = { agents: { ref: (id: string) => ({ refresh: async () => found(agent(id)) }) } } as never;
    await createVendorUsageService({ read })(paseo, ["a", "b"]);
    expect(budgets[0]).toBe(budgets[1]); expect(budgets[0]).toMatchObject({ bytes: 32 * 1024 * 1024 });
    const result = await createVendorUsageService({ read })(host(vi.fn(async () => { throw Error("PRIVATE_ERROR"); })), ["a"]);
    expect(result.a).toMatchObject({ input: null, output: null }); expect(JSON.stringify(result)).not.toContain("PRIVATE_ERROR");
  });
  it("advertises only the labelled qualified source while preserving unrelated capability refusals", () => {
    const capabilities = readInstalledNavigatorCapabilities({} as never);
    expect(capabilities.usageTotals).toMatchObject({ supported: true, limits: expect.arrayContaining([expect.stringContaining("0.160.0"), expect.stringContaining("OpenCode"), expect.stringContaining("child/subagent sessions (parent_id) are excluded"), expect.stringContaining("Claude Code")]) });
    expect(JSON.stringify(capabilities.usageTotals)).not.toContain("OpenCode 1.19.3");
    expect(capabilities.takeoverBulk.supported).toBe(false);
  });
});

describe("native provider routes and alias homes", () => {
  it.each(["opencode", "claude"] as const)("reads a native %s agent from its exact session identity", async (base) => {
    const native = nativeAgent("a", base), read = readStub();
    expect(await createVendorUsageService({ read })(host(vi.fn(async () => found(native))), ["a"])).toEqual({ a: value });
    expect(read).toHaveBeenCalledExactlyOnceWith({ base, source: expect.any(String) }, base === "claude" ? CLAUDE : OPENCODE, expect.any(Object));
  });
  it("uses a verified Codex alias's own CODEX_HOME and never exposes other config content", async () => {
    const native = aliasAgent("a", "codex-work", "codex"), read = readStub();
    const result = await createVendorUsageService({ read })(aliasHost(vi.fn(async () => found(native)), [native.provider], { CODEX_HOME: "/tmp/fixture-codex-work" }), ["a"]);
    expect(result).toEqual({ a: value });
    expect(read).toHaveBeenCalledWith({ base: "codex", source: "/tmp/fixture-codex-work" }, THREAD, expect.any(Object));
    expect(JSON.stringify(result)).not.toContain("PRIVATE_PROVIDER_CONFIG");
  });
  it("uses a verified Claude alias's own CLAUDE_CONFIG_DIR projects root", async () => {
    const native = aliasAgent("a", "claude-work", "claude"), read = readStub();
    const metadata = aliasMetadata([native.provider], { CLAUDE_CONFIG_DIR: "/tmp/fixture-claude-work" });
    metadata.config.get.mockResolvedValue({ config: { providers: { [native.provider]: { extends: "claude", env: { CLAUDE_CONFIG_DIR: "/tmp/fixture-claude-work" }, secret: "PRIVATE_PROVIDER_CONFIG" } } } });
    expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toEqual({ a: value });
    expect(read).toHaveBeenCalledWith({ base: "claude", source: "/tmp/fixture-claude-work/projects" }, CLAUDE, expect.any(Object));
  });
  it("uses a verified OpenCode alias's own XDG_DATA_HOME database path", async () => {
    const native = aliasAgent("a", "opencode-deepseek", "opencode"), read = readStub();
    const metadata = aliasMetadata([native.provider], { XDG_DATA_HOME: "/tmp/fixture-opencode-data" });
    metadata.config.get.mockResolvedValue({ config: { providers: { [native.provider]: { extends: "opencode", env: { XDG_DATA_HOME: "/tmp/fixture-opencode-data" } } } } });
    expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toEqual({ a: value });
    expect(read).toHaveBeenCalledWith({ base: "opencode", source: "/tmp/fixture-opencode-data/opencode/opencode.db" }, OPENCODE, expect.any(Object));
  });
  it("rejects an alias home change after the read instead of attaching the old source", async () => {
    const native = aliasAgent("a", "codex-work", "codex"), read = readStub();
    const metadata = aliasMetadata([native.provider], { CODEX_HOME: "/tmp/fixture-codex-work-a" });
    metadata.config.get.mockResolvedValueOnce({ config: { providers: { [native.provider]: { extends: "codex", env: { CODEX_HOME: "/tmp/fixture-codex-work-a" } } } } })
      .mockResolvedValueOnce({ config: { providers: { [native.provider]: { extends: "codex", env: { CODEX_HOME: "/tmp/fixture-codex-work-b" } } } } });
    expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toMatchObject({ a: { input: null, source: expect.stringContaining("changed") } });
    expect(read).toHaveBeenCalledTimes(1);
  });
  it("rejects an alias whose configured home is not an absolute path", async () => {
    const native = aliasAgent("a", "codex-work", "codex"), read = readStub();
    const metadata = aliasMetadata([native.provider], { CODEX_HOME: "relative/home" });
    metadata.config.get.mockResolvedValue({ config: { providers: { [native.provider]: { extends: "codex", env: { CODEX_HOME: "relative/home" } } } } });
    expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toMatchObject({ a: { input: null, output: null } });
    expect(read).not.toHaveBeenCalled();
  });
});

it("completes eight legal 5MiB sources across bounded service batches", async () => {
  const fs = await import("node:fs/promises"), { join } = await import("node:path"), { tmpdir } = await import("node:os");
  const { createCodexUsageReader } = await import("../server/codex-usage");
  const root = await fs.mkdtemp(join(tmpdir(), "kontor-usage-composition-"));
  try {
    const dir = join(root, "sessions"); await fs.mkdir(dir);
    const ids = Array.from({ length: 8 }, (_, i) => `00000000-0000-4000-8000-${String(i + 1).padStart(12, "0")}`);
    for (const id of ids) {
      await fs.writeFile(join(dir, `rollout-${id}.jsonl`), [
        JSON.stringify({ type: "session_meta", payload: { id, session_id: id, cli_version: "0.160.0", originator: "codex_cli_rs", source: "cli" } }),
        JSON.stringify({ type: "response_item", payload: "x".repeat(5 * 1024 * 1024) }),
        JSON.stringify({ type: "event_msg", timestamp: "2026-01-01T00:00:00.000Z", payload: { type: "token_count", info: { total_token_usage: { input_tokens: 100, output_tokens: 8, total_tokens: 108 } } } }),
      ].join("\n") + "\n");
    }
    const paseo = { agents: { ref: (id: string) => ({ refresh: async () => found(agent(id, id)) }) } } as never;
    let result: Awaited<ReturnType<ReturnType<typeof createVendorUsageService>>>;
    for (let batch = 0; batch < 3; batch++) {
      const source = createCodexUsageReader(root, join(root, "cache"));
      // A fresh reader also proves cache-only continuation without retained transcript buffers.
      result = await createVendorUsageService({ read: (route, id, budget) => source.read(id, budget) })(paseo, ids);
      if (batch === 0) expect(Object.values(result).some((row) => row?.input === 100)).toBe(true);
    }
    for (const id of ids) expect(result![id]).toMatchObject({ input: 100, output: 8 });
  } finally { await fs.rm(root, { recursive: true, force: true }); }
});

it("preserves separate recorded child usage through the exact-identity service", async () => {
  const recorded = { ...value, input: null, output: null, recordedChildUsage: {
    scope: "recorded-child-responses" as const, label: "Recorded child response usage" as const,
    qualification: "Excludes parent history; completeness and billing unverified." as const, input: 100, output: 8, total: 108, responses: 1,
  } };
  expect(await createVendorUsageService({ read: async () => recorded })(host(vi.fn(async () => found(agent("a")))), ["a"])).toEqual({ a: recorded });
});

describe("session-usage provider coverage boundary", () => {
  const unsupported = [...new Set([
    ...BUILTIN_PROVIDER_IDS.filter((id) => !["codex", "claude", "opencode"].includes(id)),
    "antigravity", "muse", "cursor", "deepseek-harness", "kimi", "qoder", "glm-acp-agent",
    "fixture-unsupported-claude-alias", "fixture-unsupported-opencode-alias",
  ])];
  it.each(unsupported)("keeps %s unknown even with latest usage and account quota reports", async (provider) => {
    const read = readStub();
    const native = { ...agent("a", THREAD, provider), lastUsage: { inputTokens: 999, outputTokens: 99 } };
    const paseo = { agents: { ref: () => ({ refresh: async () => found(native) }) },
      usage: { listReports: vi.fn(async () => [{ sourceId: provider, report: { status: "available", windows: [{ used: 42 }] } }]) } };
    const result = await createVendorUsageService({ read })(paseo as never, ["a"]);
    expect(result.a).toMatchObject({ input: null, output: null, scope: "provider-cumulative", source: expect.stringContaining("No qualified session-usage source") });
    expect(result.a?.recordedChildUsage).toBeUndefined();
    expect(read).not.toHaveBeenCalled(); expect(paseo.usage.listReports).not.toHaveBeenCalled();
  });
  it("distinguishes a missing Codex identity from an unsupported provider", async () => {
    const native = { ...agent("a"), persistence: null }, read = readStub();
    expect(await createVendorUsageService({ read })(host(vi.fn(async () => found(native as never))), ["a"])).toMatchObject({ a: { input: null, source: expect.stringContaining("Codex exact native thread identity") } });
    expect(read).not.toHaveBeenCalled();
  });
  it("qualifies aliases by canonical Codex persistence and exact source, never model vendor", async () => {
    const native = { ...agent("a"), provider: "fixture-codex-alias", model: "other-model" }, read = readStub();
    expect(await createVendorUsageService({ read })(aliasHost(vi.fn(async () => found(native)), [native.provider]), ["a"])).toEqual({ a: value });
    expect(read).toHaveBeenCalledExactlyOnceWith({ base: "codex", source: expect.any(String) }, THREAD, expect.any(Object));
  });
});

describe("positive route provenance for retained Codex handles", () => {
  it.each(["cursor", "glm-acp-agent"])("withholds native %s with a retained Codex handle", async (provider) => {
    const native = { ...agent("a"), provider }, read = readStub();
    const metadata = aliasMetadata([provider]);
    metadata.providers.snapshot.mockResolvedValue({ entries: [{ provider, source: "builtin" }] });
    expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toMatchObject({ a: { input: null, output: null } });
    expect(read).not.toHaveBeenCalled();
    const unverified = aliasMetadata([provider]);
    unverified.config.get.mockResolvedValue({ config: { providers: {} } });
    expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), unverified), ["a"])).toMatchObject({ a: { input: null, output: null } });
    expect(await createVendorUsageService({ read })(host(vi.fn(async () => found(native))), ["a"])).toMatchObject({ a: { input: null, output: null } });
    expect(read).not.toHaveBeenCalled();
  });
  it("withholds arbitrary routes whose alias mapping is absent or maps to another provider", async () => {
    const native = { ...agent("a"), provider: "fixture-unverified-route" }, read = readStub();
    for (const mapping of [{}, { extends: "opencode" }]) {
      const metadata = aliasMetadata([native.provider]);
      metadata.config.get.mockResolvedValue({ config: { providers: { [native.provider]: mapping as never } } });
      expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toMatchObject({ a: { input: null, output: null } });
    }
    expect(read).not.toHaveBeenCalled();
  });
  it.each(["cursor", "glm-acp-agent"])("discards usage after a Codex to native %s route transition", async (provider) => {
    const after = { ...agent("a"), provider }, read = readStub();
    const refresh = vi.fn().mockResolvedValueOnce(found(agent("a"))).mockResolvedValueOnce(found(after));
    expect(await createVendorUsageService({ read })(host(refresh), ["a"])).toMatchObject({ a: { input: null, output: null } });
    expect(read).toHaveBeenCalledTimes(1);
  });
  it("revalidates a genuine alias mapping after the exact usage read", async () => {
    const native = { ...agent("a"), provider: "fixture-codex-alias" }, metadata = aliasMetadata(["fixture-codex-alias"]);
    metadata.config.get.mockResolvedValueOnce({ config: { providers: { [native.provider]: { extends: "codex" } } } })
      .mockResolvedValueOnce({ config: { providers: { [native.provider]: { extends: "opencode" } } } });
    const read = readStub();
    expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toMatchObject({ a: { input: null, source: expect.stringContaining("changed") } });
    expect(read).toHaveBeenCalledTimes(1); expect(metadata.config.get).toHaveBeenCalledTimes(2);
  });
  it("withholds alias usage when supported metadata reads fail", async () => {
    const native = { ...agent("a"), provider: "fixture-codex-alias" }, metadata = aliasMetadata(["fixture-codex-alias"]);
    metadata.providers.snapshot.mockRejectedValue(Error("PRIVATE_PROVIDER_CONFIG"));
    const read = readStub();
    const result = await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"]);
    expect(result.a).toMatchObject({ input: null, output: null }); expect(read).not.toHaveBeenCalled();
    expect(JSON.stringify(result)).not.toContain("PRIVATE_PROVIDER_CONFIG");
  });
});

it("discards a formerly verified alias if the host now classifies it as native", async () => {
  const native = { ...agent("a"), provider: "fixture-codex-alias" }, metadata = aliasMetadata(["fixture-codex-alias"]);
  metadata.providers.snapshot.mockResolvedValueOnce({ entries: [{ provider: native.provider, source: "custom" }] })
    .mockResolvedValueOnce({ entries: [{ provider: native.provider, source: "builtin" }] });
  const read = readStub();
  expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toMatchObject({ a: { input: null, output: null } });
  expect(read).toHaveBeenCalledTimes(1);
});
