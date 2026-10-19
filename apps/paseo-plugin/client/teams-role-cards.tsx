import { useState } from "react";
import { View } from "react-native";
import type { TeamsBindingMetadata, TeamsFleetData, TeamsTemplatesData } from "../shared/teams-source-snapshot";
import { Action, Card, Copy, type Colors } from "./page-controls";
import { BindingDetail } from "./teams-binding-detail";
const PAGE_SIZE = 8;
function Paging({ page, count, onPage, colors, label }: { page: number; count: number; onPage: (page: number) => void; colors: Colors; label: string }) {
  const pages = Math.max(1, Math.ceil(count / PAGE_SIZE));
  return <View style={{ flexDirection: "row", flexWrap: "wrap" }}><Action label={`Previous ${label}`} disabled={page === 0} colors={colors} onPress={() => onPage(page - 1)} /><Copy colors={colors}>{page + 1}/{pages} · {count} {label}</Copy><Action label={`Next ${label}`} disabled={page + 1 >= pages} colors={colors} onPress={() => onPage(page + 1)} /></View>;
}

export function FleetRoleCards({ data, metadata, colors }: { data: TeamsFleetData; metadata: readonly TeamsBindingMetadata[]; colors: Colors }) {
  const [groupId, setGroupId] = useState<string | null>(null);
  const [reference, setReference] = useState<string | null>(null);
  const [groupPage, setGroupPage] = useState(0);
  const [rolePage, setRolePage] = useState(0);
  const groups = [...new Set(data.bindings.map((binding) => `${binding.groupKind}/${binding.groupId}`))];
  const roles = data.bindings.filter((binding) => `${binding.groupKind}/${binding.groupId}` === groupId);
  const selected = roles.find((binding) => binding.reference === reference);
  const visibleGroupPage = Math.min(groupPage, Math.max(0, Math.ceil(groups.length / PAGE_SIZE) - 1));
  const visibleRolePage = Math.min(rolePage, Math.max(0, Math.ceil(roles.length / PAGE_SIZE) - 1));
  if (!roles.length) return <View style={{ gap: 6 }}><Paging page={visibleGroupPage} count={groups.length} onPage={setGroupPage} colors={colors} label="groups" />{groups.slice(visibleGroupPage * PAGE_SIZE, (visibleGroupPage + 1) * PAGE_SIZE).map((group) => <Card key={group} title={group} colors={colors}>
    <Action label={`Roles: ${group}`} colors={colors} onPress={() => { setGroupId(group); setReference(null); setRolePage(0); }} />
    <Copy colors={colors}>{data.bindings.filter((binding) => `${binding.groupKind}/${binding.groupId}` === group).length} declared bindings · definition identity unresolved</Copy>
  </Card>)}</View>;
  return <View style={{ gap: 8 }}>
    <Action label="All configured groups" colors={colors} onPress={() => { setGroupId(null); setReference(null); }} />
    <Copy colors={colors}>{groupId} · {roles.length} declared roles · no native assignment inferred</Copy>
    {selected ? <>
      <Action label="All group roles" colors={colors} onPress={() => setReference(null)} />
      <Card title={`${selected.slotId ?? selected.groupId} → ${selected.chainName}`} colors={colors}>
        <BindingDetail binding={selected} data={data} colors={colors} />
        {metadata.filter((item) => item.bindingReference === selected.reference).map((item) => <View key={item.sourceId}>
          <Copy colors={colors}>Template / leadership identity: {item.state}. {item.reason}</Copy>
          {item.templateCandidates.map((candidate) => <Copy key={`${candidate.sourceId}:${candidate.templateId}:${candidate.version}:${candidate.slotId}`} colors={colors}>Candidate only: {candidate.templateId} v{candidate.version} · {candidate.slotId}</Copy>)}
        </View>)}
      </Card>
    </> : <><Paging page={visibleRolePage} count={roles.length} onPage={setRolePage} colors={colors} label="roles" />{roles.slice(visibleRolePage * PAGE_SIZE, (visibleRolePage + 1) * PAGE_SIZE).map((role) => <Card key={role.reference} title={role.slotId ?? role.groupId} colors={colors}>
      <Action label={`Role: ${role.reference} → ${role.chainName}`} colors={colors} onPress={() => setReference(role.reference)} />
      <Copy colors={colors}>Declared chain · {role.chainResolved ? "definition captured" : "definition unavailable"} · activation not attested</Copy>
    </Card>)}</>}
  </View>;
}

