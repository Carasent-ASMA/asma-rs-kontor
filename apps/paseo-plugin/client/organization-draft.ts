import type { OrganizationSettings } from "../shared/organization";
function equal(left: unknown, right: unknown): boolean { return JSON.stringify(left) === JSON.stringify(right); }
function mergeMap<Value>(base: Record<string, Value>, local: Record<string, Value>, current: Record<string, Value>): Record<string, Value> {
  const result = { ...current };
  for (const key of new Set([...Object.keys(base), ...Object.keys(local)])) {
    if (equal(base[key], local[key])) continue;
    const value = local[key];
    if (value === undefined) delete result[key]; else result[key] = value;
  }
  return result;
}
function mergeSet(base: readonly string[], local: readonly string[], current: readonly string[]): string[] {
  const removed = new Set(base.filter((id) => !local.includes(id)));
  return [...new Set([...current.filter((id) => !removed.has(id)), ...local.filter((id) => !base.includes(id))])];
}
/** Explicit user reapply: retain concurrent changes to fields the local draft did not edit. */
export function rebaseOrganizationDraft(base: OrganizationSettings, local: OrganizationSettings, current: OrganizationSettings): OrganizationSettings {
  const mappings = mergeMap(base.displayMappings, local.displayMappings, current.displayMappings);
  for (const [id, mapping] of Object.entries(local.displayMappings)) {
    const before = base.displayMappings[id];
    if (equal(before, mapping)) continue;
    mappings[id] = { ...current.displayMappings[id],
      ...(before?.workstream === mapping.workstream ? {} : { workstream: mapping.workstream }),
      ...(before?.epic === mapping.epic ? {} : { epic: mapping.epic }) };
  }
  const associations = mergeMap(base.planAssociations, local.planAssociations, current.planAssociations);
  for (const [id, values] of Object.entries(local.planAssociations)) if (!equal(base.planAssociations[id], values)) {
    associations[id] = mergeSet(base.planAssociations[id] ?? [], values, current.planAssociations[id] ?? []);
  }
  return { ...current,
    grouping: local.grouping === base.grouping ? current.grouping : local.grouping,
    appearance: local.appearance === base.appearance ? current.appearance : local.appearance,
    favorites: mergeSet(base.favorites, local.favorites, current.favorites),
    expandedGroups: mergeMap(base.expandedGroups, local.expandedGroups, current.expandedGroups),
    siblingOrder: mergeMap(base.siblingOrder, local.siblingOrder, current.siblingOrder),
    displayMappings: mappings, planAssociations: associations,
  };
}
