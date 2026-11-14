import { defineRpc } from "@getpaseo/plugin";
import { z } from "zod";

export const TOKENOMICS_PAGES = {
  overview: ["Tokenomics", "Overview"], progress: ["Tokenomics", "Progress between checks"],
  activity: ["Tokenomics", "Activity"], roles: ["Tokenomics", "Roles & models"],
  pivot: ["Tokenomics", "Pivot"], usage: ["Tokenomics", "All usage"], data: ["Tokenomics", "Coverage & prices"],
  "cr-overview": ["Code Review", "Overview"], "cr-fix": ["Code Review", "Fix rate"],
  "cr-submitters": ["Code Review", "Submitters"], "cr-repos": ["Code Review", "Repositories"], "cr-prs": ["Code Review", "PRs"],
  "dv-overview": ["Delivery", "Overview"], "dv-people": ["Delivery", "People"], "dv-repos": ["Delivery", "Repositories"],
  "dv-jira": ["Delivery", "Jira vs GitHub"], "dv-bugs": ["Delivery", "Bugs & code quality"], "dv-prs": ["Delivery", "Pull requests"],
} as const;
export type TokenomicsPage = keyof typeof TOKENOMICS_PAGES;
export const DIMENSIONS = ["ws", "user", "epic", "task", "seat", "rg", "rc", "acct", "vendor", "model", "day", "adam", "tt"] as const;
export const FILTER_DIMENSIONS = ["user", "epic", "rc", "rg", "seat", "vendor", "acct", "model", "data"] as const;
export const DIMENSION_LABELS: Record<string, string> = { ws: "Workstream", user: "User", epic: "Epic", task: "Task", seat: "Seat", rg: "Role group", rc: "Role (canonical)", acct: "Account", vendor: "Vendor", model: "Model", day: "Day (Oslo)", adam: "Adam tree", tt: "Token type", data: "Data status" };
export const ACTIVITY_METRICS = { act: "Seat-time (hours)", cost: "API cost ($)", tok: "Tokens", peak: "Parallel seats (busiest hour)", cph: "API cost per active hour", hit: "Cache hit rate (%)", prs: "PRs merged", cm: "Commits", lines: "Lines changed", cpp: "API cost per merged PR" } as const;
const text = z.string().max(8192);
const number = z.number().finite();
const count = number.nonnegative();
const record = <T extends z.ZodType>(value: T) => z.record(z.string().max(512), value);
const optionalText = text.nullish();
const validDay = (s: string) => Number.isFinite(Date.parse(`${s}T00:00:00Z`)) && new Date(`${s}T00:00:00Z`).toISOString().slice(0, 10) === s;
const day = z.string().regex(/^\d{4}-\d{2}-\d{2}$/).refine(validDay, "Invalid calendar date");
export const tokenomicsQuerySchema = z.object({
  page: z.enum(Object.keys(TOKENOMICS_PAGES) as [TokenomicsPage, ...TokenomicsPage[]]).default("overview"),
  period: z.string().regex(/^(all|custom|prevm|\d{1,3}|m:\d{4}-\d{2}|w:\d{4}-\d{2}-\d{2})$/)
    .refine((s) => s.startsWith("m:") ? validDay(`${s.slice(2)}-01`) : s.startsWith("w:") ? validDay(s.slice(2)) : !/^\d+$/.test(s) || +s > 0).default("all"),
  from: day.nullable().default(null), to: day.nullable().default(null),
  rev: text.nullable().default(null), compare: text.nullable().default(null),
  show: z.enum(["usage", "idle", "all", "report"]).default("usage"),
  workstreams: z.array(text).max(512).nullable().default(null),
  filters: z.partialRecord(z.enum(FILTER_DIMENSIONS), z.array(text).max(100000)).default({}),
  groups: z.array(z.enum(DIMENSIONS)).min(1).max(4).default(["ws", "epic", "task", "seat"]),
  measure: z.enum(["cost", "tok", "act"]).default("cost"), stack: z.enum(DIMENSIONS).default("ws"),
  share: z.enum(["rg", "rc", "seat"]).default("rg"), split: z.boolean().default(false),
  expanded: z.array(text).max(100000).default([]),
  metric: z.enum(["act", "cost", "tok", "peak", "cph", "hit", "prs", "cm", "lines", "cpp"]).default("act"),
  range: z.enum(["all", "60", "30", "14"]).default("all"), dual: text.default("all"),
  allMeasure: z.enum(["cost", "tok"]).default("cost"), allStack: z.enum(["c", "g", "s", "st", "v"]).default("c"),
  allGranularity: z.enum(["week", "day"]).default("week"), allFrom: z.enum(["first", "aug"]).default("first"),
  search: text.default(""), sort: text.default(""), descending: z.boolean().default(true),
  offset: z.number().int().min(0).max(1_000_000).default(0),
  offsets: z.record(text, z.number().int().min(0).max(1_000_000)).default({}),
}).strict();
export type TokenomicsQuery = z.infer<typeof tokenomicsQuerySchema>;

