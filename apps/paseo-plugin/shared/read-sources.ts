import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

export const sourceSummarySchema = z.object({
  id: z.string(), title: z.string(), kind: z.enum(["plan", "teams", "delivery", "decisions"]),
  path: z.string(), workstream: z.string().nullable(), epic: z.string().nullable(),
  projectId: z.string().nullable(), revisionKind: z.enum(["saved draft", "working copy", "approved revision"]),
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
export const sourceReadSchema = z.object({
  source: sourceSummarySchema, status: z.enum(["ready", "partial", "unavailable"]),
  revision: z.string().nullable(), readAt: z.string(), modifiedAt: z.string().nullable(),
  content: z.string().nullable(), graph: canonicalGraphSchema.nullable(), records: recordsSchema.nullable(),
  diagnostics: z.array(z.string()),
});
export const listReadSources = defineRpc({ name: "sources.list", input: z.object({}).strict(),
  output: z.object({ sources: z.array(sourceSummarySchema), diagnostics: z.array(z.string()) }) });
export const readSource = defineRpc({ name: "sources.read", input: z.object({ sourceId: z.string().min(1).max(128) }).strict(),
  output: sourceReadSchema });
export const readPlanAssociations = defineRpc({ name: "sources.associations", input: z.object({}).strict(),
  output: z.object({ status: z.enum(["ready", "invalid"]), associations: z.record(z.string(), z.array(z.string())),
    sources: z.array(sourceSummarySchema), error: z.string().nullable() }) });
export type ReadSourceSummary = z.infer<typeof sourceSummarySchema>;
export type SourceRead = z.infer<typeof sourceReadSchema>;
export type CanonicalGraph = z.infer<typeof canonicalGraphSchema>;
