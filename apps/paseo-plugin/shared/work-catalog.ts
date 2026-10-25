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

/** One native project, reduced to what the plugin displays and groups. */
export const workCatalogNativeProjectSchema = z.object({
  projectId: z.string(),
  displayName: z.string(),
  customName: z.string().nullable(),
  projectKey: z.string().nullable(),
  rootPath: z.string(),
  projectKind: z.string().nullable(),
}).strict();

/** One native workspace, reduced to what the plugin displays and groups. */
export const workCatalogNativeWorkspaceSchema = z.object({
  workspaceId: z.string(),
  projectId: z.string(),
  name: z.string(),
  title: z.string().nullable(),
  directory: z.string(),
  kind: z.string(),
  archivingAt: z.string().nullable(),
  worktreeSlug: z.string().nullable(),
  projectDisplayName: z.string().nullable(),
  projectCustomName: z.string().nullable(),
  projectRootPath: z.string().nullable(),
  projectKind: z.string().nullable(),
}).strict();

const workCatalogNativeCheckoutSchema = z.object({
  cwd: z.string().nullable(),
  branch: z.string().nullable(),
  worktreeRoot: z.string().nullable(),
  isGit: z.boolean().nullable(),
  isPaseoOwnedWorktree: z.boolean().nullable(),
}).strict();

/** One native session. The session id is identity; the title is display data. */
export const workCatalogNativeSessionSchema = z.object({
  sessionId: z.string(),
  workspaceId: z.string().nullable(),
  projectId: z.string().nullable(),
  projectName: z.string().nullable(),
  workspaceName: z.string().nullable(),
  title: z.string().nullable(),
  provider: z.string(),
  model: z.string().nullable(),
  status: z.string(),
  archivedAt: z.string().nullable(),
  attentionReason: z.string().nullable(),
  cwd: z.string(),
  createdAt: z.string().nullable(),
  updatedAt: z.string().nullable(),
  lastUserMessageAt: z.string().nullable(),
  parentSessionId: z.string().nullable(),
  checkout: workCatalogNativeCheckoutSchema.nullable(),
}).strict();

/**
 * One generation-consistent native inventory. Every row carries an exact
 * native id; a bounded page says so through `truncated` rather than implying a
 * complete host.
 */
export const workCatalogNativeViewSchema = z.object({
  runtimeInstance: z.string().min(1),
  generation: z.number().int().nonnegative(),
  asOf: z.string().nullable(),
  source: z.enum(["database", "snapshot"]),
  archiveScope: z.boolean(),
  truncated: z.boolean(),
  projects: z.array(workCatalogNativeProjectSchema).max(3000),
  workspaces: z.array(workCatalogNativeWorkspaceSchema).max(3000),
  sessions: z.array(workCatalogNativeSessionSchema).max(3000),
}).strict();

