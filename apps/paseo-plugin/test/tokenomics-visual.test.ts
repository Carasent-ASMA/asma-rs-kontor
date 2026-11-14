import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/ui", () => ({ Icon: "Icon" }));
import { findSvgIconProblem } from "@getpaseo/plugin";
import { activityChartSummary, activityEveryoneKeys, chartSvgLayers, tokenomicsChartScene, type ChartScene } from "../client/tokenomics-charts";
import { reportMoney, reportNumber, reportValue } from "../client/tokenomics-format";
import type { Colors } from "../client/page-controls";
import { buildTokenomics, paginateTokenomics } from "../shared/tokenomics-model";
import { tokenomicsQuerySchema } from "../shared/tokenomics";
import { tokenomicsFixture, tokenomicsLiveFixture } from "./fixtures/tokenomics";
const colors = { surface0: "#f4f3f0", surface1: "#fcfcfb", surface2: "#f0efec", border: "#e2e1dc", foreground: "#0b0b0b", foregroundMuted: "#52514e", accent: "#2a78d6" } as Colors;
const query = (input: object = {}) => tokenomicsQuerySchema.parse(input);
describe("native Tokenomics visual contracts", () => {
  it("matches web formatting without changing accounting values", () => {
    expect(reportNumber(65432109876)).toBe("65.43B"); expect(reportMoney(12345.67)).toBe("$12,346"); expect(reportMoney(6)).toBe("$6.00");
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
    const q = query({ page: "roles" }), t = paginateTokenomics(buildTokenomics(tokenomicsFixture(), tokenomicsLiveFixture(), q), q).find((t) => t.id === "share")!;
    t.plot!.rows[0]![0] = '<script> & "role"'; expect(chartSvgLayers(tokenomicsChartScene(t, 348, colors, q)).every((svg) => findSvgIconProblem(svg) === null)).toBe(true);
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
