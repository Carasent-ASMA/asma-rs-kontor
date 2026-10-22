import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

/**
 * One bounded creation: an ad-hoc LSA Adam tab in a desk's `ADAM` workspace, titled
 * `LSA • ADHOC • <topic>`. The role and scope are fixed; the session promotes its own title to a
 * confirmed epic key later (see the persona's rename rules), because a plugin cannot rename an agent.
 */
export const CREATE_ROLE = "LSA";
export const CREATE_SCOPE = "ADHOC";
export const TITLE_SEPARATOR = " • ";
export const TOPIC_MIN = 3;
export const TOPIC_MAX = 48;
/** The label that correlates a native session with the request that created it. */
export const CREATE_REQUEST_LABEL = "create.request";
/** The title the session or Kontor last wrote; a native rename that differs stops automation. */
export const TITLE_AUTO_LABEL = "title.auto";
/** Providers whose `systemPrompt` mapping was confirmed on the host; any other is unverified. */
export const PERSONA_PROVIDERS: readonly string[] = ["claude", "codex"];

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

export function adhocTitle(topic: string): string {
  return [CREATE_ROLE, CREATE_SCOPE, topic].join(TITLE_SEPARATOR);
}

/** The desk a native project's name makes it, `DESK • PR REVIEW` → `PR REVIEW`; null for any other project. */
export function deskName(projectName: string | null | undefined): string | null {
  const parts = (projectName ?? "").split("•");
  if (parts.length < 2 || parts[0]!.trim().toLowerCase() !== "desk") return null;
  const name = parts.slice(1).join("•").trim();
  return name || null;
}

/** A desk's Adam workspace is named exactly ADAM (any case). */
export function isAdamWorkspace(name: string | null | undefined): boolean {
  return (name ?? "").trim().toUpperCase() === "ADAM";
}

const id = z.string().min(1).max(200);

export const createDeskSchema = z.object({ workspaceId: id, projectId: id, desk: z.string(), project: z.string() }).strict();
export const createProviderSchema = z.object({
  id, label: z.string(), available: z.boolean(), note: z.string().nullable(), persona: z.enum(["supported", "unverified"]),
  models: z.array(z.object({ id, label: z.string(), efforts: z.array(z.object({ id, label: z.string() })), defaultEffort: z.string().nullable() })),
  defaultModel: z.string().nullable(),
  modes: z.array(z.object({ id, label: z.string(), dangerous: z.boolean() })),
  defaultMode: z.string().nullable(),
}).strict();
export type CreateProvider = z.infer<typeof createProviderSchema>;

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
  providers: z.array(createProviderSchema),
  /** The host's `LSA` agent profile, the form's defaults when present. */
  profile: z.object({ name: z.string(), provider: z.string(), model: z.string().nullable(), modeId: z.string().nullable(), effort: z.string().nullable() }).nullable(),
  /** An earlier request whose effect is not settled; it blocks a new creation until a Check settles it. */
  pending: createOutcomeSchema.nullable(),
  /** Read problems that do not stop the form, such as an unreadable profile list. */
  notes: z.array(z.string()),
}).strict();
export type CreateOptions = z.infer<typeof createOptionsSchema>;

export const createRequestSchema = z.object({
  requestId: z.string().uuid(),
  topic: z.string().max(400),
  workspaceId: id,
  provider: id,
  model: id,
  effort: id.nullable(),
  modeId: id.nullable(),
}).strict();
export type CreateRequest = z.infer<typeof createRequestSchema>;

export const readCreateOptions = defineRpc({ name: "sessions.create-options", input: z.object({}).strict(), output: createOptionsSchema });
export const executeCreate = defineRpc({ name: "sessions.create", input: createRequestSchema, output: createOutcomeSchema });
export const checkCreate = defineRpc({ name: "sessions.create-check", input: z.object({ requestId: z.string().uuid() }).strict(), output: createOutcomeSchema });
