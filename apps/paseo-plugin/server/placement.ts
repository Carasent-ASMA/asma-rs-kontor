import { isAbsolute, relative, resolve } from "node:path";

export interface WorkspaceSourceLike {
  readonly kind?: string;
  readonly path?: string;
  readonly cwd?: string;
  readonly projectId?: string;
}

export interface WorkspaceCreateLike {
  readonly source?: WorkspaceSourceLike;
  readonly callerAgentId?: string;
}

export type PlacementDecision =
  | { readonly allowed: true }
  | { readonly allowed: false; readonly reason: string };

export interface PlacementRules {
  readonly paseoWorktreesRoot: string;
  readonly placementRoots: readonly string[];
}

export interface PlacementBinding {
  /** The native Paseo project the seat's own workspace belongs to. */
  readonly projectId: string;
  /** The canonical worktree the seat's binding labels name. */
  readonly directory: string;
}

export function within(child: string, parent: string): boolean {
  const rel = relative(parent, child);
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}

function refuse(reason: string): PlacementDecision {
  return { allowed: false, reason };
}

/**
 * Apply Kontor placement to one explicit workspace creation.
 *
 * Kontor places a seat into its own canonical directory under its own Paseo
 * project, both taken from the seat's binding. Everything else is outside that
 * rule: worktree creation (Paseo owns the worktree), an inferred project (the
 * caller's checkout wins silently), another epic's project or directory, and
 * Paseo's own `~/.paseo/worktrees` root (the fallback project from the
 * QNR-P1-06 misplacement).
 *
 * A `binding` of `null` marks a Paseo-direct seat, which Kontor never binds:
 * only the binding-free checks apply. A resolved binding adds the native
 * project and canonical worktree checks. The binding is resolved read-only
 * before this check. The exact native readback and every write stay with
 * `kontord`; the plugin holds no Kontor credential.
 */
export function evaluatePlacement(
  request: WorkspaceCreateLike,
  rules: PlacementRules,
  binding: PlacementBinding | null,
): PlacementDecision {
  const source = request.source;
  if (!source || typeof source !== "object") {
    return refuse("the request has no workspace source");
  }
  if (source.kind !== "directory") {
    return refuse(
      "Kontor placement registers an existing directory; worktree creation is outside it",
    );
  }
  const projectId = source.projectId?.trim();
  if (!projectId) {
    return refuse("Kontor placement requires an explicit projectId");
  }
  const path = source.path?.trim();
  if (!path || !isAbsolute(path)) {
    return refuse("Kontor placement requires an absolute directory path");
  }
  const resolved = resolve(path);
  if (within(resolved, resolve(rules.paseoWorktreesRoot))) {
    return refuse("the path is inside Paseo's own worktree root");
  }
  if (
    rules.placementRoots.length > 0 &&
    !rules.placementRoots.some((root) => within(resolved, resolve(root)))
  ) {
    return refuse("the path is outside the configured Kontor placement roots");
  }
  if (binding === null) {
    return { allowed: true };
  }
  const permittedProject = binding.projectId.trim();
  const permittedDirectory = binding.directory.trim();
  if (!permittedProject || !permittedDirectory || !isAbsolute(permittedDirectory)) {
    return refuse("the seat's binding names no native project and canonical directory");
  }
  if (projectId !== permittedProject) {
    return refuse("the request targets a project outside the seat's binding");
  }
  if (resolved !== resolve(permittedDirectory)) {
    return refuse("the request targets a directory outside the seat's canonical worktree");
  }
  return { allowed: true };
}
