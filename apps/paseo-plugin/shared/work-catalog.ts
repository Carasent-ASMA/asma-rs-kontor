import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

/** One bounded coverage line the catalog reports for a source. */
export const workCatalogCoverageSchema = z.object({
  subject: z.string(),
  state: z.string(),
  detail: z.string(),
}).strict();

/** One conflict a catalog source exposes. */
export const workCatalogConflictSchema = z.object({
  kind: z.string(),
  subject: z.string().nullable(),
  message: z.string(),
  observedAt: z.string().nullable(),
}).strict();

/** Freshness, coverage, errors, retry deadline and conflicts for one source. */
export const workCatalogSourceSchema = z.object({
  source: z.enum(["plan", "jira", "native"]),
  state: z.enum(["not_configured", "never_collected", "complete", "incomplete", "failed"]),
  generation: z.number().int().nonnegative(),
  attemptedGeneration: z.number().int().nonnegative(),
  asOf: z.string().nullable(),
  error: z.string().nullable(),
  retryAt: z.string().nullable(),
  coverage: z.array(workCatalogCoverageSchema).max(64),
  conflicts: z.array(workCatalogConflictSchema).max(64),
  pendingVerification: z.number().int().nonnegative(),
}).strict();

/** One catalog entry, reduced to what the plugin displays and maps. */
export const workCatalogEntrySchema = z.object({
  logicalPlanId: z.string(),
  kind: z.enum(["plan", "execution_unit"]),
  title: z.string(),
  state: z.enum(["active", "removed"]),
  sourceIds: z.array(z.string()).max(64),
  epic: z.string().nullable(),
  workstream: z.string().nullable(),
  repositoryIds: z.array(z.string()).max(64),
  relativePaths: z.array(z.string()).max(64),
  conflicts: z.array(z.object({ kind: z.string(), message: z.string(), observedAt: z.string() }).strict()).max(64),
  pendingVerification: z.number().int().nonnegative(),
  lastSeen: z.string(),
}).strict();

/** The plugin's projection of one catalog generation. */
export const workCatalogViewSchema = z.object({
  schemaVersion: z.literal(1),
  sources: z.array(workCatalogSourceSchema).length(3),
  entries: z.array(workCatalogEntrySchema).max(3000),
  entriesTruncated: z.boolean(),
}).strict();

/** The versioned last-known snapshot the plugin owns, never the daemon database. */
export const workCatalogSnapshotSchema = z.object({
  schema: z.literal("kontor/plugin-work-catalog@1"),
  version: z.literal(1),
  savedAt: z.string(),
  sourceAsOf: z.string().nullable(),
  view: workCatalogViewSchema,
}).strict();

/**
 * One catalog read answer. `checkedAt` is when the plugin asked, never a source
 * freshness instant; each source carries its own `asOf`.
 */
export const workCatalogReadSchema = z.object({
  mode: z.enum(["catalog", "snapshot", "unavailable"]),
  checkedAt: z.string(),
  sourceAsOf: z.string().nullable(),
  view: workCatalogViewSchema.nullable(),
  error: z.string().nullable(),
  snapshot: z.object({ savedAt: z.string(), sourceAsOf: z.string().nullable() }).strict().nullable(),
}).strict();

/** The daemon's sources document, exactly as the observer tool returns it. */
export const daemonSourceSchema = z.object({
  source: z.enum(["plan", "jira", "native"]),
  state: z.enum(["not_configured", "never_collected", "complete", "incomplete", "failed"]),
  generation: z.number().int().nonnegative(),
  attempted_generation: z.number().int().nonnegative(),
  as_of: z.string().nullable(),
  error: z.string().nullable(),
  retry_at: z.string().nullable(),
  coverage: z.array(z.object({ subject: z.string(), state: z.string(), detail: z.string() }).strict()).max(64),
  conflicts: z.array(z.object({ kind: z.string(), subject: z.string().nullable(), message: z.string(), observed_at: z.string().nullable() }).strict()).max(64),
  pending_verification: z.number().int().nonnegative(),
}).strict();

