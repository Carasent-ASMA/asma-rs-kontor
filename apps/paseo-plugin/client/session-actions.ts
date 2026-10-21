import type { PluginScreenProps } from "@getpaseo/plugin/client";
import type { DirectoryEntry } from "./session-directory";

export type SessionNavigation = NonNullable<PluginScreenProps["navigation"]>;

export type OpenSessionResult =
  | { readonly ok: true }
  | {
      readonly ok: false;
      readonly reason: "missing" | "navigation" | "failed";
      readonly message: string;
    };

/**
 * Opens the existing native session by its host ID. A display title is never an
 * identity: duplicate names stay distinct rows and only the selected ID leaves
 * this function.
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
  if (entry.availability?.state === "missing" && entry.availability.revision === entry.updatedAt) {
    return { ok: false, reason: "missing", message: "The host could not find this exact session ID at the last availability check. Refresh availability before opening it again." };
  }
  if (!input.navigation) {
    return {
      ok: false,
      reason: "navigation",
      message: "This Paseo app does not expose native session navigation to plugins.",
    };
  }
  try {
    input.navigation.openAgent({ agentId: entry.id, serverId: input.serverId });
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
