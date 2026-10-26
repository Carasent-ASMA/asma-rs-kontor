import { QUICK_STATUS_FILTERS, statusFilterLabel, type StatusFilter } from "./session-view";

/** Shapes carry the meaning even when status colours cannot be distinguished. */
export const QUICK_FILTER_ICONS: Record<StatusFilter, string> = {
  recent: "Clock", activity: "Activity", current: "Activity", all: "LayoutGrid", running: "Play", idle: "Pause", error: "CircleAlert",
  needs_input: "MessageCircleQuestion", attention: "Bell", favorites: "Star",
  initializing: "Loader", closed: "Square", earlier_error: "History", provider_blocked: "Ban",
};

export const QUICK_FILTER_MEANINGS: Record<StatusFilter, string> = {
  recent: "Sessions you opened from Kontor on this host and device, most recent first, whatever their state. It keeps the last 500. Native sidebar opens and other devices are not included.",
  activity: "Sessions with a recorded prompt or running turn, newest first. Renames, metadata updates and Mark as unread never count; Paseo records no time for a turn's end.",
  current: "Native-state visibility: running, starting, idle and input-waiting sessions. Provider history is not verified. All includes closed and error sessions; unknown availability stays visible.",
  all: "All sessions in the current search and scope.",
  running: "Sessions currently running.",
  idle: "Sessions currently idle.",
  error: "Sessions currently in error. Earlier errors are separate.",
  needs_input: "Sessions waiting for input or permission.",
  attention: "Sessions currently marked for attention by Paseo, including finished notices. May overlap another state.",
  favorites: "Sessions you have starred in this host.",
  initializing: "Sessions currently starting.",
  closed: "Closed sessions in the current scope.",
  earlier_error: "Sessions with a retained error that are no longer in error.",
  provider_blocked: "Error sessions whose exact read in the last minute saw a provider refusal. Sessions not checked are counted, not listed. Not a provider health check.",
};

export function quickFilterLabel(status: StatusFilter): string {
  return statusFilterLabel(status).replace("★ ", "");
}

/**
 * The shortcuts a row of `width` shows as whole tiles, in order, after More and the trailing action keep their own two
 * targets; a target never shrinks to fit, so a row without room for one whole tile shows none and More reaches them all.
 * A chosen shortcut past them takes the last tile; a state from More or Filters never becomes a shortcut.
 */
export function visibleQuickFilters(width: number, selected: StatusFilter, target: number, gap = 4): StatusFilter[] {
  const slots = Math.max(0, Math.min(QUICK_STATUS_FILTERS.length, Math.floor((width + gap) / (target + gap)) - 2));
  const visible = QUICK_STATUS_FILTERS.slice(0, slots);
  if (slots && QUICK_STATUS_FILTERS.includes(selected) && !visible.includes(selected)) visible[slots - 1] = selected;
  return visible;
}

export function quickFilterHelp(status: StatusFilter, count: number): string {
  return `${quickFilterLabel(status)} · ${count} sessions. ${QUICK_FILTER_MEANINGS[status]}`;
}
