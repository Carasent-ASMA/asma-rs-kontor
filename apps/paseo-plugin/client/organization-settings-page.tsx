import { useState } from "react";
import { ScrollView, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import type { OrganizationSettings } from "../shared/organization";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import type { DirectoryEntry } from "./session-directory";
import type { OrganizationSettingsState } from "./use-organization-settings";
import { SourceDetails } from "./source-details";
import { Action, Card, Copy, type Colors } from "./page-controls";
import { rebaseOrganizationDraft, setDisplayMappingField } from "./organization-draft";
import type { OrganizationWorkspace } from "./session-organization";

export function OrganizationSettingsPage({ state, colors, entries, workspaces, sources, reads, sourceError, refresh }: {
  state: OrganizationSettingsState; colors: Colors; entries: readonly DirectoryEntry[];
  sources: readonly ReadSourceSummary[]; sourceError: string | null; refresh: () => void;
  workspaces: readonly OrganizationWorkspace[];
  reads: Readonly<Record<string, SourceRead>>;
}) {
  // A draft holds its opening revision across incoming saves from another device.
  const [draft, setDraft] = useState<{ values: OrganizationSettings; base: OrganizationSettings; revision: string } | null>(null);
  const [agentId, setAgentId] = useState<string | null>(null);
  const [mappingId, setMappingId] = useState<string | null>(null);
  const [message, setMessage] = useState<string | null>(null);
  const values = draft?.values;
  const update = (patch: Partial<OrganizationSettings>) => {
    if (draft) setDraft({ ...draft, values: { ...draft.values, ...patch } });
  };
  return <ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={{ padding: 12, paddingBottom: 32, gap: 12 }}>
    <Card title="Presentation and associations" colors={colors}>
      <Copy colors={colors}>Host settings: {state.status}{state.saving ? " · Saving…" : ""}. Shared across clients of this host. Filesystem sources are configured on the server.</Copy>
      {state.status === "ready" ? <Copy colors={colors} muted>Saved revision {state.revision}</Copy> : state.status === "loading" ? <Copy colors={colors}>Reading saved preferences…</Copy> : <Copy colors={colors}>{state.error}</Copy>}
      {state.saveError ? <Copy colors={colors}>Save failed: {state.saveError}. Your draft is retained. Reload saved values, then review or discard the draft before retrying.</Copy> : null}
      {message ? <Copy colors={colors}>{message}</Copy> : null}
      <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
        <Action colors={colors} label="Reload saved values" onPress={() => { void state.reload(); }} />
        {state.status === "ready" && !draft ? <Action colors={colors} label="Edit presentation" onPress={() => { setDraft({ values: state.values, base: state.values, revision: state.revision }); setMessage(null); }} /> : null}
        {state.status === "invalid" ? <Action colors={colors} label="Reset invalid preferences to defaults" disabled={state.saving} onPress={() => { void state.reset(); }} /> : null}
      </View>
      {draft && values ? <>
        <Copy colors={colors}>Editing revision {draft.revision}{state.status === "ready" && state.revision !== draft.revision ? " · newer saved values exist; this draft must be reviewed" : ""}</Copy>
        <Copy colors={colors}>Grouping</Copy>
        <View style={{ flexDirection: "row" }}>{(["hierarchy", "role", "flat"] as const).map((grouping) => <Action key={grouping} colors={colors} label={grouping} selected={values.grouping === grouping} onPress={() => update({ grouping })} />)}</View>
        <Copy colors={colors}>Appearance</Copy>
        <View style={{ flexDirection: "row" }}>{(["system", "light", "dark"] as const).map((appearance) => <Action key={appearance} colors={colors} label={appearance} selected={values.appearance === appearance} onPress={() => update({ appearance })} />)}</View>
        <Copy colors={colors}>Explicit conversation → configured plan association</Copy>
        <ScrollView style={{ maxHeight: 220 }} nestedScrollEnabled>{entries.map((entry) => <Action key={entry.id} colors={colors} label={`${entry.title} · ${entry.id}`} selected={agentId === entry.id} onPress={() => setAgentId(entry.id)} />)}</ScrollView>
        {agentId ? sources.filter((source) => source.kind === "plan").map((source) => <Action key={source.id} colors={colors} label={`Link ${source.title}`} selected={values.planAssociations[agentId]?.includes(source.id)} onPress={() => {
          const current = values.planAssociations[agentId] ?? [];
          update({ planAssociations: { ...values.planAssociations, [agentId]: current.includes(source.id) ? current.filter((id) => id !== source.id) : [...current, source.id] } });
        }} />) : null}
        <Copy colors={colors}>Display mapping only — does not relabel ownership or move sessions</Copy>
        <ScrollView style={{ maxHeight: 180 }} nestedScrollEnabled>
          {workspaces.map((workspace) => <Action key={`workspace:${workspace.id}`} colors={colors} label={`Map workspace: ${workspace.name}`} selected={mappingId === `workspace:${workspace.id}`} onPress={() => setMappingId(`workspace:${workspace.id}`)} />)}
          {entries.map((entry) => <Action key={`session:${entry.id}`} colors={colors} label={`Map session: ${entry.title}`} selected={mappingId === `session:${entry.id}`} onPress={() => setMappingId(`session:${entry.id}`)} />)}
        </ScrollView>
        {mappingId ? <>
          <TextInput accessibilityLabel="Display workstream" placeholder="Display workstream" value={values.displayMappings[mappingId]?.workstream ?? ""} onChangeText={(workstream) => update({ displayMappings: setDisplayMappingField(values.displayMappings, mappingId, "workstream", workstream) })} style={{ color: colors.foreground, minHeight: 44, borderWidth: 1, borderColor: colors.border, padding: 10 }} />
          <TextInput accessibilityLabel="Display epic" placeholder="Display epic" value={values.displayMappings[mappingId]?.epic ?? ""} onChangeText={(epic) => update({ displayMappings: setDisplayMappingField(values.displayMappings, mappingId, "epic", epic) })} style={{ color: colors.foreground, minHeight: 44, borderWidth: 1, borderColor: colors.border, padding: 10 }} />
        </> : null}
        <View style={{ flexDirection: "row" }}>
          {state.status === "ready" && draft.revision !== state.revision ? <Action colors={colors} label="Reapply local changes to latest saved values" onPress={() => { setDraft({ base: state.values, values: rebaseOrganizationDraft(draft.base, draft.values, state.values), revision: state.revision }); setMessage("Local edits reapplied. Review the draft, then save against the latest revision."); }} /> : null}
          <Action colors={colors} label="Save preferences" disabled={state.status !== "ready" || state.saving} onPress={() => {
            void state.save(draft.values, draft.revision).then((saved) => { if (saved) { setDraft(null); setMessage("Preferences saved on this host."); } else setMessage("Preferences were not saved. Draft retained; inspect the conflict or validation error."); });
          }} />
          <Action colors={colors} label="Discard draft" onPress={() => { setDraft(null); setMessage(null); }} />
        </View>
      </> : null}
    </Card>
    <Card title="Configured source status" colors={colors}>
      <Copy colors={colors}>{sourceError ?? "Read-only source catalog available."}</Copy>
      {sources.map((source) => reads[source.id] ? <SourceDetails key={source.id} read={reads[source.id]!} colors={colors} /> : <Copy key={source.id} colors={colors}>{source.title} · Not read · observed time Unknown</Copy>)}
      <Action colors={colors} label="Refresh source reads" onPress={refresh} />
      <Copy colors={colors}>Governed scheduler/gates: kontord is not enabled. Console owns policy and administration.</Copy>
    </Card>
  </ScrollView>;
}
