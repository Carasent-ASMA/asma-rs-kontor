import { useEffect, useRef, useState, type ReactNode, type Ref, type RefObject } from "react";
import { Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import { useActionHelp } from "./control-help";
import { COARSE_POINTER, type Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import { exactTime, parseTime } from "./session-time";

/**
 * Proposed consumer contract, not a published authority API. One owner's answer for one exact native session: for
 * Take over, the governed seat's route preview, which picks the first available route in the seat's model chain; for
 * Retry, the lifecycle row of a same-ID retry. The bar never derives support from role, title, status, chain or a
 * projection; it only checks that an answer is for this host and session, and sends its revision back so the owner
 * can refuse a session that changed after it answered.
 */
export type OwnerAnswer = { readonly hostId: string; readonly agentId: string; readonly revision: string }
  & ({ readonly supported: true; readonly consequence: string } | { readonly supported: false; readonly reason: string });
/** Sent only after Confirm. */
export type OwnerRequest = { readonly hostId: string; readonly agentId: string; readonly revision: string };
/** One line per request, naming the session it reports on; a reply for another host or session is never shown. */
export type OwnerReply = { readonly hostId: string; readonly agentId: string; readonly text: string };
/** `answer` returns `undefined` until it has answered. An action with no owner here stays unavailable. */
export type Owner = { readonly answer: (agentId: string) => OwnerAnswer | undefined; readonly run: (requests: readonly OwnerRequest[]) => Promise<readonly OwnerReply[]> };
export type Owners = { readonly takeOver?: Owner; readonly retry?: Owner };
export type SelectedSession = Pick<DirectoryEntry, "id" | "title" | "status" | "archivedAt">;
type Supported = Extract<OwnerAnswer, { supported: true }>;

export type SelectAction = "archive" | "takeOver" | "retry";
export type ActionState = { readonly enabled: true; readonly line: string; readonly ids: readonly string[]; readonly answers: readonly Supported[] }
  | { readonly enabled: false; readonly line: string };

const ACTIONS: Record<SelectAction, { icon: string; label: string }> = {
  archive: { icon: "Archive", label: "Archive…" },
  takeOver: { icon: "ArrowRightLeft", label: "Take over…" },
  retry: { icon: "RotateCcw", label: "Retry in this session…" },
};
const NONE = "Select a session first.";
const off = (line: string): ActionState => ({ enabled: false, line });
const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;

/** An answer counts only for this host and this exact session; anything else is no answer yet. */
function answerFor(owner: Owner, hostId: string, id: string): OwnerAnswer | undefined {
  const answer = owner.answer(id);
  return answer && answer.hostId === hostId && answer.agentId === id ? answer : undefined;
}

/**
 * Selection grants nothing. Archive keeps its existing review path; Take over and Retry act on exactly one session and
 * are enabled only by their owner's answer, so Select all never takes over a batch. The only local rules refuse, never
 * enable: v1 takes over no running or starting session, and Retry never revives an archived one.
 */
export function selectActions(hostId: string, selected: readonly SelectedSession[], owners: Owners): Record<SelectAction, ActionState> {
  const one = selected.length === 1 ? selected[0]! : null;
  const live = selected.filter((session) => session.archivedAt === null), archived = selected.length - live.length;
  const archive = !selected.length ? off(NONE) : !live.length ? off(selected.length === 1 ? "Already archived." : "All selected sessions are already archived.")
    : { enabled: true as const, line: `Review archive of ${plural(live.length, "session")}, then confirm${archived ? ` · ${archived} already archived` : ""}.`, ids: live.map((session) => session.id), answers: [] };
  return {
    archive,
    takeOver: !selected.length ? off(NONE) : !one ? off("Take over needs exactly one selected session.")
      : one.status === "running" ? off("This session is running; v1 cannot take it over. Stop it in the session or archive it first.")
      : one.status === "initializing" ? off("This session is starting; v1 cannot take it over.")
      : asked(hostId, one, owners.takeOver, "Take over is unavailable: no owner here can pick a route from this seat's model chain.", "Checking whether it can be taken over."),
    retry: !selected.length ? off(NONE) : !one ? off("Retry in this session needs exactly one selected session.")
      : one.archivedAt !== null ? off("Archived: Retry never revives an archived session.")
      : asked(hostId, one, owners.retry, "Retry in this session is not available here.", "Checking whether it can retry in this session."),
  };
}

function asked(hostId: string, one: SelectedSession, owner: Owner | undefined, missing: string, checking: string): ActionState {
  if (!owner) return off(missing);
  const answer = answerFor(owner, hostId, one.id);
  if (!answer) return off(checking);
  return answer.supported ? { enabled: true, line: answer.consequence, ids: [one.id], answers: [answer] } : off(answer.reason);
}

/** The fields of an exact-ID provider refusal read that this line shows; the read and its rules belong to its owner. */
export type RefusalView = {
  readonly kind: "quota" | "auth" | "unavailable" | "unknown"; readonly failureAt: string; readonly observedAt: string;
  readonly read: "fresh" | "stale" | "unreadable"; readonly state: "current" | "history"; readonly recovery: "confirmed" | "not-confirmed";
};
const KIND: Record<RefusalView["kind"], string> = { quota: " (quota)", auth: " (sign-in)", unavailable: " (provider unavailable)", unknown: "" };
const at = (value: string, zone: string) => { const time = parseTime(value); return time === null ? "unknown" : exactTime(time, zone); };

/**
 * One line beside the native status, for an exact read only: never a badge, count or filter. A stale or unreadable
 * read never clears a refusal, and only a confirmed recovery on a fresh read drops "Recovery not confirmed".
 */
export function providerStateText(refusal: RefusalView | "unreadable" | null, zone: string): string | null {
  if (refusal === "unreadable") return "Provider state unknown (read failed)";
  if (!refusal) return null;
  const confirmed = refusal.recovery === "confirmed" && refusal.read === "fresh";
  return `Last refusal${KIND[refusal.kind]}: ${refusal.failureAt === "unknown" ? "time unknown" : at(refusal.failureAt, zone)} (observed ${at(refusal.observedAt, zone)}) · Recovery ${confirmed ? "confirmed" : "not confirmed"}`;
}

export function ProviderStateLine({ refusal, zone, colors }: { refusal: RefusalView | "unreadable" | null; zone: string; colors: Colors }) {
  const text = providerStateText(refusal, zone);
  if (!text) return null;
  const ink = refusal !== "unreadable" && refusal?.state === "current" ? colors.statusDanger : colors.statusWarning;
  return <View testID="session-provider-state" style={{ flexDirection: "row", alignItems: "center", gap: 4 }}>
    <View accessibilityElementsHidden importantForAccessibility="no-hide-descendants"><Icon name="CircleAlert" size={12} color={ink} /></View>
    <Text selectable style={{ flexShrink: 1, color: ink, fontSize: 12, lineHeight: 17 }}>{text}</Text>
  </View>;
}

/**
 * A 44 pt icon action that stays focusable when it does not apply: hover, focus and a deliberate hold show its line,
 * and a tap on an unavailable action shows the reason and runs nothing (`disabled` would hide the reason on web).
 */
export function SelectActionButton({ testID, icon, name, line, enabled, expanded, colors, onPress, ref }: {
  testID: string; icon: string; name: string; line: string; enabled: boolean; expanded?: boolean; colors: Colors; onPress: () => void; ref?: Ref<View>;
}) {
  const text = `${name}. ${line}`, help = useActionHelp(text, ref);
  return <Pressable {...help.bindings} testID={testID} accessibilityRole="button" accessibilityLabel={name} accessibilityHint={line}
    accessibilityState={{ disabled: !enabled, expanded }} aria-disabled={!enabled} aria-expanded={expanded}
    onPress={() => { if (!enabled) { help.hold(); return; } help.dismiss(); onPress(); }}
    style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ width: 44, height: 44, alignItems: "center", justifyContent: "center", borderRadius: 6,
      borderWidth: 1, borderColor: focused ? colors.foreground : colors.border, backgroundColor: pressed && enabled ? colors.surface2 : undefined, opacity: enabled ? 1 : 0.5 })}>
    <View accessibilityElementsHidden importantForAccessibility="no-hide-descendants"><Icon name={icon} size={18} color={enabled ? colors.foreground : colors.foregroundMuted} /></View>
  </Pressable>;
}

