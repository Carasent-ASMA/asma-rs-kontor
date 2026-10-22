import { useState, type ReactNode } from "react";
import { ScrollView, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import { organizationPresentation, type OrganizationSettings } from "../shared/organization";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import type { TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import type { DirectoryEntry } from "./session-directory";
import type { OrganizationSettingsState } from "./use-organization-settings";
import { SourceDetails } from "./source-details";
import { TeamsSnapshotDetails } from "./teams-source-page";
import { Action, Card, Copy, Disclosure, type Colors } from "./page-controls";
import { kontordSentence } from "./kontord-copy";
import { rebaseOrganizationDraft, setDisplayMappingField } from "./organization-draft";
import { captureDay } from "./graph-page";
import type { OrganizationWorkspace } from "./session-organization";

const SOURCE_GROUPS: ReadonlyArray<readonly [ReadSourceSummary["kind"], string]> = [["plan", "Plans"], ["jira", "Jira history"], ["delivery", "Delivery records"], ["decisions", "Decision records"], ["teams", "Teams configuration"]];
const plural = (count: number, noun: string) => `${count} ${noun}${count === 1 ? "" : "s"}`;
const catalogNote = (kontord: string) => `Read-only. Sources are configured on the server; no path, title or renamed ID is inferred. Governed scheduler, gates and team activation: not shown here. ${kontord} Console owns policy and administration.`;
/** One visible line per configured source: its exact title, status only when not ready, and a capture cue for Jira unless the title names that day. */
function sourceSummary(read: SourceRead, colors: Colors) {
  const title = read.source.title;
  const generated = read.jiraHistory?.generated ?? "Unknown";
  const day = read.source.kind === "jira" ? captureDay(generated) : null;
  const parts = [title, read.status === "ready" ? null : read.status, day && !title.includes(day) && !title.includes(generated.slice(0, 10)) ? `captured ${day}` : null];
  const full = `${read.source.title} · ${read.status} · ${read.source.kind === "jira" ? "Historical Jira snapshot" : read.source.revisionKind}. ${read.source.kind === "jira" ? `Observed ${read.jiraHistory?.generated ?? "Unknown"} · not live` : `Read ${read.readAt}`}`;
  return <Text selectable accessibilityLabel={full} style={{ color: colors.foreground, fontSize: 13, lineHeight: 20 }}>{parts.filter(Boolean).join(" · ")}</Text>;
}
export function OrganizationSettingsPage({ state, colors, entries, workspaces, sources, reads, sourceError, teamsSnapshots = {}, teamsErrors = {}, sourcesAvailable = false, refresh, onOpenTeams, keyboard, kontord = kontordSentence(null) }: {
  state: OrganizationSettingsState; colors: Colors; entries: readonly DirectoryEntry[];
  sources: readonly ReadSourceSummary[]; sourceError: string | null; refresh: () => void;
  workspaces: readonly OrganizationWorkspace[];
  reads: Readonly<Record<string, SourceRead>>;
  teamsSnapshots?: Readonly<Record<string, TeamsSourceSnapshot>>;
  teamsErrors?: Readonly<Record<string, string>>;
  sourcesAvailable?: boolean;
  /** Opens the configured teams & templates browser; omitted when the host cannot navigate there. */
  onOpenTeams?: () => void;
  /** The Keyboard card, kept in its own settings document. */
  keyboard?: ReactNode;
  /** What kontor.status observed on this host; unknown when absent. */
  kontord?: string;
}) {
  // A draft holds its opening revision across incoming saves from another device.
  const [draft, setDraft] = useState<{ values: OrganizationSettings; base: OrganizationSettings; revision: string } | null>(null);
  const [agentId, setAgentId] = useState<string | null>(null);
  const [mappingId, setMappingId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const values = draft?.values;
  const presentation = values ? organizationPresentation(values) : null;
  const teamsFormats = teamsSourceBindings(sources).formats;
  // The source hook reports an error whenever it is offline or failed; without data or error it is still reading.
  const reading = !sourcesAvailable && sourceError === null;
  const plans = sources.filter((source) => source.kind === "plan");
  const update = (patch: Partial<OrganizationSettings>) => {
    if (draft) setDraft({ ...draft, values: { ...draft.values, ...patch } });
  };
  return <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={{ padding: 12, paddingBottom: 32, gap: 12, width: "100%", maxWidth: 820, alignSelf: "center" }}>
    <Card title="Presentation and associations" colors={colors}>
      <Copy colors={colors} muted small>Shared by every client of this host.{draft ? " Changes apply only after Save." : ""}</Copy>
      {/* Saved and not editing: the revision is in the summary's label; any other state is shown. */}
      {state.status === "ready" && !draft && !state.saving ? null : <Copy colors={colors}>{state.status === "loading" ? "Reading saved preferences…" : state.status === "ready" ? `Saved revision ${state.revision.length > 16 ? `${state.revision.slice(0, 12)}…` : state.revision}` : state.status === "invalid" ? `Saved preferences are invalid and not applied: ${state.error}` : `Preferences unavailable: ${state.error}`}{state.saving ? " · Saving…" : ""}</Copy>}
      {state.status === "ready" && !draft ? (() => {
        const savedPresentation = organizationPresentation(state.values);
        const linked = Object.values(state.values.planAssociations).filter((ids) => ids.length).length;
        const mapped = Object.keys(state.values.displayMappings).length;
        const counts = [linked ? `${plural(linked, "conversation")} with explicit plan links` : null, mapped ? plural(mapped, "display mapping") : null].filter(Boolean).join(" · ");
        return <View testID="settings-saved-summary" accessible accessibilityLabel={`Saved revision ${state.revision.length > 16 ? `${state.revision.slice(0, 12)}…` : state.revision}. Layout ${savedPresentation.layout} · grouping ${savedPresentation.grouping} · appearance ${state.values.appearance} · ${plural(linked, "conversation")} with explicit plan links · ${plural(mapped, "display mapping")}`} style={{ gap: 2 }}>
          <Copy colors={colors} muted small>Layout {savedPresentation.layout} · grouping {savedPresentation.grouping} · appearance {state.values.appearance}</Copy>
          {counts ? <Copy colors={colors} muted small>{counts}</Copy> : null}
        </View>;
      })() : null}
      {state.saveError ? <Copy colors={colors}>Save failed: {state.saveError}. Your draft is retained. Reload saved values, then review or discard the draft before retrying.</Copy> : null}
      {message ? <Copy colors={colors}>{message}</Copy> : null}
      <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
        <Action colors={colors} label="Reload saved values" onPress={() => { void state.reload(); }} />
        {state.status === "ready" && !draft ? <Action colors={colors} label="Edit presentation" onPress={() => { setDraft({ values: state.values, base: state.values, revision: state.revision }); setMessage(null); }} /> : null}
        {state.status === "invalid" ? <Action colors={colors} label="Reset invalid preferences to defaults" disabled={state.saving} onPress={() => { void state.reset(); }} /> : null}
      </View>
      {draft && values ? <>
        <Copy colors={colors}>Editing revision {draft.revision}{state.status === "ready" && state.revision !== draft.revision ? " · newer saved values exist; this draft must be reviewed" : ""}</Copy>
        <Copy colors={colors} muted small>Layout</Copy>
        <View style={{ flexDirection: "row", flexWrap: "wrap" }}>{(["grouped", "flat"] as const).map((layout) => <Action key={layout} colors={colors} label={layout === "flat" ? "Flat" : "Grouped"} selected={presentation?.layout === layout} onPress={() => update({ layout, grouping: presentation?.grouping ?? "hierarchy" })} />)}</View>
        {presentation?.layout === "grouped" ? <>
          <Copy colors={colors} muted small>Group by</Copy>
          <View style={{ flexDirection: "row", flexWrap: "wrap" }}>{(["hierarchy", "role"] as const).map((grouping) => <Action key={grouping} colors={colors} label={grouping === "role" ? "Role" : "Desks & workstreams"} selected={presentation.grouping === grouping} onPress={() => update({ grouping })} />)}</View>
        </> : null}
        <Copy colors={colors} muted small>Appearance</Copy>
        <View style={{ flexDirection: "row", flexWrap: "wrap" }}>{(["system", "light", "dark"] as const).map((appearance) => <Action key={appearance} colors={colors} label={appearance} selected={values.appearance === appearance} onPress={() => update({ appearance })} />)}</View>
        <Copy colors={colors} muted small>Explicit conversation → configured plan association</Copy>
        {!entries.length ? <Copy colors={colors} muted>No conversations in the current native inventory.</Copy> : null}
        <ScrollView style={{ maxHeight: 220 }} nestedScrollEnabled>{entries.map((entry) => <Action key={entry.id} colors={colors} label={`${entry.title} · ${entry.id}`} selected={agentId === entry.id} onPress={() => setAgentId(entry.id)} />)}</ScrollView>
        {agentId && !plans.length ? <Copy colors={colors} muted>No configured plan sources on this host.</Copy> : null}
        {agentId ? plans.map((source) => <Action key={source.id} colors={colors} label={`Link ${source.title}`} selected={values.planAssociations[agentId]?.includes(source.id)} onPress={() => {
          const current = values.planAssociations[agentId] ?? [];
          update({ planAssociations: { ...values.planAssociations, [agentId]: current.includes(source.id) ? current.filter((id) => id !== source.id) : [...current, source.id] } });
        }} />) : null}
        <Copy colors={colors} muted small>Display mapping only — does not relabel ownership or move sessions</Copy>
        {!workspaces.length && !entries.length ? <Copy colors={colors} muted>No workspaces or conversations to map.</Copy> : null}
        <ScrollView style={{ maxHeight: 180 }} nestedScrollEnabled>
          {workspaces.map((workspace) => <Action key={`workspace:${workspace.id}`} colors={colors} label={`Map workspace: ${workspace.name}`} selected={mappingId === `workspace:${workspace.id}`} onPress={() => setMappingId(`workspace:${workspace.id}`)} />)}
          {entries.map((entry) => <Action key={`session:${entry.id}`} colors={colors} label={`Map session: ${entry.title}`} selected={mappingId === `session:${entry.id}`} onPress={() => setMappingId(`session:${entry.id}`)} />)}
        </ScrollView>
        {mappingId ? <>
          <TextInput accessibilityLabel="Display workstream" placeholder="Display workstream" value={values.displayMappings[mappingId]?.workstream ?? ""} onChangeText={(workstream) => update({ displayMappings: setDisplayMappingField(values.displayMappings, mappingId, "workstream", workstream) })} style={{ color: colors.foreground, minHeight: 44, borderWidth: 1, borderColor: colors.border, padding: 10 }} />
          <TextInput accessibilityLabel="Display epic" placeholder="Display epic" value={values.displayMappings[mappingId]?.epic ?? ""} onChangeText={(epic) => update({ displayMappings: setDisplayMappingField(values.displayMappings, mappingId, "epic", epic) })} style={{ color: colors.foreground, minHeight: 44, borderWidth: 1, borderColor: colors.border, padding: 10 }} />
        </> : null}
        <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
          {state.status === "ready" && draft.revision !== state.revision ? <Action colors={colors} label="Reapply local changes to latest saved values" onPress={() => { setDraft({ base: state.values, values: rebaseOrganizationDraft(draft.base, draft.values, state.values), revision: state.revision }); setMessage("Local edits reapplied. Review the draft, then save against the latest revision."); }} /> : null}
          <Action colors={colors} label="Save preferences" disabled={state.status !== "ready" || state.saving} onPress={() => {
            void state.save(draft.values, draft.revision).then((saved) => { if (saved) { setDraft(null); setMessage("Preferences saved on this host."); } else setMessage("Preferences were not saved. Draft retained; inspect the conflict or validation error."); });
          }} />
          <Action colors={colors} label="Discard draft" onPress={() => { setDraft(null); setMessage(null); }} />
        </View>
      </> : null}
    </Card>
    {keyboard}
    <Card title="Configured sources" colors={colors}>
      <View testID="settings-catalog-note"><Disclosure small label="Read-only · configured on the server" colors={colors}><Copy colors={colors} muted small>{catalogNote(kontord)}</Copy></Disclosure></View>
      {sourceError || reading ? <Copy colors={colors}>{sourceError ?? "Reading configured sources…"}</Copy> : null}
      {!sources.length && sourcesAvailable ? <Copy colors={colors}>No sources are configured on this host.</Copy> : null}
      {SOURCE_GROUPS.filter(([kind]) => sources.some((source) => source.kind === kind)).map(([kind, heading]) => <View key={kind} style={{ gap: 6 }}>
        <Copy colors={colors} muted small>{heading} ({sources.filter((source) => source.kind === kind).length})</Copy>
        {sources.filter((source) => source.kind === kind).map((source) => {
          if (!teamsFormats[source.id]) return reads[source.id] ? <SourceDetails key={source.id} read={reads[source.id]!} colors={colors} summary={sourceSummary(reads[source.id]!, colors)} /> : <Copy key={source.id} colors={colors}>{source.title} · Not read · observed time Unknown</Copy>;
          const snapshot = teamsSnapshots[source.id];
          const error = teamsErrors[source.id];
          const status = snapshot?.status ?? (error ? "unavailable" : "Not read");
          const stale = snapshot && (!sourcesAvailable || error) ? " · retained snapshot stale" : "";
          // One line per source; the configuration state, read time, observed time and activation are in Source details and the label.
          const state = `${snapshot?.configurationState ?? teamsFormats[source.id]}`;
          const times = `Read ${snapshot?.readAt ?? "Not captured"} · observed time Unknown · activation: ${snapshot?.activation ?? "not-attested"}`;
          return <View key={source.id} style={{ gap: 2 }}>
            <Text selectable accessibilityLabel={`${source.title} · ${status}${stale} · ${state}. ${times}`} style={{ color: colors.foreground, fontSize: 13, lineHeight: 20 }}>{source.title}{status === "ready" && !stale ? "" : ` · ${status}`}{stale}</Text>
            {snapshot ? null : <Copy colors={colors} muted small>{times}</Copy>}
            {error ? <Copy colors={colors}>Adapter request failed: {error}</Copy> : null}
            {snapshot ? <TeamsSnapshotDetails snapshot={snapshot} colors={colors} notes={<><Copy colors={colors}>{source.title} · {status}{stale} · {state}</Copy><Copy colors={colors}>{times}</Copy></>} /> : null}
          </View>;
        })}
      </View>)}
      <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
        <Action colors={colors} label="Refresh source reads" icon="RefreshCw" iconOnly onPress={refresh} />
        {onOpenTeams ? <Action colors={colors} label="Browse configured teams & templates" onPress={onOpenTeams} /> : null}
      </View>
    </Card>
  </ScrollView>;
}
