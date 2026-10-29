import { describe, expect, it } from "vitest";
import { byLastActivity, compactAge, lastActivity, rowOrigin, recordedChildUsageCompact, tokenTotals, totalsDetail } from "../client/session-row-format";
import { effortShort, modelAlias, modelVendor, routeLabel, rowModel } from "../client/session-usage";

const NOW = Date.parse("2026-10-25T22:00:00Z");
const ago = (ms: number) => NOW - ms;
const MIN = 60_000, HOUR = 60 * MIN, DAY = 24 * HOUR;

describe("compact age", () => {
  it.each([
    [30_000, "now"], [5 * MIN, "5m"], [59 * MIN, "59m"], [3 * HOUR, "3h"], [2 * DAY, "2d"], [6 * DAY, "6d"],
    [7 * DAY, "1w"], [3 * 7 * DAY, "3w"], [45 * DAY, "1mo"], [200 * DAY, "6mo"], [400 * DAY, "1y"], [3 * 365 * DAY, "3y"],
  ])("%i ms ago reads %s", (ms, label) => {
    expect(compactAge(ago(ms), NOW)).toBe(label);
  });

  it("gives no age for an unreadable time or one ahead of this device's clock", () => {
    expect(compactAge(null, NOW)).toBeNull();
    expect(compactAge(NOW + 5 * MIN, NOW)).toBeNull();
  });
});

describe("last activity", () => {
  const entry = { activeTurnStartedAt: null, lastUserMessageAt: null } as const;

  it("never reads metadata updatedAt", () => {
    expect(lastActivity({ ...entry, updatedAt: "2026-10-25T21:59:00Z" } as never)).toBeNull();
  });

  it("takes the newer of a running turn's start and the last prompt", () => {
    expect(lastActivity({ ...entry, lastUserMessageAt: "2026-10-25T20:00:00Z" })).toEqual({ at: Date.parse("2026-10-25T20:00:00Z"), source: "prompt" });
    expect(lastActivity({ ...entry, activeTurnStartedAt: "2026-10-25T21:30:00Z", lastUserMessageAt: "2026-10-25T21:29:00Z" })?.source).toBe("turn started");
  });

  it("never reads an attention time: Mark as unread raises it and it survives later turns", () => {
    expect(lastActivity({ ...entry, attentionReason: "finished", attentionTimestamp: "2026-10-25T21:00:00Z" } as never)).toBeNull();
  });

  it("keeps a session marked unread or renamed where its real activity puts it", () => {
    const quiet = { id: "marked-unread", ...entry, lastUserMessageAt: "2026-10-20T00:00:00Z", requiresAttention: true, attentionReason: "finished", attentionTimestamp: "2026-10-25T21:59:00Z", updatedAt: "2026-10-25T21:59:00Z" };
    const renamed = { id: "renamed", ...entry, lastUserMessageAt: "2026-10-21T00:00:00Z", updatedAt: "2026-10-25T21:58:00Z" };
    const busy = { id: "busy", ...entry, lastUserMessageAt: "2026-10-25T12:00:00Z" };
    expect(byLastActivity([quiet, renamed, busy]).map((row) => row.id)).toEqual(["busy", "renamed", "marked-unread"]);
  });

  it("orders newest first and keeps sessions without activity last, in their order", () => {
    const rows = [
      { id: "none-1", ...entry },
      { id: "old", ...entry, lastUserMessageAt: "2026-10-20T00:00:00Z" },
      { id: "none-2", ...entry },
      { id: "new", ...entry, activeTurnStartedAt: "2026-10-25T21:00:00Z" },
    ];
    expect(byLastActivity(rows).map((row) => row.id)).toEqual(["new", "old", "none-1", "none-2"]);
  });
});

