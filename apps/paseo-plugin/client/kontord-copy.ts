import type { KontorStatus } from "../shared/kontor-status";

const OBSERVED: Record<KontorStatus["observation"], string> = {
  ready: "answering",
  unavailable: "not answering",
  refused: "refused the observer read",
  unknown: "unknown",
};

/** What kontor.status observed on this host; null (offline, not yet read, failed read) is unknown. Never "not enabled". */
export function kontordSentence(status: KontorStatus | null): string {
  if (status?.composition === "mcp-off") return "kontord on this host: not checked; the Kontor MCP is off.";
  return `kontord on this host: ${OBSERVED[status?.observation ?? "unknown"]}.`;
}

const TITLE: Record<KontorStatus["observation"], string> = {
  ready: "Kontor connected",
  unavailable: "Kontor not answering",
  refused: "Kontor refused the status check",
  unknown: "Kontor status unknown",
};

const OBSERVATION_DETAIL: Record<KontorStatus["observation"], string> = {
  ready: "",
  unavailable: "This host asked Kontor for its status and got no usable answer. ",
  refused: "Kontor answered but refused this host's read-only status check. ",
  unknown: "The last check could not confirm Kontor's status. ",
};

const COMPOSITION_DETAIL: Record<KontorStatus["composition"], string> = {
  composed: "Automatic agent setup and workspace checks are on, and eligible new agents get Kontor's read-only tools.",
  "mcp-off": "The Kontor MCP is off, so this host does not check Kontor and new agents do not get its tools. Automatic agent setup and workspace checks are still on.",
  "floor-off": "You can browse sessions and project information. Automatic agent setup and workspace checks are turned off.",
};

export type KontordBannerCopy = { title: string; checked: string | null; details: string };

/**
 * The Kontor screen's status banner. "connected" only for a ready observation, "Navigation-only mode" only
 * when it is also floor-off; mcp-off asks nothing, so it has no check time. Null has no time and no mode.
 */
export function kontordBanner(status: KontorStatus | null): KontordBannerCopy {
  if (!status) return { title: TITLE.unknown, checked: null, details: "This host has not reported Kontor's status: it is offline, not checked yet, or the check failed." };
  const checked = `Last checked ${new Date(status.checkedAt).toLocaleTimeString()}.`;
  const details = COMPOSITION_DETAIL[status.composition];
  if (status.composition === "mcp-off") return { title: "Kontor not checked", checked: null, details };
  const mode = status.observation === "ready" && status.composition === "floor-off" ? " · Navigation-only mode" : "";
  return { title: TITLE[status.observation] + mode, checked, details: OBSERVATION_DETAIL[status.observation] + details };
}
