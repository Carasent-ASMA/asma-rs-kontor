import type {
  DirectoryEntry,
  SessionStatus,
} from "../client/session-directory";

type SessionEntryOverrides = {
  title?: string;
  status?: SessionStatus;
  requiresAttention?: boolean;
  attentionReason?: "finished" | "error" | "permission" | null;
  archivedAt?: string | null;
  workspaceName?: string | null;
  projectName?: string | null;
  projectKey?: string | null;
  workspaceId?: string | null;
  cwd?: string;
  updatedAt?: string;
};

export function sessionEntry(
  id: string,
  overrides: SessionEntryOverrides = {},
): DirectoryEntry {
  const merged = {
    id,
    title: overrides.title ?? `Session ${id}`,
    status: overrides.status ?? "idle",
    requiresAttention: overrides.requiresAttention ?? false,
    attentionReason: overrides.attentionReason ?? null,
    attentionTimestamp: null,
    archivedAt: overrides.archivedAt ?? null,
    createdAt: "2026-10-18T09:00:00.000Z",
    updatedAt: overrides.updatedAt ?? "2026-10-18T10:00:00.000Z",
    cwd: overrides.cwd ?? "/tmp/work",
    workspaceId:
      overrides.workspaceId === undefined ? "wks_1" : overrides.workspaceId,
    provider: "opencode/deepseek/deepseek-flash",
    model: null,
    parentAgentId: null,
    labels: {},
    projectKey: overrides.projectKey ?? "prj_1",
    projectName: overrides.projectName ?? "Project",
    workspaceName: overrides.workspaceName ?? "Workspace",
  };
  return {
    ...merged,
    searchText: [
      merged.title,
      merged.workspaceName,
      merged.projectName,
      merged.projectKey,
      merged.cwd,
    ]
      .filter((part): part is string => part !== null)
      .join("\n")
      .toLowerCase(),
  };
}
