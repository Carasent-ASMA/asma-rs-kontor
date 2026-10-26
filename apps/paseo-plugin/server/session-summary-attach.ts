import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, readFile, rename } from "node:fs/promises";
import { dirname, join } from "node:path";
import { z } from "zod";
import type { PaseoAgentListResult, PaseoApi } from "@getpaseo/client";
import { UNSUPPORTED, attachOutcomeSchema, type AttachOutcome, type Capability, type SummaryPin, type SummaryRevision } from "../shared/session-navigator";
import { readConfig } from "./config";
import { paseoSummaryGenerator, readPinnedSummary, withSourceStoreLock } from "./session-summaries";

type Agent = PaseoAgentListResult["entries"][number]["agent"];

/** Baseline gap when the host SDK exposes no draft-placement route (see shared UNSUPPORTED.draftAttach). */
export const DRAFT_PLACEMENT_API_GAP = UNSUPPORTED.draftAttach.reason;

/** Exact gap when placement exists without a supported host draft read for verify/reconcile. */
export const DRAFT_READ_API_GAP =
  "The host SDK exposes placeSummaryDraft but not readSummaryDraft; Kontor cannot verify draft placement or reconcile attach intents on this host.";

const ATTACH_STORE_LIMIT =
  "Attach intents and outcomes persist under the Kontor plugin host-backed state root (plugin-state/kontor/session-summary-attach-v1); this plugin is the sole writer for those records on this host.";

export const MALFORMED_HOST_DRAFT_READ =
  "The host readSummaryDraft result was malformed; Kontor cannot treat it as empty or occupied. Nothing was placed.";

const hostSummaryDraftReadSchema = z.object({ summaryId: z.string().uuid(), revision: z.number().int().positive() }).strict();
export type HostSummaryDraftPin = z.infer<typeof hostSummaryDraftReadSchema>;

/** Only host `null` means no draft; a valid pin means occupied; every other value is malformed. */
export function parseHostSummaryDraftRead(raw: unknown): { state: "empty" } | { state: "occupied"; draft: HostSummaryDraftPin } | { state: "malformed" } {
  if (raw === null) return { state: "empty" };
  if (raw === undefined || raw === "" || raw === false) return { state: "malformed" };
  const parsed = hostSummaryDraftReadSchema.safeParse(raw);
  return parsed.success ? { state: "occupied", draft: parsed.data } : { state: "malformed" };
}

export const DRAFT_ATTACH_LIMITS = [
  "Attach places the saved revision in the destination seat's draft through the host SDK placeSummaryDraft route; nothing is sent.",
  "Draft occupancy and retry reconciliation are read only through readSummaryDraft on the host; only a host null means empty and a valid summaryId/revision means occupied.",
  "Attach refuses destinations that are running, initializing, or in an active turn until the seat is idle.",
  "Each destination keeps at most one host draft; Attach never replaces an existing draft.",
  ATTACH_STORE_LIMIT,
  "Attach intent serialization is in-process per worker on this host, like the summary store; multi-process workers are not qualified without a shared lock.",
];

export type SummaryDraftPlacer = {
  placeDraft(destinationAgentId: string, revision: SummaryRevision): Promise<void>;
  readDraft(destinationAgentId: string): Promise<{ summaryId: string; revision: number } | null>;
};

/** Qualified only when both placeSummaryDraft and readSummaryDraft exist on the SDK. */
export function paseoSummaryDraftPlacer(paseo: PaseoApi): SummaryDraftPlacer | undefined {
  const host = paseo as {
    agents?: typeof paseo.agents & {
      placeSummaryDraft?(destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }): Promise<void>;
      readSummaryDraft?(destinationAgentId: string): Promise<{ summaryId: string; revision: number } | null>;
    };
  };
  if (typeof host.agents?.placeSummaryDraft !== "function" || typeof host.agents?.readSummaryDraft !== "function") return undefined;
  const place = host.agents.placeSummaryDraft.bind(host.agents);
  const readDraft = host.agents.readSummaryDraft.bind(host.agents);
  return {
    placeDraft: (destinationAgentId, revision) => place(destinationAgentId, { summaryId: revision.summaryId, revision: revision.revision, text: revision.text, sourceAgentId: revision.sourceAgentId }),
    readDraft: async (destinationAgentId) => {
      const parsed = parseHostSummaryDraftRead(await readDraft(destinationAgentId));
      if (parsed.state === "malformed") throw new Error(MALFORMED_HOST_DRAFT_READ);
      return parsed.state === "empty" ? null : parsed.draft;
    },
  };
}

export function draftAttachInstalledCapability(paseo: PaseoApi): Capability {
  if (!paseoSummaryGenerator(paseo)) return { supported: false, reason: "This host has no saved summary store; Kontor cannot attach a summary revision here." };
  const host = paseo as { agents?: { placeSummaryDraft?: unknown; readSummaryDraft?: unknown } };
  if (typeof host.agents?.placeSummaryDraft !== "function") return { supported: false, reason: DRAFT_PLACEMENT_API_GAP };
  if (typeof host.agents?.readSummaryDraft !== "function") return { supported: false, reason: DRAFT_READ_API_GAP };
  return { supported: true, limits: DRAFT_ATTACH_LIMITS };
}

