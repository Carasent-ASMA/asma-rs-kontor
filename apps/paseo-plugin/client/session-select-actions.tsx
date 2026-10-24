import { useEffect, useRef, useState, type Ref } from "react";
import { Pressable, Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { useActionHelp } from "./control-help";
import { COARSE_POINTER, type Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import { exactTime, parseTime } from "./session-time";

/**
 * Proposed consumer contract, not a published authority API. One owner's answer for one exact native session: the
 * lifecycle Resume preflight for Resume, the qualified handoff profile for Take over. The bar never derives support
 * from role, title, status or a projection; it only checks that an answer is for this host and session, and sends
 * its revision back so the owner can refuse a session that changed after it answered.
 */
export type OwnerAnswer = { readonly hostId: string; readonly agentId: string; readonly revision: string }
  & ({ readonly supported: true; readonly consequence: string } | { readonly supported: false; readonly reason: string });
/** Sent only after Confirm. */
export type OwnerRequest = { readonly hostId: string; readonly agentId: string; readonly revision: string };
/** One line per request, naming the session it reports on; a reply for another host or session is never shown. */
export type OwnerReply = { readonly hostId: string; readonly agentId: string; readonly text: string };
/** `answer` returns `undefined` until it has answered. An action with no owner here stays unavailable. */
export type Owner = { readonly answer: (agentId: string) => OwnerAnswer | undefined; readonly run: (requests: readonly OwnerRequest[]) => Promise<readonly OwnerReply[]> };
export type Owners = { readonly resume?: Owner; readonly takeOver?: Owner };
export type SelectedSession = Pick<DirectoryEntry, "id" | "title" | "status" | "archivedAt">;
type Supported = Extract<OwnerAnswer, { supported: true }>;

export type SelectAction = "open" | "archive" | "resume" | "takeOver";
export type ActionState = { readonly enabled: true; readonly line: string; readonly ids: readonly string[]; readonly answers: readonly Supported[] }
  | { readonly enabled: false; readonly line: string };

const ACTIONS: Record<SelectAction, { icon: string; name: string }> = {
  open: { icon: "ExternalLink", name: "Open session" },
  archive: { icon: "Archive", name: "Archive sessions" },
  resume: { icon: "RotateCcw", name: "Resume session" },
  takeOver: { icon: "ArrowRightLeft", name: "Take over session" },
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
 * Selection grants nothing. Open and Archive keep their existing paths (Archive reviews before it runs); Resume and
 * Take over are enabled only by their owner's answer. The only local rules refuse, never enable: v1 takes over no
 * running or starting session, and Resume never revives an archived one.
 */
export function selectActions(hostId: string, selected: readonly SelectedSession[], owners: Owners): Record<SelectAction, ActionState> {
  const one = selected.length === 1 ? selected[0]! : null;
  const live = selected.filter((session) => session.archivedAt === null), archived = selected.length - live.length;
  const open = !selected.length ? off(NONE) : one ? { enabled: true as const, line: `Open ${one.title}.`, ids: [one.id], answers: [] } : off("Select one session to open.");
  const archive = !selected.length ? off(NONE) : !live.length ? off(selected.length === 1 ? "Already archived." : "All selected sessions are already archived.")
    : { enabled: true as const, line: `Review archive of ${plural(live.length, "session")}, then confirm${archived ? ` · ${archived} already archived` : ""}.`, ids: live.map((session) => session.id), answers: [] };
  return { open, archive, resume: resume(hostId, selected, owners.resume), takeOver: takeOver(hostId, one, selected.length, owners.takeOver) };
}

function resume(hostId: string, selected: readonly SelectedSession[], owner: Owner | undefined): ActionState {
  if (!selected.length) return off(NONE);
  if (!owner) return off("Resume is not available here.");
  const reasons: string[] = [], answers: Supported[] = [];
  for (const session of selected) {
    const answer = session.archivedAt === null ? answerFor(owner, hostId, session.id) : undefined;
    if (session.archivedAt !== null) reasons.push("Archived: Resume never revives an archived session.");
    else if (!answer) reasons.push("Checking whether it can resume.");
    else if (answer.supported) answers.push(answer);
    else reasons.push(answer.reason);
  }
  if (!answers.length) return off(selected.length === 1 ? reasons[0]! : `None of the selected sessions can resume: ${reasons[0]}`);
  const line = answers.length === 1 && selected.length === 1 ? answers[0]!.consequence
    : `Resume ${answers.length}${reasons.length ? ` · ${reasons.length} not eligible` : ""}. Each retries in itself on its own route; no new session, no provider change.`;
  return { enabled: true, line, ids: answers.map((answer) => answer.agentId), answers };
}

function takeOver(hostId: string, one: SelectedSession | null, count: number, owner: Owner | undefined): ActionState {
  if (!count) return off(NONE);
  if (!one) return off("Take over needs exactly one selected session.");
  if (one.status === "running") return off("This session is running; v1 cannot take it over. Stop it in the session or archive it first.");
  if (one.status === "initializing") return off("This session is starting; v1 cannot take it over.");
  if (!owner) return off("Take over needs a qualified handoff profile, and none is available yet.");
  const answer = answerFor(owner, hostId, one.id);
  if (!answer) return off("Checking whether it can be taken over.");
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
export function SelectActionButton({ testID, icon, name, line, enabled, colors, onPress, ref }: {
  testID: string; icon: string; name: string; line: string; enabled: boolean; colors: Colors; onPress: () => void; ref?: Ref<View>;
}) {
  const text = `${name}. ${line}`, help = useActionHelp(text, ref);
  return <Pressable {...help.bindings} testID={testID} accessibilityRole="button" accessibilityLabel={name} accessibilityHint={line}
    accessibilityState={{ disabled: !enabled }} aria-disabled={!enabled}
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

type Confirm = { readonly action: "resume" | "takeOver"; readonly requests: readonly OwnerRequest[]; readonly line: string };

/**
 * The docked bar for any selected native session, archived ones included: "<n> selected", four icon actions and Clear.
 * Resume and Take over confirm inline with one consequence line and text buttons; nothing reaches an owner's `run`
 * before Confirm. Changing the selection or host drops an open confirmation and ignores a reply still on its way.
 */
export function SelectActionBar({ hostId, selected, owners, blocked, refusal, zone, colors, archiveRef, onOpen, onArchive, onClear }: {
  hostId: string; selected: readonly SelectedSession[]; owners: Owners;
  /** Why no action can start right now, such as an open review or an offline host; every action shows it instead. */
  blocked?: string;
  /** The exact read for a single selected session; omit it for none or several. */
  refusal?: RefusalView | "unreadable" | null; zone: string; colors: Colors;
  /** Anchors the existing archive review and receives focus back from it. */
  archiveRef?: Ref<View>;
  onOpen: (id: string) => void; onArchive: (ids: readonly string[]) => void; onClear: () => void;
}) {
  const [confirm, setConfirm] = useState<Confirm | null>(null), [pending, setPending] = useState(false), [result, setResult] = useState<string[] | null>(null);
  const token = useRef(0);
  const scope = `${hostId}\n${selected.map((session) => session.id).join("\n")}`;
  useEffect(() => { token.current++; setConfirm(null); setPending(false); setResult(null); }, [scope]);
  const states = blocked ? { open: off(blocked), archive: off(blocked), resume: off(blocked), takeOver: off(blocked) } : selectActions(hostId, selected, owners);
  const press = (action: SelectAction) => {
    const state = states[action];
    if (!state.enabled) return;
    if (action === "open") onOpen(state.ids[0]!);
    else if (action === "archive") onArchive(state.ids);
    else { setResult(null); setConfirm({ action, line: state.line, requests: state.answers.map(({ hostId, agentId, revision }) => ({ hostId, agentId, revision })) }); }
  };
  const run = async (current: Confirm) => {
    const mine = ++token.current;
    setPending(true);
    let lines: string[];
    try {
      const replies = await owners[current.action]!.run(current.requests);
      lines = current.requests.map((request) => {
        const reply = replies.find((value) => value.hostId === request.hostId && value.agentId === request.agentId);
        const title = selected.find((session) => session.id === request.agentId)?.title ?? request.agentId;
        const text = reply?.text ?? "No result reported. Check the session before trying again.";
        return current.requests.length === 1 ? text : `${title}: ${text}`;
      });
    } catch (error) {
      lines = [`Result unknown: ${error instanceof Error ? error.message : String(error)}. Check the session before trying again.`];
    }
    if (mine !== token.current) return;
    setPending(false); setConfirm(null); setResult(lines);
  };
  const title = confirm?.action === "takeOver" ? `Take over ${selected[0]?.title ?? ""}?` : confirm ? `Resume ${confirm.requests.length === 1 ? `${selected.find((session) => session.id === confirm.requests[0]!.agentId)?.title ?? ""}?` : `${confirm.requests.length} sessions?`}` : "";
  return <View testID="sessions-select-actions" role="toolbar" accessibilityLabel="Selected session actions" style={{ gap: 6 }}>
    <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
      <Text style={{ color: colors.foreground, fontSize: 13, fontWeight: "600", fontVariant: ["tabular-nums"] }}>{`${selected.length} selected`}</Text>
      <View style={{ flexGrow: 1 }} />
      {(Object.keys(ACTIONS) as SelectAction[]).map((action) => <SelectActionButton key={action} testID={`sessions-select-action-${action}`} icon={ACTIONS[action].icon} name={ACTIONS[action].name}
        line={states[action].line} enabled={states[action].enabled && !pending} colors={colors} ref={action === "archive" ? archiveRef : undefined} onPress={() => press(action)} />)}
      <SelectActionButton testID="sessions-select-action-clear" icon="X" name="Clear selection" line={blocked ?? "Clear the selection; nothing runs."} enabled={!pending && !blocked} colors={colors} onPress={onClear} />
    </View>
    {selected.length === 1 && refusal !== undefined ? <ProviderStateLine refusal={refusal} zone={zone} colors={colors} /> : null}
    {confirm ? <View testID="sessions-select-confirm" style={{ gap: 6, padding: 10, borderWidth: 1, borderColor: colors.border, borderRadius: 8, backgroundColor: colors.surface1 }}>
      <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>{title}</Text>
      <Text style={{ color: colors.foreground, fontSize: 13, lineHeight: 19 }}>{confirm.line}</Text>
      <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8 }}>
        <TextButton label="Cancel" colors={colors} disabled={pending} onPress={() => { token.current++; setConfirm(null); }} />
        <TextButton primary label={pending ? "Sending…" : confirm.action === "takeOver" ? "Take over" : "Resume"} colors={colors} disabled={pending} onPress={() => { void run(confirm); }} />
      </View>
    </View> : null}
    {result ? <Text testID="sessions-select-result" role="status" accessibilityLiveRegion="polite" selectable style={{ color: colors.foreground, fontSize: 12, lineHeight: 17 }}>{result.join("\n")}</Text> : null}
  </View>;
}
