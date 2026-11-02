import { readFileSync } from "node:fs";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createCompleteScheduleReader } from "../server/paseo-host";

const HOST = "127.0.0.1:6767";

function connector({
  before = "server-1",
  after,
  schedules = [] as unknown[],
  error = null as string | null,
  calls = [] as { connection: string; config: Record<string, unknown> }[],
}: { before?: string; after?: string; schedules?: unknown[]; error?: string | null; calls?: { connection: string; config: Record<string, unknown> }[] } = {}) {
  return async () => ({
    createHostClient(connection: string, config: Record<string, unknown>) {
      calls.push({ connection, config });
      let identityReads = 0;
      return {
        connect: async () => {},
        close: async () => {},
        getLastServerInfoMessage: () => ({ serverId: identityReads++ === 0 ? before : after ?? before }),
        scheduleList: async () => ({ schedules, error }),
      };
    },
  });
}

describe("complete public host schedule reader", () => {
  it("requires an explicit server-selected host and does not load a connector without one", async () => {
    let loaded = false;
    const read = createCompleteScheduleReader({ paseoHost: null }, async () => { loaded = true; return null; });
    expect(await read()).toMatchObject({ complete: false, gap: expect.stringContaining("No server-selected paired host") });
    expect(loaded).toBe(false);
  });

  it("reports the public connector version floor without blaming an older development SDK", async () => {
    const read = createCompleteScheduleReader({ paseoHost: HOST }, async () => null);
    const missing = await read();
    expect(missing).toMatchObject({ complete: false });
    if (!missing.complete) {
      expect(missing.gap).toContain("0.13.2 or later");
      expect(missing.gap).not.toContain("0.11.4");
    }
  });

  it("returns an empty complete inventory as complete and host-qualified", async () => {
    const calls: { connection: string; config: Record<string, unknown> }[] = [];
    const read = createCompleteScheduleReader({ paseoHost: HOST }, connector({ calls }));
    expect(await read()).toEqual({ complete: true, hostId: "server-1", schedules: [] });
    expect(calls).toHaveLength(1);
    expect(calls[0]!.connection).toBe(HOST);
    expect(calls[0]!.config).toMatchObject({ clientId: "kontor-paseo-plugin", connectTimeoutMs: 10_000 });
  });

  it("accepts unrelated existing-agent rows without requiring them for another source", async () => {
    const read = createCompleteScheduleReader({ paseoHost: HOST }, connector({
      schedules: [{ target: { type: "agent", agentId: "other-agent" }, status: "active" }, { target: { type: "new-agent" }, status: "paused" }],
    }));
    expect(await read()).toMatchObject({ complete: true, hostId: "server-1", schedules: [
      { target: { type: "agent", agentId: "other-agent" }, status: "active" },
      { target: { type: "new-agent" }, status: "paused" },
    ] });
  });

  it("does not qualify a list when the daemon identity changes during the read", async () => {
    const read = createCompleteScheduleReader({ paseoHost: HOST }, connector({ before: "server-1", after: "server-2" }));
    expect(await read()).toMatchObject({ complete: false, gap: expect.stringContaining("identity changed") });
  });

  it("does not qualify an unreadable list or an unrecognized target", async () => {
    const unreadable = createCompleteScheduleReader({ paseoHost: HOST }, connector({ error: "rpc failed" }));
    expect(await unreadable()).toMatchObject({ complete: false, gap: expect.stringContaining("unreadable") });
    const malformed = createCompleteScheduleReader({ paseoHost: HOST }, connector({ schedules: [{ target: { type: "other" }, status: "active" }] }));
    expect(await malformed()).toMatchObject({ complete: false, gap: expect.stringContaining("not complete") });
  });

  it.each(["", " ", "\t"])("rejects an empty or whitespace target identity (%j)", async (agentId) => {
    const read = createCompleteScheduleReader({ paseoHost: HOST }, connector({ schedules: [{ target: { type: "agent", agentId }, status: "active" }] }));
    expect(await read()).toMatchObject({ complete: false, gap: expect.stringContaining("not complete") });
  });
});

describe("complete reader connector boundary", () => {
  it("uses only the public node subpath and no hidden plugin object or private store", () => {
    const text = readFileSync("server/paseo-host.ts", "utf8");
    expect(text).toContain('const specifier = "@getpaseo/client/node";');
    expect(text).not.toMatch(/paseo\.(?:schedules|agents|workspaces)|daemon\.json|credentials|localCredential|createPaseoApi/);
  });
});

describe("default public connector loader", () => {
  afterEach(() => {
    vi.doUnmock("@getpaseo/client/node");
    vi.resetModules();
  });

  it("reports the connector gap when the installed node entry lacks createHostClient", async () => {
    vi.doMock("@getpaseo/client/node", () => ({ createHostClient: undefined }));
    const { createCompleteScheduleReader: reader } = await import("../server/paseo-host");
    expect(await reader({ paseoHost: HOST })()).toMatchObject({ complete: false, gap: expect.stringContaining("does not export createHostClient") });
  });
});

const SDK_FLOOR = [0, 13, 2] as const;

/** Numeric `major.minor.patch` comparison against the floor: 0.13.10 passes and 0.13.1 fails. */
function meetsSdkFloor(version: string): boolean {
  const parts = /^(\d+)\.(\d+)\.(\d+)$/.exec(version)?.slice(1).map(Number);
  if (!parts) return false;
  for (const [index, floor] of SDK_FLOOR.entries()) {
    if (parts[index] !== floor) return parts[index]! > floor;
  }
  return true;
}

describe("Paseo development SDK floor", () => {
  it.each([
    ["0.13.1", false],
    ["0.12.99", false],
    ["0.13", false],
    ["0.13.2", true],
    ["0.13.10", true],
    ["0.14.0", true],
    ["1.0.0", true],
  ] as const)("compares %s with the 0.13.2 floor numerically (%s)", (version, meets) => {
    expect(meetsSdkFloor(version)).toBe(meets);
  });

  it("declares the three SDK packages at a floor of 0.13.2 or later and exports the public node connector", async () => {
    const manifest = JSON.parse(readFileSync("package.json", "utf8")) as {
      devDependencies: Record<string, string>;
    };
    for (const name of ["@getpaseo/client", "@getpaseo/plugin", "@getpaseo/protocol"]) {
      const floor = /^>=(\S+)$/.exec(manifest.devDependencies[name] ?? "")?.[1];
      expect(floor, `${name} declares a >= floor`).toBeDefined();
      expect(meetsSdkFloor(floor!), `${name} floor ${floor}`).toBe(true);
    }
    const node = await import("@getpaseo/client/node") as { createHostClient?: unknown };
    expect(typeof node.createHostClient, "@getpaseo/client/node createHostClient").toBe("function");
  });
});