const attachIntentSchema = z.object({
  intentId: z.string().uuid(),
  placementKey: z.string().min(1),
  summaryId: z.string().uuid(),
  revision: z.number().int().positive(),
  destinationAgentId: z.string().min(1),
  requestedAt: z.string(),
}).strict();
type AttachIntent = z.infer<typeof attachIntentSchema>;

const destinationAttachSchema = z.object({
  placements: z.record(z.string(), attachOutcomeSchema),
  intents: z.record(z.string(), attachIntentSchema),
}).strict();
type DestinationAttachState = z.infer<typeof destinationAttachSchema>;

const destinationPath = (root: string, destinationAgentId: string) => join(root, `dest-${createHash("sha256").update(destinationAgentId).digest("hex")}.json`);

function placementKey(pin: SummaryPin, destinationAgentId: string) {
  return `${pin.summaryId}:${pin.revision}:${destinationAgentId}`;
}

function pinMatches(draft: { summaryId: string; revision: number } | null, pin: SummaryPin) {
  return !!draft && draft.summaryId === pin.summaryId && draft.revision === pin.revision;
}

export async function readDestinationAttachState(root: string, destinationAgentId: string): Promise<DestinationAttachState> {
  try {
    return destinationAttachSchema.parse(JSON.parse(await readFile(destinationPath(root, destinationAgentId), "utf8")));
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return { placements: {}, intents: {} };
    throw error;
  }
}

async function writeDestinationAttachState(root: string, destinationAgentId: string, state: DestinationAttachState) {
  await mkdir(root, { recursive: true, mode: 0o700 });
  const file = destinationPath(root, destinationAgentId), temporary = `${file}.${randomUUID()}.tmp`;
  const output = await open(temporary, "wx", 0o600);
  try { await output.writeFile(JSON.stringify(destinationAttachSchema.parse(state))); await output.sync(); } finally { await output.close(); }
  await rename(temporary, file);
}

export function attachStateRoot() {
  return join(dirname(readConfig().paseoWorktreesRoot), "plugin-state", "kontor", "session-summary-attach-v1");
}

function draftedOutcome(revision: SummaryRevision, destination: Agent, message: string): AttachOutcome {
  return { state: "drafted", message, destinationAgentId: destination.id, summaryId: revision.summaryId, revision: revision.revision };
}

function refusedOutcome(pin: SummaryPin, destinationAgentId: string, message: string): AttachOutcome {
  return { state: "refused", message, destinationAgentId, summaryId: pin.summaryId, revision: pin.revision };
}

function unknownOutcome(pin: SummaryPin, destinationAgentId: string, message: string): AttachOutcome {
  return { state: "unknown", message, destinationAgentId, summaryId: pin.summaryId, revision: pin.revision };
}

function destinationBusy(agent: Agent): string | null {
  if (agent.status === "running" || agent.status === "initializing") return `“${agent.title ?? agent.id}” is ${agent.status}; Attach waits until the seat is idle. Nothing was drafted.`;
  if (agent.activeTurn) return `“${agent.title ?? agent.id}” has an active turn; Attach waits until it finishes. Nothing was drafted.`;
  return null;
}

function destinationAttachRefusal(agent: Agent | null, pin: SummaryPin, revision: SummaryRevision, destinationAgentId: string): AttachOutcome | null {
  if (!agent) return refusedOutcome(pin, destinationAgentId, `No session ${destinationAgentId} is on this host; nothing was placed.`);
  if (agent.archivedAt) return refusedOutcome(pin, agent.id, `“${agent.title ?? agent.id}” is archived, so it has no draft. Nothing was drafted.`);
  if (agent.id === revision.sourceAgentId) return refusedOutcome(pin, agent.id, "A summary goes to another seat, not back to its own source. Nothing was drafted.");
  const busy = destinationBusy(agent);
  if (busy) return refusedOutcome(pin, agent.id, busy);
  return null;
}

