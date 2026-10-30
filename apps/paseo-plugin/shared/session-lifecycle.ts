import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";
import { featureValuesSchema } from "./session-create";

export const TAKEOVER_LIMIT = "Archive acts only on the sessions selected here. Nothing is retried or replaced; Take over is its own reviewed action.";
/** Resume was removed with its toolbar. Its receipts stay readable as history; none is executed or replayed. */
export const RESUME_REMOVED = "Resume is no longer offered. This receipt is kept as history; nothing was sent.";
const id = z.string().min(1).max(200);
export const recoveryTargetSchema = z.object({ provider: id, model: id, effort: id }).strict();
export type RecoveryTarget = z.infer<typeof recoveryTargetSchema>;
export const DEFAULT_RECOVERY_TARGET: RecoveryTarget = { provider: "codex", model: "gpt-6.1-sol", effort: "xhigh" };
export const availabilitySchema = z.object({ agentId: id, nativeStatus: z.string().optional(), archivedAt: z.string().nullable().optional(), providerHistory: z.enum(["unverified", "not-reported"]).optional(), localRemoval: z.boolean().optional(), state: z.enum(["readable", "missing", "unknown"]), reason: z.string(), checkedAt: z.string(), revision: z.string() });
export type SessionAvailability = z.infer<typeof availabilitySchema>;
export const AVAILABILITY_TTL_MS = 60_000;
/** Exact native read evidence expires; neither a timestamp nor provider failure proves absence. */
export function freshAvailability(entry: { id: string; updatedAt: string; availability?: SessionAvailability }, now = Date.now()): SessionAvailability | undefined {
  const value = entry.availability, age = value ? now - Date.parse(value.checkedAt) : NaN;
  return value?.agentId === entry.id && value.revision === entry.updatedAt && age >= 0 && age < AVAILABILITY_TTL_MS ? value : undefined;
}
export const lifecycleRowSchema = z.object({
  id, title: z.string(), workspaceId: z.string().nullable(), cwd: z.string(), provider: z.string(), model: z.string().nullable(),
  effort: z.string().nullable(), mode: z.string().nullable(), nativeStatus: z.string(), fingerprint: z.string(),
  category: z.enum(["candidate", "unknown", "excluded"]), eligible: z.boolean(), reason: z.string(),
  children: z.array(z.object({ id, title: z.string(), status: z.string(), workspaceId: z.string().nullable() })),
  availability: availabilitySchema.optional(),
  reconcileReviewId: z.string().uuid().optional(),
  outcome: z.enum(["pending", "intent", "submitted", "archived", "blocked", "uncertain", "absent", "already_archived"]), message: z.string().nullable(),
  // Archive membership and runtime retirement are separate facts: "closed" only from an exact-ID read after the
  // archive, "not-observed" when the archive returned but the read is not closed, "unconfirmed" when the reply failed.
  runtime: z.enum(["closed", "not-observed", "unconfirmed"]).optional(),
});
export type LifecycleRow = z.infer<typeof lifecycleRowSchema>;
export const ARCHIVE_BATCH_LIMIT = 50;
/** Stable child-first order; a cyclic native relationship cannot be archived safely. */
export function archiveOrder(rows: LifecycleRow[]): LifecycleRow[] {
  const byId = new Map(rows.map((row) => [row.id, row])), visiting = new Set<string>(), done = new Set<string>(), ordered: LifecycleRow[] = [];
  function visit(row: LifecycleRow) {
    if (done.has(row.id)) return;
    if (visiting.has(row.id)) throw new Error("Managed-child cycle: no archive batch can run. Review native relationships first.");
    visiting.add(row.id);
    for (const child of row.children) { const selected = byId.get(child.id); if (selected) visit(selected); }
    visiting.delete(row.id); done.add(row.id); ordered.push(row);
  }
  rows.forEach(visit);
  return ordered;
}
export function archiveSummary(rows: LifecycleRow[]): string {
  const archived = rows.filter((row) => row.outcome === "archived" && row.runtime !== "unconfirmed").length;
  const uncertain = rows.filter((row) => unresolvedArchive(row)).length;
  const blocked = rows.filter((row) => row.outcome === "blocked").length;
  const absent = rows.filter((row) => row.outcome === "absent").length;
  const existing = rows.filter((row) => row.outcome === "already_archived").length;
  const pending = rows.filter((row) => row.outcome === "pending").length;
  return [`Archived ${archived}`, uncertain ? `${uncertain} not confirmed` : null, blocked ? `${blocked} blocked` : null,
    absent ? `${absent} absent (local removal only)` : null, existing ? `${existing} already archived` : null, pending ? `${pending} not attempted` : null].filter(Boolean).join(" · ");
}
/** An archive whose fence stays held until an exact-ID read settles it. */
export const unresolvedArchive = (row: Pick<LifecycleRow, "outcome" | "runtime">) =>
  row.outcome === "intent" || row.outcome === "uncertain" || (row.outcome === "archived" && row.runtime === "unconfirmed");
