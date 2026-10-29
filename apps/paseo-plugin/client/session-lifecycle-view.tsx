import { Text, View } from "react-native";
import type { Colors } from "./page-controls";
import type { ProjectionConflict, SessionProjection } from "../shared/session-lifecycle";
import { SessionDetailSection } from "./session-detail-section";
import type { DetailValue } from "./session-metadata-content";

const RETIREMENT_LABELS = { retain: "Retain", ready: "Ready", unknown: "Unknown" } as const;
export function retirementDispositionLabel(disposition: SessionProjection["retirement"]["disposition"]): string {
  return RETIREMENT_LABELS[disposition];
}

/** Read-only details beside the existing row controls; it reuses the shared named detail section. */
export function SessionRetirementDetails({ projection, colors }: { readonly projection: SessionProjection; readonly colors: Colors }) {
  const label = retirementDispositionLabel(projection.retirement.disposition);
  const scope = projection.scope.claims.length
    ? projection.scope.claims.map((claim) => `${claim.kind}: ${claim.value} (${claim.binding}; ${claim.source.kind})`).join("; ")
    : "No scope association observed";
  const responsibility = projection.responsibility.status === "verified"
    ? `${projection.responsibility.role ?? "unknown role"} · ${projection.responsibility.owner ?? "owner unknown"} · ${projection.responsibility.duty ?? "duty unknown"}`
    : `${projection.responsibility.claimedRole ? `Claimed role: ${projection.responsibility.claimedRole}. ` : ""}Responsibility unknown; ${projection.responsibility.missing.join(" ") || "no owner source verifies it"}`;
  const intent = projection.intent.state === "none" ? "None recorded" : `${projection.intent.state}${projection.intent.reviewId ? ` · review ${projection.intent.reviewId}` : ""}${projection.intent.observedAt ? ` · observed ${projection.intent.observedAt}` : ""}`;
  const promotion = projection.promotion ? `${projection.promotion.epic} · logical epic LSA · ${projection.promotion.source.detail}` : "None recorded";
  const values: DetailValue[] = [
    { label: "Retirement", value: label },
    { label: "Reason", value: projection.retirement.reason },
    { label: "Native status", value: projection.native.status },
    { label: "Archive membership", value: projection.native.archiveMembership === "archived" ? "Archived" : "Not archived" },
    { label: "Scope", value: scope },
    { label: "Role responsibility", value: responsibility },
    { label: "Logical promotion", value: promotion },
    { label: "Archive intent", value: intent },
  ];
  return <SessionDetailSection title="Retirement evidence" values={values} colors={colors}>
    <View style={{ paddingHorizontal: 6, paddingVertical: 3, gap: 3 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>Missing evidence</Text>
      <Text selectable style={{ color: colors.foreground, fontSize: 13, lineHeight: 18 }}>{projection.retirement.missing.length ? projection.retirement.missing.join(" ") : "No missing evidence recorded."}</Text>
    </View>
    <View style={{ paddingHorizontal: 6, paddingVertical: 3, gap: 3 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>Evidence sources</Text>
      {projection.sources.length ? projection.sources.map((source) => <Text key={source.id} selectable style={{ color: colors.foreground, fontSize: 12, lineHeight: 17 }}>
        {`${source.kind} · ${source.detail} · ${source.observedAt ?? "time unknown"} · revision ${source.revision ?? "unknown"} · ${source.fresh}`}
      </Text>) : <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>No source evidence was read.</Text>}
    </View>
    <SessionRetirementConflict conflicts={projection.conflicts} colors={colors} />
  </SessionDetailSection>;
}

/** A conflict is shown separately from ordinary membership and status; nothing here archives or revives. */
export function SessionRetirementConflict({ conflicts, colors }: { readonly conflicts: readonly ProjectionConflict[]; readonly colors: Colors }) {
  if (!conflicts.length) return null;
  const archived = conflicts.some((conflict) => conflict.kind.startsWith("archived-with-"));
  return <View testID="session-retirement-conflicts" style={{ gap: 2 }}>
    <Text accessibilityRole="header" style={{ color: colors.statusWarning, fontSize: 12, fontWeight: "600" }}>{archived ? "Archived conflict" : "Evidence conflict"}</Text>
    {conflicts.map((conflict) => <Text key={`${conflict.kind}:${conflict.detail}`} selectable style={{ color: colors.foreground, fontSize: 12, lineHeight: 17 }}>{`${conflict.kind}: ${conflict.detail}`}</Text>)}
  </View>;
}
