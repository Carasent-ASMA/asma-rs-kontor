import { mkdtemp, mkdir, writeFile, rm, symlink } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { readFile } from "node:fs/promises";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { PluginServerContext } from "@getpaseo/plugin/server";
import { TOKENOMICS_PAGES, readTokenomics, tokenomicsDataSchema, tokenomicsLiveSchema, tokenomicsQuerySchema, tokenomicsViewSchema } from "../shared/tokenomics";
import { buildTokenomics, osloMidnight, paginateTokenomics, tokenomicsCsv, tokens, usageModel } from "../shared/tokenomics-model";
import { contributeTokenomics, parseLiveAssignment, readReportFile, readTokenomicsSnapshot, tokenomicsView } from "../server/tokenomics";
import { goBack, navigate, readPageView, writePageView } from "../client/navigation-state";
import { tokenomicsFixture, tokenomicsLiveFixture } from "./fixtures/tokenomics";

const d = tokenomicsFixture(), live = tokenomicsLiveFixture();
const query = (patch: object = {}) => tokenomicsQuerySchema.parse({ show: "all", ...patch });
const dirs: string[] = [];
afterEach(async () => { vi.unstubAllEnvs(); await Promise.all(dirs.splice(0).map((p) => rm(p, { recursive: true, force: true }))); });
async function folder() { const p = await mkdtemp(join(tmpdir(), "kontor-tokenomics-")); dirs.push(p); await mkdir(join(p, "_stats")); return p; }

