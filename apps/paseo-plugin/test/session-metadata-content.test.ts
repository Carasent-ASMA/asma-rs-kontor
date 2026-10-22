import { describe, expect, it } from "vitest";
import { modelSourceHelp, reportedUsageHelp, sessionMetadataSections, sessionReportNotes } from "../client/session-metadata-content";
import { sessionUsageLabel, type SessionUsageEntry } from "../client/session-usage";
import { compactUsage } from "../client/session-usage-density";

const configured: SessionUsageEntry = { provider: "codex", model: "gpt-6.1-sol" };
const values = (entry: SessionUsageEntry, stale = false) => Object.fromEntries(sessionMetadataSections(entry, stale, "en").flatMap(section => section.values.map(line => [line.label, line.value])));

describe("contextual metadata help", () => {
  it("explains only the applicable model source, independently of usage and the symbol guide", () => {
    expect(modelSourceHelp(configured)).toContain("Configured model");
    const selected = { ...configured, telemetry: { runtimeInfo: { sessionId: null, provider: "codex", model: "other-selection" } } };
    expect(modelSourceHelp(selected)).toContain("Runtime model selection");
    const reported = { ...selected, telemetry: { runtimeInfo: { ...selected.telemetry.runtimeInfo, extra: { runtimeModel: "older-report" } } } };
    expect(modelSourceHelp(reported)).toContain("precede a switch");
    expect(values(reported)).toMatchObject({ "Model ID": "older-report", "Configured model": "gpt-6.1-sol", "Runtime model selection": "other-selection" });
    expect(modelSourceHelp({ provider: "opaque", model: null })).not.toContain("active call");
    for (const entry of [configured, selected, reported]) expect(compactUsage(entry).modelHelp).not.toMatch(/arrows|tokens|sliders|radio/i);
  });

  it("gives missing/invalid counter context without confusing explicit zero with absence", () => {
    const partial = { ...configured, telemetry: { lastUsage: { inputTokens: 0 } } };
    expect(values(partial)).toMatchObject({ "Input tokens": "0", "Output tokens": "Not reported" });
    expect(reportedUsageHelp(partial)).toContain("Output count not reported");
    const invalid = { ...configured, telemetry: { lastUsage: { inputTokens: -1, outputTokens: 0 } } };
    expect(values(invalid)["Input tokens"]).toBe("Invalid report (not shown)");
    expect(reportedUsageHelp(invalid)).toContain("Input count invalid");
    expect(reportedUsageHelp(configured)).not.toContain("session total");
  });

  it("names the last-report scope, separate cache reads and the lower bound in the counter help", () => {
    const lastUsage = { inputTokens: 4, cachedInputTokens: 200_611, outputTokens: 16 };
    const claude = { provider: "claude", model: null, telemetry: { lastUsage } };
    expect(reportedUsageHelp(claude)).toBe("Last provider report, not a session total; Paseo supplies no session total or report time. Input includes cache reads, which this adapter reports apart from input. ≥ marks a lower bound: cache writes or cache inclusion are not reported.");
    expect(reportedUsageHelp({ ...configured, telemetry: { lastUsage: { inputTokens: 12, cachedInputTokens: 10, outputTokens: 4 } } })).toBe("Last provider report, not a session total; Paseo supplies no session total or report time.");
    expect(values(claude)["Input with cache reads"]).toBe("200,615 or more");
    expect(values(claude)["Cache composition"]).toBeUndefined();
    expect(values({ ...configured, telemetry: { lastUsage: { inputTokens: 12, cachedInputTokens: 10 } } })["Input with cache reads"]).toBeUndefined();
    // The same counters on an unidentified route: shown apart, with no claim about how the provider reports them.
    const aliased = { provider: "claude-team-route", model: null, telemetry: { lastUsage } };
    expect(reportedUsageHelp(aliased)).toBe("Last provider report, not a session total; Paseo supplies no session total or report time. Cache reads are shown apart: this route does not establish whether input includes them. ≥ marks a lower bound: cache writes or cache inclusion are not reported.");
    expect(values(aliased)).toMatchObject({ "Input tokens": "4", "Cache-read tokens": "200,611", "Cache composition": "Unknown for this route; cache reads are not added to input" });
    expect(values(aliased)["Input with cache reads"]).toBeUndefined();
    expect(sessionReportNotes(aliased).join(" ")).toContain("Cache reads are shown apart and never added to input");
  });

  it("reports an unsafe input sum as unknown in help and Details", () => {
    const huge = { provider: "claude", model: null, telemetry: { lastUsage: { inputTokens: Number.MAX_SAFE_INTEGER, cachedInputTokens: 10, outputTokens: 1 } } };
    expect(reportedUsageHelp(huge)).toContain("Input is unknown: input plus cache reads exceeds the exact integer range.");
    expect(reportedUsageHelp(huge)).not.toContain("Input count");
    expect(values(huge)["Input with cache reads"]).toBe("Unknown; the sum exceeds the exact integer range");
  });

  it("keeps exact custom IDs/counts and uncertainty among visible facts", () => {
    const model = `custom/${"very-long-version-".repeat(12)}`;
    const entry = { provider: "private-route", model, telemetry: { runtimeInfo: { sessionId: null, provider: "claude", model }, lastUsage: { inputTokens: 1_234_567, outputTokens: 0, cachedInputTokens: 123 } } };
    expect(sessionMetadataSections(entry).map(section => section.title)).toEqual(["Model", "Last usage report"]);
    expect(values(entry, true)).toMatchObject({ "Model ID": model, "Input tokens": "1,234,567", "Output tokens": "0", "Cache-read tokens": "123", "Report time": "Unknown", "Model attribution": "Unknown", "Active execution": "Not confirmed", "Runtime provider": "claude" });
    expect(values(entry)).toMatchObject({ "Scope": "Latest provider report; each report replaces it", "Session total": "Unknown; Paseo supplies only the latest report", "Input with cache reads": "1,234,690 or more" });
    expect(values(entry, true)["Inventory"]).toContain("Last known");
    expect(values(entry)["Configured model"]).toBeUndefined();
  });

  it("shows only applicable provider notes, and no provider caveats without reported counters", () => {
    const report = { ...configured, telemetry: { lastUsage: { inputTokens: 12, outputTokens: 4 } } };
    expect(sessionReportNotes(report).join(" ")).toContain("Codex");
    expect(sessionReportNotes(report).join(" ")).not.toContain("Claude");
    expect(sessionReportNotes({ ...report, provider: "claude" }).join(" ")).toContain("Cache-write tokens are not exposed");
    expect(sessionReportNotes(configured).join(" ")).not.toMatch(/Codex|cache|model switches/);
    expect(values(configured)["Report time"]).toBeUndefined();
  });

  it("row announcements contain the session facts without repeated static instructions", () => {
    const label = sessionUsageLabel({ ...configured, telemetry: { lastUsage: { outputTokens: 0 } } });
    expect(label).toContain("gpt-6.1-sol");
    expect(label).toContain("Output tokens: 0.");
    expect(label).toContain("not a session total");
    expect(label).not.toMatch(/Details|sliders|attribution|Report age/);
  });
});
