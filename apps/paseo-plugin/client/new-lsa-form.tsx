import { useEffect, useRef, useState } from "react";
import { Pressable, ScrollView, Text, View } from "react-native";
import { Icon, TextInput } from "@getpaseo/plugin/client/react-native";
import { getPaseoClient, useRpc } from "@getpaseo/plugin/client";
import { newRequestId } from "./request-id";
import { checkCreate, executeCreate, featureValuesProblem, issueDesk, issueTopic, leadTitle, leadWorkspaceName, normalizeTopic, topicProblem, TOPIC_MAX,
  type CreateFeature, type CreateOptions, type CreateOutcome, type CreateProvider, type CreateRole, type CreateScope, type CreateStart, type FeatureValues, type JiraIssue } from "../shared/session-create";
import type { NewLeadDefault } from "../shared/organization";
import type { SummaryRevision } from "../shared/session-navigator";
import { Action, COARSE_POINTER, Copy, IconButton, type Colors } from "./page-controls";
import { readableInk } from "./session-status-ink";
import { prefixBeforeSeparator } from "./session-organization";
import type { CreateOptionsState } from "./start-switch-sheet";
import type { OpenSessionResult } from "./session-actions";
import type { DirectoryEntry } from "./session-directory";
import { JiraIssuePicker } from "./jira-issue-picker";
import { SourceSessionPicker } from "./source-session-picker";
import { readSource, sourceState, type SourceChoice } from "./source-picker";
import { focusControl } from "./mobile-filters";
import { useOrganizationSettings } from "./use-organization-settings";
import { Choices, Disclosure, featureLabels, featureValue, ModelPicker, useCreateFeatures, type PickedModel } from "./model-picker";
import { DenseSessionRow } from "./dense-session-row";
import { useClock } from "./session-time";
import { CODEX, codexDefault, leadDefault, type LeadModel } from "./new-lead-defaults";
import { LeadSourcePicker, type LeadSource } from "./new-lead-source";
import { modelAlias, modelVendor, routeLabel } from "./session-usage";
import { VendorMark } from "./vendor-mark";

export type CreateDraft = { readonly topic: string; readonly role?: CreateRole; readonly start?: CreateStart; readonly projectId?: string;
  readonly issueKey?: string; readonly sourceAgentId?: string; readonly provider?: string; readonly model?: string; readonly effort?: string | null; readonly modeId?: string | null;
  /** The From Jira search text. */
  readonly issueQuery?: string;
  /** The chosen issue as listed, so a later search that does not list it keeps the binding. */
  readonly picked?: JiraIssue;
  /** The From Jira title topic as edited; absent means the issue summary. */
  readonly jiraTopic?: string;
  /** Speed, Plan and other listed settings chosen in Model & settings; absent takes the saved default's, else none. */
  readonly featureValues?: FeatureValues };

const AD_HOC = "ad-hoc" as const, FROM_JIRA = "from-jira" as const;
/** The listed Speed setting and its Fast value: the model button marks Fast in its corner and never names a speed. */
const SPEED = "service_tier", FAST = "priority";
/** The selected role's full name, under the role switch (board B1). */
const ROLE_NAMES: Record<CreateRole, string> = { LSA: "Lead Software Architect", TPM: "Technical Program Manager" };
/** "DESK • PR REVIEW" is PR REVIEW under the Desk label; another project name stays whole. */
function deskShortName(project: string): string {
  return prefixBeforeSeparator(project)?.toLocaleLowerCase() === "desk" ? project.slice(project.indexOf("\u2022") + 1).trim() || project : project;
}

/** Why no Codex default can stand in for a model the operator has not chosen. */
function codexUnknown(providers: readonly CreateProvider[]): string {
  const codex = providers.find((entry) => entry.id === CODEX);
  const why = !codex ? "this host lists no Codex provider" : !codex.available ? `${codex.label}: ${(codex.note ?? "not available").replace(/\.$/, "")}` : "Codex flags no default model here";
  return `Codex default unknown: ${why}. Choose a model for this lead.`;
}

/**
 * Defaults fill only unspecified choices: an unchosen model is the saved new-lead default while this host offers it,
 * else Codex's own catalog default at its default effort; a role profile or the first listed model never stands in.
 * An explicit choice that disappears stays invalid until the operator chooses again; a catalog refresh never silently
 * substitutes another model or permission mode. From Jira takes its desk from the issue, never from the desk the dialog opened in.
 */
