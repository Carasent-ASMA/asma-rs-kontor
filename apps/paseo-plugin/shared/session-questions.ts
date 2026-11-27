import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

/**
 * One operator action settles several host permission requests. The host owns
 * the requests and the responses; this module owns only the deterministic
 * recommendation, the review item and its content pin. It never carries a
 * client-supplied response: the server recomputes every answer from the
 * request it just re-read.
 */

const id = z.string().min(1).max(400);
const digest = z.string().regex(/^[a-f0-9]{64}$/);

export const permissionActionSchema = z.object({
  id: id,
  label: z.string(),
  behavior: z.enum(["allow", "deny"]),
  variant: z.enum(["primary", "secondary", "danger"]).optional(),
  intent: z.string().optional(),
});
export type PermissionAction = z.infer<typeof permissionActionSchema>;

export type PermissionRequestLike = {
  readonly kind: string;
  readonly name: string;
  readonly title?: string;
  readonly input?: Readonly<Record<string, unknown>>;
  readonly actions?: readonly PermissionAction[];
};

/** The only response shape this feature sends: allow, with an action id or updated input. */
export const permissionResponseSchema = z.object({
  behavior: z.literal("allow"),
  selectedActionId: z.string().optional(),
  updatedInput: z.record(z.string(), z.unknown()).optional(),
}).strict();
export type PermissionResponse = z.infer<typeof permissionResponseSchema>;

export const sessionQuestionItemSchema = z.object({
  agentId: id,
  agentTitle: z.string(),
  agentStatus: z.string(),
  requestId: id,
  kind: z.string(),
  title: z.string(),
  /** The exact human sentence the review shows for a supported request. */
  label: z.string(),
  /** Exactly what the server would send; null when the request needs the operator. */
  response: permissionResponseSchema.nullable(),
  /** SHA-256 over the request content the review pinned. */
  hash: digest,
  supported: z.boolean(),
  /** Why an unsupported request needs the operator; null when supported. */
  reason: z.string().nullable(),
}).strict();
export type SessionQuestionItem = z.infer<typeof sessionQuestionItemSchema>;

export const pendingQuestionsSchema = z.object({ items: z.array(sessionQuestionItemSchema) }).strict();
export type PendingQuestions = z.infer<typeof pendingQuestionsSchema>;

export const acceptQuestionsItemSchema = z.object({ agentId: id, requestId: id, hash: digest }).strict();
export type AcceptQuestionsItem = z.infer<typeof acceptQuestionsItemSchema>;

export const acceptQuestionsInputSchema = z.object({ items: z.array(acceptQuestionsItemSchema) }).strict();
export type AcceptQuestionsInput = z.infer<typeof acceptQuestionsInputSchema>;

export const sessionQuestionOutcomeSchema = z.enum(["answered", "already-answered", "changed", "unsupported", "uncertain"]);
export type SessionQuestionOutcome = z.infer<typeof sessionQuestionOutcomeSchema>;

export const sessionQuestionResultSchema = z.object({
  agentId: id,
  requestId: id,
  outcome: sessionQuestionOutcomeSchema,
  message: z.string().nullable(),
}).strict();
export type SessionQuestionResult = z.infer<typeof sessionQuestionResultSchema>;

export const acceptQuestionsOutputSchema = z.object({ results: z.array(sessionQuestionResultSchema) }).strict();
export type AcceptQuestionsOutput = z.infer<typeof acceptQuestionsOutputSchema>;

export const readPendingQuestions = defineRpc({
  name: "sessions.pending-questions",
  input: z.object({}).strict(),
  output: pendingQuestionsSchema,
});

export const acceptPendingQuestions = defineRpc({
  name: "sessions.accept-questions",
  input: acceptQuestionsInputSchema,
  output: acceptQuestionsOutputSchema,
});

export const NEEDS_OPERATOR = "Needs the operator";

export type PermissionRecommendation = {
  readonly supported: boolean;
  readonly response: PermissionResponse | null;
  readonly label: string;
  readonly reason: string | null;
};

const unsupported = (reason: string): PermissionRecommendation => ({
  supported: false,
  response: null,
  label: NEEDS_OPERATOR,
  reason,
});

