import type { OrganizationGroup, OrganizationRow } from "./session-organization";
import { sessionKey } from "./session-organization";
export function rowKey(row: OrganizationRow): string { return row.kind === "group" ? row.group.key : sessionKey(row.entry.id); }
export function siblingKeys(groups: readonly OrganizationGroup[], parentKey: string): string[] {
  if (parentKey === "root") return groups.map((group) => group.key);
  for (const group of groups) {
    if (group.key === parentKey) return [...group.groups.map((child) => child.key), ...group.sessions.map((entry) => sessionKey(entry.id))];
    const found = siblingKeys(group.groups, parentKey);
    if (found.length) return found;
  }
  return [];
}
export function reorderedSiblings(keys: readonly string[], key: string, distance: number): string[] {
  const index = keys.indexOf(key);
  if (index < 0 || !Number.isFinite(distance)) return [...keys];
  const result = [...keys];
  result.splice(index, 1);
  result.splice(Math.max(0, Math.min(keys.length - 1, index + Math.trunc(distance))), 0, key);
  return result;
}
/** Replace only movable slots; never save a rank partition as the whole order. */
export function reorderSavedSiblingOrder(saved: readonly string[], siblings: readonly string[], movable: readonly string[], key: string, distance: number): string[] {
  const full = [...new Set([...saved, ...siblings])];
  const allowed = new Set(movable);
  const next = reorderedSiblings(full.filter((id) => allowed.has(id)), key, distance);
  let index = 0;
  return full.map((id) => allowed.has(id) ? next[index++]! : id);
}
