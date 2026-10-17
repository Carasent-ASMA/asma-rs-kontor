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

function within(child: string, parent: string): boolean {
  const rel = relative(parent, child);
  return rel === "" || (!rel.startsWith("..") && !isAbsolute(rel));
}

function refuse(reason: string): PlacementDecision {
  return { allowed: false, reason };
}

/**
 * Apply Kontor placement to one explicit workspace creation.
 *
 * Kontor places a seat into an existing canonical directory registered under
 * the epic's Paseo project. Everything else is outside that rule: worktree
 * creation (Paseo owns the worktree), an inferred project (the caller's
 * checkout wins silently), and Paseo's own `~/.paseo/worktrees` root (the
 * fallback project from the QNR-P1-06 misplacement).
 *
 * This is the shape check only. The exact project, canonical cwd and native
 * readback stay with `kontord`; the plugin holds no Kontor credential.
 */
export function evaluatePlacement(
  request: WorkspaceCreateLike,
  rules: PlacementRules,
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
  return { allowed: true };
}
