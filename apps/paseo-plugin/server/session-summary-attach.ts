import { createHash, randomUUID } from "node:crypto";
import { mkdir, open, readFile, rename } from "node:fs/promises";
import { dirname, join } from "node:path";
import { z } from "zod";
import type { PaseoAgentListResult, PaseoApi } from "@getpaseo/client";
import { attachOutcomeSchema, type AttachOutcome, type Capability, type SummaryPin, type SummaryRevision } from "../shared/session-navigator";
import { readConfig } from "./config";
import { paseoSummaryGenerator, readPinnedSummary, withSourceStoreLock } from "./session-summaries";

type Agent = PaseoAgentListResult["entries"][number]["agent"];

/** Attach requires the accepted atomic helper and the connected host's summary-draft capability. */
export const DRAFT_ATTACH_API_GAP =
  "This host/client does not support atomic summary-draft attachment (agents.attachSummaryDraft with agentSummaryDraft); nothing can be attached here.";

/** Exact gap when placement exists without a supported host draft read for verify/reconcile. */
export const DRAFT_READ_API_GAP =
  "The host SDK exposes attachSummaryDraft but not readSummaryDraft; Kontor cannot verify draft placement or reconcile attach intents on this host.";

const ATTACH_STORE_LIMIT =
  "Attach intents and outcomes persist under the Kontor plugin host-backed state root (plugin-state/kontor/session-summary-attach-v1); this plugin is the sole writer for those records on this host.";

/** Thrown by the placer when the host reader returns a non-null non-pin value. */
export class MalformedHostDraftReadError extends Error {
  constructor() { super("MalformedHostDraftReadError"); }
}

export const MALFORMED_HOST_DRAFT_READ_PRE =
  "The host readSummaryDraft result was malformed; Kontor cannot treat it as empty or occupied. Nothing was placed.";

export const MALFORMED_HOST_DRAFT_READ_POST =
  "Draft placement was sent, but the host readSummaryDraft result was malformed afterward; placement may have occurred on the host. The attach intent stays recorded and Kontor will not place again. Check the destination's draft.";

/** @deprecated Use {@link MALFORMED_HOST_DRAFT_READ_PRE} for pre-place messaging. */
export const MALFORMED_HOST_DRAFT_READ = MALFORMED_HOST_DRAFT_READ_PRE;

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
  "Attach uses the host SDK agents.attachSummaryDraft route for the exact saved revision; nothing is sent.",
  "Draft occupancy and retry reconciliation are read only through readSummaryDraft on the host; only a host null means empty and a valid summaryId/revision means occupied.",
  "Attach refuses destinations that are running, initializing, or in an active turn until the seat is idle.",
  "Each destination keeps at most one host draft; Attach never replaces an existing draft.",
  "The host performs atomic no-replace attachment in summary-drafts-v1; existing tasks and drafts are never replaced. Native/composer installation still needs matched-release readback.",
  ATTACH_STORE_LIMIT,
  "Attach intent serialization is in-process per worker on this host, like the summary store; multi-process workers are not qualified without a shared lock.",
];

const hostSummaryDraftAttachSchema = z.object({
  state: z.enum(["drafted", "already", "refused"]), draft: hostSummaryDraftReadSchema.nullable(),
}).strict();
export type HostSummaryDraftAttachResult = z.infer<typeof hostSummaryDraftAttachSchema>;
export type SummaryDraftAttacher = {
  attachDraft(destinationAgentId: string, revision: SummaryRevision): Promise<HostSummaryDraftAttachResult>;
  readDraft(destinationAgentId: string): Promise<HostSummaryDraftPin | null>;
};

function atomicAttachSupported(paseo: PaseoApi): boolean {
  const host = paseo as { supportsHostFeature?(feature: "agentSummaryDraft"): boolean; agents?: { attachSummaryDraft?: unknown } };
  return host.supportsHostFeature?.("agentSummaryDraft") === true && typeof host.agents?.attachSummaryDraft === "function";
}

/** No legacy place route: the host alone owns atomic no-replace attachment. */
export function paseoSummaryDraftAttacher(paseo: PaseoApi): SummaryDraftAttacher | undefined {
  const host = paseo as { agents?: {
    attachSummaryDraft?(destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }): Promise<HostSummaryDraftAttachResult>;
    readSummaryDraft?(destinationAgentId: string): Promise<HostSummaryDraftPin | null>;
  } };
  if (!atomicAttachSupported(paseo) || typeof host.agents?.readSummaryDraft !== "function") return undefined;
  const attach = host.agents.attachSummaryDraft!.bind(host.agents);
  const readDraft = host.agents.readSummaryDraft.bind(host.agents);
  return {
    attachDraft: async (destinationAgentId, revision) => hostSummaryDraftAttachSchema.parse(await attach(destinationAgentId, { summaryId: revision.summaryId, revision: revision.revision, text: revision.text, sourceAgentId: revision.sourceAgentId })),
    readDraft: async (destinationAgentId) => {
      const parsed = parseHostSummaryDraftRead(await readDraft(destinationAgentId));
      if (parsed.state === "malformed") throw new MalformedHostDraftReadError();
      return parsed.state === "empty" ? null : parsed.draft;
    },
  };
}

