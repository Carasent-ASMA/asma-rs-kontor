import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

export const sourceSummarySchema = z.object({
  id: z.string(), title: z.string(), kind: z.enum(["plan", "teams", "delivery", "decisions", "jira"]),
  path: z.string(), workstream: z.string().nullable(), epic: z.string().nullable(),
  projectId: z.string().nullable(), revisionKind: z.enum(["saved draft", "working copy", "approved revision"]),
  logicalPlanId: z.string().optional(),
  expectedSha256: z.string().regex(/^[a-f0-9]{64}$/).nullable().optional(),
});
export const canonicalGraphSchema = z.object({
  schema_version: z.literal(1), source: z.string(), source_sha256: z.string().regex(/^[a-f0-9]{64}$/),
  graph_sha256: z.string(), epic: z.object({ jira: z.string().nullable(), kontor_id: z.string().nullable() }),
  nodes: z.array(z.object({ id: z.string(), title: z.string(), phase: z.string(), jira: z.string().nullable(),
    kontor_id: z.string().nullable(), depends_on: z.array(z.string()), wave: z.number().int() })),
  edges: z.array(z.object({ from: z.string(), to: z.string() })),
  waves: z.array(z.object({ wave: z.number().int(), tasks: z.array(z.string()) })),
  external_fences: z.array(z.json()),
});
const decisionSchema = z.object({ id: z.string(), title: z.string(), state: z.enum(["open", "closed", "unknown"]),
  owner: z.string().optional(), detail: z.string().optional(), url: z.url().optional() }).strict();
const deliverySchema = z.object({ id: z.string(), title: z.string(),
  kind: z.enum(["pull request", "check", "release"]), state: z.string(), url: z.url().optional(),
  recordedAt: z.string().optional() }).strict();
const teamSchema = z.object({ id: z.string(), title: z.string(),
  roles: z.array(z.object({ role: z.string(), chain: z.array(z.string()) }).strict()) }).strict();
export const recordsSchema = z.object({
  teams: z.array(teamSchema).default([]), decisions: z.array(decisionSchema).default([]),
  delivery: z.array(deliverySchema).default([]),
}).strict();
const jiraKey = z.string().regex(/^ASMA-[1-9]\d*$/);
/** A plan read's diagnostic when its catalog and front-matter workstreams differ; the read then declares none. */
export const WORKSTREAM_CONFLICT = "Catalog and frontmatter workstreams conflict; automatic workstream association is disabled.";
/** The plan front-matter workstream grammar; the capture writes only registered `ws:<key>` labels. */
export const workstreamKey = z.string().regex(/^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/);
export const jiraIssueSchema = z.object({ key: jiraKey, status: z.string().min(1).max(160), cat: z.enum(["new", "indeterminate", "done"]) }).strict();
/** `ws` is the epic's Jira `ws:<key>` label as captured; absent when the capture names none. */
export const jiraEpicSchema = jiraIssueSchema.extend({ ws: workstreamKey.optional(), children: z.array(jiraIssueSchema).max(2000) }).strict();
/** Bounded field projection of the existing historical source, never a live feed. */
export const jiraHistorySchema = z.object({
  schema: z.literal("consumption/data@2"),
  generated: z.string().refine((value) => Number.isFinite(Date.parse(value)), "Invalid observation time"),
  provenance: z.object({ path: z.string().min(1).max(4096), sha256: z.string().regex(/^[a-f0-9]{64}$/), capturedEpics: z.number().int().nonnegative().max(2000) }).strict(),
  selection: z.array(jiraKey).min(1).max(2000),
  epics: z.array(jiraEpicSchema).max(2000),
}).strict().superRefine((value, context) => {
  const keys = new Set<string>();
  for (const epic of value.epics) {
    if (!value.selection.includes(epic.key)) context.addIssue({ code: "custom", message: "Epic outside projection selection" });
    for (const issue of [epic, ...epic.children]) {
      if (keys.has(issue.key)) context.addIssue({ code: "custom", message: `Ambiguous duplicate Jira key ${issue.key}` });
      keys.add(issue.key);
    }
  }
});
export type JiraHistory = z.infer<typeof jiraHistorySchema>;
export type JiraIssue = z.infer<typeof jiraIssueSchema>;
export const workstreamRegistrySchema = z.object({
  path: z.string().nullable(), revision: z.string().nullable(),
  workstreams: z.record(z.string(), z.object({ name: z.string() })),
  // A `fallback` desk holds only sessions not bound to an epic with a workstream; `paseo_project` names its native project.
  desks: z.record(z.string(), z.object({ name: z.string(), match: z.enum(["first", "fallback"]).optional(), paseo_project: z.string().optional() })), diagnostics: z.array(z.string()),
});
export type WorkstreamRegistry = z.infer<typeof workstreamRegistrySchema>;
export const sourceReadSchema = z.object({
  source: sourceSummarySchema, status: z.enum(["ready", "partial", "unavailable"]),
  revision: z.string().nullable(), readAt: z.string(), modifiedAt: z.string().nullable(),
  content: z.string().nullable(), graph: canonicalGraphSchema.nullable(), records: recordsSchema.nullable(),
  diagnostics: z.array(z.string()), jiraHistory: jiraHistorySchema.nullable().optional(),
  planMetadata: z.object({ workstream: z.string().nullable(), epic: z.string().nullable(), diagnostics: z.array(z.string()) }).optional(),
});
export const listReadSources = defineRpc({ name: "sources.list", input: z.object({}).strict(),
  output: z.object({ sources: z.array(sourceSummarySchema), diagnostics: z.array(z.string()), registry: workstreamRegistrySchema.optional() }) });
export const readSource = defineRpc({ name: "sources.read", input: z.object({ sourceId: z.string().min(1).max(128) }).strict(),
  output: sourceReadSchema });
/**
 * Metadata-only projection: no Markdown content, no canonical graph and no JSON records.
 * The Jira projection stays complete because the Sessions list reads captured epics at mount.
 */
export const sourceSummaryReadSchema = sourceReadSchema.extend({
  content: z.null(), graph: z.null(), records: z.null(),
});
export type SourceSummaryRead = z.infer<typeof sourceSummaryReadSchema>;
export const readSourceSummaries = defineRpc({ name: "sources.summaries", input: z.object({}).strict(),
  output: z.object({ sources: z.array(sourceSummarySchema), diagnostics: z.array(z.string()),
    registry: workstreamRegistrySchema.optional(), reads: z.record(z.string(), sourceSummaryReadSchema) }) });
export const readPlanAssociations = defineRpc({ name: "sources.associations", input: z.object({}).strict(),
  output: z.object({ status: z.enum(["ready", "invalid", "unavailable"]), associations: z.record(z.string(), z.array(z.string())),
    sources: z.array(sourceSummarySchema), error: z.string().nullable() }) });
export type ReadSourceSummary = z.infer<typeof sourceSummarySchema>;
export type SourceRead = z.infer<typeof sourceReadSchema>;
export type CanonicalGraph = z.infer<typeof canonicalGraphSchema>;
/** A summary is present but its exact Markdown, graph or JSON records have not been read yet. */
export function sourceReadPending(read: SourceRead): boolean {
  return read.status !== "unavailable" && read.content === null && read.graph === null
    && read.records === null && read.jiraHistory === undefined;
}
