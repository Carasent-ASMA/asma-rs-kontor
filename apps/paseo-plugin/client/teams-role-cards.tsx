import { useState, type ReactNode } from "react";
import { Pressable, Text, View } from "react-native";
import type { TeamsBindingMetadata, TeamsFleetData, TeamsTemplatesData } from "../shared/teams-source-snapshot";
import { Action, Card, Copy, type Colors } from "./page-controls";
import { BindingDetail } from "./teams-binding-detail";
const PAGE_SIZE = 8;
/** Serializable drill-down state; selections are stable IDs, never list positions. */
export type FleetCardsView = Readonly<{ group: string | null; reference: string | null; groupPage: number; rolePage: number }>;
export type TemplateCardsView = Readonly<{ template: string | null; slot: string | null; templatePage: number; slotPage: number }>;
export const DEFAULT_FLEET_CARDS_VIEW: FleetCardsView = { group: null, reference: null, groupPage: 0, rolePage: 0 };
export const DEFAULT_TEMPLATE_CARDS_VIEW: TemplateCardsView = { template: null, slot: null, templatePage: 0, slotPage: 0 };
type Binding = TeamsFleetData["bindings"][number];
const groupKey = (binding: Binding) => `${binding.groupKind}/${binding.groupId}`;
const templateKey = (template: { id: string; version: number }) => `${template.id}@${template.version}`;
export function filterMatches(filter: string, ...values: readonly (string | null | undefined)[]): boolean {
  const needle = filter.trim().toLocaleLowerCase();
  return !needle || values.some((value) => value?.toLocaleLowerCase().includes(needle));
}
const pageOf = (page: number, count: number) => Math.max(0, Math.min(Number.isInteger(page) ? page : 0, Math.ceil(count / PAGE_SIZE) - 1));
/** Drops a group or role that the current snapshot no longer declares. */
export function currentFleetView(view: Partial<FleetCardsView>, data: TeamsFleetData): FleetCardsView {
  const saved = { ...DEFAULT_FLEET_CARDS_VIEW, ...view };
  const group = data.bindings.some((binding) => groupKey(binding) === saved.group) ? saved.group : null;
  const reference = data.bindings.some((binding) => groupKey(binding) === group && binding.reference === saved.reference) ? saved.reference : null;
  return { ...saved, group, reference, rolePage: group === saved.group ? saved.rolePage : 0 };
}
/** Drops a template version or slot that the current snapshot no longer declares. */
export function currentTemplateView(view: Partial<TemplateCardsView>, data: TeamsTemplatesData): TemplateCardsView {
  const saved = { ...DEFAULT_TEMPLATE_CARDS_VIEW, ...view };
  const template = data.templates.find((item) => templateKey(item) === saved.template);
  const slot = template?.slots.some((item) => item.id === saved.slot) ? saved.slot : null;
  return { ...saved, template: template ? saved.template : null, slot, slotPage: template ? saved.slotPage : 0 };
}
function useCardsView<T>(fallback: T, view: T | undefined, onView: ((view: T) => void) | undefined): [T, (next: T) => void] {
  const [local, setLocal] = useState<T>(fallback);
  return [view ?? local, (next) => { if (view === undefined) setLocal(next); onView?.(next); }];
}
function Paging({ page, count, onPage, colors, label }: { page: number; count: number; onPage: (page: number) => void; colors: Colors; label: string }) {
  const pages = Math.max(1, Math.ceil(count / PAGE_SIZE));
  if (pages === 1) return <Copy colors={colors} muted small>{count} {label}</Copy>;
  return <View style={{ flexDirection: "row", flexWrap: "wrap", alignItems: "center" }}><Action label={`Previous ${label}`} icon="ChevronLeft" iconOnly disabled={page === 0} colors={colors} onPress={() => onPage(page - 1)} /><Copy colors={colors} muted small>{page + 1}/{pages} · {count} {label}</Copy><Action label={`Next ${label}`} icon="ChevronRight" iconOnly disabled={page + 1 >= pages} colors={colors} onPress={() => onPage(page + 1)} /></View>;
}
/** Compact selectable card, as in the prototype template grid. */
function CardButton({ title, meta, detail, accessibilityLabel, onPress, colors }: { title: string; meta: string; detail?: string; accessibilityLabel: string; onPress: () => void; colors: Colors }) {
  return <Pressable accessibilityRole="button" accessibilityLabel={accessibilityLabel} onPress={onPress} style={{ minHeight: 44, flexGrow: 1, flexBasis: 220, padding: 10, gap: 2, justifyContent: "center", borderWidth: 1, borderColor: colors.border, borderRadius: 8, backgroundColor: colors.surface1 }}>
    <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{title}</Text>
    <Text numberOfLines={2} style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>{meta}</Text>
    {detail ? <Text numberOfLines={1} style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>{detail}</Text> : null}
  </Pressable>;
}
function Grid({ children }: { children: ReactNode }) {
  return <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>{children}</View>;
}

