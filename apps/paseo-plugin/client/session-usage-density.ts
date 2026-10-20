import { modelPresentation, usagePresentation, type SessionUsageEntry } from "./session-usage";

export const SESSION_METADATA_HELP = "Model source: sliders = selected configuration; radio = last reported runtime, which can predate a switch. Neither confirms the active call. Question mark = model not supplied. Token arrows mean reported input / output, not whole-session totals. Dash = not reported; zero = explicit zero. Report age and model attribution are unknown. Open Details for exact values and provider limits.";

/** Presentation only: source selection, validation and exact Details remain in session-usage. */
export function compactUsage(entry: SessionUsageEntry) {
  const model = modelPresentation(entry);
  const usage = usagePresentation(entry);
  const sourceHelp = model.model === null ? "Model not supplied; custom IDs are shown verbatim when supplied."
    : model.source === "Last reported runtime model" ? "Last reported runtime model; may predate a switch and does not confirm the active call."
    : "Selected model configuration; runtime execution is not established.";
  return {
    model: model.model === null ? "—" : model.short,
    modelIcon: model.model === null ? "CircleHelp" : model.source === "Last reported runtime model" ? "Radio" : "SlidersHorizontal",
    tokens: usage.label === "Usage not reported" ? "—" : usage.label.replace(/^Report: /, "").replace(/ in · /, " ↓ · ").replace(/ out$/, " ↑"),
    help: `${sourceHelp} Arrows: reported input / output counters, not whole-session totals. Dash = not reported; zero = explicit zero. Report age and model attribution are unknown. Open Details for exact model IDs, counts and provider limits.`,
  };
}
