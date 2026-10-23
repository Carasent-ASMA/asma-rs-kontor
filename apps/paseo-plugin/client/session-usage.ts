import type { PaseoAgent } from "@getpaseo/client";

/** Native snapshot fields, copied with each directory update, never accumulated. */
export type SessionTelemetry = Pick<PaseoAgent,
  "runtimeInfo" | "lastUsage" | "thinkingOptionId" | "effectiveThinkingOptionId"
>;

export type SessionUsageEntry = {
  readonly provider: string;
  readonly model: string | null;
  readonly telemetry?: SessionTelemetry;
};

function text(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

/** Only known model IDs get a family label. Provider/account names are not models. */
export function shortModel(model: string | null): string {
  if (model === null) return "Model unknown";
  if (/^gpt-\d+(?:\.\d+)*-sol(?:-\d{4}-\d{2}-\d{2})?$/.test(model)) return "sol";
  if (/^claude-opus-\d+(?:[-.]\d+)*(?:-\d{8})?$/.test(model)) return "opus";
  if (model === "deepseek/deepseek-flash" || model === "deepseek-flash") return "ds-flash";
  if (/^(?:openrouter\/)?z-ai\/glm-\d+(?:\.\d+)*-flash$/.test(model)) return "glm-flash";
  // Unknown/custom IDs retain their full spelling and may wrap. Never slice two models into one label.
  return model;
}

export function modelPresentation(entry: SessionUsageEntry) {
  const runtime = entry.telemetry?.runtimeInfo;
  const observed = text(runtime?.extra?.runtimeModel);
  const configured = text(entry.model);
  const selection = text(runtime?.model);
  const model = observed ?? selection ?? configured;
  let source = "Model unknown";
  if (observed) source = "Last reported runtime model";
  else if (selection) source = "Runtime model selection";
  else if (configured) source = "Configured model";
  const qualifier = observed ? "reported" : "configured";
  return {
    model, source, configured, selection,
    short: shortModel(model),
    label: model === null ? shortModel(model) : `${shortModel(model)} · ${qualifier}`,
  };
}

export function validTokenCount(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

// Compact row units stay K/M across device locales; exact Details keep localized numbers.
const compactTokens = new Intl.NumberFormat("en", { notation: "compact", maximumFractionDigits: 1 });
const exactTokenNumber = new Intl.NumberFormat();

export function formatTokens(value: number, _locale?: string): string {
  return compactTokens.format(value);
}

/** Format an exact report, preserving absent/invalid values and explicit zero. */
export function exactTokens(value: unknown, locale?: string): string {
  if (value === undefined || value === null) return "Not reported";
  const formatter = locale === undefined ? exactTokenNumber : new Intl.NumberFormat(locale);
  return validTokenCount(value) ? formatter.format(value) : "Invalid report (not shown)";
}

// An opaque route cannot establish which underlying adapter is in use.
const adapterOf = (entry: SessionUsageEntry) => entry.telemetry?.runtimeInfo?.provider ?? entry.provider;

/**
 * The one reading of Paseo's lastUsage: the latest provider report, which each report replaces.
 * Paseo supplies no session total, so none is derived here. Only the adapter contract decides
 * whether cache reads overlap input; counter values never do, so nothing is counted twice.
 */
export function latestReport(entry: SessionUsageEntry) {
  const usage = entry.telemetry?.lastUsage;
  const reported = validTokenCount(usage?.inputTokens) ? usage.inputTokens : null;
  const cached = validTokenCount(usage?.cachedInputTokens) ? usage.cachedInputTokens : null;
  const adapter = adapterOf(entry);
  // Claude and OpenCode report cache reads apart from input. Codex input includes them.
  const separate = adapter === "claude" || adapter === "opencode";
  const added = separate && reported !== null && cached !== null;
  const sum = added ? reported + cached : reported;
  const inputOverflow = sum !== null && !Number.isSafeInteger(sum);
  return {
    input: inputOverflow ? null : sum,
    inputOverflow,
    addedCacheReads: added ? cached : null,
    // An unidentified route does not say whether input includes cache reads, so they stay apart.
    unplacedCacheReads: separate || adapter === "codex" ? null : cached,
    // Only Codex input is complete (cache reads included, no cache writes); elsewhere it is a lower bound.
    lowerBound: reported !== null && adapter !== "codex",
    output: validTokenCount(usage?.outputTokens) ? usage.outputTokens : null,
  };
}

/** Row text and spoken label for the latest report; never a session total. */
export function usagePresentation(entry: SessionUsageEntry, locale?: string) {
  const usage = entry.telemetry?.lastUsage;
  const report = latestReport(entry);
  const input = report.input === null ? "—" : `${report.lowerBound ? "≥" : ""}${formatTokens(report.input, locale)}`;
  const output = report.output === null ? "—" : formatTokens(report.output, locale);
  const unplaced = report.unplacedCacheReads === null ? "" : ` · ${formatTokens(report.unplacedCacheReads, locale)} cached`;
  const spokenInput = report.inputOverflow ? "unknown; input plus cache reads exceeds the exact integer range"
    : report.input === null ? exactTokens(usage?.inputTokens, locale)
      : `${report.lowerBound ? "at least " : ""}${exactTokens(report.input, locale)}${report.addedCacheReads === null ? "" : `, including ${exactTokens(report.addedCacheReads, locale)} cache reads`}`;
  const spokenCache = report.unplacedCacheReads === null ? "" : ` Cache reads: ${exactTokens(report.unplacedCacheReads, locale)}; whether input includes them is unknown for this route.`;
  return {
    label: report.input === null && report.output === null && report.unplacedCacheReads === null ? "—" : `Last report ${input} ↓ · ${output} ↑${unplaced}`,
    spoken: `Last usage report, not a session total. Input tokens: ${spokenInput}. Output tokens: ${exactTokens(usage?.outputTokens, locale)}.${spokenCache}`,
  };
}

export function sessionUsageLabel(entry: SessionUsageEntry): string {
  const model = modelPresentation(entry);
  return `${model.source}: ${model.model ?? "Unknown"}. ${usagePresentation(entry).spoken}`;
}

/** Reporting limits for the provider actually identified by this snapshot. */
export function usageScope(entry: SessionUsageEntry): string {
  const provider = adapterOf(entry);
  if (provider === "codex") return "Codex tokenUsage.last: the latest model request, not the turn or the session. Cached input is included in input; it is not added again. Output includes reasoning. Paseo does not pass on the thread total.";
  if (provider === "claude") return "Claude result for the latest turn. Input excludes cache reads and cache writes; the row adds cache reads. Cache-write tokens are not exposed, so input is a lower bound. Output includes thinking. Streaming reports may contain context only.";
  if (provider === "opencode") return "OpenCode report. v1 reports the latest step; v2 projects session counters. This snapshot does not identify the adapter version. Input excludes cache reads; the row adds them. Cache-write and reasoning counts are not exposed, so counts are lower bounds.";
  return "Provider-specific latest report; the reporting window and cache inclusion are not established for this route. Cache reads are shown apart and never added to input. No session total can be calculated.";
}

export function sessionUsageLines(entry: SessionUsageEntry, stale = false, locale?: string): { label: string; value: string }[] {
  const model = modelPresentation(entry);
  const native = entry.telemetry;
  return [
    { label: "Model", value: model.model ?? "Unknown: the native snapshot did not supply a model" },
    { label: "Model source", value: model.source === "Last reported runtime model" ? "Native runtimeInfo.extra.runtimeModel (last observed). May precede a model switch; not proof of the active call's model." : `${model.source}. runtimeInfo.model can be configuration; actual runtime execution is not established.` },
    { label: "Configured model", value: model.configured ?? "Not reported" },
    { label: "Runtime selection", value: model.selection ?? "Not reported" },
    { label: "Provider / route", value: entry.provider },
    { label: "Runtime provider", value: text(native?.runtimeInfo?.provider) ?? "Not reported" },
    { label: "Effort", value: `Effective: ${text(native?.effectiveThinkingOptionId) ?? "not reported"}; configured: ${text(native?.thinkingOptionId) ?? "not reported"}; runtime selection: ${text(native?.runtimeInfo?.thinkingOptionId) ?? "not reported"}` },
    { label: "Reported input tokens", value: exactTokens(native?.lastUsage?.inputTokens, locale) },
    { label: "Reported output tokens", value: exactTokens(native?.lastUsage?.outputTokens, locale) },
    { label: "Reported cache-read tokens", value: `${exactTokens(native?.lastUsage?.cachedInputTokens, locale)}. Inclusion depends on the adapter (see report scope). Added to the row's input only for adapters that report them apart (Claude, OpenCode).` },
    { label: "Report source and scope", value: `Native lastUsage. ${usageScope(entry)}` },
    { label: "Report freshness", value: `${stale ? "Host inventory is stale/offline; showing last known fields. " : ""}No usage observation timestamp is supplied. Session last activity is not the usage time; age is unknown, even on a live connection.` },
    { label: "Report limits", value: "Input and output are separate reported counters, not a whole-session consumed total or context occupancy. Model attribution is unknown, including after model/provider switches. Repeated reports replace the snapshot; decreases/resets are not negative consumption. Missing counters stay unknown; explicit zero stays zero. No history is reconstructed." },
  ];
}
