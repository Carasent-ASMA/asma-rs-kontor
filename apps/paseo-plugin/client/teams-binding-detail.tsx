import { View } from "react-native";
import type { TeamsFleetData } from "../shared/teams-source-snapshot";
import { Copy, type Colors } from "./page-controls";
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
export function BindingDetail({ binding, data, colors }: { binding: TeamsFleetData["bindings"][number]; data: TeamsFleetData; colors: Colors }) {
  const chain = binding.chainResolved ? data.chains.find((candidate) => candidate.name === binding.chainName) : undefined;
  const independence = Object.entries(data.rules.independent_of).filter(([from, of]) => from === binding.reference || of === binding.reference);
  return <>
        <Copy colors={colors}>groupKind: {binding.groupKind} · groupId: {binding.groupId} · slotId: {binding.slotId ?? "Not supplied"} · declared chainName: {binding.chainName}</Copy>
        <Copy colors={colors}>Binding source provenance: {binding.provenance.sourceId} · {binding.provenance.pointer}</Copy>
        <Copy colors={colors}>Declared rules for this binding: calibration_required {data.rules.calibration_required.includes(binding.reference) ? "declared" : "not declared"}; vision_required {data.rules.vision_required.includes(binding.reference) ? "declared" : "not declared"}. Policy eligibility is not evaluated.</Copy>
        <Copy colors={colors} muted>Rule source: {binding.provenance.sourceId} · /rules/calibration_required · /rules/vision_required · /rules/independent_of</Copy>
        {independence.length ? independence.map(([from, of]) => <Copy key={from} colors={colors}>Declared independent_of: {from} → {of}</Copy>) : <Copy colors={colors}>independent_of: no relationship declared for this binding.</Copy>}
        {chain ? <ChainDetails chain={chain} colors={colors} /> : <Copy colors={colors}>Declared chain {binding.chainName}: definition unavailable. Mapping retained; rungs and model metadata Unknown.</Copy>}
  </>;
}
