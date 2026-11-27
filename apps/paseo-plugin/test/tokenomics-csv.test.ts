import { describe, expect, it } from "vitest";
import { tokenomicsCsvExport } from "../server/tokenomics";
import { buildTokenomics, tokenomicsCsv } from "../shared/tokenomics-model";
import { tokenomicsQuerySchema } from "../shared/tokenomics";
import { tokenomicsFixture, tokenomicsLiveFixture } from "./fixtures/tokenomics";

function rows(csv: string): string[][] {
  const result: string[][] = []; let row: string[] = [], cell = "", quoted = false;
  for (let i = 0; i < csv.length; i++) {
    const c = csv[i]!;
    if (c === '"') { if (quoted && csv[i + 1] === '"') { cell += '"'; i++; } else quoted = !quoted; }
    else if (!quoted && (c === "," || c === "\r" || c === "\n")) {
      row.push(cell); cell = "";
      if (c !== ",") { result.push(row); row = []; if (c === "\r" && csv[i + 1] === "\n") i++; }
    } else cell += c;
  }
  row.push(cell); result.push(row); return result;
}
const snapshot = () => ({ data: tokenomicsFixture(), live: tokenomicsLiveFixture(), revision: "synthetic-source", diagnostics: [], readAt: "2026-11-03T13:00:00Z" });

describe("complete CSV scope and cost qualifiers", () => {
  it("carries normalized revisions, generation, selection and singular lower-bound notices without changing numeric rows", () => {
    const source = snapshot(), q = tokenomicsQuerySchema.parse({ page: "roles", show: "all", rev: "check-b" });
    const result = tokenomicsCsvExport(source, q), parsed = rows(result.content);
    const meta = new Map(parsed.filter((row) => row.length === 2).map(([key, value]) => [key!, value!]));
    expect(meta.get("Generated")).toBe(source.data.generated);
    expect(meta.get("Source revision")).toBe("synthetic-source");
    expect(meta.get("Revision")).toBe("check-b"); expect(meta.get("Comparison")).toBe("check-a");
    expect(JSON.parse(meta.get("Selection")!).filters).toEqual({});
    const warnings = parsed.filter(([key]) => key === "Warning").map((row) => row[1]);
    expect(warnings).toContain("1 session lacks token data; usage and cost are lower bounds.");
    expect(warnings).toContain("1 session is unpriced; cost excludes their recorded tokens.");
    expect(meta.get("Cost interpretation")).toContain("not a known zero cost");
    expect(parsed.some((row) => row[0] === "future-model" && row[1] === "0" && row[2] === "350")).toBe(true);
    expect(result.content.endsWith(tokenomicsCsv(buildTokenomics(source.data, source.live, q)))).toBe(true);
  });

  it("records filtered/custom scope and ordinary priced values, without carrying unrelated warning counts", () => {
    const source = snapshot(), q = tokenomicsQuerySchema.parse({ page: "roles", show: "all", period: "custom", from: "2026-11-02", to: "2026-11-02", filters: { model: ["demo-model"] } });
    const parsed = rows(tokenomicsCsvExport(source, q).content), selection = JSON.parse(parsed.find(([key]) => key === "Selection")![1]!);
    expect(selection).toMatchObject({ period: "custom", from: "2026-11-02", to: "2026-11-02", filters: { model: ["demo-model"] }, rev: "check-b", compare: "check-a" });
    expect(parsed.filter(([key]) => key === "Warning")).toEqual([]);
    expect(parsed.some((row) => row[0] === "demo-model" && row[1] === "4" && row[2] === "700")).toBe(true);
    expect(parsed.some((row) => row[0] === "future-model")).toBe(false);
  });

  it("preserves source diagnostics, ignored-filter scope, UTF-8, quoted newlines and formula escaping", () => {
    const source = snapshot(), delivery = source.live.delivery!; delivery.people["person-a"] = 'Ångström, "Team"\nBuilder';
    delivery.prs[0]!.t = '=HYPERLINK("synthetic")';
    delivery.jira = false;
    const parsed = rows(tokenomicsCsvExport(source, tokenomicsQuerySchema.parse({ page: "dv-prs", filters: { user: ["=nobody"] } })).content);
    expect(parsed.some(([key, text]) => key === "Warning" && text?.includes("incomplete"))).toBe(true);
    expect(parsed.some(([key, text]) => key === "Note" && text?.includes("other filters do not apply"))).toBe(true);
    expect(parsed.flat()).toContain('Ångström, "Team"\nBuilder');
    expect(parsed.flat()).toContain("'=HYPERLINK(\"synthetic\")");
  });

  it("keeps the complete-export byte bound after metadata and CSV escaping", () => {
    const source = snapshot(), delivery = source.live.delivery!, base = delivery.prs[0]!;
    delivery.prs = Array.from({ length: 2100 }, (_, i) => ({ ...base, ref: `demo-repo#${i + 1}`, t: "x".repeat(8192) }));
    expect(() => tokenomicsCsvExport(source, tokenomicsQuerySchema.parse({ page: "dv-prs" }))).toThrow("bounded response limit");
  });
});
