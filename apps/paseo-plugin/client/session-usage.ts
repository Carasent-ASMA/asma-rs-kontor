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

const compactTokens = new Intl.NumberFormat(undefined, { notation: "compact", maximumFractionDigits: 1 });
const exactTokenNumber = new Intl.NumberFormat();

export function formatTokens(value: number, locale?: string): string {
  const formatter = locale === undefined ? compactTokens : new Intl.NumberFormat(locale, { notation: "compact", maximumFractionDigits: 1 });
  return formatter.format(value);
}

function exactTokens(value: unknown, locale?: string): string {
  if (value === undefined || value === null) return "Not reported";
  const formatter = locale === undefined ? exactTokenNumber : new Intl.NumberFormat(locale);
  return validTokenCount(value) ? formatter.format(value) : "Invalid report (not shown)";
}

/** No total: these providers do not share input/cache or reporting-window semantics. */
export function usagePresentation(entry: SessionUsageEntry, locale?: string) {
  const usage = entry.telemetry?.lastUsage;
  const input = validTokenCount(usage?.inputTokens) ? formatTokens(usage.inputTokens, locale) : "—";
  const output = validTokenCount(usage?.outputTokens) ? formatTokens(usage.outputTokens, locale) : "—";
  const reported = validTokenCount(usage?.inputTokens) || validTokenCount(usage?.outputTokens);
  return {
    label: reported ? `Report: ${input} in · ${output} out` : "Usage not reported",
    spoken: `Reported input tokens: ${exactTokens(usage?.inputTokens, locale)}. Reported output tokens: ${exactTokens(usage?.outputTokens, locale)}.`,
  };
}

export function sessionUsageLabel(entry: SessionUsageEntry): string {
  const model = modelPresentation(entry);
  return `${model.source}: ${model.model ?? "Unknown"}. ${usagePresentation(entry).spoken} Report age and model attribution unknown; see Details.`;
}

function usageScope(entry: SessionUsageEntry): string {
  // An opaque route cannot establish which underlying adapter is in use.
  const provider = entry.telemetry?.runtimeInfo?.provider ?? entry.provider;
  if (provider === "codex") return "Codex tokenUsage.last report, not a session total. The reporting window is not identified in the snapshot. Cached input is included in input; do not add it again. Reasoning breakdown and cumulative totals are not exposed by this snapshot.";
  if (provider === "claude") return "Claude result report. Input excludes cache reads and cache writes. Cache-write tokens are not exposed, so these counts cannot give all consumed tokens. Streaming reports may contain context only.";
  if (provider === "opencode") return "OpenCode report. v1 reports the latest step; v2 projects session counters. This snapshot does not identify the adapter version. Cache-write and reasoning counts are not exposed separately; do not sum these fields.";
  return "Provider-specific latest report; the reporting window and cache inclusion are not established for this route. No session total can be calculated.";
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
    { label: "Reported cache-read tokens", value: `${exactTokens(native?.lastUsage?.cachedInputTokens, locale)}. Inclusion depends on the adapter (see report scope). Never added to the row's counts.` },
    { label: "Report source and scope", value: `Native lastUsage. ${usageScope(entry)}` },
    { label: "Report freshness", value: `${stale ? "Host inventory is stale/offline; showing last known fields. " : ""}No usage observation timestamp is supplied. Session last activity is not the usage time; age is unknown, even on a live connection.` },
    { label: "Report limits", value: "Input and output are separate reported counters, not a whole-session consumed total or context occupancy. Model attribution is unknown, including after model/provider switches. Repeated reports replace the snapshot; decreases/resets are not negative consumption. Missing counters stay unknown; explicit zero stays zero. No history is reconstructed." },
  ];
}
