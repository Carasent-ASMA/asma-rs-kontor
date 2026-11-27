import { describe, expect, it } from "vitest";
import { simulatedTextLayout } from "../src/host/react-native";
import { tokenomicsRpc } from "../src/fixtures/tokenomics";
import { tokenomicsViewSchema } from "../../paseo-plugin/shared/tokenomics";

describe("explicit harness text-scale diagnostics", () => {
  it("scales native text props once and respects inheritance, opt-out and multiplier caps", () => {
    const parent = { size: 14, allow: true }, style = { fontSize: 16, lineHeight: 20 };
    const enlarged = simulatedTextLayout(style, undefined, undefined, parent, 2);
    expect(enlarged.style).toEqual({ fontSize: 32, lineHeight: 40 }); expect(enlarged.next.size).toBe(16);
    expect(simulatedTextLayout({}, undefined, undefined, enlarged.next, 2).style).toEqual({ fontSize: 32, lineHeight: 40 });
    expect(simulatedTextLayout(style, false, undefined, parent, 2).style.fontSize).toBe(16);
    expect(simulatedTextLayout({}, undefined, undefined, { ...parent, allow: false }, 2).style.fontSize).toBe(14);
    expect(simulatedTextLayout(style, true, 1.5, parent, 2).style.fontSize).toBe(24);
    expect(simulatedTextLayout(style, true, 0, { ...parent, max: 1.5 }, 2).style.fontSize).toBe(32);
    expect(simulatedTextLayout(style, true, undefined, parent, 1).style).toEqual(style); expect(style).toEqual({ fontSize: 16, lineHeight: 20 });
  });
  it("keeps the harness projection and full export on one synthetic revision while scrolling", () => {
    const first = tokenomicsViewSchema.parse(tokenomicsRpc("tokenomics.read", { page: "dv-prs" }, true));
    const next = tokenomicsViewSchema.parse(tokenomicsRpc("tokenomics.read", { page: "dv-prs", offsets: { "delivery-prs": 50 } }, true));
    const a = first.tables.find((t) => t.id === "delivery-prs")!, b = next.tables.find((t) => t.id === "delivery-prs")!;
    expect(a.rows).toHaveLength(50); expect(b.rows).toHaveLength(23); expect(a.total).toBe(73); expect(first.revision).toBe(next.revision);
    const csv = tokenomicsRpc("tokenomics.export", { page: "dv-prs", offsets: { "delivery-prs": 50 } }, true) as { content: string };
    expect(csv.content.split("\n").filter((s) => s.includes("demo-repo#"))).toHaveLength(73);
  });
});
