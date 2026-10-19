import { resolve } from "node:path";
import { bounded, type LabelReadOptions } from "./agent-labels";
import type { AgentLabels } from "./labels";
import type { PlacementBinding } from "./placement";

export const KONTOR_WORKSPACE_ID_LABEL = "kontor.workspace_id";
export const KONTOR_WORKTREE_LABEL = "kontor.worktree";

interface WorkspaceSnapshotLike {
  readonly projectId?: string | null;
  readonly workspaceDirectory?: string | null;
}

interface WorkspaceHandleLike {
  current(): WorkspaceSnapshotLike | null;
  refresh(): Promise<WorkspaceSnapshotLike | null>;
}

interface WorkspaceApiLike {
  ref(workspaceId: string): WorkspaceHandleLike;
}

export interface BindingPaseoLike {
  readonly workspaces: WorkspaceApiLike;
}

/**
 * Resolve one seat's permitted placement from its binding, read-only.
 *
 * `kontor.worktree` is the canonical directory and `kontor.workspace_id` names
 * the workspace the seat runs in; a Paseo read of that workspace supplies the
 * native project and proves the directory still matches the label. Every
 * failure returns `null`, and a labelled seat with an unresolved binding is
 * refused rather than failed open. The plugin holds no Kontor credential and
 * performs no write.
 */
export async function readPlacementBinding(
  paseo: BindingPaseoLike,
  labels: AgentLabels,
  options: LabelReadOptions,
): Promise<PlacementBinding | null> {
  const workspaceId = labels?.[KONTOR_WORKSPACE_ID_LABEL]?.trim();
  const directory = labels?.[KONTOR_WORKTREE_LABEL]?.trim();
  if (!workspaceId || !directory) {
    return null;
  }
  try {
    const handle = paseo.workspaces.ref(workspaceId);
    const workspace =
      handle.current() ?? (await bounded(handle.refresh(), options.timeoutMs, options.signal));
    if (!workspace) {
      return null;
    }
    const projectId = workspace.projectId?.trim();
    if (!projectId) {
      return null;
    }
    const workspaceDirectory = workspace.workspaceDirectory?.trim();
    if (workspaceDirectory && resolve(workspaceDirectory) !== resolve(directory)) {
      return null;
    }
    return { projectId, directory };
  } catch {
    return null;
  }
}
