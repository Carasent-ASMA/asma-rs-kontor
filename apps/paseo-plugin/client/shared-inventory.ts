import { useCallback, useMemo } from "react";
import type { WorkCatalogNativeRead, WorkCatalogNativeView, WorkCatalogRead } from "../shared/work-catalog";
import {
  ATTENTION_REASONS,
  SESSION_STATUSES,
  deriveNeedsInput,
  searchTextOf,
  sortDirectoryEntries,
  type AttentionReason,
  type DirectoryEntry,
  type DirectoryState,
  type SessionStatus,
} from "./session-directory";
import { useSessionDirectory, type HostStatus } from "./use-session-directory";
import { useWorkspaceDirectory, type DirectWorkspaceDirectory } from "./use-workspace-directory";
import { useWorkCatalog, type WorkCatalogState } from "./use-work-catalog";

export type SharedWorkspace = {
  readonly id: string;
  readonly projectId: string;
  readonly name: string;
  readonly title: string | null;
  readonly projectCustomName: string | null;
  readonly projectDisplayName: string;
  readonly projectRootPath: string;
};

export type SharedProject = {
  readonly projectId: string;
  readonly projectDisplayName: string;
  readonly projectCustomName: string | null;
  readonly projectRootPath: string;
};

export type SharedWorkspaceDirectory = {
  readonly entries: readonly SharedWorkspace[];
  readonly projects: readonly SharedProject[];
  readonly error: string | null;
  readonly readAt: string | null;
  readonly projectError: string | null;
  readonly projectsReadAt: string | null;
  readonly source: "catalog" | "catalog-snapshot" | "direct";
  readonly partial: boolean;
};

export type SharedInventory = {
  readonly directory: DirectoryState;
  readonly workspaces: SharedWorkspaceDirectory;
};

export type SharedInventoryState = {
  readonly directory: { readonly state: DirectoryState; readonly refresh: () => void; readonly retry?: () => void };
  readonly workspaces: SharedWorkspaceDirectory;
  readonly catalog: WorkCatalogRead | null;
};

type CatalogView = {
  readonly read: WorkCatalogNativeRead;
  readonly view: WorkCatalogNativeView;
};

const knownStatus = (value: string): value is SessionStatus => (SESSION_STATUSES as readonly string[]).includes(value);
const knownAttention = (value: string | null): AttentionReason | null =>
  value !== null && (ATTENTION_REASONS as readonly string[]).includes(value) ? value as AttentionReason : null;

function timeOf(value: string | null): number | null {
  if (value === null) return null;
  const parsed = Date.parse(value);
  return Number.isNaN(parsed) ? null : parsed;
}

function usableCatalogView(read: WorkCatalogNativeRead | null): CatalogView | null {
  if (read === null || read.view === null || read.mode === "unavailable" || read.runtimeInstance === null) return null;
  const view = read.view;
  if (view.runtimeInstance !== read.runtimeInstance) return null;
  const sessionIds = new Set<string>();
  for (const session of view.sessions) {
    if (!knownStatus(session.status)) return null;
    if (sessionIds.has(session.sessionId)) return null;
    sessionIds.add(session.sessionId);
  }
  return { read, view };
}

function catalogComplete(view: WorkCatalogNativeView, includeArchived: boolean): boolean {
  return timeOf(view.asOf) !== null && !view.truncated && (!includeArchived || view.archiveScope);
}

function catalogPartialReason(view: WorkCatalogNativeView, includeArchived: boolean): string {
  if (view.truncated) return "The shared catalog inventory is bounded and may omit sessions; the direct directory is unavailable.";
  if (includeArchived && !view.archiveScope) return "The shared catalog did not collect archived sessions; the archived view is partial.";
  if (timeOf(view.asOf) === null) return "The shared catalog inventory has no completion instant; this read is partial.";
  return "The shared catalog inventory is partial.";
}

