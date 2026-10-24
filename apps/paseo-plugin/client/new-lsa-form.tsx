import { useEffect, useRef, useState } from "react";
import { Platform, Pressable, ScrollView, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import { useRpc } from "@getpaseo/plugin/client";
import { adhocTitle, checkCreate, executeCreate, normalizeTopic, topicProblem, TOPIC_MAX, type CreateOptions, type CreateOutcome } from "../shared/session-create";
import { Action, COARSE_POINTER, Copy, Disclosure, type Colors } from "./page-controls";
import type { CreateOptionsState } from "./start-switch-sheet";
import type { OpenSessionResult } from "./session-actions";

export type CreateDraft = { readonly topic: string; readonly workspaceId?: string; readonly provider?: string; readonly model?: string; readonly effort?: string | null; readonly modeId?: string | null };

/**
 * Defaults fill only unspecified choices. An explicit or saved choice that disappears stays invalid until
 * the operator chooses again; a catalog refresh never silently substitutes another model or permission mode.
 */
export function resolveDraft(options: CreateOptions, draft: CreateDraft) {
  const desk = (draft.workspaceId ? options.desks.find((entry) => entry.workspaceId === draft.workspaceId) : options.desks[0]) ?? null;
  const profile = options.profile;
  const providerId = draft.provider ?? profile?.provider ?? options.providers.find((entry) => entry.available)?.id;
  const provider = options.providers.find((entry) => entry.id === providerId) ?? null;
  const preset = profile && profile.provider === provider?.id ? profile : null;
  const modelId = draft.model ?? preset?.model ?? provider?.defaultModel ?? provider?.models[0]?.id;
  const model = provider?.available ? provider.models.find((entry) => entry.id === modelId) ?? null : null;
  const effortId = draft.effort !== undefined ? draft.effort : (preset?.model === model?.id ? preset?.effort : null) ?? model?.defaultEffort ?? model?.efforts[0]?.id;
  const effort = model?.efforts.find((entry) => entry.id === effortId) ?? null;
  const modeId = draft.modeId !== undefined ? draft.modeId : preset?.modeId ?? provider?.defaultMode ?? provider?.modes[0]?.id;
  const mode = provider?.available ? provider.modes.find((entry) => entry.id === modeId) ?? null : null;
  const staleEffort = !effort && (!!effortId || !!model?.efforts.length);
  const staleMode = !mode && (!!modeId || !!provider?.modes.length);
  const topic = normalizeTopic(draft.topic);
  let problem: string | null = null;
  if (!options.enabled) problem = options.disabledReason ?? "New LSA is off on this host.";
  else if (options.pending) problem = `An earlier creation, “${options.pending.title}”, is not settled. Check it first.`;
  else if (!desk) problem = options.desks.length ? "The selected desk is no longer available. Choose a listed desk or Refresh desks." : "No desk on this host has an ADAM workspace.";
  else if (options.catalogWorkspaceId !== desk.workspaceId) problem = "Refresh this desk's provider catalog before creating a session.";
  else if (!provider?.available) problem = provider?.note ?? (providerId ? `Provider ${providerId} is no longer active. Choose an active provider.` : "No active provider is available for this desk.");
  else if (!model) problem = modelId ? `Model ${modelId} is no longer active for ${provider.label}. Choose an active model.` : "No selectable model.";
  else if (staleEffort) problem = `Effort ${effortId ?? "none"} is no longer offered for ${model.label}. Choose an available effort.`;
  else if (staleMode) problem = `Mode ${modeId ?? "none"} is no longer offered by ${provider.label}. Choose an available mode.`;
  else problem = topicProblem(topic);
  return { desk, provider, model, effort, mode, topic, title: adhocTitle(topic), problem, fromProfile: preset !== null, staleEffort, staleMode };
}

/** A version-4 UUID for the request's idempotency; not a secret. */
export function newRequestId(): string {
  const bytes = new Uint8Array(16);
  const crypto = (globalThis as { crypto?: { getRandomValues?: (array: Uint8Array) => Uint8Array } }).crypto;
  if (crypto?.getRandomValues) crypto.getRandomValues(bytes);
  else bytes.forEach((_, index) => { bytes[index] = Math.floor(Math.random() * 256); });
  bytes[6] = (bytes[6]! & 0x0f) | 0x40; bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(16, 20)}-${hex.slice(20)}`;
}

const HEADINGS: Record<CreateOutcome["state"], string> = {
  verified: "Created and verified", differences: "Created with differences", unknown: "Outcome unknown", refused: "Not created",
};

export function personaLine(persona: CreateOutcome["persona"], providerLabel: string): string {
  return persona === "supported" ? "Persona: sent as the system prompt; Claude and Codex apply it at creation." : `Persona delivery unverified: ${providerLabel} is not confirmed to apply a system prompt.`;
}

type Phase =
  | { readonly kind: "draft" }
  | { readonly kind: "busy"; readonly requestId: string; readonly label: string }
  | { readonly kind: "result"; readonly outcome: CreateOutcome }
  /** The plugin server's reply was lost: the request may or may not have been dispatched. */
  | { readonly kind: "lost"; readonly requestId: string; readonly title: string; readonly message: string };

export function NewLsaForm({ hostId, colors, compact, options, onRefresh, canOpen, isActive = () => true, onOpenCreated, onCancel }: {
  hostId: string; colors: Colors; compact: boolean; options: CreateOptionsState; onRefresh: (workspaceId?: string) => void; canOpen: boolean;
  onOpenCreated: (outcome: CreateOutcome) => OpenSessionResult; onCancel: () => void;
  /** A sheet close invalidates pending continuations immediately, before React's unmount cleanup. */
  isActive?: () => boolean;
}) {
  const execute = useRpc(executeCreate), check = useRpc(checkCreate);
  const [draft, setDraft] = useState<CreateDraft>(() => ({ topic: "", workspaceId: options.status === "ready" ? options.options.catalogWorkspaceId ?? undefined : undefined }));
  const [phase, setPhase] = useState<Phase>({ kind: "draft" });
  const [advanced, setAdvanced] = useState(!compact);
  const submitted = useRef(false);
  // Keep controls mounted during a scoped refresh so keyboard focus/selection survives. This snapshot is
  // presentation only: submit still requires current ready options and the server's fresh validation.
  const presentation = useRef<{ hostId: string; options: CreateOptions } | null>(null);
  if (options.status === "ready") presentation.current = { hostId, options: options.options };
  else if (options.status === "offline" || presentation.current?.hostId !== hostId) presentation.current = null;
  const live = useRef(true), currentHost = useRef(hostId), currentOptions = useRef(options), currentCanOpen = useRef(canOpen);
  currentHost.current = hostId; currentOptions.current = options; currentCanOpen.current = canOpen;
  useEffect(() => { live.current = true; return () => { live.current = false; }; }, []);
  const isCurrent = () => live.current && currentHost.current === hostId && isActive();
  const opening = useRef<string | null>(null);
  const [openState, setOpenState] = useState<{ requestId: string; status: "opening" | "opened" | "failed"; message?: string } | null>(null);
  const tall = compact || COARSE_POINTER;
  const muted = { color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 } as const;
  // The footer stays outside the scroll so Create is never below the fold of a short panel.
  const wrap = (children: React.ReactNode, footer?: React.ReactNode) => <>
    <ScrollView testID="kontor-new-lsa" keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }} contentContainerStyle={{ gap: 10, padding: 6 }}>{children}</ScrollView>
    {footer ? <View style={{ flexDirection: "row", justifyContent: "flex-end", gap: 8, paddingHorizontal: 6, paddingTop: 6, borderTopWidth: 1, borderColor: colors.border }}>{footer}</View> : null}
  </>;

  const openCreated = (outcome: CreateOutcome) => {
    if (!isCurrent() || !currentCanOpen.current || currentOptions.current.status === "offline" || opening.current === outcome.requestId) return;
    opening.current = outcome.requestId;
    setOpenState({ requestId: outcome.requestId, status: "opening" });
    let result: OpenSessionResult;
    try { result = onOpenCreated(outcome); }
    catch (error) { result = { ok: false, reason: "failed", message: error instanceof Error ? error.message : "Paseo could not open this session." }; }
    if (!result.ok) opening.current = null;
    if (isCurrent()) setOpenState(result.ok ? { requestId: outcome.requestId, status: "opened" } : { requestId: outcome.requestId, status: "failed", message: result.message });
  };
  const finish = (outcome: CreateOutcome, requestId: string, title: string) => {
    if (!isCurrent()) return;
    if (outcome.requestId !== requestId) {
      setPhase({ kind: "lost", requestId, title, message: "The host replied with a different request ID. Check this request before opening any session." });
      return;
    }
    setPhase({ kind: "result", outcome });
    if (outcome.state === "unknown" || outcome.state === "refused") onRefresh(draft.workspaceId);
  };
  const runCheck = (requestId: string, title: string) => {
    if (!isCurrent()) return;
    setPhase({ kind: "busy", requestId, label: "Checking the host…" });
    check({ requestId }).then((outcome) => finish(outcome, requestId, title),
      (error: unknown) => { if (isCurrent()) setPhase({ kind: "lost", requestId, title, message: `Check did not complete: ${error instanceof Error ? error.message : String(error)}` }); });
  };
  const outcomeView = (outcome: CreateOutcome, providerLabel: string) => {
    const created = outcome.state === "verified" || outcome.state === "differences";
    let openNote: string | null = null;
    if (created && !canOpen) openNote = "This Paseo app cannot open it from Kontor. Use Sessions to find this exact ID.";
    const opened = openState?.requestId === outcome.requestId ? openState : null;
    return wrap(<View testID={`kontor-new-lsa-${outcome.state}`} accessibilityLiveRegion="polite" aria-live="polite" style={{ gap: 8 }}>
      <Text accessibilityRole="header" style={{ color: outcome.state === "verified" ? colors.statusSuccess : outcome.state === "differences" || outcome.state === "unknown" ? colors.statusWarning : colors.foreground, fontSize: 14, fontWeight: "600" }}>{HEADINGS[outcome.state]}</Text>
      <Text selectable style={{ color: colors.foreground, fontSize: 13 }}>{outcome.title}</Text>
      {outcome.agentId ? <Text selectable style={muted}>Session ID: {outcome.agentId}</Text> : null}
      <Copy colors={colors} small>{outcome.message}</Copy>
      {outcome.differences.map((line) => <Text key={line} style={muted}>• {line}</Text>)}
      {created ? <Text style={muted}>{personaLine(outcome.persona, providerLabel)}</Text> : null}
      {openNote ? <Text style={muted}>{openNote}</Text> : null}
      {opened ? <Text testID="kontor-new-lsa-open-status" accessibilityLiveRegion="polite" style={muted}>{opened.status === "opening" ? "Opening session…" : opened.status === "opened" ? "Opened in Paseo." : `Created. Opening failed: ${opened.message} Use Open session to try again.`}</Text> : null}
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
        {created && outcome.agentId && canOpen && opened?.status !== "opened" ? <Action small label="Open session" colors={colors} disabled={opened?.status === "opening" || options.status === "offline"} onPress={() => openCreated(outcome)} /> : null}
        {outcome.state === "unknown" ? <Action small label="Check" accessibilityLabel="Check the host for this creation" colors={colors} onPress={() => runCheck(outcome.requestId, outcome.title)} /> : null}
        {outcome.state === "refused" ? <Action small label="Back to the form" colors={colors} onPress={() => { submitted.current = false; setPhase({ kind: "draft" }); }} /> : null}
        {outcome.state !== "refused" && outcome.state !== "unknown" ? <Action small label="Done" colors={colors} onPress={onCancel} /> : null}
      </View>
    </View>);
  };
  const ready = presentation.current?.options;
  const choice = ready ? resolveDraft(ready, draft) : null;
  const providerLabel = choice?.provider?.label ?? "This provider";
  if (phase.kind === "busy") return wrap(<View accessibilityLiveRegion="polite" aria-live="polite"><Copy colors={colors}>{phase.label} Nothing is reported until the host answers.</Copy></View>);
  if (phase.kind === "result") return outcomeView(phase.outcome, providerLabel);
  if (phase.kind === "lost") return wrap(<View testID="kontor-new-lsa-lost" accessibilityLiveRegion="polite" aria-live="polite" style={{ gap: 8 }}>
    <Text accessibilityRole="header" style={{ color: colors.statusWarning, fontSize: 14, fontWeight: "600" }}>Outcome unknown</Text>
    <Text selectable style={{ color: colors.foreground, fontSize: 13 }}>{phase.title}</Text>
    <Copy colors={colors} small>{phase.message} The request may or may not have reached the host. Do not retry; Check asks the host.</Copy>
    <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
      <Action small label="Check" accessibilityLabel="Check the host for this creation" colors={colors} onPress={() => runCheck(phase.requestId, phase.title)} />
    </View>
  </View>);
  if (!choice || !ready) {
    const text = options.status === "error" ? `Kontor could not read this host's options (${options.message}).` : options.status === "offline" ? "Host offline. Nothing can be created until it reconnects." : "Reading this host's desks and providers…";
    return wrap(<>
      <Copy colors={colors} muted>{text}</Copy>
      {options.status === "error" ? <Action small label="Try again" colors={colors} onPress={() => onRefresh(draft.workspaceId)} /> : null}
    </>);
  }
  const problem = options.status === "ready" ? choice.problem : options.status === "error"
    ? `Choices could not be refreshed (${options.message}). Refresh choices before creating a session.`
    : "Refreshing this desk's choices… Creation stays disabled until the host answers.";
  // An unsettled earlier request (this device or another) is shown instead of the form until a Check settles it.
  if (ready.pending) return outcomeView(ready.pending, providerLabel);

  const submit = () => {
    if (!isCurrent() || currentOptions.current.status !== "ready" || submitted.current || problem || !choice.desk || !choice.provider || !choice.model) return;
    submitted.current = true;
    const requestId = newRequestId(), title = choice.title;
    setPhase({ kind: "busy", requestId, label: `Creating “${title}”…` });
    execute({ requestId, topic: choice.topic, workspaceId: choice.desk.workspaceId, provider: choice.provider.id, model: choice.model.id, effort: choice.effort?.id ?? null, modeId: choice.mode?.id ?? null })
      .then((outcome) => finish(outcome, requestId, title),
        (error: unknown) => { if (isCurrent()) setPhase({ kind: "lost", requestId, title, message: `The Kontor server did not answer (${error instanceof Error ? error.message : String(error)}).` }); });
  };
  const set = (patch: Partial<CreateDraft>) => setDraft({ ...draft, ...patch });
  const summary = choice.desk ? [choice.provider?.label, choice.model?.label, choice.effort?.label, choice.mode ? `${choice.mode.label}${choice.mode.dangerous ? " ⚠" : ""}` : null].filter(Boolean).join(" · ") : "Choose a desk first";
  const providerEmpty = !choice.desk ? ready.desks.length ? "Choose an available desk to read its providers." : "Provider choices require an ADAM workspace."
    : ready.catalogWorkspaceId !== choice.desk.workspaceId ? "Refresh choices to read this desk's providers." : "No active provider is available for this desk.";
  return wrap(<>
    <View style={{ gap: 4 }}>
      <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600" }}>Topic</Text>
      <TextInput testID="kontor-new-lsa-topic" accessibilityLabel="Topic" placeholder="What this session is about" autoFocus={!compact} maxLength={TOPIC_MAX} value={draft.topic}
        onChangeText={(topic) => set({ topic })} onSubmitEditing={() => { if (!problem) submit(); }} returnKeyType="go"
        placeholderTextColor={colors.foregroundMuted} style={{ color: colors.foreground, fontSize: 13, minHeight: tall ? 44 : 34, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6 }} />
      <Text testID="kontor-new-lsa-title" style={muted}>Title: {choice.title}. The session promotes ADHOC to a confirmed epic key itself.</Text>
    </View>
    <Choices label="Desk" colors={colors} tall={tall} selected={choice.desk?.workspaceId ?? null} onSelect={(workspaceId) => { set({ workspaceId }); onRefresh(workspaceId); }}
      options={ready.desks.map((desk) => ({ id: desk.workspaceId, label: `${desk.project} / ADAM` }))} empty="No desk on this host has an ADAM workspace." />
    <Disclosure small label={`Provider, model and mode: ${summary || "none available"}`} accessibilityLabel={`Provider, model and mode. ${summary || "None available"}`} colors={colors} open={advanced} onToggle={setAdvanced}>
      <View style={{ gap: 10, paddingTop: 4 }}>
        <Text style={muted}>{choice.fromProfile ? `Defaults from the host's ${ready.profile!.name} profile.` : "Defaults: the provider's own."}</Text>
        <Choices label="Provider" colors={colors} tall={tall} selected={choice.provider?.id ?? null} onSelect={(provider) => set({ provider, model: undefined, effort: undefined, modeId: undefined })}
          options={ready.providers.filter((provider) => provider.available).map((provider) => ({ id: provider.id, label: provider.label }))} empty={providerEmpty} />
        {choice.provider?.available ? <Choices label="Model" colors={colors} tall={tall} selected={choice.model?.id ?? null} onSelect={(model) => set({ model, effort: undefined })}
          options={choice.provider.models.map((model) => ({ id: model.id, label: model.label }))} empty="No selectable model." /> : null}
        {choice.model?.efforts.length ? <Choices label="Effort" colors={colors} tall={tall} selected={choice.effort?.id ?? null} onSelect={(effort) => set({ effort })}
          options={choice.model.efforts} /> : choice.model ? <Text style={muted}>No effort setting for this model.</Text> : null}
        {choice.provider?.available && choice.provider.modes.length ? <Choices label="Mode" colors={colors} tall={tall} selected={choice.mode?.id ?? null} onSelect={(modeId) => set({ modeId })}
          options={choice.provider.modes.map((mode) => ({ id: mode.id, label: mode.dangerous ? `${mode.label} ⚠` : mode.label }))} /> : null}
        {choice.model && choice.staleEffort ? <Action small label="Use current effort default" colors={colors} onPress={() => set({ effort: choice.model!.efforts.find((effort) => effort.id === choice.model!.defaultEffort)?.id ?? choice.model!.efforts[0]?.id ?? null })} /> : null}
        {choice.provider?.available && choice.staleMode ? <Action small label="Use current mode default" colors={colors} onPress={() => set({ modeId: choice.provider!.modes.find((mode) => mode.id === choice.provider!.defaultMode)?.id ?? choice.provider!.modes[0]?.id ?? null })} /> : null}
      </View>
    </Disclosure>
    {ready.providers.some((provider) => provider.status === "loading") ? <Text style={muted}>Some providers are still loading.</Text> : null}
    {ready.providers.some((provider) => provider.status === "unknown") ? <Text style={muted}>Some provider availability is unknown.</Text> : null}
    {ready.providers.some((provider) => provider.status === "error") ? <Text style={muted}>Some providers could not be read.</Text> : null}
    {ready.providers.some((provider) => provider.status === "unavailable") ? <Text style={muted}>Some providers are unavailable.</Text> : null}
    <Action small label={choice.desk ? "Refresh choices" : "Refresh desks"} colors={colors} onPress={() => onRefresh(draft.workspaceId)} />
    {choice.mode?.dangerous ? <Text testID="kontor-new-lsa-danger" style={{ color: colors.statusWarning, fontSize: 12 }}>⚠ {choice.mode.label} is a dangerous permission mode on this host.</Text> : null}
    {choice.provider ? <Text style={muted}>{personaLine(choice.provider.persona, providerLabel)}</Text> : null}
    {ready.notes.map((note) => <Text key={note} style={muted}>{note}</Text>)}
    {problem ? <Text testID="kontor-new-lsa-problem" accessibilityLiveRegion="polite" style={{ color: colors.foregroundMuted, fontSize: 12 }}>{problem}</Text> : null}
  </>, <>
      <Action small label="Cancel" colors={colors} onPress={onCancel} />
      <Pressable testID="kontor-new-lsa-create" accessibilityRole="button" accessibilityLabel={`Create ${choice.title}`} accessibilityHint={problem ?? undefined} accessibilityState={{ disabled: !!problem }} aria-disabled={!!problem}
        disabled={!!problem} onPress={submit} style={{ minHeight: tall ? 44 : 30, paddingHorizontal: 12, borderRadius: 6, justifyContent: "center", backgroundColor: problem ? colors.surface2 : colors.accent, opacity: problem ? 0.6 : 1 }}>
        <Text style={{ color: problem ? colors.foregroundMuted : colors.accentForeground, fontSize: 12, fontWeight: "600" }}>Create LSA</Text>
      </Pressable>
  </>);
}

