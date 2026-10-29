import { describe, expect, it, vi } from "vitest";
import type { PaseoAgentUpdate } from "@getpaseo/client";
import { adaptSharedInventory } from "../client/shared-inventory";
import {
  emptyDirectoryState,
  SessionDirectory,
  type AgentDirectoryPage,
  type DirectoryClient,
  type DirectoryState,
} from "../client/session-directory";
import type { WorkCatalogNativeRead, WorkCatalogNativeView, WorkCatalogRead } from "../shared/work-catalog";

const AS_OF = "2026-10-25T06:14:01Z";
const HOST_ID = "srv_host_a";
const RUNTIME = "paseo:ws://127.0.0.1:6767/ws";

function nativeSession(id: string, overrides: Record<string, unknown> = {}) {
  return {
    sessionId: id, workspaceId: "workspace-1", projectId: "project-1",
    projectName: "Project One", workspaceName: "Workspace One", title: `Session ${id}`,
    provider: "claude", model: "sonnet", status: "idle", archivedAt: null, attentionReason: null,
    cwd: "/work", createdAt: AS_OF, updatedAt: AS_OF, lastUserMessageAt: null,
    parentSessionId: null, checkout: null, ...overrides,
  };
}

const nativeView = (overrides: Partial<WorkCatalogNativeView> = {}): WorkCatalogNativeView => ({
  runtimeInstance: RUNTIME, generation: 4, asOf: AS_OF, source: "database", archiveScope: true, truncated: false,
  projects: [], workspaces: [], sessions: [nativeSession("session-a"), nativeSession("session-x")], ...overrides,
});

const catalog = (view: WorkCatalogNativeView): WorkCatalogRead => ({
  mode: "catalog", checkedAt: "2026-10-25T09:00:00Z", sourceAsOf: view.asOf, view: null, error: null, snapshot: null,
  native: { mode: "catalog", hostId: HOST_ID, runtimeInstance: RUNTIME, checkedAt: "2026-10-25T09:00:00Z", view, error: null, snapshotSavedAt: null } as WorkCatalogNativeRead,
});

const adapt = (direct: DirectoryState, view: WorkCatalogNativeView) => adaptSharedInventory({
  direct,
  directWorkspaces: { entries: [], error: null, readAt: AS_OF, projects: [], projectError: null, projectsReadAt: AS_OF },
  catalog: catalog(view),
  includeArchived: false,
}).directory;

function agent(id: string, at: string, overrides: Record<string, unknown> = {}): AgentDirectoryPage {
  return {
    agent: {
      id, provider: "claude", cwd: "/tmp/work", workspaceId: "wks_1", model: null,
      createdAt: at, updatedAt: at, lastUserMessageAt: null, status: "idle",
      capabilities: {}, currentModeId: null, availableModes: [], pendingPermissions: [], persistence: null,
      title: `Session ${id}`, labels: {}, requiresAttention: false, attentionReason: null, attentionTimestamp: null,
      archivedAt: null, ...overrides,
    },
    project: { projectKey: "prj_1", projectName: "Project", workspaceName: "Workspace", checkout: { kind: "not_git", projectRootPath: "/tmp/work" } },
  } as unknown as AgentDirectoryPage;
}

function fakeHost(initial: AgentDirectoryPage[]) {
  let rows = initial;
  let observer: { update: (message: { type: "agent_update"; payload: PaseoAgentUpdate }) => void; error?: (error: unknown) => void } | null = null;
  const done = { hasMore: false, nextCursor: null, prevCursor: null };
  const list = async (options: { filter?: { includeArchived?: boolean }; page?: { limit?: number }; subscribe?: boolean } = {}) => {
    const includeArchived = options.filter?.includeArchived === true;
    const visible = rows.filter((row) => includeArchived || row.agent.archivedAt === null);
    if (options.subscribe) {
      const snapshot = { requestId: "s", subscriptionId: "sub", entries: visible.slice(0, options.page?.limit ?? 200), pageInfo: done };
      return { ...snapshot, subscription: { subscriptionId: "sub", ready: Promise.resolve(snapshot),
        subscribe: (next: any) => { observer = next; next.snapshot(snapshot); return () => { if (observer === next) observer = null; }; },
        release: async () => undefined } };
    }
    return { requestId: "p", entries: visible, pageInfo: done };
  };
  return {
    client: { agents: { list } } as unknown as DirectoryClient,
    setRows: (next: AgentDirectoryPage[]) => { rows = next; },
    emit: (update: PaseoAgentUpdate) => observer?.update({ type: "agent_update", payload: update }),
    fail: (error: unknown) => observer?.error?.(error),
  };
}

async function liveDirectory(rows: AgentDirectoryPage[]) {
  const host = fakeHost(rows);
  let latest = emptyDirectoryState();
  const directory = new SessionDirectory({ client: () => host.client, includeArchived: false, onChange: (state) => { latest = state; } });
  await directory.start();
  await vi.waitFor(() => { if (!(latest.status === "ready" && latest.live)) throw new Error("not ready"); });
  return { host, directory, state: () => latest };
}

describe("R5-F1 host-stamped archive event", () => {
  it("advances directKnownThrough and keeps an older catalog from reviving the archived session", async () => {
    const { host, directory, state } = await liveDirectory([
      agent("session-a", "2026-10-25T06:10:00Z"),
      agent("session-x", "2026-10-25T06:00:00Z"),
    ]);
    host.emit({
      kind: "upsert",
      agent: agent("session-x", "2026-10-25T06:20:00Z", { archivedAt: "2026-10-25T06:20:00Z" }).agent,
    } as PaseoAgentUpdate);
    expect(state().directKnownThrough).toBe(Date.parse("2026-10-25T06:20:00Z"));
    expect(state().entries.map((entry) => entry.id)).toEqual(["session-a"]);

    host.fail(new Error("subscription stopped"));
    const merged = adapt(state(), nativeView({
      sessions: [
        nativeSession("session-a", { updatedAt: "2026-10-25T06:10:00Z" }),
        nativeSession("session-x", { updatedAt: "2026-10-25T06:00:00Z" }),
      ],
    }));
    expect(merged.entries.map((entry) => entry.id)).not.toContain("session-x");
    expect(merged.entries.find((entry) => entry.id === "session-x")?.catalogCopyAt).toBeUndefined();
    await directory.dispose();
  });
});