describe("native Tokenomics accounting parity", () => {
  it.skipIf(!process.env.KONTOR_TOKENOMICS_ORACLE_TEMPLATE)("prepares the same synthetic snapshot for the existing renderer [oracle fixture]", async () => {
    const { resolve } = await import("node:path");
    const p = resolve("../../target/tokenomics/oracle"); await mkdir(join(p, "_stats"), { recursive: true });
    const template = await readFile(process.env.KONTOR_TOKENOMICS_ORACLE_TEMPLATE!, "utf8");
    await writeFile(join(p, "index.html"), template); await writeFile(join(p, "data.json"), JSON.stringify(d));
    await writeFile(join(p, "_stats/live.js"), `window.ASMA_LIVE=${JSON.stringify(live)};`);
    expect(template).toContain("function renderKpis");
  });
  it("keeps the existing 19-field accounting, reasoning inclusion, missing seats and unpriced tokens", () => {
    const m = usageModel(d, query());
    expect(tokens(m.total)).toBe(1400); expect(m.total.cost).toBe(6); expect(m.total.reas).toBe(40);
    expect(m.total.ar).toBe(340); expect(m.total.sub).toBe(80); expect(m.total.cU).toBe(2);
    expect(m.total.seats.size).toBe(2); expect(m.scope.length).toBe(3);
    const unpriced = m.grouped(m.facts, ["model"]).find((r) => r.cells[0] === "future-model")!;
    expect(unpriced.cells.slice(1, 3)).toEqual([0, 350]);
  });
  it("excludes the cut's first bucket and compares re-cut totals using the same current period", () => {
    const m = usageModel(d, query({ compare: "check-a" }));
    expect(tokens(m.total)).toBe(1400); expect(tokens(m.prior!)).toBe(350);
    const custom = usageModel(d, query({ period: "custom", from: "2026-11-02", to: "2026-11-02", compare: "check-a" }));
    expect(tokens(custom.total)).toBe(700); expect(tokens(custom.prior!)).toBe(0);
  });
  it("uses Oslo DST day boundaries instead of assuming every day has 24 hours", () => {
    expect((osloMidnight("2026-10-25") - osloMidnight("2026-10-24")) / 3600000).toBe(24);
    expect((osloMidnight("2026-10-26") - osloMidnight("2026-10-25")) / 3600000).toBe(25);
    expect((osloMidnight("2026-03-30") - osloMidnight("2026-03-29")) / 3600000).toBe(23);
  });
  it("separates captured progression from re-cut history and ignores its hidden filters", () => {
    const result = buildTokenomics(d, live, query({ page: "progress", rev: "check-b", compare: "check-a", period: "custom", from: "2026-11-03", to: "2026-11-03", filters: { user: ["nobody"] } }));
    expect(result.tables[0]!.rows.find((r) => r.cells[0] === "tok")!.cells.slice(1, 4)).toEqual([123, 999, 876]);
    expect(result.tables.find((t) => t.id === "progress-tasks")!.rows[0]!.cells.slice(2, 4)).toEqual(["To do", "Done"]);
  });
  it("retains the entire local ledger and Unknown bucket independently of the revision and filters", () => {
    const a = buildTokenomics(d, live, query({ page: "usage", rev: "check-a", period: "7", filters: { user: ["nobody"] } }));
    expect(a.tiles[0]!.value).toBe(700);
    const ledger = a.tables.find((t) => t.id === "ledger")!;
    expect(ledger.rows.reduce((n, r) => n + (r.cells[2] as number), 0)).toBe(700);
    expect(ledger.rows.find((r) => r.cells[0] === "UNKNOWN")!.cells[2]).toBe(200);
    expect(a.tables.find((t) => t.id === "unknown")!.rows[0]!.cells[1]).toBe(200);
  });
  it("uses live period-only Code Review, the decided-finding denominator and attributed desk costs", () => {
    const r = buildTokenomics(d, live, query({ page: "cr-overview", rev: "check-a", filters: { user: ["nobody"] } }));
    expect(r.tiles.find((t) => t.label === "Fix rate")!.value).toBe("100.0%");
    expect(r.tiles.find((t) => t.label === "Findings")!.value).toBe(2);
    expect(r.tiles.find((t) => t.label === "Desk cost")!.value).toBe("$2.00");
    expect(buildTokenomics(d, live, query({ page: "cr-overview", period: "custom", from: "2026-11-02", to: "2026-11-03" })).tiles.find((t) => t.label === "Findings")!.value).toBe(0);
  });
  it("excludes bots and preserves delivery line, churn, SZZ and period semantics", () => {
    const a = buildTokenomics(d, live, query({ page: "dv-overview", rev: "check-a", filters: { user: ["nobody"] } }));
    expect(a.tiles.find((t) => t.label === "PRs merged")!.value).toBe(1);
    expect(a.tiles.find((t) => t.label === "Code lines added")!.value).toBe(1000);
    expect(a.tiles.find((t) => t.label === "Bugs per 1K code lines")!.value).toBe(1);
    const issues = buildTokenomics(d, live, query({ page: "dv-jira" })).tables.find((t) => t.id === "jira-issues")!;
    expect(issues.rows[0]!.cells.slice(10, 13)).toEqual([1, 2, 0]);
  });
  it("provides all 18 real page projections with valid bounded native contracts", () => {
    expect(Object.keys(TOKENOMICS_PAGES)).toHaveLength(18);
    for (const page of Object.keys(TOKENOMICS_PAGES)) {
      const q = query({ page }); const result = buildTokenomics(d, live, q);
      expect(result.tables.length, page).toBeGreaterThan(0);
      expect(tokenomicsViewSchema.safeParse(tokenomicsView({ data: d, live, revision: "fixture", diagnostics: [], readAt: "2026-11-03T12:00:00Z" }, q)).success, page).toBe(true);
    }
  });
  it("exports all rows beyond display pagination, quotes CSV and neutralizes spreadsheet formulas", () => {
    const result = buildTokenomics(d, live, query({ page: "pivot" })), t = result.tables[0]!;
    t.rows = Array.from({ length: 72 }, (_, i) => ({ key: JSON.stringify([String(i)]), cells: [i === 71 ? '=HYPERLINK("bad")' : `Row ${i}`, i], detail: [] }));
    const csv = tokenomicsCsv(result);
    expect(paginateTokenomics(result, query())[0]!.rows).toHaveLength(50);
    expect(paginateTokenomics(result, query({ offsets: { pivot: 50 } }))[0]!.rows).toHaveLength(22);
    expect(csv).toContain('"Row 70"'); expect(csv).toContain("'=HYPERLINK");
  });
  it("rejects invalid indices, duplicate IDs and arbitrary client paths", () => {
    const data = structuredClone(d); data.facts[0]![0] = 999;
    expect(tokenomicsDataSchema.safeParse(data).success).toBe(false);
    expect(tokenomicsDataSchema.safeParse({ ...d, seats: [d.seats[0], d.seats[0]] }).success).toBe(false);
    expect(readTokenomics.input.safeParse({ path: "/private/secret" }).success).toBe(false);
    expect(tokenomicsQuerySchema.safeParse({ from: "2026-02-30" }).success).toBe(false);
    expect(tokenomicsQuerySchema.safeParse({ period: "m:2026-99" }).success).toBe(false);
  });
  it("retains pivot subtotals/drill-down while exporting leaves and keeps split columns off other pages", () => {
    const q = query({ page: "pivot", groups: ["ws", "model"] });
    const built = buildTokenomics(d, live, q), rows = built.tables[0]!.rows;
    expect(rows.some((r) => r.hasChildren)).toBe(true);
    const closed = paginateTokenomics(built, q)[0]!, expanded = paginateTokenomics(built, { ...q, expanded: rows.filter((r) => r.hasChildren).map((r) => r.key) })[0]!;
    expect(expanded.total).toBeGreaterThan(closed.total);
    expect(tokenomicsCsv(built).split("\r\n")).toHaveLength(rows.filter((r) => !r.hasChildren).length + 1);
    for (const page of ["overview", "roles"]) for (const t of buildTokenomics(d, live, query({ page, split: true })).tables) expect(t.rows.every((r) => r.cells.length === t.columns.length), t.id).toBe(true);
    expect(tokens(usageModel(d, query({ filters: { user: [] } })).total)).toBe(0);
  });
  it("preserves native host navigation and report filters through Back", () => {
    navigate("tokenomics-a", { tab: "Tokenomics" }); writePageView("tokenomics-a", navigate("tokenomics-a", {}), query({ page: "roles", filters: { user: ["Person A"] } }));
    navigate("tokenomics-a", { tab: "Sessions" }); const route = goBack("tokenomics-a");
    expect(route.tab).toBe("Tokenomics"); expect(readPageView("tokenomics-a", route, query()).page).toBe("roles");
    expect(readPageView("tokenomics-b", { ...route }, query()).page).toBe("overview");
  });
});

