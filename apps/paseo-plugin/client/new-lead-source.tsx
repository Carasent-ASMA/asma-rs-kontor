import { useEffect, useState } from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { useRpc } from "@getpaseo/plugin/client";
import { latestSummaries, readNavigatorCapabilities, saveSummary, type Capability, type SummaryRevision } from "../shared/session-navigator";
import type { DirectoryEntry } from "./session-directory";
import { matchesQuery } from "./session-view";
import { DenseSessionRow } from "./dense-session-row";
import { Action, COARSE_POINTER, type Colors } from "./page-controls";
import { readableInk } from "./session-status-ink";
import { lastActivity } from "./session-row-format";
import { parseTime, useClock } from "./session-time";

/** A source as Use hands it back: the session, and the saved summary revision pinned for the lead, or null when none is. */
export type LeadSource = { readonly entry: DirectoryEntry; readonly summary: SummaryRevision | null };

const failure = (error: unknown) => error instanceof Error ? error.message : String(error);

/** True when the session has activity after the revision's cutoff, so saving again makes a new revision. */
export function summaryBehind(entry: DirectoryEntry, revision: SummaryRevision): boolean {
  const activity = lastActivity(entry)?.at ?? null, cutoff = parseTime(revision.cutoff);
  return activity !== null && cutoff !== null && activity > cutoff;
}

/**
 * Choose source session: one of `sources`, on the shared session rows. Where the host keeps saved summaries, Use pins
 * the session's latest revision, saving one first when it has none; a session that moved on since its revision can
 * save a newer one. On any other host the reason is shown and Use names the session alone, whose last assistant
 * message travels instead.
 */
