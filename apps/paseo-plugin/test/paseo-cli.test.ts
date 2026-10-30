import { describe, expect, it } from "vitest";
import { createPaseoPublicCli } from "../server/paseo-cli";

const SOURCE = "123e4567-e89b-42d3-a456-426614174000";
const config = { paseoCommand: "/opt/paseo", paseoHome: "/srv/paseo", paseoHost: "127.0.0.1:6767" };

describe("bounded public Paseo CLI stop", () => {
  it("stops one exact ID with only the configured home and host", async () => {
    const calls: { command: string; args: string[] }[] = [];
    const cli = createPaseoPublicCli(config, async (command, args) => { calls.push({ command, args }); return "{}"; });
    expect(await cli.stop(SOURCE)).toEqual({ ok: true });
    expect(calls).toEqual([{ command: "/opt/paseo", args: ["agent", "stop", SOURCE, "--json", "--home", "/srv/paseo", "--host", "127.0.0.1:6767"] }]);
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
    expect(calls).toEqual([["agent", "update", SOURCE, "--name", "STALE • Fixture", "--json", "--home", config.paseoHome, "--host", config.paseoHost]]);
    expect(await cli.rename!("--all", "Fixture")).toMatchObject({ ok: false });
    expect(calls).toHaveLength(1);
  });

});
