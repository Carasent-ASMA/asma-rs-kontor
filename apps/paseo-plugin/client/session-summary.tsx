import { useEffect, useRef, useState } from "react";
import { Text, View } from "react-native";
import { copyText, Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { useRpc } from "@getpaseo/plugin/client";
import { attachSummary, readSummary, saveSummary, type AttachOutcome, type SummaryRevision } from "../shared/session-navigator";
import { DenseSessionRow } from "./dense-session-row";
import { Choices } from "./model-picker";
import { OperationSheet, SheetFooter } from "./operation-sheet";
import { COARSE_POINTER, type Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import { exactTime, parseTime, useClock } from "./session-time";
import { matchesQuery } from "./session-view";

const failure = (error: unknown) => error instanceof Error ? error.message : String(error);
/** Destinations listed at once; a search narrows the rest. */
const DESTINATIONS_SHOWN = 20;

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
      primary={{ testID: "summary-attach", label: "Attach to another seat…", disabled: !!attachUnavailable, hint: attachUnavailable, onPress: () => onAttach(revision) }} />
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

/**
 * Board C3: places a saved revision in another seat's draft, or starts a new lead from it. Attach only prepares a
 * draft for that seat's operator: nothing is sent and the source keeps its seat. `newLead` is the surface's way to
 * open New lead with the revision pinned, or why it has none.
 */
export function AttachSheet({ source, revision: given, destinations, opened, colors, compact, newLead, onClose }: {
  source: DirectoryEntry; revision: SummaryRevision | null; destinations: readonly DirectoryEntry[]; opened: ReadonlyMap<string, number>;
  colors: Colors; compact: boolean; newLead: ((revision: SummaryRevision) => void) | string; onClose: () => void;
}) {
  const attach = useRpc(attachSummary), clock = useClock();
  const { state, retry } = useSavedSummary(source, given);
  const [target, setTarget] = useState<"existing" | "new">("existing");
  const [query, setQuery] = useState("");
  const [pick, setPick] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [result, setResult] = useState<{ outcome: AttachOutcome | null; problem: string | null } | null>(null);
  const revision = state?.revision ?? null, tall = compact || COARSE_POINTER;
  const muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 } as const;
  const matches = destinations.filter((entry) => entry.id !== source.id && entry.archivedAt === null && matchesQuery(entry, query));
  const destination = destinations.find((entry) => entry.id === pick) ?? null;
  const drafted = result?.outcome?.state === "drafted";
  const place = async () => {
    if (!revision || !destination || sending) return;
    setSending(true);
    try { setResult({ outcome: await attach({ summaryId: revision.summaryId, revision: revision.revision, destinationAgentId: destination.id }), problem: null }); }
    catch (error) { setResult({ outcome: null, problem: `Kontor could not attach the summary (${failure(error)}). Check ${destination.title}'s draft before trying again.` }); }
    finally { setSending(false); }
  };
  const primary = target === "new"
    ? { testID: "attach-new-lead", label: revision ? `New lead with r${revision.revision}` : "New lead", disabled: !revision || typeof newLead === "string",
      hint: typeof newLead === "string" ? newLead : null, onPress: () => { if (revision && typeof newLead !== "string") newLead(revision); } }
    : { testID: "attach-confirm", label: drafted ? "Attached" : sending ? "Attaching…" : "Attach to draft", disabled: !revision || !destination || sending || drafted,
      hint: !destination ? "Choose a destination." : null, onPress: () => { void place(); } };
  return <OperationSheet testID="attach-sheet" title="Attach summary" colors={colors} onClose={onClose}
    footer={<SheetFooter colors={colors} note="Review before sending. Nothing is sent automatically." primary={revision ? primary : state ? { label: "Try again", onPress: retry } : null} />}>
    {revision ? <SourceChip revision={revision} colors={colors} /> : <Text role="status" style={{ ...muted, color: state ? colors.statusWarning : colors.foregroundMuted }}>
      {state ? `The summary was not saved (${state.problem}). Nothing can be attached.` : `Saving a summary of ${source.title}…`}</Text>}
    <Choices layout="segmented" bare label="Destination" colors={colors} tall={tall} selected={target} onSelect={(next) => { setTarget(next as "existing" | "new"); setResult(null); }}
      options={[{ id: "existing", label: "Existing seat" }, { id: "new", label: "New lead" }]} />
    {target === "new" ? <Text style={muted}>{typeof newLead === "string" ? newLead : "New lead opens with this revision as its source; it is attached before the lead's first task."}</Text> : <>
      <TextInput testID="attach-search" accessibilityLabel="Find destination" placeholder="Find destination…" value={query} onChangeText={setQuery} maxLength={200} placeholderTextColor={colors.foregroundMuted}
        style={{ color: colors.foreground, fontSize: 13, minHeight: tall ? 44 : 34, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6 }} />
      <View role="radiogroup" accessibilityLabel="Destination seat" style={{ gap: 2 }}>
        {matches.slice(0, DESTINATIONS_SHOWN).map((entry) => <DenseSessionRow key={entry.id} testID={`attach-destination-${entry.id}`} entry={entry} colors={colors} clock={clock}
          opened={opened.get(entry.id) ?? null} mode={{ kind: "pick", selected: entry.id === pick, unavailable: drafted ? "Already attached." : null }} onPress={() => { setPick(entry.id); setResult(null); }} />)}
        {matches.length ? null : <Text style={muted}>{query.trim() ? `No session matches “${query.trim()}”.` : "No other session can receive a draft."}</Text>}
        {matches.length > DESTINATIONS_SHOWN ? <Text style={muted}>{`${matches.length - DESTINATIONS_SHOWN} more match; narrow the search.`}</Text> : null}
      </View>
      {revision && destination ? <View testID="attach-preview" style={{ gap: 4, padding: 10, borderRadius: 8, backgroundColor: colors.surface2 }}>
        <Text style={{ ...muted, fontWeight: "600" }}>{`Draft preview · ${destination.title}`}</Text>
        <Text style={muted}>{`Context from ${revision.sourceTitle} (r${revision.revision})`}</Text>
        <Text numberOfLines={5} style={{ color: colors.foreground, fontSize: 12, lineHeight: 17 }}>{revision.text}</Text>
      </View> : null}
      {result ? <Text testID="attach-result" role="status" accessibilityLiveRegion="polite" style={{ color: drafted ? colors.foreground : colors.statusWarning, fontSize: 12, lineHeight: 17 }}>
        {result.outcome ? result.outcome.state === "unknown" ? `${result.outcome.message} Check the destination's draft before trying again.` : result.outcome.message : result.problem}</Text> : null}
    </>}
  </OperationSheet>;
}
