import type { PaseoAgentListOptions, PaseoAgentListResult, PaseoAgentUpdate } from "@getpaseo/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// Returning to Kontor remounts the screen while the plugin runtime lives on; these tests remount the hooks over fake hosts.
const react = vi.hoisted(() => ({
  slots: [] as Array<{ value?: unknown; deps?: unknown[]; cleanup?: (() => void) | void }>, cursor: 0, queue: [] as Array<() => void>,
  hosts: new Map<string, unknown>(), dispositions: undefined as unknown as ReturnType<typeof vi.fn>,
}));
vi.mock("react", () => {
  const slot = () => react.slots[react.cursor] ?? (react.slots[react.cursor] = {});
  return {
    useState: (initial: unknown) => { const s = slot(); react.cursor++; if (!("value" in s)) s.value = typeof initial === "function" ? initial() : initial;
      return [s.value, (next: unknown) => { s.value = typeof next === "function" ? next(s.value) : next; }]; },
    useRef: (initial: unknown) => { const s = slot(); react.cursor++; return s.value ?? (s.value = { current: initial }); },
    useMemo: (fn: () => unknown) => fn(), useCallback: (fn: unknown) => fn,
    useEffect: (fn: () => (() => void) | void, deps: unknown[]) => { const s = slot(); react.cursor++;
      if (s.deps && deps.every((d, i) => Object.is(d, s.deps![i]))) return; s.deps = deps; react.queue.push(() => { s.cleanup?.(); s.cleanup = fn(); }); },
  };
});
vi.mock("@getpaseo/plugin/client", () => ({
  getPaseoClient: (hostId: string) => react.hosts.get(hostId),
  useRpc: (rpc: { name: string }) => rpc.name === "sessions.lifecycle-dispositions" ? react.dispositions : async () => ({}),
}));
import { disposeRetainedDirectories, useSessionDirectory, type HostStatus } from "../client/use-session-directory";
import { useSessionAvailability } from "../client/use-session-availability";
import { forgetWorkspaceDirectories, useWorkspaceDirectory } from "../client/use-workspace-directory";
import type { AgentDirectoryPage } from "../client/session-directory";
import { sessionEntry } from "./session-fixtures";

type Observer = {
  snapshot: (snapshot: unknown) => void;
  update: (message: { type: "agent_update"; payload: PaseoAgentUpdate }) => void;
  error?: (error: unknown) => void;
};

function agent(id: string, archivedAt: string | null = null, title = `Session ${id}`): AgentDirectoryPage {
  return { agent: { id, title, status: "idle", provider: "fixture", cwd: "/fixture", workspaceId: null, model: null, labels: {},
    createdAt: "2026-10-22T09:00:00.000Z", updatedAt: "2026-10-22T10:00:00.000Z", archivedAt, pendingPermissions: [],
    requiresAttention: false, attentionReason: null, attentionTimestamp: null }, project: null } as unknown as AgentDirectoryPage;
}
const sessions = (count: number) => Array.from({ length: count }, (_, index) => agent(`s-${index}`));

/** A fictional host: offset cursors over `rows`, one owned subscription per subscribe call. */
function fakeHost(hostId: string, rows: AgentDirectoryPage[]) {
  const host = {
    rows, calls: [] as PaseoAgentListOptions[], observers: [] as Observer[], released: 0, gate: null as Promise<void> | null,
    workspaces: vi.fn(async () => ({ entries: [{ id: `${hostId}-wks` }], pageInfo: { hasMore: false, nextCursor: null, prevCursor: null } })),
    projects: vi.fn(async () => ({ projects: [{ projectId: `${hostId}-prj` }] })),
    get subscribes() { return host.calls.filter((call) => call.subscribe); },
    get walks() { return host.calls.filter((call) => !call.subscribe); },
    holdWalk() { let release!: () => void; host.gate = new Promise((resolve) => { release = resolve; }); return () => { host.gate = null; release(); }; },
  };
  const list = async (options: PaseoAgentListOptions = {}) => {
    host.calls.push(options);
    const visible = host.rows.filter((row) => options.filter?.includeArchived || row.agent.archivedAt === null);
    const start = Number(options.page?.cursor ?? 0), end = start + (options.page?.limit ?? 200);
    const result = { requestId: "fixture", entries: visible.slice(start, end),
      pageInfo: { hasMore: end < visible.length, nextCursor: end < visible.length ? String(end) : null, prevCursor: null } };
    if (!options.subscribe) { if (host.gate) await host.gate; return result; }
    const snapshot = { ...result, subscriptionId: `sub-${host.calls.length}` } as PaseoAgentListResult & { subscriptionId: string };
    return { ...snapshot, subscription: {
      subscriptionId: snapshot.subscriptionId, ready: Promise.resolve(snapshot),
      subscribe: (observer: Observer) => { host.observers.push(observer); observer.snapshot(snapshot); return () => undefined; },
      release: async () => { host.released += 1; },
    } };
  };
  react.hosts.set(hostId, { agents: { list }, workspaces: { list: host.workspaces }, projects: { list: host.projects } });
  return host;
}

