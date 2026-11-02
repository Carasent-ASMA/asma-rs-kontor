import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { applyTakeover } from "../shared/session-lifecycle";
import { contributeSessionLifecycle } from "../server/session-lifecycle";
import { createPaseoPublicCli } from "../server/paseo-cli";
import { createCompleteScheduleReader } from "../server/paseo-host";

const seen = vi.hoisted(() => ({ argv: [] as string[][], connections: [] as string[] }));
// The real factories, with only the process runner and the connector loader replaced, so the handler's own wiring runs.
vi.mock("../server/paseo-cli", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../server/paseo-cli")>();
  return { ...actual, createPaseoPublicCli: vi.fn((config: Parameters<typeof actual.createPaseoPublicCli>[0]) =>
    actual.createPaseoPublicCli(config, async (_command, args) => { seen.argv.push(args); return "{}"; })) };
});
vi.mock("../server/paseo-host", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../server/paseo-host")>();
  return { ...actual, createCompleteScheduleReader: vi.fn((config: Parameters<typeof actual.createCompleteScheduleReader>[0]) =>
    actual.createCompleteScheduleReader(config, async () => ({
      createHostClient(connection: string) {
        seen.connections.push(connection);
        return { connect: async () => {}, close: async () => {}, getLastServerInfoMessage: () => ({ serverId: "server-1" }), scheduleList: async () => ({ schedules: [] }) };
      },
    }))) };
});

const SOURCE = "123e4567-e89b-42d3-a456-426614174000";

describe("installed takeover wiring", () => {
  afterEach(() => { vi.unstubAllEnvs(); });

  it("builds the public CLI and the schedule reader from one config: one --home selector, the daemon.listen host", async () => {
    const home = await mkdtemp(join(tmpdir(), "kontor-takeover-wiring-"));
    try {
      await writeFile(join(home, "config.json"), JSON.stringify({ daemon: { listen: "127.0.0.1:7124" } }));
      vi.stubEnv("PASEO_HOME", home); vi.stubEnv("KONTOR_PLUGIN_PASEO_HOME", ""); vi.stubEnv("KONTOR_PLUGIN_PASEO_HOST", "");
      const handlers = new Map<string, (...args: any[]) => unknown>();
      contributeSessionLifecycle({ handle: (rpc: { name: string }, handler: (...args: any[]) => unknown) => handlers.set(rpc.name, handler) } as unknown as PluginServerContext);
      const host = { schedules: { forAgent: async () => ({ entries: [] }) }, agents: { ref: () => ({}) }, workspaces: { ref: () => ({ agents: {} }) } };
      await expect(handlers.get(applyTakeover.name)!({ reviewId: "33333333-3333-4333-8333-333333333333", revision: 1, digest: "a".repeat(64) }, { paseo: host }))
        .rejects.toThrow("This review is not on this host");
      expect(vi.mocked(createPaseoPublicCli)).toHaveBeenCalledTimes(1);
      expect(vi.mocked(createCompleteScheduleReader)).toHaveBeenCalledTimes(1);
      const cli = vi.mocked(createPaseoPublicCli).mock.results[0]!.value as ReturnType<typeof createPaseoPublicCli>;
      await cli.stop(SOURCE);
      await cli.rename!(SOURCE, "Fixture");
      expect(seen.argv).toEqual([["agent", "stop", SOURCE, "--json", "--home", home], ["agent", "update", SOURCE, "--name", "Fixture", "--json", "--home", home]]);
      const read = vi.mocked(createCompleteScheduleReader).mock.results[0]!.value as ReturnType<typeof createCompleteScheduleReader>;
      expect(await read()).toEqual({ complete: true, hostId: "server-1", schedules: [] });
      expect(seen.connections).toEqual(["127.0.0.1:7124"]);
    } finally {
      await rm(home, { recursive: true, force: true });
    }
  });
});
