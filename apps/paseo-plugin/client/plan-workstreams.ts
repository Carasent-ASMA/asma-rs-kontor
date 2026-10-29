import { WORKSTREAM_CONFLICT, type ReadSourceSummary, type SourceRead, type WorkstreamRegistry } from "../shared/read-sources";
import type { DirectoryEntry } from "./session-directory";
import { declaredScope, nativeDesk, organizationScope, OTHER_WORKSTREAM, type OrganizationInput, type OrganizationWorkspace } from "./session-organization";
import { sourceEpic } from "./source-scope";

export function declaredPlanWorkstream(source: ReadSourceSummary, read?: SourceRead): string | null {
  if (source.kind !== "plan" || !read || read.status === "unavailable") return null;
  if (read.status !== "ready" && (!read.planMetadata || read.content === null)) return null;
  return read.planMetadata ? read.planMetadata.workstream : source.workstream;
}
/** Each epic's `ws:` label across the ready Jira captures; null where captures disagree, absent where none names one. */
export function capturedWorkstreams(reads: readonly SourceRead[]): Map<string, string | null> {
  const result = new Map<string, string | null>();
  for (const read of reads) {
    if (read.source.kind !== "jira" || read.status !== "ready") continue;
    for (const epic of read.jiraHistory?.epics ?? []) {
      if (epic.ws === undefined) continue;
      const known = result.get(epic.key);
      result.set(epic.key, known === undefined || known === epic.ws ? epic.ws : null);
    }
  }
  return result;
}
/**
 * Display-only fallback. Never written to settings or native labels, and never inferred from names. A session's
 * known epic takes the workstream its ready plans declare or its captured Jira `ws:` label names; a disagreement
 * between them links nothing. Explicit plan links use plans alone. A native desk holds its sessions unless the
 * registry calls it a `fallback` desk, whose sessions follow a bound epic's workstream.
 */
export function planWorkstreamMappings(sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, entries: readonly DirectoryEntry[], workspaces: readonly OrganizationWorkspace[], mappings: OrganizationInput["displayMappings"], associations: Readonly<Record<string, readonly string[]>>, registry?: WorkstreamRegistry) {
  const result = { ...mappings };
  const captured = capturedWorkstreams(Object.values(reads));
  const fallback = new Set(Object.values(registry?.desks ?? {}).flatMap((desk) => desk.match === "fallback" ? [nativeDesk(desk.paseo_project)] : []));
  const holds = (projectName: string | null | undefined) => { const desk = nativeDesk(projectName); return desk !== null && !fallback.has(desk); };
  const choose = (epic: string | null, projectId: string | null, ids?: readonly string[]) => {
    const plans = sources.filter((source) => source.kind === "plan" && (source.projectId === null || source.projectId === projectId) && (ids?.length ? ids.includes(source.id) : epic !== null && sourceEpic(source, reads[source.id]) === epic));
    if (ids?.length && plans.length !== new Set(ids).size) return null;
    if (epic !== null && plans.some((source) => sourceEpic(source, reads[source.id]) !== null && sourceEpic(source, reads[source.id]) !== epic)) return null;
    const keys = plans.map((source) => reads[source.id]?.status === "ready" ? declaredPlanWorkstream(source, reads[source.id]) : null);
    const label = ids?.length || epic === null ? undefined : captured.get(epic);
    if (label !== undefined) return label !== null && keys.every((key) => key === null || key === label) ? label : null;
    return keys.length && keys.every((key) => key !== null && key === keys[0]) ? keys[0]! : null;
  };
  for (const entry of entries) {
    const scope = declaredScope(entry, mappings);
    if (scope.workstream !== null || holds(entry.projectName)) continue;
    const epic = scope.epic && /^ASMA-[1-9]\d*$/.test(scope.epic.toUpperCase()) ? scope.epic.toUpperCase() : null;
    const key = choose(epic, entry.projectKey, associations[entry.id]);
    if (key) result[`session:${entry.id}`] = { ...result[`session:${entry.id}`], workstream: key };
  }
  for (const workspace of workspaces) {
    const mapping = result[`workspace:${workspace.id}`];
    if (mapping?.workstream?.trim() || holds(workspace.projectName)) continue;
    const key = choose(mapping?.epic?.toUpperCase() ?? null, workspace.projectKey ?? null);
    if (key) result[`workspace:${workspace.id}`] = { ...mapping, workstream: key };
  }
  return result;
}

