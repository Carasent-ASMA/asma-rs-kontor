import { ACTIVITY_METRICS, DIMENSION_LABELS, DIMENSIONS, FILTER_DIMENSIONS, tokenomicsTableSort, type TokenomicsData, type TokenomicsLive, type TokenomicsQuery, type TokenomicsTable, type TokenomicsView, type UsageFact, type UsageRevision, type UsageSeat } from "./tokenomics";

// Accounting and period rules follow the existing consumption/data@2 renderer. Costs arrive already valued.
export const I = { s: 0, h: 1, m: 2, fresh: 3, read: 4, w5: 5, w1: 6, out: 7, reas: 8, n: 9, cost: 10, act: 11, basis: 12, cF: 13, cR: 14, cW: 15, cO: 16, sens: 17, sub: 18 } as const;
export const usd = (n: number) => `$${n.toLocaleString("en-US", { minimumFractionDigits: 2, maximumFractionDigits: 2 })}`;
export const ratio = (a: number, b: number) => b ? `${(100 * a / b).toFixed(1)}%` : "—";
export const hours = (seconds: number) => `${(seconds / 3600).toFixed(1)} h`;
const osloDayFormat = new Intl.DateTimeFormat("sv-SE", { timeZone: "Europe/Oslo" });
export const dayOf = (ms: number) => osloDayFormat.format(ms);
const dayMs = (day: string) => Date.parse(`${day}T12:00:00Z`);
export const addDays = (day: string, n: number) => dayOf(dayMs(day) + n * 86400000);
export function osloMidnight(day: string): number {
  const offset = (ms: number) => {
    const parts = Object.fromEntries(new Intl.DateTimeFormat("en-US", { timeZone: "Europe/Oslo", hour12: false,
      year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit" }).formatToParts(ms).map((p) => [p.type, p.value]));
    return Date.UTC(+parts.year!, +parts.month! - 1, +parts.day!, +parts.hour! % 24, +parts.minute!, +parts.second!) - ms;
  };
  const guess = Date.parse(`${day}T00:00:00Z`), first = guess - offset(guess);
  return guess - offset(first);
}
const monthEnd = (m: string) => new Date(Date.UTC(+m.slice(0, 4), +m.slice(5, 7), 0)).toISOString().slice(0, 10);
const weekOf = (d: string) => addDays(d, -(new Date(dayMs(d)).getUTCDay() + 6) % 7);
export function periodDays(q: TokenomicsQuery, end: string): [string | null, string] | null {
  if (q.period === "all") return null;
  if (q.period === "custom") return [q.from, q.to ?? end];
  if (q.period === "prevm") { const m = addDays(`${end.slice(0, 7)}-01`, -1).slice(0, 7); return [`${m}-01`, monthEnd(m)]; }
  if (q.period.startsWith("m:")) { const m = q.period.slice(2); return [`${m}-01`, monthEnd(m)]; }
  if (q.period.startsWith("w:")) return [q.period.slice(2), addDays(q.period.slice(2), 6)];
  return [addDays(end, 1 - Number(q.period)), end];
}
const inDays = (d: string | null | undefined, p: ReturnType<typeof periodDays>) => !!d && (!p || ((!p[0] || d >= p[0]) && d.slice(0, 10) <= p[1]));
export function blank() { return { seats: new Set<number>(), n: 0, act: 0, fresh: 0, read: 0, write: 0, out: 0, reas: 0, cost: 0, cF: 0, cR: 0, cW: 0, cO: 0, cU: 0, sens: 0, sub: 0, ar: 0 }; }
export type Aggregate = ReturnType<typeof blank>;
export function add(a: Aggregate, f: UsageFact, part?: number): void {
  a.seats.add(f[0]);
  const billed = f[12] === 3 ? f[10] : 0;
  if (part !== undefined) {
    const tok = [f[3], f[4], f[5] + f[6], f[7]][part]!, cost = [f[13], f[14], f[15], f[16]][part]!;
    const key = (["fresh", "read", "write", "out"] as const)[part]!;
    a[key] += tok; a.cost += cost + (part === 0 ? billed : 0);
    if (part === 0) { a.cF += cost; a.cU += billed; } else if (part === 1) a.cR += cost; else if (part === 2) a.cW += cost; else { a.cO += cost; a.reas += f[8]; }
    return;
  }
  a.n += f[9]; a.act += f[11]; a.fresh += f[3]; a.read += f[4]; a.write += f[5] + f[6]; a.out += f[7]; a.reas += f[8];
  a.cost += f[10]; a.cF += f[13]; a.cR += f[14]; a.cW += f[15]; a.cO += f[16]; a.cU += billed; a.sens += f[17]; a.sub += f[18];
  if (f[12] === 6) a.ar += f[3] + f[4] + f[7];
}
export const tokens = (a: Aggregate) => a.fresh + a.read + a.write + a.out;
const aggregate = (facts: UsageFact[]) => { const a = blank(); facts.forEach((f) => add(a, f)); return a; };
const measure = (a: Aggregate, kind: string) => kind === "cost" ? a.cost : kind === "tok" ? tokens(a) : a.act / 3600;
const metricCells = (a: Aggregate) => [a.cost, tokens(a), a.seats.size, a.n, a.act / 3600, ratio(a.read, a.fresh + a.read + a.write), a.fresh, a.read, a.write, a.out, a.reas, a.sub];
const METRIC_COLUMNS = ["API-equivalent cost ($)", "Tokens", "Seats with data", "Responses", "Active seat-hours", "Cache hit", "Input miss", "Cache read", "Cache write", "Output", "Reasoning (included in output)", "Sub-thread"];
type Cell = string | number | null;
type Row = { key: string; cells: Cell[]; detail: string[]; url?: string; hasChildren?: boolean; depth?: number; span?: number; heading?: boolean };
type Table = Omit<TokenomicsTable, "total"> & { display?: { columns: string[]; rows: Row[] } };
const row = (key: string, cells: Cell[], detail: string[] = [], url?: string): Row => ({ key, cells, detail, ...(url ? { url } : {}) });
const table = (id: string, title: string, columns: string[], rows: Row[], note = "", chart = false): Table => ({ id, title, columns, rows, note, chart });
const median = (values: number[]) => { const s = [...values].sort((a, b) => a - b), m = Math.floor(s.length / 2); return s.length ? s.length % 2 ? s[m]! : (s[m - 1]! + s[m]!) / 2 : null; };
const elapsedDays = (a: string | null | undefined, b: string | null | undefined) => a && b ? Math.round((dayMs(b.slice(0, 10)) - dayMs(a.slice(0, 10))) / 86400000) : null;
const safeLink = (base: string, suffix: string) => { try { const u = new URL(base + suffix); return u.protocol === "https:" && !u.username && !u.password ? u.href : undefined; } catch { return undefined; } };

export function usageModel(d: TokenomicsData, q: TokenomicsQuery) {
  const split = q.page === "pivot" && q.split;
  const rev = d.revs.find((r) => r.id === q.rev) ?? d.revs.at(-1), requestedCompare = q.compare === "previous" ? d.revs.at(-2) : d.revs.find((r) => r.id === q.compare), compare = requestedCompare?.id === rev?.id ? undefined : requestedCompare;
  const cutOf = (r?: UsageRevision) => r ? Date.parse(r.cut) : Infinity;
  const anchor = dayOf(Math.min(cutOf(rev) - 1, Date.parse(d.generated))), period = periodDays(q, anchor);
  const bounds = period ? [period[0] ? osloMidnight(period[0]) / 3600000 : -Infinity, osloMidnight(addDays(period[1], 1)) / 3600000] : null;
  const epics = new Map(d.epics.map((e) => [e.key, e]));
  const epicLabel = (k: string) => epics.has(k) ? `${k} · ${epics.get(k)!.summary}` : d.rows[k] ?? (k === "(portfolio)" ? "Workstream coordination (no single epic)" : k);
  const dim = (key: string, f: UsageFact | null, s: UsageSeat): string => {
    switch (key) {
      case "ws": return d.ws[s.ws]!.name; case "epic": return epicLabel(s.epic); case "rc": return s.rc || "UNKNOWN";
      case "seat": return `${s.short} · ${s.title}`; case "vendor": return f ? d.mvendor[f[2]]! : s.vendor;
      case "model": return f ? d.models[f[2]]! : s.model; case "day": return f ? dayOf(f[1] * 3600000) : "";
      case "adam": return s.adam ? `Adam ${s.short}` : s.root ? `under Adam ${s.root}` : "outside Adam tree (attributed by Jira key / cwd / watchdog)";
      default: return s[key as "user" | "task" | "rg" | "acct" | "data"] ?? "UNKNOWN";
    }
  };
  const classes: Record<string, string> = {}, recent = new Set<string>();
  const usage = bounds ?? [osloMidnight(addDays(anchor, 1 - d.activeNowDays)) / 3600000, osloMidnight(addDays(anchor, 1)) / 3600000];
  for (const f of d.facts) if (f[1] * 3600000 < cutOf(rev) && f[1] >= usage[0]! && f[1] < usage[1]! && (f[9] || f[10])) recent.add(d.seats[f[0]]!.ws);
  const end = period && period[1] < anchor ? period[1] : anchor;
  for (const [w, def] of Object.entries(d.ws)) {
    const open = def.kind === "desk" || def.spans.some(([c, r]) => c <= end && (!r || r > end));
    classes[w] = recent.has(w) ? open ? "active" : "used" : open ? "idle" : "hist";
  }
  const show = { usage: ["active"], idle: ["idle"], all: ["active", "idle", "used", "hist"], report: ["active", "used"] }[q.show];
  const wsOn = (ws: string) => (q.workstreams === null || q.workstreams.includes(ws)) && show.includes(classes[ws]!);
  const seatPass = (s: UsageSeat, f: UsageFact | null) => wsOn(s.ws)
    && FILTER_DIMENSIONS.every((key) => q.filters[key] === undefined || q.filters[key]!.includes(dim(key, f, s)));
  const filtered = (r = rev, lifetime = false) => d.facts.filter((f) => f[1] * 3600000 < cutOf(r)
    && (lifetime || !bounds || (f[1] >= bounds[0]! && f[1] < bounds[1]!)) && seatPass(d.seats[f[0]]!, f));
  const scope = d.seats.filter((s) => (rev === d.revs.at(-1) || Date.parse(s.created) < cutOf(rev)) && seatPass(s, null)
    && (!bounds || (Date.parse(s.updated || s.created) / 3600000 >= bounds[0]! && Date.parse(s.created) / 3600000 < bounds[1]!)));
  const facts = filtered(), total = aggregate(facts), prior = compare ? aggregate(filtered(compare)) : null;
  const grouped = (ff: UsageFact[], dims: readonly string[]) => {
    const groups = new Map<string, { labels: string[]; value: Aggregate }>();
    for (const f of ff) for (const part of dims.includes("tt") ? [0, 1, 2, 3] : [undefined]) {
      const s = d.seats[f[0]]!, labels = dims.map((key) => key === "tt" ? ["Cache miss (input)", "Cache hit (read)", "Cache write", "Output"][part!]! : dim(key, f, s));
      const key = JSON.stringify(labels); let group = groups.get(key);
      if (!group) { group = { labels, value: blank() }; groups.set(key, group); } add(group.value, f, part);
    }
    return [...groups.entries()].map(([key, g]) => row(key, [...g.labels, ...metricCells(g.value), ...(split ? [g.value.cF + g.value.cU, g.value.cR, g.value.cW, g.value.cO] : [])]));
  };
  const groupedTable = (id: string, title: string, ff: UsageFact[], dims: readonly string[], chart = false, note = "") => table(id, title, [...dims.map((key) => DIMENSION_LABELS[key]!), ...METRIC_COLUMNS, ...(split ? ["$ miss (incl. unsplit billed)", "$ read", "$ write", "$ output"] : [])], grouped(ff, dims), note, chart);
  return { rev, compare, cutOf, anchor, period, bounds, epics, epicLabel, dim, classes, wsOn, filtered, scope, facts, total, prior, grouped, groupedTable };
}

export function tokenomicsOptions(d: TokenomicsData, m: ReturnType<typeof usageModel>) {
  const pairs = (values: Iterable<string>) => [...new Set(values)].sort().map((s) => ({ value: s, label: s }));
  const options: TokenomicsView["options"] = {
    activeWindow: [{ value: String(d.activeNowDays), label: `in the last ${d.activeNowDays} days` }],
    workstreamInfo: Object.entries(d.ws).map(([value, w]) => ({ value, label: w.kind === "desk" ? `desk · ${w.project ?? ""}` : `ws:${value} · ${w.epics.length} labelled epics` })),
    workstreams: Object.entries(d.ws).map(([value, w]) => ({ value, label: `${w.name} · ${m.classes[value]}` })),
    revisions: d.revs.map((r) => ({ value: r.id, label: r.label })), groups: DIMENSIONS.map((value) => ({ value, label: DIMENSION_LABELS[value]! })),
    period: [{ value: "all", label: "All time" }, ...d.rollingDays.map((n) => ({ value: String(n), label: `Last ${n} days` })), { value: "prevm", label: "Previous month" }, { value: "custom", label: "Custom dates" },
      ...pairs(d.facts.map((f) => `m:${dayOf(f[1] * 3600000).slice(0, 7)}`)).reverse().map((o) => ({ ...o, label: new Date(`${o.value.slice(2)}-15T12:00:00Z`).toLocaleDateString("en-GB", { month: "long", year: "numeric", timeZone: "UTC" }) })), ...pairs(d.facts.map((f) => `w:${weekOf(dayOf(f[1] * 3600000))}`)).reverse().map((o) => ({ ...o, label: `Week of ${o.value.slice(2)}` }))],
    dual: [{ value: "all", label: "All selected workstreams" }, ...Object.keys(d.ws).filter(m.wsOn).map((value) => ({ value, label: d.ws[value]!.name })), ...d.epics.filter((e) => m.wsOn(e.ws) && d.ws[e.ws]!.epics.length > 1).map((e) => ({ value: e.key, label: m.epicLabel(e.key) }))],
  };
  for (const key of FILTER_DIMENSIONS) options[key] = pairs(key === "vendor" ? d.mvendor : key === "model" ? d.models : d.seats.map((s) => m.dim(key, null, s)));
  return options;
}

export function pivotRows(m: ReturnType<typeof usageModel>, q: TokenomicsQuery): Row[] {
  const levels = q.groups.flatMap((_, index) => m.grouped(m.facts, q.groups.slice(0, index + 1)).map((r) => {
    const labels = JSON.parse(r.key) as string[];
    return { ...r, cells: [...r.cells.slice(0, index + 1), ...Array<Cell>(q.groups.length - index - 1).fill(null), ...r.cells.slice(index + 1)], depth: index, hasChildren: index + 1 < q.groups.length, labels };
  }));
  const lookup = new Map(levels.map((r) => [r.key, r])), chosen = q.sorts.pivot ?? { column: q.sort || "API-equivalent cost ($)", descending: q.descending }, cols = [...q.groups.map((g) => DIMENSION_LABELS[g]!), ...METRIC_COLUMNS, ...(q.split ? ["$ miss (incl. unsplit billed)", "$ read", "$ write", "$ output"] : [])], column = cols.indexOf(chosen.column);
  levels.sort((a, b) => {
    for (let i = 0; i < Math.min(a.labels.length, b.labels.length); i++) if (a.labels[i] !== b.labels[i]) {
      const x = lookup.get(JSON.stringify(a.labels.slice(0, i + 1)))!, y = lookup.get(JSON.stringify(b.labels.slice(0, i + 1)))!;
      return (column >= 0 ? (Number(x.cells[column]) - Number(y.cells[column])) * (chosen.descending ? -1 : 1) : 0) || a.labels[i]!.localeCompare(b.labels[i]!);
    }
    return a.labels.length - b.labels.length;
  });
  return levels;
}

function movedChildren(d: TokenomicsData, from: UsageRevision | undefined, to: UsageRevision | undefined, keys: string[]) {
  const children = new Set(d.epics.filter((e) => keys.includes(e.key)).flatMap((e) => e.children.map((c) => c.key))), out = [0, 0];
  if (!from || !to) return out;
  for (const [key, b] of Object.entries(to.tasks)) if (children.has(key)) { const a = from.tasks[key]; if (!a || a[0] !== b[0]) { if (b[0] === "done") out[0] = out[0]! + 1; else if (b[0] === "indeterminate" && a?.[0] !== "done") out[1] = out[1]! + 1; } }
  return out;
}

function progressTables(d: TokenomicsData, q: TokenomicsQuery): Table[] {
  const from = d.revs.find((r) => r.id === q.compare) ?? d.revs.at(-2), to = d.revs.find((r) => r.id === q.rev) ?? d.revs.at(-1);
  if (!from || !to) return [table("progress", "Progress between checks", ["State"], [row("missing", ["Two captured checks are required."])])];
  if (from.scope && to.scope && from.scope !== to.scope) return [table("progress", "Progress between checks", ["State"], [row("scope", ["The selected checks have different scopes and cannot be compared."])])];
  const jira = (r: UsageRevision, keys: string[]) => keys.reduce((a, k) => { const e = r.epics[k]; return e ? [a[0]! + e.done, a[1]! + e.wip, a[2]! + e.todo] : a; }, [0, 0, 0]);
  const keys = [...new Set([...Object.keys(from.epics), ...Object.keys(to.epics)])], jf = jira(from, keys), jt = jira(to, keys), a = from.usage.overall, b = to.usage.overall;
  const metrics: [string, number, number][] = [["Jira issues done", jf[0]!, jt[0]!], ["Jira issues in progress", jf[1]!, jt[1]!], ["Jira issues to do", jf[2]!, jt[2]!],
    ...(["tok", "miss", "hit", "write", "out", "cost", "act", "seats", "n"] as const).map((k): [string, number, number] => [k === "act" ? "Active seat-hours" : k === "cost" ? "API-equivalent cost ($)" : k, k === "act" ? a[k] / 3600 : a[k], k === "act" ? b[k] / 3600 : b[k]]),
    ["Cache hit rate (%)", 100 * a.hit / Math.max(a.miss + a.hit + a.write, 1), 100 * b.hit / Math.max(b.miss + b.hit + b.write, 1)]];
  const note = `Captured aggregates; ignores period, user, workstream and other filters. ${from.replayed || to.replayed ? "Contains replayed history. " : ""}${from.label} → ${to.label}`;
  const result = [table("progress", "Captured totals", ["Metric", from.label, to.label, "Change", "Change (%)"], metrics.map(([label, x, y]) => row(label, [label, x, y, y - x, x ? 100 * (y - x) / x : null])), note),
    table("progress-ws", "By workstream", ["Workstream", "Done before", "Done after", "WIP before", "WIP after", "Δ seat-hours", "Δ tokens", "Δ cost ($)"], Object.entries(d.ws).filter(([w, def]) => from.usage.ws[w] || to.usage.ws[w] || def.epics.some((k) => from.epics[k] || to.epics[k])).map(([w, def]) => {
      const x = from.usage.ws[w], y = to.usage.ws[w], jx = jira(from, def.epics), jy = jira(to, def.epics);
      return row(w, [def.name, jx[0]!, jy[0]!, jx[1]!, jy[1]!, ((y?.act ?? 0) - (x?.act ?? 0)) / 3600, (y?.tok ?? 0) - (x?.tok ?? 0), (y?.cost ?? 0) - (x?.cost ?? 0)]);
    }), note),
    table("progress-epics", "Epic progress", ["Epic", "Status before", "Status after", "Done before", "Done after", "WIP before", "WIP after", "Todo before", "Todo after", "New done", "New in progress"], keys.map((k) => { const x = from.epics[k], y = to.epics[k]; return row(k, [k, x?.status ?? "absent", y?.status ?? "absent", x?.done ?? 0, y?.done ?? 0, x?.wip ?? 0, y?.wip ?? 0, x?.todo ?? 0, y?.todo ?? 0, ...movedChildren(d, from, to, [k])]); }), note),
    table("progress-tasks", "Task-level changes", ["Task", "Epic", "Status before", "Status after", "Resolved"], d.epics.flatMap((e) => e.children.filter((c) => JSON.stringify(from.tasks[c.key]) !== JSON.stringify(to.tasks[c.key])).map((c) => row(c.key, [c.key, e.key, from.tasks[c.key]?.[1] ?? "absent", to.tasks[c.key]?.[1] ?? "removed", c.resolved ?? null]))), note)];
  const byWs = result.find((t) => t.id === "progress-ws")!;
  byWs.rows.sort((a, b) => (to.usage.ws[b.key]?.cost ?? 0) - (to.usage.ws[a.key]?.cost ?? 0));
  byWs.footer = [row("__total__", ["All registered workstreams", jf[0]!, jt[0]!, jf[1]!, jt[1]!, (to.usage.overall.act - from.usage.overall.act) / 3600, to.usage.overall.tok - from.usage.overall.tok, to.usage.overall.cost - from.usage.overall.cost])];
  const epics = result.find((t) => t.id === "progress-epics")!;
  epics.display = { columns: epics.columns, rows: byWs.rows.flatMap((w) => {
    const rows = d.ws[w.key]!.epics.flatMap((key) => { const r = epics.rows.find((r) => r.key === key); return r ? [{ ...r, cells: [d.epics.find((e) => e.key === key) ? `${key} · ${d.epics.find((e) => e.key === key)!.summary}` : key, ...r.cells.slice(1)], url: safeLink(d.jiraBase ?? "", key) }] : []; });
    return rows.length ? [{ ...row(`heading-${w.key}`, [w.cells[0]!]), span: 6, heading: true }, ...rows] : [];
  }) };
  return result;
}

export type BuiltTokenomics = { tiles: TokenomicsView["tiles"]; tables: Table[]; notes: string[] };
export function buildTokenomics(d: TokenomicsData, live: TokenomicsLive, q: TokenomicsQuery): BuiltTokenomics {
  const m = usageModel(d, q), a = m.total, tiles: TokenomicsView["tiles"] = [], tables: Table[] = [], notes: string[] = [];
  const tile = (label: string, value: Cell, note = "") => tiles.push({ label, value, note });
  const priceNote = "Costs mix list-price equivalents, billed OpenRouter and blended GLM bases. Unpriced rows retain recorded tokens at $0. Coverage gaps make usage a floor.";
  if (q.page === "progress") return { tiles, tables: progressTables(d, q), notes };
  if (q.page.startsWith("cr-")) return reviewTables(d, live, q);
  if (q.page.startsWith("dv-")) return deliveryTables(d, live, q);
  if (q.page === "overview") {
    const prior = m.prior;
    const delta = (value: number, before: number | undefined, format: (n: number) => string, epsilon = 1e-9) => before === undefined || Math.abs(value - before) <= epsilon ? undefined : `Δ ${value > before ? "+" : "−"}${format(Math.abs(value - before))} vs ${m.compare!.label} · re-cut`;
    const number = (n: number) => n >= 1e9 ? `${(n / 1e9).toFixed(2)}B` : n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${(n / 1e3).toFixed(1)}K` : String(Math.round(n));
    const hit = (a: Aggregate) => 100 * a.read / Math.max(a.fresh + a.read + a.write, 1), sub = (a: Aggregate) => 100 * a.sub / Math.max(tokens(a), 1);
    const adamAct = (ff: UsageFact[]) => ff.filter((f) => d.seats[f[0]]!.adam).reduce((n, f) => n + f[11], 0);
    const overviewTile = (label: string, value: Cell, note: string, change?: string) => tiles.push({ label, value, note, ...(change ? { delta: change } : {}) });
    overviewTile("API-equivalent cost", usd(a.cost), "list-price equivalent; billed OpenRouter and blended GLM rows per the price table", delta(a.cost, prior?.cost, usd));
    overviewTile("Tokens processed", tokens(a), `${a.n.toLocaleString("en-US")} model responses`, delta(tokens(a), prior ? tokens(prior) : undefined, number));
    overviewTile("Cache hit rate", ratio(a.read, a.fresh + a.read + a.write), `${number(a.read)} read from cache`, delta(hit(a), prior ? hit(prior) : undefined, (n) => `${n.toFixed(1)} pp`, 0.05));
    overviewTile("Cache miss (input)", a.fresh, `${usd(a.cF + a.cU)} of cost`, delta(a.fresh, prior?.fresh, number));
    overviewTile("Cache write", a.write, `${usd(a.cW)} of cost`, delta(a.write, prior?.write, number));
    overviewTile("Output tokens", a.out, `${usd(a.cO)} · reasoning ${number(a.reas)}`, delta(a.out, prior?.out, number));
    overviewTile("Seats", `${a.seats.size} / ${m.scope.length}`, "with token data / in scope", delta(a.seats.size, prior?.seats.size, (n) => `${n} seat${n === 1 ? "" : "s"}`));
    overviewTile("Active seat-hours", hours(a.act), "all seats; Codex/Claude gaps capped at 5 min, OpenCode exact", delta(a.act, prior?.act, hours));
    overviewTile("Orchestrator active hours", hours(adamAct(m.facts)), "Adam sessions only", delta(adamAct(m.facts), m.compare ? adamAct(m.filtered(m.compare)) : undefined, hours));
    overviewTile("Sub-thread tokens", ratio(a.sub, tokens(a)), `${number(a.sub)} from Codex guardian / spawned sub-agents`, delta(sub(a), prior ? sub(prior) : undefined, (n) => `${n.toFixed(1)} pp`, 0.05));
    overviewTile("Codex Auto Review", a.ar, `no public price → $0; at ${d.sensModel} rates ≈ ${usd(a.sens)}`, delta(a.ar, prior?.ar, number));
    const values = m.grouped(m.facts, ["ws", "epic"]);
    for (const [w, def] of Object.entries(d.ws)) if (m.wsOn(w)) for (const key of new Set([...def.epics, ...m.scope.filter((s) => s.ws === w).map((s) => s.epic)])) {
      const labels = [def.name, m.epicLabel(key)]; if (!values.some((r) => r.cells[0] === labels[0] && r.cells[1] === labels[1])) values.push(row(JSON.stringify(labels), [...labels, ...metricCells(blank())]));
    }
    tables.push(table("epics", "Epic progress and consumption", ["Workstream", "Epic / desk row", ...METRIC_COLUMNS, "Status", "Done", "In progress", "Todo", "Δ cost ($)", "Δ tokens", "Before status", "Δ done", "Δ in progress", "Δ todo", ...METRIC_COLUMNS.map((c) => `Δ ${c}`), "Jira category", "New done", "New in progress"], values.map((r) => {
      const key = [...m.epics.keys()].find((k) => m.epicLabel(k) === r.cells[1]), e = key ? m.epics.get(key) : undefined;
      const progress = key ? m.rev?.epics[key] : undefined, children = e?.children ?? [];
      const ff = m.facts.filter((f) => m.dim("ws", f, d.seats[f[0]]!) === r.cells[0] && m.dim("epic", f, d.seats[f[0]]!) === r.cells[1]);
      const priorFacts = m.compare ? m.filtered(m.compare).filter((f) => m.dim("ws", f, d.seats[f[0]]!) === r.cells[0] && m.dim("epic", f, d.seats[f[0]]!) === r.cells[1]) : null;
      const pa = priorFacts ? aggregate(priorFacts) : null, current = aggregate(ff);
      const beforeProgress = key ? m.compare?.epics[key] : undefined;
      return { ...r, cells: [...r.cells, progress?.status ?? e?.status ?? "Desk / coordination", progress?.done ?? children.filter((c) => c.cat === "done").length, progress?.wip ?? children.filter((c) => c.cat === "indeterminate").length, progress?.todo ?? children.filter((c) => !["done", "indeterminate"].includes(c.cat)).length, pa ? current.cost - pa.cost : null, pa ? tokens(current) - tokens(pa) : null, beforeProgress?.status ?? null,
        m.compare ? (progress?.done ?? 0) - (beforeProgress?.done ?? 0) : null, m.compare ? (progress?.wip ?? 0) - (beforeProgress?.wip ?? 0) : null, m.compare ? (progress?.todo ?? 0) - (beforeProgress?.todo ?? 0) : null, ...metricCells(current).map((n, i) => pa ? typeof n === "number" ? n - Number(metricCells(pa)[i]) : i === 5 ? 100 * current.read / Math.max(1, current.fresh + current.read + current.write) - 100 * pa.read / Math.max(1, pa.fresh + pa.read + pa.write) : null : null), progress?.cat ?? e?.cat ?? "", ...movedChildren(d, m.compare, m.rev, key ? [key] : [])],
        detail: children.map((c) => `${c.key} · ${c.summary} · ${m.rev?.tasks[c.key]?.[1] ?? c.status} · resolved ${c.resolved || "—"}`), ...(key ? { url: safeLink(d.jiraBase ?? "", key) } : {}) };
    }), "Jira state at the selected check. Consumption and deltas re-cut the latest extract; captured progression has its own page."));
    const workstreams = m.groupedTable("workstream-totals", "Workstream consumption totals", m.facts, ["ws"]);
    const priorWs = m.compare ? m.grouped(m.filtered(m.compare), ["ws"]) : [];
    workstreams.columns.push("Done", "In progress", "Todo", "Δ done", "Δ in progress", "Δ todo", ...METRIC_COLUMNS.map((c) => `Δ ${c}`), "Orchestrator time", "Δ Orchestrator time", "New done", "New in progress");
    workstreams.rows = Object.entries(d.ws).filter(([w]) => m.wsOn(w)).map(([w, def]) => {
      const r = workstreams.rows.find((r) => r.cells[0] === def.name) ?? row(w, [def.name, ...metricCells(blank())]);
      const progress = (revision: UsageRevision | undefined) => def.epics.reduce<[number, number, number]>((a, key) => { const p = revision?.epics[key]; if (p) { a[0] += p.done; a[1] += p.wip; a[2] += p.todo; } return a; }, [0, 0, 0]);
      const current = progress(m.rev), previous = progress(m.compare), prior = priorWs.find((p) => p.cells[0] === def.name);
      return { ...r, key: w, cells: [...r.cells, ...current, ...current.map((n, i) => m.compare ? n - previous[i]! : null), ...METRIC_COLUMNS.map((_, i) => m.compare ? typeof r.cells[i + 1] === "number" ? Number(r.cells[i + 1]) - Number(prior?.cells[i + 1] ?? 0) : i === 5 ? (parseFloat(String(r.cells[i + 1])) || 0) - (parseFloat(String(prior?.cells[i + 1])) || 0) : null : null), m.facts.filter((f) => d.seats[f[0]]!.ws === w && d.seats[f[0]]!.adam).reduce((n, f) => n + f[11] / 3600, 0), m.compare ? m.facts.filter((f) => d.seats[f[0]]!.ws === w && d.seats[f[0]]!.adam).reduce((n, f) => n + f[11] / 3600, 0) - m.filtered(m.compare).filter((f) => d.seats[f[0]]!.ws === w && d.seats[f[0]]!.adam).reduce((n, f) => n + f[11] / 3600, 0) : null, ...movedChildren(d, m.compare, m.rev, def.epics)] };
    });
    tables.push(workstreams);
    // Keep open workstreams with no recorded usage visible when explicitly requested.
    tables.push(table("workstream-state", "Workstream coverage", ["Workstream", "Lifecycle", "Seats in scope"], Object.entries(d.ws).filter(([w]) => q.workstreams === null || q.workstreams.includes(w)).map(([w, def]) => row(w, [def.name, m.classes[w]!, m.scope.filter((s) => s.ws === w).length]))));
  } else if (q.page === "pivot") {
    const base = m.groupedTable("pivot", "Pivot", m.facts, q.groups);
    const rows = pivotRows(m, q);
    tables.push({ ...base, rows, footer: [row("__total__", ["Total", ...Array<Cell>(q.groups.length - 1).fill(null), ...metricCells(m.total), ...(q.split ? [m.total.cF + m.total.cU, m.total.cR, m.total.cW, m.total.cO] : []), null])], note: "Subtotals include their children; do not add parent and child rows. Expand groups to drill down. CSV exports leaf rows for the whole selection. Active time/responses are not allocated across token types." });
  } else if (q.page === "roles") {
    const shareGroups = new Map<string, Aggregate>();
    for (const f of m.facts) { const key = m.dim(q.share, f, d.seats[f[0]]!), a = shareGroups.get(key) ?? blank(); add(a, f); shareGroups.set(key, a); }
    const share = table("share", "Role consumption share", [DIMENSION_LABELS[q.share]!, "Tokens", "Share of tokens", "API cost ($)", "Responses", "Cache hit", "Seats"], [...shareGroups].map(([key, a]) => row(key, [key, tokens(a), ratio(tokens(a), tokens(m.total)), a.cost, a.n, ratio(a.read, a.fresh + a.read + a.write), a.seats.size])), "Share of consumed tokens by role in the current selection. Colours are stable per role; the tail is grouped as Other. Inspect a segment for its share, tokens, cost and responses; exact values are below.", true);
    share.rows.sort((a, b) => Number(b.cells[1]) - Number(a.cells[1]));
    share.footer = [row("__total__", ["Total", tokens(m.total), "100.0%", m.total.cost, m.total.n, ratio(m.total.read, m.total.fresh + m.total.read + m.total.write), m.total.seats.size])];
    tables.push(share);
    tables.push(table("adams", "Adam sessions (orchestrator hours)", ["Workstream", "Adam session", "Account", "Model", "Created", "Last activity", "Status", "Open hours", ...METRIC_COLUMNS], d.seats.flatMap((s, index) => {
      if (!s.adam || !m.wsOn(s.ws)) return [];
      const captured = m.rev !== d.revs.at(-1) ? m.rev?.seats?.[s.id] : undefined, updated = captured?.[1] || s.updated, status = captured?.[0] || s.status;
      const a = aggregate(m.facts.filter((f) => f[0] === index));
      return [row(s.id, [d.ws[s.ws]!.name, `${s.short} · ${s.title}`, s.acct, s.model, s.created, updated, status, captured ? Math.max(0, (Date.parse(updated) - Date.parse(s.created)) / 3600000) : s.open_h, ...metricCells(a)])];
    }), "Open = created to last observed activity; older checks use captured lifecycle observations where present. Active uses measured durations/capped gaps. An Adam without recorded tokens remains visible."));
    const adams = tables.find((t) => t.id === "adams")!, adamTotal = aggregate(m.facts.filter((f) => d.seats[f[0]]!.adam));
    adams.rows.sort((a, b) => String(a.cells[0]).localeCompare(String(b.cells[0])) || String(a.cells[4]).localeCompare(String(b.cells[4])));
    adams.footer = [row("__total__", ["Total", "", "", "", "", "", "", adams.rows.reduce((n, r) => n + Number(r.cells[7]), 0), ...metricCells(adamTotal)])];
    const mix = m.groupedTable("mix", "Model token mix and price", m.facts, ["model"]);
    mix.columns.push("$ miss (incl. unsplit billed)", "$ read", "$ write", "$ output", "Vendor");
    mix.rows = mix.rows.map((r) => { const a = aggregate(m.facts.filter((f) => d.models[f[2]] === r.cells[0])); return { ...r, cells: [...r.cells, a.cF + a.cU, a.cR, a.cW, a.cO, d.mvendor[d.models.indexOf(String(r.cells[0]))] ?? "Unknown"] }; });
    mix.rows.sort((a, b) => Number(b.cells[1]) - Number(a.cells[1]));
    mix.footer = [row("__total__", ["Total", ...metricCells(m.total), m.total.cF + m.total.cU, m.total.cR, m.total.cW, m.total.cO, ""])];
    mix.note = "Output includes reasoning. Billed OpenRouter totals with no split remain in input cost. The price table shows each valuation basis."; tables.push(mix);
  } else if (q.page === "data") {
    notes.push("Seats without transcripts remain counted with zero recorded tokens. Actual usage is unknown; totals are floors.");
    tables.push(table("coverage", "Data coverage", ["Workstream", "Seats", "With tokens", "Transcript missing", "No token store", "No session ID", "Open hours without data"], Object.entries(d.ws).map(([w, def]) => {
      const ss = m.scope.filter((s) => s.ws === w); return row(w, [def.name, ss.length, ss.filter((s) => s.data === "ok").length, ss.filter((s) => s.data === "transcript-missing").length, ss.filter((s) => s.data === "no-token-store").length, ss.filter((s) => s.data === "no-session-id").length, ss.filter((s) => s.data !== "ok").reduce((n, s) => n + s.open_h, 0)]);
    })));
    tables.push(table("missing", "Seats without recoverable tokens", ["Seat", "Account", "Model", "Data status", "Created", "Open hours"], m.scope.filter((s) => s.data !== "ok").map((s) => row(s.id, [`${s.short} · ${s.title}`, s.acct, s.model, s.data, s.created, s.open_h]))));
    const coverage = tables.find((t) => t.id === "coverage")!;
    coverage.display = { columns: ["Workstream", "Seats in scope", "With token data", "Transcript purged", "No local token store (Cursor/DSH)", "No session id", "Open hours of seats without data"], rows: [
      ...coverage.rows.filter((r) => Number(r.cells[1]) > 0),
      { ...row("missing-heading", ["Seats without recoverable token data (counted as seats, zero tokens)"]), span: 7, heading: true },
      ...m.scope.filter((s) => s.data !== "ok").sort((a, b) => a.created.localeCompare(b.created)).map((s) => ({ ...row(s.id, [`${s.short} · ${s.title} · ${s.acct}/${s.model}`, null, null, null, s.data, s.created.slice(0, 10), s.open_h]), span: 4 })),
    ] };
    coverage.note = "Seats counted in a workstream are those whose Paseo record falls in the selected period. Purged transcripts and sources without local token counts leave unrecoverable usage; totals are therefore floors for the earliest periods.";
    tables.push(table("prices", "Price table used (USD per 1M tokens)", ["Model", "Vendor", "Input miss", "Cache read", "Write 5m", "Write 1h", "Output", "Basis"], d.prices.map((p, i) => row(String(i), p)), priceNote));
  } else if (q.page === "usage") return allUsageTables(d, q);
  else if (q.page === "activity") return activityTables(d, live, q, m);
  notes.push(priceNote);
  return { tiles, tables, notes };
}

export function tokenomicsSummary(m: ReturnType<typeof usageModel>): NonNullable<TokenomicsView["summary"]> {
  return { cost: m.total.cost, tokens: tokens(m.total), responses: m.total.n, hours: m.total.act / 3600, cached: m.total.read,
    seats: m.total.seats.size, scope: m.scope.length, hit: ratio(m.total.read, m.total.fresh + m.total.read + m.total.write), revision: m.rev?.label ?? "Latest check", compare: m.compare?.label ?? "none" };
}
export const matchesTokenomicsSearch = (row: { cells: Cell[]; detail: string[] }, id: string, q: TokenomicsQuery) => !q.search.trim() || (q.page === "dv-jira" && id !== "jira-issues") || (q.page === "dv-bugs" && id !== "bugs") || [...row.cells, ...row.detail].join(" ").toLowerCase().includes(q.search.trim().toLowerCase());
export function paginateTokenomics(result: BuiltTokenomics, q: TokenomicsQuery): TokenomicsTable[] {
  const presented = result.tables.filter((t) => !(q.page === "data" && t.id === "missing") && !(q.page === "usage" && t.id === "unknown")).map(({ display, ...t }) => display ? { ...t, columns: display.columns, rows: display.rows } : t);
  for (const t of presented) if (t.id === "pivot") { const dims = t.columns.length - METRIC_COLUMNS.length - (q.split ? 4 : 0), total = Number(t.footer?.[0]?.cells[dims]) || 0; t.columns = [...t.columns, "Cost share"]; t.rows = t.rows.map((r) => ({ ...r, cells: [...r.cells, ratio(Number(r.cells[dims]) || 0, total)] })); }
  for (const t of presented) if (t.columns.includes("Code added")) { const stems = ["Code", "Tests", "Docs"].filter((stem) => t.columns.includes(`${stem} added`) && t.columns.includes(`${stem} deleted`)); const indices = stems.map((stem) => [t.columns.indexOf(`${stem} added`), t.columns.indexOf(`${stem} deleted`)]); t.columns = [...t.columns, ...stems.map((stem) => `${stem === "Tests" ? "Test" : stem === "Docs" ? "Doc" : stem} lines`)]; t.rows = t.rows.map((r) => ({ ...r, cells: [...r.cells, ...indices.map(([a, b]) => r.cells[a!] == null || r.cells[b!] == null ? null : Number(r.cells[a!]) + Number(r.cells[b!]))] })); }
  const scoped = presented.flatMap((t) => t.id !== "epics" || q.page !== "overview" ? [t] : (result.tables.find((t) => t.id === "workstream-totals")?.rows ?? []).filter((r) => q.openWorkstreams.includes(r.key)).map((r) => ({ ...t, id: `epics:${r.key}`, rows: t.rows.filter((e) => e.cells[0] === r.cells[0]) })));
  return scoped.map((t) => {
    const sort = tokenomicsTableSort(t.id, q);
    const index = t.columns.indexOf(sort.column), needle = q.search.trim().toLowerCase();
    const rows = t.rows.filter((r) => {
      if (needle) return matchesTokenomicsSearch(r, t.id, q);
      if (t.id === "all-categories" && r.key.startsWith("sub-")) return q.expanded.includes(`category-${r.key.split("-")[1]}`);
      if (t.id !== "pivot") return true;
      const path = JSON.parse(r.key) as string[];
      return path.slice(0, -1).every((_, index) => q.expanded.includes(JSON.stringify(path.slice(0, index + 1))));
    });
    if (index >= 0 && t.id !== "pivot") rows.sort((a, b) => { const x = a.cells[index], y = b.cells[index]; return Number(x == null) - Number(y == null) || (x == null || y == null ? 0 : (typeof x === "number" && typeof y === "number" ? x - y : String(x).localeCompare(String(y))) * (sort.descending ? -1 : 1)); });
    const offset = q.offsets[t.id] ?? q.offset;
    // Charts retain their complete selection and original time scale, independently of table paging/search.
    const plot = t.chart ? { ...t.plot, columns: t.columns, rows: t.rows.map((r) => r.cells), ...(t.id === "timeline" ? { spans: result.tables.find((t) => t.id === "dates")?.rows.map((r) => r.cells), flags: result.tables.find((t) => t.id === "flags")?.rows.map((r) => r.cells), end: result.tables.find((t) => t.id === "parallel")?.rows.at(-1)?.cells[0] as string | undefined } : {}) } : undefined;
    return { ...t, rows: rows.slice(offset, offset + 50), total: rows.length, ...(plot ? { plot } : {}) };
  });
}

function allUsageTables(d: TokenomicsData, q: TokenomicsQuery): BuiltTokenomics {
  // All-store rows contain session counts, not distinct seat identities.
  const columns = METRIC_COLUMNS.filter((_, i) => i !== 2), cells = (a: Aggregate) => metricCells(a).filter((_, i) => i !== 2);
  const all = d.all, sum = (fs: typeof all.facts) => fs.reduce((a, f) => {
    a.fresh += f[5]; a.read += f[6]; a.write += f[7]; a.out += f[8]; a.n += f[9]; a.act += f[10]; a.cost += f[11];
    a.cF += f[12]; a.cR += f[13]; a.cW += f[14]; a.cO += f[15]; a.sens += f[16]; return a;
  }, blank()), total = sum(all.facts);
  const grouping = (keys: (f: typeof all.facts[number]) => string[]) => {
    const groups = new Map<string, { labels: string[]; facts: typeof all.facts }>();
    for (const f of all.facts) { const labels = keys(f), k = JSON.stringify(labels), item = groups.get(k) ?? { labels, facts: [] }; item.facts.push(f); groups.set(k, item); }
    return [...groups].map(([k, g]) => row(k, [...g.labels, ...cells(sum(g.facts))]));
  };
  const groups = [...new Set(all.cats.map((c) => c[2]))], before = sum(all.facts.filter((f) => f[0] < all.firstKS));
  const tiles: TokenomicsView["tiles"] = [{ label: "Total known usage", value: tokens(total), note: `${usd(total.cost)} · ${all.sessions.total} sessions` },
    ...groups.map((g) => { const a = sum(all.facts.filter((f) => all.cats[f[1]]![2] === g)); return { label: g, value: tokens(a), note: `${usd(a.cost)} · ${ratio(a.cost, total.cost)} of cost` }; }),
    { label: "Before orchestrated work began", value: tokens(before), note: `Before ${all.firstKS}; recoverable evidence only · ${usd(before.cost)}` },
    { label: "Registered workstreams", value: tokens(sum(all.facts.filter((f) => f[1] === 0))), note: `${Object.keys(d.ws).length} registered` },
    { label: "Cache hit rate (all)", value: ratio(total.read, total.fresh + total.read + total.write), note: "Recorded tokens only" }];
  const stack = (f: typeof all.facts[number]) => q.allStack === "c" ? all.cats[f[1]]![1] : q.allStack === "g" ? all.cats[f[1]]![2] : q.allStack === "s" ? all.subs[f[2]]! : q.allStack === "st" ? all.stores[f[3]]! : all.mvendor[f[4]]!;
  const chartGroups = new Map<string, { day: string; label: string; value: number }>();
  for (const f of all.facts) {
    if (q.allFrom === "aug" && f[0] < all.firstKS) continue;
    const day = q.allGranularity === "week" ? weekOf(f[0]) : f[0], label = stack(f), key = JSON.stringify([day, label]), g = chartGroups.get(key) ?? { day, label, value: 0 };
    g.value += q.allMeasure === "cost" ? f[11] : f[5] + f[6] + f[7] + f[8]; chartGroups.set(key, g);
  }
  const categories = all.cats.flatMap((c, i) => {
    const fs = all.facts.filter((f) => f[1] === i), a = sum(fs);
    return [row(`category-${i}`, [c[2], c[1], "All sub-categories", ...cells(a), all.sessions.cat[String(i)] ?? 0, ratio(a.cost, total.cost)],
      [...new Set(fs.map((f) => f[2]))].map((j) => { const a = sum(fs.filter((f) => f[2] === j)); return `${all.subs[j]} · ${tokens(a)} tokens · ${usd(a.cost)} · ${all.sessions.sub[`${i}|${j}`] ?? 0} sessions`; })),
      ...[...new Set(fs.map((f) => f[2]))].map((j) => row(`sub-${i}-${j}`, [c[2], c[1], all.subs[j]!, ...cells(sum(fs.filter((f) => f[2] === j))), all.sessions.sub[`${i}|${j}`] ?? 0, ratio(sum(fs.filter((f) => f[2] === j)).cost, total.cost)]))];
  });
  const ledgerNote = `${ratio(all.ledgerLinked, all.ledgerTotal)} linked to a Jira epic; ${all.ledgerTotal} recoverable tokens. Unknown reasons remain explicit. Sessions across epics are not additive.`;
  const result: BuiltTokenomics = { tiles, notes: ["All usage is local-store evidence and ignores user, workstream, period and revision filters. The chart's From control affects the chart only.", "Cursor and other sources without token counts remain missing evidence. Costs retain the collector's price bases; no repricing is performed."], tables: [
    table("all-chart", "All known usage over time", [q.allGranularity === "week" ? "Week" : "Day", "Stack", q.allMeasure === "cost" ? "API cost ($)" : "Tokens"], [...chartGroups].sort((a, b) => a[1].day.localeCompare(b[1].day)).map(([k, g]) => row(k, [g.day, g.label, g.value])), "", true),
    table("all-categories", "Categories and sub-categories", ["Group", "Category", "Sub-category", ...columns, "Sessions", "Share of cost"], categories, "Category totals and their sub-categories overlap. Do not add parent and child rows."),
    table("all-months", "By month", ["Month", "Category", ...columns], grouping((f) => [f[0].slice(0, 7), all.cats[f[1]]![1]])),
    table("all-coverage", "What each store still holds", ["Store", "Earliest", "Latest", "Sessions", "Tokens", "Missing evidence"], Object.entries(all.coverage).map(([k, v]) => row(k, [k, v.first ?? null, v.last ?? null, v.sessions, v.tok, all.missing[k] ?? "—"]))),
    table("ledger", "All usage by Jira epic", ["Epic / bucket", "Title", "Tokens", "API cost ($)", "Sessions", "Share", "First", "Last"], all.ledger.map((r, i) => row(String(i), [r.epic ?? "UNKNOWN", r.title, r.tokens, r.cost, r.sessions, ratio(r.tokens, all.ledgerTotal), r.first ?? null, r.last ?? null], [], r.epic && r.epic !== "UNKNOWN" ? safeLink(d.jiraBase ?? "", r.epic) : undefined)), ledgerNote),
    table("unknown", "Unknown epic reasons", ["Reason", "Tokens", "Share of all tokens"], Object.entries(all.unknownReasons).map(([k, v]) => row(k, [k, v, ratio(v, all.ledgerTotal)])), ledgerNote),
  ] };
  const categoriesTable = result.tables.find((t) => t.id === "all-categories")!;
  const displayCells = (a: Aggregate, sessions: number): Cell[] => [sessions, a.n, a.act / 3600, a.fresh, a.read, a.write, a.out, tokens(a), ratio(a.read, a.fresh + a.read + a.write), a.cF, a.cR, a.cW, a.cO, a.cost, ratio(a.cost, total.cost)];
  const displayRow = (key: string, name: string, fs: typeof all.facts, sessions: number, depth: number, hasChildren = false): Row => ({ ...row(key, [name, ...displayCells(sum(fs), sessions)]), depth, hasChildren });
  categoriesTable.display = { columns: ["Category → sub-category", "Sessions", "Responses", "Active seat-hours", "Input miss", "Cache read", "Cache write", "Output", "Tokens", "Cache hit", "$ miss", "$ hit", "$ write", "$ output", "API-equivalent cost ($)", "Share of cost"], rows: groups.flatMap((g) => [
    displayRow(`group-${g}`, g, all.facts.filter((f) => all.cats[f[1]]![2] === g), all.sessions.grp[g] ?? 0, 0),
    ...all.cats.flatMap((c, i) => { const fs = all.facts.filter((f) => f[1] === i); return c[2] !== g || !fs.length ? [] : [displayRow(`category-${i}`, c[1], fs, all.sessions.cat[String(i)] ?? 0, 1, true), ...[...new Set(fs.map((f) => f[2]))].sort((a, b) => sum(fs.filter((f) => f[2] === b)).cost - sum(fs.filter((f) => f[2] === a)).cost).map((j) => displayRow(`sub-${i}-${j}`, all.subs[j]!, fs.filter((f) => f[2] === j), all.sessions.sub[`${i}|${j}`] ?? 0, 2))]; }),
  ]).concat([{ ...row("__total__", ["Total known usage", ...displayCells(total, all.sessions.total)]), heading: true }]) };
  categoriesTable.note = "";
  const months = result.tables.find((t) => t.id === "all-months")!;
  months.display = { columns: ["Month", ...all.cats.map((c) => c[1]), "Total"], rows: [...new Set(all.facts.map((f) => f[0].slice(0, 7)))].sort().map((month) => row(month, [month, ...all.cats.map((_, i) => { const a = sum(all.facts.filter((f) => f[0].startsWith(month) && f[1] === i)); return tokens(a) ? `${tokens(a)} · $${a.cost}` : "—"; }), (() => { const a = sum(all.facts.filter((f) => f[0].startsWith(month))); return `${tokens(a)} · $${a.cost}`; })()])) };
  const ledger = result.tables.find((t) => t.id === "ledger")!;
  ledger.note = `${ratio(all.ledgerLinked, all.ledgerTotal)} of all recoverable tokens (${all.ledgerTotal}) is linked to a Jira epic; the rest is carried as Unknown epic. Unknown reasons: ${Object.entries(all.unknownReasons).map(([k, v]) => `${k} — ${v}`).join(" · ") || "—"}. Every recoverable token in the local stores is counted exactly once.`;
  ledger.display = { columns: ledger.columns, rows: ledger.rows.flatMap((r) => [r, ...(r.cells[0] === "UNKNOWN" ? Object.entries(all.unknownReasons).map(([k, v]) => ({ ...row(`unknown-${k}`, [k, "", v, null, null, ratio(v, all.ledgerTotal), "", ""]), depth: 1 })) : [])]) };
  const chart = result.tables.find((t) => t.id === "all-chart")!;
  const ranked = new Map<string, number>(); for (const f of all.facts) ranked.set(stack(f), (ranked.get(stack(f)) ?? 0) + f[11]);
  chart.plot = { columns: chart.columns, rows: [], order: q.allStack === "c" ? all.cats.map((c) => c[1]) : q.allStack === "g" ? groups : [...ranked].sort((a, b) => b[1] - a[1]).map(([k]) => k), events: [[all.firstKS, "orchestrated work begins"]] };
  tiles[0]!.note = `${usd(total.cost)} · since ${all.facts.map((f) => f[0]).sort()[0] ?? "unknown"} · ${all.sessions.total.toLocaleString("en-US")} sessions`;
  tiles[tiles.length - 2]!.note = `${usd(sum(all.facts.filter((f) => f[1] === 0)).cost)} · ${ratio(sum(all.facts.filter((f) => f[1] === 0)).cost, total.cost)} of cost · ${Object.keys(d.ws).length} in the registry`;
  tiles.at(-1)!.note = `${total.read} read · ${total.fresh} miss`;
  return result;
}

function reviewTables(d: TokenomicsData, live: TokenomicsLive, q: TokenomicsQuery): BuiltTokenomics {
  const rv = live.review, tiles: TokenomicsView["tiles"] = [], tables: Table[] = [];
  if (!rv) return { tiles, tables, notes: ["Code Review is unavailable: no PR desk snapshot was collected on this host. This is missing data, not zero findings."] };
  const pd = periodDays(q, dayOf(Date.parse(d.generated))), inP = (day: string | null | undefined) => inDays(day, pd);
  type Review = { key: string; prs: Set<string>; n: number; sev: Record<string, number>; out: Record<string, number>; rev: number; fix: number; fixes: number; findings: typeof rv.findings };
  const create = (key: string): Review => ({ key, prs: new Set(), n: 0, sev: {}, out: {}, rev: 0, fix: 0, fixes: 0, findings: [] });
  const prs = new Map<string, Review>(), get = (key: string) => { let p = prs.get(key); if (!p) { p = create(key); p.prs.add(key); prs.set(key, p); } return p; };
  const findings = rv.findings.filter((f) => inP(f.d)), fixes = rv.fixes.filter((f) => inP(f.d));
  const addFinding = (p: Review, f: typeof findings[number]) => { p.prs.add(f.pr); p.n++; p.sev[f.lab] = (p.sev[f.lab] ?? 0) + 1; p.out[f.out] = (p.out[f.out] ?? 0) + 1; p.findings.push(f); };
  findings.forEach((f) => addFinding(get(f.pr), f)); fixes.forEach((f) => get(f.pr).fixes++);
  const seatRefs = d.seats.map((s) => { const match = /^desk:RSW\b.*?([\w.-]+#\d+)/.exec(s.epic); return match ? { ref: match[1]!, kind: / • fix r\d+/.test(s.epic) ? "fix" as const : "rev" as const } : null; });
  const ws = d.seats.find((_, i) => seatRefs[i])?.ws; let desk = 0;
  for (const f of d.facts) if (d.seats[f[0]]!.ws === ws && inP(dayOf(f[1] * 3600000))) { desk += f[10]; const r = seatRefs[f[0]]; if (r) get(r.ref)[r.kind] += f[10]; }
  const merge = (a: Review, b: Review) => { b.prs.forEach((p) => a.prs.add(p)); a.n += b.n; a.rev += b.rev; a.fix += b.fix; a.fixes += b.fixes;
    for (const key of ["sev", "out"] as const) for (const [k, n] of Object.entries(b[key])) a[key][k] = (a[key][k] ?? 0) + n; a.findings.push(...b.findings); };
  const all = create("all"); prs.forEach((p) => merge(all, p));
  const cells = (p: Review): Cell[] => [p.prs.size, p.n, p.sev.blocking ?? 0, p.prs.size ? p.n / p.prs.size : null, ratio(p.out.fixed ?? 0, p.n - (p.out.open ?? 0)),
    p.out.fixed ?? 0, p.out.rejected ?? 0, p.out.merged ?? 0, p.out.noanswer ?? 0, p.out.open ?? 0, p.fixes, p.rev, p.fix, p.prs.size ? (p.rev + p.fix) / p.prs.size : null];
  const cols = ["PRs", "Findings", "Blocking", "Findings / PR", "Fix rate", "Fixed", "Rejected", "Merged unfixed", "No answer", "Open", "Fix PRs", "Review cost ($)", "Fix cost ($)", "Cost / PR ($)"];
  const detail = (p: Review) => p.findings.map((f) => `${f.out} · ${f.lab} · ${f.seats.join(" + ")} · ${f.file}:${f.line} · raised ${f.d} · last ${f.dl}${f.r ? ` · round ${f.r}` : ""}`);
  const group = (key: (p: Review) => string) => { const groups = new Map<string, Review>(); for (const p of prs.values()) { const k = key(p), g = groups.get(k) ?? create(k); merge(g, p); groups.set(k, g); } return [...groups].map(([k, g]) => row(k, [k, ...cells(g)], detail(g))); };
  const findingGroup = (keys: (f: typeof findings[number]) => string[]) => { const groups = new Map<string, Review>(); for (const f of findings) for (const k of keys(f)) { const g = groups.get(k) ?? create(k); addFinding(g, f); groups.set(k, g); } return [...groups].map(([k, g]) => row(k, [k, ...cells(g)], detail(g))); };
  if (q.page === "cr-overview") {
    tiles.push({ label: "PRs reviewed", value: prs.size, note: `${[...prs.values()].filter((p) => p.n).length} with findings` }, { label: "Findings", value: all.n, note: `${all.sev.blocking ?? 0} blocking` },
      { label: "Findings per PR", value: prs.size ? all.n / prs.size : null, note: "PRs with findings or review cost" },
      { label: "Fix rate", value: ratio(all.out.fixed ?? 0, all.n - (all.out.open ?? 0)), note: "Fixed ÷ all findings except open. A floor: author commits may be unobserved." },
      { label: "Rejected", value: all.out.rejected ?? 0, note: "Declined by the author or operator" }, { label: "Left unanswered", value: (all.out.merged ?? 0) + (all.out.noanswer ?? 0), note: "Merged unfixed + closed without answer" },
      { label: "Desk fix PRs", value: fixes.length, note: `${fixes.filter((f) => f.st === "merged").length} merged by author` },
      { label: "Desk cost", value: usd(desk), note: `Review ${usd(all.rev)} · fix ${usd(all.fix)} · coordination ${usd(desk - all.rev - all.fix)}` },
      { label: "Cost per PR", value: prs.size ? usd((all.rev + all.fix) / prs.size) : null, note: "Review + fix, excluding coordination" });
    tables.push(table("review-severity", "By severity", ["Severity", ...cols], findingGroup((f) => [f.lab])));
  }
  if (["cr-overview", "cr-fix"].includes(q.page)) {
    const groups = new Map<string, Record<string, number>>();
    const days = [...new Set(findings.map((f) => f.d))].sort(), weekly = days.length > 0 && elapsedDays(days[0], days.at(-1))! > 60;
    for (const f of findings) { const day = weekly ? weekOf(f.d) : f.d, g = groups.get(day) ?? {}; g[f.out] = (g[f.out] ?? 0) + 1; groups.set(day, g); }
    tables.push(table("review-trend", q.page === "cr-fix" ? "Outcome share and fix rate over time" : "Findings by day raised", [weekly ? "Week" : "Day", "Fixed", "Rejected", "Merged unfixed", "No answer", "Open", "Fix rate"], [...groups].sort().map(([k, g]) => row(k, [k, ...["fixed", "rejected", "merged", "noanswer", "open"].map((o) => g[o] ?? 0), ratio(g.fixed ?? 0, Object.values(g).reduce((a, b) => a + b, 0) - (g.open ?? 0))])), "Counts by day raised; outcome is the latest desk state. Exact data is available with the bars.", true));
  }
  if (q.page === "cr-fix") {
    tables.push(table("reviewer", "By reviewer seat", ["Reviewer", ...cols], findingGroup((f) => f.seats), "A finding raised by two reviewers counts once for each reviewer."));
    tables.push(table("review-type", "By file type", ["File type", ...cols], findingGroup((f) => [f.file.split(".").at(-1) ?? "(no extension)"])));
    const states = [...new Set(fixes.map((f) => f.st))];
    tables.push(table("fix-prs", "Desk fix PRs", ["Status", "Fix PRs", "Author PRs", "Round findings", "Findings fixed"], states.map((s) => { const fs = fixes.filter((f) => f.st === s); return row(s, [s, fs.length, new Set(fs.map((f) => f.pr)).size, fs.reduce((n, f) => n + f.n, 0), fs.reduce((n, f) => n + f.fx, 0)], fs.map((f) => `${f.pr} · round ${f.r ?? "—"} · fix PR ${f.fixPr ?? "not opened"} · ${f.d}`)); })));
  }
  if (q.page === "cr-submitters") tables.push(table("submitters", "Submitters", ["Submitter", ...cols], group((p) => rv.prs[p.key]?.author ?? "unknown")));
  if (q.page === "cr-repos") tables.push(table("review-repos", "Repositories", ["Repository", ...cols], group((p) => rv.prs[p.key]?.repo ?? p.key.split("#")[0]!)));
  if (q.page === "cr-prs") tables.push(table("review-prs", "Pull requests", ["PR", "Title", "Author", "Status", "Jira key", ...cols], [...prs.values()].map((p) => { const meta = rv.prs[p.key]; return row(p.key, [p.key, meta?.title ?? "", meta?.author ?? "unknown", `${meta?.status ?? "unknown"}${meta?.draft ? " (draft)" : ""}${meta?.judge ? ` · JUDGE ${meta.judge}` : ""}`, meta?.key ?? "", ...cells(p)], detail(p), meta ? safeLink(`https://github.com/${meta.repo}/pull/`, String(meta.n)) : undefined); })));
  return { tiles, tables, notes: [`Live desk state as of ${rv.asOf ?? "unknown"}; since ${rv.since ?? "unknown"}. Only Period applies; revisions and other filters do not.`, "Fix rate = fixed ÷ decided (every finding except open). Review costs come from existing PR-workspace usage, with coordination separate."] };
}

function deliveryTables(d: TokenomicsData, live: TokenomicsLive, q: TokenomicsQuery): BuiltTokenomics {
  const dv = live.delivery, tiles: TokenomicsView["tiles"] = [], tables: Table[] = [];
  if (!dv) return { tiles, tables, notes: ["Delivery is unavailable: no delivery snapshot was collected on this host. This is missing data, not zero delivery."] };
  const pd = periodDays(q, dayOf(Date.parse(d.generated))), inP = (day: string | null | undefined) => inDays(day, pd), bots = new Set(dv.bots);
  const prs = dv.prs.filter((p) => !bots.has(p.a) && (inP(p.c) || inP(p.m))), direct = dv.direct.filter((c) => !bots.has(c.a) && inP(c.d)), bugs = dv.bugs.filter((b) => inP(b.c));
  const name = (a: string) => dv.people[a] ?? a;
  const create = (key: string) => ({ key, prs: 0, opened: 0, cm: 0, direct: 0, rw: 0, own: 0, ttm: [] as number[], size: [] as number[], nokey: 0, bin: 0, bfix: 0, bugs: new Set<string>(), la: 0, ld: 0, ta: 0, td: 0, da: 0, dd: 0 });
  type Delivery = ReturnType<typeof create>;
  const people = new Map<string, Delivery>(), repos = new Map<string, Delivery>();
  const get = (map: Map<string, Delivery>, key: string) => { let a = map.get(key); if (!a) { a = create(key); map.set(key, a); } return a; };
  const both = (who: string, repo: string, fn: (a: Delivery) => void) => { fn(get(people, who)); fn(get(repos, repo)); };
  const lines = ["la", "ld", "ta", "td", "da", "dd"] as const;
  for (const p of prs) {
    const repo = p.ref.split("#")[0]!;
    if (inP(p.c)) both(p.a, repo, (a) => a.opened++);
    if (p.st === "merged" && inP(p.m)) both(p.a, repo, (a) => {
      a.prs++; a.cm += p.cm; if (!p.keys.length) a.nokey++; if (p.hm) a.ttm.push((Date.parse(p.hm) - Date.parse(p.hc)) / 3600000);
      if (p.main) { for (const k of lines) a[k] += p[k]; a.rw += p.rw; a.size.push(p.la + p.ld + p.ta + p.td); }
    });
  }
  direct.forEach((c) => both(c.a, c.repo, (a) => { a.direct++; for (const k of lines) a[k] += c[k]; a.rw += c.rw; }));
  for (const [day, repo, who, , n] of dv.churn) if (inP(day) && !bots.has(who)) both(who, repo, (a) => a.own += n);
  for (const b of dv.bugs) {
    const total = b.intro.reduce((n, i) => n + i.n, 0);
    for (const i of b.intro) if (inP(i.d) && i.d >= dv.since && !bots.has(i.a) && total > 0) both(i.a, i.repo, (a) => { a.bin += i.n / total; a.bugs.add(b.key); });
    const fx = b.fix.filter((f) => inP(f.d) && !bots.has(f.a)), authors = [...new Set(fx.map((f) => f.a))], rs = [...new Set(fx.map((f) => f.repo))];
    authors.forEach((a) => get(people, a).bfix += 1 / authors.length); rs.forEach((r) => get(repos, r).bfix += 1 / rs.length);
  }
  const all = create("all");
  for (const a of people.values()) {
    for (const k of ["prs", "opened", "cm", "direct", "rw", "own", "nokey", "bin", "bfix", ...lines] as const) all[k] += a[k];
    all.ttm.push(...a.ttm); all.size.push(...a.size);
  }
  const churn = (a: Delivery) => a.la + a.ta ? 100 * a.own / (a.la + a.ta) : null, perK = (a: Delivery) => a.la >= 500 ? a.bin / (a.la / 1000) : null;
  const cols = ["PRs merged", "Opened", "Direct commits", "Code added", "Code deleted", "Tests added", "Tests deleted", "Docs added", "Docs deleted", "Test share", "Median PR size", "Median time to merge (h)", "Churn (%)", "Rework", "No Jira key", "Bugs introduced", "Bugs / 1K code lines", "Bugs fixed"];
  const cells = (a: Delivery): Cell[] => [a.prs, a.opened, a.direct, a.la, a.ld, a.ta, a.td, a.da, a.dd, ratio(a.ta, a.la + a.ta), median(a.size), median(a.ttm), churn(a), a.rw, ratio(a.nokey, a.prs), a.bin, perK(a), a.bfix];
  const quality = `Lines are default-branch net diffs; tests/docs separate; generated files excluded. Churn: code/test lines rewritten within ${dv.churnDays} days ÷ added lines. Rework is charged to the changing author. Bugs / 1K is shown from 500 code lines. SZZ is partial evidence; recent periods read low.`;
  const merged = prs.filter((p) => p.st === "merged" && inP(p.m));
  const prRows = (items: typeof prs) => items.map((p) => row(p.ref, [p.ref, p.t, name(p.a), `${p.st}${p.dr ? " (draft)" : ""}`, p.base, p.c, p.m ?? null,
    p.hm ? (Date.parse(p.hm) - Date.parse(p.hc)) / 3600000 : null, p.main ? p.la : null, p.main ? p.ld : null, p.main ? p.ta : null, p.main ? p.td : null, p.main ? p.da : null, p.main ? p.dd : null, p.main ? p.rw : null, p.cm, p.keys.join(", ")], [], safeLink(`https://github.com/${dv.org}/${p.ref.split("#")[0]}/pull/`, p.ref.split("#")[1]!)));
  const prCols = ["PR", "Title", "Author", "State", "Base", "Opened", "Merged", "Time to merge (h)", "Code added", "Code deleted", "Tests added", "Tests deleted", "Docs added", "Docs deleted", "Rework", "Commits", "Jira keys"];
  const daily = (id: string, title: string, values: { day: string; key: string }[]) => {
    const days = values.map((v) => v.day).sort(), weekly = days.length > 0 && elapsedDays(days[0], days.at(-1))! > 60;
    const groups = new Map<string, { day: string; who: string; n: number }>();
    values.forEach((v) => { const day = weekly ? weekOf(v.day) : v.day, key = JSON.stringify([day, v.key]), g = groups.get(key) ?? { day, who: v.key, n: 0 }; g.n++; groups.set(key, g); });
    return table(id, title, [weekly ? "Week" : "Day", "Series", "Count"], [...groups].sort().map(([k, g]) => row(k, [g.day, g.who, g.n])), "Exact values accompany native bars.", true);
  };
  if (q.page === "dv-overview") {
    tiles.push({ label: "PRs merged", value: all.prs, note: `${all.opened} opened in period · ${dv.prs.filter((p) => p.st === "open" && !bots.has(p.a)).length} open now` },
      { label: "Code lines added", value: all.la, note: `−${all.ld} deleted · tests +${all.ta}/−${all.td} · docs +${all.da}/−${all.dd}` },
      { label: "Median time to merge", value: median(all.ttm), note: `median PR ${median(all.size) ?? "—"} lines of code and tests` },
      { label: "Churn (%)", value: churn(all), note: `${all.own} lines rewritten within ${dv.churnDays} days` },
      { label: "PRs without Jira key", value: ratio(all.nokey, all.prs), note: `${all.nokey} of ${all.prs} merged · ${all.direct} direct commits` },
      { label: "Bugs reported", value: bugs.length, note: `${bugs.filter((b) => b.why === "traced").length} traced` },
      { label: "Bugs per 1K code lines", value: perK(all), note: `${all.bin.toFixed(1)} bugs traced to code merged in period` });
    tables.push(daily("delivery-trend", "PRs merged over time by author", merged.map((p) => ({ day: p.m!, key: name(p.a) }))));
  }
  if (q.page === "dv-people" || q.page === "dv-repos") {
    const isPeople = q.page === "dv-people";
    tables.push(table(isPeople ? "delivery-people" : "delivery-repos", isPeople ? "People: output and quality" : "Repositories: output and quality", [isPeople ? "Person" : "Repository", ...cols], [...(isPeople ? people : repos)].map(([k, a]) => row(k, [isPeople ? name(k) : k, ...cells(a)])), quality));
  }
  if (q.page === "dv-prs") tables.push(table("delivery-prs", "Pull requests", prCols, prRows(prs), "Only Period applies. PRs merged to a non-default branch retain unknown line counts."));
  if (q.page === "dv-bugs") {
    const traced = bugs.filter((b) => b.why === "traced"), lag = traced.map((b) => elapsedDays(b.intro.map((i) => i.d).sort().at(-1), b.c)).filter((v): v is number => v !== null && v >= 0);
    tiles.push({ label: "Bugs reported", value: bugs.length, note: `${bugs.filter((b) => b.cat === "done").length} done in Jira` }, { label: "With a fix in git", value: bugs.filter((b) => b.fix.length).length, note: "Merged PR or commit names the bug" },
      { label: "Traced", value: ratio(traced.length, bugs.length), note: "Introducing commit found" }, { label: "Introduction → report", value: median(lag) == null ? null : `${median(lag)} d`, note: "median days from the code landing to the bug being reported" }, { label: "Linked in Jira", value: bugs.filter((b) => b.cause.length).length, note: "Has an is-caused-by link" });
    tables.push(daily("bug-trend", "Bugs reported by trace result", bugs.map((b) => ({ day: b.c, key: b.why }))));
    tables.push(table("bug-people", "Who introduced and fixed bugs", ["Person", ...cols, "Bugs"], [...people].filter(([, a]) => a.bin || a.bfix || a.la).map(([k, a]) => row(k, [name(k), ...cells(a), [...a.bugs].join(", ")])), quality));
    tables.push(table("bugs", "Bugs", ["Bug", "Summary", "Priority", "State", "Reported", "Introduced by", "Days to report", "Fixed by", "Trace"], bugs.map((b) => row(b.key, [b.key, b.sum, b.pri ?? null, b.s, b.c, [...new Set(b.intro.map((i) => name(i.a)))].join(", ") || null, elapsedDays(b.intro[0]?.d, b.c), [...new Set(b.fix.map((f) => name(f.a)))].join(", ") || null, b.why],
      [...b.cause.map((k) => `Jira: is caused by ${k} · ${dv.issues[k]?.sum ?? ""}`), ...b.intro.map((i) => `${ratio(i.n, b.intro.reduce((n, x) => n + x.n, 0))} of blamed lines (${i.n}) · ${name(i.a)} · ${i.d} · ${i.repo} · ${i.pr ?? i.c} · ${i.keys.join(", ")} · ${i.s}`), ...b.fix.map((f) => `Fix · ${name(f.a)} · ${f.d} · ${f.repo} · ${f.pr ?? f.c}`)], safeLink(d.jiraBase ?? "", b.key))), "Bugs by reported day. Introduction attribution is shared by blamed lines and charged on the introducing code's merge day."));
  }
  if (q.page === "dv-jira") {
    type Issue = { key: string; j: typeof dv.issues[string] | null; prs: typeof prs; merged: typeof prs; la: number; ld: number; rw: number; flag: string; first: string | null; last: string | null };
    const issues = new Map<string, Issue>(), getIssue = (k: string) => { let x = issues.get(k); if (!x) { x = { key: k, j: dv.issues[k] ?? null, prs: [], merged: [], la: 0, ld: 0, rw: 0, flag: "", first: null, last: null }; issues.set(k, x); } return x; };
    for (const p of dv.prs) if (!bots.has(p.a)) for (const k of p.keys) { const x = getIssue(k); x.prs.push(p); if (p.st === "merged") { x.merged.push(p); if (p.main) { x.la += p.la; x.ld += p.ld; x.rw += p.rw; } } }
    for (const e of d.epics) for (const c of e.children) { const x = getIssue(c.key); x.j ??= { t: "Issue", s: c.status, cat: c.cat, sum: c.summary, r: c.resolved, e: e.key }; }
    const today = d.generated.slice(0, 10), rows: Issue[] = [];
    for (const x of issues.values()) {
      const j = x.j, open = x.prs.filter((p) => p.st === "open"); x.first = x.prs.map((p) => p.hc).sort()[0] ?? null; x.last = x.merged.map((p) => p.hm).filter((s): s is string => !!s).sort().at(-1) ?? null;
      x.flag = j?.cat === "done" && !x.prs.length && (j.r ?? "") >= dv.since ? "done, no PR" : j?.cat === "indeterminate" && !x.prs.length ? "in progress, no PR"
        : x.merged.length && !open.length && j?.cat && j.cat !== "done" && (elapsedDays(x.last, today) ?? -1) >= 3 ? "merged, Jira not done"
          : open.some((p) => !p.dr && (elapsedDays(p.hc, today) ?? 0) > 7) ? "PR open over 7 days" : "";
      if ((x.prs.length || x.flag) && ([x.first, x.last, j?.c, j?.st, j?.r, ...x.prs.map((p) => p.hm)].some(inP) || (x.flag && x.flag !== "done, no PR"))) rows.push(x);
    }
    const cost = new Map<string, number>();
    for (const f of d.facts) if (inP(dayOf(f[1] * 3600000))) { const k = d.seats[f[0]]!.epic; cost.set(k, (cost.get(k) ?? 0) + f[10]); }
    const epicGroups = new Map<string, { issues: number; done: number; flags: number; prs: Set<string>; la: number; ld: number; rw: number }>();
    for (const x of rows) { const k = x.j?.e ?? "(no epic)", g = epicGroups.get(k) ?? { issues: 0, done: 0, flags: 0, prs: new Set(), la: 0, ld: 0, rw: 0 };
      if (x.j?.t !== "Epic") { g.issues++; if (x.j?.cat === "done") g.done++; if (x.flag) g.flags++; }
      for (const p of x.merged) if (inP(p.m) && !g.prs.has(p.ref)) { g.prs.add(p.ref); if (p.main) { g.la += p.la; g.ld += p.ld; g.rw += p.rw; } } epicGroups.set(k, g);
    }
    tables.push(table("jira-epics", "By epic", ["Epic", "Issues", "Done", "PRs merged", "Code added", "Code deleted", "Rework", "Flagged", "AI cost ($)", "AI cost / PR ($)"], [...epicGroups].map(([k, g]) => row(k, [k, g.issues, g.done, g.prs.size, g.la, g.ld, g.rw, g.flags, cost.get(k) ?? 0, g.prs.size && cost.get(k) ? cost.get(k)! / g.prs.size : null])), "AI cost is this machine's usage in the period; revision and other filters do not apply."));
    tables.push(table("jira-issues", "By issue", ["Issue", "Summary", "Epic", "Jira status", "Assignee", "Created", "Started", "Done", "First PR", "Last merge", "Start → PR (days)", "PR → merge (days)", "Merge → done (days)", "Merged PRs", "All PRs", "Code added", "Code deleted", "Flag"], rows.filter((x) => x.j?.t !== "Epic").map((x) => row(x.key, [x.key, x.j?.sum ?? "not readable", x.j?.e ?? null, x.j?.s ?? null, x.j?.asg ?? null, x.j?.c ?? null, x.j?.st ?? null, x.j?.r ?? null, x.first, x.last, elapsedDays(x.j?.st, x.first), elapsedDays(x.first, x.last), elapsedDays(x.last, x.j?.r), x.merged.length, x.prs.length, x.la, x.ld, x.flag], x.prs.map((p) => `${p.ref} · ${name(p.a)} · ${p.st}${p.dr ? " (draft)" : ""} · opened ${p.c} · merged ${p.m ?? "—"} · ${p.t}`), safeLink(d.jiraBase ?? "", x.key))), "Issues may belong to several PRs. A PR counts once per epic. Existing unresolved flags remain visible across periods."));
  }
  return { tiles, tables, notes: [`Live GitHub ${dv.org}: ${dv.repos.length} repositories, PRs since ${dv.since}; as of ${dv.asOf}. Only Period applies; bots excluded.`, ...(dv.jira ? [] : ["Jira was unavailable on the last collection; issue/bug coverage is incomplete."]), ...dv.errors, quality] };
}

function activityTables(d: TokenomicsData, live: TokenomicsLive, q: TokenomicsQuery, m: ReturnType<typeof usageModel>): BuiltTokenomics {
  const tables: Table[] = [], notes = ["Timeline and parallel work cover lifetime up to the selected revision cut and ignore Period; other filters apply. Daily consumption follows Period. Parallel work counts distinct active seats in the busiest clock hour, not exact simultaneous execution."];
  type Day = { value: Aggregate; hours: Map<number, Set<number>>; prs: number; cm: number; lines: number };
  type Days = Map<string, Day>;
  const create = (): Day => ({ value: blank(), hours: new Map(), prs: 0, cm: 0, lines: 0 });
  const team: Days = new Map(), people = new Map<string, Days>(), workstreamDays = new Map<string, Days>(), epics = new Map<string, Days>(), byPersonEpic = new Map<string, { user: string; ws: string; epic: string; days: Days }>();
  const getDays = (map: Map<string, Days>, k: string) => { let x = map.get(k); if (!x) { x = new Map(); map.set(k, x); } return x; };
  const getDay = (days: Days, day: string) => { let x = days.get(day); if (!x) { x = create(); days.set(day, x); } return x; };
  const mapsFor = (user: string, ws: string, epic: string) => {
    const key = JSON.stringify([user, ws, epic]); let x = byPersonEpic.get(key);
    if (!x) { x = { user, ws, epic, days: new Map() }; byPersonEpic.set(key, x); }
    return [team, getDays(people, user), getDays(workstreamDays, JSON.stringify([user, ws])), getDays(epics, epic), x.days];
  };
  for (const f of m.filtered(m.rev, true)) {
    if (!f[9]) continue;
    const s = d.seats[f[0]]!, day = dayOf(f[1] * 3600000);
    for (const days of mapsFor(s.user || "Unattributed", s.ws, s.epic)) {
      const x = getDay(days, day); add(x.value, f); const hs = x.hours.get(f[1]) ?? new Set<number>(); hs.add(f[0]); x.hours.set(f[1], hs);
    }
  }
  const visibleWs = new Set(Object.keys(d.ws).filter(m.wsOn));
  for (const [user, day, epic, prs, cm, la, ld] of live.dev?.rows ?? []) {
    const ws = m.epics.get(epic)?.ws;
    if (!ws || !visibleWs.has(ws) || (q.filters.user !== undefined && !q.filters.user.includes(user)) || (q.filters.epic !== undefined && !q.filters.epic.includes(m.epicLabel(epic)))) continue;
    for (const days of mapsFor(user, ws, epic)) { const x = getDay(days, day); x.prs += prs; x.cm += cm; x.lines += la + ld; }
  }
  const peak = (x: Day) => Math.max(0, ...[...x.hours.values()].map((s) => s.size));
  const value = (x: Day): number | null => {
    const a = x.value;
    switch (q.metric) {
      case "act": return a.act / 3600; case "cost": return a.cost; case "tok": return tokens(a); case "peak": return peak(x);
      case "cph": return a.act >= 600 ? a.cost * 3600 / a.act : null; case "hit": return a.fresh + a.read + a.write ? 100 * a.read / (a.fresh + a.read + a.write) : null;
      case "prs": return live.dev ? x.prs : null; case "cm": return live.dev ? x.cm : null; case "lines": return live.dev ? x.lines : null; case "cpp": return live.dev && x.prs ? a.cost / x.prs : null;
    }
  };
  const today = dayOf(Date.parse(d.generated)), since = q.range === "all" ? "" : addDays(today, 1 - Number(q.range));
  const timeline: Row[] = [];
  const pushDays = (label: string, days: Days, user: string, ws = "", epic = "") => {
    for (const [day, x] of [...days].sort()) if (day >= since) timeline.push(row(JSON.stringify([label, day, ws, epic]), [label, user, ws, epic ? m.epicLabel(epic) : "", day, x.value.n || (["prs", "cm", "lines", "cpp"].includes(q.metric) && x.prs + x.cm + x.lines) ? value(x) : null, x.value.act / 3600, x.value.cost, tokens(x.value), peak(x), x.prs, x.cm, x.lines, x.value.fresh + x.value.read + x.value.write, x.value.read, x.value.n],
      [`Tasks resolved: ${d.epics.flatMap((e) => e.children.filter((c) => c.resolved === day).map((c) => c.key)).join(", ") || "none recorded"}`]));
  };
  pushDays("Team total", team, "All users");
  for (const [user, days] of people) pushDays("Person", days, user);
  for (const [key, days] of workstreamDays) { const [user, ws] = JSON.parse(key) as [string, string]; pushDays("Workstream", days, user, d.ws[ws]!.name); }
  for (const x of byPersonEpic.values()) pushDays("Epic / desk row", x.days, x.user, d.ws[x.ws]!.name, x.epic);
  tables.push(table("timeline", "Activity timeline: team → people → workstreams → epics", ["Level", "Person", "Workstream", "Epic", "Day", ACTIVITY_METRICS[q.metric], "Seat-hours", "Cost ($)", "Tokens", "Peak seats", "PRs merged", "Commits", "Lines changed", "Input tokens", "Cached input tokens", "Model responses"], timeline, "Use search to isolate a person, workstream, epic or date. Expand a row for Jira progress. Exact values provide the accessible chart alternative.", true));
  const merge = (days: Days, start: string, end: string): Day => {
    const out = create();
    for (const [day, x] of days) if (day >= start && day <= end) {
      for (const k of Object.keys(out.value) as (keyof Aggregate)[]) if (k !== "seats") out.value[k] += x.value[k];
      x.value.seats.forEach((s) => out.value.seats.add(s)); x.hours.forEach((s, h) => out.hours.set(h, s)); out.prs += x.prs; out.cm += x.cm; out.lines += x.lines;
    }
    return out;
  };
  tables.push(table("last-seven", "Last 7 days versus the previous 7", ["Person", "Last 7", "Previous 7", "Change", "Change (%) / pp"], [...people].map(([user, days]) => {
    const cur = value(merge(days, addDays(today, -6), today)), prev = value(merge(days, addDays(today, -13), addDays(today, -7)));
    return row(user, [user, cur, prev, cur !== null && prev !== null ? cur - prev : null, cur !== null && prev !== null ? q.metric === "hit" ? cur - prev : prev ? 100 * (cur - prev) / prev : cur ? "new" : "—" : null]);
  })));
  const flags: Row[] = [];
  const usual = (history: number[]) => { const values = history.slice(-28); if (values.length < 7) return null; const med = median(values)!, mad = median(values.map((v) => Math.abs(v - med)))!; return { med, mad, hi: mad ? med + 3.5 * mad / 0.6745 : Infinity, lo: mad ? med - 3.5 * mad / 0.6745 : -Infinity }; };
  for (const [user, days] of people) {
    const history: Record<string, number[]> = { act: [], cost: [], cph: [], hit: [] };
    for (const [day, x] of [...days].sort()) {
      const a = x.value; if (!a.n) continue;
      const big = a.act >= 3600 && a.cost >= 50, vals: Record<string, number | null> = { act: a.act / 3600, cost: a.cost, cph: big ? a.cost * 3600 / a.act : null, hit: big && a.fresh + a.read + a.write ? 100 * a.read / (a.fresh + a.read + a.write) : null };
      for (const [k, v] of Object.entries(vals)) {
        if (v === null) continue; const b = usual(history[k]!);
        const flagged = b && (k === "act" || k === "cost" ? v > b.hi && v >= 2 * b.med && v >= (k === "act" ? 8 : 100) : k === "cph" ? v > b.hi && v >= 1.5 * b.med : v <= b.med - 5 && (!b.mad || v < b.lo));
        if (flagged && b) flags.push(row(`${user}-${day}-${k}`, [k === "act" || k === "cost" ? "Spike" : "Efficiency drop", user, "", day, day, k, v, b.med, b.hi === Infinity ? "no finite band" : b.hi], [`Median of previous 28 active days; at least 7 days. ${k === "hit" ? "At least 5 percentage points below usual." : "Modified z-score threshold 3.5."}`]));
        history[k]!.push(v);
      }
    }
  }
  const selectedEpics = d.epics.filter((e) => visibleWs.has(e.ws) && (q.filters.epic === undefined || q.filters.epic.includes(m.epicLabel(e.key))));
  const nDone = selectedEpics.reduce((n, e) => n + e.children.filter((c) => c.resolved).length, 0), lifetimeCost = selectedEpics.reduce((n, e) => n + [...(epics.get(e.key)?.values() ?? [])].reduce((a, x) => a + x.value.cost, 0), 0), cpi = nDone ? lifetimeCost / nDone : Infinity;
  for (const e of selectedEpics) {
    const days = epics.get(e.key) ?? new Map<string, Day>(), progress = new Set(e.children.map((c) => c.resolved).filter((s): s is string => !!s));
    for (const [day, x] of days) if (x.prs + x.cm) progress.add(day);
    let run: { from: string; to: string; days: number; cost: number; at: string | null } | null = null;
    const flush = (open: boolean) => { if (run && run.at) flags.push(row(`swp-${e.key}-${run.from}`, ["Spend without progress", "", e.key, run.from, open && !e.resolved ? today : run.to, "cost", run.cost, cpi, 3 * cpi, run.at], [`${run.days} active days without resolved issue or delivery progress.`])); run = null; };
    for (const day of [...new Set([...days.keys(), ...progress])].sort()) {
      if (progress.has(day)) { flush(false); continue; } const x = days.get(day); if (!x?.value.n) continue;
      run ??= { from: day, to: day, days: 0, cost: 0, at: null }; run.to = day; run.days++; run.cost += x.value.cost; if (!run.at && run.days >= 3 && run.cost >= 3 * cpi) run.at = day;
    }
    flush(true);
    if (e.due && !e.resolved && today > addDays(e.due, 10)) flags.push(row(`due-${e.key}`, ["Overdue", "", e.key, addDays(e.due, 10), today, "days", elapsedDays(e.due, today), null, 10], [`Jira due date ${e.due}`]));
  }
  tables.push(table("flags", "Activity flags", ["Flag", "Person", "Epic", "From", "To", "Metric", "Value", "Usual / cost per issue", "Threshold", "Band starts"], flags.map((r) => r.cells.length < 10 ? { ...r, cells: [...r.cells, null] } : r), "Signals use the existing dashboard thresholds. Missing Jira due dates and missing delivery data remain unknown."));
  const flagTable = tables.find((t) => t.id === "flags")!;
  const episodes: { rows: Row[]; from: string; to: string; key: string }[] = [];
  for (const f of [...flags].sort((a, b) => String(a.cells[3]).localeCompare(String(b.cells[3])))) {
    const [type, person, epic, from, to, metric] = f.cells, key = JSON.stringify([type, person, epic, metric]);
    const previous = ["Spike", "Efficiency drop"].includes(String(type)) ? episodes.filter((e) => e.key === key).at(-1) : undefined;
    if (previous && Date.parse(String(from)) - Date.parse(previous.to) <= 2 * 86400000) { previous.rows.push(f); previous.to = String(to); }
    else episodes.push({ rows: [f], from: String(from), to: String(to), key });
  }
  const visibleFlags = episodes.filter((e) => e.to >= since).sort((a, b) => b.to.localeCompare(a.to) || b.from.localeCompare(a.from));
  flagTable.title = `Needs attention — ${visibleFlags.length ? `${visibleFlags.length} flag${visibleFlags.length === 1 ? "" : "s"} in this range` : "nothing flagged in this range"}`;
  flagTable.display = { columns: ["When", "Who / where", "Signal", "What happened"], rows: (q.expanded.includes("flags:all") ? visibleFlags : visibleFlags.slice(0, 8)).map((e) => {
    const severity = (r: Row) => r.cells[5] === "hit" ? Number(r.cells[7]) - Number(r.cells[6]) : Number(r.cells[6]) / Math.max(Number(r.cells[7]), 1e-9);
    const worst = [...e.rows].sort((a, b) => severity(b) - severity(a))[0]!, [type, person, epic, , , metric, value, usual] = worst.cells;
    const signal = ["Spike", "Efficiency drop", "Spend without progress", "Overdue"].indexOf(String(type)), fmt = (n: Cell) => typeof n !== "number" ? String(n ?? "—") : metric === "act" ? hours(n * 3600) : metric === "hit" ? `${n.toFixed(1)}%` : metric === "days" ? `${n} days` : usd(n);
    return row(e.key + e.from, [e.from === e.to ? e.from.slice(5) : `${e.from.slice(5)} → ${e.to === today && ["Overdue", "Spend without progress"].includes(String(type)) ? "today" : e.to.slice(5)}`, epic ? m.epicLabel(String(epic)) : person!, `${["▲", "◆", "▬", "▼"][signal]} ${type}${["Spike", "Efficiency drop"].includes(String(type)) ? ` · ${{ act: "seat-time", cost: "API cost", cph: "cost/hour", hit: "cache hit" }[String(metric)] ?? metric}` : ""}`, `${e.rows.length > 1 ? `Worst on ${String(worst.cells[3]).slice(5)} of ${e.rows.length} flagged days: ` : ""}${fmt(value!)}${usual == null ? "" : `; usual ${fmt(usual)}`} · ${worst.detail.join(" ")}`]);
  }) };
  flagTable.note = "";
  const dualFacts = m.filtered(m.rev, true).filter((f) => q.dual === "all" || d.seats[f[0]]!.ws === q.dual || d.seats[f[0]]!.epic === q.dual), daily = new Map<string, Day>();
  for (const f of dualFacts) if (f[9]) { const day = dayOf(f[1] * 3600000), x = getDay(daily, day); add(x.value, f); const hs = x.hours.get(f[1]) ?? new Set(); hs.add(f[0]); x.hours.set(f[1], hs); }
  const parallelRows: Row[] = [], firstDay = [...daily.keys()].sort()[0], selectedEpic = m.epics.get(q.dual);
  const spanStart = selectedEpic?.created || firstDay, spanEnd = [selectedEpic?.resolved || today, ...daily.keys()].sort().at(-1)!;
  if (spanStart) {
    const spanDays = elapsedDays(spanStart, spanEnd) ?? 0;
    if (spanDays > 10000) throw new Error("Report activity span exceeds the bounded native calendar");
    for (let n = 0; n <= spanDays; n++) { const day = addDays(spanStart, n), x = daily.get(day); parallelRows.push(row(day, [day, x ? x.value.act / 3600 : 0, x ? peak(x) : 0, x ? "no" : "yes"])); }
  }
  tables.push(table("parallel", "Daily activity and parallel work", ["Day", "Active seat-hours", "Peak parallel seats", "Idle in span"], parallelRows, "Independent quantities: seat-hours sum all work; peak seats counts the busiest clock hour. Scope starts at first activity (or Jira creation for an epic).", true));
  tables.push(table("dates", "Epic and desk dates", ["Workstream", "Epic / desk", "Created", "First seat", "First activity", "Last activity", "Resolved", "Due", "Idle days"], [...new Set([...selectedEpics.map((e) => e.key), ...epics.keys()])].map((key) => {
    const e = m.epics.get(key), days = [...(epics.get(key)?.keys() ?? [])].sort(), ss = d.seats.filter((s) => s.epic === key), first = e?.created || days[0], end = e?.resolved || today;
    return row(key, [e ? d.ws[e.ws]!.name : d.ws[ss[0]?.ws ?? ""]?.name ?? "", m.epicLabel(key), e?.created ?? null, ss.map((s) => s.created).sort()[0] ?? null, days[0] ?? null, days.at(-1) ?? null, e?.resolved ?? null, e?.due ?? null, first ? Math.max(0, (elapsedDays(first, end) ?? 0) + 1 - days.length) : null]);
  })));
  const dates = tables.find((t) => t.id === "dates")!;
  dates.title = "Epic start dates, active and idle days";
  dates.display = { columns: ["Epic", "Status", "Workstream", "Jira created", "First seat", "First activity", "Last activity", "Resolved", "Span", "Active days", "Idle days", "Longest idle", "Peak parallel", "Avg parallel (active days)", "Active seat-hours", "API cost (lifetime)"], rows: dates.rows.map((r) => {
    const e = m.epics.get(r.key), ds = epics.get(r.key) ?? new Map<string, Day>(), active = [...ds].filter(([, x]) => x.value.n).map(([day]) => day).sort();
    const firstSeat = d.seats.filter((s) => s.epic === r.key).map((s) => dayOf(Date.parse(s.created))).sort()[0];
    const start = [e?.created, active[0]].filter((x): x is string => !!x).sort()[0], last = e?.resolved ? [e.resolved, active.at(-1) ?? e.resolved].sort().at(-1)! : today, end = last < today ? last : today;
    const span = start ? Math.max(0, (elapsedDays(start, end) ?? 0) + 1) : 0;
    let longest = 0, run = 0, longestEnd = "", peakDay = "", maxPeak = 0;
    for (let i = 0; i < span; i++) { const day = addDays(start!, i), x = ds.get(day); if (x?.value.n) run = 0; else if (++run > longest) { longest = run; longestEnd = day; } if (x && peak(x) > maxPeak) { maxPeak = peak(x); peakDay = day; } }
    return { ...r, cells: [m.epicLabel(r.key), e?.status ?? "", r.cells[0]!, e?.created ?? null, firstSeat ?? null, active[0] ?? null, active.at(-1) ?? null, e?.resolved ?? "open", `${span} d`, active.length, span - active.length, longest ? `${longest} d (to ${longestEnd.slice(5)})` : "0", maxPeak ? `${maxPeak} (${peakDay.slice(5)})` : "—", active.length ? active.reduce((n, day) => n + peak(ds.get(day)!), 0) / active.length : 0, [...ds.values()].reduce((n, x) => n + x.value.act / 3600, 0), [...ds.values()].reduce((n, x) => n + x.value.cost, 0)] };
  }) };
  const ff = m.facts, trendGroups = new Map<string, { labels: string[]; a: Aggregate }>();
  for (const f of ff) for (const part of q.stack === "tt" ? [0, 1, 2, 3] : [undefined]) {
    const labels = [dayOf(f[1] * 3600000), q.stack === "tt" ? ["Cache miss", "Cache read", "Cache write", "Output"][part!]! : m.dim(q.stack, f, d.seats[f[0]]!)], key = JSON.stringify(labels), g = trendGroups.get(key) ?? { labels, a: blank() };
    add(g.a, f, part); trendGroups.set(key, g);
  }
  tables.push(table("trend", "Daily consumption", ["Day", "Stack", q.measure === "cost" ? "API cost ($)" : q.measure === "tok" ? "Tokens" : "Active seat-hours"], [...trendGroups].sort().map(([k, g]) => row(k, [...g.labels, measure(g.a, q.measure)])), "Uses Period, filters and revision cut. Token types do not allocate active time.", true));
  const trend = tables.find((t) => t.id === "trend")!;
  trend.plot = { columns: trend.columns, rows: [], order: q.stack === "tt" ? ["Cache miss", "Cache read", "Cache write", "Output"] : m.grouped(d.facts, [q.stack]).sort((a, b) => Number(b.cells[1]) - Number(a.cells[1])).map((r) => String(r.cells[0])) };
  const tl = tables.find((t) => t.id === "timeline")!;
  tl.plot = { columns: tl.columns, rows: [], workstreams: Object.values(d.ws).map((w) => w.name), epicOrder: Object.values(d.ws).flatMap((w) => w.epics.map(m.epicLabel)), desks: Object.values(d.ws).filter((w) => w.kind === "desk").map((w) => w.name), order: [...new Set(d.seats.map((s) => s.user || "Unattributed"))].sort(), events: selectedEpics.flatMap((e) => e.children.filter((c) => c.resolved).map((c) => [m.epicLabel(e.key), c.resolved!, c.key])), gaps: m.scope.filter((s) => s.data !== "ok").map((s) => [s.user, d.ws[s.ws]!.name, m.epicLabel(s.epic), s.created.slice(0, 10), s.data]) };
  if (!live.dev) notes.push("Delivery timeline metrics have not been collected; PRs, commits, lines and cost per PR remain unavailable.");
  return { tiles: [], tables, notes };
}

export function tokenomicsCsv(result: BuiltTokenomics): string {
  const escape = (value: Cell) => {
    let s = value === null ? "" : String(value);
    if (typeof value === "string" && /^[=+\-@\t\r]/.test(s)) s = `'${s}`;
    return `"${s.replaceAll('"', '""')}"`;
  };
  return result.tables.map((t) => [...(t.id === "pivot" ? [] : [[t.title]]), t.columns, ...t.rows.filter((r) => t.id !== "pivot" || !r.hasChildren).map((r) => r.cells)].map((r) => r.map(escape).join(",")).join("\r\n")).join("\r\n\r\n");
}