export const unresolvedRefSchema = z.object({ reviewId: z.string().uuid(), agentId: id }).strict();
export type UnresolvedRef = z.infer<typeof unresolvedRefSchema>;
export const lifecycleReviewSchema = z.object({
  id: z.string().uuid(), createdAt: z.string(), action: z.enum(["resume", "archive"]), scope: z.enum(["filtered", "host"]),
  target: recoveryTargetSchema, rows: z.array(lifecycleRowSchema), limitation: z.string(),
  // Derived only by the latest read; never saved. A latest.json pointer alone would lose an earlier unresolved archive.
  unresolved: z.array(unresolvedRefSchema).max(100).optional(),
});
export type LifecycleReview = z.infer<typeof lifecycleReviewSchema>;
export const previewLifecycle = defineRpc({ name: "sessions.lifecycle-preview", input: z.object({
  action: z.literal("archive"), scope: z.enum(["filtered", "host"]), agentIds: z.array(id).max(10_000), target: recoveryTargetSchema,
}).strict(), output: lifecycleReviewSchema });
export const executeLifecycleItem = defineRpc({ name: "sessions.lifecycle-execute-item", input: z.object({ reviewId: z.string().uuid(), agentId: id }).strict(), output: lifecycleRowSchema });
export const reconcileLifecycleItem = defineRpc({ name: "sessions.lifecycle-reconcile-item", input: z.object({ reviewId: z.string().uuid(), agentId: id }).strict(), output: lifecycleRowSchema });
export const readLifecycleReview = defineRpc({ name: "sessions.lifecycle-review", input: z.object({ reviewId: z.string().uuid().optional() }).strict(), output: lifecycleReviewSchema.nullable() });
export const probeSessionAvailability = defineRpc({ name: "sessions.availability", input: z.object({ agentIds: z.array(id).max(100) }).strict(),
  output: z.record(z.string(), availabilitySchema) });
export const readSessionDispositions = defineRpc({ name: "sessions.lifecycle-dispositions", input: z.object({ agentIds: z.array(id).max(100) }).strict(), output: z.record(z.string(), availabilitySchema) });

/**
 * Public surfaces this operation needs but does not own. The boundary is data so refusals stay exact and testable;
 * it is never authority to invent a replacement writer.
 */