/** The plugin's native-inventory read, independent of the plan-catalog mode. */
export const workCatalogNativeReadSchema = z.object({
  mode: z.enum(["catalog", "snapshot", "unavailable"]),
  hostId: z.string(),
  runtimeInstance: z.string().nullable(),
  checkedAt: z.string(),
  view: workCatalogNativeViewSchema.nullable(),
  error: z.string().nullable(),
  snapshotSavedAt: z.string().nullable(),
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
 * Snapshot v2 adds the selected host identity and its native inventory. It is
 * bound to one host; a different host refuses it rather than reading another
 * host's rows. A v1 snapshot remains readable for the plan view and carries no
 * native inventory, so a v1 file can never be mistaken for a native snapshot.
 */
export const workCatalogSnapshotV2Schema = z.object({
  schema: z.literal("kontor/plugin-work-catalog@2"),
  version: z.literal(2),
  savedAt: z.string(),
  sourceAsOf: z.string().nullable(),
  hostId: z.string().min(1),
  runtimeInstance: z.string().nullable(),
  nativeAsOf: z.string().nullable(),
  native: workCatalogNativeViewSchema.nullable(),
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
  native: workCatalogNativeReadSchema,
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

const daemonNativeRuntimeCoverageSchema = z.object({
  runtime_instance: z.string(),
  client_id: z.string(),
  server_id: z.string().nullable(),
  server_version: z.string().nullable(),
  include_archived: z.boolean(),
  state: z.enum(["scanned", "incomplete", "failed"]),
  projects: z.number().int().nonnegative(),
  workspaces: z.number().int().nonnegative(),
  sessions: z.number().int().nonnegative(),
  archived_sessions: z.number().int().nonnegative(),
  error: z.string().nullable(),
}).passthrough();

/** The daemon's native status, reduced to the exact identity and coverage. */
export const daemonNativeStatusSchema = z.object({
  schema_version: z.literal(1),
  generation: z.number().int().nonnegative(),
  attempted_generation: z.number().int().nonnegative(),
  as_of: z.string().nullable(),
  collection: z.object({
    state: z.enum(["never_collected", "complete", "incomplete", "failed"]),
    include_archived: z.boolean(),
    coverage: z.array(daemonNativeRuntimeCoverageSchema).max(64),
  }).passthrough(),
}).passthrough();

const daemonNativeProjectSchema = z.object({
  runtime_instance: z.string(),
  project_id: z.string(),
  display_name: z.string(),
  custom_name: z.string().nullable(),
  project_key: z.string().nullable(),
  root_path: z.string(),
  project_kind: z.string().nullable(),
  state: z.enum(["active", "removed"]),
  first_seen: z.string(),
  last_seen: z.string(),
}).strict();

const daemonNativeWorkspaceSchema = z.object({
  runtime_instance: z.string(),
  workspace_id: z.string(),
  project_id: z.string(),
  name: z.string(),
  title: z.string().nullable(),
  directory: z.string(),
  kind: z.string(),
  archiving_at: z.string().nullable(),
  worktree_slug: z.string().nullable(),
  project_display_name: z.string().nullable(),
  project_custom_name: z.string().nullable(),
  project_root_path: z.string().nullable(),
  project_kind: z.string().nullable(),
  state: z.enum(["active", "removed"]),
  first_seen: z.string(),
  last_seen: z.string(),
}).strict();

const daemonNativeCheckoutSchema = z.object({
  cwd: z.string().nullable(),
  branch: z.string().nullable(),
  worktree_root: z.string().nullable(),
  is_git: z.boolean().nullable(),
  is_paseo_owned_worktree: z.boolean().nullable(),
}).strict();

const daemonNativeSessionSchema = z.object({
  runtime_instance: z.string(),
  session_id: z.string(),
  workspace_id: z.string().nullable(),
  project_id: z.string().nullable(),
  project_name: z.string().nullable(),
  workspace_name: z.string().nullable(),
  title: z.string().nullable(),
  provider: z.string(),
  model: z.string().nullable(),
  status: z.string(),
  archived_at: z.string().nullable(),
  attention_reason: z.string().nullable(),
  cwd: z.string(),
  created_at: z.string().nullable(),
  updated_at: z.string().nullable(),
  last_user_message_at: z.string().nullable(),
  parent_session_id: z.string().nullable(),
  checkout: daemonNativeCheckoutSchema.nullable(),
  state: z.enum(["active", "removed"]),
  first_seen: z.string(),
  last_seen: z.string(),
}).strict();

/** The daemon's page of native projects, workspaces and sessions. */
export const daemonNativeInventorySchema = z.object({
  schema_version: z.literal(1),
  generation: z.number().int().nonnegative(),
  as_of: z.string().nullable(),
  source: z.enum(["database", "snapshot"]),
  archive_scope: z.boolean(),
  projects: z.array(daemonNativeProjectSchema).max(1000),
  workspaces: z.array(daemonNativeWorkspaceSchema).max(1000),
  sessions: z.array(daemonNativeSessionSchema).max(1000),
  truncated: z.boolean(),
  next_cursor: z.string().nullable().optional(),
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
export type WorkCatalogSnapshotV2 = z.infer<typeof workCatalogSnapshotV2Schema>;
export type WorkCatalogNativeProject = z.infer<typeof workCatalogNativeProjectSchema>;
export type WorkCatalogNativeWorkspace = z.infer<typeof workCatalogNativeWorkspaceSchema>;
export type WorkCatalogNativeSession = z.infer<typeof workCatalogNativeSessionSchema>;
export type WorkCatalogNativeView = z.infer<typeof workCatalogNativeViewSchema>;
export type WorkCatalogNativeRead = z.infer<typeof workCatalogNativeReadSchema>;
export type WorkCatalogRead = z.infer<typeof workCatalogReadSchema>;
export type DaemonSourcesView = z.infer<typeof daemonSourcesViewSchema>;
export type DaemonEntriesView = z.infer<typeof daemonEntriesViewSchema>;
export type DaemonNativeStatus = z.infer<typeof daemonNativeStatusSchema>;
export type DaemonNativeInventory = z.infer<typeof daemonNativeInventorySchema>;

/** An entry's `pendingVerification` counts associations no source has verified. */
export function pendingAssociations(associations: readonly { verified: boolean; dangling: boolean }[]): number {
  return associations.filter((association) => !association.verified && !association.dangling).length;
}

/** The oldest source completion instant in a view; null when no source completed. */
export function oldestSourceAsOf(view: WorkCatalogView): string | null {
  const instants = view.sources.flatMap((source) => source.asOf === null ? [] : [source.asOf]).sort();
  return instants[0] ?? null;
}

/** A host id is the exact Paseo `serverId`; it is never a label or title. */
export const readWorkCatalog = defineRpc({
  name: "work-catalog.read",
  input: z.object({ hostId: z.string().min(1).max(512) }).strict(),
  output: workCatalogReadSchema,
});
