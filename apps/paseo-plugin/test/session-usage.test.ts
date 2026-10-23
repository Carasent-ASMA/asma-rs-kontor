import { describe, expect, it } from "vitest";
import { modelPresentation, shortModel, formatTokens, validTokenCount, latestReport, usagePresentation, sessionUsageLabel, sessionUsageLines, type SessionUsageEntry } from "../client/session-usage";
import { sessionEntry } from "./session-fixtures";

const configured: SessionUsageEntry = { provider: "codex", model: "gpt-6.1-sol" };
const report: SessionUsageEntry = { ...configured, telemetry: { lastUsage: { inputTokens: 12_400, outputTokens: 320, cachedInputTokens: 10_000 } } };
const lines = (entry: SessionUsageEntry, stale = false) => Object.fromEntries(sessionUsageLines(entry, stale, "en").map(({ label, value }) => [label, value]));

describe("native model presentation", () => {
  it("prefers a last observed model while exposing configured selection and observation limits", () => {
    const switched = { ...configured, telemetry: { runtimeInfo: { sessionId: null, provider: "claude", model: "claude-opus-5-5", extra: { runtimeModel: "claude-opus-5-5-20261001" } } } };
    expect(modelPresentation(switched)).toMatchObject({ model: "claude-opus-5-5-20261001", source: "Last reported runtime model", short: "opus" });
    expect(lines(switched)["Configured model"]).toBe("gpt-6.1-sol");
    expect(lines(switched)["Model source"]).toContain("May precede a model switch");
  });

  it("does not call runtimeInfo.model observed execution", () => {
    const entry = { provider: "private-route", model: null, telemetry: { runtimeInfo: { sessionId: null, provider: "codex", model: "gpt-6.1-sol" } } };
    expect(modelPresentation(entry).source).toBe("Runtime model selection");
    expect(modelPresentation(entry).label).toContain("configured");
  });

  it("prefers the runtime selection to a different launch configuration without claiming observation", () => {
    const entry = { ...configured, telemetry: { runtimeInfo: { sessionId: null, provider: "codex", model: "later-selection" } } };
    expect(modelPresentation(entry).model).toBe("later-selection");
    expect(modelPresentation(entry).source).toBe("Runtime model selection");
    expect(lines(entry)["Configured model"]).toBe("gpt-6.1-sol");
  });

  it("never obtains a model from route, account or seat title", () => {
    const entry = { ...sessionEntry("fictional"), provider: "opencode/deepseek/deepseek-flash", title: "SWE • sol opus fable", model: null };
    expect(modelPresentation(entry).model).toBeNull();
    expect(modelPresentation(entry).short).toBe("Model unknown");
  });

  it.each([
    ["gpt-6.1-sol", "sol"], ["claude-opus-5-5", "opus"],
    ["deepseek/deepseek-flash", "ds-flash"], ["openrouter/z-ai/glm-5.3-flash", "glm-flash"],
    ["glm-unknown-flash", "glm-unknown-flash"], ["private-account-fable", "private-account-fable"],
    ["custom/model-with-a-long-specific-version-123456789", "custom/model-with-a-long-specific-version-123456789"],
  ])("labels only recognized IDs: %s", (model, label) => expect(shortModel(model)).toBe(label));

  it("keeps exact model, route, version and both effort sources accessible", () => {
    const entry = { ...report, provider: "private-route", telemetry: { ...report.telemetry, thinkingOptionId: "high", effectiveThinkingOptionId: "xhigh", runtimeInfo: { sessionId: null, provider: "codex", model: "gpt-6.1-sol" } } };
    expect(sessionUsageLabel(entry)).toContain("gpt-6.1-sol");
    expect(lines(entry)["Provider / route"]).toBe("private-route");
    expect(lines(entry)["Effort"]).toContain("Effective: xhigh; configured: high");
  });
});

