import { describe, expect, it, vi } from "vitest";
import type { PaseoWorkspace } from "@getpaseo/client";
import { declaredScope } from "../client/session-organization";
import { openSessionById } from "../client/session-actions";
import { adaptSharedInventory } from "../client/shared-inventory";
import { emptyDirectoryState, type DirectoryState } from "../client/session-directory";
import { sessionEntry } from "./session-fixtures";
import type { DirectWorkspaceDirectory } from "../client/use-workspace-directory";
import type {
  WorkCatalogNativeRead,
  WorkCatalogNativeView,
  WorkCatalogRead,
} from "../shared/work-catalog";

const AS_OF = "2026-10-25T06:14:01Z";
const HOST_ID = "srv_host_a";
const RUNTIME = "paseo:ws://127.0.0.1:6767/ws";

const nativeView = (overrides: Partial<WorkCatalogNativeView> = {}): WorkCatalogNativeView => ({
  runtimeInstance: RUNTIME,
  generation: 4,
  asOf: AS_OF,
  source: "database",
  archiveScope: true,
  truncated: false,
  projects: [{
    projectId: "project-1", displayName: "Project One", customName: null,
    projectKey: "project-1", rootPath: "/work", projectKind: "git",
  }],
  workspaces: [{
    workspaceId: "workspace-1", projectId: "project-1", name: "Workspace One", title: null,
    directory: "/work", kind: "checkout", archivingAt: null, worktreeSlug: null,
    projectDisplayName: "Project One", projectCustomName: null, projectRootPath: "/work", projectKind: "git",
  }],
  sessions: [nativeSession("session-a"), nativeSession("session-b")],
  ...overrides,
});

function nativeSession(id: string, overrides: Record<string, unknown> = {}) {
  return {
    sessionId: id, workspaceId: "workspace-1", projectId: "project-1",
    projectName: "Project One", workspaceName: "Workspace One", title: `Session ${id}`,
    provider: "claude", model: "sonnet", status: "idle", archivedAt: null, attentionReason: null,
    cwd: "/work", createdAt: AS_OF, updatedAt: AS_OF, lastUserMessageAt: null,
    parentSessionId: null, checkout: null,
    ...overrides,
  };
}

const native = (overrides: Partial<WorkCatalogNativeRead> = {}): WorkCatalogNativeRead => ({
  mode: "catalog",
  hostId: HOST_ID,
  runtimeInstance: RUNTIME,
  checkedAt: "2026-10-25T09:00:00Z",
  view: nativeView(),
  error: null,
  snapshotSavedAt: null,
  ...overrides,
});

const catalog = (nativeRead: WorkCatalogNativeRead): WorkCatalogRead => ({
  mode: "catalog",
  checkedAt: "2026-10-25T09:00:00Z",
  sourceAsOf: AS_OF,
  view: null,
  error: null,
  snapshot: null,
  native: nativeRead,
});

const direct = (overrides: Partial<DirectoryState> = {}): DirectoryState => ({
  ...emptyDirectoryState(false),
  status: "error",
  error: "Paseo is unavailable",
  ...overrides,
});

const directWorkspaces = (overrides: Partial<DirectWorkspaceDirectory> = {}): DirectWorkspaceDirectory => ({
  entries: [],
  error: null,
  readAt: AS_OF,
  projects: [],
  projectError: null,
  projectsReadAt: AS_OF,
  ...overrides,
});

const adapt = (input: {
  direct?: Partial<DirectoryState>;
  workspaces?: Partial<DirectWorkspaceDirectory>;
  catalog?: WorkCatalogRead | null;
  includeArchived?: boolean;
} = {}) => adaptSharedInventory({
  direct: direct(input.direct),
  directWorkspaces: directWorkspaces(input.workspaces),
  catalog: input.catalog ?? null,
  includeArchived: input.includeArchived ?? false,
});

