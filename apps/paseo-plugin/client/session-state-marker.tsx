import { createContext, useContext } from "react";
import { ComposerPulse } from "./composer-pulse";
import { View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { QUICK_FILTER_ICONS } from "./mobile-quick-filter-options";
import type { Colors } from "./page-controls";
import type { OrganizationState } from "./session-organization";
import type { StatusFilter } from "./session-view";
import { sessionStatusInk } from "./session-status-ink";

/**
 * The exact quick-filter icon for a state. The row already
 * names its status; this adds no focus target or repeated screen-reader speech.
 * Labeled status filters teach the same shapes, without another permanent legend.
 * D6 + the TPM ruling: a LIVE organization badge led by Running animates on the shared carrier like
 * any live row; a NOT-LIVE one keeps runningMarkNode's muted ink — the rainbow never claims live.
 * Filter controls always animate: they call SessionStatusIcon directly and never gate on liveness.
 */
export const SessionPulseContext = createContext({ live: false, visible: false });

/** The badge's running mark goes muted whenever the data is not live: an explicit stale flag wins
 * (the banner's own condition), otherwise the surrounding SessionPulseContext liveness decides.
 * Outside any provider the default is not-live, so a forgotten tree can never animate stale data. */
export const badgeStale = (stale: boolean | undefined, live: boolean | undefined): boolean => stale ?? live === false;

export function SessionStatusIcon({ status, colors, size = 16, session = false, stale = false }: { status: StatusFilter; colors: Colors; size?: number; session?: boolean; stale?: boolean }) {
  return <View testID="session-state-marker" accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" aria-hidden
    style={{ width: size, height: Math.max(18, size), flexShrink: 0, alignItems: "center", justifyContent: "center" }}>
    {session && status === "running"
      ? <RunningSessionMark colors={colors} size={size} />
      : status === "running"
        ? stale
          ? runningMarkNode({ live: false, visible: true }, colors, size)
          : <ComposerPulse colors={colors} size={size} name="Play" />
        : <Icon name={QUICK_FILTER_ICONS[status]} color={sessionStatusInk(status, colors)} size={size} />}
  </View>;
}

export function SessionStateMarker({ state, colors, size, session = false, stale }: { state: OrganizationState; colors: Colors; size?: number; session?: boolean; stale?: boolean }) {
  const scope = useContext(SessionPulseContext);
  return <SessionStatusIcon status={state === "failed" ? "error" : state} colors={colors} size={size} session={session} stale={badgeStale(stale, scope.live)} />;
}

/** Live runs the shared pulse; last known keeps today's muted ink — the rainbow never claims live. */
export function runningMarkNode({ live, visible }: { live: boolean; visible: boolean }, colors: Colors, size: number) {
  return live
    ? <ComposerPulse name={QUICK_FILTER_ICONS.running} colors={colors} size={size} enabled={visible} />
    : <Icon name={QUICK_FILTER_ICONS.running} color={colors.foregroundMuted} size={size} />;
}

function RunningSessionMark({ colors, size }: { colors: Colors; size: number }) {
  const scope = useContext(SessionPulseContext);
  return runningMarkNode(scope, colors, size);
}
