import type { DirectoryEntry } from "./session-directory";
import { roleLanes, type RoleLane, type TaskLink } from "./session-results";
import type { SessionTaskFilter } from "./session-view-store";

/**
 * A task's team, workflow and risk tier as one configured source declares them.
 * A display model, not a Kontor binding: Board places a session under a role by
 * the session's own role label, which assigns nothing.
 */
export type TaskTeamDeclaration = {
  /** The declaring source's title, named in Source details. */
  readonly source: string;
  readonly risk: string | null;
  readonly workflow: string | null;
  readonly team: string | null;
  /** Role codes in the source's order; empty when the source names a team without roles. */
  readonly roles: readonly string[];
  /** Declared handoffs. Roles with no path between them work in parallel; nothing is ordered unless declared. */
  readonly handoffs: readonly { readonly from: string; readonly to: string }[];
};

export type TaskTeamRead =
  | { readonly state: "declared"; readonly declaration: TaskTeamDeclaration }
  /** `coverage` says which configured sources were read and why none of them answers. */
  | { readonly state: "not-provided"; readonly coverage: string };

export type TaskTeamReader = (task: SessionTaskFilter) => TaskTeamRead;

/** The form `labelledRole` gives a role label, so a declared code and a label compare equal. */
function roleKey(role: string): string {
  return role.trim().replace(/[-_]+/g, " ").toLocaleUpperCase();
}

/**
 * Declared roles in steps: a role sits after every role that hands off to it, and
 * roles in one step work in parallel. A handoff back to an earlier role (a rework
 * loop) does not move it. `ordered` is false when the source declares no handoffs.
 */
export function teamSteps(declaration: TaskTeamDeclaration): { steps: string[][]; ordered: boolean } {
  const roles = [...new Set(declaration.roles.map(roleKey))];
  const known = new Set(roles);
  const edges = declaration.handoffs.map(({ from, to }) => [roleKey(from), roleKey(to)] as const).filter(([from, to]) => known.has(from) && known.has(to) && from !== to);
  if (!edges.length) return { steps: roles.length ? [roles] : [], ordered: false };
  const next = new Map(roles.map((role) => [role, edges.filter(([from]) => from === role).map(([, to]) => to)]));
  const forward: (readonly [string, string])[] = [];
  const done = new Set<string>();
  const visit = (role: string, path: Set<string>) => {
    if (done.has(role)) return;
    path.add(role);
    for (const to of next.get(role) ?? []) {
      if (path.has(to)) continue;
      forward.push([role, to]);
      visit(to, path);
    }
    path.delete(role);
    done.add(role);
  };
  const incoming = new Set(edges.map(([, to]) => to));
  for (const role of roles.filter((value) => !incoming.has(value))) visit(role, new Set());
  for (const role of roles) visit(role, new Set());
  const depth = new Map(roles.map((role) => [role, 0]));
  // The forward edges form a DAG over at most `roles.length` roles; this many passes settle the longest path.
  for (let pass = 0; pass < roles.length; pass += 1) {
    for (const [from, to] of forward) depth.set(to, Math.max(depth.get(to)!, depth.get(from)! + 1));
  }
  const steps: string[][] = [];
  for (const role of roles) (steps[depth.get(role)!] ??= []).push(role);
  return { steps: steps.filter((step) => step.length > 0), ordered: true };
}

export type TeamRoleView = { readonly role: string; readonly current: readonly DirectoryEntry[]; readonly earlier: number };

/**
 * The declared steps with the linked sessions whose role label names each role,
 * and the lanes of linked sessions the declaration does not name.
 */
export function taskTeamView(declaration: TaskTeamDeclaration, links: readonly TaskLink[]): { steps: TeamRoleView[][]; ordered: boolean; outside: RoleLane[] } {
  const lanes = roleLanes(links);
  const { steps, ordered } = teamSteps(declaration);
  const declared = new Set(steps.flat());
  const lane = (role: string) => lanes.find((value) => value.role === role);
  return {
    steps: steps.map((step) => step.map((role) => ({ role, current: lane(role)?.current ?? [], earlier: lane(role)?.earlier.length ?? 0 }))),
    ordered,
    outside: lanes.filter((value) => value.role === null || !declared.has(value.role)),
  };
}

/** "High-stakes · Standard delivery", from the declared risk tier and workflow only. */
export function teamHeadline(declaration: TaskTeamDeclaration): string {
  return [declaration.risk ?? "Risk tier not declared", declaration.workflow ?? "workflow not declared"].join(" · ");
}
