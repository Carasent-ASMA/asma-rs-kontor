import type { ReadSourceSummary } from "./read-sources";
import type { TeamsSourceFormat } from "./teams-source-snapshot";
/** Explicitly authorized source-ID bindings; never derive a format or path from a title. */
export const AUTHORIZED_TEAMS_FORMATS = {
  authoring: "fleet-authoring", operator: "fleet-operator", roster: "core-roster", templates: "team-templates",
} as const satisfies Readonly<Record<string, TeamsSourceFormat>>;
export function teamsSourceBindings(sources: readonly ReadSourceSummary[]) {
  const formats: Record<string, TeamsSourceFormat> = {};
  const diagnostics: string[] = [];
  for (const [id, format] of Object.entries(AUTHORIZED_TEAMS_FORMATS)) {
    const matches = sources.filter((source) => source.id === id);
    if (matches.length !== 1 || matches[0]?.kind !== "teams") {
      diagnostics.push(`Teams adapter ${id}: ${matches.length === 0 ? "ID missing from the configured catalog" : "expected exactly one kind teams entry"}. Mapping disabled; no path or replacement ID inferred.`);
    } else formats[id] = format;
  }
  return { formats, diagnostics };
}
