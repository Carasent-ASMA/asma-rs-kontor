import type { DirectoryEntry } from "./session-directory";
import { NO_LINKED_EPIC, sessionEpic, type OrganizationInput } from "./session-organization";

export type EpicScope = { readonly kind: "known"; readonly key: string } | { readonly kind: "unknown" };
export type ScopeChoice = { readonly key: string; readonly label: string; readonly detail?: string };
type Mappings = OrganizationInput["displayMappings"];

export function epicScopeKey(scope: EpicScope | null | undefined): string | null {
  return !scope ? null : scope.kind === "unknown" ? "unknown" : `known:${scope.key}`;
}

export function epicScopeChoice(key: string | null): EpicScope | null {
  if (key === null) return null;
  return key === "unknown" ? { kind: "unknown" } : { kind: "known", key: key.slice("known:".length) };
}

export function epicScopeLabel(scope: EpicScope): string {
  return scope.kind === "unknown" ? NO_LINKED_EPIC : scope.key;
}

export function matchesEpicScope(entry: DirectoryEntry, scope: EpicScope | null | undefined, mappings: Mappings): boolean {
  if (!scope) return true;
  const epic = sessionEpic(entry, mappings).epic;
  return scope.kind === "unknown" ? epic === null : epic?.toUpperCase() === scope.key;
}

/** Choices and membership use the same resolver; titles never establish an epic. */
export function epicScopeChoices(entries: readonly DirectoryEntry[], mappings: Mappings): ScopeChoice[] {
  const keys = new Set(entries.flatMap((entry) => {
    const epic = sessionEpic(entry, mappings).epic;
    return epic === null ? [] : [epic.toUpperCase()];
  }));
  return [...keys].sort().map((key) => ({ key: `known:${key}`, label: key }))
    .concat({ key: "unknown", label: NO_LINKED_EPIC });
}