/** A labelled single choice as wrapping radio chips; an unavailable option stays visible with its reason. */
function Choices({ label, options, selected, onSelect, colors, tall, empty }: {
  label: string; options: readonly { id: string; label: string; disabled?: boolean; note?: string | null }[]; selected: string | null;
  onSelect: (id: string) => void; colors: Colors; tall: boolean; empty?: string;
}) {
  const enabled = options.filter((option) => !option.disabled);
  const tabStop = enabled.find((option) => option.id === selected)?.id ?? enabled[0]?.id;
  return <View accessibilityRole="radiogroup" accessibilityLabel={label} style={{ gap: 4 }}>
    <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600" }}>{label}</Text>
    {options.length ? <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 6 }}>
      {options.map((option) => {
        const checked = option.id === selected, disabled = !!option.disabled;
        return <Pressable key={option.id} accessibilityRole="radio" accessibilityLabel={option.note ? `${option.label}, ${option.note}` : option.label} accessibilityState={{ checked, disabled }} aria-checked={checked} aria-disabled={disabled}
          {...(Platform.OS === "web" ? { tabIndex: !disabled && option.id === tabStop ? 0 : -1, onKeyDown: (event: {
            key: string; repeat?: boolean; altKey?: boolean; ctrlKey?: boolean; metaKey?: boolean;
            currentTarget: { parentElement: { querySelectorAll(selector: string): ArrayLike<{ focus(): void }> } | null };
            preventDefault(): void; stopPropagation(): void;
          }) => {
            if (disabled || event.altKey || event.ctrlKey || event.metaKey) return;
            if (event.key === " " || event.key === "Spacebar") {
              event.preventDefault(); event.stopPropagation();
              if (!event.repeat) onSelect(option.id);
              return;
            }
            if (!["ArrowLeft", "ArrowRight", "ArrowUp", "ArrowDown", "Home", "End"].includes(event.key)) return;
            event.preventDefault(); event.stopPropagation();
            const index = enabled.findIndex((choice) => choice.id === option.id);
            const step = event.key === "ArrowLeft" || event.key === "ArrowUp" ? -1 : 1;
            const next = enabled[event.key === "Home" ? 0 : event.key === "End" ? enabled.length - 1 : (index + step + enabled.length) % enabled.length];
            if (!next) return;
            onSelect(next.id);
            event.currentTarget.parentElement?.querySelectorAll('[role="radio"]')[options.findIndex((choice) => choice.id === next.id)]?.focus();
          } } : {})}
          disabled={disabled} onPress={() => onSelect(option.id)} style={({ focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: tall ? 44 : 30, maxWidth: "100%", paddingHorizontal: 10, paddingVertical: 4, borderRadius: 6, borderWidth: 1, justifyContent: "center",
            borderColor: checked || focused ? colors.accent : colors.border, backgroundColor: checked ? colors.surface2 : undefined, opacity: disabled ? 0.5 : 1 })}>
          <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: checked ? "600" : "400" }}>{option.label}</Text>
        </Pressable>;
      })}
    </View> : <Text style={{ color: colors.foregroundMuted, fontSize: 12 }}>{empty}</Text>}
  </View>;
}
