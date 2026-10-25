import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

/** A lead role and the way its session starts; together they select the persona and the lead workspace. */
export const CREATE_ROLES = ["LSA", "TPM"] as const;
export type CreateRole = (typeof CREATE_ROLES)[number];
export const CREATE_STARTS = ["ad-hoc", "from-jira"] as const;
export type CreateStart = (typeof CREATE_STARTS)[number];
/** The scope word for an unbound LSA; it is not a workspace-kind code. */
export const CREATE_SCOPE = "ADHOC";
export const LEAD_WORKSPACES = ["ADHOC", "LSA", "TPM"] as const;
export type LeadWorkspace = (typeof LEAD_WORKSPACES)[number];
export const TITLE_SEPARATOR = " • ";
export const TOPIC_MIN = 3;
export const TOPIC_MAX = 48;
/** The label that correlates a native session with the request that created it. */
export const CREATE_REQUEST_LABEL = "create.request";
/** The title the session or Kontor last wrote; a native rename that differs stops automation. */
export const TITLE_AUTO_LABEL = "title.auto";
/** Providers whose `systemPrompt` mapping was confirmed on the host; any other gets a bootstrap prompt. */
export const PERSONA_PROVIDERS: readonly string[] = ["claude", "codex"];
export const JIRA_KEY_PATTERN = /^ASMA-[1-9]\d*$/;

/** NFC, trimmed, whitespace runs collapsed; case kept as typed. */
export function normalizeTopic(raw: string): string {
  return raw.normalize("NFC").trim().replace(/\s+/g, " ");
}

/** Why a normalized topic is refused, or null. Refused, never escaped, so a topic cannot forge a title segment. */
export function topicProblem(topic: string): string | null {
  if (/[•·]/.test(topic)) return "A topic cannot contain • or ·, which separate title parts.";
  if (/\p{Cc}/u.test(topic)) return "A topic cannot contain control characters.";
  const length = [...topic].length;
  if (length < TOPIC_MIN) return `Type a topic of at least ${TOPIC_MIN} characters.`;
  if (length > TOPIC_MAX) return `A topic has at most ${TOPIC_MAX} characters.`;
  return null;
}

/** A Jira summary as a topic: normalized, then cut to the title budget without splitting a character. */
export function issueTopic(summary: string): string {
  return [...normalizeTopic(summary)].slice(0, TOPIC_MAX).join("").trim();
}

export function leadTitle(role: CreateRole, key: string | null, topic: string): string {
  return [role, key ?? CREATE_SCOPE, topic].join(TITLE_SEPARATOR);
}

/** Which lead workspace a role and start belong in. */
export function leadWorkspaceName(role: CreateRole, start: CreateStart): LeadWorkspace {
  return start === "ad-hoc" ? "ADHOC" : role;
}

/** The desk a native project's name makes it, `DESK • PR REVIEW` → `PR REVIEW`; null for any other project. */
export function deskName(projectName: string | null | undefined): string | null {
  const parts = (projectName ?? "").split("•");
  if (parts.length < 2 || parts[0]!.trim().toLowerCase() !== "desk") return null;
  const name = parts.slice(1).join("•").trim();
  return name || null;
}

export type DeskKind = "adam" | "tickets" | "pr-review" | "other";
/** How a desk's start list is constrained: epics on ADAM, non-epic issues on TICKETS, ad hoc elsewhere. */
export function deskKind(desk: string): DeskKind {
  const name = desk.trim().toUpperCase();
  return name === "ADAM" ? "adam" : name === "TICKETS" ? "tickets" : name === "PR REVIEW" ? "pr-review" : "other";
}

/** A desk's Adam workspace is named exactly ADAM (any case). Kept for the legacy desk read. */
export function isAdamWorkspace(name: string | null | undefined): boolean {
  return (name ?? "").trim().toUpperCase() === "ADAM";
}

/** A lead workspace is named exactly ADHOC, LSA or TPM (any case). */
export function isLeadWorkspace(name: string | null | undefined): name is LeadWorkspace {
  return (LEAD_WORKSPACES as readonly string[]).includes((name ?? "").trim().toUpperCase());
}

const id = z.string().min(1).max(200);

export const createDeskSchema = z.object({
  projectId: id, project: z.string(), desk: z.string(), kind: z.enum(["adam", "tickets", "pr-review", "other"]),
  /** The desk checkout's working directory; null only when no workspace on the desk has one. */
  cwd: z.string().nullable(),
  /** The desk's lead workspaces by kind; null when the desk does not have one yet. */
  lead: z.object({ ADHOC: id.nullable(), LSA: id.nullable(), TPM: id.nullable() }).strict(),
}).strict();
export type CreateDesk = z.infer<typeof createDeskSchema>;

export const createProviderSchema = z.object({
  id, label: z.string(), available: z.boolean(), note: z.string().nullable(), persona: z.enum(["supported", "unverified"]),
  status: z.enum(["ready", "loading", "error", "unavailable", "disabled", "unknown"]),
  models: z.array(z.object({ id, label: z.string(), efforts: z.array(z.object({ id, label: z.string() })), defaultEffort: z.string().nullable() })),
  defaultModel: z.string().nullable(),
  modes: z.array(z.object({ id, label: z.string(), dangerous: z.boolean() })),
  defaultMode: z.string().nullable(),
}).strict();
export type CreateProvider = z.infer<typeof createProviderSchema>;