// These are the collector's existing positional contracts; no pricing or collection is performed here.
export const factSchema = z.tuple([count.int(), number.int(), count.int(), count, count, count, count, count, count, count,
  count, count, count.int(), count, count, count, count, count, count]);
export const seatSchema = z.object({ id: text, short: text, title: text, ws: text, epic: text, task: text,
  role: text, rg: text, rc: text.optional(), acct: text, model: text, vendor: text, user: text,
  created: text, updated: text, status: text, data: text, adam: z.boolean(), root: text,
  open_h: count, first: optionalText, last: optionalText,
});
const issueSchema = z.object({ key: text, summary: text, status: text, cat: text, created: optionalText, resolved: optionalText });
const usageSchema = z.object({ seats: count, n: count, act: count, miss: count, hit: count, write: count, out: count, tok: count, cost: count });
const progressSchema = z.object({ status: text, cat: text, done: count, wip: count, todo: count });
export const revisionSchema = z.object({ id: text, label: text, generated: text, cut: text,
  checkTime: optionalText, scope: text.optional(), replayed: z.json().optional(),
  epics: record(progressSchema), tasks: record(z.tuple([text, text])).default({}),
  seats: record(z.tuple([text.nullable(), text.nullable()])).optional(),
  usage: z.object({ overall: usageSchema, ws: record(usageSchema) }),
});
const allFactSchema = z.tuple([day, count.int(), count.int(), count.int(), count.int(), count, count, count, count, count, count, count, count, count, count, count, count]);
const coverageSchema = z.object({ first: optionalText, last: optionalText, sessions: count, tok: count });
export const tokenomicsDataSchema = z.object({
  schema: z.literal("consumption/data@2"), generated: text.refine((s) => Number.isFinite(Date.parse(s))),
  ws: record(z.object({ name: text, kind: text, epics: z.array(text).max(10000), status: text,
    completedAt: optionalText, spans: z.array(z.tuple([text, text.nullable()])).max(10000).default([]) })),
  rows: record(text).default({}), seats: z.array(seatSchema).max(100_000), models: z.array(text).max(4096), mvendor: z.array(text).max(4096),
  facts: z.array(factSchema).max(500_000), epics: z.array(issueSchema.extend({ ws: text, due: optionalText, children: z.array(issueSchema).max(20000) })).max(10000),
  prices: z.array(z.tuple([text, text, z.union([number, text]), z.union([number, text]), z.union([number, text]), z.union([number, text]), z.union([number, text]), text])).max(4096),
  jiraBase: text.optional(), sensModel: text.default("reference model"), dashboardPeriod: text.optional(),
  rollingDays: z.array(count.int()).max(12).default([7, 14, 30]), activeNowDays: count.default(7),
  revs: z.array(revisionSchema).max(4096).default([]),
  all: z.object({ ledger: z.array(z.object({ epic: text.nullable(), title: text, tokens: count, cost: count, sessions: count, first: optionalText, last: optionalText })).max(10000),
    unknownReasons: record(count), ledgerTotal: count, ledgerLinked: count,
    cats: z.array(z.tuple([text, text, text])).max(32), subs: z.array(text).max(4096), stores: z.array(text).max(512), models: z.array(text).max(4096), mvendor: z.array(text).max(4096),
    facts: z.array(allFactSchema).max(500_000), sessions: z.object({ total: count, cat: record(count), sub: record(count), grp: record(count) }),
    coverage: record(coverageSchema), firstKS: text, cut: text, missing: record(text),
  }),
}).superRefine((d, context) => {
  if (d.models.length !== d.mvendor.length || new Set(d.seats.map((s) => s.id)).size !== d.seats.length
    || new Set(d.revs.map((r) => r.id)).size !== d.revs.length || d.seats.some((s) => !Object.hasOwn(d.ws, s.ws))
    || d.facts.some((f) => f[0] >= d.seats.length || f[2] >= d.models.length)
    || d.revs.some((r) => !Number.isFinite(Date.parse(r.cut)))
    || d.all.facts.some((f) => f[1] >= d.all.cats.length || f[2] >= d.all.subs.length || f[3] >= d.all.stores.length || f[4] >= d.all.models.length)) {
    context.addIssue({ code: "custom", message: "Invalid report references, duplicate identities or revision cut" });
  }
});
export type TokenomicsData = z.infer<typeof tokenomicsDataSchema>;
export type UsageFact = z.infer<typeof factSchema>;
export type UsageSeat = z.infer<typeof seatSchema>;
export type UsageRevision = z.infer<typeof revisionSchema>;
const linesSchema = z.object({ la: count, ld: count, ta: count, td: count, da: count, dd: count, rw: count });
const fixSchema = z.object({ c: text, repo: text, pr: text.nullable(), a: text, d: text });
const deliveryPrSchema = linesSchema.partial().extend({ ref: text, a: text, t: text, st: text, dr: z.boolean(), c: text, m: optionalText, x: optionalText,
  hc: text, hm: optionalText, base: text, cm: count, keys: z.array(text).max(512), main: z.boolean() })
  .superRefine((p, context) => { if (p.main && [p.la, p.ld, p.ta, p.td, p.da, p.dd, p.rw].some((n) => n === undefined)) context.addIssue({ code: "custom", message: "Default-branch PR is missing its line accounting" }); })
  .transform((p) => ({ ...p, la: p.la ?? 0, ld: p.ld ?? 0, ta: p.ta ?? 0, td: p.td ?? 0, da: p.da ?? 0, dd: p.dd ?? 0, rw: p.rw ?? 0 }));
