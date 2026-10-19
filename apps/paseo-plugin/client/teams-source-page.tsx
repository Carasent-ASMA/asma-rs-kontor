import { useState, type ReactNode } from "react";
import { Pressable, Text, View } from "react-native";
import type { DirectoryEntry } from "./session-directory";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import { composeTeamsSourceView, type TeamsFleetData, type TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { Action, Card, Copy, type Colors } from "./page-controls";
import { SourceDetails } from "./source-details";
function Disclosure({ label, colors, children }: { label: string; colors: Colors; children: ReactNode }) {
  const [open, setOpen] = useState(false);
  return <View><Pressable accessibilityRole="button" accessibilityState={{ expanded: open }} accessibilityLabel={label} onPress={() => setOpen(!open)} style={{ minHeight: 44, justifyContent: "center" }}>
    <Text style={{ color: colors.accent }}>{open ? "⌄" : "›"} {label}</Text>
  </Pressable>{open ? children : null}</View>;
}
function SnapshotDetails({ snapshot, colors }: { snapshot: TeamsSourceSnapshot; colors: Colors }) {
  return <Disclosure label="Source details" colors={colors}>
    <Copy colors={colors}>{snapshot.source.path} · {snapshot.format}</Copy>
    <Copy colors={colors}>SHA-256 {snapshot.sha256 ?? "Unknown"} · file modified {snapshot.modifiedAt ?? "Unknown"}</Copy>
    {snapshot.data?.kind === "roster" ? <Copy colors={colors}>Role catalog hash {snapshot.data.roleCatalog.content_hash}</Copy> : null}
    {snapshot.data?.kind === "fleet" ? <>
      {snapshot.data.domains.map((domain) => <Copy key={domain.alias} colors={colors}>{domain.alias}: provider {domain.provider}; configured accounts {domain.accounts.join(", ")}. {domain.provenance.pointer}</Copy>)}
      <Copy colors={colors}>Declared unavailable in this file: domains {snapshot.data.declaredUnavailable.domains.join(", ") || "None"}; accounts {snapshot.data.declaredUnavailable.accounts.join(", ") || "None"}. Current availability is not attested.</Copy>
    </> : null}
    {snapshot.diagnostics.map((diagnostic, index) => <Copy key={index} colors={colors}>{diagnostic.code} · {diagnostic.pointer}: {diagnostic.message}</Copy>)}
  </Disclosure>;
}
function ChainDetails({ chain, colors }: { chain: TeamsFleetData["chains"][number]; colors: Colors }) {
  return <>
    <Copy colors={colors}>Source provenance: {chain.provenance.sourceId} · {chain.provenance.pointer}</Copy>
    <Copy colors={colors}>Rungs keep source order. Alternatives within a rung are alternatives, not serial fallbacks. All provider/account and model fields are declared configuration, not availability or eligibility.</Copy>
    {chain.rungs.map((rung) => <View key={rung.index} style={{ paddingVertical: 4 }}>
      <Copy colors={colors}>Rung {rung.index + 1} · {rung.alternatives.length} alternatives</Copy>
      {rung.alternatives.map((alternative, index) => <View key={index} style={{ paddingLeft: 8 }}>
        <Copy colors={colors}>Alternative {index + 1}: {alternative.reference} · selected effort: {alternative.selectedEffort ?? "Not supplied"}</Copy>
        <Copy colors={colors}>Model alias {alternative.modelAlias} · model ID {alternative.model?.id ?? "Unknown"} · vendor {alternative.model?.vendor ?? "Unknown"}</Copy>
        <Copy colors={colors}>Domain alias {alternative.model?.domain ?? "Unknown"} · provider {alternative.domain?.provider ?? "Unknown"} · account aliases {alternative.domain?.accounts.join(", ") || "Unknown"} · model prefix {alternative.domain?.modelPrefix ?? "Not supplied"}</Copy>
        <Copy colors={colors}>Declared model metadata: supported efforts {alternative.model?.supportedEfforts?.join(", ") ?? "Not supplied"}; vision {alternative.model ? String(alternative.model.vision) : "Unknown"}; calibrated {alternative.model ? String(alternative.model.calibrated) : "Unknown"}.</Copy>
        <Copy colors={colors} muted>Source provenance: {alternative.provenance.sourceId} · {alternative.provenance.pointer}{alternative.model ? ` · ${alternative.model.provenance.pointer}` : ""}{alternative.domain ? ` · ${alternative.domain.provenance.pointer}` : ""}</Copy>
        {alternative.diagnostics.map((diagnostic, issue) => <Copy key={issue} colors={colors}>{diagnostic.code} · {diagnostic.pointer}: {diagnostic.message}</Copy>)}
      </View>)}
    </View>)}
  </>;
}
const BINDINGS_PER_PAGE = 8;
function ConfiguredBindings({ data, colors }: { data: TeamsFleetData; colors: Colors }) {
  const [page, setPage] = useState(0);
  const pages = Math.max(1, Math.ceil(data.bindings.length / BINDINGS_PER_PAGE));
  const current = Math.min(page, pages - 1);
  const start = current * BINDINGS_PER_PAGE;
  return <Disclosure label={`Configured bindings (${data.bindings.length})`} colors={colors}>
    <Copy colors={colors}>Declared group/slot → chain mappings are known configuration. Template ID/version candidates and leadership identities remain separate and unresolved. These mappings do not assign native sessions or attest activation.</Copy>
    <Copy colors={colors}>Bindings {data.bindings.length ? start + 1 : 0}–{Math.min(start + BINDINGS_PER_PAGE, data.bindings.length)} of {data.bindings.length} · page {current + 1}/{pages}</Copy>
    <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
      <Action label="Previous bindings" colors={colors} disabled={current === 0} onPress={() => setPage(current - 1)} />
      <Action label="Next bindings" colors={colors} disabled={current + 1 >= pages} onPress={() => setPage(current + 1)} />
    </View>
    {data.bindings.slice(start, start + BINDINGS_PER_PAGE).map((binding) => {
      const chain = binding.chainResolved ? data.chains.find((candidate) => candidate.name === binding.chainName) : undefined;
      const independence = Object.entries(data.rules.independent_of).filter(([from, of]) => from === binding.reference || of === binding.reference);
      return <Disclosure key={binding.reference} label={`${binding.reference} → ${binding.chainName}`} colors={colors}>
        <Copy colors={colors}>groupKind: {binding.groupKind} · groupId: {binding.groupId} · slotId: {binding.slotId ?? "Not supplied"} · declared chainName: {binding.chainName}</Copy>
        <Copy colors={colors}>Binding source provenance: {binding.provenance.sourceId} · {binding.provenance.pointer}</Copy>
        <Copy colors={colors}>Declared rules for this binding: calibration_required {data.rules.calibration_required.includes(binding.reference) ? "declared" : "not declared"}; vision_required {data.rules.vision_required.includes(binding.reference) ? "declared" : "not declared"}. Policy eligibility is not evaluated.</Copy>
        <Copy colors={colors} muted>Rule source: {binding.provenance.sourceId} · /rules/calibration_required · /rules/vision_required · /rules/independent_of</Copy>
        {independence.length ? independence.map(([from, of]) => <Copy key={from} colors={colors}>Declared independent_of: {from} → {of}</Copy>) : <Copy colors={colors}>independent_of: no relationship declared for this binding.</Copy>}
        {chain ? <ChainDetails chain={chain} colors={colors} /> : <Copy colors={colors}>Declared chain {binding.chainName}: definition unavailable. Mapping retained; rungs and model metadata Unknown.</Copy>}
      </Disclosure>;
    })}
  </Disclosure>;
}
function SnapshotData({ snapshot, colors }: { snapshot: TeamsSourceSnapshot; colors: Colors }) {
  const data = snapshot.data;
  if (!data) return <Copy colors={colors}>Configured source unavailable. Other sources and native sessions remain usable.</Copy>;
  if (data.kind === "fleet") return <>
    <Copy colors={colors}>Fleet schema {data.schemaVersion} · {data.chains.length} chains · {data.models.length} model definitions · {data.bindings.length} bindings</Copy>
    <ConfiguredBindings data={data} colors={colors} />
    <Disclosure label={`Configured chains (${data.chains.length})`} colors={colors}>
      <Copy colors={colors}>Rungs keep source order. Alternatives within a rung are alternatives, not serial fallbacks. Targets are configuration, not running models or availability.</Copy>
      {data.chains.map((chain) => <View key={chain.name} style={{ paddingVertical: 4 }}>
        <Copy colors={colors}>{chain.name}</Copy>
        {chain.rungs.map((rung) => <Copy key={rung.index} colors={colors} muted>Rung {rung.index + 1}: {rung.alternatives.map((alternative) => `${alternative.reference}: ${alternative.domain?.provider ?? "Unknown provider"}/${alternative.model?.id ?? "Unknown model"}; effort ${alternative.selectedEffort ?? "Not supplied"}`).join(" | ")}</Copy>)}
      </View>)}
    </Disclosure>
  </>;
  if (data.kind === "roster") return <>
    <Copy colors={colors}>Core roster v{data.version} · role catalog {data.roleCatalog.catalog_id} v{data.roleCatalog.version}. This roster does not establish a leadership binding identity.</Copy>
    {data.seats.map((seat) => <Copy key={seat.role_slot_id} colors={colors}>{seat.role_slot_id} · {seat.role_code} · configured presence {seat.presence} · ad hoc allowed by definition: {String(seat.ad_hoc_allowed)}</Copy>)}
  </>;
  return <Disclosure label={`Bundled template defaults (${data.templates.length})`} colors={colors}>
    <Copy colors={colors}>Every source version is retained. Defaults are separate from fleet configuration and observed sessions.</Copy>
    {data.templates.map((template) => <View key={`${template.id}:${template.version}`}>
      <Copy colors={colors}>{template.name} · {template.id} v{template.version}</Copy>
      {template.slots.map((slot) => <Copy key={slot.id} colors={colors}>{slot.id} · role {slot.role.role} v{slot.role.version} · default rungs: {slot.defaultRungs.map((rung) => `${rung.provider}/${rung.model}${rung.effort ? ` @${rung.effort}` : ""}`).join(" → ")}</Copy>)}
    </View>)}
  </Disclosure>;
}
export function TeamsSourcePage({ sources, snapshots = {}, errors = {}, bindingDiagnostics = [], available, reads, entries, observedAt, colors, children }: {
  sources: readonly ReadSourceSummary[]; snapshots?: Readonly<Record<string, TeamsSourceSnapshot>>;
  errors?: Readonly<Record<string, string>>; bindingDiagnostics?: readonly string[]; available: boolean;
  reads: Readonly<Record<string, SourceRead>>; entries: readonly DirectoryEntry[]; observedAt: string;
  colors: Colors; children: ReactNode;
}) {
  const bindings = teamsSourceBindings(sources);
  const configured = sources.filter((source) => bindings.formats[source.id]);
  const view = composeTeamsSourceView(configured.flatMap((source) => snapshots[source.id] ? [snapshots[source.id]!] : []), entries.map((entry) => ({ nativeSessionId: entry.id, provider: entry.provider, model: entry.model, effort: null, observedAt })));
  const diagnostics = bindingDiagnostics.length ? bindingDiagnostics : bindings.diagnostics;
  return <>
    <Copy colors={colors}>Host configuration snapshots are separate from native sessions in the selected context. Activation, eligibility and account/model availability are not attested.</Copy>
    {!available ? <Copy colors={colors}>Teams source service unavailable or not read. Retained snapshots are stale.</Copy> : null}
    {diagnostics.length ? <Disclosure label="Teams adapter mapping diagnostics" colors={colors}>{diagnostics.map((message, index) => <Copy key={index} colors={colors}>{message}</Copy>)}</Disclosure> : null}
    {!configured.length ? <Copy colors={colors}>No authorized Teams adapter IDs configured. No paths or renamed IDs inferred.</Copy> : null}
    {configured.map((source) => {
      const snapshot = snapshots[source.id];
      return <Card key={source.id} title={`${source.title} · ${bindings.formats[source.id]}`} colors={colors}>
        <Copy colors={colors}>Source state: {snapshot?.status ?? "unavailable"}{available ? "" : " · stale"} · {snapshot?.configurationState ?? "Not read"}</Copy>
        <Copy colors={colors}>Read {snapshot?.readAt ?? "Not captured"} · source observation time: Not supplied · activation: {snapshot?.activation ?? "not-attested"}</Copy>
        {snapshot ? <><SnapshotDetails snapshot={snapshot} colors={colors} /><SnapshotData snapshot={snapshot} colors={colors} /></> : <Disclosure label="Source details" colors={colors}><Copy colors={colors}>{source.path} · {errors[source.id] ?? "Awaiting a successful adapter read; observed state Unknown."}</Copy></Disclosure>}
      </Card>;
    })}
    {view.metadata.length ? <Card title="Unresolved configuration metadata" colors={colors}>
      <Copy colors={colors}>{view.metadata.length} binding identities remain unresolved. Candidates do not select a template version; roster slots do not establish leadership identities.</Copy>
      <Disclosure label="Unresolved template/leadership details" colors={colors}>{view.metadata.map((metadata) => <View key={`${metadata.sourceId}:${metadata.bindingReference}`}>
        <Copy colors={colors}>{metadata.sourceId} · {metadata.bindingReference}: {metadata.state}. {metadata.reason}</Copy>
        {metadata.templateCandidates.map((candidate) => <Copy key={`${candidate.sourceId}:${candidate.templateId}:${candidate.version}:${candidate.slotId}`} colors={colors} muted>Candidate only: {candidate.sourceId} · {candidate.templateId} v{candidate.version} · slot {candidate.slotId}</Copy>)}
      </View>)}</Disclosure>
    </Card> : null}
    {sources.filter((source) => !bindings.formats[source.id]).map((source) => {
      const read = reads[source.id];
      return <Card key={source.id} title={`${source.title} · JSON record source`} colors={colors}>
        {read ? <><SourceDetails read={read} colors={colors} />{read.records?.teams.map((team) => <Copy key={team.id} colors={colors}>{team.title}: {team.roles.map((role) => `${role.role}: ${role.chain.join(" → ") || "Not captured"}`).join("; ")}</Copy>)}</> : <Copy colors={colors}>JSON record source unavailable or not read. This ID is not mapped to a YAML/template adapter format.</Copy>}
      </Card>;
    })}
    <Copy colors={colors}>Native observations: {view.observedSessions.length} sessions · observed {observedAt} · native effort: Not supplied. No configuration-to-session assignment is inferred. Governed fleet: kontord is not enabled.</Copy>
    {children}
  </>;
}
