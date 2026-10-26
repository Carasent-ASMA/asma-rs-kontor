import type { FixtureSession } from "../fixtures/sessions";
import { logHostEvent } from "./event-log";
import type { AttachOutcome, NavigatorCapabilities, SummaryPin, SummaryRevision, UsageTotals } from "../../../paseo-plugin/shared/session-navigator";

/** What every simulated takeover states up front: its effects stay in this page. */
export const TAKEOVER_SIMULATED = "SIMULATED: no provider, account or Paseo session is contacted; every effect changes page memory only.";
/** Session token totals were never read for any inventory here, so rows keep them unknown unless a scenario supplies SYNTHETIC ones. */
const TOTALS_UNKNOWN = "SIMULATED host: no session token totals were read, so rows show them as unknown.";

/** The newest event a revision can cover: the running turn's start or the last prompt, else the metadata time. */
function cutoffOf(agent: FixtureSession["agent"]): string {
  return [agent.activeTurn?.startedAt, agent.lastUserMessageAt].filter((value): value is string => !!value && Number.isFinite(Date.parse(value)))
    .sort().at(-1) ?? agent.updatedAt;
}

/** A revision as stored: frozen through its sections, so neither a caller nor a later save can change it. */
const frozen = (revision: SummaryRevision): SummaryRevision => Object.freeze({ ...revision, sections: Object.freeze({ ...revision.sections }) });

/** What a destination's draft holds after Attach, and what a created seat was pinned to; both inspectable, neither sent. */
export type SummaryPlacement = { readonly summaryId: string; readonly revision: number; readonly sourceAgentId: string; readonly text: string; readonly at: string };

/**
 * Page-memory simulation of saved summaries, draft attach and creation pins. A summary is written from the session's
 * title and status only: no transcript is read, and every line says SIMULATED. Saving again without a newer source
 * event returns the same revision; a newer event makes the next one, and no revision changes after it is saved.
 * Attach and pinning resolve the exact revision first and refuse an unknown or mismatched one before any effect.
 * Takeover is simulated in full by `session-takeover.ts`, so every takeover capability is supported here, stating the simulation as its limit. `totals`, when
 * given, is a SYNTHETIC whole-session table; a session it does not list reads null.
 */
