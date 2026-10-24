import { useEffect, useRef, useState, type RefObject } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { ScrollView, Icon } from "@getpaseo/plugin/client/react-native";
import { Action, COARSE_POINTER, Disclosure, type Colors } from "./page-controls";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import { useControlHelp } from "./control-help";
import { RESUME_PROMPT, TAKEOVER_LIMIT, unresolvedArchive, type ProjectionConflict, type SessionProjection } from "../shared/session-lifecycle";
import type { useSessionLifecycle } from "./use-session-lifecycle";
import { SessionDetailSection } from "./session-detail-section";
import type { DetailValue } from "./session-metadata-content";

export function ArchiveSessionButton({ session, onPress, colors, disabled = false, pending = false, compact = false, buttonRef }: { session: { id: string; title: string }; onPress: () => void; colors: Colors; disabled?: boolean; pending?: boolean; compact?: boolean; buttonRef?: (node: View | null) => void }) {
  const help = useControlHelp("Archive session", buttonRef);
  return <>
    <Pressable {...help.bindings} accessibilityRole="button" accessibilityLabel={pending ? `Archiving ${session.title}…` : `Archive session: ${session.title} (${session.id})`} accessibilityHint="Stop and archive this exact session. History and files remain. Sessions with managed children are protected."
      disabled={disabled} accessibilityState={{ disabled, busy: pending }} onPress={() => { help.dismiss(); onPress(); }}
      style={{ width: compact || COARSE_POINTER ? 44 : 30, height: compact || COARSE_POINTER ? 44 : 30, alignItems: "center", justifyContent: "center", opacity: disabled ? 0.4 : 1 }}>
      <Icon name="X" size={14} color={colors.foregroundMuted} />
    </Pressable>
  </>;
}

