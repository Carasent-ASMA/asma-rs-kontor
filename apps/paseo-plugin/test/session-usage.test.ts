import { describe, expect, it } from "vitest";
import { modelPresentation, shortModel, formatTokens, validTokenCount, usagePresentation, sessionUsageLabel, sessionUsageLines, type SessionUsageEntry } from "../client/session-usage";
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

describe("reported tokens, without invented consumed totals", () => {
  it("keeps input/output separate and never adds overlapping cache input", () => {
    expect(usagePresentation(report, "en").label).toBe("Report: 12.4K in · 320 out");
    expect(lines(report)["Reported input tokens"]).toBe("12,400");
    expect(lines(report)["Reported cache-read tokens"]).toContain("10,000");
    expect(lines(report)["Report source and scope"]).toContain("included in input");
  });

  it("distinguishes explicit zero from missing or context-only usage", () => {
    expect(usagePresentation({ ...configured, telemetry: { lastUsage: { inputTokens: 0, outputTokens: 0 } } }).label).toBe("Report: 0 in · 0 out");
    expect(usagePresentation(configured).label).toBe("Usage not reported");
    expect(usagePresentation({ ...configured, telemetry: { lastUsage: { contextWindowUsedTokens: 123_456 } } }).label).toBe("Usage not reported");
    expect(usagePresentation({ ...configured, telemetry: { lastUsage: { outputTokens: 0 } } }).label).toBe("Report: — in · 0 out");
  });

  it.each([-1, NaN, Infinity, 1.5, Number.MAX_SAFE_INTEGER + 1])("rejects invalid counters %s", (inputTokens) => {
    const entry = { ...configured, telemetry: { lastUsage: { inputTokens, outputTokens: 2 } } };
    expect(validTokenCount(inputTokens)).toBe(false);
    expect(usagePresentation(entry).label).toBe("Report: — in · 2 out");
    expect(lines(entry)["Reported input tokens"]).toBe("Invalid report (not shown)");
  });

  it("formats large counts with Intl while preserving exact localized details", () => {
    expect(formatTokens(1_234_567, "en")).toBe("1.2M");
    expect(lines({ ...configured, telemetry: { lastUsage: { outputTokens: 1_234_567 } } })["Reported output tokens"]).toBe("1,234,567");
    expect(formatTokens(12_400, "nb")).toBe(new Intl.NumberFormat("nb", { notation: "compact", maximumFractionDigits: 1 }).format(12_400));
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