describe("latest report, without invented session totals", () => {
  const routed = (provider: string, lastUsage: NonNullable<SessionUsageEntry["telemetry"]>["lastUsage"]): SessionUsageEntry =>
    ({ provider, model: null, telemetry: { runtimeInfo: { sessionId: null, provider }, lastUsage } });

  it("labels a days-long Claude turn report as the last report and counts its separate cache reads", () => {
    // Synthetic copy of an observed Claude report: 4 uncached input tokens, ~200K cache reads.
    const claude = routed("claude", { inputTokens: 4, cachedInputTokens: 200_611, outputTokens: 429, contextWindowUsedTokens: 101_500 });
    expect(latestReport(claude)).toEqual({ input: 200_615, inputOverflow: false, addedCacheReads: 200_611, unplacedCacheReads: null, lowerBound: true, output: 429 });
    expect(usagePresentation(claude, "en").label).toBe("Last report ≥200.6K ↓ · 429 ↑");
    expect(usagePresentation(claude, "en").spoken).toBe("Last usage report, not a session total. Input tokens: at least 200,615, including 200,611 cache reads. Output tokens: 429.");
  });

  it("never adds cache reads that Codex input already includes", () => {
    expect(usagePresentation(report, "en").label).toBe("Last report 12.4K ↓ · 320 ↑");
    expect(latestReport(report).addedCacheReads).toBeNull();
    expect(lines(report)["Reported input tokens"]).toBe("12,400");
    expect(lines(report)["Reported cache-read tokens"]).toContain("10,000");
    expect(lines(report)["Report source and scope"]).toContain("included in input");
  });

  it("does not treat a cache count larger than Codex input as proof of separate counters", () => {
    const codex = routed("codex", { inputTokens: 50, cachedInputTokens: 100, outputTokens: 3 });
    expect(latestReport(codex)).toEqual({ input: 50, inputOverflow: false, addedCacheReads: null, unplacedCacheReads: null, lowerBound: false, output: 3 });
    expect(usagePresentation(codex, "en").label).toBe("Last report 50 ↓ · 3 ↑");
    expect(usagePresentation(codex, "en").spoken).not.toMatch(/including|cache reads/i);
  });

  it.each([
    ["codex-team-route", { inputTokens: 276_814, cachedInputTokens: 275_072, outputTokens: 19 }, "Last report ≥276.8K ↓ · 19 ↑ · 275.1K cached"],
    ["claude-team-route", { inputTokens: 4, cachedInputTokens: 200_611, outputTokens: 429 }, "Last report ≥4 ↓ · 429 ↑ · 200.6K cached"],
  ])("keeps an unidentified route's cache reads apart without claiming their provenance: %s", (provider, lastUsage, label) => {
    const aliased = routed(provider, lastUsage);
    expect(latestReport(aliased)).toEqual({ input: lastUsage.inputTokens, inputOverflow: false, addedCacheReads: null, unplacedCacheReads: lastUsage.cachedInputTokens, lowerBound: true, output: lastUsage.outputTokens });
    expect(usagePresentation(aliased, "en").label).toBe(label);
    const spoken = usagePresentation(aliased, "en").spoken;
    expect(spoken).toContain("whether input includes them is unknown for this route");
    expect(spoken).not.toMatch(/including|reports? (them )?(separately|apart)/);
    expect(lines(aliased)["Report source and scope"]).toContain("never added to input");
  });

  it("shows an unidentified route's cache reads when it reports no input or output", () => {
    const cacheOnly = routed("opaque-route", { cachedInputTokens: 9 });
    expect(usagePresentation(cacheOnly, "en").label).toBe("Last report — ↓ · — ↑ · 9 cached");
    expect(usagePresentation(cacheOnly, "en").spoken).toContain("Cache reads: 9; whether input includes them is unknown for this route.");
    // Identified adapters place cache reads by contract, so cache alone is still no consumption counter.
    expect(usagePresentation(routed("codex", { cachedInputTokens: 9 })).label).toBe("—");
    expect(usagePresentation(routed("claude", { cachedInputTokens: 9 })).label).toBe("—");
  });

  it.each(["claude", "opencode"])("turns an unsafe %s input sum into unknown, never a false exact total", (provider) => {
    const huge = routed(provider, { inputTokens: Number.MAX_SAFE_INTEGER, cachedInputTokens: 10, outputTokens: 1 });
    expect(latestReport(huge)).toMatchObject({ input: null, inputOverflow: true, addedCacheReads: 10, output: 1 });
    expect(usagePresentation(huge, "en").label).toBe("Last report — ↓ · 1 ↑");
    expect(usagePresentation(huge, "en").spoken).toContain("Input tokens: unknown; input plus cache reads exceeds the exact integer range.");
    expect(latestReport(routed(provider, { inputTokens: Number.MAX_SAFE_INTEGER - 10, cachedInputTokens: 10 })).input).toBe(Number.MAX_SAFE_INTEGER);
  });

  it.each([
    ["claude", { inputTokens: 3_000, cachedInputTokens: 2_000, outputTokens: 5 }, 5_000],
    ["opencode", { inputTokens: 100, cachedInputTokens: 50, outputTokens: 7 }, 150],
    ["claude", { inputTokens: 3_000, outputTokens: 5 }, 3_000],
    ["claude", { inputTokens: 3_000, cachedInputTokens: -1, outputTokens: 5 }, 3_000],
  ])("follows %s cache semantics once", (provider, lastUsage, input) => {
    expect(latestReport(routed(provider, lastUsage))).toMatchObject({ input, lowerBound: true, output: lastUsage.outputTokens });
  });

  it("replaces rather than accumulates repeated, streamed or lower reports", () => {
    const turn = routed("claude", { inputTokens: 4, cachedInputTokens: 100_000, outputTokens: 16 });
    const first = usagePresentation(turn, "en");
    expect(usagePresentation({ ...turn }, "en")).toEqual(first);
    // A Claude streaming update carries context size only: unknown, not zero and not the previous turn.
    expect(usagePresentation(routed("claude", { contextWindowUsedTokens: 100_020 })).label).toBe("—");
    expect(usagePresentation(routed("claude", { inputTokens: 2, cachedInputTokens: 10, outputTokens: 1 }), "en").label).toBe("Last report ≥12 ↓ · 1 ↑");
  });

  it("distinguishes explicit zero from missing or context-only usage", () => {
    expect(usagePresentation({ ...configured, telemetry: { lastUsage: { inputTokens: 0, outputTokens: 0 } } }).label).toBe("Last report 0 ↓ · 0 ↑");
    expect(usagePresentation(configured).label).toBe("—");
    expect(usagePresentation({ ...configured, telemetry: { lastUsage: { contextWindowUsedTokens: 123_456 } } }).label).toBe("—");
    expect(usagePresentation({ ...configured, telemetry: { lastUsage: { outputTokens: 0 } } }).label).toBe("Last report — ↓ · 0 ↑");
    expect(usagePresentation({ ...configured, telemetry: { lastUsage: { cachedInputTokens: 9, outputTokens: 0 } } }).spoken).toContain("Input tokens: Not reported.");
  });

  it.each([-1, NaN, Infinity, 1.5, Number.MAX_SAFE_INTEGER + 1])("rejects invalid counters %s", (inputTokens) => {
    const entry = { ...configured, telemetry: { lastUsage: { inputTokens, outputTokens: 2 } } };
    expect(validTokenCount(inputTokens)).toBe(false);
    expect(usagePresentation(entry).label).toBe("Last report — ↓ · 2 ↑");
    expect(usagePresentation(entry).spoken).toContain("Input tokens: Invalid report (not shown).");
    expect(lines(entry)["Reported input tokens"]).toBe("Invalid report (not shown)");
  });

  it("keeps compact row units stable while preserving exact localized details", () => {
    expect(formatTokens(1_234_567, "en")).toBe("1.2M");
    expect(lines({ ...configured, telemetry: { lastUsage: { outputTokens: 1_234_567 } } })["Reported output tokens"]).toBe("1,234,567");
    expect(formatTokens(12_400, "nb")).toBe("12.4K");
  });

  it.each([undefined, "en", "nb-NO", "de-DE"])("uses the same precision and threshold rounding in %s", locale => {
    for (const [value, label] of [[0, "0"], [999, "999"], [1_000, "1K"], [1_049, "1K"], [1_050, "1.1K"], [300_000, "300K"],
      [999_949, "999.9K"], [999_950, "1M"], [1_000_000, "1M"], [1_200_000, "1.2M"]] as const) {
      expect(formatTokens(value, locale)).toBe(label);
    }
    const entry = routed("opaque-route", { inputTokens: 300_000, outputTokens: 1_200_000, cachedInputTokens: 234_567 });
    expect(usagePresentation(entry, locale).label).toBe("Last report ≥300K ↓ · 1.2M ↑ · 234.6K cached");
    expect(usagePresentation(entry, locale).spoken).toContain(new Intl.NumberFormat(locale).format(300_000));
    expect(sessionUsageLines(entry, false, locale).find(line => line.label === "Reported output tokens")?.value).toBe(new Intl.NumberFormat(locale).format(1_200_000));
  });

  it.each(["claude", "opencode", "cursor", "opaque-alias"])("discloses provider-specific limits for %s", (provider) => {
    const details = lines({ ...report, provider });
    expect(details["Report source and scope"]).toContain("Native lastUsage");
    expect(details["Report limits"]).toContain("not a whole-session consumed total");
    if (provider === "claude") expect(details["Report source and scope"]).toContain("Cache-write tokens are not exposed");
    if (provider === "opencode") expect(details["Report source and scope"]).toContain("does not identify the adapter version");
  });

  it("does not attribute old counts to a newly selected model", () => {
    const switched = { ...report, model: "another/model" };
    expect(lines(switched)["Report limits"]).toContain("Model attribution is unknown");
    expect(usagePresentation(switched).label).toBe(usagePresentation(report).label);
  });

  it("names age as unknown even when live and identifies offline inventory", () => {
    expect(lines(report)["Report freshness"]).toContain("age is unknown");
    expect(lines(report, true)["Report freshness"]).toContain("stale/offline");
    expect(lines(report)["Report limits"]).toContain("decreases/resets");
  });
});
