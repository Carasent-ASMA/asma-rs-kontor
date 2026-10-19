import type { PluginHostSummary } from "@getpaseo/plugin/client";
import { getPaseoClient } from "@getpaseo/plugin/client";
import { useCallback, useEffect, useRef, useState } from "react";
import {
  emptyDirectoryState,
  SessionDirectory,
  type DirectoryState,
} from "./session-directory";

export type HostStatus = PluginHostSummary["status"];

function offlineReason(status: HostStatus): string {
  if (status === "offline" || status === "error") {
    return "The Paseo host is disconnected. Showing the last inventory.";
  }
  return "The Paseo host is reconnecting. Showing the last inventory.";
}

export function useSessionDirectory(input: {
  readonly hostId: string;
  readonly hostStatus: HostStatus;
  readonly includeArchived: boolean;
}): {
  readonly state: DirectoryState;
  readonly refresh: () => void;
} {
  const { hostId, hostStatus, includeArchived } = input;
  const [stateForHost, setStateForHost] = useState<{
    readonly hostId: string;
    readonly state: DirectoryState;
  }>(() => ({ hostId, state: emptyDirectoryState(includeArchived) }));
  const state =
    stateForHost.hostId === hostId
      ? stateForHost.state
      : emptyDirectoryState(includeArchived);
  const directoryRef = useRef<SessionDirectory | null>(null);
  const includeArchivedRef = useRef(includeArchived);
  includeArchivedRef.current = includeArchived;

  useEffect(() => {
    let active = true;
    const directory = new SessionDirectory({
      client: () => getPaseoClient(hostId),
      includeArchived: includeArchivedRef.current,
      onChange: (next) => {
        if (active) {
          setStateForHost({ hostId, state: next });
        }
      },
    });
    directoryRef.current = directory;
    setStateForHost({
      hostId,
      state: emptyDirectoryState(includeArchivedRef.current),
    });
    return () => {
      active = false;
      if (directoryRef.current === directory) {
        directoryRef.current = null;
      }
      void directory.dispose();
    };
  }, [hostId]);

  useEffect(() => {
    const directory = directoryRef.current;
    if (!directory) {
      return;
    }
    if (hostStatus === "online") {
      void directory.reconnect();
    } else {
      directory.markHostOffline(offlineReason(hostStatus));
    }
  }, [hostId, hostStatus]);

  useEffect(() => {
    void directoryRef.current?.setIncludeArchived(includeArchived);
  }, [includeArchived]);

  const refresh = useCallback(() => {
    void directoryRef.current?.refresh("Refreshing at your request.");
  }, []);

  return { state, refresh };
}
