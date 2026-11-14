import { describe, expect, it, vi } from "vitest";
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/ui", () => ({ Icon: "Icon", ScrollView: "ScrollView", TextInput: "TextInput", Modal: "Modal" }));
import { choiceOptions, choiceSearchable } from "../client/tokenomics-choice";
import { mobileFilterPatch, resetTokenomicsFilters, tokenomicsFilterCount } from "../client/tokenomics-mobile";
import { flagHighlightFrames } from "../client/tokenomics-charts";
import { tokenomicsView } from "../server/tokenomics";
import { tokenomicsQuerySchema, tokenomicsViewSchema } from "../shared/tokenomics";
import { tokenomicsFixture, tokenomicsLiveFixture } from "./fixtures/tokenomics";

describe("Tokenomics adaptive controls", () => {
  it("finds every long-list value without option pagination and omits search for short lists", () => {
    const options = Array.from({ length: 6000 }, (_, i) => ({ value: String(i), label: `Seat ${i}` })), all = options.map((o) => o.value);
    expect(choiceSearchable(3)).toBe(false); expect(choiceSearchable(15)).toBe(false); expect(choiceSearchable(16)).toBe(true);
    expect(choiceOptions(options, all, "").rows).toHaveLength(100);
    expect(choiceOptions(options, ["5999"], "").rows[0]!.value).toBe("5999");
    expect(choiceOptions(options, all, "Seat 5999").rows.map((r) => r.value)).toEqual(["5999"]);
    expect(choiceOptions(options, [], "not present")).toEqual({ rows: [], matches: 0 });
  });
  it("resets only filters/scope and applies staged fields without changing page controls", () => {
    const q = tokenomicsQuerySchema.parse({ page: "pivot", period: "7", rev: "now", compare: "before", groups: ["model"], search: "keep", filters: { user: ["Person A"], model: [] }, show: "all", workstreams: [] });
    expect(tokenomicsFilterCount(q, { workstreams: [{ value: "demo", label: "Demo" }] })).toBe(4);
    const reset = resetTokenomicsFilters(q); expect(reset.filters).toEqual({}); expect(reset.workstreams).toBeNull(); expect(reset.show).toBe("usage");
    expect(reset.period).toBe("7"); expect(reset.rev).toBe("now"); expect(reset.compare).toBe("before"); expect(reset.groups).toEqual(["model"]); expect(reset.search).toBe("keep");
    expect(mobileFilterPatch(reset)).not.toHaveProperty("page"); expect(mobileFilterPatch(reset)).not.toHaveProperty("groups"); expect(q.filters.user).toEqual(["Person A"]);
  });
  it("projects explicit source-state consequences without changing accounting totals", () => {
    const data = tokenomicsFixture(), live = tokenomicsLiveFixture(), snap = { data, live, revision: "fixed", readAt: "2026-11-03T13:00:00Z", diagnostics: [] };
    const a = tokenomicsView(snap, tokenomicsQuerySchema.parse({}));
    expect(a.notices.some((n) => n.includes("lower bounds"))).toBe(true); expect(tokenomicsViewSchema.safeParse(a).success).toBe(true);
    const noReview = tokenomicsView({ ...snap, live: { ...live, review: null } }, tokenomicsQuerySchema.parse({ page: "cr-overview" }));
    expect(noReview.notices).toContain("Review data is unavailable; findings and costs are unknown."); expect(noReview.tiles).toEqual([]);
    const b = tokenomicsView({ ...snap, readAt: "2026-11-20T00:00:00Z" }, tokenomicsQuerySchema.parse({}));
    expect(b.notices.some((n) => n.includes("48 hours"))).toBe(true); expect(b.summary).toEqual(a.summary);
  });
  it("matches the original signal band over the whole timeline without crossing plot bounds", () => {
    const hit = (day: string, who = "", x = 10) => ({ x, y: who ? 100 : 24, width: 8, height: 24, label: `${day}${who ? ` · ${who}` : ""}`, values: [] });
    const scene = { width: 100, height: 160, parts: [], legend: [], hits: [hit("2026-11-01"), hit("2026-11-02", "", 18), hit("2026-11-02", "Person A", 18), hit("2026-11-02", "Person B", 18), hit("2026-11-03", "Person A", 26)] };
    expect(flagHighlightFrames(scene, { from: "2026-11-01", to: "2026-11-02", person: "Person A", epic: "", kind: "Spike" })).toEqual([{ x: 10, y: 18, width: 16, height: 138 }]);
    expect(flagHighlightFrames(scene, { from: "2026-12-01", to: "2026-12-02", person: "Person A", epic: "", kind: "Overdue" })).toEqual([]);
  });
});
