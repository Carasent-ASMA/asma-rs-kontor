import { freshAvailability, type LifecycleRow, type TakeoverOutcome } from "../shared/session-lifecycle";
import type { CreateOutcome } from "../shared/session-create";
import type { PluginScreenProps } from "@getpaseo/plugin/client";
import type { DirectoryEntry } from "./session-directory";
import { recordVisit } from "./recent-sessions";
import type { SwitchView } from "./session-state-shortcuts";

export type SessionNavigation = NonNullable<PluginScreenProps["navigation"]>;

export type OpenSessionResult =
  /** `handoff`: the Kontor screen received the request; nothing has opened yet. */
  | { readonly ok: true; readonly handoff?: true }
  | {
      readonly ok: false;
      readonly reason: "missing" | "archived" | "navigation" | "failed" | "stale";
      readonly message: string;
    };

/** What a composer popover asks the Kontor screen to open; plugin popovers get no session navigation of their own. */
export type HandOffTarget =
  | { readonly kind: "session"; readonly agentId: string }
  | { readonly kind: "created"; readonly outcome: CreateOutcome }
  | { readonly kind: "takeover"; readonly outcome: TakeoverOutcome; readonly onOpened?: (result: OpenSessionResult) => void }
  | ({ readonly kind: "view" } & SwitchView);

/**
 * The one request in flight from a popover to the Kontor screen. It lives in memory only and the screen URL
 * carries just its key, so Back, history, a remount and a reload find nothing to replay. Only its own key takes it.
 */
let handOff: { readonly hostId: string; readonly key: string; readonly target: HandOffTarget } | null = null;
let handOffCount = 0;

/** Replaces any earlier request and returns the key the screen must present. */
export function handOffOpen(hostId: string, target: HandOffTarget): string {
  const key = `${Date.now().toString(36)}-${++handOffCount}`;
  handOff = { hostId, key, target };
  return key;
}

/**
 * Consumes the request its key names, before any host check or navigation; another host gets nothing from it.
 * An older or unknown key gets nothing and leaves the current request alone, so a stale screen cannot cancel the latest click.
 */
export function takeHandOff(hostId: string, key: string): HandOffTarget | null {
  const current = handOff;
  if (current?.key !== key) return null;
  handOff = null;
  return current.hostId === hostId ? current.target : null;
}

/** Drops a request the screen never received, or every request when the plugin is disposed. */
export function withdrawHandOff(key?: string): void {
  if (key === undefined || handOff?.key === key) handOff = null;
}

/**
 * The Kontor screen's half of a hand-off: consumes the request before navigating, then opens through the shared
 * path below. Null when the screen was opened without a request; a used, replaced or foreign key opens nothing.
 */
export function openHandOff(input: {
  readonly hostId: string;
  readonly key: string | undefined;
  readonly entries: readonly DirectoryEntry[];
  readonly navigation: SessionNavigation | undefined;
  readonly onView?: (view: SwitchView) => void;
}): OpenSessionResult | null {
  if (!input.key) return null;
  const target = takeHandOff(input.hostId, input.key);
  if (!target) {
    return { ok: false, reason: "stale", message: "This Kontor request was already handled or has expired. Nothing was opened; choose the session again." };
  }
  if (target.kind === "view") {
    if (!input.onView) return { ok: false, reason: "navigation", message: "This Kontor screen cannot show the switcher's sessions. Open Sessions and choose the state and search again." };
    try { input.onView({ status: target.status, query: target.query, membership: target.membership }); }
    catch (error) { return { ok: false, reason: "navigation", message: error instanceof Error ? error.message : String(error) }; }
    return { ok: true };
  }
  if (target.kind === "session") {
    return openSessionById({ entries: input.entries, id: target.agentId, serverId: input.hostId, navigation: input.navigation });
  }
  if (target.kind === "takeover") {
    const result = openTakeoverSuccessor({ outcome: target.outcome, serverId: input.hostId, navigation: input.navigation });
    target.onOpened?.(result);
    return result;
  }
  const result = openCreatedSession({ outcome: target.outcome, serverId: input.hostId, navigation: input.navigation });
  // The creation itself succeeded; keep its exact identity in the refusal so it can still be found.
  return result.ok ? result : { ...result, message: `Created “${target.outcome.title}” (session ID ${target.outcome.agentId}). ${result.message}` };
}

