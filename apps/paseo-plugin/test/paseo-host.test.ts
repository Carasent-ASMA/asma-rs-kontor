import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
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

  it("reports the exact public connector version boundary without blaming the pinned development SDK", async () => {
    const read = createCompleteScheduleReader({ paseoHost: HOST }, async () => null);
    const missing = await read();
    expect(missing).toMatchObject({ complete: false });
    if (!missing.complete) {
      expect(missing.gap).toContain("0.13.2");
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

describe("pinned Paseo development SDK", () => {
  it("pins the three SDK packages to the public node connector release and exports it", async () => {
    const manifest = JSON.parse(readFileSync("package.json", "utf8")) as {
      devDependencies: Record<string, string>;
    };
    for (const name of ["@getpaseo/client", "@getpaseo/plugin", "@getpaseo/protocol"]) {
      expect(manifest.devDependencies[name], name).toBe("0.13.2");
    }
    const node = await import("@getpaseo/client/node") as { createHostClient?: unknown };
    expect(typeof node.createHostClient, "@getpaseo/client/node createHostClient").toBe("function");
  });
});
