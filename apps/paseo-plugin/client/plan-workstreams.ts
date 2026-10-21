import type { ReadSourceSummary, SourceRead, WorkstreamRegistry } from "../shared/read-sources";
import type { DirectoryEntry } from "./session-directory";
import { declaredScope, nativeDesk, organizationScope, OTHER_WORKSTREAM, type OrganizationInput, type OrganizationWorkspace } from "./session-organization";
import { sourceEpic } from "./source-scope";

export function declaredPlanWorkstream(source: ReadSourceSummary, read?: SourceRead): string | null {
  if (source.kind !== "plan" || !read || read.status === "unavailable") return null;
  if (read.status !== "ready" && (!read.planMetadata || read.content === null)) return null;
  return read.planMetadata ? read.planMetadata.workstream : source.workstream;
}
/** Display-only fallback. Never written to settings or native labels, and never inferred from names. */
export function planWorkstreamMappings(sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, entries: readonly DirectoryEntry[], workspaces: readonly OrganizationWorkspace[], mappings: OrganizationInput["displayMappings"], associations: Readonly<Record<string, readonly string[]>>) {
  const result = { ...mappings };
  const choose = (epic: string | null, projectId: string | null, ids?: readonly string[]) => {
    const plans = sources.filter((source) => source.kind === "plan" && (source.projectId === null || source.projectId === projectId) && (ids?.length ? ids.includes(source.id) : epic !== null && sourceEpic(source, reads[source.id]) === epic));
    if (ids?.length && plans.length !== new Set(ids).size) return null;
    if (epic !== null && plans.some((source) => sourceEpic(source, reads[source.id]) !== null && sourceEpic(source, reads[source.id]) !== epic)) return null;
    const keys = plans.map((source) => reads[source.id]?.status === "ready" ? declaredPlanWorkstream(source, reads[source.id]) : null);
    return keys.length && keys.every((key) => key !== null && key === keys[0]) ? keys[0]! : null;
  };
  for (const entry of entries) {
    const scope = declaredScope(entry, mappings);
    if (scope.workstream !== null || nativeDesk(entry.projectName) !== null) continue;
    const epic = scope.epic && /^ASMA-[1-9]\d*$/.test(scope.epic.toUpperCase()) ? scope.epic.toUpperCase() : null;
    const key = choose(epic, entry.projectKey, associations[entry.id]);
    if (key) result[`session:${entry.id}`] = { ...result[`session:${entry.id}`], workstream: key };
  }
  for (const workspace of workspaces) {
    const mapping = result[`workspace:${workspace.id}`];
    if (mapping?.workstream?.trim() || nativeDesk(workspace.projectName) !== null) continue;
    const key = choose(mapping?.epic?.toUpperCase() ?? null, workspace.projectKey ?? null);
    if (key) result[`workspace:${workspace.id}`] = { ...mapping, workstream: key };
  }
  return result;
}

