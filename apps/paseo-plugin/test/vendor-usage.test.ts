import { describe, expect, it, vi } from "vitest";
import { BUILTIN_PROVIDER_IDS } from "@getpaseo/protocol/provider-manifest";
import { createVendorUsageService, codexThreadIdentity } from "../server/vendor-usage";
import { readInstalledNavigatorCapabilities } from "../server/session-navigator";
const THREAD = "00000000-0000-4000-8000-000000000001", NEXT = "00000000-0000-4000-8000-000000000002";
const agent = (id: string, nativeHandle = THREAD, provider = "codex") => ({ id, provider, title: "Same title", persistence: { provider, sessionId: "tree-root-is-not-thread-id", nativeHandle, metadata: { threadId: nativeHandle } } });
const found = (a: ReturnType<typeof agent>) => ({ agent: a });
const host = (refresh: ReturnType<typeof vi.fn>) => ({ agents: { ref: vi.fn(() => ({ refresh })) } }) as never;
const aliasMetadata = (aliases: string[]) => ({
 providers: { snapshot: vi.fn(async () => ({ entries: aliases.map(provider => ({ provider, source: "custom" as "custom" | "builtin" })) })) },
 config: { get: vi.fn(async () => ({ config: { providers: Object.fromEntries(aliases.map(provider => [provider, { extends: "codex" }])) } })) },
});
const aliasHost = (refresh: ReturnType<typeof vi.fn>, aliases: string[]) => Object.assign(host(refresh), aliasMetadata(aliases));
const value = { input: 100, output: 5, scope: "provider-cumulative" as const, source: "Fixture provider cumulative; history unavailable" };

describe("vendor-native usage ownership", () => {
  it("uses the exact native thread and public read-only refresh, never the tree root/title/model", async () => {
    const refresh = vi.fn(async () => found(agent("a"))), read = vi.fn(async () => value), service = createVendorUsageService({ read });
    expect(await service(host(refresh), ["a", "a"])).toEqual({ a: value });
    expect(read).toHaveBeenCalledExactlyOnceWith(THREAD, expect.any(Object)); expect(refresh).toHaveBeenCalledTimes(2);
  });
  it("rejects conflicting identities, wrong native agent and unqualified Claude without inventing numbers", async () => {
    const read = vi.fn(async () => value), service = createVendorUsageService({ read });
    for (const a of [agent("wrong"), agent("a", THREAD, "claude"), { ...agent("a"), persistence: { ...agent("a").persistence, metadata: { threadId: NEXT } } }]) {
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
    const read = vi.fn(async () => value);
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
    const budgets: object[] = [], read = vi.fn(async (_id, budget) => { budgets.push(budget); return value; });
    const paseo = { agents: { ref: (id: string) => ({ refresh: async () => found(agent(id)) }) } } as never;
    await createVendorUsageService({ read })(paseo, ["a", "b"]);
    expect(budgets[0]).toBe(budgets[1]); expect(budgets[0]).toMatchObject({ bytes: 32 * 1024 * 1024 });
    const result = await createVendorUsageService({ read })(host(vi.fn(async () => { throw Error("PRIVATE_ERROR"); })), ["a"]);
    expect(result.a).toMatchObject({ input: null, output: null }); expect(JSON.stringify(result)).not.toContain("PRIVATE_ERROR");
  });
  it("advertises only the labelled qualified source while preserving unrelated capability refusals", () => {
    const capabilities = readInstalledNavigatorCapabilities({} as never);
    expect(capabilities.usageTotals).toMatchObject({ supported: true, limits: expect.arrayContaining([expect.stringContaining("0.160.0"), expect.stringContaining("Historical completeness")]) });
    expect(capabilities.takeoverBulk.supported).toBe(false);
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
      // A fresh reader also proves cache-only continuation without retained transcript buffers.
      result = await createVendorUsageService(createCodexUsageReader(root, join(root, "cache")))(paseo, ids);
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
  ...BUILTIN_PROVIDER_IDS.filter((id) => id !== "codex"),
  "antigravity", "muse", "cursor", "deepseek-harness", "kimi", "qoder", "glm-acp-agent",
  "fixture-claude-alias", "fixture-opencode-alias",
 ])];
 it.each(unsupported)("keeps %s unknown even with latest usage and account quota reports", async (provider) => {
  const read = vi.fn(async () => value);
  const native = { ...agent("a", THREAD, provider), model: "gpt-example", lastUsage: { inputTokens: 999, outputTokens: 99 } };
  const paseo = { agents: { ref: () => ({ refresh: async () => found(native) }) },
   usage: { listReports: vi.fn(async () => [{ sourceId: provider, report: { status: "available", windows: [{ used: 42 }] } }]) } };
  const result = await createVendorUsageService({ read })(paseo as never, ["a"]);
  expect(result.a).toMatchObject({ input: null, output: null, scope: "provider-cumulative", source: expect.stringContaining("No qualified session-usage source") });
  expect(result.a?.recordedChildUsage).toBeUndefined();
  expect(read).not.toHaveBeenCalled(); expect(paseo.usage.listReports).not.toHaveBeenCalled();
 });
 it("distinguishes a missing Codex identity from an unsupported provider", async () => {
  const native = { ...agent("a"), persistence: null }, read = vi.fn(async () => value);
  expect(await createVendorUsageService({ read })(host(vi.fn(async () => found(native as never))), ["a"])).toMatchObject({ a: { input: null, source: expect.stringContaining("Codex exact native thread identity") } });
  expect(read).not.toHaveBeenCalled();
 });
 it("qualifies aliases by canonical Codex persistence and exact source, never model vendor", async () => {
  const native = { ...agent("a"), provider: "fixture-codex-alias", model: "other-model" }, read = vi.fn(async () => value);
  expect(await createVendorUsageService({ read })(aliasHost(vi.fn(async () => found(native)), [native.provider]), ["a"])).toEqual({ a: value });
  expect(read).toHaveBeenCalledExactlyOnceWith(THREAD, expect.any(Object));
 });
});