function catalogEntry(
  session: WorkCatalogNativeView["sessions"][number],
  view: WorkCatalogNativeView,
  live: DirectoryEntry | undefined,
): DirectoryEntry {
  const project = view.projects.find((value) => value.projectId === session.projectId);
  const workspace = view.workspaces.find((value) => value.workspaceId === session.workspaceId);
  const attentionReason = knownAttention(session.attentionReason);
  const title = session.title?.trim() || live?.title || "Untitled session";
  const projectName = session.projectName ?? project?.displayName ?? project?.customName ?? live?.projectName ?? null;
  const workspaceName = session.workspaceName ?? workspace?.title ?? workspace?.name ?? live?.workspaceName ?? null;
  const projectKey = session.projectId ?? live?.projectKey ?? null;
  const cwd = session.cwd || live?.cwd || "";
  const labels = live?.labels ?? {};
  const pendingPermissionCount = live?.pendingPermissionCount ?? 0;
  return {
    id: session.sessionId,
    title,
    status: session.status as SessionStatus,
    requiresAttention: live?.requiresAttention ?? attentionReason !== null,
    attentionReason: live?.attentionReason ?? attentionReason,
    attentionTimestamp: live?.attentionTimestamp ?? null,
    pendingPermissionCount,
    needsInput: live ? deriveNeedsInput({ pendingPermissionCount, attentionReason: live.attentionReason }) : attentionReason === "permission",
    archivedAt: session.archivedAt,
    createdAt: session.createdAt ?? live?.createdAt ?? "",
    updatedAt: session.updatedAt ?? live?.updatedAt ?? "",
    lastUserMessageAt: session.lastUserMessageAt ?? live?.lastUserMessageAt ?? null,
    activeTurnStartedAt: live?.activeTurnStartedAt ?? null,
    cwd,
    workspaceId: session.workspaceId ?? live?.workspaceId ?? null,
    provider: session.provider || live?.provider || "unknown",
    model: session.model ?? live?.model ?? null,
    telemetry: live?.telemetry,
    parentAgentId: session.parentSessionId ?? live?.parentAgentId ?? null,
    labels,
    projectKey,
    projectName,
    workspaceName,
    searchText: searchTextOf({ title, workspaceName, projectName, projectKey, cwd, labels,
      usage: { provider: session.provider || live?.provider || "unknown", model: session.model ?? live?.model ?? null, telemetry: live?.telemetry } }),
    availability: live?.availability,
    jira: live?.jira,
  };
}

function catalogDirectory(
  view: WorkCatalogNativeView,
  direct: DirectoryState,
  includeArchived: boolean,
  partial: boolean,
  warning: string | null,
): DirectoryState {
  // The catalog and direct rows describe different instants. Compare per id:
  // a newer catalog row replaces a frozen direct row, while a newer direct row
  // keeps the re-walk window from rolling a session back to sweep time. A
  // complete catalog newer than the direct walk may also drop a direct-only row
  // that the catalog no longer lists, which is how a deleted ghost disappears.
  const directRows = new Map(direct.entries.map((entry) => [entry.id, entry]));
  const catalogSessions = view.sessions.filter((session) => includeArchived || session.archivedAt === null);
  const catalogRows = new Map(catalogSessions.map((session) => [session.sessionId, session]));
  const asOf = timeOf(view.asOf);
  const directWalkedAt = direct.lastUpdatedAt;
  const catalogCoversDirect =
    !partial
    && asOf !== null
    && directWalkedAt !== null
    && asOf > directWalkedAt;
  const entries: DirectoryEntry[] = [];
  let keptDirect = false;
  let catalogFilled = false;
  for (const row of direct.entries) {
    const catalog = catalogRows.get(row.id);
    if (catalog) {
      const catalogAt = timeOf(catalog.updatedAt);
      const directAt = timeOf(row.updatedAt);
      if (catalogAt !== null && (directAt === null || catalogAt > directAt)) {
        entries.push(catalogEntry(catalog, view, undefined));
        catalogFilled = true;
      } else {
        entries.push(row);
        keptDirect = true;
      }
      continue;
    }
    if (catalogCoversDirect) continue;
    entries.push(row);
    keptDirect = true;
  }
  for (const session of catalogSessions) {
    if (directRows.has(session.sessionId)) continue;
    entries.push(catalogEntry(session, view, undefined));
    catalogFilled = true;
  }
  const directStale = keptDirect ? direct.staleReason : null;
  return {
    entries: sortDirectoryEntries(entries),
    status: partial || warning !== null || directStale !== null ? "stale" : "ready",
    live: false,
    error: null,
    staleReason: partial ? catalogPartialReason(view, includeArchived) : warning ?? directStale,
    pages: 1,
    lastUpdatedAt: asOf,
    inventoryComplete: !partial,
    includeArchived,
    source: warning !== null && view.source === "snapshot" ? "catalog-snapshot" : "catalog",
    sourceAsOf: view.asOf,
    labelsComplete: !catalogFilled,
    catalogGeneration: view.generation,
    catalogRuntimeInstance: view.runtimeInstance,
  };
}