describe("bounded Tokenomics host source", () => {
  it("registers read-only RPCs, refuses missing live exports and exports beyond display pagination", async () => {
    const p = await folder(); await writeFile(join(p, "data.json"), JSON.stringify(d)); vi.stubEnv("KONTOR_PLUGIN_TOKENOMICS_ROOT", p);
    const handlers = new Map<string, (input: ReturnType<typeof query>) => Promise<unknown>>();
    contributeTokenomics({ handle: (contract: { name: string }, handler: (input: ReturnType<typeof query>) => Promise<unknown>) => { handlers.set(contract.name, handler); } } as unknown as PluginServerContext);
    expect([...handlers.keys()]).toEqual(["tokenomics.read", "tokenomics.export"]);
    await expect(handlers.get("tokenomics.export")!(query({ page: "cr-prs" }))).rejects.toThrow("source is unavailable");
    const view = await handlers.get("tokenomics.read")!(query({ page: "pivot" })); expect(tokenomicsViewSchema.safeParse(view).success).toBe(true);
    const exported = await handlers.get("tokenomics.export")!(query({ page: "pivot", groups: ["model"], offsets: { pivot: 999 } })) as { content: string; revision: string };
    expect(exported.content).toContain("demo-model"); expect(exported.content).toContain("future-model"); expect(exported.revision).toMatch(/^[a-f0-9]{64}$/);
  });
  it.skipIf(!process.env.KONTOR_TOKENOMICS_PARITY_ROOT)("reads the operator-selected existing snapshot without collection [local parity]", async () => {
    const raw = await readFile(join(process.env.KONTOR_TOKENOMICS_PARITY_ROOT!, "_stats/live.js"), "utf8");
    const parsed = tokenomicsLiveSchema.safeParse(JSON.parse(raw.slice(raw.indexOf("=") + 1).trim().replace(/;$/, "")));
    expect(parsed.success, JSON.stringify(parsed.error?.issues.slice(0, 12).map((i) => ({ code: i.code, path: i.path.map((s) => typeof s === "number" ? "[]" : s).slice(0, 4), message: i.message })))).toBe(true);
    const snapshot = await readTokenomicsSnapshot(process.env.KONTOR_TOKENOMICS_PARITY_ROOT!);
    expect(snapshot.diagnostics.filter((s) => /match|unreadable|malformed/.test(s))).toEqual([]);
    expect(snapshot.data).not.toBeNull();
    for (const page of Object.keys(TOKENOMICS_PAGES)) expect(tokenomicsViewSchema.safeParse(tokenomicsView(snapshot, query({ page }))).success, page).toBe(true);
  });
  it("accepts JSON assignments and rejects executable JavaScript and malformed live contracts", () => {
    expect(parseLiveAssignment(`window.ASMA_LIVE = ${JSON.stringify(live)};`).review?.findings).toHaveLength(2);
    for (const value of ['window.ASMA_LIVE = {}; steal();', 'window.ASMA_LIVE = (() => ({}))();', 'window.ASMA_LIVE = {review: null};', 'window.ASMA_DATA = {};']) expect(() => parseLiveAssignment(value)).toThrow();
  });
  it("refuses oversized files, directories and symlinks without exposing their contents", async () => {
    const p = await folder(); await writeFile(join(p, "too-big"), "12345");
    await expect(readReportFile(join(p, "too-big"), 4)).rejects.toThrow(); await expect(readReportFile(p, 100)).rejects.toThrow();
    await symlink(join(p, "too-big"), join(p, "link")); await expect(readReportFile(join(p, "link"), 100)).rejects.toThrow();
  });
  it("shows missing, invalid, stale and partial states and still renders token data without live data", async () => {
    const p = await folder(); const now = new Date("2026-11-07T12:00:00Z");
    expect((await readTokenomicsSnapshot(null, now)).data).toBeNull();
    await writeFile(join(p, "data.json"), JSON.stringify(d));
    const partial = await readTokenomicsSnapshot(p, now), view = tokenomicsView(partial, query());
    expect(view.status).toBe("partial"); expect(view.tiles.find((t) => t.label === "Tokens processed")!.value).toBe(1400);
    expect(view.diagnostics.join(" ")).toContain("stale"); expect(view.diagnostics.join(" ")).toContain("Private live report");
    await writeFile(join(p, "_stats/live.js"), `window.ASMA_LIVE=${JSON.stringify(live)};`);
    const ready = await readTokenomicsSnapshot(p, new Date("2026-11-03T13:00:00Z")); expect(ready.diagnostics).toEqual([]);
    expect(tokenomicsView(ready, query()).status).toBe("ready");
    await writeFile(join(p, "data.json"), '{"schema":"wrong"}'); expect((await readTokenomicsSnapshot(p, now)).data).toBeNull();
  });
  it("keeps unsupported selection and reversed periods explicit", () => {
    const snapshot = { data: d, live: tokenomicsLiveSchema.parse({}), revision: "fixture", diagnostics: [], readAt: "now" };
    expect(tokenomicsView(snapshot, query({ rev: "removed-check" })).status).toBe("unavailable");
    expect(tokenomicsView(snapshot, query({ from: "2026-11-03", to: "2026-11-01", period: "custom" })).status).toBe("unavailable");
  });
});
