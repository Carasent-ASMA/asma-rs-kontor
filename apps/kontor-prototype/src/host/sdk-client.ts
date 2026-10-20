/**
 * Fixture stand-in for the host-supplied `@getpaseo/plugin/client` module.
 * The Paseo compiler leaves this specifier external and the app injects its
 * own implementation; here the real SDK hooks (usePaseo, useAgent,
 * useWorkspace, useRpc) are reused unchanged and only the host-owned
 * functions are replaced by the fixture runtime.
 */
import { useCallback, useSyncExternalStore } from "react";
import type { ZodType } from "zod";
import { currentHostStatus, fakePaseo, HOST_ID, HOST_LABEL, keyboardSnapshot, saveKeyboard, saveSettings, settingsSnapshot, subscribeHost, subscribeKeyboard, subscribeSettings } from "./runtime";
import { logHostEvent } from "./event-log";

export { usePaseo, useAgent, useWorkspace, useRpc } from "paseo-sdk/client";

type HostSummary = { readonly serverId: string; readonly label: string; readonly status: "idle" | "connecting" | "online" | "offline" | "error" };
let hostsCache: { status: string; value: readonly HostSummary[] } | null = null;
function hostsSnapshot(): readonly HostSummary[] {
  const status = currentHostStatus();
  if (hostsCache?.status !== status) hostsCache = { status, value: [{ serverId: HOST_ID, label: HOST_LABEL, status }] };
  return hostsCache.value;
}

export function useHosts(): readonly HostSummary[] {
  return useSyncExternalStore(subscribeHost, hostsSnapshot);
}

export function getPaseoClient(serverId: string) {
  if (serverId !== HOST_ID) throw new Error(`Unknown prototype host ${serverId}`);
  return fakePaseo;
}

export async function openExternalUrl(url: string): Promise<void> {
  logHostEvent("openExternalUrl", url);
}

type SettingsDefinition = { id: string; schema: ZodType };
type SettingsSnapshot = { values: unknown; revision: string; saving: boolean };
type SettingsDocument = { snapshot(): SettingsSnapshot; subscribe(listener: () => void): () => void; save(values: unknown, revision: string): Promise<boolean> };
/** A stable snapshot per document, replaced only when its revision or saving state changes. */
function cachedSnapshot(read: () => SettingsSnapshot): () => SettingsSnapshot {
  let cache: SettingsSnapshot | null = null;
  return () => {
    const next = read();
    if (cache?.revision !== next.revision || cache.saving !== next.saving) cache = next;
    return cache;
  };
}
const DOCUMENTS: Record<string, SettingsDocument> = {
  organization: { snapshot: cachedSnapshot(settingsSnapshot), subscribe: subscribeSettings, save: saveSettings },
  keyboard: { snapshot: cachedSnapshot(keyboardSnapshot), subscribe: subscribeKeyboard, save: saveKeyboard },
};

/** The plugin's two settings documents, organization and keyboard, exist in the fixture host. */
export function useSettings(definition: SettingsDefinition) {
  const document = DOCUMENTS[definition.id];
  if (!document) throw new Error(`Unknown settings ${definition.id}`);
  const snapshot = useSyncExternalStore(document.subscribe, document.snapshot);
  const save = useCallback((values: unknown, revision: string) => document.save(values, revision), [document]);
  return {
    status: "ready" as const, values: snapshot.values, revision: snapshot.revision,
    saving: snapshot.saving, saveError: null, save,
    reset: async () => false, reload: async () => undefined,
  };
}
