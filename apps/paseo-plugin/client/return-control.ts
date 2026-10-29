import { createElement } from "react";
import type {
  PluginButtonContentProps,
  PluginButtonRegistration,
  PluginClientContext,
} from "@getpaseo/plugin/client";
import { SummaryAttachmentAvailability } from "./summary-attachments";
import { withdrawHandOff } from "./session-actions";
import { StartSwitchPopover } from "./start-switch-sheet";
import { bindHostDirectory } from "./use-session-directory";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import { composerSessionPresentation } from "./composer-session-state";

export const RETURN_CONTROL_ID = "return-to-kontor";
export const RETURN_CONTROL_TITLE = "Start or switch session";
export const RETURN_CONTROL_LABEL = "Kontor";

type RegisteredPill = {
  readonly workspaceId: string;
  readonly registration: PluginButtonRegistration;
  readonly presentation: ReturnType<typeof composerSessionPresentation>;
};

/**
 * Adds one understated Kontor pill to each live agent's composer track. It opens
 * the Start or switch popover, whose Open Kontor action keeps the old return.
 * The host owns the pill's styling, spacing, touch target, ordering and the
 * popover or bottom sheet; this module supplies the icon, label and body.
 *
 * Pills follow the host's one shared session directory. The first bootstrap
 * page registers them at the first round-trip; only a complete inventory walk
 * may remove a pill it no longer lists. All registrations are removed on
 * cleanup, and a late state after cleanup registers nothing.
 */
export function contributeReturnControl(
  client: PluginClientContext,
  screenId: string,
  summaryAttachmentAvailable = false,
): () => void {
  const pills = new Map<string, RegisteredPill>();
  let stopped = false;
  // One body for every pill. This installation's client opens screens on its own host, the pill's host.
  const Content = (props: PluginButtonContentProps) =>
    createElement(SummaryAttachmentAvailability.Provider, { value: summaryAttachmentAvailable }, createElement(StartSwitchPopover, {
      ...props,
      openScreen: (input) => client.openScreen(input),
      screenId,
      summaryAttachmentAvailable,
    }));

  function removePill(agentId: string): void {
    const current = pills.get(agentId);
    if (!current) {
      return;
    }
    pills.delete(agentId);
    current.registration.remove();
  }

  function removeAll(): void {
    for (const agentId of [...pills.keys()]) {
      removePill(agentId);
    }
  }

  function registerAgent(entry: DirectoryEntry, state: DirectoryState): void {
    if (stopped) {
      return;
    }
    const workspaceId = entry.workspaceId?.trim() ?? "";
    if (workspaceId.length === 0 || entry.archivedAt !== null) {
      removePill(entry.id);
      return;
    }
    const current = pills.get(entry.id);
    const presentation = composerSessionPresentation(entry, state, entry.title);
    try {
      if (current?.workspaceId === workspaceId) {
        if (current.presentation.icon !== presentation.icon || current.presentation.title !== presentation.title) {
          current.registration.update(presentation);
          pills.set(entry.id, { ...current, presentation });
        }
        return;
      }
      if (current) {
        removePill(entry.id);
      }
      const registration = client.addComposerPill({
        id: RETURN_CONTROL_ID,
        workspaceId,
        agentId: entry.id,
        button: {
          ...presentation,
          label: RETURN_CONTROL_LABEL,
          behavior: { kind: "popover", Content },
        },
      });
      pills.set(entry.id, { workspaceId, registration, presentation });
    } catch (error) {
      console.error("Kontor return control could not reconcile a composer pill", error);
    }
  }

  function apply(state: DirectoryState): void {
    if (stopped) {
      return;
    }
    if (state.entries.length === 0 && state.status !== "ready") {
      return;
    }
    const seen = new Set<string>();
    for (const entry of state.entries) {
      seen.add(entry.id);
      registerAgent(entry, state);
    }
    // A provisional page and a partial read may omit older sessions. Only a
    // complete inventory may establish that a missing pill should go away.
    if (state.status === "ready" && state.inventoryComplete === true) {
      for (const agentId of [...pills.keys()]) {
        if (!seen.has(agentId)) {
          removePill(agentId);
        }
      }
    }
  }

  const releaseDirectory = bindHostDirectory(apply);

  return () => {
    stopped = true;
    // First, so a throwing removal cannot leave a request for a later screen to open.
    withdrawHandOff();
    releaseDirectory();
    removeAll();
  };
}
