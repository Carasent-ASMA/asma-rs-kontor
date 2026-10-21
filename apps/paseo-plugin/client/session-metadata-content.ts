import { exactTokens, modelPresentation, usageScope, validTokenCount, type SessionUsageEntry } from "./session-usage";

export type DetailValue = { readonly label: string; readonly value: string };
export type DetailSection = { readonly title: string; readonly values: readonly DetailValue[] };
const supplied = (value: string | null | undefined) => value?.trim() || null;

/** Explain only the source that supplies this row's model. Exact IDs stay in Details. */
export function modelSourceHelp(entry: SessionUsageEntry): string {
  const model = modelPresentation(entry);
  if (model.model === null) return "Paseo did not supply a model.";
  if (model.source === "Last reported runtime model") return "Last reported runtime model; may precede a switch and does not confirm the active call.";
  if (model.source === "Runtime model selection") return "Runtime model selection; active execution is not confirmed.";
  return "Configured model; active execution is not confirmed.";
}

/** Explain applicable counters and missing/invalid fields without a repeated symbol legend. */
export function reportedUsageHelp(entry: SessionUsageEntry): string {
  const usage = entry.telemetry?.lastUsage;
  const input = validTokenCount(usage?.inputTokens);
  const output = validTokenCount(usage?.outputTokens);
  if (input && output) return "Reported input and output counters; no session total or report time is supplied.";
  const unavailable = (value: unknown) => value == null ? "not reported" : "invalid";
  if (input) return `Output count ${unavailable(usage?.outputTokens)}; input is a reported counter, not a session total.`;
  if (output) return `Input count ${unavailable(usage?.inputTokens)}; output is a reported counter, not a session total.`;
  return `Input count ${unavailable(usage?.inputTokens)}; output count ${unavailable(usage?.outputTokens)}.`;
}

/** Exact facts grouped for reading; no source selection, accumulation or freshness inference. */
export function sessionMetadataSections(entry: SessionUsageEntry, stale = false, locale?: string): readonly DetailSection[] {
  const model = modelPresentation(entry);
  const native = entry.telemetry;
  const runtime = native?.runtimeInfo;
  const modelValues: DetailValue[] = [
    { label: "Model ID", value: model.model ?? "Unknown" },
    { label: "Source", value: model.source },
  ];
  if (model.model !== null) modelValues.push({ label: "Active execution", value: model.source === "Last reported runtime model" ? "Not confirmed; the report may precede a model switch" : "Not confirmed" });
  if (model.configured && model.configured !== model.model) modelValues.push({ label: "Configured model", value: model.configured });
  if (model.selection && model.selection !== model.model && model.selection !== model.configured) modelValues.push({ label: "Runtime model selection", value: model.selection });
  modelValues.push({ label: "Provider / route", value: entry.provider });
  const runtimeProvider = supplied(runtime?.provider);
  if (runtimeProvider && runtimeProvider !== entry.provider) modelValues.push({ label: "Runtime provider", value: runtimeProvider });
  for (const [label, value] of [["Effective effort", native?.effectiveThinkingOptionId], ["Configured effort", native?.thinkingOptionId], ["Runtime effort selection", runtime?.thinkingOptionId]] as const) {
    const effort = supplied(value);
    if (effort) modelValues.push({ label, value: effort });
  }
  const usage = native?.lastUsage;
  const values: DetailValue[] = [
    { label: "Input tokens", value: exactTokens(usage?.inputTokens, locale) },
    { label: "Output tokens", value: exactTokens(usage?.outputTokens, locale) },
    { label: "Cache-read tokens", value: exactTokens(usage?.cachedInputTokens, locale) },
  ];
  if ([usage?.inputTokens, usage?.outputTokens, usage?.cachedInputTokens].some(validTokenCount)) values.push(
    { label: "Scope", value: "Reported counters; session total unavailable" },
    { label: "Report time", value: "Unknown" },
    { label: "Model attribution", value: "Unknown" },
  );
  if (stale) values.push({ label: "Inventory", value: "Last known fields; host is stale or offline" });
  return [{ title: "Model", values: modelValues }, { title: "Reported usage", values }];
}

/** Optional explanations apply to this snapshot, rather than every provider and every row. */
export function sessionReportNotes(entry: SessionUsageEntry): readonly string[] {
  const usage = entry.telemetry?.lastUsage;
  if (!usage) return ["Paseo supplied no token report for this session."];
  const hasCounters = [usage.inputTokens, usage.outputTokens, usage.cachedInputTokens].some(validTokenCount);
  if (!hasCounters) return ["This snapshot supplies no valid consumption counters. Context-window values, if present, are not consumption counts."];
  return [
    usageScope(entry),
    "Session Updated time is not the usage observation time. These counters cannot be attributed to the displayed model, including after model or provider switches.",
    "Each report replaces the snapshot. Counts can decrease or reset; no history is reconstructed. Cache reads are never added to the row's input/output counts.",
  ];
}