function render<T>(draw: () => T): T { react.cursor = 0; const value = draw(); for (const run of react.queue.splice(0)) run(); return value; }
function unmount() { for (const s of react.slots) s.cleanup?.(); react.slots = []; react.queue = []; }
async function settle() { for (let n = 0; n < 50; n++) await Promise.resolve(); }
function kontor(hostId = "host-a", hostStatus: HostStatus = "online", includeArchived = false) {
  return render(() => useSessionDirectory({ hostId, hostStatus, includeArchived }));
}

beforeEach(async () => {
  unmount(); react.hosts.clear(); react.dispositions = vi.fn(async () => ({}));
  await disposeRetainedDirectories(); forgetWorkspaceDirectories();
});
afterEach(() => { unmount(); vi.useRealTimers(); });

describe("returning to Kontor reuses the host's live session directory", () => {
  it("walks once on first entry with a one-row subscription bootstrap, then returns to live rows without a read", async () => {
    const host = fakeHost("host-a", sessions(450));
    kontor(); await settle();
    expect(kontor().state).toMatchObject({ status: "ready", live: true, inventoryComplete: true });
    expect(kontor().state.entries).toHaveLength(450);
    expect(host.subscribes.map((call) => call.page)).toEqual([{ limit: 1 }]);
    expect(host.walks.map((call) => call.page)).toEqual([{ limit: 200 }, { limit: 200, cursor: "200" }, { limit: 200, cursor: "400" }]);

    unmount();
    const first = kontor();
    expect(first.state).toMatchObject({ status: "ready", live: true, inventoryComplete: true });
    expect(first.state.entries).toHaveLength(450);
    await settle();
    expect(host.calls).toHaveLength(4);
    expect(host.released).toBe(0);
  });

  it("shows native updates that arrived while Kontor was closed", async () => {
    const host = fakeHost("host-a", sessions(3));
    kontor(); await settle(); unmount();
    host.observers.at(-1)!.update({ type: "agent_update", payload: { kind: "upsert", ...agent("s-1", null, "Renamed while away") } as PaseoAgentUpdate });
    host.observers.at(-1)!.update({ type: "agent_update", payload: { kind: "remove", agentId: "s-2" } as PaseoAgentUpdate });
    const first = kontor();
    expect(first.state.entries.map((entry) => entry.title).sort()).toEqual(["Renamed while away", "Session s-0"]);
    expect(first.state.live).toBe(true);
    expect(host.calls).toHaveLength(2);
  });

  it("lets a first walk still in flight finish for the reopened screen instead of restarting it", async () => {
    const host = fakeHost("host-a", sessions(450));
    const release = host.holdWalk();
    kontor(); await settle();
    expect(kontor().state.status).toBe("loading");
    unmount();
    expect(kontor().state.status).toBe("loading");
    release(); await settle();
    expect(kontor().state).toMatchObject({ status: "ready", live: true });
    expect(host.subscribes).toHaveLength(1);
    expect(host.walks).toHaveLength(3);
  });

  it("keeps the last rows visibly stale and walks again on return when live updates had stopped", async () => {
    const host = fakeHost("host-a", sessions(3));
    kontor(); await settle(); unmount();
    host.observers.at(-1)!.error!(new Error("stream closed"));
    const first = kontor();
    expect(first.state).toMatchObject({ live: false, staleReason: expect.any(String) });
    expect(first.state.entries).toHaveLength(3);
    await settle();
    expect(kontor().state).toMatchObject({ status: "ready", live: true });
    expect(host.subscribes).toHaveLength(2);
    expect(host.walks).toHaveLength(2);
  });

  it("reconciles a host reconnect that happened while Kontor was closed", async () => {
    const host = fakeHost("host-a", sessions(3));
    kontor(); await settle(); unmount();
    host.rows = host.rows.slice(1);
    host.observers.at(-1)!.snapshot({});
    await settle();
    const first = kontor();
    expect(first.state).toMatchObject({ status: "ready", live: true });
    expect(first.state.entries.map((entry) => entry.id).sort()).toEqual(["s-1", "s-2"]);
    await settle();
    expect(host.walks).toHaveLength(2);
  });

  it("walks again on return after the host went offline", async () => {
    const host = fakeHost("host-a", sessions(3));
    kontor(); await settle(); kontor("host-a", "offline");
    expect(kontor("host-a", "offline").state).toMatchObject({ status: "stale", live: false });
    expect(host.released).toBe(1);
    unmount();
    kontor(); await settle();
    expect(kontor().state).toMatchObject({ status: "ready", live: true });
    expect(host.subscribes).toHaveLength(2);
  });

  it("never shows one host's sessions for another host", async () => {
    const a = fakeHost("host-a", sessions(3)), b = fakeHost("host-b", [agent("b-only")]);
    kontor("host-a"); await settle(); unmount();
    expect(kontor("host-b").state.entries).toEqual([]);
    await settle();
    expect(kontor("host-b").state.entries.map((entry) => entry.id)).toEqual(["b-only"]);
    expect(a.calls).toHaveLength(2);
    expect(b.calls).toHaveLength(2);
  });

  it("walks once with archived sessions when the returning scope includes them", async () => {
    const host = fakeHost("host-a", [agent("live"), agent("archived", "2026-10-21T00:00:00.000Z")]);
    kontor(); await settle(); unmount();
    kontor("host-a", "online", true); await settle();
    expect(kontor("host-a", "online", true).state.entries.map((entry) => entry.id).sort()).toEqual(["archived", "live"]);
    expect(host.subscribes.map((call) => call.filter)).toEqual([{ includeArchived: false }, { includeArchived: true }]);
    expect(host.walks).toHaveLength(2);
  });

  it("releases the retained subscription on plugin cleanup; the next entry starts cold", async () => {
    const host = fakeHost("host-a", sessions(3));
    kontor(); await settle(); unmount();
    await disposeRetainedDirectories();
    expect(host.released).toBe(1);
    expect(kontor().state.entries).toEqual([]);
    await settle();
    expect(host.walks).toHaveLength(2);
  });
});