describe("model alias and vendor", () => {
  it.each([
    ["gpt-6-astra", "A6", "openai"],
    ["gpt-6.1-sol", "S6.1", "openai"],
    ["claude-opus-5-5", "O5.5", "anthropic"],
    ["claude-sonnet-4-5-20250929", "S4.5", "anthropic"],
    ["deepseek/deepseek-flash", "DSF", "deepseek"],
    ["glm-5.3-flash", "F5.3", "zai"],
    ["grok-4.6", "G4.6", "xai"],
    ["composer-2.5", "C2.5", "cursor"],
    ["google/gemma-4-31b-it", "G431", "google"],
  ])("%s is %s by %s", (model, alias, vendor) => {
    expect(modelAlias(model)).toBe(alias);
    expect(modelAlias(model).length).toBeLessThanOrEqual(4);
    expect(modelVendor(model)).toBe(vendor);
  });

  // R7: the model's author names the tile, never the route that serves it; each of the author's models has its own alias.
  const ROUTED = [
    ["openrouter/nvidia/nemotron-3.5-lightning:free", "nvidia", "L3.5"], ["opencode/nemotron-3.5-lightning-free", "nvidia", "L3.5"],
    ["openrouter/nvidia/nemotron-3-ultra-550b-a55b:free", "nvidia", "U3"], ["opencode/nemotron-3-ultra-free", "nvidia", "U3"],
    ["openrouter/nvidia/nemotron-3-super-120b-a12b:free", "nvidia", "S3"], ["openrouter/nvidia/nemotron-3-nano-omni-30b-a3b-reasoning:free", "nvidia", "N3"],
    ["opencode/nemotron-3.5-super-vl-free", "nvidia", "S3.5"],
    ["openrouter/cohere/north-mini-code:free", "cohere", "NORT"], ["openrouter/liquid/lfm-2.5-2.6b:free", "liquid", "L2.5"],
    ["opencode/mimo-v2.6-flash-free", "xiaomi", "MMVF"], ["opencode/minimax-m3.1-flash-free", "minimax", "MXMF"], ["opencode/longcat-2.5-preview-free", "meituan", "P2.5"],
    ["openrouter/inclusionai/ling-3.1-flash", "inclusionai", "F3.1"],
    ["openrouter/thinkingmachines/inkling:free", "thinkingmachines", "I"], ["openrouter/thinkingmachines/inkling-small:free", "thinkingmachines", "IS"],
    ["openrouter/google/gemma-4-26b-a4b-it:free", "google", "G426"], ["openrouter/google/gemma-4-31b-it:free", "google", "G431"],
    ["claude-opus-4-8[1m]", "anthropic", "O48M"], ["claude-opus-4-8", "anthropic", "O4.8"], ["claude-sonnet-5[1m]", "anthropic", "S5M"], ["claude-sonnet-5", "anthropic", "S5"],
    ["openrouter/openrouter/free", "unknown", "FREE"], ["opencode/beam-free", "unknown", "BEAM"], ["opencode/big-pickle", "unknown", "BIG"],
  ] as const;

  it.each(ROUTED)("%s is made by %s as %s", (model, vendor, alias) => {
    expect([modelVendor(model), modelAlias(model)]).toEqual([vendor, alias]);
    expect(alias.length).toBeLessThanOrEqual(4);
  });

  it("gives distinct models of one author distinct aliases; only one model on two routes shares one", () => {
    // One underlying model, without its route and pricing tag: "openrouter/nvidia/nemotron-3-ultra-550b-a55b:free" and "opencode/nemotron-3-ultra-free" are both Nemotron 3 Ultra.
    const underlying = (model: string) => model.split("/").pop()!.replace(/(:|-)free$/, "").replace(/-\d+b-a\d+b$/, "");
    const seen = new Map<string, string>();
    for (const [model, vendor, alias] of ROUTED) {
      const key = `${vendor} ${alias}`, earlier = seen.get(key);
      if (vendor !== "unknown" && earlier) expect(underlying(model)).toBe(earlier);
      seen.set(key, underlying(model));
    }
  });

  it("names the underlying vendor through a route, never the route", () => {
    const entry = { provider: "opencode", model: "deepseek/deepseek-flash", telemetry: { runtimeInfo: { sessionId: null, provider: "opencode", model: "deepseek/deepseek-flash" }, effectiveThinkingOptionId: "max" } };
    expect(rowModel(entry)).toMatchObject({ vendor: "deepseek", alias: "DSF", effort: "MAX", full: "deepseek/deepseek-flash", route: "via OpenCode" });
  });

  it("falls back to the runtime's reported effort only when the session reports none", () => {
    const runtimeInfo = { sessionId: null, provider: "codex", model: "gpt-6-astra", thinkingOptionId: "high" };
    expect(rowModel({ provider: "codex", model: "gpt-6-astra", telemetry: { runtimeInfo } }).effort).toBe("H");
    expect(rowModel({ provider: "codex", model: "gpt-6-astra", telemetry: { runtimeInfo, thinkingOptionId: "xhigh" } }).effort).toBe("XH");
  });

  it("keeps an unknown model unknown", () => {
    expect(modelAlias(null)).toBe("?");
    expect(modelVendor("house-model")).toBe("unknown");
    expect(modelAlias("house-model-without-version").length).toBeLessThanOrEqual(4);
  });

  it("shortens efforts and labels routes", () => {
    expect([effortShort("xhigh"), effortShort("high"), effortShort("max"), effortShort(null), effortShort("turbo")]).toEqual(["XH", "H", "MAX", null, "TUR"]);
    expect(routeLabel("codex")).toBe("via Codex");
    expect(routeLabel("opencode-zai", "OpenCode · Z.AI")).toBe("via OpenCode · Z.AI");
  });
});

