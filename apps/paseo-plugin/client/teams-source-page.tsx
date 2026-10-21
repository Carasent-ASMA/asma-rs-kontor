import { BindingDetail } from "./teams-binding-detail";
import { currentFleetView, currentTemplateView, DEFAULT_FLEET_CARDS_VIEW, DEFAULT_TEMPLATE_CARDS_VIEW, filterMatches, FleetRoleCards, TemplateRoleCards, type FleetCardsView, type TemplateCardsView } from "./teams-role-cards";
import { useState, type ReactNode } from "react";
import { View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import type { DirectoryEntry } from "./session-directory";
import type { ReadSourceSummary, SourceRead } from "../shared/read-sources";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import { composeTeamsSourceView, type TeamsBindingMetadata, type TeamsFleetData, type TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { Action, Card, Copy, Disclosure, type Colors } from "./page-controls";
/** Per-source drill-down, keyed by configured source ID. Open bindings are keyed by binding reference. */
export type TeamsSourceView = Readonly<{
  details: boolean; bindings: boolean; bindingPage: number; openBindings: readonly string[]; chains: boolean;
  fleet: FleetCardsView; templates: TemplateCardsView;
}>;
/** Serializable Teams page state: every disclosure, selection, page index and the filter text. */
export type TeamsView = Readonly<{ configured: boolean; filter: string; diagnostics: boolean; unresolved: boolean; sources: Readonly<Record<string, TeamsSourceView>> }>;
export const DEFAULT_TEAMS_SOURCE_VIEW: TeamsSourceView = { details: false, bindings: false, bindingPage: 0, openBindings: [], chains: false, fleet: DEFAULT_FLEET_CARDS_VIEW, templates: DEFAULT_TEMPLATE_CARDS_VIEW };
export const DEFAULT_TEAMS_VIEW: TeamsView = { configured: false, filter: "", diagnostics: false, unresolved: false, sources: {} };
/**
 * Restores a saved view against the current catalog and snapshots: missing fields take defaults,
 * sources no longer in the catalog and selections the snapshot no longer declares are dropped.
 * A source without readable data keeps its selections; their absence is not proven.
 */
function restoreTeamsView(view: TeamsView | undefined, sources: readonly ReadSourceSummary[], snapshots: Readonly<Record<string, TeamsSourceSnapshot>>): TeamsView {
  const saved: TeamsView = { ...DEFAULT_TEAMS_VIEW, ...(typeof view === "object" && view !== null ? view : {}) };
  const known = new Set(sources.map((source) => source.id));
  const restored: Record<string, TeamsSourceView> = {};
  for (const [sourceId, item] of Object.entries(saved.sources ?? {})) {
    if (known.size && !known.has(sourceId)) continue;
    const data = snapshots[sourceId]?.data;
    const value: TeamsSourceView = { ...DEFAULT_TEAMS_SOURCE_VIEW, ...item };
    restored[sourceId] = data?.kind === "fleet" ? { ...value, openBindings: value.openBindings.filter((reference) => data.bindings.some((binding) => binding.reference === reference)), fleet: currentFleetView(value.fleet, data) }
      : data?.kind === "templates" ? { ...value, templates: currentTemplateView(value.templates, data) } : value;
  }
  return { ...saved, sources: restored };
}
/** `notes` are the caller's moved summary lines, shown first when expanded. */
export function TeamsSnapshotDetails({ snapshot, colors, open, onToggle, notes }: { snapshot: TeamsSourceSnapshot; colors: Colors; open?: boolean; onToggle?: (open: boolean) => void; notes?: ReactNode }) {
  return <Disclosure label="Source details" colors={colors} open={open} onToggle={onToggle}>
    {notes}
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
type SourceProps = { view: TeamsSourceView; onView: (patch: Partial<TeamsSourceView>) => void; filter: string; colors: Colors };
const BINDINGS_PER_PAGE = 8;
function ConfiguredBindings({ data, view, onView, filter, colors }: SourceProps & { data: TeamsFleetData }) {
  const bindings = data.bindings.filter((binding) => filterMatches(filter, binding.reference, binding.slotId, binding.chainName));
  const pages = Math.max(1, Math.ceil(bindings.length / BINDINGS_PER_PAGE));
  const current = Math.max(0, Math.min(view.bindingPage, pages - 1));
  const start = current * BINDINGS_PER_PAGE;
  return <Disclosure label={`Configured bindings (${bindings.length === data.bindings.length ? data.bindings.length : `${bindings.length} of ${data.bindings.length}`})`} colors={colors} open={view.bindings} onToggle={(open) => onView({ bindings: open })}>
    <Copy colors={colors} muted small>Declared group/slot → chain mappings are known configuration. Template ID/version candidates and leadership identities remain separate and unresolved. These mappings do not assign native sessions or attest activation.</Copy>
    <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center" }}>
      <Action label="Previous bindings" icon="ChevronLeft" iconOnly colors={colors} disabled={current === 0} onPress={() => onView({ bindingPage: current - 1 })} />
      <Copy colors={colors} muted small>Bindings {bindings.length ? start + 1 : 0}–{Math.min(start + BINDINGS_PER_PAGE, bindings.length)} of {bindings.length} · page {current + 1}/{pages}</Copy>
      <Action label="Next bindings" icon="ChevronRight" iconOnly colors={colors} disabled={current + 1 >= pages} onPress={() => onView({ bindingPage: current + 1 })} />
    </View>
    {bindings.slice(start, start + BINDINGS_PER_PAGE).map((binding) => <Disclosure key={binding.reference} label={`${binding.reference} → ${binding.chainName}`} colors={colors} open={view.openBindings.includes(binding.reference)}
      onToggle={(open) => onView({ openBindings: open ? [...view.openBindings, binding.reference] : view.openBindings.filter((reference) => reference !== binding.reference) })}>
      <BindingDetail binding={binding} data={data} colors={colors} />
    </Disclosure>)}
  </Disclosure>;
}
function SnapshotData({ snapshot, metadata, view, onView, filter, colors }: SourceProps & { snapshot: TeamsSourceSnapshot; metadata: readonly TeamsBindingMetadata[] }) {
  const data = snapshot.data;
  if (!data) return <Copy colors={colors}>Configured source unavailable. Other sources and native sessions remain usable.</Copy>;
  if (data.kind === "fleet") return <>
    <Copy colors={colors} muted small>Fleet schema {data.schemaVersion} · {data.chains.length} chains · {data.models.length} model definitions · {data.bindings.length} bindings</Copy>
    <FleetRoleCards key={snapshot.source.id} data={data} metadata={metadata.filter((item) => item.sourceId === snapshot.source.id)} filter={filter} view={view.fleet} onView={(fleet) => onView({ fleet })} colors={colors} />
    <ConfiguredBindings data={data} view={view} onView={onView} filter={filter} colors={colors} />
    <Disclosure label={`Configured chains (${data.chains.length})`} colors={colors} open={view.chains} onToggle={(open) => onView({ chains: open })}>
      <Copy colors={colors} muted small>Rungs keep source order. Alternatives within a rung are alternatives, not serial fallbacks. Targets are configuration, not running models or availability.</Copy>
      {data.chains.map((chain) => <View key={chain.name} style={{ paddingVertical: 4 }}>
        <Copy colors={colors}>{chain.name}</Copy>
        {chain.rungs.map((rung) => <Copy key={rung.index} colors={colors} muted>Rung {rung.index + 1}: {rung.alternatives.map((alternative) => `${alternative.reference}: ${alternative.domain?.provider ?? "Unknown provider"}/${alternative.model?.id ?? "Unknown model"}; effort ${alternative.selectedEffort ?? "Not supplied"}`).join(" | ")}</Copy>)}
      </View>)}
    </Disclosure>
  </>;
  if (data.kind === "roster") {
    const seats = data.seats.filter((seat) => filterMatches(filter, seat.role_slot_id, seat.role_code));
    return <>
      <Copy colors={colors} muted small>Core roster v{data.version} · role catalog {data.roleCatalog.catalog_id} v{data.roleCatalog.version}. This roster does not establish a leadership binding identity.</Copy>
      {!seats.length ? <Copy colors={colors} muted>No roster seat matches “{filter.trim()}”.</Copy> : null}
      {seats.map((seat) => <Copy key={seat.role_slot_id} colors={colors}>{seat.role_slot_id} · {seat.role_code} · configured presence {seat.presence} · ad hoc allowed by definition: {String(seat.ad_hoc_allowed)}</Copy>)}
    </>;
  }
  return <>
    <Copy colors={colors} muted small>Bundled template defaults · {data.templates.length} template versions. Every source version is retained. Defaults are separate from fleet configuration and observed sessions.</Copy>
    <TemplateRoleCards key={snapshot.source.id} data={data} filter={filter} view={view.templates} onView={(templates) => onView({ templates })} colors={colors} />
  </>;
}
/** Shown with the native observations; the context session list carries it in its header label. */
export const TEAMS_SESSIONS_NOTE = "Native sessions and labels are observations, not activated configured teams. Planned or governed assignment: Unknown.";
function sourceProblem(snapshot: TeamsSourceSnapshot | undefined, error: string | undefined, available: boolean): string | null {
  if (!snapshot) return error ? `adapter request failed: ${error}` : "not read · observed state Unknown";
  if (snapshot.status === "ready" && !error) return null;
  const diagnostics = snapshot.diagnostics.length ? ` · ${snapshot.diagnostics.length} diagnostic${snapshot.diagnostics.length === 1 ? "" : "s"} in Source details` : "";
  return `${snapshot.status}${available && !error ? "" : " · retained snapshot stale"}${error ? ` · adapter request failed: ${error}` : ""}${diagnostics}`;
}
/**
 * Project context first (children: the observed sessions in this context), then configured
 * fleet/template browsing as a separate, collapsed section. Configuration never assigns sessions.
 * Pass view/onView to keep the drill-down in a route store; without them the page keeps it locally.
 */
export function TeamsSourcePage({ sources, snapshots = {}, errors = {}, bindingDiagnostics = [], available, reads, entries, observedAt, colors, children, view, onView }: {
  sources: readonly ReadSourceSummary[]; snapshots?: Readonly<Record<string, TeamsSourceSnapshot>>;
  errors?: Readonly<Record<string, string>>; bindingDiagnostics?: readonly string[]; available: boolean;
  reads: Readonly<Record<string, SourceRead>>; entries: readonly DirectoryEntry[]; observedAt: string;
  colors: Colors; children: ReactNode; view?: TeamsView; onView?: (view: TeamsView) => void;
}) {
  const [local, setLocal] = useState<TeamsView>(DEFAULT_TEAMS_VIEW);
  const current = restoreTeamsView(view ?? local, sources, snapshots);
  const emit = (next: TeamsView) => { if (view === undefined) setLocal(next); onView?.(next); };
  const sourceView = (id: string) => current.sources[id] ?? DEFAULT_TEAMS_SOURCE_VIEW;
  const updateSource = (id: string) => (patch: Partial<TeamsSourceView>) => emit({ ...current, sources: { ...current.sources, [id]: { ...sourceView(id), ...patch } } });
  // A new filter starts every list at its first page.
  const setFilter = (filter: string) => emit({ ...current, filter, sources: Object.fromEntries(Object.entries(current.sources).map(([id, item]) => [id, { ...item, bindingPage: 0, fleet: { ...item.fleet, groupPage: 0, rolePage: 0 }, templates: { ...item.templates, templatePage: 0, slotPage: 0 } }])) });
  const bindings = teamsSourceBindings(sources);
  const configured = sources.filter((source) => bindings.formats[source.id]);
  const records = sources.filter((source) => !bindings.formats[source.id]);
  const composed = composeTeamsSourceView(configured.flatMap((source) => snapshots[source.id] ? [snapshots[source.id]!] : []), entries.map((entry) => ({ nativeSessionId: entry.id, provider: entry.provider, model: entry.model, effort: null, observedAt })));
  const diagnostics = bindingDiagnostics.length ? bindingDiagnostics : bindings.diagnostics;
  const problems = [
    ...configured.flatMap((source) => { const problem = sourceProblem(snapshots[source.id], errors[source.id], available); return problem ? [{ id: source.id, text: `${source.title} · ${bindings.formats[source.id]}: ${problem}` }] : []; }),
    ...records.flatMap((source) => { const read = reads[source.id]; return !read || read.status !== "ready" ? [{ id: source.id, text: `${source.title} · JSON record source: ${read ? read.status : "unavailable or not read"}` }] : []; }),
  ];
  // How native sessions relate to configuration: inside the configuration disclosure when there is one, otherwise shown.
  const observations = <View testID="teams-observations" style={{ gap: 4 }}>
    <Copy colors={colors} muted small>{TEAMS_SESSIONS_NOTE}</Copy>
    <Copy colors={colors} muted small>Native observations: {composed.observedSessions.length} sessions · observed {observedAt} · native effort: Not supplied. No configuration-to-session assignment is inferred. Governed fleet: kontord is not enabled.</Copy>
  </View>;
  return <>
    {children}
    {sources.length ? null : observations}
    <View testID="teams-configuration" style={{ gap: 6, paddingTop: 10, borderTopWidth: 1, borderColor: colors.border }}>
      {!available ? <Copy colors={colors}>Teams source service unavailable or not read. Retained snapshots are stale.</Copy> : null}
      {!configured.length ? <Copy colors={colors}>No authorized Teams adapter IDs configured. No paths or renamed IDs inferred.</Copy> : null}
      {diagnostics.length ? <Disclosure label={`Teams adapter mapping diagnostics (${diagnostics.length})`} colors={colors} open={current.diagnostics} onToggle={(open) => emit({ ...current, diagnostics: open })}>{diagnostics.map((message, index) => <Copy key={index} colors={colors}>{message}</Copy>)}</Disclosure> : null}
      {sources.length ? <Disclosure label={`Configured teams & templates (${sources.length} ${sources.length === 1 ? "source" : "sources"})`} colors={colors} open={current.configured} onToggle={(open) => emit({ ...current, configured: open })}>
        <View style={{ gap: 10, paddingTop: 4 }}>
          {observations}
          <Copy colors={colors} muted small>Host configuration snapshots, separate from the native sessions above. Activation, eligibility and account/model availability are not attested.</Copy>
          <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center", gap: 6 }}>
            <TextInput accessibilityLabel="Filter configured teams, roles and templates" placeholder="Filter by template, group or role name" value={current.filter} onChangeText={setFilter}
              style={{ flexGrow: 1, flexBasis: 220, color: colors.foreground, minHeight: 44, paddingHorizontal: 10, borderWidth: 1, borderColor: colors.border, borderRadius: 8 }} />
            {current.filter ? <Action label="Clear filter" icon="X" iconOnly colors={colors} onPress={() => setFilter("")} /> : null}
          </View>
          {configured.map((source) => {
            const snapshot = snapshots[source.id];
            const error = errors[source.id];
            const item = sourceView(source.id);
            const update = updateSource(source.id);
            return <Card key={source.id} title={`${source.title} · ${bindings.formats[source.id]}`} colors={colors}>
              <Copy colors={colors} small>Source state: {snapshot?.status ?? "unavailable"}{available ? "" : " · stale"} · {snapshot?.configurationState ?? "Not read"}</Copy>
              <Copy colors={colors} muted small>Read {snapshot?.readAt ?? "Not captured"} · source observation time: Not supplied · activation: {snapshot?.activation ?? "not-attested"}</Copy>
              {error ? <Copy colors={colors}>Adapter request failed: {error}</Copy> : null}
              {snapshot ? <><SnapshotData snapshot={snapshot} metadata={composed.metadata} view={item} onView={update} filter={current.filter} colors={colors} /><TeamsSnapshotDetails snapshot={snapshot} colors={colors} open={item.details} onToggle={(open) => update({ details: open })} /></>
                : <Disclosure label="Source details" colors={colors} open={item.details} onToggle={(open) => update({ details: open })}><Copy colors={colors}>{source.path} · {error ?? "Awaiting a successful adapter read; observed state Unknown."}</Copy></Disclosure>}
            </Card>;
          })}
          {records.map((source) => {
            const read = reads[source.id];
            const item = sourceView(source.id);
            const teams = read?.records?.teams.filter((team) => filterMatches(current.filter, team.title, ...team.roles.map((role) => role.role))) ?? [];
            return <Card key={source.id} title={`${source.title} · JSON record source`} colors={colors}>
              {read ? <>
                <Copy colors={colors} small>{read.status} · {read.source.revisionKind} · Read {read.readAt}</Copy>
                {teams.map((team) => <Copy key={team.id} colors={colors}>{team.title}: {team.roles.map((role) => `${role.role}: ${role.chain.join(" → ") || "Not captured"}`).join("; ")}</Copy>)}
                <Disclosure label="Source details" colors={colors} open={item.details} onToggle={(open) => updateSource(source.id)({ details: open })}>
                  <Copy colors={colors}>{read.source.path}</Copy>
                  <Copy colors={colors}>SHA-256 {read.revision ?? "Unknown"} · modified {read.modifiedAt ?? "Unknown"}</Copy>
                  {read.diagnostics.map((diagnostic, index) => <Copy key={index} colors={colors}>{diagnostic}</Copy>)}
                </Disclosure>
              </> : <Copy colors={colors}>JSON record source unavailable or not read. This ID is not mapped to a YAML/template adapter format.</Copy>}
            </Card>;
          })}
          {composed.metadata.length ? <View style={{ gap: 2 }}>
            <Copy colors={colors} muted small>{composed.metadata.length} binding identities remain unresolved. Candidates do not select a template version; roster slots do not establish leadership identities.</Copy>
            <Disclosure label={`Unresolved template/leadership details (${composed.metadata.length})`} colors={colors} open={current.unresolved} onToggle={(open) => emit({ ...current, unresolved: open })}>{composed.metadata.map((metadata) => <View key={`${metadata.sourceId}:${metadata.bindingReference}`}>
              <Copy colors={colors}>{metadata.sourceId} · {metadata.bindingReference}: {metadata.state}. {metadata.reason}</Copy>
              {metadata.templateCandidates.map((candidate) => <Copy key={`${candidate.sourceId}:${candidate.templateId}:${candidate.version}:${candidate.slotId}`} colors={colors} muted>Candidate only: {candidate.sourceId} · {candidate.templateId} v{candidate.version} · slot {candidate.slotId}</Copy>)}
            </View>)}</Disclosure>
          </View> : null}
        </View>
      </Disclosure> : null}
      {/* Collapsed configuration still reports failed, partial and unread sources. */}
      {!current.configured ? problems.map((problem) => <Copy key={problem.id} colors={colors}>{problem.text}</Copy>) : null}
    </View>
  </>;
}
