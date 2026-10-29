import type { OpenSessionResult } from "./session-actions";
import type { TakeoverOutcome } from "../shared/session-lifecycle";
import { useSummaryAttachmentAvailability, SUMMARY_ATTACHMENT_UNAVAILABLE } from "./summary-attachments";
import { useEffect, useRef, useState } from "react";
import { Pressable, Text } from "react-native";
import { Modal, useToast } from "@getpaseo/plugin/client/react-native";
import { useRpc } from "@getpaseo/plugin/client";
import { readCreateOptions, type CreateOptions } from "../shared/session-create";
import type { NavigatorCapability, SummaryRevision } from "../shared/session-navigator";
import { ModelPicker } from "./model-picker";
import { codexDefault } from "./new-lead-defaults";
import type { Colors } from "./page-controls";
import type { SessionAction } from "./session-actions-menu";
import type { DirectoryEntry } from "./session-directory";
import { AttachSheet, SummarySheet } from "./session-summary";
import { readableInk } from "./session-status-ink";
import { seatAction, TAKEOVER_EFFECTS, takeoverBusy, takeoverSummary, TakeoverResults, unresolved, useTakeoverRun, type TakeoverRoute } from "./session-takeover";
import { useNavigatorCapabilities } from "./use-navigator";

const failure = (error: unknown) => error instanceof Error ? error.message : String(error);

type Sheet = { readonly kind: "summary"; readonly source: DirectoryEntry }
  | { readonly kind: "attach"; readonly source: DirectoryEntry; readonly revision: SummaryRevision | null }
  | { readonly kind: "picker"; readonly seats: readonly DirectoryEntry[] }
  | { readonly kind: "results" };

/**
 * Board D2: the model picker for one takeover. Auto stays the default; a chosen model and its settings apply to these
 * seats only. One seat shows its own permission mode; several keep theirs each.
 */
function TakeoverPicker({ seats, disclosure, colors, compact, onUse, onCancel }: { seats: readonly DirectoryEntry[]; disclosure: string; colors: Colors; compact: boolean; onUse: (route: TakeoverRoute) => void; onCancel: () => void }) {
  const read = useRpc(readCreateOptions);
  const [options, setOptions] = useState<CreateOptions | string | null>(null);
  useEffect(() => {
    let live = true;
    read({}).then((value) => { if (live) setOptions(value); }, (error: unknown) => { if (live) setOptions(`Kontor could not read this host's models (${failure(error)}).`); });
    return () => { live = false; };
  }, []);
  const count = seats.length;
  return <Modal title="Take over" open onOpenChange={(open) => { if (!open) onCancel(); }}>
    <Modal.Content scrollable={false} contentContainerStyle={{ padding: 8, gap: 8 }}>
      {options && typeof options === "object" ? <ModelPicker colors={colors} compact={compact} providers={options.providers} projectId={options.catalogProjectId} value={null} codex={codexDefault(options.providers)}
        subtitle={count === 1 ? `For ${seats[0]!.title}` : `For ${count} selected seats`} permissions={count === 1 ? { inherited: seats[0]!.mode ?? null, provider: seats[0]!.provider } : { seats: count }} note={`Only this takeover · Defaults stay unchanged. ${disclosure}`}
        action={(alias) => `Take over${count === 1 ? "" : ` ${count}`}${alias ? ` with ${alias}` : ""}`} onAuto={() => onUse(null)}
        onUse={(choice) => onUse({ provider: choice.provider, model: choice.model, effort: choice.effort, featureValues: choice.featureValues })} onCancel={onCancel} />
        : <Text role="status" style={{ color: options ? colors.statusWarning : colors.foregroundMuted, fontSize: 12, padding: 8 }}>{options ?? "Reading this host's models…"}</Text>}
    </Modal.Content>
  </Modal>;
}

/**
 * Take over, Summarize & copy and Attach summary for one host's sessions, shared by the switcher and Kontor's
 * Sessions: the row menu's actions, the selection bar's takeover, and the sheets they open. What the host's server
 * does not support stays listed with its reason. `newLead` opens New lead with a revision pinned, or says why this
 * surface cannot.
 */
