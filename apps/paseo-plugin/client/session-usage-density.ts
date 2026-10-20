import { modelPresentation, usagePresentation, type SessionUsageEntry } from "./session-usage";

export const SESSION_METADATA_HELP = "Model source: sliders = selected configuration; radio = last reported runtime, which can predate a switch. Neither confirms the active call. Question mark = model not supplied. Token arrows mean reported input / output, not whole-session totals. Dash = not reported; zero = explicit zero. Report age and model attribution are unknown. Open Details for exact values and provider limits.";

/** Presentation only: source selection, validation and exact Details remain in session-usage. */
export function compactUsage(entry: SessionUsageEntry) {
  const model = modelPresentation(entry);
  const usage = usagePresentation(entry);
  return {
    model: model.model === null ? "—" : model.short,
    modelIcon: model.model === null ? "CircleHelp" : model.source === "Last reported runtime model" ? "Radio" : "SlidersHorizontal",
    tokens: usage.label === "Usage not reported" ? "—" : usage.label.replace(/^Report: /, "").replace(/ in · /, " ↓ · ").replace(/ out$/, " ↑"),
    help: `${model.source}: ${model.model ?? "not supplied"}. ${usage.spoken} ${SESSION_METADATA_HELP}`,
  };
}
