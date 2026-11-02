import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  DEFAULT_CREDENTIAL_TIER,
  DEFAULT_MCP_COMMAND,
  readConfig,
} from "../server/config";

describe("readConfig", () => {
  it("defaults to the documented state root and observer authority", () => {
    const config = readConfig({ HOME: "/Users/test" });
    expect(config.stateRoot).toBe("/Users/test/.local/state/kontor/asma");
    expect(config.mcpCommand).toBe(DEFAULT_MCP_COMMAND);
    expect(config.cliCommand).toBe("kontor");
    expect(config.paseoCommand).toBe("paseo");
    expect(config.paseoHome).toBeNull();
    expect(config.paseoHost).toBeNull();
    expect(config.mcpEnabled).toBe(true);
    expect(config.serveProfile).toBeNull();
    expect(config.placementRoots).toEqual([]);
    expect(config.paseoWorktreesRoot).toBe("/Users/test/.paseo/worktrees");
    expect(DEFAULT_CREDENTIAL_TIER).toBe("observer");
  });

  it("follows a custom Paseo home for the worktree root", () => {
    const config = readConfig({ HOME: "/Users/test", PASEO_HOME: "/srv/paseo" });
    expect(config.paseoWorktreesRoot).toBe("/srv/paseo/worktrees");
    expect(config.paseoHome).toBe("/srv/paseo");
  });

  it("reads the plugin's read-only overrides", () => {
    const config = readConfig({
      HOME: "/Users/test",
      KONTOR_PLUGIN_STATE_ROOT: "/srv/kontor",
      KONTOR_PLUGIN_MCP_COMMAND: "/opt/kontor-mcp",
      KONTOR_PLUGIN_CLI_COMMAND: "/opt/kontor",
      KONTOR_PLUGIN_PASEO_COMMAND: "/opt/paseo",
      KONTOR_PLUGIN_PASEO_HOME: "/srv/paseo-home",
      KONTOR_PLUGIN_PASEO_HOST: "127.0.0.1:6767",
      KONTOR_PLUGIN_MCP: "off",
      KONTOR_PLUGIN_SERVE_PROFILE: "leadership",
      KONTOR_PLUGIN_PLACEMENT_ROOTS: "/Users/test/repos, /Users/test/other",
      KONTOR_PLUGIN_LABEL_TIMEOUT_MS: "250",
    });
    expect(config.stateRoot).toBe("/srv/kontor");
    expect(config.mcpCommand).toBe("/opt/kontor-mcp");
    expect(config.cliCommand).toBe("/opt/kontor");
    expect(config.paseoCommand).toBe("/opt/paseo");
    expect(config.paseoHome).toBe("/srv/paseo-home");
    expect(config.paseoHost).toBe("127.0.0.1:6767");
    expect(config.mcpEnabled).toBe(false);
    expect(config.serveProfile).toBe("leadership");
    expect(config.placementRoots).toEqual(["/Users/test/repos", "/Users/test/other"]);
    expect(config.labelReadTimeoutMs).toBe(250);
  });

  it("carries no credential setting", () => {
    const config = readConfig({ HOME: "/Users/test" });
    expect(Object.keys(config)).not.toContain("credentialTier");
    expect(Object.keys(config)).not.toContain("auth");
    expect(JSON.stringify(config)).not.toContain("KONTOR_AUTH");
  });
});