export function draftAttachInstalledCapability(paseo: PaseoApi): Capability {
  if (!paseoSummaryGenerator(paseo)) return { supported: false, reason: "This host has no saved summary store; Kontor cannot attach a summary revision here." };
  if (!atomicAttachSupported(paseo)) return { supported: false, reason: DRAFT_ATTACH_API_GAP };
  if (!paseoSummaryDraftAttacher(paseo)) return { supported: false, reason: DRAFT_READ_API_GAP };
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

function draftedOutcome(revision: SummaryPin, destination: Pick<Agent, "id">, message: string): AttachOutcome {
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
    const placer = paseoSummaryDraftAttacher(paseo);
    if (!placer) {
      if (!atomicAttachSupported(paseo)) throw new Error(DRAFT_ATTACH_API_GAP);
      throw new Error(DRAFT_READ_API_GAP);
    }
    if (!paseoSummaryGenerator(paseo)) throw new Error("This host has no saved summary store; Kontor cannot attach a summary revision here.");

    return withSourceStoreLock(attachRoot, pin.destinationAgentId, async () => {
      const key = placementKey(pin, pin.destinationAgentId);
      let state = await readDestinationAttachState(attachRoot, pin.destinationAgentId);
      const prior = state.placements[key];

      const failText = (error: unknown) => error instanceof Error ? error.message : String(error);
      async function abandonIntentAfterPrePlaceReadFailure(message: string, agentId: string): Promise<AttachOutcome> {
        try {
          const latest = await readDestinationAttachState(attachRoot, pin.destinationAgentId);
          delete latest.intents[key];
          await writeDestinationAttachState(attachRoot, pin.destinationAgentId, latest);
          return unknownOutcome(pin, agentId, message);
        } catch (error) {
          return unknownOutcome(pin, agentId, `${message} Kontor could not clear the attach intent on the host (${failText(error)}); the outcome is uncertain. Check the destination's draft before retrying.`);
        }
      }

      let hostDraft: { summaryId: string; revision: number } | null;
      const pending = state.intents[key];
      if (pending || prior) {
        // Reconcile durable history before new-action validation; a missing session is not proof of no placement.
        const recordedPin = `${pin.summaryId} r${pin.revision}`;
        try { hostDraft = await placer.readDraft(pin.destinationAgentId); }
        catch {
          return unknownOutcome(pin, pin.destinationAgentId, `An attach intent or outcome is recorded for ${recordedPin}, but host draft readback is unavailable or malformed. Placement may already exist; Kontor will not attach again.`);
        }
        if (pinMatches(hostDraft, pin)) {
          const outcome = draftedOutcome(pin, { id: pin.destinationAgentId }, `${recordedPin} is already in the host draft for destination ${pin.destinationAgentId}; no new attachment was dispatched and nothing was sent.`);
          state.placements[key] = outcome;
          delete state.intents[key];
          await writeDestinationAttachState(attachRoot, pin.destinationAgentId, state);
          return outcome;
        }
        const occupancy = hostDraft ? `Current occupied pin: ${hostDraft.summaryId} r${hostDraft.revision}.` : "The host draft currently returns no pin.";
        return unknownOutcome(pin, pin.destinationAgentId, `An attach intent or outcome is recorded for ${recordedPin}. ${occupancy} Earlier placement may have occurred; existing work is preserved and Kontor will not attach again.`);
      }

      let revision: SummaryRevision;
      try { revision = await readPinnedSummary(summaryRoot, pin); }
      catch { return refusedOutcome(pin, pin.destinationAgentId, `No saved summary ${pin.summaryId} r${pin.revision} is stored on this host; nothing was placed.`); }

      if (revision.summaryId !== pin.summaryId || revision.revision !== pin.revision) return refusedOutcome(pin, pin.destinationAgentId, "The saved revision does not match the requested pin; nothing was attached.");

      if (pin.destinationAgentId === revision.sourceAgentId) return refusedOutcome(pin, pin.destinationAgentId, "A summary goes to another seat, not back to its own source. Nothing was drafted.");

      let destination = await exact(pin.destinationAgentId);
      let refusal = destinationAttachRefusal(destination, pin, revision, pin.destinationAgentId);
      if (refusal) return refusal;

      try { hostDraft = await placer.readDraft(pin.destinationAgentId); }
      catch (error) {
        if (error instanceof MalformedHostDraftReadError) return unknownOutcome(pin, destination!.id, `${MALFORMED_HOST_DRAFT_READ_PRE} Check the destination's draft on the host.`);
        return unknownOutcome(pin, destination!.id, `The host draft could not be read (${error instanceof Error ? error.message : String(error)}). Nothing was placed.`);
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

      try { hostDraft = await placer.readDraft(pin.destinationAgentId); }
      catch (error) {
        const message = error instanceof MalformedHostDraftReadError
          ? `${MALFORMED_HOST_DRAFT_READ_PRE} Check the destination's draft on the host.`
          : `The host draft could not be read (${failText(error)}). Nothing was placed.`;
        return abandonIntentAfterPrePlaceReadFailure(message, destination!.id);
      }
      if (hostDraft && !pinMatches(hostDraft, pin)) {
        state = await readDestinationAttachState(attachRoot, pin.destinationAgentId);
        delete state.intents[key];
        await writeDestinationAttachState(attachRoot, pin.destinationAgentId, state);
        return refusedOutcome(pin, destination!.id, `“${destination!.title ?? destination!.id}” already has a draft (r${hostDraft.revision} of another summary). Attach never replaces a draft; send or clear it there first. Nothing was drafted.`);
      }
      if (pinMatches(hostDraft, pin)) {
        const outcome = draftedOutcome(revision, destination!, `r${revision.revision} is already in the draft of “${destination!.title ?? destination!.id}”; nothing changed and nothing was sent.`);
        state = await readDestinationAttachState(attachRoot, pin.destinationAgentId);
        state.placements[key] = outcome;
        delete state.intents[key];
        await writeDestinationAttachState(attachRoot, pin.destinationAgentId, state);
        return outcome;
      }

      let attached: HostSummaryDraftAttachResult;
      try { attached = await placer.attachDraft(pin.destinationAgentId, revision); }
      catch {
        // The accepted SDK throws for host refusals as well as transport failures, without a typed outcome.
        let occupancy = "";
        try {
          const current = await placer.readDraft(pin.destinationAgentId);
          if (current) occupancy = ` Current host pin: ${current.summaryId} r${current.revision}.`;
        } catch { occupancy = " Host draft readback is also unavailable."; }
        return unknownOutcome(pin, destination!.id, `The host did not confirm atomic attachment.${occupancy} The attach intent stays recorded; check the destination's draft. Kontor will not attach again automatically.`);
      }
      if (attached.state === "refused") {
        delete state.intents[key];
        await writeDestinationAttachState(attachRoot, pin.destinationAgentId, state);
        const occupied = attached.draft ? ` Occupied pin: ${attached.draft.summaryId} r${attached.draft.revision}.` : "";
        return refusedOutcome(pin, destination!.id, `The host refused attachment; existing work was preserved.${occupied} Nothing was sent.`);
      }
      if (!pinMatches(attached.draft, pin)) return unknownOutcome(pin, destination!.id, "The host attachment result did not confirm the exact saved revision. Check the destination's draft; Kontor will not attach again automatically.");

      try { hostDraft = await placer.readDraft(pin.destinationAgentId); }
      catch (error) {
        if (error instanceof MalformedHostDraftReadError) return unknownOutcome(pin, destination!.id, MALFORMED_HOST_DRAFT_READ_POST);
        return unknownOutcome(pin, destination!.id, `Draft placement was sent but the host draft could not be read back (${error instanceof Error ? error.message : String(error)}). Check the destination's draft; Kontor will not place it again automatically.`);
      }
      if (!pinMatches(hostDraft, pin)) {
        return unknownOutcome(pin, destination!.id, "Draft placement was sent but the host draft does not show this summary revision yet. Check the destination's draft; Kontor will not place it again automatically.");
      }

      const outcome = draftedOutcome(revision, destination!, `r${revision.revision} ${attached.state === "already" ? "was already" : "is"} in the draft of “${destination!.title ?? destination!.id}” for review. Nothing was sent and nobody else owns it.`);
      state = await readDestinationAttachState(attachRoot, pin.destinationAgentId);
      state.placements[key] = outcome;
      delete state.intents[key];
      await writeDestinationAttachState(attachRoot, pin.destinationAgentId, state);
      return outcome;
    });
  }

  return { attach };
}