export function SessionLifecycleDialog({ lifecycle, scopeIds, root, anchor, rootSize, colors, compact, onClose }: {
  lifecycle: ReturnType<typeof useSessionLifecycle>; scopeIds: string[]; root: RefObject<View | null>; anchor: RefObject<View | null>; rootSize: MenuSize; colors: Colors; compact: boolean; onClose: () => void;
}) {
  const [scope, setScope] = useState<"filtered" | "host">("filtered");
  const closeButton = useRef<View | null>(null);
  const { review, running, busy, target, capabilities, selected } = lifecycle;
  const archive = (review?.action ?? lifecycle.action) === "archive";
  const eligibleSelected = review?.rows.filter((row) => row.eligible && row.outcome === "pending" && selected.includes(row.id)).length ?? 0;
  useEffect(() => { const timer = setTimeout(() => (closeButton.current as View & { focus?: () => void } | null)?.focus?.(), 0); return () => clearTimeout(timer); }, [review?.id]);
  const providers = capabilities?.providers ?? [];
  const models = providers.find((provider) => provider.id === target.provider)?.models ?? [];
  const efforts = models.find((model) => model.id === target.model)?.efforts ?? [];
  const text = (value: string, muted = false) => <Text style={{ color: muted ? colors.foregroundMuted : colors.foreground, fontSize: 12, lineHeight: 18 }}>{value}</Text>;
  const close = () => { lifecycle.stop(); onClose(); };
  // This covers the screen; a row can disappear as soon as archive succeeds.
  // Measure the stable screen root, never the disappearing row's touch target.
  return <AnchoredMenu anchor={root} root={root} rootSize={rootSize} placement={{ align: compact ? "stretch" : "end", width: 660, cover: true }} onClose={close} colors={colors} label={archive ? "Archive session outcome" : "Resume sessions"} dialog testID="session-lifecycle-dialog"
    header={<View style={{ gap: 6 }}><View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}><Text accessibilityRole="header" style={{ color: colors.foreground, fontWeight: "600", fontSize: 16 }}>{archive ? "Archive session" : "Resume sessions"}</Text><Action ref={closeButton} colors={colors} label="Close" onPress={close} /></View>
      {review ? <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{!archive ? <Action colors={colors} label={running ? "Executing one session at a time…" : `Resume ${eligibleSelected} eligible · ${selected.length} selected`} disabled={running || eligibleSelected === 0} onPress={() => { void lifecycle.run(); }} /> : null}
        {running && !archive ? <Action colors={colors} label="Cancel before next session" onPress={lifecycle.stop} /> : null}
        {archive && !running && review.rows.some((row) => row.outcome === "blocked" && !row.reconcileReviewId || row.outcome === "pending") ? <Action colors={colors} label="Retry Archive" disabled={busy} onPress={() => { void lifecycle.prepare("archive", "filtered", review.rows.map((row) => row.id)); }} /> : null}</View> : null}
    </View>}>
    <ScrollView style={{ flexShrink: 1 }} contentContainerStyle={{ gap: 10, padding: 12 }}>
      {!archive ? text(TAKEOVER_LIMIT, true) : null}
      {!review && busy ? text("Reading the exact session scope and child impact…") : !review ? <>
        {text("Choose scope, then review exact sessions and reasons. Nothing runs during review.")}
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}><Action colors={colors} label={`Filtered scope (${scopeIds.length})`} selected={scope === "filtered"} onPress={() => setScope("filtered")} /><Action colors={colors} label="Whole selected host" selected={scope === "host"} onPress={() => setScope("host")} /></View>
        {text("Resume keeps each session's current provider, model, effort and permissions. A different target is listed as requiring takeover. Codex account routing belongs to the host's ZeroSub Accounts settings.", true)}
        {text(`Target: ${target.provider} / ${target.model} / ${target.effort}`)}
        <Disclosure colors={colors} label="Target configuration">
        <Action colors={colors} label="Discover provider options" onPress={() => { void lifecycle.loadCapabilities(); }} />
        <ScrollView style={{ maxHeight: 180 }}>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{providers.map((provider) => <Action key={provider.id} colors={colors} label={`${provider.id} · ${provider.status}`} selected={target.provider === provider.id} disabled={provider.status !== "ready"} onPress={() => lifecycle.setTarget({ ...target, provider: provider.id })} />)}</View>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{models.map((model) => <Action key={model.id} colors={colors} label={model.id} selected={target.model === model.id} onPress={() => lifecycle.setTarget({ ...target, model: model.id })} />)}</View>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{efforts.map((effort) => <Action key={effort} colors={colors} label={effort} selected={target.effort === effort} onPress={() => lifecycle.setTarget({ ...target, effort })} />)}</View>
        </ScrollView></Disclosure>
        <Action colors={colors} label={busy ? "Reading native sessions…" : "Review sessions"} disabled={busy} onPress={() => { void lifecycle.prepare("resume", scope, scope === "filtered" ? scopeIds : []); }} />
        <Action colors={colors} label="Read last action receipt" disabled={busy} onPress={() => { void lifecycle.restore(); }} />
      </> : <>
        {!archive ? text(`${review.scope === "host" ? "Whole selected host" : "Filtered scope"} · ${review.rows.length} session(s) · ${review.rows.filter((row) => row.eligible).length} eligible · Review ${review.id}`) : text(running || busy ? "Stopping and archiving the selected session…" : "Exact session outcome")}
        {text(review.action === "resume" ? "Selected sessions receive this continuation instruction:" : "History and files are preserved. Managed child sessions are protected from cascade.", true)}
        {review.action === "resume" ? text(RESUME_PROMPT, true) : null}
        {!archive ? <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
          <Action colors={colors} label="Select eligible" disabled={running} onPress={() => lifecycle.setSelected(review.rows.filter((row) => row.eligible && row.outcome === "pending").map((row) => row.id))} />
          <Action colors={colors} label="Clear selection" disabled={running} onPress={() => lifecycle.setSelected([])} />
          {review.action === "resume" ? <Action colors={colors} label="New review" disabled={running} onPress={lifecycle.clear} /> : null}
        </View> : null}
        {review.rows.map((row) => <View key={row.id} style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 7, padding: 8, gap: 4 }}>
          {archive ? text(row.title) : <Pressable accessibilityRole="checkbox" accessibilityLabel={`Select ${row.title} (${row.id}). ${row.reason}`} accessibilityState={{ checked: selected.includes(row.id), disabled: running || row.outcome !== "pending" }} aria-checked={selected.includes(row.id)} aria-disabled={running || row.outcome !== "pending"} disabled={running || row.outcome !== "pending"}
            {...(Platform.OS === "web" ? { onKeyDown: (event: { key: string; repeat?: boolean; preventDefault(): void; stopPropagation(): void }) => {
              // React Native Web's Pressable only handles Space for button-like roles.
              if (event.key !== " " && event.key !== "Spacebar") return;
              event.preventDefault(); event.stopPropagation();
              if (!event.repeat && !running && row.outcome === "pending") lifecycle.setSelected(selected.includes(row.id) ? selected.filter((id) => id !== row.id) : [...selected, row.id]);
            } } : {})}
            onPress={() => lifecycle.setSelected(selected.includes(row.id) ? selected.filter((id) => id !== row.id) : [...selected, row.id])} style={{ minHeight: 44, justifyContent: "center" }}>
            {text(`${selected.includes(row.id) ? "☑" : "☐"} ${row.title}`)}
          </Pressable>}
          {/* Older saved receipts wrote "missing" for a failed read too: only the row's own confirmed read may say missing. */}
          {text(`${row.id} · ${row.nativeStatus === "missing" && row.availability?.state !== "missing" ? "unknown" : row.nativeStatus}`, true)}
          {!archive || row.outcome === "pending" || row.outcome === "blocked" ? text(`${row.category === "unknown" ? "Unknown" : row.eligible ? "Eligible" : "Unavailable for this action"}: ${row.reason}`) : null}
          <Disclosure colors={colors} label="Session details">
            {text(`${row.provider}/${row.model ?? "unknown"}/${row.effort ?? "unknown"}`, true)}
            {text(`Workspace: ${row.workspaceId ?? "No workspace"} · ${row.cwd || "No checkout evidence"} · permissions ${row.mode ?? "unknown"}`, true)}
          </Disclosure>
          {row.children.map((child) => text(`Protected child: ${child.title} · ${child.id} · ${child.status} · ${child.workspaceId ?? "no workspace"}`, true))}
          {row.outcome !== "pending" ? <Text role="status" accessibilityLiveRegion="polite" style={{ color: colors.foreground, fontSize: 12 }}>{`${row.outcome}: ${row.message ?? "Read receipt before retry."}`}</Text> : null}
          {archive && (unresolvedArchive(row) || row.reconcileReviewId || lifecycle.error) ? <Action colors={colors} label="Refresh status" disabled={busy || running} onPress={() => { void lifecycle.reconcile(row.id); }} /> : null}
        </View>)}
        {!archive ? text("Unselected and blocked sessions remain unchanged. Preflight blocks can be reviewed again; an uncertain dispatch is held for native reconciliation. Close/cancel does not undo a request already sent.", true) : null}
      </>}
      {lifecycle.error ? <Text accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12 }}>{lifecycle.error}</Text> : null}
    </ScrollView>
  </AnchoredMenu>;
}

