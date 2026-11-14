import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/ui", () => ({ Icon: "Icon" }));
import { findSvgIconProblem } from "@getpaseo/plugin";
import { activityChartSummary, activityEveryoneKeys, chartSvgLayers, tokenomicsChartScene, type ChartScene } from "../client/tokenomics-charts";
import { reportComparedChange, reportLinePair, reportLivePercent, reportMoney, reportNumber, reportTableValue, reportValue } from "../client/tokenomics-format";
import type { Colors } from "../client/page-controls";
import { buildTokenomics, paginateTokenomics, tokenomicsSummary, usageModel } from "../shared/tokenomics-model";
import { tokenomicsQuerySchema } from "../shared/tokenomics";
import { tokenomicsFixture, tokenomicsLiveFixture } from "./fixtures/tokenomics";
const colors = { surface0: "#f4f3f0", surface1: "#fcfcfb", surface2: "#f0efec", border: "#e2e1dc", foreground: "#0b0b0b", foregroundMuted: "#52514e", accent: "#2a78d6" } as Colors;
const query = (input: object = {}) => tokenomicsQuerySchema.parse(input);
describe("native Tokenomics visual contracts", () => {
  it("matches web formatting without changing accounting values", () => {
    expect(reportNumber(65432109876)).toBe("65.43B"); expect(reportMoney(12345.67)).toBe("$12,346"); expect(reportMoney(6)).toBe("$6.00"); expect(reportTableValue(0.25, "Cache read", "prices")).toBe("0.25"); expect(reportLivePercent("100.0%")).toBe("100%"); expect(reportLivePercent(13.123)).toBe("13%"); expect(reportLivePercent(5.9)).toBe("5.9%");
    expect(reportTableValue("2026-11-03T12:34:00Z", "Merged", "delivery-prs")).toBe("2026-11-03"); expect(reportComparedChange(0.001, "API cost ($)")).toBe("±0"); expect(reportComparedChange(-0.002, "Cache hit (pp)")).toBe("±0"); expect(reportComparedChange(1.234, "Active seat-hours")).toBe("+1.2 h"); expect(reportLinePair(0, 0)).toBe("—"); expect(reportLinePair(null, null)).toBe("—"); expect(reportLinePair(1234, 100)).toBe("+1.2K / −100"); expect(reportTableValue(1608, "With token data", "coverage")).toBe("1608"); expect(reportTableValue("A, B, C, D, E, F, G", "Bugs", "bug-people")).toBe("A, B, C, D, E, F +1");
    expect(reportValue(1234567890, "Codex Auto Review")).toBe("1.23B"); expect(reportValue(13.123456789, "Churn (%)")).toBe("13.1%"); expect(reportValue(1.2345, "Bugs per 1K code lines")).toBe("1.23");
    const d = tokenomicsFixture(), before = JSON.stringify(d); buildTokenomics(d, tokenomicsLiveFixture(), query()); expect(JSON.stringify(d)).toBe(before);
  });
  it("keeps complete chart data independent of pagination, search and per-table header sorts", () => {
    const result = buildTokenomics(tokenomicsFixture(), tokenomicsLiveFixture(), query({ page: "activity" })), t = result.tables.find((t) => t.id === "trend")!;
    t.rows = Array.from({ length: 72 }, (_, i) => ({ key: String(i), cells: [`2026-10-${String(i % 28 + 1).padStart(2, "0")}`, "Demo", i], detail: [] }));
    const a = paginateTokenomics(result, query({ page: "activity" })).find((t) => t.id === "trend")!, b = paginateTokenomics(result, query({ page: "activity", search: "Demo", offsets: { trend: 50 }, sorts: { trend: { column: t.columns[2]!, descending: true } } })).find((t) => t.id === "trend")!;
    expect(a.rows).toHaveLength(50); expect(b.rows).toHaveLength(22); expect(a.plot).toEqual(b.plot); expect(a.plot!.rows).toHaveLength(72); expect(b.rows[0]!.cells[2]).toBe(21);
  });
  it("preserves rectangular geometry, axes and hit bounds with host-valid SVGs", () => {
    for (const page of ["activity", "roles", "usage", "cr-overview", "cr-fix", "dv-overview", "dv-bugs"] as const) {
      const q = query({ page }), result = buildTokenomics(tokenomicsFixture(), tokenomicsLiveFixture(), q);
      for (const t of paginateTokenomics(result, q).filter((t) => t.chart)) for (const width of [348, 1138]) {
        const scene = tokenomicsChartScene(t, width, colors, q), layers = chartSvgLayers(scene);
        for (const svg of layers) { expect(findSvgIconProblem(svg)).toBeNull(); expect(new TextEncoder().encode(svg).length).toBeLessThanOrEqual(65536); expect(svg).toContain(`viewBox="0 0 ${width} ${scene.height}"`); }
        for (const h of scene.hits) { expect(h.x).toBeGreaterThanOrEqual(0); expect(h.y).toBeGreaterThanOrEqual(0); expect(h.x+h.width).toBeLessThanOrEqual(width+0.01); expect(h.y+h.height).toBeLessThanOrEqual(scene.height+0.01); }
      }
    }
  });
  it("partitions an over-limit chart losslessly on the shared coordinate system", () => {
    const parts = Array.from({ length: 2200 }, (_, i) => `<rect data-index="${i}" x="${i % 100}" y="0" width="1" height="1" fill="#000"/>`), scene: ChartScene = { width: 1138, height: 340, parts, hits: [], legend: [] }, layers = chartSvgLayers(scene);
    expect(layers.length).toBeGreaterThan(1); for (const part of parts) expect(layers.filter((svg) => svg.includes(part))).toHaveLength(1); for (const svg of layers) expect(findSvgIconProblem(svg)).toBeNull();
    expect(() => chartSvgLayers({ ...scene, parts: [" ".repeat(65536)] })).toThrow("One chart path exceeds");
    expect(() => chartSvgLayers({ ...scene, parts: Array(2048).fill(" ".repeat(16370)) })).toThrow("Chart composition exceeds");
    expect(() => chartSvgLayers({ ...scene, parts: Array(2100).fill('<rect x="0" y="0" width="1" height="1"/>' + " ".repeat(16000)) })).toThrow("Chart composition exceeds");
    const q = query({ page: "activity" }), t = paginateTokenomics(buildTokenomics(tokenomicsFixture(), tokenomicsLiveFixture(), q), q).find((t) => t.id === "timeline")!;
    for (const r of t.plot!.rows) if (r[0] === "Person") r[1] = '<script>&"';
    const escaped = chartSvgLayers(tokenomicsChartScene(t, 1138, colors, q)).join(""); expect(escaped).toContain("&lt;script&gt;&amp;&quot;"); expect(escaped).not.toContain("<script>");

  });
  it("shows team rollups, signed deltas and typed flag episodes; delivery-only users stay quiet in consumption views", () => {
    const d = tokenomicsFixture(), live = tokenomicsLiveFixture();
    live.dev!.rows.push(["Delivery-only person", "2026-11-03", "EXAMPLE-101", 1, 1, 20, 0]);
    const q = query({ page: "activity" }), tables = paginateTokenomics(buildTokenomics(d, live, q), q), t = tables.find((t) => t.id === "timeline")!;
    expect(t.plot!.rows.filter((r) => r[0] === "Person" && r[1] === "Delivery-only person").every((r) => r[5] === null)).toBe(true);
    expect(activityEveryoneKeys(t)).not.toContain("tl:p:Delivery-only person");
    const row = (day: string, hours: number) => ["Team total", "All users", "", "", day, hours, hours, 20, 100, 1, 0, 0, 0, 20, 15, 1];
    const flags = [["Spike", "Person A", "", "2026-11-01", "2026-11-01", "cost", 200, 50, 100], ["Spike", "Person A", "", "2026-11-03", "2026-11-03", "cost", 220, 50, 100], ["Spike", "Person A", "", "2026-11-02", "2026-11-02", "act", 10, 2, 8], ["Efficiency drop", "Person A", "", "2026-11-02", "2026-11-02", "hit", 70, 90, 80]];
    const rows = [row("2026-10-27", 5), row("2026-11-03", 10)];
    const summary = activityChartSummary(rows, "act", "2026-11-03", flags, "2026-10-01");
    expect(summary).toMatchObject({ current: 10, prior: 5, delta: "+100%", flags: [{ type: "Spike", count: 2 }, { type: "Efficiency drop", count: 1 }] });
    t.plot!.rows = [...rows, ...rows.map((r) => ["Person", "Person A", ...r.slice(2)])]; t.plot!.end = "2026-11-03"; t.plot!.flags = flags; t.plot!.events = [["EXAMPLE-101", "2026-11-03", "EXAMPLE-102"], ["EXAMPLE-101", "2026-11-03", "EXAMPLE-103"]];
    const svg = chartSvgLayers(tokenomicsChartScene(t, 1138, colors, q)).join("");
    expect(svg).toContain("+100%"); expect(svg).toContain("▲2"); expect(svg).toContain("◆1"); expect(svg).toContain(">✓</text>"); expect(svg).not.toContain(">✓2</text>");
  });
  it("projects complete category/month/coverage data while keeping raw CSV and totals independent of display state", () => {
    const d = tokenomicsFixture(), live = tokenomicsLiveFixture(), q = query({ page: "usage" }), built = buildTokenomics(d, live, q), tables = paginateTokenomics(built, q);
    const cats = tables.find((t) => t.id === "all-categories")!, total = cats.rows.find((r) => r.key === "__total__")!;
    expect(total.cells[cats.columns.indexOf("Tokens")]).toBe(700); expect(total.cells[cats.columns.indexOf("API-equivalent cost ($)")]).toBe(4);
    expect(cats.rows.some((r) => r.key.startsWith("sub-"))).toBe(false);
    const opened = paginateTokenomics(built, query({ page: "usage", expanded: ["category-0"] })).find((t) => t.id === "all-categories")!;
    expect(opened.rows.find((r) => r.key === "sub-0-0")!.cells[cats.columns.indexOf("Tokens")]).toBe(350);
    const month = tables.find((t) => t.id === "all-months")!; expect(month.columns).toHaveLength(4); expect(month.rows[0]!.cells).toEqual(["2026-10", "350 · $2", "—", "350 · $2"]);
    const raw = built.tables.find((t) => t.id === "all-months")!; expect(raw.columns).toContain("Category"); expect(raw.rows[0]!.cells[1]).toBe("Registered workstreams");
    const data = paginateTokenomics(buildTokenomics(d, live, query({ page: "data" })), query({ page: "data" })); expect(data.map((t) => t.id)).toEqual(["coverage", "prices"]); expect(data[0]!.rows.some((r) => r.span === 4 && r.cells[4] === "transcript-missing")).toBe(true);
  });
  it("preserves epic titles and source links in the Jira delivery projection", () => {
    const d = tokenomicsFixture(), live = tokenomicsLiveFixture(), q = query({ page: "dv-jira" });
    const epic = d.epics[0]!; for (const issue of Object.values(live.delivery!.issues)) issue.e = epic.key;
    const t = buildTokenomics(d, live, q).tables.find((t) => t.id === "jira-epics")!, r = t.rows.find((r) => r.cells[0] === epic.key)!;
    expect(r.cells[t.columns.indexOf("Summary")]).toBe(epic.summary); expect(r.url).toContain(epic.key);
  });
  it("keeps pivot children beside their parents when sorting and reports full-selection footer totals", () => {
    const d = tokenomicsFixture(), live = tokenomicsLiveFixture(), q = query({ page: "pivot", groups: ["ws", "model"], expanded: [JSON.stringify(["Demo build"])], sorts: { pivot: { column: "API-equivalent cost ($)", descending: true } } });
    const built = buildTokenomics(d, live, q), t = paginateTokenomics(built, q)[0]!;
    expect(t.rows[0]!.cells[0]).toBe("Demo build"); expect(t.rows[1]!.depth).toBe(1); expect(t.footer![0]!.cells[t.columns.indexOf("Tokens")]).toBe(1400);
    const selected = query({ page: "overview", openWorkstreams: ["build"] }), overview = paginateTokenomics(buildTokenomics(d, live, selected), selected);
    expect(overview.find((t) => t.id === "epics:build")!.rows[0]!.cells).toContain("EXAMPLE-101 · Synthetic epic");
    expect(overview.find((t) => t.id === "workstream-totals")!.rows.find((r) => r.key === "build")!.cells).toContain(1);
  });
  it("distinguishes newly completed issues from unchanged net totals and suppresses same-revision comparisons", () => {
    const d = tokenomicsFixture(), live = tokenomicsLiveFixture();
    d.epics[0]!.children.push({ ...d.epics[0]!.children[0]!, key: "EXAMPLE-104", status: "To do", cat: "new", resolved: null });
    d.revs[0]!.tasks["EXAMPLE-104"] = ["done", "Done"]; d.revs[1]!.tasks["EXAMPLE-104"] = ["new", "To do"];
    d.revs[0]!.epics["EXAMPLE-101"]!.done = 1; d.revs[1]!.epics["EXAMPLE-101"]!.todo = 1;
    const q = query({ page: "overview", compare: "check-a", rev: "check-b" }), wt = buildTokenomics(d, live, q).tables.find((t) => t.id === "workstream-totals")!, r = wt.rows.find((r) => r.key === "build")!;
    expect(r.cells[wt.columns.indexOf("Δ done")]).toBe(0); expect(r.cells[wt.columns.indexOf("New done")]).toBe(1);
    const same = query({ compare: "check-b", rev: "check-b" }); expect(tokenomicsSummary(usageModel(d, same)).compare).toBe("none"); expect(buildTokenomics(d, live, same).tiles.every((t) => !t.delta)).toBe(true);
  });
  it("keeps registered overview epics before activity-ranked desk rows without changing raw export rows", () => {
    const d = tokenomicsFixture(), base = d.epics[0]!, zero = { ...base, key: "EXAMPLE-099", summary: "Registered zero-usage epic", children: [] };
    d.epics.unshift(zero); d.ws.build!.epics.unshift(zero.key);
    const q = query({ page: "overview", openWorkstreams: ["build"] }), built = buildTokenomics(d, tokenomicsLiveFixture(), q), projected = paginateTokenomics(built, q).find((t) => t.id === "epics:build")!;
    expect(projected.rows[0]!.cells[1]).toBe("EXAMPLE-099 · Registered zero-usage epic");
    const wt = built.tables.find((t) => t.id === "workstream-totals")!, build = wt.rows.find((r) => r.key === "build")!; expect(build.cells[wt.columns.indexOf("Before Jira captured")]).toBe(1);
    d.ws.build!.epics = []; const missing = buildTokenomics(d, tokenomicsLiveFixture(), q).tables.find((t) => t.id === "workstream-totals")!; expect(missing.rows.find((r) => r.key === "build")!.cells[missing.columns.indexOf("Before Jira captured")]).toBe(0);
    expect(built.tables.find((t) => t.id === "epics")!.rows).toHaveLength(3);
  });
  it("computes workstream cache-hit deltas from full values before display rounding", () => {
    const d = tokenomicsFixture(); d.facts = d.facts.slice(0, 2);
    const before = d.facts[0]!, after = d.facts[1]!;
    for (const f of d.facts) { f[3] = 49851; f[4] = 50149; f[5] = 0; f[6] = 0; }
    after[3] = 49847; after[4] = 50153;
    const q = query({ compare: "check-a", rev: "check-b" }), t = buildTokenomics(d, tokenomicsLiveFixture(), q).tables.find((t) => t.id === "workstream-totals")!, r = t.rows.find((r) => r.key === "build")!;
    expect(r.cells[t.columns.indexOf("Δ Cache hit")]).toBeCloseTo(0.002, 8);
    expect(before[4]).toBe(50149);
  });
  it("shows the original empty activity state instead of an empty chart frame", () => {
    const q = query({ page: "activity", filters: { user: ["Nobody selected"] } }), t = paginateTokenomics(buildTokenomics(tokenomicsFixture(), tokenomicsLiveFixture(), q), q).find((t) => t.id === "timeline")!, scene = tokenomicsChartScene(t, 1138, colors, q);
    expect(scene.height).toBe(40); expect(scene.parts.join("")).toContain("No recorded activity for the current filters.");
  });
  it("defaults timeline range to chart width while retaining an explicit full-lifetime selection", () => {
    const q = query({ page: "activity" }), t = paginateTokenomics(buildTokenomics(tokenomicsFixture(), tokenomicsLiveFixture(), q), q).find((t) => t.id === "timeline")!;
    expect(q.range).toBe("auto");
    t.plot!.end = "2026-12-31";
    const auto = tokenomicsChartScene(t, 348, colors, q), full = tokenomicsChartScene(t, 348, colors, { ...q, range: "all" });
    expect(auto.hits.some((h) => h.label.startsWith("2026-10-31"))).toBe(false);
    expect(full.hits.some((h) => h.label.startsWith("2026-10-31"))).toBe(true);
  });
  it.skipIf(!process.env.KONTOR_TOKENOMICS_VISUAL_ROOT)("validates frozen wide/timeline projections [local visual proof]", async () => {
    const { readTokenomicsSnapshot, tokenomicsView } = await import("../server/tokenomics"), snapshot = await readTokenomicsSnapshot(process.env.KONTOR_TOKENOMICS_VISUAL_ROOT!);
    expect(snapshot.data).not.toBeNull(); const q = query({ page: "activity" }), view = tokenomicsView(snapshot, q), timeline = view.tables.find((t) => t.id === "timeline")!, parallel = view.tables.find((t) => t.id === "parallel")!;
    const opened = query({ page: "activity", expanded: activityEveryoneKeys(timeline) }), records = [];
    for (const [label, t, selection] of [["wide", parallel, q], ["timeline-default", timeline, q], ["timeline-expanded", timeline, opened]] as const) for (const width of [348, 1138]) {
      const scene = tokenomicsChartScene(t, width, colors, selection), layers = chartSvgLayers(scene);
      records.push({ label, width, height: scene.height, parts: scene.parts.length, hits: scene.hits.length, bytes: layers.map((svg) => new TextEncoder().encode(svg).length), hostValid: layers.every((svg) => findSvgIconProblem(svg) === null) }); expect(records.at(-1)!.hostValid).toBe(true);
      if (process.env.KONTOR_TOKENOMICS_VISUAL_EVIDENCE) { const { writeFile } = await import("node:fs/promises"), { join } = await import("node:path"); for (const [i, svg] of layers.entries()) await writeFile(join(process.env.KONTOR_TOKENOMICS_VISUAL_EVIDENCE, `${label}-${width}-${i}.svg`), svg); if (label === "timeline-expanded" && width === 1138) await writeFile(join(process.env.KONTOR_TOKENOMICS_VISUAL_EVIDENCE, "expanded-hierarchy.json"), JSON.stringify(scene.hits.filter((h) => h.toggle).map(({ label, y, height, open, toggle }) => ({ label, y, height, open, toggle })), null, 2)); }
    }
    if (process.env.KONTOR_TOKENOMICS_VISUAL_EVIDENCE) { const { writeFile } = await import("node:fs/promises"), { join } = await import("node:path"); await writeFile(join(process.env.KONTOR_TOKENOMICS_VISUAL_EVIDENCE, "svg-proof.json"), JSON.stringify({ revision: snapshot.revision, records }, null, 2)); }
  });
});
