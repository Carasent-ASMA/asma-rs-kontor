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

  it("keeps exact custom IDs/counts and uncertainty among visible facts", () => {
    const model = `custom/${"very-long-version-".repeat(12)}`;
    const entry = { provider: "private-route", model, telemetry: { runtimeInfo: { sessionId: null, provider: "claude", model }, lastUsage: { inputTokens: 1_234_567, outputTokens: 0, cachedInputTokens: 123 } } };
    expect(sessionMetadataSections(entry).map(section => section.title)).toEqual(["Model", "Reported usage"]);
    expect(values(entry, true)).toMatchObject({ "Model ID": model, "Input tokens": "1,234,567", "Output tokens": "0", "Cache-read tokens": "123", "Report time": "Unknown", "Model attribution": "Unknown", "Active execution": "Not confirmed", "Runtime provider": "claude" });
    expect(values(entry)["Scope"]).toContain("session total unavailable");
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
    expect(label).toContain("Reported output tokens: 0");
    expect(label).not.toMatch(/Details|sliders|attribution|Report age/);
  });
});
