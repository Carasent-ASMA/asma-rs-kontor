import type { ComponentType } from "react";
import { callPluginRpc } from "paseo-sdk/host";
import contribute from "../../../paseo-plugin/index.client";
import { logHostEvent } from "./event-log";
import { fakePaseo, invokeRpc } from "./runtime";

/**
 * Calls the plugin's real `contribute()` entry with a fixture client context and
 * keeps what it registers: the screen, the sidebar row, command items and the
 * per-session composer pills. Contribution kinds the plugin does not use are
 * accepted and ignored.
 */
type Screen = { id: string; title: unknown; Component: ComponentType<any> };
type SidebarItem = { id: string; title: string; Component: ComponentType<any> };
type Button = { title: string; icon: unknown; label?: string; visible?: boolean; disabled?: boolean; behavior: { kind: string; onPress?: () => void | Promise<void> } };
export type Pill = { id: string; workspaceId: string; agentId: string; button: Button };
export type HostRoute =
  | { kind: "screen"; screenId: string; params: Record<string, string> }
  | { kind: "agent"; agentId: string }
  | { kind: "workspace"; workspaceId: string };

const screens = new Map<string, Screen>();
const sidebar: SidebarItem[] = [];
const pills = new Map<string, Pill>();
const listeners = new Set<() => void>();
let version = 0;
function changed() { version += 1; for (const listener of listeners) listener(); }
export function subscribeRegistry(listener: () => void) { listeners.add(listener); return () => listeners.delete(listener); }
export function registryVersion() { return version; }
export function contributedScreen(id: string) { return screens.get(id); }
export function contributedSidebar(): readonly SidebarItem[] { return sidebar; }
export function pillsFor(agentId: string): Pill[] {
  return [...pills.values()].filter((pill) => pill.agentId === agentId && pill.button.visible !== false);
}

let openScreenHandler: (route: HostRoute) => void = () => undefined;
export function setOpenScreenHandler(handler: (route: HostRoute) => void) { openScreenHandler = handler; }

const cleanup = () => undefined;
const client = {
  paseo: fakePaseo,
  rpc: (contract: never, input: unknown) => callPluginRpc(contract, invokeRpc, input as never),
  openScreen(input: { screenId: string; params?: Record<string, string> }) {
    logHostEvent("openScreen", `${input.screenId} ${JSON.stringify(input.params ?? {})}`);
    openScreenHandler({ kind: "screen", screenId: input.screenId, params: input.params ?? {} });
  },
  openSurface(id: string) { client.openScreen({ screenId: id }); },
  openSettings() { return undefined; },
  playAudio: async () => undefined,
  addScreen(contribution: Screen) { screens.set(contribution.id, contribution); changed(); return () => { screens.delete(contribution.id); changed(); }; },
  addSidebarHeaderItem(contribution: SidebarItem) { sidebar.push(contribution); changed(); return cleanup; },
  addSidebarFooterItem: () => cleanup,
  addSettingsScreen: () => cleanup,
  addWorkspacePanel: () => cleanup,
  addCommandCenterItem: () => cleanup,
  addSlashCommand: () => cleanup,
  addAttachmentSource: () => cleanup,
  addTheme: () => cleanup,
  addTimelineTransformer: () => cleanup,
  addTimelineRenderer: () => cleanup,
  addSurface: () => cleanup,
  addSidebarItem: () => cleanup,
  openPanel: () => undefined,
  addHeaderButton: () => ({ update: () => undefined, remove: () => undefined }),
  addComposerPill(contribution: Pill) {
    const key = `${contribution.agentId}:${contribution.id}`;
    pills.set(key, { ...contribution });
    changed();
    return {
      update(patch: Partial<Button>) { const current = pills.get(key); if (current) { pills.set(key, { ...current, button: { ...current.button, ...patch } }); changed(); } },
      remove() { if (pills.delete(key)) changed(); },
    };
  },
};

export const stopContribution = contribute(client as never);