describe("the shared native inventory adapter", () => {
  it("renders catalog rows when direct enumeration fails, with exact native ids", () => {
    const adapted = adapt({ catalog: catalog(native()) });
    expect(adapted.directory.source).toBe("catalog");
    expect(adapted.directory.status).toBe("ready");
    expect(adapted.directory.inventoryComplete).toBe(true);
    expect(adapted.directory.lastUpdatedAt).toBe(Date.parse(AS_OF));
    expect(adapted.directory.staleReason).toBeNull();
    expect(adapted.directory.entries.map((entry) => entry.id)).toEqual(["session-a", "session-b"]);
    expect(adapted.directory.entries[0]).toMatchObject({
      title: "Session session-a", projectKey: "project-1", projectName: "Project One",
      workspaceName: "Workspace One", cwd: "/work", provider: "claude", model: "sonnet",
    });
    expect(adapted.workspaces.source).toBe("catalog");
    expect(adapted.workspaces.entries.map((workspace) => workspace.id)).toEqual(["workspace-1"]);
    expect(adapted.workspaces.projects.map((project) => project.projectId)).toEqual(["project-1"]);
  });

  it("does not let the daemon's native snapshot override a usable direct directory", () => {
    const live = sessionEntry("session-direct", { labels: { epic: "ASMA-1" } });
    const daemonSnapshot = catalog(native({ view: nativeView({ source: "snapshot" }) }));
    const adapted = adapt({
      direct: { status: "ready", live: true, entries: [live], inventoryComplete: true, lastUpdatedAt: Date.parse(AS_OF) },
      catalog: daemonSnapshot,
    });
    expect(adapted.directory.source).toBe("direct");
    expect(adapted.directory.entries).toEqual([live]);
    expect(adapted.workspaces.source).toBe("direct");
  });

  it("uses the daemon's native snapshot only when both live sources cannot answer", () => {
    const adapted = adapt({
      workspaces: { readAt: null, projectsReadAt: null, error: "Paseo is unavailable", projectError: "Paseo is unavailable" },
      catalog: catalog(native({ view: nativeView({ source: "snapshot" }) })),
    });
    expect(adapted.directory.source).toBe("catalog-snapshot");
    expect(adapted.directory.status).toBe("stale");
    expect(adapted.directory.entries.map((entry) => entry.id)).toEqual(["session-a", "session-b"]);
    expect(adapted.directory.staleReason).toContain("last-known native snapshot");
    expect(adapted.directory.inventoryComplete).toBe(true);
    expect(adapted.workspaces.source).toBe("catalog-snapshot");
  });

  it("restores the direct directory when the catalog is unavailable", () => {
    const live = sessionEntry("session-direct", { labels: { epic: "ASMA-1" } });
    const adapted = adapt({
      direct: { status: "ready", live: true, entries: [live], inventoryComplete: true, lastUpdatedAt: Date.parse(AS_OF) },
      catalog: catalog(native({ mode: "unavailable", view: null, runtimeInstance: null, error: "no exact runtime" })),
    });
    expect(adapted.directory.source).toBe("direct");
    expect(adapted.directory.live).toBe(true);
    expect(adapted.directory.entries).toEqual([live]);
    expect(adapted.workspaces.source).toBe("direct");
  });

  it("retains a labelled last-known snapshot when both live sources fail", () => {
    const adapted = adapt({
      workspaces: { readAt: null, projectsReadAt: null, error: "Paseo is unavailable", projectError: "Paseo is unavailable" },
      catalog: catalog(native({ mode: "snapshot", snapshotSavedAt: "2026-10-25T08:00:00Z" })),
    });
    expect(adapted.directory.source).toBe("catalog-snapshot");
    expect(adapted.directory.status).toBe("stale");
    expect(adapted.directory.entries.map((entry) => entry.id)).toEqual(["session-a", "session-b"]);
    expect(adapted.directory.staleReason).toContain("last-known native snapshot");
    expect(adapted.directory.inventoryComplete).toBe(true);
    expect(adapted.workspaces.entries.map((workspace) => workspace.id)).toEqual(["workspace-1"]);
  });

  it("distinguishes an exact empty inventory from an unavailable or partial one", () => {
    const empty = adapt({
      direct: { status: "ready", live: true, entries: [], inventoryComplete: true, lastUpdatedAt: Date.parse(AS_OF) },
      catalog: catalog(native({ view: nativeView({ projects: [], workspaces: [], sessions: [] }) })),
    });
    expect(empty.directory.source).toBe("catalog");
    expect(empty.directory.status).toBe("ready");
    expect(empty.directory.entries).toEqual([]);
    expect(empty.directory.inventoryComplete).toBe(true);

    const unavailable = adapt();
    expect(unavailable.directory.source).toBe("unavailable");
    expect(unavailable.directory.entries).toEqual([]);
    expect(unavailable.directory.inventoryComplete).not.toBe(true);
    expect(unavailable.directory.status).toBe("error");

    const partial = adapt({ catalog: catalog(native({ view: nativeView({ truncated: true }) })) });
    expect(partial.directory.source).toBe("catalog");
    expect(partial.directory.status).toBe("stale");
    expect(partial.directory.entries).not.toEqual([]);
    expect(partial.directory.inventoryComplete).toBe(false);
    expect(partial.directory.staleReason).toContain("bounded");
  });

  it("honours archive coverage and says when the archived view is partial", () => {
    const view = nativeView({
      sessions: [nativeSession("active"), nativeSession("archived", { archivedAt: AS_OF })],
    });
    const active = adapt({ catalog: catalog(native({ view })) });
    expect(active.directory.entries.map((entry) => entry.id)).toEqual(["active"]);

    const both = adapt({ catalog: catalog(native({ view })), includeArchived: true });
    expect(both.directory.entries.map((entry) => entry.id)).toEqual(["active", "archived"]);

    const partial = adapt({
      catalog: catalog(native({ view: nativeView({ archiveScope: false }) })),
      includeArchived: true,
    });
    expect(partial.directory.inventoryComplete).toBe(false);
    expect(partial.directory.status).toBe("stale");
    expect(partial.directory.staleReason).toContain("archived sessions");

    const activeOnly = adapt({
      catalog: catalog(native({ view: nativeView({ archiveScope: false }) })),
      includeArchived: false,
    });
    expect(activeOnly.directory.inventoryComplete).toBe(true);
    expect(activeOnly.directory.status).toBe("ready");
  });

  it("keeps duplicate titles as distinct rows and enriches only the exact session id", () => {
    const view = nativeView({
      sessions: [nativeSession("session-a", { title: "Same title" }), nativeSession("session-b", { title: "Same title" })],
    });
    const renamed = sessionEntry("session-a", {
      title: "Live title", labels: { epic: "ASMA-1" }, pendingPermissionCount: 2, attentionReason: "permission",
    });
    const wrongTitle = sessionEntry("session-x", { title: "Same title", labels: { epic: "ASMA-9" } });
    const adapted = adapt({
      direct: { status: "stale", entries: [renamed, wrongTitle], inventoryComplete: false, lastUpdatedAt: Date.parse(AS_OF) },
      catalog: catalog(native({ view })),
    });
    const a = adapted.directory.entries.find((entry) => entry.id === "session-a")!;
    const b = adapted.directory.entries.find((entry) => entry.id === "session-b")!;
    expect(adapted.directory.entries).toHaveLength(2);
    expect(a.title).toBe("Same title");
    expect(a.labels).toEqual({ epic: "ASMA-1" });
    expect(a.pendingPermissionCount).toBe(2);
    expect(a.needsInput).toBe(true);
    expect(b.labels).toEqual({});
    expect(b.needsInput).toBe(false);
    expect(a.id).not.toBe(b.id);
  });

  it("keeps manual associations on the exact id rather than the title", () => {
    const adapted = adapt({ catalog: catalog(native()) });
    const a = adapted.directory.entries.find((entry) => entry.id === "session-a")!;
    const b = adapted.directory.entries.find((entry) => entry.id === "session-b")!;
    const mappings = { "session:session-a": { workstream: "kontor" } } as Parameters<typeof declaredScope>[1];
    expect(declaredScope(a, mappings).workstream).toBe("kontor");
    expect(declaredScope(b, mappings).workstream).toBeNull();
  });

  it("opens a catalog row through the native Paseo host, not kontord", () => {
    const adapted = adapt({ catalog: catalog(native()) });
    const navigation = { openAgent: vi.fn(), openWorkspace: vi.fn() };
    const result = openSessionById({ entries: adapted.directory.entries, id: "session-a", serverId: HOST_ID, navigation });
    expect(result.ok).toBe(true);
    expect(navigation.openAgent).toHaveBeenCalledExactlyOnceWith({ agentId: "session-a", serverId: HOST_ID });
  });

  it("uses catalog workspaces for selectors only when the direct workspace read cannot answer", () => {
    const fromCatalog = adapt({ catalog: catalog(native()) });
    expect(fromCatalog.workspaces.source).toBe("catalog");
    expect(fromCatalog.workspaces.entries[0]).toMatchObject({ id: "workspace-1", projectId: "project-1" });

    const workspace = {
      id: "workspace-direct", projectId: "project-direct", name: "Direct", title: null,
      projectDisplayName: "Direct Project", projectCustomName: null, projectRootPath: "/direct",
    } as unknown as PaseoWorkspace;
    const fromDirect = adapt({
      workspaces: { entries: [workspace], projects: [{ projectId: "project-direct", projectDisplayName: "Direct Project", projectRootPath: "/direct", projectKind: "git" }] },
      catalog: catalog(native({ mode: "unavailable", view: null, runtimeInstance: null, error: "no exact runtime" })),
    });
    expect(fromDirect.workspaces.source).toBe("direct");
    expect(fromDirect.workspaces.entries[0]?.id).toBe("workspace-direct");

    const partialCatalog = adapt({
      workspaces: { entries: [workspace] },
      catalog: catalog(native({ view: nativeView({ truncated: true }) })),
    });
    expect(partialCatalog.workspaces.source).toBe("direct");
  });
});