/** One ASMA issue as `asma jira search` returns it; the parent is present only for a child issue. */
export const jiraIssueSchema = z.object({
  key: z.string(), summary: z.string(), type: z.string(), epic: z.boolean(),
  status: z.string(), statusCategory: z.string(), open: z.boolean(),
  parent: z.object({ key: z.string(), summary: z.string(), statusCategory: z.string(), open: z.boolean() }).strict().nullable(),
}).strict();
export type JiraIssue = z.infer<typeof jiraIssueSchema>;

/** Why an issue cannot start a lead on this desk; null when it can. */
export function deskIssueProblem(desk: Pick<CreateDesk, "project" | "kind">, issue: JiraIssue): string | null {
  if (!issue.open) return `${issue.key} is ${issue.status}; choose an open issue.`;
  if (desk.kind === "adam" && !issue.epic) return `${issue.key} is a ${issue.type}, not an epic. ${desk.project} takes epics; a non-epic issue belongs on DESK • TICKETS.`;
  if (desk.kind === "tickets" && issue.epic) return `${issue.key} is an epic. ${desk.project} takes non-epic issues; an epic belongs on DESK • ADAM.`;
  if (desk.kind === "tickets" && issue.parent?.open) return `${issue.key} is under the active epic ${issue.parent.key} (${issue.parent.statusCategory}), whose ESW delivers it. Start from ${issue.parent.key} instead.`;
  return null;
}

/**
 * The desk an issue starts on, whatever desk the dialog opened in: an epic on DESK • ADAM, a non-epic issue on
 * DESK • TICKETS. `problem` says why it cannot start; the desk is null then.
 */
export function issueDesk<D extends Pick<CreateDesk, "project" | "kind">>(desks: readonly D[], issue: JiraIssue): { desk: D | null; problem: string | null } {
  const kind = issue.epic ? "adam" : "tickets", name = issue.epic ? "DESK • ADAM" : "DESK • TICKETS";
  const desk = desks.find((entry) => entry.kind === kind) ?? null;
  const problem = deskIssueProblem({ kind, project: desk?.project ?? name }, issue)
    ?? (desk ? null : `${issue.key} starts on ${name}, which has no checkout on this host. Create its lead workspaces through adoption first.`);
  return problem ? { desk: null, problem } : { desk, problem: null };
}

/**
 * - `verified`: the host's readback of the new session matches every requested field.
 * - `differences`: created; the readback differs in the listed fields.
 * - `unknown`: dispatched, but neither a reply nor a readback confirmed the effect. Do not retry; Check.
 * - `refused`: nothing was dispatched.
 */
export const createOutcomeSchema = z.object({
  requestId: z.string().uuid(),
  state: z.enum(["verified", "differences", "unknown", "refused"]),
  title: z.string(),
  agentId: id.nullable(),
  workspaceId: id.nullable(),
  message: z.string(),
  differences: z.array(z.string()),
  persona: z.enum(["supported", "unverified"]),
  checkedAt: z.string(),
}).strict();
export type CreateOutcome = z.infer<typeof createOutcomeSchema>;

export const createOptionsSchema = z.object({
  enabled: z.boolean(),
  /** Why creation is off; null when it is on. */
  disabledReason: z.string().nullable(),
  desks: z.array(createDeskSchema),
  /** The desk whose working directory scoped this provider catalog; null when no desk is available. */
  catalogProjectId: id.nullable(),
  providers: z.array(createProviderSchema),
  /** The host's agent profile for the chosen role, the form's defaults when present. */
  profile: z.object({ name: z.string(), provider: z.string(), model: z.string().nullable(), modeId: z.string().nullable(), effort: z.string().nullable() }).nullable(),
  /** The open issues matching a From Jira ticket search, of every type; `issueDesk` says where each starts. Empty for any other start. */
  issues: z.array(jiraIssueSchema),
  /** Why the issue list could not be read; null when it was or was not requested. */
  jiraProblem: z.string().nullable(),
  /** An earlier request whose effect is not settled; it blocks a new creation until a Check settles it. */
  pending: createOutcomeSchema.nullable(),
  /** Read problems that do not stop the form, such as an unreadable profile list. */
  notes: z.array(z.string()),
}).strict();
export type CreateOptions = z.infer<typeof createOptionsSchema>;

export const createScopeSchema = z.object({
  projectId: id.optional(),
  /** The workspace the dialog was opened in; the server resolves its desk when projectId is absent. */
  workspaceId: id.optional(),
  role: z.enum(CREATE_ROLES).optional(),
  start: z.enum(CREATE_STARTS).optional(),
  /** The From Jira search: an ASMA key or words; empty lists the newest open issues. */
  issueKey: z.string().max(200).optional(),
}).strict();
export type CreateScope = z.infer<typeof createScopeSchema>;

export const createRequestSchema = z.object({
  requestId: z.string().uuid(),
  role: z.enum(CREATE_ROLES),
  start: z.enum(CREATE_STARTS),
  projectId: id,
  /** The title topic. A From Jira start without one takes the issue summary; the key is never part of it. */
  topic: z.string().max(400).optional(),
  issueKey: z.string().max(200).nullable().optional(),
  /** A source session whose handoff summary the new lead reads before its first task. */
  sourceAgentId: z.string().max(200).nullable().optional(),
  provider: id,
  model: id,
  effort: id.nullable(),
  modeId: id.nullable(),
}).strict();
export type CreateRequest = z.infer<typeof createRequestSchema>;

export const readCreateOptions = defineRpc({ name: "sessions.create-options", input: createScopeSchema, output: createOptionsSchema });
export const executeCreate = defineRpc({ name: "sessions.create", input: createRequestSchema, output: createOutcomeSchema });
export const checkCreate = defineRpc({ name: "sessions.create-check", input: z.object({ requestId: z.string().uuid() }).strict(), output: createOutcomeSchema });
