import type { OrganizationSettings } from "../shared/organization";
/** Remove cleared fields instead of emitting undefined, which the host JSON contract rejects. */
export function setDisplayMappingField(mappings: OrganizationSettings["displayMappings"], id: string, field: "workstream" | "epic", value: string | undefined): OrganizationSettings["displayMappings"] {
  const next = { ...mappings };
  const mapping = { ...next[id] };
  const text = value?.trim();
  if (text) mapping[field] = text; else delete mapping[field];
  if (Object.keys(mapping).length) next[id] = mapping; else delete next[id];
  return next;
}
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
  let mappings = { ...current.displayMappings };
  for (const id of new Set([...Object.keys(base.displayMappings), ...Object.keys(local.displayMappings)])) {
    for (const field of ["workstream", "epic"] as const) {
      if (base.displayMappings[id]?.[field] !== local.displayMappings[id]?.[field]) {
        mappings = setDisplayMappingField(mappings, id, field, local.displayMappings[id]?.[field]);
      }
    }
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