const RETIREMENT_LABELS = { retain: "Retain", ready: "Ready", unknown: "Unknown" } as const;
export function retirementDispositionLabel(disposition: SessionProjection["retirement"]["disposition"]): string {
  return RETIREMENT_LABELS[disposition];
}

/** One read-only row: disposition, exact reason, native status/membership and any archived conflict. */
export function SessionRetirementSummary({ projection, colors }: { readonly projection: SessionProjection; readonly colors: Colors }) {
  const label = retirementDispositionLabel(projection.retirement.disposition);
  return <View testID={`session-retirement-summary-${projection.sessionId}`} style={{ gap: 3 }}>
    <Text accessibilityRole="header" style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{`Retirement: ${label}`}</Text>
    <Text selectable style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>{projection.retirement.reason}</Text>
    <Text selectable style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>{`Native ${projection.native.status} · ${projection.native.archiveMembership === "archived" ? "Archived" : "Not archived"}`}</Text>
    {projection.retirement.attention ? <Text role="status" accessibilityLiveRegion="polite" style={{ color: colors.statusWarning, fontSize: 12, lineHeight: 17 }}>Attention is required; no automatic effect runs.</Text> : null}
    <SessionRetirementConflict conflicts={projection.conflicts} colors={colors} />
  </View>;
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
  const values: DetailValue[] = [
    { label: "Retirement", value: label },
    { label: "Reason", value: projection.retirement.reason },
    { label: "Native status", value: projection.native.status },
    { label: "Archive membership", value: projection.native.archiveMembership === "archived" ? "Archived" : "Not archived" },
    { label: "Scope", value: scope },
    { label: "Role responsibility", value: responsibility },
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
