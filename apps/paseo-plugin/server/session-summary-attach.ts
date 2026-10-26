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

const ATTACH_STORE_LIMIT =
  "Attach idempotency and destination draft occupancy persist under the Kontor plugin host-backed state root (plugin-state/kontor/session-summary-attach-v1); this plugin is the sole writer for those records on this host.";

export const DRAFT_ATTACH_LIMITS = [
  "Attach places the saved revision in the destination seat's draft through the host SDK placeSummaryDraft route; nothing is sent.",
  "Each destination keeps at most one draft; Attach never replaces an existing draft or an in-flight task.",
  ATTACH_STORE_LIMIT,
];

export type SummaryDraftPlacer = {
  placeDraft(destinationAgentId: string, revision: SummaryRevision): Promise<void>;
  readDraft?(destinationAgentId: string): Promise<{ summaryId: string; revision: number } | null>;
};

/** Optional host SDK route when the installed client adds agents.placeSummaryDraft. */
export function paseoSummaryDraftPlacer(paseo: PaseoApi): SummaryDraftPlacer | undefined {
  const host = paseo as {
    agents?: typeof paseo.agents & {
      placeSummaryDraft?(destinationAgentId: string, draft: { summaryId: string; revision: number; text: string; sourceAgentId: string }): Promise<void>;
      readSummaryDraft?(destinationAgentId: string): Promise<{ summaryId: string; revision: number } | null>;
    };
  };
  if (typeof host.agents?.placeSummaryDraft !== "function") return undefined;
  const place = host.agents.placeSummaryDraft.bind(host.agents);
  const readDraft = typeof host.agents.readSummaryDraft === "function" ? host.agents.readSummaryDraft.bind(host.agents) : undefined;
  return {
    placeDraft: (destinationAgentId, revision) => place(destinationAgentId, { summaryId: revision.summaryId, revision: revision.revision, text: revision.text, sourceAgentId: revision.sourceAgentId }),
    readDraft,
  };
}

export function draftAttachInstalledCapability(paseo: PaseoApi): Capability {
  if (!paseoSummaryGenerator(paseo)) return { supported: false, reason: "This host has no saved summary store; Kontor cannot attach a summary revision here." };
  if (!paseoSummaryDraftPlacer(paseo)) return UNSUPPORTED.draftAttach;
  return { supported: true, limits: DRAFT_ATTACH_LIMITS };
}

const destinationDraftSchema = z.object({
  summaryId: z.string().uuid(),
  revision: z.number().int().positive(),
  sourceAgentId: z.string().min(1),
  at: z.string(),
}).strict().nullable();

const destinationAttachSchema = z.object({
  draft: destinationDraftSchema,
  placements: z.record(z.string(), attachOutcomeSchema),
}).strict();
type DestinationAttachState = z.infer<typeof destinationAttachSchema>;

const destinationPath = (root: string, destinationAgentId: string) => join(root, `dest-${createHash("sha256").update(destinationAgentId).digest("hex")}.json`);

function placementKey(pin: SummaryPin, destinationAgentId: string) {
  return `${pin.summaryId}:${pin.revision}:${destinationAgentId}`;
}

async function readDestinationState(root: string, destinationAgentId: string): Promise<DestinationAttachState> {
  try {
    return destinationAttachSchema.parse(JSON.parse(await readFile(destinationPath(root, destinationAgentId), "utf8")));
  } catch (error) {
    if (error instanceof Error && "code" in error && error.code === "ENOENT") return { draft: null, placements: {} };
    throw error;
  }
}

async function writeDestinationState(root: string, destinationAgentId: string, state: DestinationAttachState) {
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

function refusedOutcome(revision: SummaryPin, destinationAgentId: string, message: string): AttachOutcome {
  return { state: "refused", message, destinationAgentId, summaryId: revision.summaryId, revision: revision.revision };
}

export function createSummaryAttachService(paseo: PaseoApi, summaryRoot: string, attachRoot: string) {
  async function exact(agentId: string): Promise<Agent | null> {
    const found = await paseo.agents.ref(agentId).refresh();
    return found?.agent.id === agentId ? found.agent : null;
  }

  async function attach(pin: SummaryPin & { destinationAgentId: string }): Promise<AttachOutcome> {
    const placer = paseoSummaryDraftPlacer(paseo);
    if (!placer) throw new Error(DRAFT_PLACEMENT_API_GAP);
    if (!paseoSummaryGenerator(paseo)) throw new Error("This host has no saved summary store; Kontor cannot attach a summary revision here.");

    let revision: SummaryRevision;
    try { revision = await readPinnedSummary(summaryRoot, pin); }
    catch { return refusedOutcome(pin, pin.destinationAgentId, `No saved summary ${pin.summaryId} r${pin.revision} is stored on this host; nothing was placed.`); }

    const destination = await exact(pin.destinationAgentId);
    if (!destination) return refusedOutcome(pin, pin.destinationAgentId, `No session ${pin.destinationAgentId} is on this host; nothing was placed.`);
    if (destination.archivedAt) return refusedOutcome(pin, destination.id, `“${destination.title ?? destination.id}” is archived, so it has no draft. Nothing was drafted.`);
    if (destination.id === revision.sourceAgentId) return refusedOutcome(pin, destination.id, "A summary goes to another seat, not back to its own source. Nothing was drafted.");

    return withSourceStoreLock(attachRoot, pin.destinationAgentId, async () => {
      const key = placementKey(pin, pin.destinationAgentId);
      const state = await readDestinationState(attachRoot, pin.destinationAgentId);
      const prior = state.placements[key];
      if (prior) return prior;

      const hostDraft = await placer.readDraft?.(pin.destinationAgentId) ?? null;
      const recorded = state.draft;
      const occupied = hostDraft ?? recorded;
      if (occupied && (occupied.summaryId !== revision.summaryId || occupied.revision !== revision.revision)) {
        const who = recorded?.sourceAgentId ?? revision.sourceAgentId;
        const rev = occupied.revision;
        return refusedOutcome(pin, destination.id, `“${destination.title ?? destination.id}” already has a draft (r${rev} of ${who}). Attach never replaces a draft; send or clear it there first. Nothing was drafted.`);
      }
      if (occupied?.summaryId === revision.summaryId && occupied.revision === revision.revision) {
        const outcome = draftedOutcome(revision, destination, `r${revision.revision} is already in the draft of “${destination.title ?? destination.id}”; nothing changed and nothing was sent.`);
        state.placements[key] = outcome;
        await writeDestinationState(attachRoot, pin.destinationAgentId, state);
        return outcome;
      }

      await placer.placeDraft(pin.destinationAgentId, revision);
      const outcome = draftedOutcome(revision, destination, `r${revision.revision} is in the draft of “${destination.title ?? destination.id}” for review. Nothing was sent and nobody else owns it.`);
      state.draft = { summaryId: revision.summaryId, revision: revision.revision, sourceAgentId: revision.sourceAgentId, at: new Date().toISOString() };
      state.placements[key] = outcome;
      await writeDestinationState(attachRoot, pin.destinationAgentId, state);
      return outcome;
    });
  }

  return { attach };
}