describe("removal receipts are read per set of sessions, not per order", () => {
  it("does not re-read dispositions when a session update only reorders the list", async () => {
    const a = sessionEntry("a"), b = sessionEntry("b");
    let entries = [a, b];
    const draw = () => render(() => useSessionAvailability("host-a", entries, false));
    draw(); await settle();
    expect(react.dispositions).toHaveBeenCalledTimes(1);
    entries = [{ ...b, updatedAt: "2026-10-22T11:00:00.000Z" }, a]; draw(); await settle();
    expect(react.dispositions).toHaveBeenCalledTimes(1);
    entries = [...entries, sessionEntry("c")]; draw(); await settle();
    expect(react.dispositions).toHaveBeenCalledTimes(2);
  });
});

describe("workspace and project lists on return", () => {
  const workspaces = (hostId = "host-a", online = true) => render(() => useWorkspaceDirectory(hostId, online));
  beforeEach(() => { vi.useFakeTimers(); vi.setSystemTime(new Date("2026-10-22T12:00:00Z")); });

  it("reuses a full read under 30 s old with its own read time and keeps the poll cadence", async () => {
    const host = fakeHost("host-a", []);
    workspaces(); await settle(); unmount();
    await vi.advanceTimersByTimeAsync(10_000);
    expect(workspaces()).toMatchObject({ entries: [{ id: "host-a-wks" }], readAt: "2026-10-22T12:00:00.000Z" });
    await settle();
    expect(host.workspaces).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(20_000);
    expect(host.workspaces).toHaveBeenCalledTimes(2);
    expect(workspaces().readAt).toBe("2026-10-22T12:00:30.000Z");
  });

  it("reads at once on return when the last read is old, failed, or the host was offline", async () => {
    const host = fakeHost("host-a", []);
    workspaces(); await settle(); unmount();
    await vi.advanceTimersByTimeAsync(30_000);
    workspaces(); await settle(); unmount();
    expect(host.workspaces).toHaveBeenCalledTimes(2);

    host.projects.mockRejectedValueOnce(new Error("project feed down"));
    await vi.advanceTimersByTimeAsync(30_000);
    workspaces(); await settle(); unmount();
    workspaces(); await settle(); unmount();
    expect(host.workspaces).toHaveBeenCalledTimes(4);

    workspaces("host-a", false); unmount();
    workspaces(); await settle();
    expect(host.workspaces).toHaveBeenCalledTimes(5);
  });

  it("does not keep another host's workspaces after a host switch", async () => {
    fakeHost("host-a", []); const b = fakeHost("host-b", []);
    b.workspaces.mockReturnValue(new Promise(() => {}));
    workspaces("host-a"); await settle(); workspaces("host-b");
    expect(workspaces("host-b")).toMatchObject({ entries: [], projects: [], readAt: null });
  });
});
