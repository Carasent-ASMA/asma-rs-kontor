import { useEffect, useRef, useState } from "react";
import { Platform, Pressable, ScrollView, Text, View } from "react-native";
import { TextInput } from "@getpaseo/plugin/client/react-native";
import { getPaseoClient, useRpc } from "@getpaseo/plugin/client";
import { checkCreate, executeCreate, issueDesk, issueTopic, leadTitle, leadWorkspaceName, normalizeTopic, topicProblem, TOPIC_MAX,
  type CreateOptions, type CreateOutcome, type CreateRole, type CreateScope, type CreateStart, type JiraIssue } from "../shared/session-create";
import { Action, COARSE_POINTER, Copy, Disclosure, type Colors } from "./page-controls";
import type { CreateOptionsState } from "./start-switch-sheet";
import type { OpenSessionResult } from "./session-actions";
import { JiraIssuePicker } from "./jira-issue-picker";
import { SourceSessionPicker } from "./source-session-picker";
import { readSource, sourceState, type SourceChoice } from "./source-picker";
import { focusControl } from "./mobile-filters";

export type CreateDraft = { readonly topic: string; readonly role?: CreateRole; readonly start?: CreateStart; readonly projectId?: string;
  readonly issueKey?: string; readonly sourceAgentId?: string; readonly provider?: string; readonly model?: string; readonly effort?: string | null; readonly modeId?: string | null;
  /** The From Jira search text. */
  readonly issueQuery?: string;
  /** The chosen issue as listed, so a later search that does not list it keeps the binding. */
  readonly picked?: JiraIssue;
  /** The From Jira title topic as edited; absent means the issue summary. */
  readonly jiraTopic?: string };

const AD_HOC = "ad-hoc" as const, FROM_JIRA = "from-jira" as const;

/**
 * Defaults fill only unspecified choices. An explicit or saved choice that disappears stays invalid until
 * the operator chooses again; a catalog refresh never silently substitutes another model or permission mode.
 * From Jira takes its desk from the issue, never from the desk the dialog opened in.
 */
export function resolveDraft(options: CreateOptions, draft: CreateDraft) {
  const role = draft.role ?? "LSA", start = draft.start ?? AD_HOC;
  const issue = start === FROM_JIRA && draft.issueKey
    ? options.issues.find((entry) => entry.key === draft.issueKey) ?? (draft.picked?.key === draft.issueKey ? draft.picked : null) : null;
  const route = issue ? issueDesk(options.desks, issue) : null;
  const desk = start === FROM_JIRA ? route?.desk ?? null : (draft.projectId ? options.desks.find((entry) => entry.projectId === draft.projectId)
    : options.desks.find((entry) => entry.projectId === options.catalogProjectId) ?? options.desks[0]) ?? null;
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
  const issueSummaryTopic = issue ? issueTopic(issue.summary) : "";
  const topic = start === AD_HOC ? normalizeTopic(draft.topic) : issue ? normalizeTopic(draft.jiraTopic ?? issueSummaryTopic) : "";
  const title = start === FROM_JIRA ? (issue ? leadTitle(role, issue.key, topic) : "") : leadTitle(role, null, topic);
  const workspaceName = leadWorkspaceName(role, start);
  const sourceAgentId = draft.sourceAgentId?.trim() || null;
  let problem: string | null = null;
  if (!options.enabled) problem = options.disabledReason ?? "New session is off on this host.";
  else if (options.pending) problem = `An earlier creation, “${options.pending.title}”, is not settled. Check it first.`;
  else if (start === FROM_JIRA && options.jiraProblem) problem = options.jiraProblem;
  else if (start === FROM_JIRA && !issue) problem = "Choose a Jira issue.";
  else if (route?.problem) problem = route.problem;
  else if (!desk) problem = options.desks.length ? "The selected desk is no longer available. Choose a listed desk or Refresh desks." : "No desk on this host has lead workspaces.";
  else if (options.catalogProjectId !== desk.projectId) problem = "Refresh this desk's provider catalog before creating a session.";
  else if (role === "TPM" && start === AD_HOC) problem = "Ad hoc creates an LSA only. Choose LSA, or start from a Jira ticket.";
  else if (topicProblem(topic)) problem = topicProblem(topic);
  // A host profile may name a provider this host has switched off: say which and why, and never substitute another.
  else if (!provider?.available) problem = !providerId ? "No active provider is available for this desk."
    : `${provider ? `${provider.label}: ${provider.note}` : `Provider ${providerId} is no longer active.`}${draft.provider === undefined && profile?.provider === providerId ? ` The host's ${profile.name} profile chooses it.` : ""} Choose an active provider.`;
  else if (!model) problem = modelId ? `Model ${modelId} is no longer active for ${provider.label}. Choose an active model.` : "No selectable model.";
  else if (staleEffort) problem = `Effort ${effortId ?? "none"} is no longer offered for ${model.label}. Choose an available effort.`;
  else if (staleMode) problem = `Mode ${modeId ?? "none"} is no longer offered by ${provider.label}. Choose an available mode.`;
  return { desk, role, start, issue, issueSummaryTopic, provider, model, effort, mode, topic, title, workspaceName,
    workspaceMissing: !!desk && !desk.lead[workspaceName], sourceAgentId, problem, fromProfile: preset !== null, staleEffort, staleMode };
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
  return persona === "supported" ? "Persona: sent as the system prompt; Claude and Codex apply it at creation."
    : `Persona: delivered as a bootstrap prompt before the first task, because Kontor has not confirmed that ${providerLabel} applies a system prompt.`;
}