export function useSessionOperations({ hostId, online, colors, compact, entries, opened, newLead, onTakeoverStart, onOpenTakeover }: {
  hostId: string; online: boolean; colors: Colors; compact: boolean;
  /** This host's known sessions, the destinations Attach offers. */
  entries: readonly DirectoryEntry[];
  opened: ReadonlyMap<string, number>;
  newLead: ((source: DirectoryEntry, revision: SummaryRevision) => void) | string;
  /** Called once a takeover starts, Auto or chosen, so a selection can end only then; cancelling the picker keeps it. */
  onTakeoverStart?: () => void;
  onOpenTakeover?: (outcome: TakeoverOutcome) => OpenSessionResult;
}) {
  const capabilities = useNavigatorCapabilities(hostId);
  const toast = useToast();
  const [sheet, setSheet] = useState<Sheet | null>(null);
  const resultsOpen = useRef(false);
  resultsOpen.current = sheet?.kind === "results";
  // What a takeover's host states up front; every surface that starts one shows it, and only these are confirmed at once.
  const disclosed = capabilities?.takeover.supported ? capabilities.takeover.limits ?? [] : [];
  const disclosure = [TAKEOVER_EFFECTS, ...disclosed].join(" ");
  // A run that finishes after its results closed still reports, once.
  const takeover = useTakeoverRun(hostId, disclosed, (run) => { if (!resultsOpen.current) toast.show(`Takeover finished: ${takeoverSummary(run.seats)}.`); }, onOpenTakeover);
  useEffect(() => { setSheet(null); }, [hostId]);
  const reason = (key: NavigatorCapability) => !capabilities ? "Reading what this host supports…" : capabilities[key].supported ? null : capabilities[key].reason;
  const offline = online ? null : "This host is offline.";
  const running = takeover.run?.running ? takeover.busy : null;
  /** Why these seats cannot be taken over together now; each seat's own refusal is its result. */
  const takeoverUnavailable = (seats: readonly DirectoryEntry[]) => {
    return offline ?? takeover.busy ?? reason("takeover") ?? (seats.length > 1 ? reason("takeoverBulk") : null);
  };
  // Read when pressed, not when drawn: a picker opened earlier, or another surface of this host, may meet unresolved seats.
  const start = (seats: readonly DirectoryEntry[], route: TakeoverRoute) => {
    const busy = takeoverBusy(hostId);
    if (busy) toast.show(busy);
    else { onTakeoverStart?.(); void takeover.start(seats, route); }
    setSheet({ kind: "results" });
  };
  // A seat whose takeover archived it, may have had an effect or waits on its review: its menu leads to that, never a fresh start.
  const pending = takeover.run?.seats.filter(unresolved) ?? [];
  const settle = (id: string) => { setSheet({ kind: "results" }); takeover.act(id); };
  const summaryUnavailable = offline ?? reason("summaries");
  const nativeAttachments = useSummaryAttachmentAvailability();
  const attachUnavailable = offline ?? (!nativeAttachments ? SUMMARY_ATTACHMENT_UNAVAILABLE : null);
  const actions = (entry: DirectoryEntry): SessionAction[] => {
    const seat = pending.find((one) => one.id === entry.id), next = seat && seatAction(seat);
    return [
      // A held review is confirmed or declined only in its results, where its limits are listed; Check and Finish run from here.
      seat ? { key: "takeover", label: next?.kind === "confirm" ? "Review takeover" : next?.label ?? "Takeover unfinished", icon: next?.kind === "check" ? "CircleHelp" : next?.kind === "confirm" ? "ShieldAlert" : "RefreshCw",
        detail: seat.line, footer: seat.detail ?? undefined, unavailable: offline ?? running ?? (next ? null : seat.detail ?? "Nothing here can finish this takeover."),
        perform: () => next?.kind === "confirm" ? setSheet({ kind: "results" }) : settle(entry.id) }
      : { key: "takeover", label: "Take over", icon: "RefreshCw", detail: "Continue in a replacement session", caption: "Auto uses the first available model in the chain.",
        footer: disclosure, unavailable: takeoverUnavailable([entry]), perform: () => start([entry], null),
        option: { label: "Auto", accessibilityLabel: `Choose a model for taking over ${entry.title}`, unavailable: reason("takeoverRoute"), perform: () => setSheet({ kind: "picker", seats: [entry] }) } },
      { key: "summarize", label: "Summarize & copy", icon: "FileText", unavailable: summaryUnavailable, perform: () => setSheet({ kind: "summary", source: entry }) },
      { key: "attach", label: "Attach summary…", icon: "GitFork", unavailable: attachUnavailable, perform: () => setSheet({ kind: "attach", source: entry, revision: null }) },
    ];
  };
  const close = () => setSheet(null);
  const layer = sheet?.kind === "summary" ? <SummarySheet source={sheet.source} colors={colors} attachUnavailable={attachUnavailable} onClose={close}
      onAttach={(revision) => setSheet({ kind: "attach", source: sheet.source, revision })} />
    : sheet?.kind === "attach" ? <AttachSheet source={sheet.source} revision={sheet.revision} colors={colors} onClose={close} />
    : sheet?.kind === "picker" ? <TakeoverPicker seats={sheet.seats} disclosure={disclosure} colors={colors} compact={compact} onCancel={close} onUse={(route) => start(sheet.seats, route)} />
    : sheet?.kind === "results" && takeover.run ? <TakeoverResults run={takeover.run} colors={colors} onRetry={takeover.retry} onSeat={takeover.act} onDecline={takeover.decline} onClose={() => { close(); takeover.clear(); }} />
    : null;
  // Unresolved seats outlive their sheet; this reopens them from any surface of the host, including archived predecessors.
  const notice = pending.length && sheet === null ? <Pressable testID="takeover-unfinished" accessibilityRole="button" accessibilityLabel={`${takeoverSummary(pending)}. Open takeover results`}
    onPress={() => setSheet({ kind: "results" })} style={{ minHeight: 44, justifyContent: "center", paddingHorizontal: 8 }}>
    <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 12, fontWeight: "600" }}>{`Takeover unfinished: ${takeoverSummary(pending)} · Open results`}</Text>
  </Pressable> : null;
  return {
    actions, layer, notice,
    /** The selection bar's takeover of `seats`: Auto at once, or the picker for this operation only. */
    takeover: (seats: readonly DirectoryEntry[]) => ({
      unavailable: takeoverUnavailable(seats), auto: reason("takeoverRoute"), disclosure,
      start: () => { if (!takeoverUnavailable(seats)) start(seats, null); }, choose: () => { if (!takeoverUnavailable(seats)) setSheet({ kind: "picker", seats }); },
    }),
    /** Whole-session totals may be read on this host. */
    totals: capabilities?.usageTotals.supported === true,
  };
}