export const TAKEOVER_EXTERNAL_BOUNDARIES = {
  schedule: {
    publicRead: "a complete host schedule/ownership inventory",
    missing: ["an atomic retarget bound to the reviewed schedule revision", "in-flight run settlement", "successor ownership readback"],
  },
  governed: {
    writers: ["Core Team route apply", "delivery seat replacement"],
    missing: ["a handoff summary", "a bootstrap acknowledgement"],
  },
} as const;
/** Take over: one session is reviewed, then confirmed by its review ID, revision and the digest the server issued. */
export const TAKEOVER_PROMPT = "The operator confirmed this takeover. Continue the task from the handoff summary in this session: read it, the conversation and the current workspace first, and keep the task scope, checkout, supervisor and permission boundaries. If context, authority or a dependency is missing, report it and wait. Do not create another session.";
/** What every ordinary takeover states before its first effect; the host capability discloses the same list. */
export const TAKEOVER_LIMITS = [
  "Archive and creation are two host calls; no host primitive makes them one step.",
  "A running session is stopped through the server-configured public Paseo CLI before its final summary is captured and it is archived. The stop is verified by reading the same host and source; if it remains active or its identity changes, nothing is archived or created.",
  "Schedules and heartbeats are readable only through a qualified host schedules.forAgent or a complete, host-qualified public host-connector inventory. The installed public CLI schedule list filters new-agent rows and is never used as no-wake proof; without a complete read, retirement is blocked.",
  "Managed children are detached through the public API and continue independently; the API exposes no re-parenting primitive.",
  "The source's listed model settings are carried to the successor and revalidated against its model; a setting the target no longer offers refuses the takeover instead of being silently dropped.",
  "The source snapshot exposes model settings but not MCP server or provider-option configuration; the successor receives the host's normal connector set and unread source connectors are not copied or invented.",
  "The successor's first message is the saved handoff summary, not the whole conversation; continuation is observed on its timeline.",
  "Check may perform the first continuation dispatch after a reconciled creation. Once a release intent is persisted, Check only reads: a missing delivery stays unknown and is never blindly resent.",
];
const digestSchema = z.string().regex(/^[a-f0-9]{64}$/);
export const takeoverBriefRefSchema = z.object({ path: z.string().min(1), sha256: z.string().regex(/^[a-f0-9]{64}$/) }).strict();
export const takeoverReviewInputSchema = z.object({
  agentId: id,
  /** Script-prepared context; hash and contents are pinned before any retirement. */
  preparedBrief: takeoverBriefRefSchema.optional(),
  /** Revises this review at its current revision; both absent opens a new review. */
  reviewId: z.string().uuid().optional(),
  revision: z.number().int().positive().optional(),
  /** One of the review's offered profiles, for a session with no recorded chain; null keeps the earlier choice. */
  bindingKey: z.string().max(400).nullable(),
  /** A model chosen for this takeover only, instead of the chain's first available route; null or absent keeps Auto. */
  route: z.object({ provider: id, model: id, effort: id.nullable(), featureValues: featureValuesSchema.optional() }).strict().nullable().optional(),
}).strict().refine((input) => (input.reviewId === undefined) === (input.revision === undefined), "A revision names its review.");
export type TakeoverReviewInput = z.infer<typeof takeoverReviewInputSchema>;
const routeViewSchema = z.object({ account: z.string(), model: z.string(), effort: z.string().nullable(), vendor: z.string(), step: z.number(), subStep: z.number() }).strict();
export const takeoverReviewSchema = z.object({
  reviewId: z.string().uuid(), revision: z.number().int().positive(), agentId: id, checkedAt: z.string(),
  /** ready: confirmable with `digest`. choose: pick one of the offered profiles. blocked: no route in the chain has fresh
   *  positive evidence. refused: the owner refuses, in `message`. unknown: something needed could not be read. */
  state: z.enum(["ready", "choose", "blocked", "refused", "unknown"]),
  message: z.string(),
  predecessor: z.object({ title: z.string(), workspaceId: id.nullable(), cwd: z.string(), provider: z.string(), model: z.string().nullable(), effort: z.string().nullable(), mode: z.string().nullable(), status: z.string() }).strict().nullable(),
  ownership: z.enum(["ordinary", "created", "orchestrated", "governed"]).nullable(),
  /** The configured chain the session takes over with: recorded for it, or chosen here from the server's choices. */
  association: z.object({ bindingKey: z.string().nullable(), source: z.enum(["recorded", "chosen"]).nullable(), choices: z.array(z.object({ bindingKey: z.string(), chain: z.string() }).strict()) }).strict(),
  route: z.object({ policyHash: z.string(), bindingKey: z.string(), chain: z.string(), selected: routeViewSchema.nullable(), considered: z.array(routeViewSchema.extend({ excluded: z.string().nullable() })) }).strict().nullable(),
  /** The one creation request a confirmation dispatches; its ID is stable for the life of the review. */
  successor: z.object({ requestId: z.string().uuid(), title: z.string(), workspaceId: id, cwd: z.string(), provider: z.string(), model: z.string(), effort: z.string().nullable(), mode: z.string().nullable(), persona: z.string().nullable() }).strict().nullable(),
  /** What this takeover cannot do on this host, stated before confirmation. */
  limits: z.array(z.string()),
  /** Opaque; present only when ready. Apply sends it back unchanged; the client never computes it. */
  digest: digestSchema.nullable(),
}).strict();
export type TakeoverReview = z.infer<typeof takeoverReviewSchema>;
/**
 * - `refused`: nothing was done; a fresh review may follow.
 * - `uncertain`: an effect was sent and is not confirmed. Only Check runs next; nothing is resent.
 * - `partial`: the predecessor was archived and no verified successor holds its task.
 * - `succeeded`: `successor` names the verified session that now holds the task.
 */
export const takeoverOutcomeSchema = z.object({
  reviewId: z.string().uuid(), revision: z.number().int().positive(), agentId: id,
  state: z.enum(["refused", "uncertain", "partial", "succeeded"]),
  phase: z.enum(["preflight", "stop", "preserve", "intent", "archive", "create", "verify", "release", "done"]),
  /** The ordinary operation stage: preparing, retiring, creating, delivering or complete; failures keep their stage. */
  stage: z.enum(["preparing", "retiring", "creating", "delivering", "complete", "failed", "uncertain"]).optional(),
  message: z.string(),
  successor: z.object({ requestId: z.string().uuid(), agentId: id.nullable(), title: z.string() }).strict().nullable(),
  /** review: a fresh review may start. check: only Check may run; it reads, and may deliver the one continuation with this operation's stable ID. none: nothing here helps. */
  next: z.enum(["review", "check", "none"]),
  checkedAt: z.string(),
}).strict();
export type TakeoverOutcome = z.infer<typeof takeoverOutcomeSchema>;
export const reviewTakeover = defineRpc({ name: "sessions.takeover-review", input: takeoverReviewInputSchema, output: takeoverReviewSchema });
export const applyTakeover = defineRpc({ name: "sessions.takeover-apply", input: z.object({ reviewId: z.string().uuid(), revision: z.number().int().positive(), digest: digestSchema }).strict(), output: takeoverOutcomeSchema });
export const checkTakeover = defineRpc({ name: "sessions.takeover-check", input: z.object({ reviewId: z.string().uuid() }).strict(), output: takeoverOutcomeSchema });

/** The installed plugin exposes no authoritative finalization owner for a direct or governed session. */
export const MISSING_OWNER_ADAPTER = "no installed direct owner/finalization adapter";
/** A Ready projection is descriptive evidence only: it is never a command, permission or automatic effect. */
export const READY_IS_DESCRIPTIVE = "Ready is descriptive evidence, never a command or permission.";