const questionOptionSchema = z.object({
  label: z.string(),
  description: z.string().optional(),
}).passthrough();

const questionSchema = z.object({
  header: z.string(),
  question: z.string(),
  options: z.array(questionOptionSchema),
  multiSelect: z.boolean().optional(),
  allowOther: z.boolean().optional(),
  allowEmpty: z.boolean().optional(),
}).passthrough();

const questionsSchema = z.array(questionSchema);

const recommendedLabel = /\brecommended\b/i;
const subjectOf = (request: PermissionRequestLike) =>
  request.name.trim() || request.title?.trim() || "this request";

/**
 * Fixed recommendation: a written question chooses the option whose label says
 * "recommended" (every recommended option on a multi-select), else its first
 * option; an action request chooses the first primary allow action, else the
 * first allow action that is not dangerous. Anything a rule cannot answer is
 * listed for the operator and never sent.
 */
export function recommendPermission(request: PermissionRequestLike): PermissionRecommendation {
  if (request.kind === "question") {
    const parsed = questionsSchema.safeParse((request.input as { questions?: unknown } | undefined)?.questions);
    if (!parsed.success) {
      return unsupported("The question content could not be read; the operator must answer it.");
    }
    if (parsed.data.length === 0) {
      return unsupported("The question request has no questions; the operator must answer it.");
    }
    const answers: Record<string, string> = {};
    const parts: string[] = [];
    for (const question of parsed.data) {
      if (question.options.length === 0) {
        return unsupported(`${question.header}: a written answer is required.`);
      }
      const recommended = question.options.filter((option) => recommendedLabel.test(option.label));
      const chosen = recommended.length
        ? (question.multiSelect ? recommended : recommended.slice(0, 1))
        : [question.options[0]!];
      if (Object.prototype.hasOwnProperty.call(answers, question.header)) {
        return unsupported(`${question.header}: two questions share this header, so one answer cannot name both.`);
      }
      const answer = chosen.map((option) => option.label).join(", ");
      answers[question.header] = answer;
      parts.push(`${question.header}: ${answer}`);
    }
    return {
      supported: true,
      response: { behavior: "allow", updatedInput: { ...(request.input ?? {}), answers } },
      label: parts.join(" · "),
      reason: null,
    };
  }

  const actions = request.actions ?? [];
  if (actions.length === 0) {
    return { supported: true, response: { behavior: "allow" }, label: `Allow · ${subjectOf(request)}`, reason: null };
  }
  const chosen = actions.find((action) => action.behavior === "allow" && action.variant === "primary")
    ?? actions.find((action) => action.behavior === "allow" && action.variant !== "danger");
  if (!chosen) {
    return unsupported(actions.some((action) => action.behavior === "allow")
      ? "Only a dangerous allow action is offered; the operator must choose it."
      : "No allow action is offered; the operator must answer it.");
  }
  return {
    supported: true,
    response: { behavior: "allow", selectedActionId: chosen.id },
    label: `${chosen.label} · ${subjectOf(request)}`,
    reason: null,
  };
}

/** Stable JSON for the content pin: object keys sorted at every depth, arrays kept in order. */
export function stableJson(value: unknown): string {
  return JSON.stringify(canonical(value));
}

function canonical(value: unknown): unknown {
  if (Array.isArray(value)) {
    return value.map(canonical);
  }
  if (value !== null && typeof value === "object") {
    const record = value as Record<string, unknown>;
    return Object.fromEntries(Object.keys(record).sort().map((key) => [key, canonical(record[key])]));
  }
  return value;
}

/** The exact content a review pins: kind, name, title, input and actions. */
export function questionHashInput(request: PermissionRequestLike): string {
  return stableJson({
    kind: request.kind,
    name: request.name,
    title: request.title ?? null,
    input: request.input ?? null,
    actions: request.actions ?? null,
  });
}

/** SHA-256 of the pinned request content, as lower-case hex. Bundled for the client, so no node:crypto. */
export function questionRequestHash(request: PermissionRequestLike): string {
  return sha256Hex(questionHashInput(request));
}

/** One review row's identity in a result map. */
export function questionKey(item: Pick<SessionQuestionItem, "agentId" | "requestId">): string {
  return `${item.agentId}\u0000${item.requestId}`;
}

const SHA256_K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