const findingSchema = z.object({ pr: text, r: optionalText, d: text, dl: text, seats: z.array(text).max(32), lab: text, file: text, line: text,
  out: z.enum(["fixed", "rejected", "merged", "noanswer", "open"]) });
export const tokenomicsLiveSchema = z.object({
  review: z.object({ schema: z.literal("consumption/review@1"), org: text, since: optionalText, asOf: optionalText,
    prs: record(z.object({ repo: text, n: count, key: text, title: text, author: text, status: text, created: optionalText, baseline: z.boolean().optional(), judge: text.optional(), draft: z.boolean().optional() })),
    findings: z.array(findingSchema).max(200000), fixes: z.array(z.object({ pr: text, r: z.union([text, number]).nullish(), st: text, d: text, n: count, fx: count, fixPr: count.nullish() })).max(100000),
  }).nullable().default(null),
  delivery: z.object({ schema: z.literal("consumption/delivery@1"), org: text, since: text, asOf: text, churnDays: count, viewer: text,
    repos: z.array(text).max(10000), people: record(text), bots: z.array(text).max(4096), prs: z.array(deliveryPrSchema).max(100000),
    direct: z.array(linesSchema.extend({ repo: text, c: text, a: text, d: text, s: text, keys: z.array(text).max(512) })).max(100000),
    churn: z.array(z.tuple([text, text, text, text, count])).max(500000),
    issues: record(z.object({ t: text, s: text, cat: text, sum: text, c: optionalText, st: optionalText, r: optionalText, p: optionalText, asg: optionalText, pri: optionalText, cause: z.array(text).optional(), e: optionalText })),
    bugs: z.array(z.object({ key: text, sum: text, c: text, r: optionalText, s: text, cat: text, pri: optionalText, asg: optionalText, e: optionalText,
      cause: z.array(text).max(512), fix: z.array(fixSchema).max(10000), intro: z.array(fixSchema.extend({ n: count, s: text, keys: z.array(text).max(512) })).max(10000), why: text })).max(100000),
    jira: z.boolean(), errors: z.array(text).max(10000),
  }).nullable().default(null),
  dev: z.object({ schema: z.literal("consumption/dev@1"), rows: z.array(z.tuple([text, day, text, count, count, count, count])).max(500000) }).nullable().default(null),
});
export type TokenomicsLive = z.infer<typeof tokenomicsLiveSchema>;
const cellSchema = z.union([text, number, z.null()]);
export const tableSchema = z.object({ id: text, title: text, note: text, columns: z.array(text).max(64),
  rows: z.array(z.object({ key: text, cells: z.array(cellSchema).max(64), detail: z.array(text).max(2000).default([]), url: text.optional(), hasChildren: z.boolean().optional(), depth: count.int().optional() })).max(100),
  total: count, chart: z.boolean().default(false),
});
export type TokenomicsTable = z.infer<typeof tableSchema>;
export const tokenomicsViewSchema = z.object({
  status: z.enum(["ready", "partial", "unavailable"]), readAt: text, generated: text.nullable(), revision: text.nullable(),
  diagnostics: z.array(text), query: tokenomicsQuerySchema,
  options: z.record(text, z.array(z.object({ value: text, label: text })).max(100000)),
  tiles: z.array(z.object({ label: text, value: cellSchema, note: text })).max(32),
  tables: z.array(tableSchema).max(32), notes: z.array(text),
});
export type TokenomicsView = z.infer<typeof tokenomicsViewSchema>;
export const readTokenomics = defineRpc({ name: "tokenomics.read", input: tokenomicsQuerySchema, output: tokenomicsViewSchema });
export const exportTokenomics = defineRpc({ name: "tokenomics.export", input: tokenomicsQuerySchema,
  output: z.object({ filename: text, content: z.string().max(16 * 1024 * 1024), revision: text }) });
