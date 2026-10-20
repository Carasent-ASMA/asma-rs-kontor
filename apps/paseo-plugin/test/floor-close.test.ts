import type { PluginServerContext } from "@getpaseo/plugin/server";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import plugin from "../index.server";
import { FLOOR_INVALID_EVENT, FLOOR_MODE_EVENT, readFloor } from "../server/floor";

// Standard errors injected on floor.json's own descriptor only; every other file uses the real calls.
const faults = vi.hoisted(() => ({ read: null as string | null, close: false, floor: new Set<number>() }));

vi.mock("node:fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:fs")>();
  const failure = (code: string, syscall: string) => Object.assign(new Error(`${code}: injected, ${syscall}`), { code, syscall });
  return {
    ...actual,
    openSync: (...args: Parameters<typeof actual.openSync>) => {
      const descriptor = actual.openSync(...args);
      if (String(args[0]).endsWith("floor.json")) faults.floor.add(descriptor);
      return descriptor;
    },
    readSync: (...args: Parameters<typeof actual.readSync>) => {
      if (faults.read && faults.floor.has(args[0])) throw failure(faults.read, "read");
      return actual.readSync(...args);
    },
    // The descriptor is released before the error, as a close that reports EIO has already released it.
    closeSync: (descriptor: number) => {
      const floor = faults.floor.delete(descriptor);
      actual.closeSync(descriptor);
      if (faults.close && floor) throw failure("EIO", "close");
    },
  };
});

const clearFaults = () => {
  faults.read = null;
  faults.close = false;
  faults.floor.clear();
};

describe("readFloor when floor.json fails to close", () => {
  let dir: string;
  let file: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "kontor-floor-close-"));
    file = join(dir, "floor.json");
  });

  afterEach(() => {
    clearFaults();
    rmSync(dir, { recursive: true, force: true });
  });

  it("does not let off stand after a failed close", () => {
    writeFileSync(file, '{"floor":"off"}');
    faults.close = true;
    expect(readFloor(file)).toEqual({ enabled: true, reason: "invalid", file, error: "cannot close: EIO: injected, close" });
  });

  it("reports a failed close after an explicit on", () => {
    writeFileSync(file, '{"floor":"on"}');
    faults.close = true;
    expect(readFloor(file)).toEqual({ enabled: true, reason: "invalid", file, error: "cannot close: EIO: injected, close" });
  });

  it("keeps a read error alongside the failed close", () => {
    writeFileSync(file, '{"floor":"off"}');
    faults.read = "EIO";
    faults.close = true;
    expect(readFloor(file)).toEqual({
      enabled: true,
      reason: "invalid",
      file,
      error: "cannot read: EIO: injected, read; cannot close: EIO: injected, close",
    });
  });

  it("closes the file after a read error", () => {
    writeFileSync(file, '{"floor":"off"}');
    faults.read = "EIO";
    expect(readFloor(file)).toEqual({ enabled: true, reason: "invalid", file, error: "cannot read: EIO: injected, read" });
    expect(faults.floor.size).toBe(0);
  });

  it("reads an ENOENT after the open as a failed read, not an absent file", () => {
    writeFileSync(file, '{"floor":"off"}');
    faults.read = "ENOENT";
    expect(readFloor(file)).toEqual({ enabled: true, reason: "invalid", file, error: "cannot read: ENOENT: injected, read" });
  });
});

describe("server entry when floor.json fails to close", () => {
  let home: string;
  let file: string;

  beforeEach(() => {
    home = mkdtempSync(join(tmpdir(), "kontor-floor-close-home-"));
    file = join(home, "plugin-config", "kontor", "floor.json");
    mkdirSync(dirname(file), { recursive: true });
    writeFileSync(file, '{"floor":"off"}', { mode: 0o600 });
    vi.stubEnv("PASEO_HOME", home);
    vi.stubEnv("KONTOR_PLUGIN_READ_SOURCES", "");
    vi.stubEnv("KONTOR_PLUGIN_MCP", "");
  });

  afterEach(() => {
    vi.unstubAllEnvs();
    clearFaults();
    rmSync(home, { recursive: true, force: true });
  });

  it("starts with the three hooks, the mode line and the diagnostic for an off file", () => {
    faults.close = true;
    const hooks: string[] = [];
    const info: unknown[] = [];
    const errors: unknown[] = [];
    const server = {
      before: (name: string) => {
        hooks.push(name);
        return () => {};
      },
      handle: () => {},
      registerSettings: () => ({
        read: async () => ({ status: "ready", revision: "r1", values: {} }),
        subscribe: () => () => {},
      }),
    } as unknown as PluginServerContext;
    const log = vi.spyOn(console, "log").mockImplementation((line) => {
      info.push(JSON.parse(String(line)));
    });
    const error = vi.spyOn(console, "error").mockImplementation((line) => {
      errors.push(JSON.parse(String(line)));
    });
    let cleanup = () => {};
    try {
      cleanup = plugin(server);
    } finally {
      log.mockRestore();
      error.mockRestore();
    }
    expect(hooks).toEqual(["agent.create", "agent.session_open", "workspace.create"]);
    expect(info).toEqual([{ event: FLOOR_MODE_EVENT, floor: "enabled", reason: "invalid", file }]);
    expect(errors).toEqual([{ event: FLOOR_INVALID_EVENT, floor: "enabled", file, error: "cannot close: EIO: injected, close" }]);
    cleanup();
  });
});