describe("readConfig Paseo host fallback", () => {
  const SECRET = "planted-daemon-secret";
  const dirs: string[] = [];
  afterEach(async () => {
    vi.restoreAllMocks();
    await Promise.all(dirs.splice(0).map((dir) => rm(dir, { recursive: true, force: true })));
  });
  async function paseoHome(config?: string): Promise<string> {
    const dir = await mkdtemp(join(tmpdir(), "kontor-config-paseo-"));
    dirs.push(dir);
    if (config !== undefined) await writeFile(join(dir, "config.json"), config);
    return dir;
  }
  const listening = (listen: unknown) => JSON.stringify({ daemon: { listen, relay: { password: SECRET } } });
  const hostFor = (paseo: string, extra: Record<string, string> = {}) =>
    readConfig({ HOME: "/Users/test", PASEO_HOME: paseo, ...extra }).paseoHost;

  it("lets the variable win over the daemon config", async () => {
    const home = await paseoHome(listening("127.0.0.1:7000"));
    expect(hostFor(home, { KONTOR_PLUGIN_PASEO_HOST: "127.0.0.1:6767" })).toBe("127.0.0.1:6767");
  });

  it.each(["", "  "])("reads daemon.listen when the variable is blank (%j)", async (value) => {
    const home = await paseoHome(listening("127.0.0.1:7000"));
    expect(hostFor(home, { KONTOR_PLUGIN_PASEO_HOST: value })).toBe("127.0.0.1:7000");
  });

  it.each([
    ["127.0.0.1:6767", "127.0.0.1:6767"],
    [" localhost:6767 ", "localhost:6767"],
    ["0.0.0.0:6767", "127.0.0.1:6767"],
    ["[::]:6767", "[::1]:6767"],
    ["[::1]:7001", "[::1]:7001"],
  ])("maps daemon.listen %j to the client host %j", async (listen, host) => {
    expect(hostFor(await paseoHome(listening(listen)))).toBe(host);
  });

  it("reads the configured Paseo home, else ~/.paseo", async () => {
    const plugin = await paseoHome(listening("127.0.0.1:7001"));
    const paseo = await paseoHome(listening("127.0.0.1:7002"));
    const user = await paseoHome();
    await mkdir(join(user, ".paseo"));
    await writeFile(join(user, ".paseo", "config.json"), listening("127.0.0.1:7003"));
    expect(readConfig({ HOME: user, PASEO_HOME: paseo, KONTOR_PLUGIN_PASEO_HOME: plugin }).paseoHost).toBe("127.0.0.1:7001");
    expect(readConfig({ HOME: user, PASEO_HOME: paseo }).paseoHost).toBe("127.0.0.1:7002");
    expect(readConfig({ HOME: user }).paseoHost).toBe("127.0.0.1:7003");
  });

  it.each([
    ["a unix socket URI", listening("unix:///tmp/paseo.sock")],
    ["a socket path", listening("/tmp/paseo.sock")],
    ["a home socket path", listening("~/.paseo/paseo.sock")],
    ["a pipe", listening("pipe://paseo")],
    ["a WebSocket URL", listening("ws://127.0.0.1:6767")],
    ["a tcp URI", listening("tcp://127.0.0.1:6767")],
    ["a host without a port", listening("localhost")],
    ["a bare port", listening("6767")],
    ["an empty host", listening(":6767")],
    ["port 0", listening("127.0.0.1:0")],
    ["an out-of-range port", listening("127.0.0.1:65536")],
    ["a path suffix", listening("127.0.0.1:6767/ws")],
    ["an empty listen", listening("")],
    ["a null listen", listening(null)],
    ["a numeric listen", listening(6767)],
    ["a missing listen", JSON.stringify({ daemon: {} })],
    ["a missing daemon", JSON.stringify({})],
    ["a JSON null", "null"],
    ["invalid JSON", `{ "daemon": { "listen": "127.0.0.1:6767", "password": "${SECRET}" `],
  ])("keeps the no-host gap for %s", async (_name, config) => {
    expect(hostFor(await paseoHome(config))).toBeNull();
  });

  it("keeps the no-host gap for a missing or unreadable config file", async () => {
    expect(hostFor(await paseoHome())).toBeNull();
    const unreadable = await paseoHome();
    await mkdir(join(unreadable, "config.json"));
    expect(hostFor(unreadable)).toBeNull();
  });

  it("never logs or carries the daemon config contents", async () => {
    const logs = (["log", "info", "warn", "error", "debug"] as const).map((method) => vi.spyOn(console, method).mockImplementation(() => {}));
    const read = readConfig({ HOME: "/Users/test", PASEO_HOME: await paseoHome(listening("0.0.0.0:6767")) });
    hostFor(await paseoHome(`{ "password": "${SECRET}" `));
    expect(read.paseoHost).toBe("127.0.0.1:6767");
    expect(JSON.stringify(read)).not.toContain(SECRET);
    for (const log of logs) expect(log).not.toHaveBeenCalled();
  });
});