export function fixtureNavigator(get: () => FixtureSession[], totals: Readonly<Record<string, UsageTotals>> | null = null) {
  const saved = new Map<string, SummaryRevision[]>();
  const drafts = new Map<string, SummaryPlacement>(), pins = new Map<string, SummaryPlacement>();
  const capabilities: NavigatorCapabilities = {
    usageTotals: totals ? { supported: true } : { supported: false, reason: TOTALS_UNKNOWN },
    summaries: { supported: true }, draftAttach: { supported: true },
    takeover: { supported: true, limits: [TAKEOVER_SIMULATED] }, takeoverBulk: { supported: true }, takeoverRoute: { supported: true },
  };
  const find = (id: string) => {
    const row = get().find(({ agent }) => agent.id === id);
    if (!row) throw new Error(`SIMULATED: no session ${id} on this host.`);
    return row;
  };
  const latest = (id: string) => saved.get(id)?.at(-1) ?? null;
  function save(id: string): SummaryRevision {
    const { agent } = find(id), cutoff = cutoffOf(agent), last = latest(id);
    if (last?.cutoff === cutoff) return last;
    const sections = {
      task: `SIMULATED summary of “${agent.title}”. No transcript was read.`,
      decisions: "SIMULATED: none recorded.",
      currentState: `SIMULATED: native status ${agent.status}.`,
      nextStep: "SIMULATED: review the source session before relying on this.",
    };
    const revision = frozen({ summaryId: last?.summaryId ?? crypto.randomUUID(), revision: (last?.revision ?? 0) + 1, sourceAgentId: id, sourceTitle: agent.title,
      cutoff, savedAt: new Date().toISOString(), sections,
      text: [`Task: ${sections.task}`, `Decisions: ${sections.decisions}`, `Current state: ${sections.currentState}`, `Next step: ${sections.nextStep}`].join("\n") });
    saved.set(id, [...saved.get(id) ?? [], revision]);
    logHostEvent("simulated.summary", `${id}: saved r${revision.revision} in page memory`);
    return revision;
  }
  /** The exact saved revision; an unknown one, or one saved from another session than `source`, is refused. */
  function resolve(pin: SummaryPin, source: string | null = null): SummaryRevision {
    const found = [...saved.values()].flat().find((entry) => entry.summaryId === pin.summaryId && entry.revision === pin.revision);
    if (!found) throw new Error(`SIMULATED: no saved summary ${pin.summaryId} r${pin.revision}; nothing was placed.`);
    if (source !== null && found.sourceAgentId !== source) throw new Error(`SIMULATED: saved summary r${pin.revision} belongs to ${found.sourceAgentId}, not ${source}; nothing was placed.`);
    return found;
  }
  const placement = (revision: SummaryRevision): SummaryPlacement => Object.freeze({ summaryId: revision.summaryId, revision: revision.revision,
    sourceAgentId: revision.sourceAgentId, text: revision.text, at: new Date().toISOString() });
  async function invoke(method: string, input: unknown): Promise<unknown> {
    if (method === "sessions.navigator-capabilities") return structuredClone(capabilities);
    if (method === "sessions.usage-totals") {
      if (!totals) throw new Error(TOTALS_UNKNOWN);
      return Object.fromEntries((input as { agentIds: string[] }).agentIds.map((id) => [id, structuredClone(totals[id] ?? null)]));
    }
    if (method === "sessions.summary-latest") return Object.fromEntries((input as { agentIds: string[] }).agentIds.map((id) => [id, structuredClone(latest(id))]));
    if (method === "sessions.summary-save") return structuredClone(save((input as { agentId: string }).agentId));
    if (method === "sessions.summary-read") return structuredClone(resolve(input as SummaryPin));
    if (method === "sessions.summary-attach") {
      const request = input as SummaryPin & { destinationAgentId: string };
      const revision = resolve(request), destination = find(request.destinationAgentId);
      const refused = (message: string): AttachOutcome => ({ state: "refused", message, destinationAgentId: request.destinationAgentId, summaryId: request.summaryId, revision: request.revision });
      if (destination.agent.id === revision.sourceAgentId) return refused("SIMULATED: a summary goes to another seat, not back to its own source. Nothing was drafted.");
      if (destination.agent.archivedAt) return refused(`SIMULATED: “${destination.agent.title}” is archived, so it has no draft. Nothing was drafted.`);
      // A draft is its operator's text: Attach never replaces or appends to one, and the same revision twice is placed once.
      const draft = drafts.get(destination.agent.id);
      if (draft?.summaryId === revision.summaryId && draft.revision === revision.revision) return { state: "drafted", message: `SIMULATED: r${revision.revision} is already in the draft of “${destination.agent.title}”; nothing changed and nothing was sent.`,
        destinationAgentId: destination.agent.id, summaryId: revision.summaryId, revision: revision.revision } satisfies AttachOutcome;
      if (draft) return refused(`SIMULATED: “${destination.agent.title}” already has a draft (r${draft.revision} of ${draft.sourceAgentId}). Attach never replaces a draft; send or clear it there first. Nothing was drafted.`);
      drafts.set(destination.agent.id, placement(revision));
      logHostEvent("simulated.attach", `r${revision.revision} into ${destination.agent.id}'s draft; nothing sent`);
      return { state: "drafted", message: `SIMULATED: r${revision.revision} is in the draft of “${destination.agent.title}” for review. Nothing was sent and nobody else owns it.`,
        destinationAgentId: destination.agent.id, summaryId: revision.summaryId, revision: revision.revision } satisfies AttachOutcome;
    }
    throw new Error(`Unsupported fixture navigator RPC ${method}`);
  }
  return {
    invoke, resolve,
    /** Records the revision a created seat reads before its first task; resolve it first, so this cannot refuse. */
    pin(agentId: string, revision: SummaryRevision) { pins.set(agentId, placement(revision)); logHostEvent("simulated.summary", `${agentId}: pinned r${revision.revision} of ${revision.sourceAgentId}`); },
    pinned: (agentId: string) => pins.get(agentId) ?? null,
    /** The drafts and pins in page memory, for review and tests. */
    inspect: () => structuredClone({ drafts: Object.fromEntries(drafts), pins: Object.fromEntries(pins) }),
  };
}
