import { modelPresentation, usagePresentation, type SessionUsageEntry } from "./session-usage";
import { modelSourceHelp, reportedUsageHelp } from "./session-metadata-content";

/** Presentation only: source selection, validation and exact Details remain in session-usage. */
export function compactUsage(entry: SessionUsageEntry) {
  const model = modelPresentation(entry);
  const usage = usagePresentation(entry);
  return {
    model: model.model === null ? "—" : model.short,
    modelIcon: model.model === null ? "CircleHelp" : model.source === "Last reported runtime model" ? "Radio" : "SlidersHorizontal",
    tokens: usage.label === "Usage not reported" ? "—" : usage.label.replace(/^Report: /, "").replace(/ in · /, " ↓ · ").replace(/ out$/, " ↑"),
    modelHelp: modelSourceHelp(entry),
    usageHelp: reportedUsageHelp(entry),
  };
}
