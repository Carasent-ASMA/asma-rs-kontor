import { describe, expect, it, vi } from "vitest";
import { createVendorUsageService, codexThreadIdentity } from "../server/vendor-usage";
import { readInstalledNavigatorCapabilities } from "../server/session-navigator";
const THREAD = "00000000-0000-4000-8000-000000000001", NEXT = "00000000-0000-4000-8000-000000000002";
const agent = (id: string, nativeHandle = THREAD, provider = "codex") => ({ id, provider, title: "Same title", persistence: { provider, sessionId: "tree-root-is-not-thread-id", nativeHandle, metadata: { threadId: nativeHandle } } });
const found = (a: ReturnType<typeof agent>) => ({ agent: a });
const host = (refresh: ReturnType<typeof vi.fn>) => ({ agents: { ref: vi.fn(() => ({ refresh })) } }) as never;
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
    expect(await createVendorUsageService({ read: async () => value })(host(vi.fn(async () => found(before))), ["a"])).toEqual({ a: value });
    const refresh = vi.fn().mockResolvedValueOnce(found(before)).mockResolvedValueOnce(found(after));
    expect(await createVendorUsageService({ read: async () => value })(host(refresh), ["a"])).toMatchObject({ a: { input: null, output: null } });
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
