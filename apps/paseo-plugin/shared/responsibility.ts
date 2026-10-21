import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

const time = z.string().refine((value) => Number.isFinite(Date.parse(value)), "Invalid time");
/** A tracker account ID as the projection and a remembered Working as carry it. */
export const ACCOUNT_ID = /^[A-Za-z0-9:_-]{1,128}$/;
const accountId = z.string().regex(ACCOUNT_ID);
const issueKey = z.string().regex(/^[A-Z][A-Z0-9_]*-[1-9]\d*$/);
const workstreamKey = z.string().trim().min(1).max(128);

/**
 * Bounded, read-only responsibility projection, separate from consumption/data@2. People are keyed by
 * stable tracker account ID. An absent `assignee` was not read; null is unassigned at `generatedAt`.
 * `workstreams` facts exist only when an operator declaration was supplied and validated; each names at least one person.
 */
export const responsibilitySchema = z.object({
  schema: z.literal("kontor/responsibility@1"),
  realm: z.string().regex(/^[a-z0-9.-]{1,253}$/),
  generatedAt: time,
  expiresAt: time,
  provenance: z.object({
    collector: z.string().min(1).max(128),
    selection: z.array(issueKey).min(1).max(64),
    complete: z.boolean(),
    workstreams: z.object({ path: z.string().min(1).max(4096), sha256: z.string().regex(/^[a-f0-9]{64}$/) }).strict().nullable(),
  }).strict(),
  people: z.array(z.object({
    accountId, displayName: z.string().min(1).max(256), active: z.boolean(), accountType: z.string().min(1).max(32),
  }).strict()).max(2000),
  issues: z.array(z.object({
    key: issueKey,
    type: z.string().min(1).max(80),
    level: z.number().int().min(-1).max(10),
    parent: issueKey.nullable(),
    assignee: accountId.nullable().optional(),
    workstreams: z.array(workstreamKey).max(32).optional(),
  }).strict()).max(5000),
  workstreams: z.array(z.object({
    key: workstreamKey, responsible: z.array(accountId).max(64), participants: z.array(accountId).max(256),
  }).strict()).max(256),
}).strict().superRefine((value, context) => {
  if (Date.parse(value.expiresAt) <= Date.parse(value.generatedAt)) context.addIssue({ code: "custom", message: "Expiry must follow the observation" });
  const people = new Set<string>();
  for (const person of value.people) {
    if (people.has(person.accountId)) context.addIssue({ code: "custom", message: "Duplicate person" });
    people.add(person.accountId);
  }
  const issues = new Set<string>();
  for (const issue of value.issues) {
    if (issues.has(issue.key)) context.addIssue({ code: "custom", message: `Duplicate issue ${issue.key}` });
    issues.add(issue.key);
    if (issue.assignee && !people.has(issue.assignee)) context.addIssue({ code: "custom", message: `Assignee of ${issue.key} is not in people` });
  }
  if (value.provenance.workstreams === null && value.workstreams.length) context.addIssue({ code: "custom", message: "Workstream facts need a declaration" });
  // A declaration names only decided workstreams; an empty one or an empty roster would read as "nobody", not unknown.
  if (value.provenance.workstreams !== null && !value.workstreams.length) context.addIssue({ code: "custom", message: "A declaration declares at least one workstream" });
  const streams = new Set<string>();
  for (const stream of value.workstreams) {
    const key = stream.key.toLocaleLowerCase();
    if (streams.has(key)) context.addIssue({ code: "custom", message: `Duplicate workstream ${stream.key}` });
    streams.add(key);
    if (!stream.responsible.length && !stream.participants.length) context.addIssue({ code: "custom", message: `Workstream ${stream.key} declares nobody` });
    if ([...stream.responsible, ...stream.participants].some((id) => !people.has(id))) context.addIssue({ code: "custom", message: `Workstream ${stream.key} names someone not in people` });
  }
});
export type Responsibility = z.infer<typeof responsibilitySchema>;

export const responsibilityReadSchema = z.object({
  status: z.enum(["ready", "invalid", "unavailable"]),
  projection: responsibilitySchema.nullable(),
  revision: z.string().nullable(),
  readAt: z.string(),
  diagnostics: z.array(z.string()),
});
export type ResponsibilityRead = z.infer<typeof responsibilityReadSchema>;

export const readResponsibility = defineRpc({ name: "responsibility.read", input: z.object({}).strict(), output: responsibilityReadSchema });
