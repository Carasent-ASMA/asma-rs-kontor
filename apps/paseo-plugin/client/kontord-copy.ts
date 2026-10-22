import type { KontorStatus } from "../shared/kontor-status";

const OBSERVED: Record<KontorStatus["observation"], string> = {
  ready: "answering",
  unavailable: "not answering",
  refused: "refused the observer read",
  unknown: "unknown",
};

/** Configuration, not proof: agent.create skips an agent that already brings its own Kontor server. */
const COMPOSED: Record<KontorStatus["composition"], string> = {
  composed: "Kontor adds its observer MCP to eligible new agents.",
  "mcp-off": "Kontor does not add its observer MCP: the Kontor MCP is off.",
  "floor-off": "Kontor does not add its observer MCP: the floor is off.",
};

/** What kontor.status observed on this host; null (offline, not yet read, failed read) is unknown. Never "not enabled". */
export function kontordSentence(status: KontorStatus | null): string {
  if (status?.composition === "mcp-off") return "kontord on this host: not checked; the Kontor MCP is off.";
  return `kontord on this host: ${OBSERVED[status?.observation ?? "unknown"]}.`;
}

/** The Kontor screen's one line: the observation, the composition and when it was checked. */
export function kontordLine(status: KontorStatus | null): string {
  if (!status) return kontordSentence(null);
  return `${kontordSentence(status)} ${COMPOSED[status.composition]} Checked ${new Date(status.checkedAt).toLocaleTimeString()}.`;
}