describe("positive route provenance for retained Codex handles", () => {
 it.each(["cursor", "glm-acp-agent"])("withholds native %s with a retained Codex handle", async (provider) => {
  const native = { ...agent("a"), provider }, read = vi.fn(async () => ({ ...value, input: 100, output: 8 }));
  // Even a conflicting config mapping cannot override the host's native/builtin classification.
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
  const native = { ...agent("a"), provider: "fixture-unverified-route" }, read = vi.fn(async () => value);
  for (const mapping of [{}, { extends: "opencode" }]) {
   const metadata = aliasMetadata([native.provider]);
   metadata.config.get.mockResolvedValue({ config: { providers: { [native.provider]: mapping as never } } });
   expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toMatchObject({ a: { input: null, output: null } });
  }
  expect(read).not.toHaveBeenCalled();
 });
 it.each(["cursor", "glm-acp-agent"])("discards usage after a Codex to native %s route transition", async (provider) => {
  const after = { ...agent("a"), provider }, read = vi.fn(async () => value);
  const refresh = vi.fn().mockResolvedValueOnce(found(agent("a"))).mockResolvedValueOnce(found(after));
  expect(await createVendorUsageService({ read })(host(refresh), ["a"])).toMatchObject({ a: { input: null, output: null } });
  expect(read).toHaveBeenCalledTimes(1);
 });
 it("revalidates a genuine alias mapping after the exact usage read", async () => {
  const native = { ...agent("a"), provider: "fixture-codex-alias" }, metadata = aliasMetadata(["fixture-codex-alias"]);
  metadata.config.get.mockResolvedValueOnce({ config: { providers: { [native.provider]: { extends: "codex" } } } })
   .mockResolvedValueOnce({ config: { providers: { [native.provider]: { extends: "opencode" } } } });
  const read = vi.fn(async () => value);
  expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toMatchObject({ a: { input: null, source: expect.stringContaining("changed") } });
  expect(read).toHaveBeenCalledTimes(1); expect(metadata.config.get).toHaveBeenCalledTimes(2);
 });
 it("withholds alias usage when supported metadata reads fail", async () => {
  const native = { ...agent("a"), provider: "fixture-codex-alias" }, metadata = aliasMetadata(["fixture-codex-alias"]);
  metadata.providers.snapshot.mockRejectedValue(Error("PRIVATE_PROVIDER_CONFIG"));
  const read = vi.fn(async () => value);
  const result = await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"]);
  expect(result.a).toMatchObject({ input: null, output: null }); expect(read).not.toHaveBeenCalled();
  expect(JSON.stringify(result)).not.toContain("PRIVATE_PROVIDER_CONFIG");
 });
});

it("discards a formerly verified alias if the host now classifies it as native", async () => {
 const native = { ...agent("a"), provider: "fixture-codex-alias" }, metadata = aliasMetadata(["fixture-codex-alias"]);
 metadata.providers.snapshot.mockResolvedValueOnce({ entries: [{ provider: native.provider, source: "custom" }] })
  .mockResolvedValueOnce({ entries: [{ provider: native.provider, source: "builtin" }] });
 const read = vi.fn(async () => value);
 expect(await createVendorUsageService({ read })(Object.assign(host(vi.fn(async () => found(native))), metadata), ["a"])).toMatchObject({ a: { input: null, output: null } });
 expect(read).toHaveBeenCalledTimes(1);
});