export const daemonSourcesViewSchema = z.object({
  schema_version: z.literal(1),
  sources: z.array(daemonSourceSchema).length(3),
}).strict();

const daemonAssociationSchema = z.object({
  relation: z.enum(["workstream", "epic", "project"]),
  value: z.string(),
  kind: z.enum(["explicit_operator", "plan_declaration"]),
  source_id: z.string().nullable(),
  verified: z.boolean(),
  dangling: z.boolean(),
  note: z.string().nullable(),
}).strict();

const daemonOccurrenceSchema = z.object({
  occurrence_id: z.string(),
  repository_id: z.string(),
  repository_root: z.string(),
  relative_path: z.string(),
  content_kind: z.enum(["tracked", "untracked_draft", "external_source"]),
  revision_kind: z.enum(["working_copy", "saved_draft", "approved_revision"]),
  content: z.object({
    revision_id: z.string(),
    byte_length: z.number().int().nonnegative(),
    modified_at: z.string(),
    title: z.string(),
    epic: z.string().nullable(),
    workstream: z.string().nullable(),
  }).strict(),
  source_ids: z.array(z.string()).max(64),
  expected_sha256: z.string().nullable(),
  pin_matches: z.boolean().nullable(),
  state: z.enum(["active", "removed"]),
  first_seen: z.string(),
  last_seen: z.string(),
}).strict();

export const daemonEntrySchema = z.object({
  logical_plan_id: z.string(),
  kind: z.enum(["plan", "execution_unit"]),
  title: z.string(),
  state: z.enum(["active", "removed"]),
  aliases: z.array(z.object({ kind: z.enum(["renamed_from", "source_id", "explicit_id"]), value: z.string(), observed_at: z.string() }).strict()).max(64),
  associations: z.array(daemonAssociationSchema).max(256),
  occurrences: z.array(daemonOccurrenceSchema).max(64),
  diagnostics: z.array(z.object({ code: z.string(), severity: z.enum(["info", "warning", "error"]), message: z.string(), subject: z.string().nullable(), logical_plan_id: z.string().nullable(), observed_at: z.string() }).strict()).max(256),
  conflicts: z.array(z.object({ kind: z.string(), logical_plan_id: z.string().nullable(), message: z.string(), observed_at: z.string() }).strict()).max(64),
  first_seen: z.string(),
  last_seen: z.string(),
}).strict();

export const daemonEntriesViewSchema = z.object({
  schema_version: z.literal(1),
  generation: z.number().int().nonnegative(),
  as_of: z.string().nullable(),
  source: z.enum(["database", "snapshot"]),
  entries: z.array(daemonEntrySchema).max(1000),
  truncated: z.boolean(),
  next_cursor: z.string().nullable().optional(),
}).strict();

export type WorkCatalogSource = z.infer<typeof workCatalogSourceSchema>;
export type WorkCatalogEntry = z.infer<typeof workCatalogEntrySchema>;
export type WorkCatalogView = z.infer<typeof workCatalogViewSchema>;
export type WorkCatalogSnapshot = z.infer<typeof workCatalogSnapshotSchema>;
export type WorkCatalogRead = z.infer<typeof workCatalogReadSchema>;
export type DaemonSourcesView = z.infer<typeof daemonSourcesViewSchema>;
export type DaemonEntriesView = z.infer<typeof daemonEntriesViewSchema>;

/** An entry's `pendingVerification` counts associations no source has verified. */
export function pendingAssociations(associations: readonly { verified: boolean; dangling: boolean }[]): number {
  return associations.filter((association) => !association.verified && !association.dangling).length;
}

/** The oldest source completion instant in a view; null when no source completed. */
export function oldestSourceAsOf(view: WorkCatalogView): string | null {
  const instants = view.sources.flatMap((source) => source.asOf === null ? [] : [source.asOf]).sort();
  return instants[0] ?? null;
}

export const readWorkCatalog = defineRpc({
  name: "work-catalog.read",
  input: z.object({}).strict(),
  output: workCatalogReadSchema,
});