function snapshotDirectory(view: WorkCatalogNativeView, direct: DirectoryState, includeArchived: boolean): DirectoryState {
  const partial = !catalogComplete(view, includeArchived);
  const state = catalogDirectory(view, direct, includeArchived, partial, "Shared catalog last-known native snapshot; the live native catalog and Paseo directory are unavailable.");
  return { ...state, source: "catalog-snapshot", inventoryComplete: !partial };
}

function directDirectory(direct: DirectoryState): DirectoryState {
  return { ...direct, source: "direct", sourceAsOf: null, labelsComplete: true, catalogGeneration: null, catalogRuntimeInstance: null };
}

function unavailableDirectory(direct: DirectoryState): DirectoryState {
  return { ...direct, source: "unavailable", sourceAsOf: null, labelsComplete: false, catalogGeneration: null, catalogRuntimeInstance: null };
}

function directWorkspaces(direct: DirectWorkspaceDirectory): SharedWorkspaceDirectory {
  const partial = direct.error !== null || direct.projectError !== null || direct.readAt === null || direct.projectsReadAt === null;
  return {
    entries: (direct.entries ?? []).map((workspace) => ({
      id: workspace.id,
      projectId: workspace.projectId,
      name: workspace.name,
      title: workspace.title ?? null,
      projectCustomName: workspace.projectCustomName ?? null,
      projectDisplayName: workspace.projectDisplayName,
      projectRootPath: workspace.projectRootPath ?? "",
    })),
    projects: (direct.projects ?? []).map((project) => ({
      projectId: project.projectId,
      projectDisplayName: project.projectDisplayName,
      projectCustomName: project.projectCustomName ?? null,
      projectRootPath: project.projectRootPath,
    })),
    error: direct.error,
    readAt: direct.readAt,
    projectError: direct.projectError,
    projectsReadAt: direct.projectsReadAt,
    source: "direct",
    partial,
  };
}

function catalogWorkspaces(view: WorkCatalogNativeView, partial: boolean, source: "catalog" | "catalog-snapshot" = "catalog"): SharedWorkspaceDirectory {
  const projectById = new Map(view.projects.map((project) => [project.projectId, project]));
  return {
    entries: view.workspaces.map((workspace) => {
      const project = projectById.get(workspace.projectId);
      return {
        id: workspace.workspaceId,
        projectId: workspace.projectId,
        name: workspace.name,
        title: workspace.title,
        projectCustomName: workspace.projectCustomName ?? project?.customName ?? null,
        projectDisplayName: workspace.projectDisplayName ?? project?.displayName ?? "",
        projectRootPath: workspace.projectRootPath ?? project?.rootPath ?? "",
      };
    }),
    projects: view.projects.map((project) => ({
      projectId: project.projectId,
      projectDisplayName: project.displayName,
      projectCustomName: project.customName,
      projectRootPath: project.rootPath,
    })),
    error: partial ? "Shared catalog workspace inventory is partial." : null,
    readAt: view.asOf,
    projectError: partial ? "Shared catalog project inventory is partial." : null,
    projectsReadAt: view.asOf,
    source,
    partial,
  };
}

function directIsUsable(direct: DirectoryState): boolean {
  // `inventoryComplete` is optional on older readers; a ready direct read is
  // usable, while a false value still renders its partial marker.
  return direct.entries.length > 0 || direct.status === "ready";
}

