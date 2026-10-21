import type { ReadSourceSummary, SourceRead, WorkstreamRegistry } from "../shared/read-sources";
import type { DirectoryEntry } from "./session-directory";
import { declaredScope, nativeDesk, organizationScope, type OrganizationInput, type OrganizationWorkspace } from "./session-organization";
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

export type WorkstreamOption = { key: string; label: string; registered: boolean; plans: number; sessions: number; workspaces: number; desk: boolean };
/** Global registered keys, scoped declared plans and scoped native counts are separate facts. */
export function planWorkstreamOptions(sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, registry: WorkstreamRegistry | undefined, entries: readonly DirectoryEntry[], mappings: OrganizationInput["displayMappings"], nativeWorkspaces: readonly OrganizationWorkspace[] = []): WorkstreamOption[] {
  const options = new Map<string, WorkstreamOption>();
  const ensure = (key: string) => {
    const normalized = key.trim().toLowerCase();
    let value = options.get(normalized);
    if (!value) { value = { key: normalized, label: registry?.workstreams[normalized]?.name ?? key, registered: !!registry?.workstreams[normalized], plans: 0, sessions: 0, workspaces: 0, desk: false }; options.set(normalized, value); }
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
  for (const entry of entries) {
    const key = organizationScope(entry, mappings).workstream;
    const value = ensure(key); value.sessions++;
    value.desk ||= !value.registered && (!!entry.labels.desk || nativeDesk(entry.projectName)?.toLowerCase() === value.key);
    const ids = workspaces.get(value.key) ?? new Set<string>();
    if (entry.workspaceId) ids.add(entry.workspaceId);
    workspaces.set(value.key, ids); value.workspaces = ids.size;
  }
  for (const workspace of nativeWorkspaces) {
    const key = mappings?.[`workspace:${workspace.id}`]?.workstream;
    if (!key) continue;
    const value = ensure(key), ids = workspaces.get(value.key) ?? new Set<string>();
    ids.add(workspace.id); workspaces.set(value.key, ids); value.workspaces = ids.size;
  }
  return [...options.values()].sort((a, b) => a.label.localeCompare(b.label));
}
