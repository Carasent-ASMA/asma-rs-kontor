import type {
  PluginButtonRegistration,
  PluginClientContext,
} from "@getpaseo/plugin/client";
import { bindHostDirectory } from "./use-session-directory";
import type { DirectoryEntry, DirectoryState } from "./session-directory";

export const RETURN_CONTROL_ID = "return-to-kontor";
export const RETURN_CONTROL_TITLE = "Open Kontor";
export const RETURN_CONTROL_LABEL = "Kontor";
export const RETURN_CONTROL_ICON = "ArrowLeft";

type RegisteredPill = {
  readonly workspaceId: string;
  readonly registration: PluginButtonRegistration;
};

/**
 * Adds one understated return pill to each live agent's composer track. The
 * host owns the pill's styling, spacing, touch target and ordering; this module
 * only supplies the Lucide arrow and the short Kontor label.
 *
 * Pills follow the host's one shared session directory. The first bootstrap
 * page registers them at the first round-trip; only a complete inventory walk
 * may remove a pill it no longer lists. All registrations are removed on
 * cleanup, and a late state after cleanup registers nothing.
 */
export function contributeReturnControl(
  client: PluginClientContext,
  screenId: string,
): () => void {
  const pills = new Map<string, RegisteredPill>();
  let stopped = false;

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

  function registerAgent(entry: DirectoryEntry): void {
    if (stopped) {
      return;
    }
    const workspaceId = entry.workspaceId?.trim() ?? "";
    if (
      workspaceId.length === 0 ||
      entry.archivedAt !== null ||
      entry.status === "closed"
    ) {
      removePill(entry.id);
      return;
    }
    const current = pills.get(entry.id);
    if (current?.workspaceId === workspaceId) {
      return;
    }
    if (current) {
      removePill(entry.id);
    }
    try {
      const registration = client.addComposerPill({
        id: RETURN_CONTROL_ID,
        workspaceId,
        agentId: entry.id,
        button: {
          title: RETURN_CONTROL_TITLE,
          icon: RETURN_CONTROL_ICON,
          label: RETURN_CONTROL_LABEL,
          behavior: {
            kind: "action",
            onPress() {
              client.openScreen({ screenId });
            },
          },
        },
      });
      pills.set(entry.id, { workspaceId, registration });
    } catch (error) {
      console.error("Kontor return control could not register a composer pill", error);
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
      registerAgent(entry);
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
    releaseDirectory();
    removeAll();
  };
}
