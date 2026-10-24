import { freshAvailability, type LifecycleRow } from "../shared/session-lifecycle";
import type { CreateOutcome } from "../shared/session-create";
import type { PluginScreenProps } from "@getpaseo/plugin/client";
import type { DirectoryEntry } from "./session-directory";
import { recordVisit } from "./recent-sessions";

export type SessionNavigation = NonNullable<PluginScreenProps["navigation"]>;

export type OpenSessionResult =
  | { readonly ok: true }
  | {
      readonly ok: false;
      readonly reason: "missing" | "archived" | "navigation" | "failed";
      readonly message: string;
    };

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