export function createSummaryAttachService(paseo: PaseoApi, summaryRoot: string, attachRoot: string) {
  async function exact(agentId: string): Promise<Agent | null> {
    const found = await paseo.agents.ref(agentId).refresh();
    return found?.agent.id === agentId ? found.agent : null;
  }

  async function attach(pin: SummaryPin & { destinationAgentId: string }): Promise<AttachOutcome> {
    const placer = paseoSummaryDraftPlacer(paseo);
    if (!placer) {
      const host = paseo as { agents?: { placeSummaryDraft?: unknown; readSummaryDraft?: unknown } };
      if (typeof host.agents?.placeSummaryDraft !== "function") throw new Error(DRAFT_PLACEMENT_API_GAP);
      throw new Error(DRAFT_READ_API_GAP);
    }
    if (!paseoSummaryGenerator(paseo)) throw new Error("This host has no saved summary store; Kontor cannot attach a summary revision here.");

    let revision: SummaryRevision;
    try { revision = await readPinnedSummary(summaryRoot, pin); }
    catch { return refusedOutcome(pin, pin.destinationAgentId, `No saved summary ${pin.summaryId} r${pin.revision} is stored on this host; nothing was placed.`); }

    if (pin.destinationAgentId === revision.sourceAgentId) return refusedOutcome(pin, pin.destinationAgentId, "A summary goes to another seat, not back to its own source. Nothing was drafted.");

    return withSourceStoreLock(attachRoot, pin.destinationAgentId, async () => {
      const key = placementKey(pin, pin.destinationAgentId);
      let state = await readDestinationAttachState(attachRoot, pin.destinationAgentId);
      const prior = state.placements[key];
      if (prior) return prior;

      let destination = await exact(pin.destinationAgentId);
      let refusal = destinationAttachRefusal(destination, pin, revision, pin.destinationAgentId);
      if (refusal) return refusal;

      let hostDraft: { summaryId: string; revision: number } | null;
      try { hostDraft = await placer.readDraft(pin.destinationAgentId); }
      catch (error) {
        const message = error instanceof Error && error.message === MALFORMED_HOST_DRAFT_READ
          ? `${MALFORMED_HOST_DRAFT_READ} Check the destination's draft on the host.`
          : `The host draft could not be read (${error instanceof Error ? error.message : String(error)}). Nothing was placed.`;
        return unknownOutcome(pin, destination!.id, message);
      }

      if (hostDraft && !pinMatches(hostDraft, pin)) {
        return refusedOutcome(pin, destination!.id, `“${destination!.title ?? destination!.id}” already has a draft (r${hostDraft.revision} of another summary). Attach never replaces a draft; send or clear it there first. Nothing was drafted.`);
      }
      if (pinMatches(hostDraft, pin)) {
        const outcome = draftedOutcome(revision, destination!, `r${revision.revision} is already in the draft of “${destination!.title ?? destination!.id}”; nothing changed and nothing was sent.`);
        state.placements[key] = outcome;
        delete state.intents[key];
        await writeDestinationAttachState(attachRoot, pin.destinationAgentId, state);
        return outcome;
      }

      const pending = state.intents[key];
      if (pending) {
        return unknownOutcome(pin, destination!.id, "An attach intent is recorded but the host draft does not show this summary revision yet; Kontor will not place it again. Check the destination's draft before retrying.");
      }

      destination = await exact(pin.destinationAgentId);
      refusal = destinationAttachRefusal(destination, pin, revision, pin.destinationAgentId);
      if (refusal) return refusal;

      const intent: AttachIntent = { intentId: randomUUID(), placementKey: key, summaryId: pin.summaryId, revision: pin.revision, destinationAgentId: pin.destinationAgentId, requestedAt: new Date().toISOString() };
      state = { ...state, intents: { ...state.intents, [key]: intent } };
      await writeDestinationAttachState(attachRoot, pin.destinationAgentId, state);

      destination = await exact(pin.destinationAgentId);
      refusal = destinationAttachRefusal(destination, pin, revision, pin.destinationAgentId);
      if (refusal) {
        state = await readDestinationAttachState(attachRoot, pin.destinationAgentId);
        delete state.intents[key];
        await writeDestinationAttachState(attachRoot, pin.destinationAgentId, state);
        return refusal;
      }

      try { await placer.placeDraft(pin.destinationAgentId, revision); }
      catch (error) {
        return unknownOutcome(pin, destination!.id, `The host did not confirm draft placement (${error instanceof Error ? error.message : String(error)}). The attach intent stays recorded; Kontor will not place it again until the host draft read shows this revision.`);
      }

      try { hostDraft = await placer.readDraft(pin.destinationAgentId); }
      catch (error) {
        const message = error instanceof Error && error.message === MALFORMED_HOST_DRAFT_READ
          ? `Draft placement was sent but ${MALFORMED_HOST_DRAFT_READ} Kontor will not record success; check the destination's draft.`
          : `Draft placement was sent but the host draft could not be read back (${error instanceof Error ? error.message : String(error)}). Check the destination's draft; Kontor will not place it again automatically.`;
        return unknownOutcome(pin, destination!.id, message);
      }
      if (!pinMatches(hostDraft, pin)) {
        return unknownOutcome(pin, destination!.id, "Draft placement was sent but the host draft does not show this summary revision yet. Check the destination's draft; Kontor will not place it again automatically.");
      }

      const outcome = draftedOutcome(revision, destination!, `r${revision.revision} is in the draft of “${destination!.title ?? destination!.id}” for review. Nothing was sent and nobody else owns it.`);
      state = await readDestinationAttachState(attachRoot, pin.destinationAgentId);
      state.placements[key] = outcome;
      delete state.intents[key];
      await writeDestinationAttachState(attachRoot, pin.destinationAgentId, state);
      return outcome;
    });
  }

  return { attach };
}