export function nativeArchiveMembership(entry: { readonly id: string; readonly updatedAt: string; readonly archivedAt: string | null; readonly availability?: SessionAvailability }): { readonly archived: boolean; readonly evidence: "native" | "availability" | "none" } {
  const evidence = freshAvailability(entry);
  return (entry.archivedAt ?? null) !== null ? { archived: true, evidence: "native" } : evidence?.state === "readable" && !!evidence.archivedAt ? { archived: true, evidence: "availability" } : { archived: false, evidence: "none" };
}

export const PROJECTION_SOURCE_KINDS = ["native", "title", "label", "workspace", "project", "plan", "jira", "responsibility", "archive", "owner", "desk", "ticket", "source", "promotion"] as const;
export type ProjectionSourceKind = (typeof PROJECTION_SOURCE_KINDS)[number];
export type ProjectionFreshness = "current" | "stale" | "unknown";
/** One observation with its own source identity, revision and time; source freshness never borrows the session's updatedAt. */
export type ProjectionSource = { readonly id: string; readonly kind: ProjectionSourceKind; readonly detail: string; readonly observedAt: string | null; readonly revision: string | null; readonly fresh: ProjectionFreshness };
export type ScopeClaimKind = "epic" | "ticket" | "desk" | "workstream" | "project" | "workspace" | "role" | "adhoc";
/** verified = owner binding; placement = exact native placement; claimed = title, label, plan or captured association. */
export type ScopeBinding = "verified" | "placement" | "claimed";
export type ScopeClaim = { readonly kind: ScopeClaimKind; readonly value: string; readonly binding: ScopeBinding; readonly source: ProjectionSource };
export type DutyObservation = { readonly duty: string; readonly state: "open" | "settled" | "unknown"; readonly source: ProjectionSource };
export type HoldObservation = { readonly reason: string | null; readonly owner: string | null; readonly liftsWhen: string | null; readonly reviewBy: string | null; readonly source: ProjectionSource };
export type MilestoneObservation = { readonly state: "open" | "complete" | "unknown"; readonly source: ProjectionSource };
export type ExternalExecutorObservation = "verified" | "missing" | "circular" | "unknown";
export type OwnerReleaseObservation =
  | { readonly state: "unavailable"; readonly reason: string }
  | { readonly state: "available"; readonly owner: string; readonly sessionId: string; readonly accepted: boolean; readonly dutiesSettled: boolean; readonly descendantsSettled: boolean; readonly wakesSettled: boolean; readonly executor: ExternalExecutorObservation; readonly source: ProjectionSource };
