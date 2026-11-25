import { isAbsolute, relative, resolve, sep } from "node:path";

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

/** Where one workflow creation may land: one of these native projects and, when set, exactly this directory. */
export interface PlacementScope {
  readonly projects: readonly string[];
  readonly directory?: string;
}

/** Whether `child` is `parent` or beneath it. Only a `..` segment leaves `parent`; a name such as `..checkout` does not. */
export function within(child: string, parent: string): boolean {
  const rel = relative(parent, child);
  return rel === "" || (rel !== ".." && !rel.startsWith(`..${sep}`) && !isAbsolute(rel));
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
 * The scope is resolved read-only before this check, from the caller's own
 * native placement or a Kontor binding. There is no unscoped pass: an empty
 * scope refuses. The exact native readback and every write stay with
 * `kontord`; the plugin holds no Kontor credential.
 */
export function evaluatePlacement(
  request: WorkspaceCreateLike,
  rules: PlacementRules,
  scope: PlacementScope,
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
  const projects = scope.projects.map((project) => project.trim()).filter(Boolean);
  const directory = scope.directory?.trim();
  if (directory !== undefined && !isAbsolute(directory)) {
    return refuse("the seat's binding names no absolute canonical directory");
  }
  if (!projects.includes(projectId)) {
    return refuse("the request targets a project outside the seat's binding");
  }
  if (directory !== undefined && resolved !== resolve(directory)) {
    return refuse("the request targets a directory outside the seat's canonical worktree");
  }
  return { allowed: true };
}