function directWorkspacesUsable(direct: DirectWorkspaceDirectory): boolean {
  return direct.entries.length > 0 || (direct.readAt !== null && direct.error === null);
}

/**
 * One adapter for the existing Sessions, project, workspace and native-action
 * views. Catalog inventory is preferred when complete; a usable direct read is
 * next; a partial catalog is shown only with a visible partial marker; the
 * last-known snapshot is shown only when neither live source can answer.
 */
export function adaptSharedInventory(input: {
  readonly direct: DirectoryState;
  readonly directWorkspaces: DirectWorkspaceDirectory;
  readonly catalog: WorkCatalogRead | null;
  readonly includeArchived: boolean;
}): SharedInventory {
  const usable = usableCatalogView(input.catalog?.native ?? null);
  // `mode: "catalog"` is not enough: the daemon can answer from its own
  // last-known native snapshot. Only a database-backed view is a complete live
  // catalog; a daemon snapshot follows the same direct-first rule as the
  // plugin's own snapshot.
  const live = usable !== null && usable.read.mode === "catalog" && usable.view.source === "database" ? usable : null;
  const snapshot = usable !== null && (usable.read.mode === "snapshot" || usable.view.source === "snapshot") ? usable : null;
  const liveComplete = live !== null && catalogComplete(live.view, input.includeArchived);
  const directUsable = directIsUsable(input.direct);
  // A ready, live, complete direct read is the authoritative answer. The
  // catalog is an observational fallback, not a replacement for a source that
  // already answered exactly; otherwise a stale catalog could shadow a fresh
  // direct inventory.
  const directAuthoritative =
    input.direct.status === "ready"
    && input.direct.live
    && input.direct.inventoryComplete === true
    && input.direct.staleReason === null;
  const directWorkspacesAuthoritative =
    directAuthoritative
    || (((input.directWorkspaces.entries?.length ?? 0) > 0
      || (input.directWorkspaces.projects?.length ?? 0) > 0)
      && input.directWorkspaces.error === null
      && input.directWorkspaces.projectError === null);

  const directory =
    directAuthoritative
      ? directDirectory(input.direct)
      : liveComplete && live !== null
        ? catalogDirectory(live.view, input.direct, input.includeArchived, false, null)
        : directUsable
          ? directDirectory(input.direct)
          : live !== null
            ? catalogDirectory(live.view, input.direct, input.includeArchived, true, null)
            : snapshot !== null
              ? snapshotDirectory(snapshot.view, input.direct, input.includeArchived)
              : unavailableDirectory(input.direct);

  const workspaces =
    directWorkspacesAuthoritative
      ? directWorkspaces(input.directWorkspaces)
      : liveComplete && live !== null
        ? catalogWorkspaces(live.view, false)
        : directWorkspacesUsable(input.directWorkspaces)
          ? directWorkspaces(input.directWorkspaces)
          : live !== null
            ? catalogWorkspaces(live.view, true)
            : snapshot !== null
              ? catalogWorkspaces(snapshot.view, !catalogComplete(snapshot.view, input.includeArchived), "catalog-snapshot")
              : directWorkspaces(input.directWorkspaces);

  return { directory, workspaces };
}

/** One hook at the composition point; no view keeps its own inventory adapter. */
export function useSharedInventory(input: {
  readonly hostId: string;
  readonly hostStatus: HostStatus;
  readonly includeArchived: boolean;
}): SharedInventoryState {
  const direct = useSessionDirectory(input);
  const workspaces = useWorkspaceDirectory(input.hostId, input.hostStatus === "online");
  const catalog: WorkCatalogState = useWorkCatalog(input.hostId, input.hostStatus);
  const adapted = useMemo(() => adaptSharedInventory({
    direct: direct.state,
    directWorkspaces: workspaces,
    catalog: catalog.read,
    includeArchived: input.includeArchived,
  }), [direct.state, workspaces, catalog.read, input.includeArchived]);
  const refresh = useCallback(() => {
    direct.refresh();
    catalog.refresh();
  }, [direct.refresh, catalog.refresh]);
  return { directory: { state: adapted.directory, refresh, retry: direct.retry }, workspaces: adapted.workspaces, catalog: catalog.read };
}