function TextButton({ label, accessibilityLabel, primary = false, disabled = false, colors, onPress }: { label: string; accessibilityLabel?: string; primary?: boolean; disabled?: boolean; colors: Colors; onPress: () => void }) {
  return <Pressable accessibilityRole="button" accessibilityLabel={accessibilityLabel ?? label} accessibilityState={{ disabled }} aria-disabled={disabled} disabled={disabled} onPress={onPress}
    style={({ focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: COARSE_POINTER ? 44 : 34, minWidth: 44, paddingHorizontal: 14, borderRadius: 8, alignItems: "center", justifyContent: "center",
      borderWidth: 2, borderColor: focused ? colors.foreground : primary ? "transparent" : colors.border, backgroundColor: primary ? disabled ? colors.surface2 : colors.accent : undefined })}>
    <Text numberOfLines={1} style={{ color: primary ? disabled ? colors.foregroundMuted : colors.accentForeground : colors.foreground, fontSize: 13, fontWeight: "600" }}>{label}</Text>
  </Pressable>;
}

/** One menu row: icon, label and its current line. An unavailable row stays focusable and shows its reason; `press` runs nothing for it. */
function ActionItem({ action, state, secondary = false, colors, onPress }: { action: SelectAction; state: ActionState; secondary?: boolean; colors: Colors; onPress: () => void }) {
  const { icon, label } = ACTIONS[action], ink = state.enabled ? secondary ? colors.foregroundMuted : colors.foreground : colors.foregroundMuted;
  return <Pressable testID={`sessions-select-action-${action}`} role="menuitem" accessibilityLabel={label} accessibilityHint={state.line}
    accessibilityState={{ disabled: !state.enabled }} aria-disabled={!state.enabled} onPress={onPress}
    style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({ minHeight: 44, paddingHorizontal: 10, paddingVertical: 6, borderRadius: 5,
      flexDirection: "row", alignItems: "center", gap: 10, backgroundColor: (pressed && state.enabled) || hovered || focused ? colors.surface2 : undefined })}>
    <View accessibilityElementsHidden importantForAccessibility="no-hide-descendants"><Icon name={icon} size={16} color={ink} /></View>
    <View style={{ flexShrink: 1, gap: 2 }}>
      <Text numberOfLines={1} style={{ color: ink, fontSize: secondary ? 12 : 13, fontWeight: secondary ? "400" : "600" }}>{label}</Text>
      <Text numberOfLines={3} style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 16 }}>{state.line}</Text>
    </View>
  </Pressable>;
}

