import { describe, expect, it, vi } from "vitest";
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: vi.fn() }));
import { tokenomicsRowChunk } from "../client/tokenomics-rows";
import { tokenomicsCsvExport, tokenomicsView } from "../server/tokenomics";
import { tokenomicsQuerySchema, tokenomicsViewSchema } from "../shared/tokenomics";
import { tokenomicsFixture, tokenomicsLiveFixture } from "./fixtures/tokenomics";

describe("continuous Tokenomics row projections", () => {
  it("reads every row in bounded chunks while totals and export scope remain unchanged", () => {
    const live = tokenomicsLiveFixture(), first = live.delivery!.prs[0]!;
    live.delivery!.prs = Array.from({ length: 73 }, (_, i) => ({ ...first, ref: `demo-repo#${i + 1}` }));
    const snapshot = { data: tokenomicsFixture(), live, revision: "fixed-snapshot", readAt: "2026-11-03T13:00:00Z", diagnostics: [] };
    const query = tokenomicsQuerySchema.parse({ page: "dv-prs" }), firstView = tokenomicsView(snapshot, query);
    const a = tokenomicsRowChunk(firstView, "delivery-prs", snapshot.revision, 0);
    const b = tokenomicsRowChunk(tokenomicsView(snapshot, { ...query, offsets: { "delivery-prs": 50 } }), "delivery-prs", snapshot.revision, 50);
    expect(a.rows).toHaveLength(50); expect(b.rows).toHaveLength(23); expect(a.total).toBe(73); expect(b.total).toBe(73);
    expect(new Set([...a.rows, ...b.rows].map((r) => r.key)).size).toBe(73);
    expect(tokenomicsViewSchema.safeParse(firstView).success).toBe(true);
    const csv = tokenomicsCsvExport(snapshot, { ...query, offsets: { "delivery-prs": 50 } }).content;
    expect(csv).toContain("demo-repo#1"); expect(csv).toContain("demo-repo#73");
    expect(csv.split("\n").filter((line) => line.includes("demo-repo#"))).toHaveLength(73);
    expect(() => tokenomicsRowChunk({ ...firstView, revision: "changed" }, "delivery-prs", snapshot.revision, 50)).toThrow("Report changed");
    expect(() => tokenomicsRowChunk({ ...firstView, tables: [{ ...a, rows: [] }] }, "delivery-prs", snapshot.revision, 50)).toThrow("no rows were skipped");
  });
});
