import type {
  DirectoryEntry,
  SessionStatus,
} from "../client/session-directory";

type SessionEntryOverrides = {
  title?: string;
  status?: SessionStatus;
  requiresAttention?: boolean;
  attentionReason?: "finished" | "error" | "permission" | null;
  pendingPermissionCount?: number;
  needsInput?: boolean;
  archivedAt?: string | null;
  workspaceName?: string | null;
  projectName?: string | null;
  projectKey?: string | null;
  workspaceId?: string | null;
  cwd?: string;
  updatedAt?: string;
  createdAt?: string;
  labels?: Readonly<Record<string, string>>;
};

export function sessionEntry(
  id: string,
  overrides: SessionEntryOverrides = {},
): DirectoryEntry {
  const pendingPermissionCount = overrides.pendingPermissionCount ?? 0;
  const attentionReason = overrides.attentionReason ?? null;
  const merged = {
    id,
    title: overrides.title ?? `Session ${id}`,
    status: overrides.status ?? "idle",
    requiresAttention: overrides.requiresAttention ?? false,
    attentionReason,
    attentionTimestamp: null,
    pendingPermissionCount,
    needsInput:
      overrides.needsInput ??
      (pendingPermissionCount > 0 || attentionReason === "permission"),
    archivedAt: overrides.archivedAt ?? null,
    createdAt: overrides.createdAt ?? "2026-10-18T09:00:00.000Z",
    updatedAt: overrides.updatedAt ?? "2026-10-18T10:00:00.000Z",
    cwd: overrides.cwd ?? "/tmp/work",
    workspaceId:
      overrides.workspaceId === undefined ? "wks_1" : overrides.workspaceId,
    provider: "opencode/deepseek/deepseek-flash",
    model: null,
    parentAgentId: null,
    labels: overrides.labels ?? {},
    projectKey: overrides.projectKey ?? "prj_1",
    projectName:
      overrides.projectName === undefined ? "Project" : overrides.projectName,
    workspaceName:
      overrides.workspaceName === undefined ? "Workspace" : overrides.workspaceName,
  };
  return {
    ...merged,
    searchText: [
      merged.title,
      merged.workspaceName,
      merged.projectName,
      merged.projectKey,
      merged.cwd,
      ...Object.values(merged.labels),
    ]
      .filter((part): part is string => part !== null)
      .join("\n")
      .toLowerCase(),
  };
}
