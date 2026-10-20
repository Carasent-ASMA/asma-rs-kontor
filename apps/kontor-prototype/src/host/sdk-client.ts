/**
 * Fixture stand-in for the host-supplied `@getpaseo/plugin/client` module.
 * The Paseo compiler leaves this specifier external and the app injects its
 * own implementation; here the real SDK hooks (usePaseo, useAgent,
 * useWorkspace, useRpc) are reused unchanged and only the host-owned
 * functions are replaced by the fixture runtime.
 */
import { useCallback, useSyncExternalStore } from "react";
import type { ZodType } from "zod";
import { currentHostStatus, fakePaseo, HOST_ID, HOST_LABEL, saveSettings, settingsSnapshot, subscribeHost, subscribeSettings } from "./runtime";
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
let settingsCache: { revision: string; saving: boolean; value: ReturnType<typeof settingsSnapshot> } | null = null;
function settingsValue() {
  const next = settingsSnapshot();
  if (settingsCache?.revision !== next.revision || settingsCache.saving !== next.saving) settingsCache = { revision: next.revision, saving: next.saving, value: next };
  return settingsCache.value;
}

/** Only the plugin's one settings document exists in the fixture host. */
export function useSettings(definition: SettingsDefinition) {
  if (definition.id !== "organization") throw new Error(`Unknown settings ${definition.id}`);
  const snapshot = useSyncExternalStore(subscribeSettings, settingsValue);
  const save = useCallback((values: unknown, revision: string) => saveSettings(values, revision), []);
  return {
    status: "ready" as const, values: snapshot.values, revision: snapshot.revision,
    saving: snapshot.saving, saveError: null, save,
    reset: async () => false, reload: async () => undefined,
  };
}
