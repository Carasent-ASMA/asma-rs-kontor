import type { PluginServerContext } from "@getpaseo/plugin/server";
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import plugin from "../index.server";
import { FLOOR_INVALID_EVENT, FLOOR_MODE_EVENT } from "../server/floor";

const HOOKS = ["agent.create", "agent.session_open", "workspace.create"];

type Started = {
  hooks: string[];
  settings: string[];
  handlers: string[];
  info: Record<string, unknown>[];
  errors: Record<string, unknown>[];
  cleanup: () => void;
};

/** One plugin server start, as a new worker runs it, against a recording host. */
function start(): Started {
  const started: Omit<Started, "cleanup"> = { hooks: [], settings: [], handlers: [], info: [], errors: [] };
  const server = {
    before: (name: string) => {
      started.hooks.push(name);
      return () => {};
    },
    handle: (contract: { readonly name: string }) => {
      started.handlers.push(contract.name);
    },
    registerSettings: (definition: { readonly id: string }) => {
      started.settings.push(definition.id);
      return {
        read: async () => ({ status: "ready", revision: "r1", values: {} }),
        subscribe: () => () => {},
      };
    },
  } as unknown as PluginServerContext;
  const log = vi.spyOn(console, "log").mockImplementation((line) => {
    started.info.push(JSON.parse(String(line)));
  });
  const error = vi.spyOn(console, "error").mockImplementation((line) => {
    started.errors.push(JSON.parse(String(line)));
  });
  try {
    return { ...started, cleanup: plugin(server) };
  } finally {
    log.mockRestore();
    error.mockRestore();
  }
}

describe("server entry floor mode", () => {
  let home: string;
  let file: string;

  beforeEach(() => {
    home = mkdtempSync(join(tmpdir(), "kontor-floor-home-"));
    file = join(home, "plugin-config", "kontor", "floor.json");
    vi.stubEnv("PASEO_HOME", home);
    vi.stubEnv("KONTOR_PLUGIN_READ_SOURCES", "");
    vi.stubEnv("KONTOR_PLUGIN_MCP", "");
    vi.stubEnv("KONTOR_PLUGIN_FLOOR", "");
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    rmSync(home, { recursive: true, force: true });
  });

  const writeFloor = (content: string) => {
    mkdirSync(join(home, "plugin-config", "kontor"), { recursive: true });
    writeFileSync(file, content, { mode: 0o600 });
  };

  it("registers the three hooks when floor.json is absent, with the mode line only", () => {
    const started = start();
    expect(started.hooks).toEqual(HOOKS);
    expect(started.info).toEqual([{ event: FLOOR_MODE_EVENT, floor: "enabled", reason: "not-configured", file }]);
    expect(started.errors).toEqual([]);
    expect(existsSync(join(home, "plugin-config"))).toBe(false);
  });

  it("registers the three hooks for an explicit on", () => {
    writeFloor('{"floor":"on"}');
    const started = start();
    expect(started.hooks).toEqual(HOOKS);
    expect(started.info).toEqual([{ event: FLOOR_MODE_EVENT, floor: "enabled", reason: "explicit-on", file }]);
    expect(started.errors).toEqual([]);
  });

  it("registers none of the three hooks for an explicit off and keeps the navigator", () => {
    const on = start();
    writeFloor('{\n  "floor": "off"\n}\n');
    const off = start();
    expect(off.hooks).toEqual([]);
    expect(off.info).toEqual([{ event: FLOOR_MODE_EVENT, floor: "disabled", reason: "explicit-off", file }]);
    expect(off.errors).toEqual([]);
    expect(off.settings).toEqual(["organization", "keyboard"]);
    expect(off.settings).toEqual(on.settings);
    expect(off.handlers).toEqual(on.handlers);
    expect(off.handlers).toEqual(expect.arrayContaining(["sources.list", "sources.summaries", "sources.read", "sources.associations", "kontor.status"]));
    expect(off.cleanup()).toBeUndefined();
  });

  it("registers the three hooks for a non-conforming file and reports why", () => {
    writeFloor('{"floor":"on","floor":"off"}');
    const started = start();
    expect(started.hooks).toEqual(HOOKS);
    expect(started.info).toEqual([{ event: FLOOR_MODE_EVENT, floor: "enabled", reason: "invalid", file }]);
    expect(started.errors).toEqual([
      { event: FLOOR_INVALID_EVENT, floor: "enabled", file, error: 'expected exactly one "floor" member, found it repeated' },
    ]);
  });

  it("never reads KONTOR_PLUGIN_FLOOR", () => {
    vi.stubEnv("KONTOR_PLUGIN_FLOOR", "off");
    expect(start().hooks).toEqual(HOOKS);
    writeFloor('{"floor":"off"}');
    vi.stubEnv("KONTOR_PLUGIN_FLOOR", "on");
    expect(start().hooks).toEqual([]);
  });

  it("keeps the three hooks with KONTOR_PLUGIN_MCP=off alone, and floor-off wins over it", () => {
    vi.stubEnv("KONTOR_PLUGIN_MCP", "off");
    expect(start().hooks).toEqual(HOOKS);
    writeFloor('{"floor":"off"}');
    expect(start().hooks).toEqual([]);
  });

  it("reads the file once per start, so only a new worker sees a change", () => {
    writeFloor('{"floor":"off"}');
    const off = start();
    writeFloor('{"floor":"on"}');
    expect(off.hooks).toEqual([]);
    const reEnabled = start();
    expect(reEnabled.hooks).toEqual(HOOKS);
    rmSync(file);
    expect(start().hooks).toEqual(HOOKS);
    writeFloor('{"floor":"off"}');
    expect(start().hooks).toEqual([]);
  });

  it("never writes, moves or removes floor.json", () => {
    writeFloor('{"floor":"off"}');
    const before = statSync(file);
    start().cleanup();
    const after = statSync(file);
    expect(after.mtimeMs).toBe(before.mtimeMs);
    expect(after.ino).toBe(before.ino);
    expect(after.mode).toBe(before.mode);
    expect(readdirSync(join(home, "plugin-config", "kontor"))).toEqual(["floor.json"]);
  });
});