describe("session token totals", () => {
  it("labels provider cumulative counts without claiming complete session totals", () => {
    const counts = { input: 200, output: 7, scope: "provider-cumulative" as const, source: "Fixture vendor snapshot; historical completeness unverified" };
    expect(tokenTotals(counts)).toMatchObject({ input: "reported ↓ 200", output: "↑ 7", spoken: "Provider-reported cumulative: 200 input, 7 output tokens" });
    expect(totalsDetail(counts)).toMatchObject({ label: "Provider-reported cumulative input ↓ and output ↑", value: expect.stringContaining("historical completeness unverified") });
    expect(tokenTotals({ ...counts, input: null, output: null }).spoken).toBe("Session token totals unknown");
  });
  it("uses only down input, up output and lowercase units", () => {
    expect(tokenTotals({ input: 124_800, output: 9_600 })).toMatchObject({ input: "↓ 124.8k", output: "↑ 9.6k" });
    expect(tokenTotals({ input: 1_200_000, output: 84_600 })).toMatchObject({ input: "↓ 1.2m", output: "↑ 84.6k" });
  });

  it("says in Details where whole-session totals come from, and that unknown ones are unknown", () => {
    expect(totalsDetail({ input: 124_800, output: 9_600, source: "SYNTHETIC totals for review; not read from any host" }).value).toBe("↓ 124.8k ↑ 9.6k · SYNTHETIC totals for review; not read from any host");
    expect(totalsDetail(null).value).toBe("Unknown: no supported read reports this session's totals.");
  });

  it("shows an unknown total as a dash, never zero", () => {
    expect(tokenTotals(null)).toEqual({ input: "↓ —", output: "↑ —", spoken: "Session token totals unknown" });
    expect(tokenTotals({ input: 0, output: null })).toMatchObject({ input: "↓ 0", output: "↑ —" });
  });
});