export type WorkstreamOption = {
  key: string; label: string; registered: boolean; plans: number; sessions: number; workspaces: number; desk: boolean;
  /** False when the shared catalog supplied rows without labels: session counts are unknown, never zero. */
  countsUnknown?: boolean;
  /** Counted sessions the tree shows under a native desk project of another key, by project name. */
  shownUnder: Record<string, number>;
  /** Sessions in this key's own native desk projects that are counted under other keys. */
  othersInDesk: number;
  /** Native workspaces of this key's desk projects with no session, which the tree lists and counts. */
  emptyInDesk: number;
};
/** Global registered keys, scoped declared plans and scoped native counts are separate facts. */
export function planWorkstreamOptions(sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, registry: WorkstreamRegistry | undefined, entries: readonly DirectoryEntry[], mappings: OrganizationInput["displayMappings"], nativeWorkspaces: readonly OrganizationWorkspace[] = [], labelsComplete = true): WorkstreamOption[] {
  const options = new Map<string, WorkstreamOption>();
  const ensure = (key: string) => {
    const normalized = key.trim().toLowerCase();
    let value = options.get(normalized);
    if (!value) { value = { key: normalized, label: registry?.workstreams[normalized]?.name ?? key, registered: !!registry?.workstreams[normalized], plans: 0, sessions: 0, workspaces: 0, desk: false, countsUnknown: !labelsComplete, shownUnder: {}, othersInDesk: 0, emptyInDesk: 0 }; options.set(normalized, value); }
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
  const kind = option.countsUnknown ? null
    : unlinked ? "no workstream link" : option.desk ? "desk category" : option.sessions + option.workspaces === 0 && option.plans > 0 ? "plan only" : null;
  const shown = option.countsUnknown ? "" : Object.entries(option.shownUnder).map(([desk, sessions]) => `${sessions} shown under ${desk}`).join(", ");
  const sessions = option.countsUnknown ? "session count unavailable" : `${counted(option.sessions, "session")}${shown ? ` (${shown})` : ""}`;
  return [option.label, kind, sessions, `${counted(option.workspaces, "workspace")}${option.emptyInDesk ? `, ${option.emptyInDesk} more empty in the desk` : ""}`, counted(option.plans, "plan"),
    !option.countsUnknown && option.othersInDesk ? `desk shows ${counted(option.othersInDesk, "more session")} filed under other workstreams` : null,
    unlinked || option.desk || !registryLoaded ? null : option.registered ? "registered" : "unregistered"].filter((part): part is string => !!part).join(" · ");
}

/**
 * Why the fallback links nothing to an unlinked session, from the plans it weighs and, when a ready Jira capture
 * is configured, that capture's `ws:` label for the epic; null with no catalog.
 */
export function planFallbackNote(sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, epic: string | null, projectId: string | null, linked: readonly string[] = []): string | null {
  if (!sources.length) return null;
  if (linked.length) return "Its explicit plan links do not resolve to ready plans that declare one workstream.";
  const captures = Object.values(reads).flatMap((read) => read.source.kind === "jira" && read.status === "ready" && read.jiraHistory ? [read.jiraHistory] : []);
  const key = epic && /^ASMA-[1-9]\d*$/i.test(epic) ? epic.toUpperCase() : null;
  if (key === null) return `No known ASMA epic, so no plan${captures.length ? " or Jira ws: label" : ""} applies.`;
  const declaring = sources.filter((source) => source.kind === "plan" && sourceEpic(source, reads[source.id]) === key);
  const plans = declaring.filter((source) => source.projectId === null || source.projectId === projectId);
  const observed = `the Jira capture of ${captures.map((capture) => capture.generated).join(" / ")}`;
  const label = capturedWorkstreams(Object.values(reads)).get(key);
  const jira = !captures.length ? "" : label === null ? ` Jira captures disagree on the ws: label of ${key}.`
    : label !== undefined ? ` Sources disagree: ${observed} labels ${key} ws:${label}, which a ready plan contradicts.`
    : captures.some((capture) => capture.epics.some((captured) => captured.key === key)) ? ` ${key} has no ws: label in ${observed}.` : ` ${key} is not in ${observed}.`;
  if (!declaring.length) return `No configured plan declares ${key}.${jira}`;
  if (!plans.length) return `Plans for ${key} are bound to other projects.${jira}`;
  const states = new Map<string | null, number>();
  for (const source of plans) {
    const state = reads[source.id]?.status === "ready" ? declaredPlanWorkstream(source, reads[source.id]) ?? "no workstream" : null;
    states.set(state, (states.get(state) ?? 0) + 1);
  }
  const parts = [...states].map(([state, n]) => state === null ? `${n} ${n === 1 ? "is" : "are"} not ready` : `${n} ${n === 1 ? "declares" : "declare"} ${state}`);
  return `Of ${counted(plans.length, "plan")} for ${key}, ${parts.join(", ")}, so none applies.${jira}`;
}

/**
 * Where the display-only fallback found a session's derived workstream `key`: the applicable ready plans declaring it,
 * the epic's captured Jira `ws:` label, and each applicable plan it did not associate. Null when neither supplied `key`.
 */
export function derivedWorkstreamNote(sources: readonly ReadSourceSummary[], reads: Readonly<Record<string, SourceRead>>, epic: string | null, projectId: string | null, key: string, linked: readonly string[] = []): string | null {
  if (linked.length) return `Workstream from its explicit plan links, which declare ${key}.`;
  const upper = epic && /^ASMA-[1-9]\d*$/i.test(epic) ? epic.toUpperCase() : null;
  if (upper === null) return null;
  const plans = sources.filter((source) => source.kind === "plan" && (source.projectId === null || source.projectId === projectId) && sourceEpic(source, reads[source.id]) === upper);
  const declaring = plans.filter((source) => reads[source.id]?.status === "ready" && declaredPlanWorkstream(source, reads[source.id]) === key).length;
  const captures = Object.values(reads).flatMap((read) => read.source.kind === "jira" && read.status === "ready" && read.jiraHistory ? [read.jiraHistory.generated] : []);
  const jira = capturedWorkstreams(Object.values(reads)).get(upper) === key ? `the Jira ws:${key} label of ${upper} (capture of ${captures.join(" / ")})` : null;
  if (!declaring && !jira) return null;
  const from = [declaring ? `${counted(declaring, "ready plan")} for ${upper} declaring ${key}` : null, jira].filter((part): part is string => !!part).join(", confirmed by ");
  const reasons = [...new Set(plans.flatMap((source) => {
    const read = reads[source.id];
    if (read?.status !== "ready") return ["not ready"];
    if (declaredPlanWorkstream(source, read) !== null) return [];
    return [read.diagnostics.includes(WORKSTREAM_CONFLICT) ? "catalog and frontmatter workstreams conflict" : "no workstream declared"];
  }))];
  const skipped = plans.length - declaring;
  return `Workstream from ${from}.${skipped ? ` ${counted(skipped, "plan")} for ${upper} ${skipped === 1 ? "is" : "are"} not associated (${reasons.join("; ")})${declaring ? "" : ", so Jira supplied the key"}.` : ""}`;
}