type Confirm = { readonly action: "takeOver" | "retry"; readonly requests: readonly OwnerRequest[]; readonly line: string };

/**
 * The docked bar for any selected native session, archived ones included: "<n> selected", Select all, one Actions menu
 * (Archive, Take over, and a secondary Retry in this session only when its owner says the session can retry) and Clear.
 * Take over and Retry confirm inline with one consequence line and text buttons; nothing reaches an owner's `run`
 * before Confirm. Changing the selection or host drops an open menu and confirmation and ignores a reply still on its way.
 */
export function SelectActionBar({ hostId, selected, owners, blocked, refusal, zone, colors, anchorRef, root, rootSize, selectAll, onArchive, onClear }: {
  hostId: string; selected: readonly SelectedSession[]; owners: Owners;
  /** Why no action can start right now, such as an open review or an offline host; every action shows it instead. */
  blocked?: string;
  /** The exact read for a single selected session; omit it for none or several. */
  refusal?: RefusalView | "unreadable" | null; zone: string; colors: Colors;
  /** The Actions trigger: anchors its menu and the archive review, and receives focus back from both. */
  anchorRef: RefObject<View | null>; root: RefObject<View | null>; rootSize: MenuSize;
  /** The screen's Select all control; it only changes the selection. */
  selectAll?: ReactNode;
  onArchive: (ids: readonly string[]) => void; onClear: () => void;
}) {
  const [menu, setMenu] = useState(false), [confirm, setConfirm] = useState<Confirm | null>(null), [pending, setPending] = useState(false), [result, setResult] = useState<string[] | null>(null);
  const token = useRef(0);
  const scope = `${hostId}\n${selected.map((session) => session.id).join("\n")}`;
  useEffect(() => { token.current++; setMenu(false); setConfirm(null); setPending(false); setResult(null); }, [scope]);
  // Asks each owner again on every call: a render's answers can be stale by the time Confirm is pressed.
  const read = () => blocked ? { archive: off(blocked), takeOver: off(blocked), retry: off(blocked) } : selectActions(hostId, selected, owners);
  const states = read();
  const press = (action: SelectAction) => {
    const state = states[action];
    if (!state.enabled) return;
    setMenu(false);
    if (action === "archive") onArchive(state.ids);
    else { setResult(null); setConfirm({ action, line: state.line, requests: state.answers.map(({ hostId, agentId, revision }) => ({ hostId, agentId, revision })) }); }
  };
  /** An open confirmation holds only while its action still applies to the same answer it showed: same host,
   *  session, revision and consequence. Otherwise it shows the current reason and calls no owner. `run` checks a
   *  fresh owner answer, not the one this render drew. */
  const invalid = (open: Confirm, now = states): string | null => {
    const state = now[open.action];
    if (!state.enabled) return state.line;
    const same = state.line === open.line && state.answers.length === open.requests.length && state.answers.every((answer, index) => {
      const request = open.requests[index]!;
      return answer.hostId === request.hostId && answer.agentId === request.agentId && answer.revision === request.revision;
    });
    return same ? null : "This changed after you opened it. Cancel and choose the action again.";
  };
  const run = async (current: Confirm) => {
    const now = read(), reason = invalid(current, now);
    if (reason) {
      // Closes the confirmation now, since a changed owner answer need not redraw the bar; the menu then carries the current state.
      setConfirm(null);
      setResult([`Nothing was sent. ${now[current.action].enabled ? "This changed after you opened it; choose the action again to review it." : reason}`]);
      return;
    }
    const mine = ++token.current;
    setPending(true);
    let lines: string[];
    try {
      const replies = await owners[current.action]!.run(current.requests);
      // One reply per requested session; several for the same one stay unknown rather than picking a winner.
      lines = current.requests.map((request) => {
        const replied = replies.filter((value) => value.hostId === request.hostId && value.agentId === request.agentId);
        return replied.length === 1 ? replied[0]!.text : replied.length ? "Result unknown: more than one reply for this session. Check the session before trying again."
          : "No result reported. Check the session before trying again.";
      });
    } catch (error) {
      lines = [`Result unknown: ${error instanceof Error ? error.message : String(error)}. Check the session before trying again.`];
    }
    if (mine !== token.current) return;
    setPending(false); setConfirm(null); setResult(lines);
  };
  const stale = confirm && !pending ? invalid(confirm) : null;
  const name = selected[0]?.title ?? "";
  const label = `Actions for ${plural(selected.length, "selected session")}`;
  const line = blocked ?? (!selected.length ? NONE : pending ? "Wait for the current action to finish." : "Archive or take over the selection.");
  return <View testID="sessions-select-actions" role="toolbar" accessibilityLabel="Selected session actions" style={{ gap: 6 }}>
    <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
      {selectAll}
      <Text style={{ color: colors.foreground, fontSize: 13, fontWeight: "600", fontVariant: ["tabular-nums"] }}>{`${selected.length} selected`}</Text>
      <View style={{ flexGrow: 1 }} />
      <SelectActionButton testID="sessions-select-menu-trigger" icon="Ellipsis" name={label} line={line} enabled={!blocked && !pending && selected.length > 0} expanded={menu}
        colors={colors} ref={anchorRef} onPress={() => setMenu((open) => !open)} />
      <SelectActionButton testID="sessions-select-action-clear" icon="X" name="Clear selection" line={blocked ?? "Clear the selection; nothing runs."} enabled={!pending && !blocked} colors={colors} onPress={onClear} />
    </View>
    {menu ? <AnchoredMenu testID="sessions-select-menu" anchor={anchorRef} root={root} rootSize={rootSize} colors={colors} label={label} onClose={() => setMenu(false)} placement={{ align: "end", width: 320 }}>
      <ActionItem action="archive" state={states.archive} colors={colors} onPress={() => press("archive")} />
      <ActionItem action="takeOver" state={states.takeOver} colors={colors} onPress={() => press("takeOver")} />
      {states.retry.enabled ? <>
        <View style={{ height: 1, backgroundColor: colors.border, marginVertical: 4 }} />
        <ActionItem action="retry" state={states.retry} secondary colors={colors} onPress={() => press("retry")} />
      </> : null}
    </AnchoredMenu> : null}
    {selected.length === 1 && refusal !== undefined ? <ProviderStateLine refusal={refusal} zone={zone} colors={colors} /> : null}
    {confirm ? <View testID="sessions-select-confirm" style={{ gap: 6, padding: 10, borderWidth: 1, borderColor: colors.border, borderRadius: 8, backgroundColor: colors.surface1 }}>
      <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{confirm.action === "takeOver" ? `Take over ${name}?` : `Retry ${name} in this session?`}</Text>
      <Text style={{ color: stale ? colors.statusWarning : colors.foreground, fontSize: 13, lineHeight: 19 }}>{stale ?? confirm.line}</Text>
      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        <TextButton label="Cancel" colors={colors} disabled={pending} onPress={() => { token.current++; setConfirm(null); }} />
        <TextButton primary label={pending ? "Sending…" : confirm.action === "takeOver" ? "Take over" : "Retry"} colors={colors} disabled={pending || !!stale} onPress={() => { void run(confirm); }} />
      </View>
    </View> : null}
    {result ? <Text testID="sessions-select-result" role="status" accessibilityLiveRegion="polite" selectable style={{ color: colors.foreground, fontSize: 12, lineHeight: 17 }}>{result.join("\n")}</Text> : null}
  </View>;
}
