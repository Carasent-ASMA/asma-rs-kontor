import { z } from "zod";
import { jiraHistorySchema, jiraIssueSchema, workstreamKey, type JiraHistory } from "../shared/read-sources";
// Pick relevant fields before export; account, usage and summary data never leave here.
const inputIssue = jiraIssueSchema.strip();
const inputSchema = z.object({ schema: z.literal("consumption/data@2"), generated: z.string(),
  epics: z.array(inputIssue.extend({ ws: workstreamKey.optional(), children: z.array(inputIssue).max(2000) })).max(2000) });
export function projectJiraHistory(raw: unknown, selection: readonly string[], provenance: { path: string; sha256: string }): JiraHistory {
  const input = inputSchema.parse(raw);
  return jiraHistorySchema.parse({ schema: input.schema, generated: input.generated,
    provenance: { ...provenance, capturedEpics: input.epics.length }, selection: [...new Set(selection)],
    epics: input.epics.filter((epic) => selection.includes(epic.key)) });
}