export type WorkstreamOption = {
  key: string; label: string; registered: boolean; plans: number; sessions: number; workspaces: number; desk: boolean;
  /** Counted sessions the tree shows under a native desk project of another key, by project name. */
  shownUnder: Record<string, number>;
  /** Sessions in this key's own native desk projects that are counted under other keys. */
  othersInDesk: number;
  /** Native workspaces of this key's desk projects with no session, which the tree lists and counts. */
  emptyInDesk: number;
};
/** Global registered keys, scoped declared plans and scoped native counts are separate facts. */
export function planWorkstreamOptions(sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, registry: WorkstreamRegistry | undefined, entries: readonly DirectoryEntry[], mappings: OrganizationInput["displayMappings"], nativeWorkspaces: readonly OrganizationWorkspace[] = []): WorkstreamOption[] {
  const options = new Map<string, WorkstreamOption>();
  const ensure = (key: string) => {
    const normalized = key.trim().toLowerCase();
    let value = options.get(normalized);
    if (!value) { value = { key: normalized, label: registry?.workstreams[normalized]?.name ?? key, registered: !!registry?.workstreams[normalized], plans: 0, sessions: 0, workspaces: 0, desk: false, shownUnder: {}, othersInDesk: 0, emptyInDesk: 0 }; options.set(normalized, value); }
    return value;
  };
  for (const key of Object.keys(registry?.workstreams ?? {})) ensure(key);
  const seen = new Set<string>();
  for (const source of sources) {
    const key = declaredPlanWorkstream(source, reads[source.id]);
    if (!key) continue;
    const identity = `${source.logicalPlanId ?? source.path}:${reads[source.id]?.revision ?? source.expectedSha256 ?? source.id}`;
    if (seen.has(identity)) continue;
    seen.add(identity); ensure(key).plans++;
  }
  const workspaces = new Map<string, Set<string>>();
  const hosted = new Map<string, number>();
  for (const entry of entries) {
    const key = organizationScope(entry, mappings).workstream;
    const value = ensure(key); value.sessions++;
    value.desk ||= !value.registered && (!!entry.labels.desk || nativeDesk(entry.projectName)?.toLowerCase() === value.key);
    // The tree keeps every session of a native desk project under that desk, whatever it is filed under.
    const host = entry.projectKey ? nativeDesk(entry.projectName) : null;
    if (host !== null && host !== value.key.replace(/[\s_]+/g, "-")) {
      value.shownUnder[entry.projectName!] = (value.shownUnder[entry.projectName!] ?? 0) + 1;
      hosted.set(host, (hosted.get(host) ?? 0) + 1);
    }
    const ids = workspaces.get(value.key) ?? new Set<string>();
    if (entry.workspaceId) ids.add(entry.workspaceId);
    workspaces.set(value.key, ids); value.workspaces = ids.size;
  }
  const occupied = new Set(entries.map((entry) => entry.workspaceId));
  const empty = new Map<string, number>();
  for (const workspace of nativeWorkspaces) {
    const key = mappings?.[`workspace:${workspace.id}`]?.workstream;
    const desk = workspace.projectKey ? nativeDesk(workspace.projectName) : null;
    if (!key && desk !== null && !occupied.has(workspace.id)) empty.set(desk, (empty.get(desk) ?? 0) + 1);
    if (!key) continue;
    const value = ensure(key), ids = workspaces.get(value.key) ?? new Set<string>();
    ids.add(workspace.id); workspaces.set(value.key, ids); value.workspaces = ids.size;
  }
  for (const [key, count] of hosted) { const value = options.get(key); if (value) value.othersInDesk = count; }
  for (const [key, count] of empty) { const value = options.get(key); if (value) value.emptyInDesk = count; }
  return [...options.values()].sort((a, b) => a.label.localeCompare(b.label));
}

const counted = (value: number, noun: string) => `${value} ${noun}${value === 1 ? "" : "s"}`;
/**
 * A choice's facts, kind first; the same text is its screen-reader name. Counts follow the association
 * the choice filters by, so where the tree shows a counted session under a native desk, the choice says so.
 */
export function workstreamOptionLabel(option: WorkstreamOption, registryLoaded: boolean): string {
  const unlinked = option.key === OTHER_WORKSTREAM.toLowerCase();
  const kind = unlinked ? "no workstream link" : option.desk ? "desk category" : option.sessions + option.workspaces === 0 && option.plans > 0 ? "plan only" : null;
  const shown = Object.entries(option.shownUnder).map(([desk, sessions]) => `${sessions} shown under ${desk}`).join(", ");
  return [option.label, kind, `${counted(option.sessions, "session")}${shown ? ` (${shown})` : ""}`, `${counted(option.workspaces, "workspace")}${option.emptyInDesk ? `, ${option.emptyInDesk} more empty in the desk` : ""}`, counted(option.plans, "plan"),
    option.othersInDesk ? `desk shows ${counted(option.othersInDesk, "more session")} filed under other workstreams` : null,
    unlinked || option.desk || !registryLoaded ? null : option.registered ? "registered" : "unregistered"].filter((part): part is string => !!part).join(" · ");
}

/** Why the plan fallback links nothing to an unlinked session, from the plans it weighs; null with no catalog. */
export function planFallbackNote(sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, epic: string | null, projectId: string | null, linked: readonly string[] = []): string | null {
  if (!sources.length) return null;
  if (linked.length) return "Its explicit plan links do not resolve to ready plans that declare one workstream.";
  const key = epic && /^ASMA-[1-9]\d*$/i.test(epic) ? epic.toUpperCase() : null;
  if (key === null) return "No known ASMA epic, so no plan applies.";
  const declaring = sources.filter((source) => source.kind === "plan" && sourceEpic(source, reads[source.id]) === key);
  const plans = declaring.filter((source) => source.projectId === null || source.projectId === projectId);
  if (!declaring.length) return `No configured plan declares ${key}.`;
  if (!plans.length) return `Plans for ${key} are bound to other projects.`;
  const states = new Map<string | null, number>();
  for (const source of plans) {
    const state = reads[source.id]?.status === "ready" ? declaredPlanWorkstream(source, reads[source.id]) ?? "no workstream" : null;
    states.set(state, (states.get(state) ?? 0) + 1);
  }
  const parts = [...states].map(([state, n]) => state === null ? `${n} ${n === 1 ? "is" : "are"} not ready` : `${n} ${n === 1 ? "declares" : "declare"} ${state}`);
  return `Of ${counted(plans.length, "plan")} for ${key}, ${parts.join(", ")}, so none applies.`;
}