export function resolveDraft(options: CreateOptions, draft: CreateDraft, saved: NewLeadDefault | null = null) {
  const role = draft.role ?? "LSA", start = draft.start ?? AD_HOC;
  const issue = start === FROM_JIRA && draft.issueKey
    ? options.issues.find((entry) => entry.key === draft.issueKey) ?? (draft.picked?.key === draft.issueKey ? draft.picked : null) : null;
  const route = issue ? issueDesk(options.desks, issue) : null;
  const desk = start === FROM_JIRA ? route?.desk ?? null : (draft.projectId ? options.desks.find((entry) => entry.projectId === draft.projectId)
    : options.desks.find((entry) => entry.projectId === options.catalogProjectId) ?? options.desks[0]) ?? null;
  const lead = leadDefault(options.providers, saved), codex = codexDefault(options.providers);
  const auto: LeadModel | null = draft.provider === undefined && draft.model === undefined ? lead.choice : null;
  const providerId = draft.provider ?? auto?.provider;
  const provider = options.providers.find((entry) => entry.id === providerId) ?? null;
  const modelId = draft.model ?? auto?.model ?? provider?.defaultModel ?? undefined;
  const model = provider?.available ? provider.models.find((entry) => entry.id === modelId) ?? null : null;
  const effortId = draft.effort !== undefined ? draft.effort : (auto?.model === model?.id ? auto?.effort : null) ?? model?.defaultEffort ?? model?.efforts[0]?.id;
  const effort = model?.efforts.find((entry) => entry.id === effortId) ?? null;
  const modeId = draft.modeId !== undefined ? draft.modeId : provider?.defaultMode ?? provider?.modes[0]?.id;
  const featureValues = draft.featureValues ?? (lead.saved ? auto?.featureValues : undefined) ?? {};
  const mode = provider?.available ? provider.modes.find((entry) => entry.id === modeId) ?? null : null;
  const staleEffort = !effort && (!!effortId || !!model?.efforts.length);
  const staleMode = !mode && (!!modeId || !!provider?.modes.length);
  const issueSummaryTopic = issue ? issueTopic(issue.summary) : "";
  const topic = start === AD_HOC ? normalizeTopic(draft.topic) : issue ? normalizeTopic(draft.jiraTopic ?? issueSummaryTopic) : "";
  const title = start === FROM_JIRA ? (issue ? leadTitle(role, issue.key, topic) : "") : leadTitle(role, null, topic);
  const workspaceName = leadWorkspaceName(role, start);
  const sourceAgentId = draft.sourceAgentId?.trim() || null;
  let problem: string | null = null;
  if (!options.enabled) problem = options.disabledReason ?? "New lead is off on this host.";
  else if (options.pending) problem = `An earlier creation, “${options.pending.title}”, is not settled. Check it first.`;
  else if (start === FROM_JIRA && options.jiraProblem) problem = options.jiraProblem;
  else if (start === FROM_JIRA && !issue) problem = "Choose a Jira issue.";
  else if (route?.problem) problem = route.problem;
  else if (!desk) problem = options.desks.length ? "The selected desk is no longer available. Choose a listed desk or Refresh desks." : "No desk on this host has lead workspaces.";
  else if (options.catalogProjectId !== desk.projectId) problem = "Refresh this desk's provider catalog before creating a lead.";
  else if (role === "TPM" && start === AD_HOC) problem = "Ad hoc creates an LSA only. Choose LSA, or start from a Jira ticket.";
  else if (topicProblem(topic)) problem = topicProblem(topic);
  // A chosen provider this host has switched off is named with why, and never substituted.
  else if (!provider?.available) problem = !providerId ? options.providers.some((entry) => entry.available) ? codexUnknown(options.providers) : "No active provider is available for this desk."
    : `${provider ? `${provider.label}: ${provider.note}` : `Provider ${providerId} is no longer active.`} Choose an active provider.`;
  else if (!model) problem = modelId ? `Model ${modelId} is no longer active for ${provider.label}. Choose an active model.` : `${provider.label} flags no default model. Choose a model for this lead.`;
  else if (staleEffort) problem = `Effort ${effortId ?? "none"} is no longer offered for ${model.label}. Choose an available effort.`;
  else if (staleMode) problem = `Mode ${modeId ?? "none"} is no longer offered by ${provider.label}. Choose an available mode.`;
  return { desk, role, start, issue, issueSummaryTopic, provider, model, effort, mode, featureValues, topic, title, workspaceName,
    workspaceMissing: !!desk && !desk.lead[workspaceName], sourceAgentId, problem, staleEffort, staleMode,
    /** Codex's own default, and the default an unchosen model takes; `note` names a saved default this host no longer offers. */
    codex, lead, defaultNote: auto || !providerId ? lead.note : null };
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

/** A chosen source; `summary` is the saved revision pinned for the lead, absent when the session's last message travels instead. */
type LeadSourceChoice = SourceChoice & { readonly summary?: SummaryRevision | null };
type Picker = "jira" | "source" | "model";

/** "GPT-6 Astra · XHigh", or null when this host does not list the model. */
function leadName(providers: readonly CreateProvider[], choice: LeadModel | null): string | null {
  const model = providers.find((entry) => entry.id === choice?.provider)?.models.find((entry) => entry.id === choice?.model);
  const effort = model?.efforts.find((entry) => entry.id === choice?.effort);
  return model ? `${model.label}${effort ? ` · ${effort.label}` : ""}` : null;
}

export function NewLsaForm({ hostId, colors, compact, options, onRefresh, canOpen, isActive = () => true, onOpenCreated, onCancel, sources, opened, header, initialSource = null, hostScrolls = false, pickerHeight }: {
  hostId: string; colors: Colors; compact: boolean; options: CreateOptionsState; onRefresh: (scope?: CreateScope) => void; canOpen: boolean;
  onOpenCreated: (outcome: CreateOutcome) => OpenSessionResult; onCancel: () => void;
  /** A sheet close invalidates pending continuations immediately, before React's unmount cleanup. */
  isActive?: () => boolean;
  /** The sessions Choose source lists, with their saved summaries; absent keeps the host-wide source search. */
  sources?: readonly DirectoryEntry[];
  /** When this device last opened each session from Kontor, for the source rows. */
  opened?: ReadonlyMap<string, number>;
  /** Inside the host's popover, which scrolls an unbounded body: the height issue/source pickers keep so its actions stay in view, given its title's line. */
  pickerHeight?: (line: number) => number;
  /** The surface's own heading: the model chip then stands at its right, pinned above the form. Pickers bring their own. */
  header?: React.ReactNode;
  /** A source chosen before the form opened, such as Attach summary's New lead, with its saved revision pinned. */
  initialSource?: LeadSource | null;
  /** The surface around the form scrolls already (the composer's popover or sheet), so Model & settings adds no scroller. */
  hostScrolls?: boolean;
}) {
  const execute = useRpc(executeCreate), check = useRpc(checkCreate);
  const settings = useOrganizationSettings();
  const [draft, setDraft] = useState<CreateDraft>(() => ({ topic: "", role: "LSA", start: AD_HOC, projectId: options.status === "ready" ? options.options.catalogProjectId ?? undefined : undefined,
    ...(initialSource ? { sourceAgentId: initialSource.entry.id } : {}) }));
  const [phase, setPhase] = useState<Phase>({ kind: "draft" });
  const [picker, setPicker] = useState<Picker | null>(null);
  const [source, setSource] = useState<LeadSourceChoice | null>(initialSource ? { hostId, entry: initialSource.entry, contextProblem: null, summary: initialSource.summary } : null);
  const [sourceProblem, setSourceProblem] = useState<string | null>(null);
  const [defaultProblem, setDefaultProblem] = useState<string | null>(null);
  // The technical Details open on demand, so the form keeps one line for them (board B1).
  const [open, setOpen] = useState<"details" | null>(null);
  const clock = useClock();
  const pickerReturn = useRef<View>(null), restorePickerFocus = useRef(false);
  const lastPicker = useRef<Picker | null>(null);
  if (picker) lastPicker.current = picker;
  useEffect(() => { if (!picker && restorePickerFocus.current) { restorePickerFocus.current = false; focusControl(pickerReturn.current); } }, [picker]);
  const closePicker = () => { restorePickerFocus.current = true; setPicker(null); };
  const deskChoosable = useRef(false);
  /** A read repeats the desk whose catalog is wanted and, for From Jira, the issue search. */
  const scopeOf = (next: CreateDraft, projectId = next.projectId): CreateScope => ({ projectId, role: next.role, start: next.start,
    ...(next.start === FROM_JIRA && next.issueQuery?.trim() ? { issueKey: next.issueQuery.trim() } : {}) });
  const submitted = useRef(false);
  /** The hint shown when the form last settled; a read in flight keeps it on screen. */
  const settledHint = useRef<string | null>(null);
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
  // The scroll's bottom inset lets its last line rise fully above the footer.
  const wrap = (children: React.ReactNode, footer?: React.ReactNode, chip: React.ReactNode = null, note: string | null = null) => <>
    {header === undefined ? null : <View style={{ flexDirection: "row", alignItems: "center", gap: 6, paddingBottom: 4 }}>{header}{chip}</View>}
    <ScrollView testID="kontor-new-lsa" keyboardShouldPersistTaps="handled" style={{ flexGrow: 0, flexShrink: 1 }} contentContainerStyle={{ gap: 8, padding: 6, paddingBottom: 24 }}>{header === undefined ? chip : null}{children}</ScrollView>
    {footer ? <View style={{ gap: 2, paddingHorizontal: 6, paddingTop: 6, borderTopWidth: 1, borderColor: colors.border }}>
      <View style={{ flexDirection: "row", justifyContent: "space-between", gap: 8 }}>{footer}</View>
      {note ? <Text testID="kontor-new-lsa-footer-note" style={{ color: colors.foregroundMuted, fontSize: 11, textAlign: "center" }}>{note}</Text> : null}
    </View> : null}
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
  const saved = settings.state.status === "ready" ? settings.state.values.newLeadDefault ?? null : null;
  const choice = ready ? resolveDraft(ready, draft, saved) : null;
  const features = useCreateFeatures(choice?.desk?.projectId ?? null, choice?.model ? choice.provider?.id ?? null : null, choice?.model?.id ?? null);
  // The settings last read for this model keep naming its chosen Speed and Plan while a role or desk change reads them
  // again, or leaves no desk to read them for; Create and its hint still wait for the fresh read. A failed read names none.
  const modelKey = choice?.model ? JSON.stringify([choice.provider?.id, choice.model.id]) : null;
  const lastListed = useRef<{ model: string; features: readonly CreateFeature[] } | null>(null);
  if (features && modelKey) lastListed.current = { model: modelKey, features: features.problem ? [] : features.features };
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
  const unchosen = draft.provider === undefined && draft.model === undefined;
  const settingsReading = unchosen && settings.state.status === "loading" ? "Reading the new-lead default…"
    : Object.keys(choice.featureValues).length && !features ? "Reading this model's settings…" : null;
  const settingsProblem = settingsReading ?? (!Object.keys(choice.featureValues).length || !features ? null
    : features.problem ? `Model settings could not be read: ${features.problem}` : featureValuesProblem(features.features, choice.featureValues));
  const problem = (options.status === "ready" ? choice.problem ?? settingsProblem : options.status === "error"
    ? `Choices could not be refreshed (${options.message}). Refresh choices before creating a lead.`
    : "Refreshing this desk's choices… Creation stays disabled until the host answers.")
    ?? (source && source.hostId !== hostId ? "Choose a source session on this host, or Clear the source." : sourceProblem);
  // A read in flight blocks Create and names itself to assistive technology, but the visible hint keeps what the last
  // settled read said, so switching a role or desk never flashes a passing status or an unconfirmed problem.
  const reading = options.status === "loading" || !!settingsReading;
  if (!reading) settledHint.current = problem;
  const hint = reading ? settledHint.current : problem;
  // An unsettled earlier request (this device or another) is shown instead of the form until a Check settles it.
  if (ready.pending) return outcomeView(ready.pending, providerLabel);

  const submit = async () => {
    if (!isCurrent() || currentOptions.current.status !== "ready" || submitted.current || problem || !choice.desk || !choice.provider || !choice.model) return;
    submitted.current = true;
    // A pinned summary revision never changes, so only a session whose last message travels is read again.
    if (source && !source.summary) {
      setPhase({ kind: "busy", requestId: "", label: "Checking the source session…" });
      try {
        const fresh = await readSource(getPaseoClient(hostId), source.entry.id);
        if (!isCurrent()) return;
        const changed = fresh && fresh.entry.title !== source.entry.title;
        if (!fresh || fresh.contextProblem || changed) {
          if (fresh) setSource({ hostId, ...fresh });
          setSourceProblem(!fresh ? "The source session was not found. Choose another source, or Clear it." : fresh.contextProblem
            ?? "The source session's title changed. Choose the source again to review it before creating.");
          submitted.current = false; setPhase({ kind: "draft" }); return;
        }
      } catch (error) {
        if (isCurrent()) { setSourceProblem(`Source lookup failed: ${error instanceof Error ? error.message : String(error)}. Choose the source again to retry, or clear it.`); submitted.current = false; setPhase({ kind: "draft" }); }
        return;
      }
    }
    if (!isCurrent() || currentOptions.current.status !== "ready") { submitted.current = false; if (isCurrent()) setPhase({ kind: "draft" }); return; }
    const requestId = newRequestId(), title = choice.title;
    setPhase({ kind: "busy", requestId, label: `Creating “${title}”…` });
    execute({ requestId, role: choice.role, start: choice.start, projectId: choice.desk.projectId, topic: choice.topic,
      ...(choice.start === FROM_JIRA ? { issueKey: choice.issue?.key ?? null } : {}),
      ...(source?.summary ? { sourceAgentId: null, sourceSummary: { summaryId: source.summary.summaryId, revision: source.summary.revision } } : { sourceAgentId: choice.sourceAgentId }),
      provider: choice.provider.id, model: choice.model.id, effort: choice.effort?.id ?? null, modeId: choice.mode?.id ?? null,
      ...(Object.keys(choice.featureValues).length ? { featureValues: choice.featureValues } : {}) })
      .then((outcome) => finish(outcome, requestId, title),
        (error: unknown) => { if (isCurrent()) setPhase({ kind: "lost", requestId, title, message: `The Kontor server did not answer (${error instanceof Error ? error.message : String(error)}).` }); });
  };
  const set = (patch: Partial<CreateDraft>) => setDraft({ ...draft, ...patch });
  // A desk that is no longer listed is never read again; without a listed desk the read lists the desks that remain.
  const listedDesk = (projectId: string | null | undefined) => ready.desks.some((entry) => entry.projectId === projectId) ? projectId ?? undefined : undefined;
  const refresh = (patch: Partial<CreateDraft>) => {
    const next = { ...draft, role: choice.role, start: choice.start, ...patch };
    onRefresh(scopeOf({ ...next, projectId: undefined }, patch.projectId ?? choice.desk?.projectId ?? listedDesk(ready.catalogProjectId) ?? listedDesk(next.projectId)));
  };
  const same = (lead: LeadModel | null) => !!lead && !!choice.model && lead.provider === choice.provider?.id && lead.model === choice.model.id && lead.effort === (choice.effort?.id ?? null);
  const tag = same(choice.codex) ? "Codex default" : choice.lead.saved && same(choice.lead.choice) ? "New-lead default" : !choice.codex ? "Codex default unknown" : null;
  // Only listed settings are named; an unreadable list names none rather than guessing.
  const listedSettings = lastListed.current?.model === modelKey ? lastListed.current.features : [];
  const shown = featureLabels(listedSettings.filter((feature) => feature.id !== SPEED), choice.featureValues);
  const speed = listedSettings.find((feature) => feature.id === SPEED);
  const fast = speed?.type === "select" && featureValue(speed, choice.featureValues) === FAST && speed.options.some((option) => option.id === FAST);
  const chipDetail = [choice.effort?.label, ...shown.selects].filter(Boolean).join(" · ");
  const settingsLine = [choice.mode ? `${choice.mode.label}${choice.mode.dangerous ? " ⚠" : ""}` : null, ...shown.toggles,
    choice.provider ? routeLabel(choice.provider.id, choice.provider.label) : null].filter(Boolean).join(" | ");
  const defaultName = choice.lead.saved ? leadName(ready.providers, choice.lead.choice) ?? "Codex default unknown"
    : choice.codex ? `Codex default (${leadName(ready.providers, choice.codex)})` : "Codex default unknown";
  const saveDefault = (newLeadDefault: NewLeadDefault | null, failed: string) => {
    setDefaultProblem(null);
    settings.save({ newLeadDefault }).then((ok) => { if (!ok && isCurrent()) setDefaultProblem(failed); });
  };
  const fromJira = choice.start === FROM_JIRA;
  const input = { color: colors.foreground, fontSize: 13, minHeight: tall ? 44 : 34, paddingHorizontal: 9, borderWidth: 1, borderColor: colors.border, borderRadius: 6 } as const;
  const heading = { color: colors.foreground, fontSize: 12, fontWeight: "600" } as const;
  const pick = (issue: JiraIssue) => {
    const desk = issueDesk(ready.desks, issue).desk;
    set({ issueKey: issue.key, picked: issue, jiraTopic: draft.issueKey === issue.key ? draft.jiraTopic : undefined, ...(desk ? { projectId: desk.projectId } : {}) });
    if (desk) refresh({ projectId: desk.projectId, issueQuery: issue.key });
  };
  if (picker === "jira") return <JiraIssuePicker height={pickerHeight} colors={colors} desks={ready.desks} scope={scopeOf(draft)} initialQuery={draft.issueQuery ?? ""} selected={choice.issue?.key ?? null}
    onBack={closePicker} onSelect={(issue, issueQuery) => { pick(issue); setDraft((previous) => ({ ...previous, issueQuery })); closePicker(); }} />;
  if (picker === "source") return sources ? <LeadSourcePicker colors={colors} compact={compact} sources={sources} opened={opened} height={pickerHeight} selected={source?.hostId === hostId ? source.entry.id : null} onCancel={closePicker}
    onUse={(next) => { setSource({ hostId, entry: next.entry, contextProblem: null, summary: next.summary }); setSourceProblem(null); set({ sourceAgentId: next.entry.id }); closePicker(); }} />
    : <SourceSessionPicker hostId={hostId} colors={colors} selected={source?.hostId === hostId ? source.entry.id : null}
      onBack={closePicker} onSelect={(next) => { setSource(next); setSourceProblem(null); set({ sourceAgentId: next.entry.id }); closePicker(); }} />;
  if (picker === "model") return <ModelPicker colors={colors} compact={compact} hostScrolls={hostScrolls} providers={ready.providers} projectId={choice.desk?.projectId ?? null} codex={choice.codex}
    value={choice.provider && choice.model ? { provider: choice.provider.id, model: choice.model.id, effort: choice.effort?.id ?? null, modeId: choice.mode?.id ?? null, featureValues: choice.featureValues } : null}
    onCancel={closePicker} onUse={(next: PickedModel) => { set({ provider: next.provider, model: next.model, effort: next.effort, modeId: next.modeId, featureValues: next.featureValues }); closePicker(); }}
    defaults={settings.state.status === "ready" ? { name: defaultName,
      make: (next) => saveDefault({ provider: next.provider, model: next.model, effort: next.effort, ...(Object.keys(next.featureValues).length ? { featureValues: next.featureValues } : {}) },
        "The new-lead default was not saved. This lead still uses the chosen model."),
      reset: () => saveDefault(null, "The new-lead default was not reset.") } : undefined} />;
  const chip = <View style={{ alignSelf: header === undefined ? "flex-end" : "auto", alignItems: "flex-end", gap: 2, flexShrink: 1, maxWidth: "62%" }}>
    <Pressable ref={lastPicker.current === "model" ? pickerReturn : undefined} testID="kontor-new-lead-model" accessibilityRole="button" onPress={() => setPicker("model")}
      accessibilityLabel={`Model: ${choice.model ? `${choice.model.label}, ${choice.model.id}` : "none chosen"}${chipDetail ? `, ${chipDetail}` : ""}${fast ? ", Fast" : ""}${tag ? `, ${tag}` : ""}. Change model`}
      style={{ maxWidth: "100%", minHeight: tall ? 44 : 36, flexDirection: "row", alignItems: "center", gap: 8, paddingLeft: 8, paddingRight: 16, paddingVertical: 4, borderWidth: 1, borderColor: colors.border, borderRadius: 8 }}>
      <VendorMark vendor={modelVendor(choice.model?.id ?? null)} colors={colors} size={18} />
      <View style={{ flexShrink: 1 }}>
        <Text numberOfLines={1} style={{ color: colors.foreground, fontSize: 13, fontWeight: "600" }}>{choice.model ? modelAlias(choice.model.id) : "Choose model"} ▾</Text>
        {chipDetail ? <Text numberOfLines={1} style={muted}>{chipDetail}</Text> : null}
      </View>
      {/* The corner is reserved in either speed, so marking Fast never resizes the button. */}
      {fast ? <View testID="kontor-new-lead-model-fast" aria-hidden style={{ position: "absolute", top: 3, right: 3 }}><Icon name="Zap" size={10} color={colors.foreground} /></View> : null}
    </Pressable>
    {tag ? <Text testID="kontor-new-lead-model-tag" style={{ color: colors.foregroundMuted, fontSize: 11 }}>{tag}</Text> : null}
  </View>;
  const toggle = (key: "details") => setOpen(open === key ? null : key);
  // A source from the listed sessions is drawn on the shared row; a host-wide search result keeps its title and state.
  const listed = source ? sources?.find((entry) => entry.id === source.entry.id) ?? null : null;
  const clearSource = () => { setSource(null); setSourceProblem(null); set({ sourceAgentId: undefined }); };
  const line = { flexDirection: "row", alignItems: "center", gap: 8, minHeight: tall ? 44 : 32 } as const;
  const value = { flexShrink: 1, color: colors.foreground, fontSize: 12 } as const;
  const deskName = choice.desk?.project ?? null;
  const deskRule = choice.issue && choice.desk ? `Desk: ${choice.desk.project}, because ${choice.issue.key} is ${choice.issue.epic ? "an epic" : `a ${choice.issue.type} without an active parent epic`}.`
    : "Desk: follows the issue. An epic starts on DESK • ADAM; another open issue without an active parent epic starts on DESK • TICKETS.";
  // One desk is a fact, several a compact choice; From Jira takes the issue's desk (board B1). A chosen desk that vanished
  // makes even one remaining desk a choice, so "Choose a listed desk" can be done; once a choice it stays one in this
  // form. Its desks sit side by side across the form, as Role and Start do, with Refresh on the Desk line, and are named
  // without the "DESK •" that line already says.
  if (ready.desks.length > 1 || !choice.desk && ready.desks.length) deskChoosable.current = true;
  const refreshDesks = <IconButton label={choice.desk || fromJira ? "Refresh choices" : "Refresh desks"} icon="RefreshCw" colors={colors} onPress={() => refresh({})} />;
  const deskChoice = !fromJira && deskChoosable.current && ready.desks.length > 0;
  const desk = fromJira ? <View style={line}>
      <Text style={{ ...heading, flex: 1 }}>Desk</Text>
      <Text testID="kontor-new-session-desk" accessibilityLabel={deskRule} numberOfLines={1} style={value}>{choice.issue && deskName ? deskName : "Follows the issue"}</Text>
    </View>
    : deskChoice ? <View testID="kontor-new-lead-desk" style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}><Text style={{ ...heading, flex: 1 }}>Desk</Text>{refreshDesks}</View>
      <Choices bare label="Desk" layout="segmented" colors={colors} tall={tall} selected={choice.desk?.projectId ?? null} onSelect={(projectId) => { set({ projectId }); refresh({ projectId }); }}
        options={ready.desks.map((entry) => ({ id: entry.projectId, label: deskShortName(entry.project) }))} />
    </View>
    : <View testID="kontor-new-lead-desk" style={line}>
      <Text style={{ ...heading, flex: 1 }}>Desk</Text>
      <Text numberOfLines={1} style={value}>{deskName ?? "No desk on this host has lead workspaces."}</Text>
    </View>;
  const binding = `${fromJira ? `The lead is bound to ${choice.issue?.key ?? "the chosen issue"}.` : "The lead starts ad hoc; when an epic key is confirmed it is promoted in place if no epic lead exists, otherwise it hands off to that exact existing lead."}${choice.workspaceMissing ? ` The ${choice.workspaceName} workspace is created on this desk.` : ""}`;
  return wrap(<>
    {settingsLine ? <Text testID="kontor-new-lead-settings" style={muted}>{settingsLine}</Text> : null}
    <Choices label="Role" layout="segmented" caption={ROLE_NAMES[choice.role]} colors={colors} tall={tall} selected={choice.role} onSelect={(role) => {
      const nextStart: CreateStart = role === "TPM" && choice.start === AD_HOC ? FROM_JIRA : choice.start;
      set({ role: role as CreateRole, start: nextStart }); refresh({ role: role as CreateRole, start: nextStart }); }}
      options={[{ id: "LSA", label: "LSA", note: ROLE_NAMES.LSA }, { id: "TPM", label: "TPM", note: ROLE_NAMES.TPM }]} />
    {deskChoice ? desk : <View style={{ flexDirection: "row", alignItems: "flex-start", gap: 6 }}>
      <View style={{ flex: 1, minWidth: 0 }}>{desk}</View>
      {refreshDesks}
    </View>}
    <Choices label="Start" layout="segmented" caption="TPM is key-bound: it starts from a Jira ticket." captionShown={choice.role === "TPM"} colors={colors} tall={tall} selected={choice.start}
      onSelect={(start) => { set({ start: start as CreateStart }); refresh({ start: start as CreateStart }); if (start === FROM_JIRA) setPicker("jira"); }}
      options={[
        { id: AD_HOC, label: "Ad hoc", disabled: choice.role === "TPM", note: choice.role === "TPM" ? "TPM is key-bound; choose From Jira." : null },
        { id: FROM_JIRA, label: "From Jira", note: "The issue chooses the desk." },
      ]} />
    {fromJira ? <View style={{ gap: 4 }}>
      <Text style={heading}>Jira issue</Text>
      {choice.issue ? <Text selectable numberOfLines={2} style={{ color: colors.foreground, fontSize: 13 }}>{choice.issue.key} · {choice.issue.summary}</Text> : null}
      {choice.issue ? <Text style={muted}>{choice.issue.type} · {choice.issue.status}</Text> : null}
      <Action ref={lastPicker.current === "jira" ? pickerReturn : undefined} label={choice.issue ? "Change Jira issue" : "Choose Jira issue"} colors={colors} onPress={() => setPicker("jira")} />
      {ready.jiraProblem ? <Text testID="kontor-new-lsa-jira" style={{ color: colors.statusWarning, fontSize: 12 }}>{ready.jiraProblem}</Text> : null}
      {choice.issue ? <View style={{ gap: 4 }}>
        <Text style={heading}>Topic</Text>
        <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <Text style={{ color: colors.foreground, fontSize: 13 }}>{leadTitle(choice.role, choice.issue.key, "").trimEnd()}</Text>
          <TextInput testID="kontor-new-session-jira-topic" accessibilityLabel={`Title topic after ${choice.role} • ${choice.issue.key}`} maxLength={TOPIC_MAX}
            value={draft.jiraTopic ?? choice.issueSummaryTopic} onChangeText={(jiraTopic) => set({ jiraTopic })} onSubmitEditing={() => { if (!problem) submit(); }} returnKeyType="go"
            placeholderTextColor={colors.foregroundMuted} style={{ ...input, flex: 1, minWidth: 0 }} />
        </View>
        <Text style={muted}>Starts as the issue summary. Editing it keeps {choice.issue.key}.</Text>
      </View> : null}
    </View> : <View style={{ gap: 4 }}>
      <Text style={heading}>Topic</Text>
      <TextInput testID="kontor-new-lsa-topic" accessibilityLabel="Topic" placeholder="What this lead is about" autoFocus={!compact} maxLength={TOPIC_MAX} value={draft.topic}
        onChangeText={(topic) => set({ topic })} onSubmitEditing={() => { if (!problem) submit(); }} returnKeyType="go"
        placeholderTextColor={colors.foregroundMuted} style={input} />
    </View>}
    <View style={{ gap: 4 }}>
      <View style={{ flexDirection: "row", alignItems: "baseline", gap: 6 }}>
        <Text style={{ ...heading, flex: 1 }}>Source session</Text>
        <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>Optional</Text>
      </View>
      {listed ? <DenseSessionRow testID="kontor-source-selected" entry={listed} colors={colors} clock={clock} opened={opened?.get(listed.id) ?? null}
        mode={{ kind: "open", actions: <IconButton label="Clear source" icon="X" colors={colors} onPress={clearSource} /> }} onPress={() => setPicker("source")} />
        : source ? <View style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
          <View style={{ flex: 1, minWidth: 0 }}>
            <Text selectable testID="kontor-source-selected" numberOfLines={1} style={{ color: colors.foreground, fontSize: 13 }}>{source.entry.title}</Text>
            <Text selectable numberOfLines={1} style={muted}>{sourceState(source.entry)} · {source.entry.id}</Text>
          </View>
          <Action ref={lastPicker.current === "source" ? pickerReturn : undefined} small label="Change source" colors={colors} onPress={() => setPicker("source")} />
          <IconButton label="Clear source" icon="X" colors={colors} onPress={clearSource} />
        </View>
        : <Action ref={lastPicker.current === "source" ? pickerReturn : undefined} label="Choose source session" colors={colors} onPress={() => setPicker("source")} />}
      {source?.summary ? <View testID="kontor-source-summary" style={{ flexDirection: "row", alignItems: "center", gap: 6 }}>
        <Icon name="FileText" size={14} color={readableInk(colors.accent, colors)} />
        <Text style={{ color: readableInk(colors.accent, colors, 4.5), fontSize: 12 }}>Saved summary r{source.summary.revision}</Text>
      </View> : source ? <Text style={muted}>No saved summary: the lead copies this session's last assistant message before its first task.</Text> : null}
    </View>
    <View style={{ gap: 4 }}>
      <Text style={heading}>Lead title preview</Text>
      <Text selectable testID="kontor-new-lsa-title" style={{ color: choice.title ? colors.foreground : colors.foregroundMuted, fontSize: 13, paddingHorizontal: 9, paddingVertical: 8, borderRadius: 6, backgroundColor: colors.surface2 }}>
        {choice.title || (fromJira ? "Choose a Jira issue to see the title." : "Type a topic to see the title.")}</Text>
    </View>
    {choice.mode?.dangerous ? <Text testID="kontor-new-lsa-danger" style={{ color: colors.statusWarning, fontSize: 12 }}>⚠ {choice.mode.label} is a dangerous permission mode on this host.</Text> : null}
    {/* How the lead is bound and how its persona arrives stay one tap away; the behavior is the same either way. */}
    <Disclosure testID="kontor-new-lead-details" label="Details" value="How this lead starts" open={open === "details"} onToggle={() => toggle("details")} colors={colors} tall={tall}>
      <View testID="kontor-new-lead-details-body" style={{ gap: 4 }}>
        <Text style={muted}>{binding}</Text>
        {fromJira ? <Text style={muted}>{deskRule}</Text> : null}
        {/* A provider that cannot start promises no persona delivery; the problem line names it and why. */}
        {choice.provider?.available ? <Text style={muted}>{personaLine(choice.provider.persona, providerLabel)}</Text> : null}
        {ready.notes.map((note) => <Text key={note} style={muted}>{note}</Text>)}
      </View>
    </Disclosure>
    {/* Notes that arrive with a later read sit below the fields, so they never push them. */}
    {choice.defaultNote ? <Text testID="kontor-new-lead-default-note" style={muted}>{choice.defaultNote}</Text> : null}
    {settings.state.status === "error" || settings.state.status === "invalid" ? <Text style={muted}>The saved new-lead default could not be read ({settings.state.error}). An unchosen model follows the Codex default.</Text> : null}
    {defaultProblem ? <Text testID="kontor-new-lead-default-problem" style={muted}>{defaultProblem}</Text> : null}
    {ready.providers.some((provider) => provider.status === "loading") ? <Text style={muted}>Some providers are still loading.</Text> : null}
    {ready.providers.some((provider) => provider.status === "unknown") ? <Text style={muted}>Some provider availability is unknown.</Text> : null}
    {ready.providers.some((provider) => provider.status === "error") ? <Text style={muted}>Some providers could not be read.</Text> : null}
    {ready.providers.some((provider) => provider.status === "unavailable") ? <Text style={muted}>Some providers are unavailable.</Text> : null}
    {/* The hint keeps one line even when empty, so a one-line hint arriving or clearing never moves the footer. */}
    <View style={{ minHeight: 17 }}>
      {hint ? <Text testID="kontor-new-lsa-problem" accessibilityLiveRegion="polite" style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>{hint}</Text> : null}
    </View>
  </>, <>
      <Action small label="Cancel" colors={colors} onPress={onCancel} />
      <Pressable testID="kontor-new-lsa-create" accessibilityRole="button" accessibilityLabel={`Create ${choice.title || "lead"}`} accessibilityHint={problem ?? undefined} accessibilityState={{ disabled: !!problem }} aria-disabled={!!problem}
        disabled={!!problem} onPress={submit} style={{ minHeight: tall ? 44 : 30, paddingHorizontal: 12, borderRadius: 6, justifyContent: "center", backgroundColor: problem ? colors.surface2 : colors.accent, opacity: problem ? 0.6 : 1 }}>
        <Text style={{ color: problem ? colors.foregroundMuted : colors.accentForeground, fontSize: 12, fontWeight: "600" }}>Create lead</Text>
      </Pressable>
  </>, chip, source?.summary ? "Source summary is attached before the first task." : null);
}
