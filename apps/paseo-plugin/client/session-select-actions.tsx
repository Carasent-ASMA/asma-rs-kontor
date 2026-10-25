import { useEffect, useRef, useState, type ReactNode, type Ref, type RefObject } from "react";
import { Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import { useActionHelp } from "./control-help";
import { COARSE_POINTER, type Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import { exactTime, parseTime } from "./session-time";

/**
 * Proposed consumer contract, not a published authority API. Each owner answers for one exact native session, and the
 * bar never derives support from role, title, status, chain or a projection.
 * - Take over: the lifecycle producer reviews only when the person chooses Take over, never on selection or render.
 *   Its review picks the first available route from the session's recorded model chain (a direct session through the
 *   plugin server, a governed seat through its authorized writer).
 * - Retry in this session: the lifecycle row's own same-ID retryability, read without side effects.
 * Confirm sends back the review and revision it showed, so the owner refuses anything that drifted since.
 */
export type OwnerAnswer = { readonly hostId: string; readonly agentId: string; readonly reviewId: string; readonly revision: string }
  & ({ readonly supported: true; readonly consequence: string } | { readonly supported: false; readonly reason: string });
/** Sent only after Confirm, bound to the review and revision the confirmation showed. */
export type OwnerRequest = { readonly hostId: string; readonly agentId: string; readonly reviewId: string; readonly revision: string };
/**
 * The reply names the session it reports on; a reply for another host or session is never shown. `partial` is a
 * Take over that retired the session without a confirmed new one; `unconfirmed` is still reconciling.
 */
export type OwnerReply = { readonly hostId: string; readonly agentId: string; readonly outcome: "succeeded" | "partial" | "refused" | "unconfirmed";
  readonly successorId?: string; readonly text: string };
export type TakeOverOwner = { readonly review: (hostId: string, agentId: string) => Promise<OwnerAnswer>; readonly apply: (request: OwnerRequest) => Promise<OwnerReply> };
/** `answer` returns `undefined` until it has read the row. */
export type RetryOwner = { readonly answer: (agentId: string) => OwnerAnswer | undefined; readonly apply: (request: OwnerRequest) => Promise<OwnerReply> };
export type Owners = { readonly takeOver?: TakeOverOwner; readonly retry?: RetryOwner };
export type SelectedSession = Pick<DirectoryEntry, "id" | "title" | "status" | "archivedAt">;

export type SelectAction = "archive" | "takeOver" | "retry";
export type ActionState = { readonly enabled: true; readonly line: string; readonly ids: readonly string[] } | { readonly enabled: false; readonly line: string };

const ACTIONS: Record<SelectAction, { icon: string; label: string }> = {
  archive: { icon: "Archive", label: "Archive…" },
  takeOver: { icon: "ArrowRightLeft", label: "Take over…" },
  retry: { icon: "RotateCcw", label: "Retry in this session…" },
};
const NONE = "Select a session first.";
const CHANGED = "This changed after you opened it. Cancel and choose the action again.";
const OUTCOME: Record<OwnerReply["outcome"], string> = { succeeded: "Done", partial: "Partly done", refused: "Refused", unconfirmed: "Not confirmed" };
const off = (line: string): ActionState => ({ enabled: false, line });
const plural = (count: number, one: string, many = `${one}s`) => `${count} ${count === 1 ? one : many}`;
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

/** A retry answer counts only for this host and this exact session; anything else is no answer yet. */
function answerFor(owner: RetryOwner, hostId: string, id: string): OwnerAnswer | undefined {
  const answer = owner.answer(id);
  return answer && answer.hostId === hostId && answer.agentId === id ? answer : undefined;
}

/**
 * Selection grants nothing. Archive keeps its existing review path; Take over and Retry act on exactly one session, so
 * Select all never takes over a batch. Take over with an owner opens its review; Retry needs its owner's answer. The
 * only local rules refuse, never enable: Take over waits for a running or starting session to go idle, and Retry never
 * revives an archived one.
 */
export function selectActions(hostId: string, selected: readonly SelectedSession[], owners: Owners): Record<SelectAction, ActionState> {
  const one = selected.length === 1 ? selected[0]! : null;
  const live = selected.filter((session) => session.archivedAt === null), archived = selected.length - live.length;
  const archive = !selected.length ? off(NONE) : !live.length ? off(selected.length === 1 ? "Already archived." : "All selected sessions are already archived.")
    : { enabled: true as const, line: `Review archive of ${plural(live.length, "session")}, then confirm${archived ? ` · ${archived} already archived` : ""}.`, ids: live.map((session) => session.id) };
  return {
    archive,
    takeOver: !selected.length ? off(NONE) : !one ? off("Take over needs exactly one selected session.")
      : one.status === "running" ? off("This session is running. Stop it in the session, then take it over.")
      : one.status === "initializing" ? off("This session is starting. Wait until it is idle, then take it over.")
      : !owners.takeOver ? off("Take over is not available yet: no qualified owner on this host can pick its route and start the new session.")
      : { enabled: true, line: "Review the route its model chain picks, then confirm.", ids: [one.id] },
    retry: !selected.length ? off(NONE) : !one ? off("Retry in this session needs exactly one selected session.")
      : one.archivedAt !== null ? off("Archived: Retry never revives an archived session.")
      : retryState(hostId, one, owners.retry),
  };
}

function retryState(hostId: string, one: SelectedSession, owner: RetryOwner | undefined): ActionState {
  if (!owner) return off("Retry in this session is not available here.");
  const answer = answerFor(owner, hostId, one.id);
  if (!answer) return off("Checking whether it can retry in this session.");
  return answer.supported ? { enabled: true, line: answer.consequence, ids: [one.id] } : off(answer.reason);
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

/** `answer` is null while Take over's review runs; a refusal, a failed review or a stale answer keeps Confirm off. */
type Confirm = { readonly action: "takeOver" | "retry"; readonly agentId: string; readonly answer: OwnerAnswer | null; readonly failure?: string };

/** A Take over counts as done only with one new session that is not the one it replaced. */
function replyLine(action: Confirm["action"], agentId: string, reply: OwnerReply): string {
  if (!Object.hasOwn(OUTCOME, reply.outcome)) return "Result unknown: the owner reported no known outcome. Check the session before trying again.";
  if (action !== "takeOver" || reply.outcome !== "succeeded") return `${OUTCOME[reply.outcome]}: ${reply.text}`;
  return reply.successorId && reply.successorId !== agentId ? `Done: new session ${reply.successorId}. ${reply.text}`
    : "Result unknown: the owner reported success without a new session. Check the session before trying again.";
}

/**
 * The docked bar for any selected native session, archived ones included: "<n> selected", Select all, one Actions menu
 * (Archive, Take over, and a secondary Retry in this session only when its owner says the session can retry) and Clear.
 * Take over asks its owner for a review only when chosen; Take over and Retry confirm inline with one consequence line
 * and text buttons, and nothing reaches an owner's `apply` before Confirm. Changing the selection or host drops an open
 * menu, review and confirmation and ignores an answer or reply still on its way.
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
  const [menu, setMenu] = useState(false), [confirm, setConfirm] = useState<Confirm | null>(null), [pending, setPending] = useState(false), [result, setResult] = useState<string | null>(null);
  const token = useRef(0);
  const scope = `${hostId}\n${selected.map((session) => session.id).join("\n")}`;
  useEffect(() => { token.current++; setMenu(false); setConfirm(null); setPending(false); setResult(null); }, [scope]);
  // Asks the owners again on every call: a render's answers can be stale by the time Confirm is pressed.
  const read = () => blocked ? { archive: off(blocked), takeOver: off(blocked), retry: off(blocked) } : selectActions(hostId, selected, owners);
  const states = read();
  const review = (agentId: string) => {
    const mine = ++token.current, done = (next: Partial<Confirm>) => { if (mine === token.current) setConfirm({ action: "takeOver", agentId, answer: null, ...next }); };
    done({});
    owners.takeOver!.review(hostId, agentId).then(
      (answer) => done(answer.hostId === hostId && answer.agentId === agentId ? { answer } : { failure: "Nothing was sent. The review answered for another session." }),
      (error) => done({ failure: `Nothing was sent. The review failed: ${errorText(error)}.` }));
  };
  const press = (action: SelectAction) => {
    const state = states[action];
    if (!state.enabled) return;
    setMenu(false); setResult(null);
    if (action === "archive") onArchive(state.ids);
    else if (action === "takeOver") review(state.ids[0]!);
    else setConfirm({ action, agentId: state.ids[0]!, answer: answerFor(owners.retry!, hostId, state.ids[0]!) ?? null });
  };
  /** An open confirmation holds only while its action still applies; Retry also needs the same answer it showed: same
   *  review, revision and consequence. Otherwise it shows the current reason and calls no owner. */
  const invalid = (open: Confirm, now = states): string | null => {
    const state = now[open.action];
    if (!state.enabled) return state.line;
    if (open.action === "takeOver" || !open.answer) return null;
    const fresh = answerFor(owners.retry!, hostId, open.agentId), shown = open.answer;
    return fresh?.supported && shown.supported && fresh.reviewId === shown.reviewId && fresh.revision === shown.revision && fresh.consequence === shown.consequence ? null : CHANGED;
  };
  const run = async (current: Confirm) => {
    const answer = current.answer;
    if (!answer?.supported) return;
    const now = read(), reason = invalid(current, now);
    if (reason) {
      // Closes the confirmation now, since a changed owner answer need not redraw the bar; the menu then carries the current state.
      setConfirm(null);
      setResult(`Nothing was sent. ${now[current.action].enabled ? "This changed after you opened it; choose the action again to review it." : reason}`);
      return;
    }
    const mine = ++token.current;
    setPending(true);
    let line: string;
    try {
      const reply = await owners[current.action]!.apply({ hostId, agentId: answer.agentId, reviewId: answer.reviewId, revision: answer.revision });
      line = reply.hostId === hostId && reply.agentId === answer.agentId ? replyLine(current.action, answer.agentId, reply) : "No result reported. Check the session before trying again.";
    } catch (error) {
      line = `Result unknown: ${errorText(error)}. Check the session before trying again.`;
    }
    if (mine !== token.current) return;
    setPending(false); setConfirm(null); setResult(line);
  };
  const stale = confirm && !pending ? invalid(confirm) : null;
  const shown = stale ?? confirm?.failure ?? (!confirm?.answer ? "Reviewing the route its model chain picks…" : confirm.answer.supported ? confirm.answer.consequence : confirm.answer.reason);
  const ready = !!confirm?.answer?.supported && !stale && !pending;
  const name = selected[0] ? `${selected[0].title} (${selected[0].id})` : "";
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
      <Text role="status" accessibilityLiveRegion="polite" style={{ color: confirm.answer?.supported && !stale ? colors.foreground : colors.statusWarning, fontSize: 13, lineHeight: 19 }}>{shown}</Text>
      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        <TextButton label="Cancel" colors={colors} disabled={pending} onPress={() => { token.current++; setConfirm(null); }} />
        <TextButton primary label={pending ? "Sending…" : confirm.action === "takeOver" ? "Take over" : "Retry"} colors={colors} disabled={!ready} onPress={() => { void run(confirm); }} />
      </View>
    </View> : null}
    {result ? <Text testID="sessions-select-result" role="status" accessibilityLiveRegion="polite" selectable style={{ color: colors.foreground, fontSize: 12, lineHeight: 17 }}>{result}</Text> : null}
  </View>;
}