/**
 * Opens the existing native session by its host ID. A display title is never an
 * identity: duplicate names stay distinct rows and only the selected ID leaves
 * this function. Every Kontor open path comes through here, so an accepted open is
 * this device's Recent visit; a refusal records nothing.
 */
export function openSessionById(input: {
  readonly entries: readonly DirectoryEntry[];
  readonly id: string;
  readonly serverId: string;
  readonly navigation: SessionNavigation | undefined;
}): OpenSessionResult {
  const entry = input.entries.find((candidate) => candidate.id === input.id);
  if (!entry) {
    return {
      ok: false,
      reason: "missing",
      message: "That session is no longer in the directory. Refresh the list and try again.",
    };
  }
  if (freshAvailability(entry)?.state === "missing") {
    return { ok: false, reason: "missing", message: "The host could not find this exact session ID at the last availability check. Refresh availability before opening it again." };
  }
  return openNativeSession({ ...input, id: entry.id });
}

/** A successful correlated creation readback supplies the exact native ID without a full directory walk. */
export function openCreatedSession(input: {
  readonly outcome: CreateOutcome;
  readonly serverId: string;
  readonly navigation: SessionNavigation | undefined;
}): OpenSessionResult {
  if (!["verified", "differences"].includes(input.outcome.state) || !input.outcome.agentId) {
    return { ok: false, reason: "missing", message: "The host has not confirmed this creation's exact session ID. Check its outcome before opening it." };
  }
  return openNativeSession({ ...input, id: input.outcome.agentId });
}

/** Only a completed takeover's verified successor may use the creation-independent native open path. */
export function openTakeoverSuccessor(input: { readonly outcome: TakeoverOutcome; readonly serverId: string; readonly navigation: SessionNavigation | undefined }): OpenSessionResult {
  const { outcome } = input;
  if (outcome.state !== "succeeded" || !outcome.successor?.agentId?.trim() || !outcome.successor.title.trim() || outcome.successor.agentId === outcome.agentId) {
    return { ok: false, reason: "missing", message: "Takeover has not confirmed an exact replacement session. Check the existing operation before opening it." };
  }
  const result = openNativeSession({ id: outcome.successor.agentId, serverId: input.serverId, navigation: input.navigation });
  return result.ok ? result : { ...result, message: `Takeover completed into “${outcome.successor.title}”. ${result.message}` };
}

/** An archive's own exact-ID read names the native ID after its row left the directory; nothing else does. */
export function openRecoveredSession(input: {
  readonly row: LifecycleRow | null;
  readonly id: string;
  readonly serverId: string;
  readonly navigation: SessionNavigation | undefined;
}): OpenSessionResult {
  if (input.row?.id !== input.id || input.row.availability?.agentId !== input.id || input.row.availability.state !== "readable") {
    return { ok: false, reason: "missing", message: "The host has not confirmed this exact session ID. Refresh status before opening it." };
  }
  if (archivedByRead(input.row, input.id)) {
    return { ok: false, reason: "archived", message: "This session is archived. Refresh status settles its archive; opening it would ask Paseo to restore its workspace." };
  }
  return openNativeSession(input);
}

/**
 * The archive's own exact-ID read shows it in the archive. Recovery never opens it: Paseo opens an archived session
 * through its workspace restore, which settling an archive does not need. The Archived list still opens it.
 */
export function archivedByRead(row: LifecycleRow | null, id: string): boolean {
  return row?.id === id && row.availability?.agentId === id && row.availability.state === "readable" && !!row.availability.archivedAt;
}

function openNativeSession(input: { readonly id: string; readonly serverId: string; readonly navigation: SessionNavigation | undefined }): OpenSessionResult {
  if (!input.navigation) {
    return {
      ok: false,
      reason: "navigation",
      message: "This Paseo app does not expose native session navigation to plugins.",
    };
  }
  try {
    input.navigation.openAgent({ agentId: input.id, serverId: input.serverId });
    recordVisit(input.serverId, input.id);
    return { ok: true };
  } catch (error) {
    return {
      ok: false,
      reason: "failed",
      message:
        error instanceof Error && error.message.trim().length > 0
          ? error.message
          : "Paseo could not open that session.",
    };
  }
}
