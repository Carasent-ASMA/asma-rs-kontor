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

type RetainedDirectory = {
  readonly directory: SessionDirectory;
  readonly listeners: Set<(state: DirectoryState) => void>;
  state: DirectoryState;
  /** The host status last applied to the directory; null before its first start. */
  appliedStatus: HostStatus | null;
};

/**
 * One live directory per host, kept across screen remounts. Opening a session
 * unmounts the Kontor screen, but the host evaluates this bundle once per host
 * connection, so a return reuses the live rows instead of walking every page.
 * The plugin cleanup disposes them; a host disconnect discards the bundle.
 */
const retained = new Map<string, RetainedDirectory>();

function retain(hostId: string, includeArchived: boolean): RetainedDirectory {
  const existing = retained.get(hostId);
  if (existing) return existing;
  const listeners = new Set<(state: DirectoryState) => void>();
  const record: RetainedDirectory = {
    listeners,
    state: emptyDirectoryState(includeArchived),
    appliedStatus: null,
    directory: new SessionDirectory({
      client: () => getPaseoClient(hostId),
      includeArchived,
      onChange: (next) => {
        record.state = next;
        for (const listener of listeners) listener(next);
      },
    }),
  };
  retained.set(hostId, record);
  return record;
}

export async function disposeRetainedDirectories(): Promise<void> {
  const records = [...retained.values()];
  retained.clear();
  await Promise.all(records.map((record) => record.directory.dispose()));
}

export function useSessionDirectory(input: {
  readonly hostId: string;
  readonly hostStatus: HostStatus;
  readonly includeArchived: boolean;
  readonly enabled?: boolean;
}): {
  readonly state: DirectoryState;
  readonly refresh: () => void;
} {
  const { hostId, hostStatus, includeArchived, enabled = true } = input;
  const [stateForHost, setStateForHost] = useState<{
    readonly hostId: string;
    readonly state: DirectoryState;
  }>(() => ({ hostId, state: retained.get(hostId)?.state ?? emptyDirectoryState(includeArchived) }));
  const state =
    stateForHost.hostId === hostId
      ? stateForHost.state
      : retained.get(hostId)?.state ?? emptyDirectoryState(includeArchived);
  const directoryRef = useRef<RetainedDirectory | null>(null);
  const includeArchivedRef = useRef(includeArchived);
  includeArchivedRef.current = includeArchived;

  useEffect(() => {
    if (!enabled) return;
    const record = retain(hostId, includeArchivedRef.current);
    const listener = (next: DirectoryState) => setStateForHost({ hostId, state: next });
    record.listeners.add(listener);
    directoryRef.current = record;
    setStateForHost({ hostId, state: record.state });
    return () => {
      record.listeners.delete(listener);
      if (directoryRef.current === record) {
        directoryRef.current = null;
      }
    };
  }, [hostId, enabled]);

  useEffect(() => {
    const record = directoryRef.current;
    if (!record) {
      return;
    }
    if (hostStatus === "online") {
      // A remount over a live or still-loading directory reuses it; a new,
      // offline or degraded one reconnects and walks every page.
      if (record.appliedStatus === "online" && (record.state.live || record.state.status === "loading")) return;
      record.appliedStatus = hostStatus;
      void record.directory.reconnect();
    } else if (record.appliedStatus !== hostStatus) {
      record.appliedStatus = hostStatus;
      record.directory.markHostOffline(offlineReason(hostStatus));
    }
  }, [hostId, hostStatus, enabled]);

  useEffect(() => {
    void directoryRef.current?.directory.setIncludeArchived(includeArchived);
  }, [includeArchived, hostId, enabled]);

  const refresh = useCallback(() => {
    void directoryRef.current?.directory.refresh("Refreshing at your request.");
  }, []);

  return { state, refresh };
}