export type OwnerResponsibilityObservation = { readonly owner: string; readonly sessionId: string; readonly role: string; readonly responsibility: string; readonly source: ProjectionSource };
export type DurableObservation = { readonly match: "exact" | "name-only"; readonly source: ProjectionSource };
export type OwnerPausedObservation = { readonly paused: boolean; readonly verified: boolean; readonly source: ProjectionSource };
export type TicketObservation = {
  readonly kind: "delivery" | "spike";
  readonly seats: readonly DutyObservation[];
  readonly pullRequest: "merged" | "closed" | "open" | "none-explicit" | "unknown";
  /** Provenance of the pull-request fact; an absent source stays unproven, never native status. */
  readonly pullRequestSource?: ProjectionSource;
  readonly escalation: "none" | "open" | "unknown";
  /** Provenance of the escalation fact; an absent source stays unproven, never native status. */
  readonly escalationSource?: ProjectionSource;
};
export type ArchiveIntentObservation = { readonly state: "none" | "unsettled" | "unknown"; readonly reviewId: string | null; readonly sessionId: string | null; readonly observedAt: string | null; readonly detail: string };
export type ProjectionConflictKind = "archived-with-active-execution" | "archived-with-pending-interaction" | "archived-with-open-obligation" | "archived-with-unsettled-intent" | "conflicting-work-claims" | "conflicting-role-claims" | "stale-or-mismatched-evidence";
export type ProjectionConflict = { readonly kind: ProjectionConflictKind; readonly detail: string; readonly sources: readonly ProjectionSource[] };
export type NativeSessionObservation = {
  readonly id: string; readonly status: string; readonly archived: boolean; readonly archivedAt: string | null;
  readonly activeTurn: boolean; readonly pendingInteraction: boolean; readonly title: string;
  readonly workspaceId: string | null; readonly projectKey: string | null; readonly provider: string; readonly model: string | null;
  /** Paseo's native record revision and the inventory read that observed it. */
  readonly updatedAt: string; readonly observedAt: string;
};
export type ProjectionCoverage = { readonly archiveInventory: "complete" | "partial" | "unknown"; readonly observedAt: string | null; readonly detail: string };
export type PromotionObservation = {
  readonly agentId: string;
  readonly epic: string;
  readonly native: { readonly workspaceId: string; readonly workspace: string; readonly title: string; readonly observedAt: string; readonly revision: string };
  readonly logical: { readonly workspace: "LSA"; readonly node: string; readonly bindingRevision: string };
  readonly source: ProjectionSource;
};
export type SessionProjectionInput = {
  readonly hostId: string;
  readonly session: NativeSessionObservation;
  readonly coverage: ProjectionCoverage;
  readonly workClaims?: readonly ScopeClaim[];
  readonly roleClaims?: readonly ScopeClaim[];
  readonly responsibilities?: readonly OwnerResponsibilityObservation[];
  readonly duties?: readonly DutyObservation[];
  readonly hold?: HoldObservation | null;
  readonly milestone?: MilestoneObservation | null;
  readonly durable?: DurableObservation | null;
  readonly ownerPaused?: OwnerPausedObservation | null;
  readonly ticket?: TicketObservation | null;
  readonly release?: OwnerReleaseObservation;
  readonly intent?: ArchiveIntentObservation;
  readonly jira?: { readonly status: string; readonly source: ProjectionSource } | null;
  readonly peer?: { readonly role: string; readonly shutdownObservedAt: string | null; readonly source: ProjectionSource } | null;
  readonly evidence?: readonly ProjectionSource[];
  /** Conflicts the caller observed in exact evidence, such as a read for another native identity. */
  readonly evidenceConflicts?: readonly ProjectionConflict[];
  readonly promotion?: PromotionObservation | null;
  readonly receiptObservedAt?: string | null;
  readonly now?: string;
};
export type RetirementDisposition = "retain" | "ready" | "unknown";
export type SessionProjection = {
  readonly hostId: string;
  readonly sessionId: string;
  readonly nativeRevision: string;
  readonly observedAt: string;
  /** Native lifecycle and archive membership stay their own facts, apart from the retirement disposition. */
  readonly native: { readonly status: string; readonly archiveMembership: "archived" | "unarchived" };
  readonly scope: { readonly placement: ScopeClaim | null; readonly work: readonly ScopeClaim[]; readonly claims: readonly ScopeClaim[] };
  readonly promotion: PromotionObservation | null;
  readonly responsibility: { readonly status: "verified" | "unknown"; readonly role: string | null; readonly owner: string | null; readonly duty: string | null; readonly claimedRole: string | null; readonly evidence: readonly ProjectionSource[]; readonly missing: readonly string[] };
  readonly retirement: { readonly disposition: RetirementDisposition; readonly reason: string; readonly evidence: readonly ProjectionSource[]; readonly missing: readonly string[]; readonly attention: boolean };
  readonly intent: ArchiveIntentObservation;
  readonly sources: readonly ProjectionSource[];
  readonly freshness: { readonly nativeObservedAt: string; readonly inventoryObservedAt: string | null; readonly receiptObservedAt: string | null };
  readonly conflicts: readonly ProjectionConflict[];
  readonly coverage: ProjectionCoverage["archiveInventory"];
};