describe("row title and origin", () => {
  const entry = { title: "LSA • ADHOC • improve session switching", labels: {}, projectName: "DESK • ADAM", workspaceName: "ADHOC", updatedAt: "2026-10-25T21:58:00Z" } as never;

  it("moves a lead title's role and scope to the origin with desk, workspace and metadata age, naming a repeated value once", () => {
    expect(rowOrigin(entry, NOW)).toEqual({ title: "improve session switching", origin: ["LSA", "ADHOC", "ADAM", "upd 2m"] });
  });

  it("keeps any other title whole and leaves out what is not known", () => {
    expect(rowOrigin({ ...(entry as object), title: "Uncommitted ASMA modules", projectName: null, workspaceName: null, updatedAt: "garbage" } as never, NOW))
      .toEqual({ title: "Uncommitted ASMA modules", origin: [] });
  });

  it("moves a lone role prefix to the origin and keeps a prefix that is not the role", () => {
    expect(rowOrigin({ ...(entry as object), title: "LSA • REORGANISE DESK ADAM" } as never, NOW).title).toBe("REORGANISE DESK ADAM");
    expect(rowOrigin({ ...(entry as object), title: "Kontor • follow-up", labels: { role: "tpm" } } as never, NOW).title).toBe("Kontor • follow-up");
  });

  it("names the role a label declares over the title's", () => {
    expect(rowOrigin({ ...(entry as object), labels: { role: "tpm" } } as never, NOW).origin[0]).toBe("TPM");
  });

  it("carries the labelled task and PR on the origin line, without repeating a task the scope or a workspace the scope names", () => {
    expect(rowOrigin({ ...(entry as object), labels: { "jira.issue": "ASMA-8650", pr: "#505" } } as never, NOW).origin).toEqual(["LSA", "ADHOC", "ASMA-8650", "PR 505", "ADAM", "upd 2m"]);
    expect(rowOrigin({ ...(entry as object), title: "TPM • ASMA-8539 • navigator", labels: { task: "asma-8539", "kontor.ticket": "tkt_1" } } as never, NOW).origin).toEqual(["TPM", "ASMA-8539", "ADAM", "ADHOC", "upd 2m"]);
    expect(rowOrigin({ ...(entry as object), labels: { "kontor.ticket": "tkt_1" } } as never, NOW).origin).toContain("tkt_1");
    // A workspace title drops only the segments already on the line, and stays whole otherwise.
    expect(rowOrigin({ ...(entry as object), title: "QA • ASMA-8641 • verify", workspaceName: "TSW • ASMA-8641" } as never, NOW).origin).toEqual(["QA", "ASMA-8641", "ADAM", "TSW", "upd 2m"]);
    expect(rowOrigin({ ...(entry as object), workspaceName: "ADHOC • Composer popup" } as never, NOW).origin).toEqual(["LSA", "ADHOC", "ADAM", "Composer popup", "upd 2m"]);
    expect(rowOrigin({ ...(entry as object), workspaceName: "TSW • roles" } as never, NOW).origin).toEqual(["LSA", "ADHOC", "ADAM", "TSW • roles", "upd 2m"]);
  });
});

it("keeps child usage scope and qualification in Details without filling cumulative counts", () => {
 const totals = { input: null, output: null, scope: "provider-cumulative" as const, source: "Fork cumulative unqualified",
  recordedChildUsage: { scope: "recorded-child-responses" as const, label: "Recorded child response usage" as const, qualification: "Excludes parent history; completeness and billing unverified." as const, input: 1200, output: 20, total: 1220, responses: 2 } };
 expect(tokenTotals(totals)).toMatchObject({ input: "reported ↓ —", output: "↑ —", spoken: expect.stringContaining("Session token totals unknown") });
 expect(totalsDetail(totals)).toMatchObject({ label: "Provider-reported cumulative input ↓ and output ↑", value: expect.stringContaining("Recorded child response usage: ↓ 1,200 / ↑ 20") });
 expect(totalsDetail(totals).value).toContain("Excludes parent history; completeness and billing unverified.");
});

it("uses shared compact units for recorded counts without changing unknown cumulative semantics", () => {
 const recorded = { scope: "recorded-child-responses" as const, label: "Recorded child response usage" as const, qualification: "Excludes parent history; completeness and billing unverified." as const, input: 451_900_000, output: 1_700_000, total: 453_600_000, responses: 2048 };
 const totals = { input: null, output: null, scope: "provider-cumulative" as const, source: "Lifetime cumulative unknown", recordedChildUsage: recorded };
 expect(recordedChildUsageCompact(totals)).toBe("Recorded ↓451.9m · ↑1.7m");
 expect(recordedChildUsageCompact({ ...totals, recordedChildUsage: { ...recorded, input: 999_950, output: 0 } })).toBe("Recorded ↓1m · ↑0");
 expect(recordedChildUsageCompact(null)).toBeNull();
 expect(recordedChildUsageCompact({ input: null, output: null })).toBeNull();
 expect(tokenTotals(totals)).toMatchObject({ input: "reported ↓ —", output: "↑ —" });
 expect(totalsDetail(totals).value).toContain("451,900,000");
 expect(totalsDetail(totals).value).toContain("2,048 responses");
 expect(totalsDetail(totals).value).toContain(recorded.qualification);
});
