import { createHash } from "node:crypto";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
const host = vi.hoisted(() => ({ home: "" }));
vi.mock("../server/config", () => ({ readConfig: () => ({ paseoWorktreesRoot: join(host.home, "worktrees") }) }));
import { contributeTokenomics, readTokenomicsSnapshot, tokenomicsView } from "../server/tokenomics";
import { readTokenomics, tokenomicsQuerySchema, tokenomicsViewSchema, type TokenomicsQuery, type TokenomicsView } from "../shared/tokenomics";
import { tokenomicsFixture, tokenomicsLiveFixture } from "./fixtures/tokenomics";

let config: string, root: string, raw: string, live: string;
const query = tokenomicsQuerySchema.parse({});
function rpc() {
  const handlers = new Map<string, (input: TokenomicsQuery) => Promise<TokenomicsView>>();
  contributeTokenomics({ handle: (contract: { name: string }, handler: (input: TokenomicsQuery) => Promise<TokenomicsView>) => handlers.set(contract.name, handler) } as unknown as PluginServerContext);
  return () => handlers.get("tokenomics.read")!(query);
}
beforeEach(async () => {
  host.home = await mkdtemp(join(tmpdir(), "kontor-tokenomics-source-"));
  config = join(host.home, "plugin-config", "kontor", "tokenomics.json");
  root = join(host.home, "report");
  await mkdir(join(host.home, "plugin-config", "kontor"), { recursive: true });
  await mkdir(join(root, "_stats"), { recursive: true });
  raw = JSON.stringify(tokenomicsFixture()); live = `window.ASMA_LIVE=${JSON.stringify(tokenomicsLiveFixture())};`;
  await writeFile(join(root, "data.json"), raw); await writeFile(join(root, "_stats", "live.js"), live);
  vi.stubEnv("KONTOR_PLUGIN_TOKENOMICS_ROOT", undefined);
});
afterEach(async () => { vi.unstubAllEnvs(); await rm(host.home, { recursive: true, force: true }); });

describe("configured native RPC source provenance", () => {
  it("attests config in the actual read result with its revision, without exposing the root", async () => {
    await writeFile(config, JSON.stringify({ root }));
    const view = await rpc()();
    expect(view.sourceKind).toBe("config");
    expect(view.revision).toBe(createHash("sha256").update(raw).update(live).digest("hex"));
    expect(tokenomicsViewSchema.safeParse(view).success).toBe(true);
    expect(JSON.stringify(view)).not.toContain(host.home);
  });
  it("preserves environment precedence over malformed config and observes changes on each read", async () => {
    await writeFile(config, JSON.stringify({ root })); const read = rpc();
    expect((await read()).sourceKind).toBe("config");
    await writeFile(config, "invalid config"); vi.stubEnv("KONTOR_PLUGIN_TOKENOMICS_ROOT", root);
    const view = await read(); expect(view.sourceKind).toBe("environment"); expect(view.revision).not.toBeNull();
    expect(JSON.stringify(view)).not.toContain(host.home);
  });
  it.each(["", "relative/report"])("refuses invalid environment root %j without falling back to valid config", async (value) => {
    await writeFile(config, JSON.stringify({ root })); vi.stubEnv("KONTOR_PLUGIN_TOKENOMICS_ROOT", value);
    const view = await rpc()(); expect(view.sourceKind).toBe("invalid"); expect(view.status).toBe("unavailable"); expect(view.revision).toBeNull();
    expect(view.diagnostics).toEqual(["Tokenomics host configuration is invalid or unreadable."]);
    expect(JSON.stringify(view)).not.toContain(host.home);
  });
  it.each(["not JSON", JSON.stringify({ root: "relative/report" }), JSON.stringify({ root: "/synthetic/report", unexpected: true })])("refuses malformed config without leaking its values", async (value) => {
    await writeFile(config, value); const view = await rpc()();
    expect(view.sourceKind).toBe("invalid"); expect(view.status).toBe("unavailable"); expect(JSON.stringify(view)).not.toContain(host.home);
  });
  it("distinguishes unconfigured from invalid and from an unreadable configured report", async () => {
    const read = rpc(); const absent = await read(); expect(absent.sourceKind).toBe("unconfigured"); expect(absent.status).toBe("unavailable");
    await writeFile(config, JSON.stringify({ root })); await writeFile(join(root, "data.json"), "invalid report");
    const unreadable = await read(); expect(unreadable.sourceKind).toBe("config"); expect(unreadable.status).toBe("unavailable");
    expect(JSON.stringify(unreadable)).not.toContain(host.home);
  });
  it("leaves direct harness roots and legacy results unattested, and rejects client provenance injection", async () => {
    const direct = tokenomicsView(await readTokenomicsSnapshot(root), query);
    expect(direct.sourceKind).toBeUndefined(); expect(JSON.parse(JSON.stringify(direct))).not.toHaveProperty("sourceKind");
    expect(tokenomicsViewSchema.safeParse(direct).success).toBe(true);
    expect(tokenomicsViewSchema.safeParse({ ...direct, sourceKind: "provided" }).success).toBe(false);
    expect(readTokenomics.input.safeParse({ sourceKind: "config" }).success).toBe(false);
  });
});