export function FleetRoleCards({ data, metadata, colors, filter = "", view, onView }: { data: TeamsFleetData; metadata: readonly TeamsBindingMetadata[]; colors: Colors; filter?: string; view?: FleetCardsView; onView?: (view: FleetCardsView) => void }) {
  const [stored, store] = useCardsView(DEFAULT_FLEET_CARDS_VIEW, view, onView);
  const current = currentFleetView(stored, data);
  const update = (patch: Partial<FleetCardsView>) => store({ ...current, ...patch });
  // A matching group shows all of its roles; otherwise only the matching roles.
  const matches = (binding: Binding) => filterMatches(filter, groupKey(binding), binding.reference, binding.slotId, binding.chainName);
  const groups = [...new Set(data.bindings.filter(matches).map(groupKey))];
  const all = data.bindings.filter((binding) => groupKey(binding) === current.group);
  const roles = all.filter(matches);
  const selected = all.find((binding) => binding.reference === current.reference);
  const groupPage = pageOf(current.groupPage, groups.length);
  const rolePage = pageOf(current.rolePage, roles.length);
  if (current.group === null) return <View style={{ gap: 6 }}>
    <Paging page={groupPage} count={groups.length} onPage={(page) => update({ groupPage: page })} colors={colors} label="groups" />
    {!groups.length ? <Copy colors={colors} muted>No configured group or role matches “{filter.trim()}”.</Copy> : null}
    <Grid>{groups.slice(groupPage * PAGE_SIZE, (groupPage + 1) * PAGE_SIZE).map((group) => {
      const members = data.bindings.filter((binding) => groupKey(binding) === group);
      return <CardButton key={group} title={group} meta={`${members.length} declared bindings · definition identity unresolved`} detail={members.map((binding) => binding.slotId ?? binding.groupId).join(" › ")}
        accessibilityLabel={`Roles: ${group}`} colors={colors} onPress={() => update({ group, reference: null, rolePage: 0 })} />;
    })}</Grid>
  </View>;
  return <View style={{ gap: 8 }}>
    <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
      <Action label="All configured groups" icon="ChevronLeft" colors={colors} onPress={() => update({ group: null, reference: null })} />
      {selected ? <Action label="All group roles" icon="ChevronLeft" colors={colors} onPress={() => update({ reference: null })} /> : null}
    </View>
    <Copy colors={colors}>{current.group} · {roles.length === all.length ? all.length : `${roles.length} of ${all.length}`} declared roles · no native assignment inferred</Copy>
    {selected ? <Card title={`${selected.slotId ?? selected.groupId} → ${selected.chainName}`} colors={colors}>
      <BindingDetail binding={selected} data={data} colors={colors} />
      {metadata.filter((item) => item.bindingReference === selected.reference).map((item) => <View key={item.sourceId}>
        <Copy colors={colors}>Template / leadership identity: {item.state}. {item.reason}</Copy>
        {item.templateCandidates.map((candidate) => <Copy key={`${candidate.sourceId}:${candidate.templateId}:${candidate.version}:${candidate.slotId}`} colors={colors}>Candidate only: {candidate.templateId} v{candidate.version} · {candidate.slotId}</Copy>)}
      </View>)}
    </Card> : <>
      {roles.length > PAGE_SIZE ? <Paging page={rolePage} count={roles.length} onPage={(page) => update({ rolePage: page })} colors={colors} label="roles" /> : null}
      {!roles.length ? <Copy colors={colors} muted>No role in this group matches “{filter.trim()}”.</Copy> : null}
      <Grid>{roles.slice(rolePage * PAGE_SIZE, (rolePage + 1) * PAGE_SIZE).map((role) => <CardButton key={role.reference} title={role.slotId ?? role.groupId} meta={`→ ${role.chainName} · ${role.chainResolved ? "definition captured" : "definition unavailable"}`} detail="Declared chain · activation not attested"
        accessibilityLabel={`Role: ${role.reference} → ${role.chainName}`} colors={colors} onPress={() => update({ reference: role.reference })} />)}</Grid>
    </>}
  </View>;
}

