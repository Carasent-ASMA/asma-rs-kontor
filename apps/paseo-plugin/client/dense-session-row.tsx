import { useEffect, useRef, type ReactNode } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { useActionHelp } from "./control-help";
import { spaceActivates, type Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import { sessionState, STATE_LABELS } from "./session-organization";
import { compactAge, lastActivity, rowOrigin, recordedChildUsageDetail, tokenTotals, type SessionTotals } from "./session-row-format";
import { SessionStateMarker } from "./session-state-marker";
import { readableInk } from "./session-status-ink";
import { exactTime, parseTime, type Clock } from "./session-time";
import { rowModel } from "./session-usage";
import { VendorMark, vendorName } from "./vendor-mark";

/** Two taps on the state icon within this window enter selection; one tap waits it out, then opens. */
const DOUBLE_TAP_MS = 280;
/**
 * The one state-icon open waiting out its window, shared by every row: a later intent anywhere, another row's press,
 * a menu opening or a selection, drops it, so no row opens after the operator moved on.
 */
let delayed: { readonly owner: object; readonly timer: ReturnType<typeof setTimeout> } | null = null;
/** Drops the waiting open: any row's when `owner` is absent, else only that row's. Menus and selection call it too. */
export function cancelDelayedOpen(owner?: object): void {
  if (!delayed || (owner && delayed.owner !== owner)) return;
  clearTimeout(delayed.timer);
  delayed = null;
}
/**
 * The row's type scale: the title leads, the origin is about half its size, the model and totals sit between, each
 * with its own tight line. Targets stay at 44px; only the content inside them is dense. Full values are in help.
 */
const TITLE = { fontSize: 13, lineHeight: 16 }, OPENED = { fontSize: 7, lineHeight: 9 }, ORIGIN = { fontSize: 6, lineHeight: 8 }, MODEL = { fontSize: 8, lineHeight: 10 };
const TARGET = 44;

/**
 * - `open`: a tap opens the session; `actions` is the trailing menu.
 * - `select`: a tap toggles it; a checkbox stands on the right.
 * - `pick`: a tap chooses it as a source or destination; a radio stands on the right.
 */
export type DenseRowMode = { readonly kind: "open"; readonly actions?: ReactNode }
  | { readonly kind: "select" | "pick"; readonly selected: boolean; /** Why it cannot be chosen now; the row stays listed and says so. */ readonly unavailable?: string | null; readonly pending?: boolean };

/** Every time the row abbreviates, written out exactly with its zone, for Details. */
export function rowTimeLines(entry: DirectoryEntry, opened: number | null, clock: Clock): { readonly label: string; readonly value: string }[] {
  const activity = lastActivity(entry), updated = parseTime(entry.updatedAt);
  return [
    { label: "Opened from Kontor on this device", value: opened === null ? "not recorded" : exactTime(opened, clock.zone) },
    { label: "Last activity", value: activity ? `${exactTime(activity.at, clock.zone)} (${activity.source})` : "none recorded" },
    { label: "Metadata updated", value: updated === null ? "unknown" : exactTime(updated, clock.zone) },
  ];
}

/** The same times in one line, for hold help and the spoken label. */
export const rowTimes = (entry: DirectoryEntry, opened: number | null, clock: Clock) => rowTimeLines(entry, opened, clock).map(({ label, value }) => `${label}: ${value}`).join(". ");

/**
 * One session in three lines: state, title, opened age and the trailing control; the origin; the model and the
 * session's token totals. The same row serves the switcher, Kontor's flat list and tree, and the session pickers.
 * Holding the row body enters selection, holding the state icon explains it, and double-tapping the state icon also
 * enters selection without opening the session.
 */
export function DenseSessionRow({ entry, colors, clock, opened, current = false, highlighted = false, totals, note = null, spokenDetail = null, mode = { kind: "open" }, onPress, onSelectStart, testID }: {
  entry: DirectoryEntry; colors: Colors; clock: Clock;
  /** When this device last opened the session from Kontor; null when it never did. */
  opened: number | null;
  /** The session this surface was opened from. */
  current?: boolean;
  /** The result a user asked to see in the tree. */
  highlighted?: boolean;
  /** Whole-session totals from a supported source; null or absent is unknown, never zero. */
  totals?: SessionTotals | null;
  /** A fourth, muted line for what this surface must still say, such as history or availability; also spoken. */
  note?: string | null;
  /** Spoken after the row's own label, such as the exact native ID and status. */
  spokenDetail?: string | null;
  mode?: DenseRowMode;
  onPress(): void;
  onSelectStart?(): void;
  testID?: string;
}) {
  const state = sessionState(entry);
  const { title, origin } = rowOrigin(entry, clock.now);
  const model = rowModel(entry);
  const tokens = tokenTotals(totals);
  const recordedChild = recordedChildUsageDetail(totals);
  const times = rowTimes(entry, opened, clock);
  const catalogCopy = entry.catalogCopyAt ? `catalog copy · as of ${entry.catalogCopyAt}` : null;
  const visibleNote = [note, catalogCopy].filter(Boolean).join(" · ") || null;
  const openedAge = current ? "now" : compactAge(opened, clock.now);
  const stateHelp = useActionHelp(`${STATE_LABELS[state]}. ${times}.${onSelectStart ? " Double-tap to select sessions." : ""}`);
  // A single tap on the state icon opens only after the double-tap window. Any press on any row drops that open; a
  // change of this row's mode or its unmounting drops only its own, so it can never land late or twice.
  const self = useRef({}).current;
  const cancelOpen = () => cancelDelayedOpen();
  useEffect(() => () => cancelDelayedOpen(self), [mode.kind]);
  const tapState = () => {
    stateHelp.dismiss();
    if (!onSelectStart || mode.kind !== "open") { cancelOpen(); onPress(); return; }
    if (delayed?.owner === self) { cancelOpen(); onSelectStart(); return; }
    cancelOpen();
    delayed = { owner: self, timer: setTimeout(() => { delayed = null; onPress(); }, DOUBLE_TAP_MS) };
  };
  const choice = mode.kind === "open" ? null : mode.selected;
  const unavailable = mode.kind === "open" ? null : mode.unavailable ?? null;
  const spoken = [`${entry.title}. ${STATE_LABELS[state]}. ${origin.join(", ")}. ${vendorName(model.vendor)} ${model.full}${model.effort ? `, effort ${model.effort}` : ""}. ${tokens.spoken}. ${times}.`,
    visibleNote, spokenDetail, unavailable].filter(Boolean).join(" ");
  const press = ({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({
    flex: 1, minWidth: 0, minHeight: TARGET, flexDirection: "row" as const, alignItems: "center" as const, gap: 8, paddingVertical: 3, paddingRight: 4, borderRadius: 6,
    borderWidth: 1, borderColor: focused ? colors.foreground : "transparent",
    backgroundColor: pressed || hovered || focused || choice ? colors.surface2 : undefined,
    ...(focused && Platform.OS === "web" ? { outlineStyle: "solid" as const, outlineWidth: 2, outlineColor: colors.accent } : {}),
  });
  const marked = current || highlighted;
  return <View testID={testID ?? `session-row-${entry.id}`} style={{ flexDirection: "row", alignItems: "flex-start", borderRadius: 6,
    backgroundColor: marked ? colors.surface2 : undefined, borderLeftWidth: marked ? 2 : 0, borderColor: colors.accent }}>
    <Pressable {...stateHelp.bindings} testID="session-row-state" accessibilityRole="button"
      accessibilityLabel={`${STATE_LABELS[state]}: ${entry.title}`} accessibilityHint={onSelectStart ? "Double-tap to select sessions" : undefined}
      onPress={tapState} style={{ width: TARGET, minHeight: TARGET, alignItems: "center", justifyContent: "center" }}>
      <SessionStateMarker state={state} colors={colors} size={22} session />
    </Pressable>
    <Pressable testID="session-row-body" role={mode.kind === "open" ? "button" : mode.kind === "select" ? "checkbox" : "radio"}
      accessibilityLabel={spoken} aria-checked={choice ?? undefined} aria-disabled={unavailable ? true : undefined}
      accessibilityState={choice === null ? undefined : { checked: choice, disabled: !!unavailable, busy: mode.kind !== "open" && !!mode.pending }}
      // A checkbox or radio row toggles on Space, as the earlier checkbox did; one that cannot be chosen stays in Tab so its reason is heard.
      {...(Platform.OS === "web" && mode.kind !== "open" ? { ...spaceActivates(() => { cancelOpen(); onPress(); }, !!unavailable), tabIndex: 0 } : {})}
      disabled={!!unavailable} onPress={() => { cancelOpen(); onPress(); }} onLongPress={onSelectStart ? () => { cancelOpen(); onSelectStart(); } : undefined} delayLongPress={450} style={press}>
      <View style={{ flex: 1, minWidth: 0, gap: 1 }}>
        <View style={{ flexDirection: "row", alignItems: "baseline", gap: 6 }}>
          <Text numberOfLines={1} style={{ ...TITLE, flex: 1, minWidth: 0, color: colors.foreground, fontWeight: current ? "600" : "400" }}>{title}</Text>
          {openedAge ? <Text testID="session-row-opened" style={{ ...OPENED, color: colors.foregroundMuted }}>open {openedAge}</Text> : null}
        </View>
        {origin.length ? <Text testID="session-row-origin" numberOfLines={1} style={{ ...ORIGIN, color: colors.foregroundMuted }}>{origin.join(" · ")}</Text> : null}
        <View testID="session-row-model" style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
          <VendorMark vendor={model.vendor} size={9} colors={colors} />
          <Text numberOfLines={1} style={{ ...MODEL, flexShrink: 1, color: colors.foreground, fontVariant: ["tabular-nums"] }}>{[model.alias, model.effort].filter(Boolean).join(" · ")}</Text>
          <View style={{ flex: 1 }} />
          <Text testID="session-row-tokens" style={{ ...MODEL, color: colors.foregroundMuted, fontVariant: ["tabular-nums"] }}>{tokens.input}   {tokens.output}</Text>
        </View>
        {recordedChild ? <Text testID="session-row-recorded-child-usage" style={{ ...MODEL, color: colors.foregroundMuted }}>{recordedChild}</Text> : null}
        {visibleNote ? <Text testID="session-row-note" numberOfLines={1} style={{ ...MODEL, color: colors.foregroundMuted }}>{visibleNote}</Text> : null}
      </View>
      {/* Inside the body, so tapping the visible box or radio is the same single toggle or choice as the row. */}
      {mode.kind === "open" ? null : <View testID="session-row-choice" importantForAccessibility="no" accessibilityElementsHidden style={{ width: 36, alignSelf: "stretch", alignItems: "center", justifyContent: "center", opacity: unavailable ? 0.4 : 1 }}>
        <Icon name={mode.pending ? "Loader" : mode.kind === "select" ? mode.selected ? "CircleCheck" : "Circle" : mode.selected ? "CircleDot" : "Circle"} size={18} color={mode.selected ? readableInk(colors.accent, colors) : colors.foregroundMuted} />
      </View>}
    </Pressable>
    {/* Watches, never claims, a touch on the trailing actions, so opening the menu drops a pending open. */}
    {mode.kind === "open" && mode.actions ? <View onStartShouldSetResponderCapture={() => { cancelOpen(); return false; }}>{mode.actions}</View> : null}
  </View>;
}
