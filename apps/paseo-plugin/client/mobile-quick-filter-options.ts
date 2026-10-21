import { QUICK_STATUS_FILTERS, statusFilterLabel, type StatusFilter } from "./session-view";

/** Shapes carry the meaning even when status colours cannot be distinguished. */
export const QUICK_FILTER_ICONS: Record<StatusFilter, string> = {
  all: "LayoutGrid", running: "Play", idle: "Pause", error: "CircleAlert",
  needs_input: "MessageCircleQuestion", attention: "Bell", favorites: "Star",
  initializing: "Loader", closed: "Square", earlier_error: "History",
};

export const QUICK_FILTER_MEANINGS: Record<StatusFilter, string> = {
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
};

export function quickFilterLabel(status: StatusFilter): string {
  return statusFilterLabel(status).replace("★ ", "");
}

/** Three utility targets (Search, More, Filters) keep their own space; never shrink touch targets. */
export function visibleQuickFilters(width: number, selected: StatusFilter, target = 44): readonly StatusFilter[] {
  const slots = Math.max(1, Math.min(7, Math.floor((width + 4) / (target + 4)) - 3));
  const visible = QUICK_STATUS_FILTERS.slice(0, slots);
  if (!visible.includes(selected)) visible[visible.length - 1] = selected;
  return visible;
}

export function quickFilterHelp(status: StatusFilter, count: number): string {
  return `${quickFilterLabel(status)} · ${count} sessions. ${QUICK_FILTER_MEANINGS[status]}`;
}
