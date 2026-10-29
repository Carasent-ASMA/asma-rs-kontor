import { describe, expect, it } from "vitest";
import { createPaseoPublicCli } from "../server/paseo-cli";

const SOURCE = "123e4567-e89b-42d3-a456-426614174000";
const config = { paseoCommand: "/opt/paseo", paseoHome: "/srv/paseo", paseoHost: "127.0.0.1:6767" };

describe("bounded public Paseo CLI", () => {
  it("stops one exact ID with only the configured home and host", async () => {
    const calls: { command: string; args: string[] }[] = [];
    const cli = createPaseoPublicCli(config, async (command, args) => { calls.push({ command, args }); return "{}"; });
    expect(await cli.stop(SOURCE)).toEqual({ ok: true });
    expect(calls).toEqual([{ command: "/opt/paseo", args: ["agent", "stop", SOURCE, "--json", "--home", "/srv/paseo", "--host", "127.0.0.1:6767"] }]);
  });

  it("rejects prefixes, paths and options instead of passing arbitrary argv", async () => {
    const calls: string[][] = [];
    const cli = createPaseoPublicCli(config, async (_command, args) => { calls.push(args); return "{}"; });
    for (const bad of ["-all", "agent/../x", "agent one", "--all", "/tmp/agent"]) {
      expect(await cli.stop(bad)).toMatchObject({ ok: false });
      expect(await cli.wakeState(bad)).toMatchObject({ wakes: "unknown" });
    }
    expect(calls).toHaveLength(0);
  });

  it("reports an unavailable CLI by name and does not claim a stop", async () => {
    const cli = createPaseoPublicCli(config, async () => { throw Object.assign(new Error("spawn ENOENT"), { code: "ENOENT" }); });
    expect(await cli.stop(SOURCE)).toEqual({ ok: false, unavailable: true, message: "The configured public Paseo CLI /opt/paseo could not be found." });
  });

  it("does not infer no source wakes from the CLI's new-agent-only schedule list", async () => {
    const cli = createPaseoPublicCli(config, async (_command, args) => args.includes("ls")
      ? JSON.stringify([{ id: "s1", target: "new-agent:opencode/model" }])
      : JSON.stringify({}));
    const read = await cli.wakeState(SOURCE);
    expect(read.wakes).toBe("unknown");
    expect(read.gap).toContain("new-agent schedules");
    expect(read.gap).toContain("heartbeat");
  });

  it("inspects exact existing-agent targets and settles only when the list can prove coverage", async () => {
    const inspected: string[] = [];
    const cli = createPaseoPublicCli(config, async (_command, args) => {
      if (args.includes("ls")) return JSON.stringify([{ id: "s1", target: "agent:other" }]);
      inspected.push(args[2]!);
      return JSON.stringify({ target: { type: "agent", agentId: "other-agent" } });
    });
    expect(await cli.wakeState(SOURCE)).toEqual({ wakes: "clear" });
    expect(inspected).toEqual(["s1"]);
  });

  it("reports a source-linked wake without copying its schedule or cadence", async () => {
    const cli = createPaseoPublicCli(config, async (_command, args) => args.includes("ls")
      ? JSON.stringify([{ id: "s1", target: "self:123e456" }])
      : JSON.stringify({ target: { type: "self", agentId: SOURCE } }));
    const read = await cli.wakeState(SOURCE);
    expect(read.wakes).toBe("unknown");
    expect(read.gap).toContain("source-linked scheduled wake");
    expect(JSON.stringify(read)).not.toContain("s1");
  });
});
