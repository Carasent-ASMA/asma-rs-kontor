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

type DirectoryListener = (state: DirectoryState) => void;

type RetainedDirectory = {
  readonly hostId: string;
  readonly directory: SessionDirectory;
  readonly listeners: Set<DirectoryListener>;
  state: DirectoryState;
  /** The host status last applied to the directory; null before its first start. */
  appliedStatus: HostStatus | null;
};

type DirectoryBinding = {
  readonly listener: DirectoryListener;
  record: RetainedDirectory | null;
};

export const BOOTSTRAP_LIMIT = 25;

/**
 * One live directory per host, kept across screen remounts. Opening a session
 * unmounts the Kontor screen, but the host evaluates this bundle once per host
 * connection, so a return reuses the live rows instead of walking every page.
 * The plugin cleanup disposes them; a host disconnect discards the bundle.
 */
const retained = new Map<string, RetainedDirectory>();

/**
 * `PluginClientContext` carries no host id, and `useHosts()` only lists hosts
 * without saying which installation is rendering. A host-bearing plugin
 * contribution (the sidebar item, or the first HostScreen when no sidebar item
 * mounted) supplies the installation's own `props.host.id`; the first such
 * mount claims this runtime's host. Controls contributed before that mount
 * wait for the claim; they never guess a host id or attach to a later host.
 */
let ownerHostId: string | null = null;
const pendingBindings = new Set<DirectoryBinding>();
const reportedListenerFailures = new Set<DirectoryListener>();

function notifyListener(
  record: RetainedDirectory,
  listener: DirectoryListener,
  state: DirectoryState,
): void {
  try {
    listener(state);
  } catch (error) {
    if (reportedListenerFailures.has(listener)) {
      return;
    }
    reportedListenerFailures.add(listener);
    console.error(
      `Kontor host directory listener failed for ${record.hostId}`,
      error,
    );
  }
}

function claimOwner(record: RetainedDirectory): void {
  if (ownerHostId !== null) {
    return;
  }
  ownerHostId = record.hostId;
  try {
    for (const binding of [...pendingBindings]) {
      binding.record = record;
      record.listeners.add(binding.listener);
      notifyListener(record, binding.listener, record.state);
    }
  } finally {
    pendingBindings.clear();
  }
}

function retain(hostId: string, includeArchived: boolean): RetainedDirectory {
  const existing = retained.get(hostId);
  if (existing) return existing;
  const listeners = new Set<(state: DirectoryState) => void>();
  const record: RetainedDirectory = {
    hostId,
    listeners,
    state: emptyDirectoryState(includeArchived),
    appliedStatus: null,
    directory: new SessionDirectory({
      client: () => getPaseoClient(hostId),
      includeArchived,
      bootstrapLimit: BOOTSTRAP_LIMIT,
      onChange: (next) => {
        record.state = next;
        for (const listener of [...listeners]) {
          notifyListener(record, listener, next);
        }
      },
    }),
  };
  retained.set(hostId, record);
  claimOwner(record);
  return record;
}

function applyHostStatus(
  record: RetainedDirectory,
  hostStatus: HostStatus,
): void {
  if (hostStatus === "online") {
    // A remount over a live or still-loading directory reuses it; a new,
    // offline or degraded one reconnects and walks every page.
    if (
      record.appliedStatus === "online" &&
      (record.state.live || record.state.status === "loading")
    ) {
      return;
    }
    record.appliedStatus = hostStatus;
    void record.directory.reconnect();
    return;
  }
  if (record.appliedStatus !== hostStatus) {
    record.appliedStatus = hostStatus;
    record.directory.markHostOffline(offlineReason(hostStatus));
  }
}

/** Starts this runtime's retained directory at the first host-bearing contribution. */
export function startHostDirectory(
  hostId: string,
  hostStatus: HostStatus = "online",
  includeArchived = false,
): void {
  applyHostStatus(retain(hostId, includeArchived), hostStatus);
}

/**
 * Binds a composer control to the directory owned by this plugin runtime.
 * The listener receives the current state immediately and every later state.
 */
export function bindHostDirectory(
  listener: (state: DirectoryState) => void,
): () => void {
  if (ownerHostId === null) {
    const binding: DirectoryBinding = { listener, record: null };
    pendingBindings.add(binding);
    return () => {
      pendingBindings.delete(binding);
      binding.record?.listeners.delete(listener);
      binding.record = null;
    };
  }
  const record = retained.get(ownerHostId);
  if (!record) {
    return () => undefined;
  }
  record.listeners.add(listener);
  notifyListener(record, listener, record.state);
  return () => {
    record.listeners.delete(listener);
  };
}

export async function disposeRetainedDirectories(): Promise<void> {
  const records = [...retained.values()];
  retained.clear();
  ownerHostId = null;
  for (const binding of pendingBindings) binding.record = null;
  pendingBindings.clear();
  for (const record of records) record.listeners.clear();
  reportedListenerFailures.clear();
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
    applyHostStatus(record, hostStatus);
  }, [hostId, hostStatus, enabled]);

  useEffect(() => {
    void directoryRef.current?.directory.setIncludeArchived(includeArchived);
  }, [includeArchived, hostId, enabled]);

  const refresh = useCallback(() => {
    void directoryRef.current?.directory.refresh("Refreshing at your request.");
  }, []);

  return { state, refresh };
}