const HARD_CONFLICTS: readonly ProjectionConflictKind[] = ["conflicting-work-claims", "conflicting-role-claims", "stale-or-mismatched-evidence"];
const UNKNOWN_INTENT: ArchiveIntentObservation = { state: "unknown", reviewId: null, sessionId: null, observedAt: null, detail: "Archive receipts were not read." };
function uniqueSources(sources: readonly ProjectionSource[]): ProjectionSource[] {
  const byId = new Map<string, ProjectionSource>();
  for (const source of sources) if (!byId.has(source.id)) byId.set(source.id, source);
  return [...byId.values()];
}
function claimConflicts(claims: readonly ScopeClaim[], role: boolean): ProjectionConflict[] {
  const groups = new Map<string, ScopeClaim[]>();
  for (const claim of claims) {
    const group = groups.get(claim.kind) ?? [];
    group.push(claim);
    groups.set(claim.kind, group);
  }
  const conflicts: ProjectionConflict[] = [];
  for (const [kind, group] of groups) {
    const values = [...new Set(group.map((claim) => claim.value.trim().toLocaleLowerCase()).filter(Boolean))];
    if (values.length > 1) conflicts.push({ kind: role ? "conflicting-role-claims" : "conflicting-work-claims", detail: `${role ? "Role" : "Work"} claims disagree for ${kind}: ${group.map((claim) => `${claim.value} (${claim.source.kind})`).join("; ")}. A claim is not an owner binding.`, sources: group.map((claim) => claim.source) });
  }
  return conflicts;
}
/** Deterministic and read-only: no input here can archive, resume, send or create anything. */
export function deriveSessionProjection(input: SessionProjectionInput): SessionProjection {
  const session = input.session, now = input.now ?? session.observedAt;
  const work = [...(input.workClaims ?? [])], roles = [...(input.roleClaims ?? [])];
  const promotion = input.promotion && input.promotion.agentId === session.id ? input.promotion : null;
  if (promotion) work.push({ kind: "epic", value: promotion.epic, binding: promotion.source.fresh === "current" ? "verified" : "claimed", source: promotion.source });
  const intent = input.intent ?? { ...UNKNOWN_INTENT, sessionId: session.id }, coverage = input.coverage;
  const nativeSource: ProjectionSource = { id: `native:${session.id}`, kind: "native", detail: `native ${session.status}; ${session.archived ? "archived" : "not archived"}`, observedAt: session.observedAt, revision: session.updatedAt, fresh: "current" };
  const conflicts: ProjectionConflict[] = [...(input.evidenceConflicts ?? []), ...claimConflicts(work, false), ...claimConflicts(roles, true)];
  if (promotion && promotion.source.fresh !== "current") conflicts.push({ kind: "stale-or-mismatched-evidence",
    detail: "The logical promotion is not a current verified canonical binding; it remains historical evidence only.", sources: [promotion.source] });
  const blocking: Array<{ readonly reason: string; readonly source: ProjectionSource }> = [];
  const missing: string[] = [];
  const evidence: ProjectionSource[] = [nativeSource];
  let attention = false;

  const staleBindings = [...work, ...roles].filter((claim) => claim.binding !== "claimed" && claim.source.fresh !== "current");
  if (staleBindings.length) conflicts.push({ kind: "stale-or-mismatched-evidence", detail: "A verified placement or binding is stale or was read for another identity.", sources: staleBindings.map((claim) => claim.source) });

  const currentEvidence = (source: ProjectionSource) => source.fresh === "current" && source.observedAt !== null && source.revision !== null;
  const settlement = (label: string, source: ProjectionSource): boolean => {
    if (currentEvidence(source)) return true;
    missing.push(`${label} settlement evidence is stale or incomplete.`);
    attention = true;
    return false;
  };
  const responsibilities = [...(input.responsibilities ?? [])];
  const validatedResponsibilities = responsibilities.filter((entry) => entry.sessionId === session.id && currentEvidence(entry.source) && entry.owner.trim().length > 0 && entry.role.trim().length > 0);
  const incompleteResponsibilities = responsibilities.filter((entry) => !validatedResponsibilities.includes(entry));
  if (incompleteResponsibilities.length) { missing.push("Responsibility evidence is stale, incomplete or names another session; it cannot verify responsibility."); attention = true; }
  const roleOwners = new Map<string, Set<string>>();
  for (const entry of validatedResponsibilities) {
    const role = entry.role.trim();
    const owners = roleOwners.get(role) ?? new Set<string>();
    owners.add(entry.owner.trim());
    roleOwners.set(role, owners);
  }
  const conflictingRoles = [...roleOwners].filter(([, owners]) => owners.size > 1);
  if ([...roleOwners.keys()].length > 1) conflicts.push({ kind: "conflicting-role-claims", detail: `Owner sources disagree about the role: ${[...roleOwners.keys()].join(", ")}.`, sources: validatedResponsibilities.map((entry) => entry.source) });
  for (const [role, owners] of conflictingRoles) conflicts.push({ kind: "conflicting-role-claims", detail: `Owner sources disagree about the owner for role ${role}: ${[...owners].join(", ")}.`, sources: validatedResponsibilities.filter((entry) => entry.role.trim() === role).map((entry) => entry.source) });
  const verifiedResponsibility = responsibilities.length > 0 && incompleteResponsibilities.length === 0 && conflictingRoles.length === 0 && [...roleOwners.keys()].length === 1 ? validatedResponsibilities[0] ?? null : null;

  const obligations: Array<{ readonly reason: string; readonly source: ProjectionSource }> = [];
  const execution = session.status === "running" || session.activeTurn;
  const block = (reason: string, source: ProjectionSource) => { blocking.push({ reason, source }); obligations.push({ reason, source }); };
  if (execution) blocking.push({ reason: session.activeTurn ? "Native active execution is present." : "Native running status is present.", source: nativeSource });
  if (session.pendingInteraction) blocking.push({ reason: "A native pending interaction is present.", source: nativeSource });
  for (const duty of input.duties ?? []) {
    evidence.push(duty.source);
    if (duty.state === "open") block(`Open duty: ${duty.duty}.`, duty.source);
    else if (duty.state === "settled") settlement(`Duty (${duty.duty})`, duty.source);
    else { missing.push(`Duty state is unknown: ${duty.duty}.`); attention = true; }
  }
  if (input.hold) {
    const hold = input.hold, reviewAt = hold.reviewBy ? Date.parse(hold.reviewBy) : NaN;
    evidence.push(hold.source);
    const incomplete = !hold.reason?.trim() || !hold.owner?.trim() || !hold.liftsWhen?.trim() || !hold.reviewBy?.trim();
    if (incomplete || Number.isNaN(reviewAt)) { missing.push("Hold metadata is missing or malformed; hold fields are reason, owner, lifts_when and review_by. Attention required, never release."); attention = true; }
    else if (reviewAt <= Date.parse(now)) { missing.push(`Hold review date passed (${hold.reviewBy}); attention required, never release.`); attention = true; }
    else block(`Hold: ${hold.reason} until ${hold.liftsWhen} with owner ${hold.owner}.`, hold.source);
  }
  if (input.milestone) {
    evidence.push(input.milestone.source);
    if (input.milestone.state === "open") block("An authorized next milestone remains open.", input.milestone.source);
    else if (input.milestone.state === "unknown") { missing.push("The next milestone state is unknown."); attention = true; }
    else settlement("Milestone completion", input.milestone.source);
  }
  if (input.durable) {
    evidence.push(input.durable.source);
    if (input.durable.match === "exact") block("An exact durable desk or workspace placement protects this session.", input.durable.source);
    else { missing.push("Durable placement is name-only, not an exact native match; no protection is established."); attention = true; }
  }
  if (input.ownerPaused?.paused) {
    evidence.push(input.ownerPaused.source);
    if (input.ownerPaused.verified) block("This session's owner is intentionally paused; no automatic effect may run.", input.ownerPaused.source);
    else { missing.push("A paused owner is claimed by name only; no exact owner match establishes that protection."); attention = true; }
  }
  if (input.ticket) {
    const ticket = input.ticket;
    if (ticket.kind === "delivery" && ticket.seats.length === 0) { missing.push("Ticket seats were not read."); attention = true; }
    for (const seat of ticket.seats) {
      evidence.push(seat.source);
      if (seat.state === "open") block(`Ticket seat is not released: ${seat.duty}.`, seat.source);
      else if (seat.state === "unknown") { missing.push(`Ticket seat state is unknown: ${seat.duty}.`); attention = true; }
      else settlement(`Ticket seat (${seat.duty})`, seat.source);
    }
    const pullRequestSource = ticket.pullRequestSource ?? { id: `ticket:pull-request:${session.id}`, kind: "ticket" as const, detail: "ticket pull request fact without a separate source", observedAt: null, revision: null, fresh: "unknown" as const };
    const escalationSource = ticket.escalationSource ?? { id: `ticket:escalation:${session.id}`, kind: "ticket" as const, detail: "ticket escalation fact without a separate source", observedAt: null, revision: null, fresh: "unknown" as const };
    evidence.push(escalationSource, pullRequestSource);
    if (!ticket.pullRequestSource) { missing.push("Ticket pull request evidence has no source provenance."); attention = true; }
    if (!ticket.escalationSource) { missing.push("Ticket escalation evidence has no source provenance."); attention = true; }
    if (ticket.escalation === "open") block("An operator escalation is open for this ticket.", escalationSource);
    else if (ticket.escalation === "unknown") { missing.push("Ticket escalation state is unknown."); attention = true; }
    else settlement("Ticket escalation absence", escalationSource);
    if (ticket.pullRequest === "open") block("The ticket pull request is open.", pullRequestSource);
    else if (ticket.pullRequest === "unknown") { missing.push("Ticket pull request state is unknown."); attention = true; }
    else if (ticket.kind === "spike" && ticket.pullRequest !== "none-explicit") { missing.push("A spike must explicitly declare that it has no pull request."); attention = true; }
    else settlement("Ticket pull request", pullRequestSource);
  }
  if (input.jira) evidence.push(input.jira.source);
  if (input.peer) evidence.push(input.peer.source);

  if (intent.state === "unsettled") { missing.push(`Archive outcome is unsettled (review ${intent.reviewId ?? "unknown"}, session ${intent.sessionId ?? "unknown"}): ${intent.detail}.`); attention = true; }
  else if (intent.state === "unknown") missing.push(`Archive receipt coverage is unknown: ${intent.detail}.`);
  if (intent.observedAt) evidence.push({ id: `archive:${intent.reviewId ?? "unknown"}`, kind: "archive", detail: intent.detail, observedAt: intent.observedAt, revision: null, fresh: intent.state === "unsettled" ? "current" : "unknown" });
  if (coverage.archiveInventory !== "complete") missing.push(`Native archive inventory is ${coverage.archiveInventory}: ${coverage.detail}.`);

  if (session.archived) {
    if (execution) conflicts.push({ kind: "archived-with-active-execution", detail: "The session is in the native archive while execution is observed; membership and execution disagree.", sources: [nativeSource] });
    if (session.pendingInteraction) conflicts.push({ kind: "archived-with-pending-interaction", detail: "The session is in the native archive while a native interaction is pending.", sources: [nativeSource] });
    if (obligations.length) conflicts.push({ kind: "archived-with-open-obligation", detail: `The session is archived while obligations remain: ${obligations.map((entry) => entry.reason).join(" ")}`, sources: obligations.map((entry) => entry.source) });
    if (intent.state === "unsettled") conflicts.push({ kind: "archived-with-unsettled-intent", detail: "The session is in the native archive while an earlier archive outcome remains unsettled.", sources: [nativeSource] });
  }
  if (conflicts.some((conflict) => conflict.kind.startsWith("archived-with-"))) attention = true;

  let releaseComplete = false;
  const release = input.release ?? { state: "unavailable" as const, reason: MISSING_OWNER_ADAPTER };
  if (release.state === "unavailable") missing.push(release.reason);
  else {
    evidence.push(release.source);
    const releaseCurrent = currentEvidence(release.source), releaseOwner = release.owner.trim();
    if (!releaseCurrent) { missing.push("Release evidence is stale, incomplete or lacks an observation time and revision."); attention = true; }
    if (release.sessionId !== session.id) missing.push("Release evidence does not name this exact native session.");
    if (!releaseOwner) missing.push("Release owner is empty; it cannot verify an owner identity.");
    if (!release.accepted) missing.push("An authoritative owner has not accepted its final duties.");
    if (!release.dutiesSettled) missing.push("An authoritative owner has not reported its duties settled.");
    if (!release.descendantsSettled) missing.push("Descendant obligations are not settled.");
    if (!release.wakesSettled) missing.push("Owned wakes and callbacks are not settled.");
    if (release.executor !== "verified") missing.push(release.executor === "circular" ? "The external executor route is circular." : "A verified external executor is missing.");
    const verifiedBinding = work.find((claim) => claim.binding === "verified" && currentEvidence(claim.source)) ?? null;
    if (!verifiedBinding) missing.push("No current verified work binding supports release.");
    if (!verifiedResponsibility) missing.push("No current verified owner responsibility supports release.");
    else if (releaseOwner !== verifiedResponsibility.owner.trim()) missing.push("Owner responsibility and release evidence disagree on the owner.");
    releaseComplete = releaseCurrent && release.sessionId === session.id && releaseOwner.length > 0 && release.accepted && release.dutiesSettled && release.descendantsSettled && release.wakesSettled && release.executor === "verified"
      && verifiedBinding !== null && verifiedResponsibility !== null && releaseOwner === verifiedResponsibility.owner.trim();
  }

  const uniqueMissing = [...new Set(missing)], hard = conflicts.filter((conflict) => HARD_CONFLICTS.includes(conflict.kind));
  const releaseSource = release.state === "available" ? [release.source] : [];
  const retirementEvidence = uniqueSources([nativeSource, ...releaseSource, ...blocking.map((entry) => entry.source), ...evidence.filter((source) => source.kind === "owner" || source.kind === "archive")]);
  let retirement: SessionProjection["retirement"];
  if (hard.length) retirement = { disposition: "unknown", reason: `Conflicting evidence prevents a release decision: ${hard.map((conflict) => conflict.detail).join(" ")}`, evidence: uniqueSources([...retirementEvidence, ...hard.flatMap((conflict) => conflict.sources)]), missing: uniqueMissing, attention: true };
  else if (blocking.length) retirement = { disposition: "retain", reason: blocking.map((entry) => entry.reason).join(" "), evidence: retirementEvidence, missing: uniqueMissing, attention };
  else if (!releaseComplete || attention || uniqueMissing.length) retirement = { disposition: "unknown", reason: `Release evidence is incomplete: ${uniqueMissing.join(" ")}`, evidence: retirementEvidence, missing: uniqueMissing, attention };
  else retirement = { disposition: "ready", reason: `An authoritative owner supplies every current release, binding, descendant and wake fact. ${READY_IS_DESCRIPTIVE}`, evidence: retirementEvidence, missing: uniqueMissing, attention };

  const placement = work.find((claim) => claim.kind === "project" && claim.binding === "placement") ?? work.find((claim) => claim.binding === "placement") ?? null;
  const claimedRole = roles.find((claim) => claim.value.trim().length > 0)?.value.trim() ?? null;
  const responsibilityMissing: string[] = [];
  if (responsibilities.length === 0) responsibilityMissing.push(MISSING_OWNER_ADAPTER);
  else if (incompleteResponsibilities.length) responsibilityMissing.push("Responsibility evidence is stale, incomplete or names another session; it cannot verify responsibility.");
  else if (conflictingRoles.length || [...roleOwners.keys()].length > 1) responsibilityMissing.push("Conflicting owner responsibility evidence cannot verify a responsibility.");
  else if (!verifiedResponsibility) responsibilityMissing.push("No current, consistent owner responsibility was supplied.");
  const responsibility: SessionProjection["responsibility"] = verifiedResponsibility
    ? { status: "verified", role: verifiedResponsibility.role.trim(), owner: verifiedResponsibility.owner.trim(), duty: verifiedResponsibility.responsibility, claimedRole, evidence: uniqueSources(validatedResponsibilities.map((entry) => entry.source)), missing: [] }
    : { status: "unknown", role: null, owner: null, duty: null, claimedRole, evidence: uniqueSources(responsibilities.length ? responsibilities.map((entry) => entry.source) : roles.map((claim) => claim.source)), missing: [...new Set(responsibilityMissing)] };
  return {
    hostId: input.hostId, sessionId: session.id, nativeRevision: session.updatedAt, observedAt: session.observedAt,
    native: { status: session.status, archiveMembership: session.archived ? "archived" : "unarchived" },
    scope: { placement, work: work.filter((claim) => claim.kind !== "role"), claims: work },
    promotion, responsibility, retirement, intent,
    sources: uniqueSources([...evidence, ...work.map((claim) => claim.source), ...roles.map((claim) => claim.source), ...responsibility.evidence]),
    freshness: { nativeObservedAt: session.observedAt, inventoryObservedAt: coverage.observedAt, receiptObservedAt: input.receiptObservedAt ?? null },
    conflicts, coverage: coverage.archiveInventory,
  };
}
