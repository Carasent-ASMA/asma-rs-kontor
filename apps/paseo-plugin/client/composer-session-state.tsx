import type { PluginButtonIconProps } from "@getpaseo/plugin/client";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { View } from "react-native";
import type { DirectoryEntry, DirectoryState } from "./session-directory";
import { QUICK_FILTER_ICONS } from "./mobile-quick-filter-options";
import { sessionState, STATE_LABELS, type OrganizationState } from "./session-organization";
import { sessionStatusInk } from "./session-status-ink";

function stateIcon(state: OrganizationState, lastKnown: boolean) {
  const status = state === "failed" ? "error" : state;
  return function ComposerSessionIcon({ theme, size }: PluginButtonIconProps) {
    return <View accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" aria-hidden>
      <Icon name={QUICK_FILTER_ICONS[status]} size={size}
        color={lastKnown ? theme.colors.foregroundMuted : sessionStatusInk(status, theme.colors)} />
    </View>;
  };
}

function iconsFor(state: OrganizationState) {
  return { current: stateIcon(state, false), lastKnown: stateIcon(state, true) };
}

// Stable component identities keep ordinary metadata updates from remounting the icon.
const icons: Record<OrganizationState, ReturnType<typeof iconsFor>> = {
  needs_input: iconsFor("needs_input"),
  failed: iconsFor("failed"),
  running: iconsFor("running"),
  attention: iconsFor("attention"),
  idle: iconsFor("idle"),
  initializing: iconsFor("initializing"),
  closed: iconsFor("closed"),
};

/** Presentation of this exact directory entry, never a second state evaluator or read. */
export function composerSessionPresentation(
  entry: DirectoryEntry,
  directory: Pick<DirectoryState, "live">,
  title: string,
) {
  const state = sessionState(entry);
  return {
    title: `${title} · ${directory.live ? "" : "Last known: "}${STATE_LABELS[state]}`,
    icon: directory.live ? icons[state].current : icons[state].lastKnown,
  };
}
