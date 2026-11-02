import { mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { readConfig } from "../server/config";
import { createPaseoPublicCli } from "../server/paseo-cli";
import { createCompleteScheduleReader } from "../server/paseo-host";

const SOURCE = "123e4567-e89b-42d3-a456-426614174000";
const config = { paseoCommand: "/opt/paseo", paseoHome: "/srv/paseo", paseoHost: "127.0.0.1:6767" };

describe("bounded public Paseo CLI stop", () => {
  it("stops one exact ID with only the configured home", async () => {
    const calls: { command: string; args: string[] }[] = [];
    const cli = createPaseoPublicCli(config, async (command, args) => { calls.push({ command, args }); return "{}"; });
    expect(await cli.stop(SOURCE)).toEqual({ ok: true });
    expect(calls).toEqual([{ command: "/opt/paseo", args: ["agent", "stop", SOURCE, "--json", "--home", "/srv/paseo"] }]);
  });

  it("rejects prefixes, paths and options instead of passing arbitrary argv", async () => {
    const calls: string[][] = [];
    const cli = createPaseoPublicCli(config, async (_command, args) => { calls.push(args); return "{}"; });
    for (const bad of ["", " ", "\t", "-all", "agent/../x", "agent one", "--all", "/tmp/agent"]) expect(await cli.stop(bad)).toMatchObject({ ok: false });
    expect(calls).toHaveLength(0);
  });

  it("reports an unavailable CLI by name and does not claim a stop", async () => {
    const cli = createPaseoPublicCli(config, async () => { throw Object.assign(new Error("spawn ENOENT"), { code: "ENOENT" }); });
    expect(await cli.stop(SOURCE)).toEqual({ ok: false, unavailable: true, message: "The configured public Paseo CLI /opt/paseo could not be found." });
  });
  it("updates the exact archived display name through a fixed public argument vector", async () => {
    const calls: string[][] = [];
    const cli = createPaseoPublicCli(config, async (_command, args) => { calls.push(args); return "{}"; });
    expect(await cli.rename!(SOURCE, "STALE • Fixture")).toEqual({ ok: true });
    expect(calls).toEqual([["agent", "update", SOURCE, "--name", "STALE • Fixture", "--json", "--home", config.paseoHome]]);
    expect(await cli.rename!("--all", "Fixture")).toMatchObject({ ok: false });
    expect(calls).toHaveLength(1);
  });

  it.each([
    ["home only", { paseoHome: "/srv/paseo", paseoHost: null }, ["--home", "/srv/paseo"]],
    ["host only", { paseoHome: null, paseoHost: "127.0.0.1:6767" }, ["--host", "127.0.0.1:6767"]],
    ["both set, home wins", { paseoHome: "/srv/paseo", paseoHost: "127.0.0.1:6767" }, ["--home", "/srv/paseo"]],
    ["neither", { paseoHome: null, paseoHost: null }, []],
  ] as const)("passes exactly one daemon selector, never --home with --host (%s)", async (_name, selected, selector) => {
    const calls: string[][] = [];
    const cli = createPaseoPublicCli({ paseoCommand: "/opt/paseo", ...selected }, async (_command, args) => { calls.push(args); return "{}"; });
    await cli.stop(SOURCE);
    await cli.rename!(SOURCE, "Fixture");
    expect(calls).toEqual([["agent", "stop", SOURCE, "--json", ...selector], ["agent", "update", SOURCE, "--name", "Fixture", "--json", ...selector]]);
    for (const args of calls) expect(args.includes("--home") && args.includes("--host")).toBe(false);
  });

  it("gives the CLI only --home and the host connector the daemon.listen fallback from one readConfig", async () => {
    const home = await mkdtemp(join(tmpdir(), "kontor-paseo-home-"));
    try {
      await writeFile(join(home, "config.json"), JSON.stringify({ daemon: { listen: "127.0.0.1:7123" } }));
      const resolved = readConfig({ HOME: "/Users/test", PASEO_HOME: home });
      expect(resolved.paseoHost).toBe("127.0.0.1:7123");
      const calls: string[][] = [];
      const cli = createPaseoPublicCli(resolved, async (_command, args) => { calls.push(args); return "{}"; });
      await cli.stop(SOURCE);
      await cli.rename!(SOURCE, "Fixture");
      expect(calls).toHaveLength(2);
      for (const args of calls) {
        expect(args.slice(args.indexOf("--json") + 1)).toEqual(["--home", home]);
        expect(args).not.toContain("--host");
      }
      const connections: string[] = [];
      const read = createCompleteScheduleReader(resolved, async () => ({
        createHostClient(connection: string) {
          connections.push(connection);
          return { connect: async () => {}, close: async () => {}, getLastServerInfoMessage: () => ({ serverId: "server-1" }), scheduleList: async () => ({ schedules: [] }) };
        },
      }));
      expect(await read()).toEqual({ complete: true, hostId: "server-1", schedules: [] });
      expect(connections).toEqual(["127.0.0.1:7123"]);
    } finally {
      await rm(home, { recursive: true, force: true });
    }
  });
});
