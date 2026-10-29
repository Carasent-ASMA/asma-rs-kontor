import { useEffect, useRef, useState } from "react";
import { Text, View } from "react-native";
import { copyText, Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { useRpc } from "@getpaseo/plugin/client";
import { latestSummaries, readSummary, saveSummary, summaryRevisionSchema, type SummaryRevision } from "../shared/session-navigator";
import { DenseSessionRow } from "./dense-session-row";
import { OperationSheet, SheetFooter } from "./operation-sheet";
import { type Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import { exactTime, parseTime, useClock } from "./session-time";
import { SUMMARY_ATTACHMENT_STEPS, SUMMARY_ATTACHMENT_LIMIT, summaryAttachmentIdentifier } from "../shared/summary-attachments";
import { useSummaryAttachmentAvailability, SUMMARY_ATTACHMENT_UNAVAILABLE } from "./summary-attachments";

const failure = (error: unknown) => error instanceof Error ? error.message : String(error);

/** Saves a revision of `source` once per sheet; null while saving. A failed save is a problem, never an empty summary. */
function useSavedSummary(source: DirectoryEntry, given: SummaryRevision | null) {
  const save = useRpc(saveSummary);
  const [state, setState] = useState<{ revision: SummaryRevision | null; problem: string | null } | null>(given ? { revision: given, problem: null } : null);
  const [attempt, setAttempt] = useState(0);
  useEffect(() => {
    if (given) return;
    let live = true;
    setState(null);
    save({ agentId: source.id }).then((revision) => { if (live) setState(revision.sourceAgentId === source.id ? { revision, problem: null } : { revision: null, problem: "The saved summary belongs to another source. Nothing was copied." }); }, (error: unknown) => { if (live) setState({ revision: null, problem: failure(error) }); });
    return () => { live = false; };
  }, [source.id, attempt]);
  return { state: state?.revision && state.revision.sourceAgentId !== source.id ? null : state, retry: () => setAttempt((value) => value + 1) };
}

function SourceChip({ revision, colors }: { revision: SummaryRevision; colors: Colors }) {
  const clock = useClock(), cutoff = parseTime(revision.cutoff);
  return <View testID="summary-source" accessibilityLabel={`Source ${revision.sourceTitle}, saved summary r${revision.revision}, covers activity until ${cutoff === null ? revision.cutoff : exactTime(cutoff, clock.zone)}`}
    style={{ gap: 2, paddingHorizontal: 10, paddingVertical: 6, borderRadius: 8, borderWidth: 1, borderColor: colors.border }}>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
      <Icon name="FileText" size={16} color={colors.foreground} />
      <Text numberOfLines={1} style={{ flex: 1, color: colors.foreground, fontSize: 12, fontWeight: "600" }}>{`${revision.sourceTitle} · r${revision.revision}`}</Text>
    </View>
    <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{`Covers activity until ${cutoff === null ? revision.cutoff : exactTime(cutoff, clock.zone)}`}</Text>
  </View>;
}

/**
 * Board C2: saves a compact summary revision of `source`, then copies it. Copied is said only after the clipboard took
 * it; a refusal keeps the saved summary on screen with Copy again. Copy again LEFT, Attach to another seat RIGHT.
 */
export function SummarySheet({ source, colors, attachUnavailable, onAttach, onClose }: {
  source: DirectoryEntry; colors: Colors;
  /** Why Attach cannot run on this host; null when it can. */
  attachUnavailable: string | null;
  onAttach: (revision: SummaryRevision) => void; onClose: () => void;
}) {
  const { state, retry } = useSavedSummary(source, null);
  const read = useRpc(readSummary);
  const [copy, setCopy] = useState<{ summaryId: string; revision: number; problem: string | null } | null>(null);
  const [verification, setVerification] = useState<{ key: string; text: string } | null>(null);
  const writing = useRef<string | null>(null);
  const activeSource = useRef(source.id);
  activeSource.current = source.id;
  const write = async (revision: SummaryRevision) => {
    const key = `${revision.summaryId}:${revision.revision}`;
    if (writing.current === key) return;
    writing.current = key;
    setCopy(null); setVerification(null);
    try {
      const stored = await read({ summaryId: revision.summaryId, revision: revision.revision });
      if (stored.summaryId !== revision.summaryId || stored.revision !== revision.revision || stored.sourceAgentId !== revision.sourceAgentId || stored.text !== revision.text) throw new Error("The saved revision readback did not match. Nothing was copied.");
      if (activeSource.current !== revision.sourceAgentId || writing.current !== key) return;
      await copyText(stored.text);
      if (activeSource.current === revision.sourceAgentId && writing.current === key) setCopy({ summaryId: revision.summaryId, revision: revision.revision, problem: null });
    } catch (error) {
      if (activeSource.current === revision.sourceAgentId && writing.current === key) setCopy({ summaryId: revision.summaryId, revision: revision.revision, problem: failure(error) });
    } finally { if (writing.current === key) writing.current = null; }
  };
  const revision = state?.revision ?? null;
  useEffect(() => { if (revision) void write(revision); }, [revision?.summaryId, revision?.revision]);
  const copied = revision && copy?.summaryId === revision.summaryId && copy.revision === revision.revision ? copy : null;
  const muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 } as const;
  const banner = !state ? `Saving a summary of ${source.title}…` : !revision ? `The summary was not saved (${state.problem}). Nothing was copied.`
    : !copied ? `Saved r${revision.revision} · Copying…` : copied.problem === null ? `Copied to clipboard · Saved r${revision.revision} · Source linked`
    : `Saved r${revision.revision} · Not copied: ${copied.problem}. Use Copy again, or select the text below.`;
  const sections = revision ? [["Task", revision.sections.task], ["Decisions", revision.sections.decisions], ["Current state", revision.sections.currentState], ["Next step", revision.sections.nextStep]] : [];
  return <OperationSheet testID="summary-sheet" title="Session summary" subtitle={source.title} colors={colors} onClose={onClose}
    footer={revision ? <SheetFooter colors={colors} note={copied?.problem === null ? "A saved summary is ready to paste." : null}
      secondary={{ testID: "summary-copy-again", label: "Copy again", onPress: () => { void write(revision); } }}
      primary={{ testID: "summary-attach", label: "Choose attachment in destination…", disabled: !!attachUnavailable, hint: attachUnavailable, onPress: () => onAttach(revision) }} />
      : <SheetFooter colors={colors} secondary={{ label: "Close", onPress: onClose }} primary={state ? { label: "Try again", onPress: retry } : null} />}>
    <Text style={muted}>A separate read-only Codex summary session uses recent source history only. Source and destination work are not prompted.</Text>
    <View testID="summary-banner" role="status" accessibilityLiveRegion="polite" style={{ flexDirection: "row", alignItems: "flex-start", gap: 8, padding: 10, borderRadius: 8, borderWidth: 1,
      borderColor: copied?.problem === null ? colors.accent : copied || (state && !revision) ? colors.statusWarning : colors.border }}>
      {copied ? <Icon name={copied.problem === null ? "CircleCheck" : "CircleAlert"} size={16} color={copied.problem === null ? colors.statusSuccess : colors.statusWarning} /> : null}
      <Text style={{ flex: 1, color: state && !revision ? colors.statusWarning : colors.foreground, fontSize: 13, lineHeight: 18, fontWeight: "600" }}>{banner}</Text>
    </View>
    {sections.map(([heading, text]) => <View key={heading} style={{ gap: 2 }}>
      <Text accessibilityRole="header" style={{ ...muted, fontWeight: "600" }}>{heading}</Text>
      <Text selectable style={{ color: colors.foreground, fontSize: 13, lineHeight: 18 }}>{text}</Text>
    </View>)}
    {revision ? <SourceChip revision={revision} colors={colors} /> : null}
    {revision && attachUnavailable ? <Text style={muted}>{attachUnavailable}</Text> : null}
    {revision && copied?.problem === null ? <View style={{ gap: 4 }}>
      <Text style={muted}>Clipboard readback is not automatic. Paste below to compare with this saved revision.</Text>
      <TextInput testID="summary-paste-verification" accessibilityLabel="Paste copied summary to verify" multiline
        value={verification?.key === `${revision.summaryId}:${revision.revision}` ? verification.text : ""}
        onChangeText={(text) => setVerification({ key: `${revision.summaryId}:${revision.revision}`, text })}
        style={{ color: colors.foreground, minHeight: 44, borderWidth: 1, borderColor: colors.border, borderRadius: 6, padding: 8 }} />
      <Text testID="summary-verification-result" role="status" style={muted}>{verification?.key !== `${revision.summaryId}:${revision.revision}` || !verification.text ? "Paste to verify the exact saved text." : verification.text === revision.text ? `Pasted text matches saved r${revision.revision}.` : "Pasted text does not match this saved revision. Use Copy again."}</Text>
    </View> : null}
  </OperationSheet>;
}

/** Read-only preparation. Only Paseo's destination-side native picker can change a draft. */
export function AttachSheet({ source, revision: given, colors, onClose }: {
  source: DirectoryEntry; revision: SummaryRevision | null; colors: Colors; onClose(): void;
}) {
  const available = useSummaryAttachmentAvailability();
  const latest = useRpc(latestSummaries), read = useRpc(readSummary);
  const requestKey = `${source.id}:${given?.summaryId ?? "latest"}:${given?.revision ?? ""}`;
  const [state, setState] = useState<{ key: string; revision: SummaryRevision | null; problem: string | null } | null>(null);
  useEffect(() => {
    let live = true;
    setState(null);
    if (!available) return;
    void (async () => {
      try {
        const pin = given ?? (await latest({ agentIds: [source.id] }))[source.id];
        if (!pin) throw new Error("No saved summary. Use Summarize & copy first.");
        const revision = summaryRevisionSchema.parse(await read({ summaryId: pin.summaryId, revision: pin.revision }));
        if (revision.summaryId !== pin.summaryId || revision.revision !== pin.revision || revision.sourceAgentId !== source.id || (given && revision.text !== given.text) || !revision.text.trim() || Object.values(revision.sections).some(text => !text.trim())) throw new Error("The saved revision is empty or does not match this source.");
        if (live) setState({ key: requestKey, revision, problem: null });
      } catch (error) { if (live) setState({ key: requestKey, revision: null, problem: failure(error) }); }
    })();
    return () => { live = false; };
  }, [source.id, given?.summaryId, given?.revision, available]);
  const current = available && state?.key === requestKey ? state : null;
  const revision = current?.revision ?? null;
  const muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 } as const;
  return <OperationSheet testID="attach-sheet" title="Choose attachment in destination" subtitle={source.title} colors={colors} onClose={onClose}
    footer={<SheetFooter colors={colors} note="No draft was changed here. Nothing was sent." secondary={{ testID: "attach-cancel", label: "Cancel", onPress: onClose }} />}>
    <Text role="status" style={muted}>{!available ? SUMMARY_ATTACHMENT_UNAVAILABLE : !current ? "Reading the saved revision…" : current.problem ?? "Saved revision ready to find in the native picker. Kontor has not changed a draft or verified a destination."}</Text>
    {revision ? <><SourceChip revision={revision} colors={colors} />
      <Text selectable testID="attach-revision-identifier" style={muted}>{summaryAttachmentIdentifier(revision.summaryId, revision.revision)}</Text>
      <Text selectable style={{ color: colors.foreground, fontSize: 12 }}>{revision.text}</Text></> : null}
    <Text style={muted}>{SUMMARY_ATTACHMENT_STEPS}</Text><Text style={muted}>{SUMMARY_ATTACHMENT_LIMIT}</Text>
  </OperationSheet>;
}
