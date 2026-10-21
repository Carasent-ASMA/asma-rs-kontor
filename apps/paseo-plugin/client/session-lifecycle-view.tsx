import { useEffect, useRef, useState, type RefObject } from "react";
import { Pressable, Text, View } from "react-native";
import { ScrollView, Icon } from "@getpaseo/plugin/client/react-native";
import { Action, COARSE_POINTER, type Colors } from "./page-controls";
import { AnchoredMenu, type MenuSize } from "./anchored-menu";
import { RESUME_PROMPT, TAKEOVER_LIMIT } from "../shared/session-lifecycle";
import type { useSessionLifecycle } from "./use-session-lifecycle";

export function ArchiveSessionButton({ onPress, colors, disabled = false, compact = false, buttonRef }: { onPress: () => void; colors: Colors; disabled?: boolean; compact?: boolean; buttonRef?: (node: View | null) => void }) {
  const [help, setHelp] = useState(false);
  return <View style={{ position: "relative" }}>
    <Pressable ref={buttonRef} accessibilityRole="button" accessibilityLabel="Archive session" accessibilityHint="Review this exact session before native archive. History and files remain."
      disabled={disabled} accessibilityState={{ disabled }} onPress={() => { setHelp(false); onPress(); }} onHoverIn={() => setHelp(true)} onHoverOut={() => setHelp(false)} onFocus={() => setHelp(true)} onBlur={() => setHelp(false)}
      style={{ width: compact || COARSE_POINTER ? 44 : 30, height: compact || COARSE_POINTER ? 44 : 30, alignItems: "center", justifyContent: "center", opacity: disabled ? 0.4 : 1 }}>
      <Icon name="X" size={14} color={colors.foregroundMuted} />
    </Pressable>
    {help ? <View pointerEvents="none" style={{ position: "absolute", right: 0, bottom: "100%", zIndex: 30, backgroundColor: colors.surface2, padding: 6, borderRadius: 5 }}><Text style={{ color: colors.foreground, fontSize: 12 }}>Archive session</Text></View> : null}
  </View>;
}

