import { useEffect, useRef, useState } from "react";
import { Pressable, ScrollView, Text, View, type LayoutChangeEvent } from "react-native";
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
 * message travels instead. With a `height`, as in the host's popover, which scrolls whatever its body is, the picker keeps
 * it: title, search and actions stay put and the rows take the rest and scroll. A picked source's summary takes up to
 * half of that, never less than one whole line of its text, and scrolls in its own box rather than push the actions
 * away. Where the rows would keep less than one row beside such a summary, the subtitle goes, the spacing tightens and
 * the summary opens in the rows' own scroll, under its row or, while a search hides that row, after them. Where even
 * then they would keep less than a row or a whole line of the summary, the title scrolls with the rows too, above them;
 * search and actions stay. The summary also says that its revision travels with the lead, so the actions take no more
 * than their own row.
 */
export function LeadSourcePicker({ colors, compact, sources, opened, selected, height, onUse, onCancel }: {
  colors: Colors; compact: boolean; sources: readonly DirectoryEntry[];
  /** When this device last opened each session from Kontor. */
  opened?: ReadonlyMap<string, number>;
  selected: string | null;
  /** The height to keep, given the height one line of the title takes as drawn. */
  height?: (line: number) => number;
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
  // What stays put, as drawn: one line of the title, as tall as the summary's tallest, the subtitle when last shown, the
  // search and the actions.
  const [drawn, setDrawn] = useState({ line: 0, about: 0, search: 0, actions: 0 });
  const measure = (part: keyof typeof drawn) => (event: LayoutChangeEvent) => {
    const next = Math.ceil(event.nativeEvent.layout.height);
    setDrawn((previous) => previous[part] === next ? previous : { ...previous, [part]: next });
  };
  const fixed = height !== undefined, line = drawn.line || 20, box = height?.(line);
  /** What the rows and a summary have once the parts that stay put, a heading of `head` among them, and the spacing are drawn. */
  const rest = (gap: number, pad: number, head: number) => (box ?? 0) - (head ? 3 : 2) * gap - 2 * pad - head - drawn.search - drawn.actions;
  // Tight when that leaves less than a row's 44 beside a summary of two lines and its padding.
  const free = rest(8, 6, line + drawn.about), tight = fixed && free < 44 + 8 + 16 + 2 * line;
  // Scrolled when even tight it leaves less than a row's 44 or a whole line of the summary and its padding.
  const scrolled = tight && rest(4, 0, line) < Math.max(44, 16 + line);
  const [gap, pad] = tight ? [4, 0] : [8, 6], listed = rows.some((row) => row.id === pick);
  const summaryMax = Math.max(0, Math.min(free - gap, Math.max(16 + line, Math.floor((free - gap) / 2))));
  const details = entry ? <View testID="kontor-lead-source-summary" accessibilityLiveRegion="polite" style={{ gap: 3, padding: 8 }}>
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
    {supported ? <Text style={{ ...muted, fontSize: 11 }}>The saved summary revision travels with this lead.</Text> : null}
  </View> : null;
  const heading = { color: colors.foreground, fontSize: 14, lineHeight: 17, fontWeight: "600" } as const;
  // The rows' place in the rows' scroll, below the title when it scrolls with them, and each row's place among them.
  const list = useRef<ScrollView>(null), top = useRef(0), places = useRef(new Map<string, { y: number; height: number }>()), shown = useRef({ y: 0, height: 0 });
  /** When the summary takes room from the rows or opens under the picked row, that row stays in their view, from its top when it is the taller. */
  const reveal = () => {
    const at = listed ? places.current.get(pick!) : undefined, { y, height: room } = shown.current;
    if (!at || !room) return;
    const start = top.current + at.y;
    if (start < y || at.height > room) list.current?.scrollTo({ y: start, animated: false });
    else if (start + at.height > y + room) list.current?.scrollTo({ y: start + at.height - room, animated: false });
  };
  return <View testID="kontor-lead-source-picker" style={{ flexShrink: 1, minHeight: 0, height: box, gap, paddingHorizontal: 6, paddingVertical: pad }}>
    {/* The title's line is measured here, above the search; while the title scrolls with the rows, an unseen copy keeps this
      place, hidden from assistive technology. It keeps its role: on the web another role is another element, measured no more. */}
    <View aria-hidden={scrolled} accessibilityElementsHidden={scrolled} importantForAccessibility={scrolled ? "no-hide-descendants" : "auto"} pointerEvents={scrolled ? "none" : "auto"}
      style={scrolled ? { position: "absolute", opacity: 0 } : undefined}>
      <Text accessibilityRole="header" numberOfLines={fixed ? 1 : undefined} onLayout={measure("line")} style={heading}>Choose source session</Text>
      {tight ? null : <Text onLayout={measure("about")} style={muted}>{supported ? "Attach a saved summary to the new lead" : "Bring context from another session"}</Text>}
    </View>
    <TextInput testID="kontor-lead-source-search" onLayout={measure("search")} accessibilityLabel="Find a session" placeholder="Find a session…" value={query} onChangeText={setQuery} maxLength={200}
      placeholderTextColor={colors.foregroundMuted} style={{ color: colors.foreground, fontSize: 13, minHeight: tall ? 44 : 34, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6 }} />
    <ScrollView ref={list} testID="kontor-lead-source-list" keyboardShouldPersistTaps="handled" scrollEventThrottle={16}
      onScroll={(event) => { shown.current.y = event.nativeEvent.contentOffset.y; }}
      onLayout={(event) => { shown.current.height = event.nativeEvent.layout.height; reveal(); }} style={fixed ? { flexGrow: 1, flexShrink: 1, flexBasis: 0, minHeight: 0 } : { flexGrow: 0, flexShrink: 1 }}>
      {scrolled ? <Text accessibilityRole="header" numberOfLines={1} style={{ ...heading, marginBottom: 4 }}>Choose source session</Text> : null}
      <View role="radiogroup" accessibilityLabel="Source session" onLayout={(event) => { top.current = event.nativeEvent.layout.y; reveal(); }} style={{ gap: 1 }}>
        {rows.length ? rows.map((row) => <View key={row.id} onLayout={(event) => { places.current.set(row.id, event.nativeEvent.layout); if (row.id === pick) reveal(); }}>
          <DenseSessionRow testID={`kontor-lead-source-${row.id}`} entry={row} colors={colors} clock={clock}
            opened={opened?.get(row.id) ?? null} mode={{ kind: "pick", selected: row.id === pick }} onPress={() => setPick(row.id)} />
          {tight && row.id === pick ? <View style={{ marginTop: 4, borderRadius: 6, backgroundColor: colors.surface2 }}>{details}</View> : null}
        </View>)
          : <Text style={muted}>{query.trim() ? `No session matches “${query.trim()}”.` : "No session to bring context from."}</Text>}
      </View>
      {tight && details && !listed ? <View style={{ marginTop: 4, borderRadius: 6, backgroundColor: colors.surface2 }}>{details}</View> : null}
    </ScrollView>
    {entry && !tight ? <ScrollView keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: fixed ? 1 : 0, maxHeight: fixed ? summaryMax : undefined, borderRadius: 6, backgroundColor: colors.surface2 }}>
      {details}
    </ScrollView> : null}
    <View onLayout={measure("actions")} style={{ borderTopWidth: 1, borderColor: colors.border, paddingTop: 6 }}>
      <View style={{ flexDirection: "row", justifyContent: "space-between", gap: 8 }}>
        <Action small label="Cancel" accessibilityLabel="Cancel source choice" colors={colors} onPress={onCancel} />
        <Pressable testID="kontor-lead-source-use" accessibilityRole="button" accessibilityLabel="Use selected source" accessibilityHint={problem ?? undefined} accessibilityState={{ disabled: !!problem }} aria-disabled={!!problem}
          disabled={!!problem} onPress={() => { void use(); }} style={{ flexShrink: 1, minHeight: tall ? 44 : 30, paddingHorizontal: 12, borderRadius: 6, justifyContent: "center", backgroundColor: problem ? colors.surface2 : colors.accent, opacity: problem ? 0.6 : 1 }}>
          <Text style={{ color: problem ? colors.foregroundMuted : colors.accentForeground, fontSize: 12, fontWeight: "600" }}>Use selected source</Text>
        </Pressable>
      </View>
    </View>
  </View>;
}
