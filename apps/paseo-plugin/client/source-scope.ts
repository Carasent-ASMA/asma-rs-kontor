import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import type { DirectoryEntry } from "./session-directory";
import { confirmedEpic } from "./page-data";

/** A project can contain several epics. These keys never attribute its unlabelled sessions. */
export function projectEpicKeys(projectId: string | null, entries: readonly DirectoryEntry[]): string[] {
  if (projectId === null) return [];
  return [...new Set(entries.filter((entry) => entry.projectKey === projectId).map(confirmedEpic).filter((key): key is string => key !== null))];
}

export function sourceEpic(source: ReadSourceSummary, read?: SourceRead): string | null {
  return read?.graph?.epic.jira ?? read?.planMetadata?.epic ?? source.epic;
}

/** Explicit project bindings constrain a source; projectless sources join exact observed epic keys. */
export function sourceInProject(source: ReadSourceSummary, read: SourceRead | undefined, projectId: string | null, projectEpics: readonly string[]): boolean {
  if (projectId === null) return true;
  if (source.projectId !== null) return source.projectId === projectId;
  if (source.kind === "jira") return true;
  const epic = sourceEpic(source, read);
  return epic !== null && projectEpics.includes(epic);
}