export function TemplateRoleCards({ data, colors }: { data: TeamsTemplatesData; colors: Colors }) {
  const [templateKey, setTemplateKey] = useState<string | null>(null);
  const [slotId, setSlotId] = useState<string | null>(null);
  const [templatePage, setTemplatePage] = useState(0);
  const [slotPage, setSlotPage] = useState(0);
  const template = data.templates.find((item) => `${item.id}@${item.version}` === templateKey);
  const slot = template?.slots.find((item) => item.id === slotId);
  const visibleTemplatePage = Math.min(templatePage, Math.max(0, Math.ceil(data.templates.length / PAGE_SIZE) - 1));
  const visibleSlotPage = Math.min(slotPage, Math.max(0, Math.ceil((template?.slots.length ?? 0) / PAGE_SIZE) - 1));
  if (!template) return <View style={{ gap: 6 }}><Paging page={visibleTemplatePage} count={data.templates.length} onPage={setTemplatePage} colors={colors} label="templates" />{data.templates.slice(visibleTemplatePage * PAGE_SIZE, (visibleTemplatePage + 1) * PAGE_SIZE).map((item) => <Card key={`${item.id}@${item.version}`} title={`${item.name} · v${item.version}`} colors={colors}>
    <Action label={`Template: ${item.id} v${item.version}`} colors={colors} onPress={() => { setTemplateKey(`${item.id}@${item.version}`); setSlotId(null); setSlotPage(0); }} />
    <Copy colors={colors}>{item.slots.length} declared slots · bundled defaults, not activated</Copy>
  </Card>)}</View>;
  return <View style={{ gap: 8 }}>
    <Action label="All template versions" colors={colors} onPress={() => { setTemplateKey(null); setSlotId(null); }} />
    <Copy colors={colors}>{template.name} · {template.id} v{template.version}</Copy>
    {slot ? <>
      <Action label="All template roles" colors={colors} onPress={() => setSlotId(null)} />
      <Card title={`${slot.id} · ${slot.role.role} v${slot.role.version}`} colors={colors}>
        <Copy colors={colors}>Ordered template defaults, separate from configured fleet alternatives and observed sessions.</Copy>
        {slot.defaultRungs.map((rung, index) => <Copy key={index} colors={colors}>Default rung {index + 1}: {rung.provider}/{rung.model} · effort {rung.effort ?? "Not supplied"}</Copy>)}
        <Copy colors={colors}>Skills: {slot.skills.map((skill) => `${skill.skill} v${skill.version}`).join(", ") || "None declared"}</Copy>
        <Copy colors={colors}>Context: {slot.context ? `${slot.context.template} v${slot.context.version}` : "Not declared"} · autonomy {slot.autonomy ?? "Not declared"}</Copy>
        <Copy colors={colors}>May evaluate: {slot.mayEvaluate.join(", ") || "None declared"} · may waive: {slot.mayWaive.join(", ") || "None declared"}. Declared policy, not execution authority.</Copy>
        {slot.waiverPolicy ? <Copy colors={colors}>Waiver definition: roles {slot.waiverPolicy.authorized_roles.join(", ")} · evidence {slot.waiverPolicy.required_evidence.join(", ")}</Copy> : null}
        {template.handoffs.filter((handoff) => handoff.from_slot === slot.id || handoff.to_slot === slot.id).map((handoff, index) => <Copy key={index} colors={colors}>Declared handoff {handoff.from_slot} → {handoff.to_slot} · after {handoff.after_phase ?? "Not declared"} · artifacts {handoff.required_artifacts.join(", ") || "None declared"}</Copy>)}
        <Copy colors={colors}>Source provenance: {slot.provenance.sourceId} · {slot.provenance.pointer}. Eligibility and identity binding remain unattested.</Copy>
      </Card>
    </> : <><Paging page={visibleSlotPage} count={template.slots.length} onPage={setSlotPage} colors={colors} label="template roles" />{template.slots.slice(visibleSlotPage * PAGE_SIZE, (visibleSlotPage + 1) * PAGE_SIZE).map((item) => <Action key={item.id} label={`Template role: ${item.id} · ${item.role.role} v${item.role.version}`} colors={colors} onPress={() => setSlotId(item.id)} />)}</>}
  </View>;
}