export function TemplateRoleCards({ data, colors, filter = "", view, onView }: { data: TeamsTemplatesData; colors: Colors; filter?: string; view?: TemplateCardsView; onView?: (view: TemplateCardsView) => void }) {
  const [stored, store] = useCardsView(DEFAULT_TEMPLATE_CARDS_VIEW, view, onView);
  const current = currentTemplateView(stored, data);
  const update = (patch: Partial<TemplateCardsView>) => store({ ...current, ...patch });
  type Template = TeamsTemplatesData["templates"][number];
  const named = (template: Template) => filterMatches(filter, template.name, template.id);
  const slotMatches = (slot: Template["slots"][number]) => filterMatches(filter, slot.id, slot.role.role);
  const templates = data.templates.filter((item) => named(item) || item.slots.some(slotMatches));
  const template = data.templates.find((item) => templateKey(item) === current.template);
  const slot = template?.slots.find((item) => item.id === current.slot);
  const slots = template ? template.slots.filter((item) => named(template) || slotMatches(item)) : [];
  const templatePage = pageOf(current.templatePage, templates.length);
  const slotPage = pageOf(current.slotPage, slots.length);
  if (!template) return <View style={{ gap: 6 }}>
    <Paging page={templatePage} count={templates.length} onPage={(page) => update({ templatePage: page })} colors={colors} label="templates" />
    {!templates.length ? <Copy colors={colors} muted>No template or template role matches “{filter.trim()}”.</Copy> : null}
    <Grid>{templates.slice(templatePage * PAGE_SIZE, (templatePage + 1) * PAGE_SIZE).map((item) => <CardButton key={templateKey(item)} title={`${item.name} · v${item.version}`} meta={`${item.slots.length} declared slots · ${item.handoffs.length} handoffs · bundled defaults, not activated`} detail={item.slots.map((entry) => entry.id).join(" › ")}
      accessibilityLabel={`Template: ${item.id} v${item.version}`} colors={colors} onPress={() => update({ template: templateKey(item), slot: null, slotPage: 0 })} />)}</Grid>
  </View>;
  return <View style={{ gap: 8 }}>
    <View style={{ flexDirection: "row", flexWrap: "wrap" }}>
      <Action label="All template versions" icon="ChevronLeft" colors={colors} onPress={() => update({ template: null, slot: null })} />
      {slot ? <Action label="All template roles" icon="ChevronLeft" colors={colors} onPress={() => update({ slot: null })} /> : null}
    </View>
    <Copy colors={colors}>{template.name} · {template.id} v{template.version}</Copy>
    {slot ? <Card title={`${slot.id} · ${slot.role.role} v${slot.role.version}`} colors={colors}>
      <Copy colors={colors}>Ordered template defaults, separate from configured fleet alternatives and observed sessions.</Copy>
      {slot.defaultRungs.map((rung, index) => <Copy key={index} colors={colors}>Default rung {index + 1}: {rung.provider}/{rung.model} · effort {rung.effort ?? "Not supplied"}</Copy>)}
      <Copy colors={colors}>Skills: {slot.skills.map((skill) => `${skill.skill} v${skill.version}`).join(", ") || "None declared"}</Copy>
      <Copy colors={colors}>Context: {slot.context ? `${slot.context.template} v${slot.context.version}` : "Not declared"} · autonomy {slot.autonomy ?? "Not declared"}</Copy>
      <Copy colors={colors}>May evaluate: {slot.mayEvaluate.join(", ") || "None declared"} · may waive: {slot.mayWaive.join(", ") || "None declared"}. Declared policy, not execution authority.</Copy>
      {slot.waiverPolicy ? <Copy colors={colors}>Waiver definition: roles {slot.waiverPolicy.authorized_roles.join(", ")} · evidence {slot.waiverPolicy.required_evidence.join(", ")}</Copy> : null}
      {template.handoffs.filter((handoff) => handoff.from_slot === slot.id || handoff.to_slot === slot.id).map((handoff, index) => <Copy key={index} colors={colors}>Declared handoff {handoff.from_slot} → {handoff.to_slot} · after {handoff.after_phase ?? "Not declared"} · artifacts {handoff.required_artifacts.join(", ") || "None declared"}</Copy>)}
      <Copy colors={colors}>Source provenance: {slot.provenance.sourceId} · {slot.provenance.pointer}. Eligibility and identity binding remain unattested.</Copy>
    </Card> : <>
      {slots.length > PAGE_SIZE ? <Paging page={slotPage} count={slots.length} onPage={(page) => update({ slotPage: page })} colors={colors} label="template roles" /> : null}
      {!slots.length ? <Copy colors={colors} muted>No role in this template matches “{filter.trim()}”.</Copy> : null}
      <Grid>{slots.slice(slotPage * PAGE_SIZE, (slotPage + 1) * PAGE_SIZE).map((item) => <CardButton key={item.id} title={item.id} meta={`${item.role.role} v${item.role.version} · ${item.defaultRungs.length} default rungs`} detail={`Autonomy ${item.autonomy ?? "not declared"} · not activated`}
        accessibilityLabel={`Template role: ${item.id} · ${item.role.role} v${item.role.version}`} colors={colors} onPress={() => update({ slot: item.id })} />)}</Grid>
    </>}
  </View>;
}