export function SessionLifecycleDialog({ lifecycle, scopeIds, root, anchor, rootSize, colors, compact, onClose }: {
  lifecycle: ReturnType<typeof useSessionLifecycle>; scopeIds: string[]; root: RefObject<View | null>; anchor: RefObject<View | null>; rootSize: MenuSize; colors: Colors; compact: boolean; onClose: () => void;
}) {
  const [scope, setScope] = useState<"filtered" | "host">("filtered");
  const closeButton = useRef<View | null>(null);
  const { review, running, busy, target, capabilities, selected } = lifecycle;
  useEffect(() => { const timer = setTimeout(() => (closeButton.current as View & { focus?: () => void } | null)?.focus?.(), 0); return () => clearTimeout(timer); }, [review?.id]);
  const providers = capabilities?.providers ?? [];
  const models = providers.find((provider) => provider.id === target.provider)?.models ?? [];
  const efforts = models.find((model) => model.id === target.model)?.efforts ?? [];
  const text = (value: string, muted = false) => <Text style={{ color: muted ? colors.foregroundMuted : colors.foreground, fontSize: 12, lineHeight: 18 }}>{value}</Text>;
  const close = () => { lifecycle.stop(); onClose(); };
  return <AnchoredMenu anchor={anchor} root={root} rootSize={rootSize} placement={{ align: compact ? "stretch" : "end", width: 660, cover: true }} onClose={close} colors={colors} label={review?.action === "archive" ? "Archive session review" : "Recover sessions"} dialog testID="session-lifecycle-dialog"
    header={<View style={{ flexDirection: "row", alignItems: "center", justifyContent: "space-between", gap: 8 }}><Text accessibilityRole="header" style={{ color: colors.foreground, fontWeight: "600", fontSize: 16 }}>{review?.action === "archive" ? "Archive session" : "Recover sessions"}</Text><Action ref={closeButton} colors={colors} label="Close" onPress={close} /></View>}>
    <ScrollView style={{ flexShrink: 1 }} contentContainerStyle={{ gap: 10, padding: 12 }}>
      {text(TAKEOVER_LIMIT, true)}
      {!review ? <>
        {text("Choose scope, then review exact sessions and reasons. Nothing runs during review.")}
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}><Action colors={colors} label={`Filtered scope (${scopeIds.length})`} selected={scope === "filtered"} onPress={() => setScope("filtered")} /><Action colors={colors} label="Whole selected host" selected={scope === "host"} onPress={() => setScope("host")} /></View>
        {text("Resume keeps each session's current provider, model, effort and permissions. A different target is listed as requiring takeover. Codex account routing belongs to the host's ZeroSub Accounts settings.", true)}
        <Action colors={colors} label="Discover provider options" onPress={() => { void lifecycle.loadCapabilities(); }} />
        {text(`Target: ${target.provider} / ${target.model} / ${target.effort}`)}
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{providers.map((provider) => <Action key={provider.id} colors={colors} label={`${provider.id} · ${provider.status}`} selected={target.provider === provider.id} disabled={provider.status !== "ready"} onPress={() => lifecycle.setTarget({ ...target, provider: provider.id })} />)}</View>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{models.map((model) => <Action key={model.id} colors={colors} label={model.id} selected={target.model === model.id} onPress={() => lifecycle.setTarget({ ...target, model: model.id })} />)}</View>
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>{efforts.map((effort) => <Action key={effort} colors={colors} label={effort} selected={target.effort === effort} onPress={() => lifecycle.setTarget({ ...target, effort })} />)}</View>
        <Action colors={colors} label={busy ? "Reading native sessions…" : "Review sessions"} disabled={busy} onPress={() => { void lifecycle.prepare("resume", scope, scope === "filtered" ? scopeIds : []); }} />
        <Action colors={colors} label="Read last action receipt" disabled={busy} onPress={() => { void lifecycle.restore(); }} />
      </> : <>
        {text(`${review.scope === "host" ? "Whole selected host" : "Filtered scope"} · ${review.rows.length} session(s) · ${review.rows.filter((row) => row.eligible).length} eligible · Review ${review.id}`)}
        {text(review.action === "resume" ? "Selected sessions receive this continuation instruction:" : "This exact session will be archived. Running sessions and sessions with children are blocked. Durable history/files are preserved; this screen has no undo.", true)}
        {review.action === "resume" ? text(RESUME_PROMPT, true) : null}
        <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 4 }}>
          <Action colors={colors} label="Select eligible" disabled={running} onPress={() => lifecycle.setSelected(review.rows.filter((row) => row.eligible && row.outcome === "pending").map((row) => row.id))} />
          <Action colors={colors} label="Clear selection" disabled={running} onPress={() => lifecycle.setSelected([])} />
          {review.action === "resume" ? <Action colors={colors} label="New review" disabled={running} onPress={lifecycle.clear} /> : null}
        </View>
        {review.rows.map((row) => <View key={row.id} style={{ borderWidth: 1, borderColor: colors.border, borderRadius: 7, padding: 8, gap: 4 }}>
          <Pressable accessibilityRole="checkbox" accessibilityLabel={`Select ${row.title}. ${row.reason}`} accessibilityState={{ checked: selected.includes(row.id), disabled: running || row.outcome !== "pending" }} disabled={running || row.outcome !== "pending"}
            onPress={() => lifecycle.setSelected(selected.includes(row.id) ? selected.filter((id) => id !== row.id) : [...selected, row.id])} style={{ minHeight: 44, justifyContent: "center" }}>
            {text(`${selected.includes(row.id) ? "☑" : "☐"} ${row.title}`)}
          </Pressable>
          {text(`${row.id} · ${row.nativeStatus} · ${row.provider}/${row.model ?? "unknown"}/${row.effort ?? "unknown"}`, true)}
          {text(`${row.category === "unknown" ? "Unknown" : row.eligible ? "Eligible" : "Unavailable for this action"}: ${row.reason}`)}
          {text(`Workspace: ${row.workspaceId ?? "No workspace"} · ${row.cwd || "No checkout evidence"} · permissions ${row.mode ?? "unknown"}`, true)}
          {row.children.map((child) => text(`Protected child: ${child.title} · ${child.id} · ${child.status} · ${child.workspaceId ?? "no workspace"}`, true))}
          {row.outcome !== "pending" ? text(`${row.outcome}: ${row.message ?? "Read receipt before retry."}`) : null}
        </View>)}
        <Action colors={colors} label={running ? "Executing one session at a time…" : review.action === "archive" ? "Archive selected session" : `Resume ${selected.length} selected session(s)`} disabled={running || !selected.some((id) => review.rows.some((row) => row.id === id && row.eligible && row.outcome === "pending"))} onPress={() => { void lifecycle.run(); }} />
        {running ? <Action colors={colors} label="Cancel before next session" onPress={lifecycle.stop} /> : null}
        {text("Unselected and blocked sessions remain unchanged. Preflight blocks can be reviewed again; an uncertain dispatch is held for native reconciliation. Close/cancel does not undo a request already sent.", true)}
      </>}
      {lifecycle.error ? <Text accessibilityRole="alert" style={{ color: colors.statusWarning, fontSize: 12 }}>{lifecycle.error}</Text> : null}
    </ScrollView>
  </AnchoredMenu>;
}