type Phase =
  | { readonly kind: "draft" }
  | { readonly kind: "busy"; readonly requestId: string; readonly label: string }
  | { readonly kind: "result"; readonly outcome: CreateOutcome }
  /** The plugin server's reply was lost: the request may or may not have been dispatched. */
  | { readonly kind: "lost"; readonly requestId: string; readonly title: string; readonly message: string };

export function NewLsaForm({ hostId, colors, compact, options, onRefresh, canOpen, isActive = () => true, onOpenCreated, onCancel }: {
  hostId: string; colors: Colors; compact: boolean; options: CreateOptionsState; onRefresh: (scope?: CreateScope) => void; canOpen: boolean;
  onOpenCreated: (outcome: CreateOutcome) => OpenSessionResult; onCancel: () => void;
  /** A sheet close invalidates pending continuations immediately, before React's unmount cleanup. */
  isActive?: () => boolean;
}) {
  const execute = useRpc(executeCreate), check = useRpc(checkCreate);
  const [draft, setDraft] = useState<CreateDraft>(() => ({ topic: "", role: "LSA", start: AD_HOC, projectId: options.status === "ready" ? options.options.catalogProjectId ?? undefined : undefined }));
  const [phase, setPhase] = useState<Phase>({ kind: "draft" });
  const [advanced, setAdvanced] = useState(!compact);
  const [picker, setPicker] = useState<"jira" | "source" | null>(null);
  const [source, setSource] = useState<SourceChoice | null>(null);
  const [sourceProblem, setSourceProblem] = useState<string | null>(null);
  const pickerReturn = useRef<View>(null), restorePickerFocus = useRef(false);
  const lastPicker = useRef<"jira" | "source" | null>(null);
  if (picker) lastPicker.current = picker;
  useEffect(() => { if (!picker && restorePickerFocus.current) { restorePickerFocus.current = false; focusControl(pickerReturn.current); } }, [picker]);
  const closePicker = () => { restorePickerFocus.current = true; setPicker(null); };
  /** A read repeats the desk whose catalog is wanted and, for From Jira, the issue search. */
  const scopeOf = (next: CreateDraft, projectId = next.projectId): CreateScope => ({ projectId, role: next.role, start: next.start,
    ...(next.start === FROM_JIRA && next.issueQuery?.trim() ? { issueKey: next.issueQuery.trim() } : {}) });
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
  const [openState, setOpenState] = useState<{ requestId: string; status: "opening" | "opened" | "handed" | "failed"; message?: string } | null>(null);
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
    // `handed` is another screen's hand-off: nothing has opened yet, so the open action stays spent.
    if (isCurrent()) setOpenState(result.ok ? { requestId: outcome.requestId, status: result.handoff ? "handed" : "opened" } : { requestId: outcome.requestId, status: "failed", message: result.message });
  };
  const finish = (outcome: CreateOutcome, requestId: string, title: string) => {
    if (!isCurrent()) return;
    if (outcome.requestId !== requestId) {
      setPhase({ kind: "lost", requestId, title, message: "The host replied with a different request ID. Check this request before opening any session." });
      return;
    }
    setPhase({ kind: "result", outcome });
    if (outcome.state === "unknown" || outcome.state === "refused") onRefresh(scopeOf(draft));
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
      {opened ? <Text testID="kontor-new-lsa-open-status" accessibilityLiveRegion="polite" style={muted}>{opened.status === "opening" ? "Opening session…" : opened.status === "opened" ? "Opened in Paseo." : opened.status === "handed" ? "Opening in Kontor…" : `Created. Opening failed: ${opened.message} Use Open session to try again.`}</Text> : null}
      <View style={{ flexDirection: "row", flexWrap: "wrap", gap: 8 }}>
        {created && outcome.agentId && canOpen && opened?.status !== "opened" && opened?.status !== "handed" ? <Action small label="Open session" colors={colors} disabled={opened?.status === "opening" || options.status === "offline"} onPress={() => openCreated(outcome)} /> : null}
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
      {options.status === "error" ? <Action small label="Try again" colors={colors} onPress={() => onRefresh(scopeOf(draft))} /> : null}
    </>);
  }
  const problem = (options.status === "ready" ? choice.problem : options.status === "error"
    ? `Choices could not be refreshed (${options.message}). Refresh choices before creating a session.`
    : "Refreshing this desk's choices… Creation stays disabled until the host answers.")
    ?? (source && source.hostId !== hostId ? "Choose a source session on this host, or Clear the source." : sourceProblem);
  // An unsettled earlier request (this device or another) is shown instead of the form until a Check settles it.
  if (ready.pending) return outcomeView(ready.pending, providerLabel);

  const submit = async () => {
    if (!isCurrent() || currentOptions.current.status !== "ready" || submitted.current || problem || !choice.desk || !choice.provider || !choice.model) return;
    submitted.current = true;
    if (source) {
      setPhase({ kind: "busy", requestId: "", label: "Checking the source session…" });
      try {
        const fresh = await readSource(getPaseoClient(hostId), source.entry.id);
        if (!isCurrent()) return;
        const changed = fresh && fresh.entry.title !== source.entry.title;
        if (!fresh || fresh.contextProblem || changed) {
          if (fresh) setSource({ hostId, ...fresh });
          setSourceProblem(!fresh ? "The source session was not found. Choose another source, or Clear it." : fresh.contextProblem
            ?? "The source session's title changed. Use Change to review it before creating.");
          submitted.current = false; setPhase({ kind: "draft" }); return;
        }
      } catch (error) {
        if (isCurrent()) { setSourceProblem(`Source lookup failed: ${error instanceof Error ? error.message : String(error)}. Use Change to retry, or Clear it.`); submitted.current = false; setPhase({ kind: "draft" }); }
        return;
      }
    }
    if (!isCurrent() || currentOptions.current.status !== "ready") { submitted.current = false; if (isCurrent()) setPhase({ kind: "draft" }); return; }
    const requestId = newRequestId(), title = choice.title;
    setPhase({ kind: "busy", requestId, label: `Creating “${title}”…` });
    execute({ requestId, role: choice.role, start: choice.start, projectId: choice.desk.projectId, topic: choice.topic,
      ...(choice.start === FROM_JIRA ? { issueKey: choice.issue?.key ?? null } : {}),
      sourceAgentId: choice.sourceAgentId, provider: choice.provider.id, model: choice.model.id, effort: choice.effort?.id ?? null, modeId: choice.mode?.id ?? null })
      .then((outcome) => finish(outcome, requestId, title),
        (error: unknown) => { if (isCurrent()) setPhase({ kind: "lost", requestId, title, message: `The Kontor server did not answer (${error instanceof Error ? error.message : String(error)}).` }); });
  };
  const set = (patch: Partial<CreateDraft>) => setDraft({ ...draft, ...patch });
  const refresh = (patch: Partial<CreateDraft>) => onRefresh(scopeOf({ ...draft, role: choice.role, start: choice.start, ...patch },
    patch.projectId ?? choice.desk?.projectId ?? ready.catalogProjectId ?? undefined));
  const summary = choice.desk ? [choice.provider && !choice.provider.available ? `${choice.provider.label}, ${choice.provider.status}` : choice.provider?.label, choice.model?.label, choice.effort?.label, choice.mode ? `${choice.mode.label}${choice.mode.dangerous ? " ⚠" : ""}` : null].filter(Boolean).join(" · ")
    : choice.start === FROM_JIRA ? "Choose an issue first" : "Choose a desk first";
  const providerEmpty = !choice.desk ? (choice.start === FROM_JIRA ? "Choose an issue to read its desk's providers." : ready.desks.length ? "Choose an available desk to read its providers." : "Provider choices require a desk working directory.")
    : ready.catalogProjectId !== choice.desk.projectId ? "Refresh choices to read this desk's providers." : "No active provider is available for this desk.";
  const fromJira = choice.start === FROM_JIRA;
  const input = { color: colors.foreground, fontSize: 13, minHeight: tall ? 44 : 34, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6 } as const;
  const heading = { color: colors.foreground, fontSize: 12, fontWeight: "600" } as const;
  const pick = (issue: JiraIssue) => {
    const desk = issueDesk(ready.desks, issue).desk;
    set({ issueKey: issue.key, picked: issue, jiraTopic: draft.issueKey === issue.key ? draft.jiraTopic : undefined, ...(desk ? { projectId: desk.projectId } : {}) });
    if (desk) refresh({ projectId: desk.projectId, issueQuery: issue.key });
  };
  if (picker === "jira") return <JiraIssuePicker colors={colors} desks={ready.desks} scope={scopeOf(draft)} initialQuery={draft.issueQuery ?? ""} selected={choice.issue?.key ?? null}
    onBack={closePicker} onSelect={(issue, issueQuery) => { pick(issue); setDraft((previous) => ({ ...previous, issueQuery })); closePicker(); }} />;
  if (picker === "source") return <SourceSessionPicker hostId={hostId} colors={colors} selected={source?.hostId === hostId ? source.entry.id : null}
    onBack={closePicker} onSelect={(next) => { setSource(next); setSourceProblem(null); set({ sourceAgentId: next.entry.id }); closePicker(); }} />;
  return wrap(<>
    <Choices label="Role" colors={colors} tall={tall} selected={choice.role} onSelect={(role) => {
      const nextStart: CreateStart = role === "TPM" && choice.start === AD_HOC ? FROM_JIRA : choice.start;
      set({ role: role as CreateRole, start: nextStart }); refresh({ role: role as CreateRole, start: nextStart }); }}
      options={[{ id: "LSA", label: "LSA · Lead Software Architect" }, { id: "TPM", label: "TPM · Technical Program Manager" }]} />
    <Choices label="Start" colors={colors} tall={tall} selected={choice.start} onSelect={(start) => { set({ start: start as CreateStart }); refresh({ start: start as CreateStart }); if (start === FROM_JIRA) setPicker("jira"); }}
      options={[
        { id: AD_HOC, label: "Ad hoc", disabled: choice.role === "TPM", note: choice.role === "TPM" ? "TPM is key-bound; choose From Jira ticket." : null },
        { id: FROM_JIRA, label: "From Jira ticket", note: "The issue chooses the desk." },
      ]} />
    {fromJira ? <View style={{ gap: 4 }}>
      <Text style={heading}>Jira issue</Text>
      {choice.issue ? <Text selectable style={{ color: colors.foreground, fontSize: 13 }}>{choice.issue.key} · {choice.issue.summary}</Text> : null}
      {choice.issue ? <Text style={muted}>{choice.issue.type} · {choice.issue.status}</Text> : null}
      <Action ref={lastPicker.current === "jira" ? pickerReturn : undefined} label={choice.issue ? "Change Jira issue" : "Choose Jira issue"} colors={colors} onPress={() => setPicker("jira")} />
      {ready.jiraProblem ? <Text testID="kontor-new-lsa-jira" style={{ color: colors.statusWarning, fontSize: 12 }}>{ready.jiraProblem}</Text> : null}
      <Text testID="kontor-new-session-desk" style={muted}>{choice.issue && choice.desk ? `Desk: ${choice.desk.project}, because ${choice.issue.key} is ${choice.issue.epic ? "an epic" : `a ${choice.issue.type} without an active parent epic`}.`
        : "Desk: follows the issue. An epic starts on DESK • ADAM; another open issue without an active parent epic starts on DESK • TICKETS."}</Text>
      {choice.issue ? <View style={{ gap: 4 }}>
        <Text style={heading}>Title</Text>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Text style={{ color: colors.foreground, fontSize: 13 }}>{leadTitle(choice.role, choice.issue.key, "").trimEnd()}</Text>
          <TextInput testID="kontor-new-session-jira-topic" accessibilityLabel={`Title topic after ${choice.role} • ${choice.issue.key}`} maxLength={TOPIC_MAX}
            value={draft.jiraTopic ?? choice.issueSummaryTopic} onChangeText={(jiraTopic) => set({ jiraTopic })} onSubmitEditing={() => { if (!problem) submit(); }} returnKeyType="go"
            placeholderTextColor={colors.foregroundMuted} style={{ ...input, flex: 1, minWidth: 0 }} />
        </View>
        <Text style={muted}>Starts as the issue summary. Editing it keeps {choice.issue.key}.</Text>
      </View> : null}
    </View> : <>
      <Choices label="Desk" colors={colors} tall={tall} selected={choice.desk?.projectId ?? null} onSelect={(projectId) => { set({ projectId }); refresh({ projectId }); }}
        options={ready.desks.map((desk) => ({ id: desk.projectId, label: desk.project }))} empty="No desk on this host has lead workspaces." />
      <View style={{ gap: 4 }}>
        <Text style={heading}>Topic</Text>
        <TextInput testID="kontor-new-lsa-topic" accessibilityLabel="Topic" placeholder="What this session is about" autoFocus={!compact} maxLength={TOPIC_MAX} value={draft.topic}
          onChangeText={(topic) => set({ topic })} onSubmitEditing={() => { if (!problem) submit(); }} returnKeyType="go"
          placeholderTextColor={colors.foregroundMuted} style={input} />
      </View>
    </>}
    <View style={{ gap: 4 }}>
      <Text style={heading}>Bring context from another session (optional)</Text>
      <Text style={muted}>Copies its last assistant message from the last 100 timeline entries before the first task.</Text>
      {source ? <>
        <Text selectable testID="kontor-source-selected" style={{ color: colors.foreground, fontSize: 13 }}>{source.entry.title}</Text>
        <Text selectable style={muted}>{sourceState(source.entry)} · {source.entry.id}</Text>
      </> : null}
      <View style={{ flexDirection: "row", gap: 6 }}>
        <Action ref={lastPicker.current === "source" ? pickerReturn : undefined} label={source ? "Change source" : "Choose source session"} colors={colors} onPress={() => setPicker("source")} />
        {source ? <Action label="Clear source" colors={colors} onPress={() => { setSource(null); setSourceProblem(null); set({ sourceAgentId: undefined }); }} /> : null}
      </View>
    </View>
    <Text testID="kontor-new-lsa-title" style={muted}>{choice.title ? `Title: ${choice.title}. ` : ""}
      {fromJira ? `The lead is bound to ${choice.issue?.key ?? "the chosen issue"}.` : "The session stays ad hoc and writes a handoff summary when a key is confirmed."}
      {choice.workspaceMissing ? ` The ${choice.workspaceName} workspace is created on this desk.` : ""}</Text>
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
    <Action small label={choice.desk || fromJira ? "Refresh choices" : "Refresh desks"} colors={colors} onPress={() => refresh({})} />
    {choice.mode?.dangerous ? <Text testID="kontor-new-lsa-danger" style={{ color: colors.statusWarning, fontSize: 12 }}>⚠ {choice.mode.label} is a dangerous permission mode on this host.</Text> : null}
    {/* A provider that cannot start promises no persona delivery; the problem line names it and why. */}
    {choice.provider?.available ? <Text style={muted}>{personaLine(choice.provider.persona, providerLabel)}</Text> : null}
    {ready.notes.map((note) => <Text key={note} style={muted}>{note}</Text>)}
    {problem ? <Text testID="kontor-new-lsa-problem" accessibilityLiveRegion="polite" style={{ color: colors.foregroundMuted, fontSize: 12 }}>{problem}</Text> : null}
  </>, <>
      <Action small label="Cancel" colors={colors} onPress={onCancel} />
      <Pressable testID="kontor-new-lsa-create" accessibilityRole="button" accessibilityLabel={`Create ${choice.title || "session"}`} accessibilityHint={problem ?? undefined} accessibilityState={{ disabled: !!problem }} aria-disabled={!!problem}
        disabled={!!problem} onPress={submit} style={{ minHeight: tall ? 44 : 30, paddingHorizontal: 12, borderRadius: 6, justifyContent: "center", backgroundColor: problem ? colors.surface2 : colors.accent, opacity: problem ? 0.6 : 1 }}>
        <Text style={{ color: problem ? colors.foregroundMuted : colors.accentForeground, fontSize: 12, fontWeight: "600" }}>Create session</Text>
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