export function LeadSourcePicker({ colors, compact, sources, opened, selected, onUse, onCancel }: {
  colors: Colors; compact: boolean; sources: readonly DirectoryEntry[];
  /** When this device last opened each session from Kontor. */
  opened?: ReadonlyMap<string, number>;
  selected: string | null;
  onUse: (source: LeadSource) => void; onCancel: () => void;
}) {
  const capabilities = useRpc(readNavigatorCapabilities), latest = useRpc(latestSummaries), save = useRpc(saveSummary);
  const clock = useClock();
  const [query, setQuery] = useState("");
  const [pick, setPick] = useState<string | null>(selected);
  const [summaries, setSummaries] = useState<Capability | null>(null);
  const [saved, setSaved] = useState<{ id: string; revision: SummaryRevision | null; problem: string | null } | null>(null);
  const [saving, setSaving] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    capabilities({}).then((answer) => { if (live) setSummaries(answer.summaries); },
      (error: unknown) => { if (live) setSummaries({ supported: false, reason: `Kontor could not read what this host supports (${failure(error)}).` }); });
    return () => { live = false; };
  }, []);
  const supported = summaries?.supported === true;
  useEffect(() => {
    if (!supported || !pick) return;
    let live = true;
    latest({ agentIds: [pick] }).then((answer) => { if (live) setSaved({ id: pick, revision: answer[pick] ?? null, problem: null }); },
      (error: unknown) => { if (live) setSaved({ id: pick, revision: null, problem: failure(error) }); });
    return () => { live = false; };
  }, [supported, pick]);
  const tall = compact || COARSE_POINTER;
  const muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 } as const;
  const rows = sources.filter((entry) => matchesQuery(entry, query));
  const entry = sources.find((candidate) => candidate.id === pick) ?? null;
  const current = saved?.id === pick ? saved : null;
  const revision = current?.revision ?? null;
  const problem = !entry ? "Choose a source session." : !summaries ? "Reading what this host supports…" : saving ? "Saving the summary…"
    : supported && !current ? "Reading the saved summary…" : current?.problem ? `The saved summary could not be read or saved: ${current.problem}` : null;
  /** Saves a revision of the picked session; resolves to it, or null when the save failed and says why. */
  const saveNow = async (): Promise<SummaryRevision | null> => {
    if (!entry) return null;
    const id = entry.id;
    setSaving(id);
    try { const next = await save({ agentId: id }); setSaved({ id, revision: next, problem: null }); return next; }
    catch (error) { setSaved({ id, revision, problem: failure(error) }); return null; }
    finally { setSaving(null); }
  };
  const use = async () => {
    if (!entry || problem) return;
    if (!supported) { onUse({ entry, summary: null }); return; }
    const pinned = revision ?? await saveNow();
    if (pinned) onUse({ entry, summary: pinned });
  };
  const behind = !!entry && !!revision && summaryBehind(entry, revision);
  return <View testID="kontor-lead-source-picker" style={{ flexShrink: 1, minHeight: 0, gap: 8, padding: 6 }}>
    <View>
      <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 14, fontWeight: "600" }}>Choose source session</Text>
      <Text style={muted}>{supported ? "Attach a saved summary to the new lead" : "Bring context from another session"}</Text>
    </View>
    <TextInput testID="kontor-lead-source-search" accessibilityLabel="Find a session" placeholder="Find a session…" value={query} onChangeText={setQuery} maxLength={200}
      placeholderTextColor={colors.foregroundMuted} style={{ color: colors.foreground, fontSize: 13, minHeight: tall ? 44 : 34, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6 }} />
    <ScrollView keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }}>
      <View role="radiogroup" accessibilityLabel="Source session" style={{ gap: 1 }}>
        {rows.length ? rows.map((row) => <DenseSessionRow key={row.id} testID={`kontor-lead-source-${row.id}`} entry={row} colors={colors} clock={clock}
          opened={opened?.get(row.id) ?? null} mode={{ kind: "pick", selected: row.id === pick }} onPress={() => setPick(row.id)} />)
          : <Text style={muted}>{query.trim() ? `No session matches “${query.trim()}”.` : "No session to bring context from."}</Text>}
      </View>
    </ScrollView>
    {entry ? <View testID="kontor-lead-source-summary" accessibilityLiveRegion="polite" style={{ gap: 3, padding: 8, borderRadius: 6, backgroundColor: colors.surface2 }}>
      {summaries && !summaries.supported ? <>
        <Text style={muted}>{summaries.reason}</Text>
        <Text style={muted}>No summary is pinned. The lead copies this session's last assistant message before its first task.</Text>
      </> : revision ? <>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Icon name="FileText" size={14} color={readableInk(colors.accent, colors)} />
          <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 12, fontWeight: "600" }}>Saved summary · r{revision.revision}</Text>
        </View>
        <Text numberOfLines={3} style={{ color: colors.foreground, fontSize: 12, lineHeight: 16 }}>{revision.sections.task}</Text>
        <Text numberOfLines={1} style={{ ...muted, fontSize: 11 }}>Source: {revision.sourceTitle} · covers up to {revision.cutoff}</Text>
        {behind ? <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Text style={{ ...muted, flex: 1 }}>The session moved on after r{revision.revision}.</Text>
          <Action small label={`Save r${revision.revision + 1}`} accessibilityLabel={`Save a new revision, r${revision.revision + 1}`} colors={colors} disabled={!!saving} onPress={() => { void saveNow(); }} />
        </View> : null}
      </> : current && !current.problem ? <Text style={muted}>No saved summary yet. Use saves one and pins it for this lead.</Text> : null}
      {problem ? <Text style={muted}>{problem}</Text> : null}
    </View> : null}
    <View style={{ gap: 2, borderTopWidth: 1, borderColor: colors.border, paddingTop: 6 }}>
      <View style={{ flexDirection: "row", justifyContent: "space-between", gap: 8 }}>
        <Action small label="Cancel" accessibilityLabel="Cancel source choice" colors={colors} onPress={onCancel} />
        <Pressable testID="kontor-lead-source-use" accessibilityRole="button" accessibilityLabel="Use selected source" accessibilityHint={problem ?? undefined} accessibilityState={{ disabled: !!problem }} aria-disabled={!!problem}
          disabled={!!problem} onPress={() => { void use(); }} style={{ minHeight: tall ? 44 : 30, paddingHorizontal: 12, borderRadius: 6, justifyContent: "center", backgroundColor: problem ? colors.surface2 : colors.accent, opacity: problem ? 0.6 : 1 }}>
          <Text style={{ color: problem ? colors.foregroundMuted : colors.accentForeground, fontSize: 12, fontWeight: "600" }}>Use selected source</Text>
        </Pressable>
      </View>
      {supported ? <Text style={{ ...muted, fontSize: 11, textAlign: "center" }}>The saved summary revision travels with this lead.</Text> : null}
    </View>
  </View>;
}