const rotr = (value: number, bits: number) => ((value >>> bits) | (value << (32 - bits))) >>> 0;

function utf8(value: string): Uint8Array {
  const bytes: number[] = [];
  for (let index = 0; index < value.length; index++) {
    let code = value.charCodeAt(index);
    if (code >= 0xd800 && code <= 0xdbff && index + 1 < value.length) {
      const low = value.charCodeAt(index + 1);
      if (low >= 0xdc00 && low <= 0xdfff) {
        code = 0x10000 + ((code - 0xd800) << 10) + (low - 0xdc00);
        index += 1;
      }
    }
    if (code >= 0xd800 && code <= 0xdfff) {
      code = 0xfffd;
    }
    if (code < 0x80) {
      bytes.push(code);
    } else if (code < 0x800) {
      bytes.push(0xc0 | (code >> 6), 0x80 | (code & 0x3f));
    } else if (code < 0x10000) {
      bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    } else {
      bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 0x3f), 0x80 | ((code >> 6) & 0x3f), 0x80 | (code & 0x3f));
    }
  }
  return new Uint8Array(bytes);
}

function sha256Hex(value: string): string {
  const message = utf8(value);
  const bitLength = message.length * 8;
  const paddedLength = Math.ceil((message.length + 9) / 64) * 64;
  const bytes = new Uint8Array(paddedLength);
  bytes.set(message);
  bytes[message.length] = 0x80;
  const high = Math.floor(bitLength / 0x1_0000_0000);
  const low = bitLength >>> 0;
  const lengthAt = paddedLength - 8;
  bytes[lengthAt] = high >>> 24;
  bytes[lengthAt + 1] = (high >>> 16) & 0xff;
  bytes[lengthAt + 2] = (high >>> 8) & 0xff;
  bytes[lengthAt + 3] = high & 0xff;
  bytes[lengthAt + 4] = low >>> 24;
  bytes[lengthAt + 5] = (low >>> 16) & 0xff;
  bytes[lengthAt + 6] = (low >>> 8) & 0xff;
  bytes[lengthAt + 7] = low & 0xff;

  const state = new Uint32Array([
    0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
  ]);
  const schedule = new Uint32Array(64);
  for (let offset = 0; offset < bytes.length; offset += 64) {
    for (let index = 0; index < 16; index++) {
      const at = offset + index * 4;
      schedule[index] = ((bytes[at]! << 24) | (bytes[at + 1]! << 16) | (bytes[at + 2]! << 8) | bytes[at + 3]!) >>> 0;
    }
    for (let index = 16; index < 64; index++) {
      const s0 = rotr(schedule[index - 15]!, 7) ^ rotr(schedule[index - 15]!, 18) ^ (schedule[index - 15]! >>> 3);
      const s1 = rotr(schedule[index - 2]!, 17) ^ rotr(schedule[index - 2]!, 19) ^ (schedule[index - 2]! >>> 10);
      schedule[index] = (schedule[index - 16]! + s0 + schedule[index - 7]! + s1) >>> 0;
    }
    let a = state[0]!, b = state[1]!, c = state[2]!, d = state[3]!, e = state[4]!, f = state[5]!, g = state[6]!, h = state[7]!;
    for (let index = 0; index < 64; index++) {
      const sum1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const choose = (e & f) ^ (~e & g);
      const temp1 = (h + sum1 + choose + SHA256_K[index]! + schedule[index]!) >>> 0;
      const sum0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const majority = (a & b) ^ (a & c) ^ (b & c);
      const temp2 = (sum0 + majority) >>> 0;
      h = g;
      g = f;
      f = e;
      e = (d + temp1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (temp1 + temp2) >>> 0;
    }
    state[0] = (state[0]! + a) >>> 0;
    state[1] = (state[1]! + b) >>> 0;
    state[2] = (state[2]! + c) >>> 0;
    state[3] = (state[3]! + d) >>> 0;
    state[4] = (state[4]! + e) >>> 0;
    state[5] = (state[5]! + f) >>> 0;
    state[6] = (state[6]! + g) >>> 0;
    state[7] = (state[7]! + h) >>> 0;
  }
  return [...state].map((word) => word.toString(16).padStart(8, "0")).join("");
}
