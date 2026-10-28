import { createElement, isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { CreateFeature, CreateOptions, CreateOutcome, CreateProvider } from "../shared/session-create";
import { DEFAULT_ORGANIZATION_SETTINGS, type OrganizationSettings } from "../shared/organization";
import { UNSUPPORTED, type SummaryRevision } from "../shared/session-navigator";

const model = vi.hoisted(() => ({
  slots: [] as unknown[], cursor: 0, effect: 0, deps: [] as (readonly unknown[] | undefined)[],
  cleanups: [] as (void | (() => void))[],
  rpc: {} as Record<string, ReturnType<typeof vi.fn>>,
  settings: null as unknown as { status: string; values?: object; revision?: string; error?: string; save: ReturnType<typeof vi.fn> },
  readSource: null as unknown as Mock<(...args: unknown[]) => Promise<unknown>>,
  windowHeight: 844,
  useState<T>(initial: T | (() => T)): [T, (next: T | ((previous: T) => T)) => void] {
    const index = this.cursor++;
    if (!(index in this.slots)) this.slots[index] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [this.slots[index] as T, (next) => { this.slots[index] = typeof next === "function" ? (next as (previous: T) => T)(this.slots[index] as T) : next; }];
  },
  useEffect(effect: () => void | (() => void), deps?: readonly unknown[]) {
    const index = this.effect++, previous = this.deps[index];
    if (previous && deps && deps.length === previous.length && deps.every((value, at) => Object.is(value, previous[at]))) return;
    this.cleanups[index]?.();
    this.deps[index] = deps;
    this.cleanups[index] = effect();
  },
  unmount() { this.cleanups.forEach((cleanup) => cleanup?.()); this.cleanups = []; },
}));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  useState: <T,>(initial: T | (() => T)) => model.useState(initial),
  useRef: <T,>(initial: T) => model.useState(() => ({ current: initial }))[0],
  useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) => model.useEffect(effect, deps),
  useCallback: <T,>(callback: T) => callback }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, useWindowDimensions: () => ({ width: 390, height: model.windowHeight }), ScrollView: "ScrollView", View: "View", Text: "Text", Pressable: "Pressable", Image: "Image" }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => model.rpc[contract.name], useSettings: () => model.settings, getPaseoClient: () => ({}) }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
vi.mock("../client/mobile-filters", () => ({ focusControl: () => {} }));
vi.mock("../client/source-picker", async (original) => ({ ...await original<typeof import("../client/source-picker")>(), readSource: (...args: unknown[]) => model.readSource(...args) }));

import { NewLsaForm, resolveDraft } from "../client/new-lsa-form";
import { ModelPicker, modelListHeight, type PickedModel } from "../client/model-picker";
import { LeadSourcePicker } from "../client/new-lead-source";
import { Action } from "../client/page-controls";
import { sessionEntry } from "./session-fixtures";

type Element = ReactElement<Record<string, unknown>>;
/** The element tree; hook-free helper components are drawn so their controls are visible. */
function elements(value: ReactNode): Element[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(value)) return [];
  // A drawn helper's own output stands for its children, so a closed Disclosure hides what it holds.
  const drawn = typeof value.type === "function" && ["Choices", "Toggle", "Disclosure"].includes(value.type.name) ? (value.type as (props: object) => ReactNode)(value.props) : null;
  return [value, ...(drawn === null ? elements(value.props.children as ReactNode) : []), ...elements(drawn)];
}
const text = (tree: Element[]) => tree.filter((node) => node.type === "Text").map((node) => [node.props.children].flat(Infinity).join("")).join("\n");
const byTestId = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id);
const byLabel = (tree: Element[], label: string) => tree.find((node) => node.props.accessibilityLabel === label);
const press = (node: Element | undefined) => (node!.props.onPress as () => void)();
/** A Disclosure's own line, the button that opens it; the Disclosure element carries the same testID. */
const opener = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id && node.props.accessibilityRole === "button");
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const EFFORTS = [{ id: "high", label: "High" }, { id: "xhigh", label: "XHigh" }];
const CLAUDE: CreateProvider = { id: "claude", label: "Claude", status: "ready", available: true, note: null, persona: "supported", defaultModel: "claude-opus-5-5", defaultMode: "default",
  models: [{ id: "claude-opus-5-5", label: "Claude Opus 5.5", efforts: [{ id: "max", label: "Max" }], defaultEffort: "max" }], modes: [{ id: "default", label: "Default", dangerous: false }] };
const CODEX: CreateProvider = { id: "codex", label: "Codex", status: "ready", available: true, note: null, persona: "supported", defaultModel: "gpt-6-astra", defaultMode: "auto",
  models: [{ id: "gpt-6-astra", label: "GPT-6 Astra", efforts: EFFORTS, defaultEffort: "xhigh" }, { id: "gpt-6.1-sol", label: "GPT-6.1 Sol", efforts: EFFORTS, defaultEffort: "high" }],
  modes: [{ id: "auto", label: "Default permissions", dangerous: false }, { id: "full-access", label: "Full access", dangerous: true }] };
const OPENCODE: CreateProvider = { id: "opencode", label: "OpenCode", status: "ready", available: true, note: null, persona: "unverified", defaultModel: null, defaultMode: null,
  models: [{ id: "deepseek/deepseek-flash", label: "DeepSeek Flash", efforts: [], defaultEffort: null }], modes: [] };
/** Claude is listed first and named by the role profile: neither may stand in for the Codex default. */
const OPTIONS: CreateOptions = {
  enabled: true, disabledReason: null, pending: null, notes: [], catalogProjectId: "prj-adam", issues: [], jiraProblem: null,
  desks: [{ projectId: "prj-adam", project: "DESK • ADAM", desk: "ADAM", kind: "adam", cwd: "/fixtures/adam", lead: { ADHOC: "wks-adhoc", LSA: "wks-lsa", TPM: "wks-tpm" } }],
  providers: [CLAUDE, CODEX, OPENCODE],
  profile: { name: "lsa", provider: "claude", model: "claude-opus-5-5", modeId: "default", effort: "max" },
};
const FEATURES: CreateFeature[] = [
  { type: "select", id: "service_tier", label: "Speed", value: null, options: [{ id: "default", label: "Normal", isDefault: true }, { id: "priority", label: "Fast", isDefault: false }] },
  { type: "toggle", id: "plan_mode", label: "Plan", value: false },
];
const SUMMARY: SummaryRevision = { summaryId: "00000000-0000-4000-8000-0000000000aa", revision: 3, sourceAgentId: "agent-src", sourceTitle: "Route review", cutoff: "2026-10-25T10:00:00Z",
  savedAt: "2026-10-25T10:05:00Z", sections: { task: "Fictional task", decisions: "", currentState: "", nextStep: "" }, text: "Fictional summary" };
const ready = (newLeadDefault?: OrganizationSettings["newLeadDefault"]) => ({ status: "ready", values: { ...DEFAULT_ORGANIZATION_SETTINGS, ...(newLeadDefault !== undefined ? { newLeadDefault } : {}) }, revision: "rev-1", save: vi.fn(async () => true) });
const reply = async (input: { requestId: string }): Promise<CreateOutcome> => ({ requestId: input.requestId, state: "verified", title: "LSA • ADHOC • Route review", agentId: "agent-new",
  workspaceId: "wks-adhoc", message: "Fixture verified.", differences: [], persona: "supported", checkedAt: "2026-10-26" });

beforeEach(() => {
  model.unmount();
  model.slots = []; model.cursor = 0; model.effect = 0; model.deps = []; model.windowHeight = 844;
  model.settings = ready();
  model.readSource = vi.fn(async (_client: unknown, id: unknown) => ({ entry: sessionEntry(String(id), { title: "Route review" }), contextProblem: null }));
  model.rpc = {
    "sessions.create": vi.fn(reply), "sessions.create-check": vi.fn(),
    "sessions.create-features": vi.fn(async (input: { provider: string }) => ({ features: input.provider === "codex" ? FEATURES : [], problem: null })),
    "sessions.navigator-capabilities": vi.fn(async () => UNSUPPORTED),
    "sessions.summary-latest": vi.fn(async (input: { agentIds: string[] }) => Object.fromEntries(input.agentIds.map((id) => [id, { ...SUMMARY, sourceAgentId: id }]))),
  };
});

/** Renders one component on the shared hook slots, as React would for a mounted instance. */
function mount<P extends object>(component: (props: P) => ReactNode, props: P) {
  let tree: Element[] = [];
  const render = () => { model.cursor = 0; model.effect = 0; tree = elements(component(props)); return tree; };
  render();
  return { render, props, get tree() { return tree; } };
}
const formProps = (patch: Partial<Parameters<typeof NewLsaForm>[0]> = {}) => ({ hostId: "host-a", colors: COLORS, compact: true, options: { status: "ready", options: OPTIONS } as const,
  onRefresh: vi.fn(), canOpen: true, onOpenCreated: vi.fn(() => ({ ok: true as const })), onCancel: vi.fn(), ...patch });
const pickerProps = (patch: Partial<Parameters<typeof ModelPicker>[0]> = {}): Parameters<typeof ModelPicker>[0] => ({ colors: COLORS, compact: true, providers: OPTIONS.providers, projectId: "prj-adam",
  value: { provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: "auto", featureValues: {} }, codex: { provider: "codex", model: "gpt-6-astra", effort: "xhigh" },
  onUse: vi.fn(), onCancel: vi.fn(), ...patch });
/** The labels of the footer's Cancel and primary action, in reading order. */
function footer(tree: Element[], primary: string): string[] {
  const row = tree.find((node) => node.type === "View" && (node.props.style as { justifyContent?: string } | undefined)?.justifyContent === "space-between"
    && elements(node.props.children as ReactNode).some((child) => child.props.testID === primary))!;
  return elements(row.props.children as ReactNode).filter((node) => node.type === Action || node.props.testID === primary).map((node) => String(node.props.label ?? node.props.testID));
}

describe("New lead default", () => {
  it("starts on Codex's catalog default at its default effort, never the role profile or the first provider", () => {
    expect(resolveDraft(OPTIONS, { topic: "Route review" })).toMatchObject({ provider: { id: "codex" }, model: { id: "gpt-6-astra" }, effort: { id: "xhigh" }, mode: { id: "auto" },
      featureValues: {}, problem: null, defaultNote: null, codex: { provider: "codex", model: "gpt-6-astra", effort: "xhigh" } });
  });

  it("starts on a saved new-lead default with its settings while this host offers it", () => {
    const saved = { provider: "codex", model: "gpt-6.1-sol", effort: "xhigh", featureValues: { service_tier: "priority" } };
    expect(resolveDraft(OPTIONS, { topic: "Route review" }, saved)).toMatchObject({ model: { id: "gpt-6.1-sol" }, effort: { id: "xhigh" }, featureValues: { service_tier: "priority" }, problem: null, lead: { saved: true } });
  });

  it("falls back to the Codex default and names a saved default this host no longer offers", () => {
    const draft = resolveDraft(OPTIONS, { topic: "Route review" }, { provider: "codex", model: "gpt-5-retired", effort: "high" });
    expect(draft).toMatchObject({ model: { id: "gpt-6-astra" }, effort: { id: "xhigh" }, featureValues: {}, problem: null });
    expect(draft.defaultNote).toBe("The saved new-lead default gpt-5-retired is not offered here now, so this lead starts on the Codex default.");
  });

  it.each([
    ["flags no default", [CLAUDE, { ...CODEX, defaultModel: null }], "Codex flags no default model here"],
    ["is not listed", [CLAUDE], "this host lists no Codex provider"],
    ["is off", [CLAUDE, { ...CODEX, available: false, status: "disabled", note: "Disabled on this host." }], "Codex: Disabled on this host"],
  ] as const)("asks for an explicit model when Codex %s", (_, providers, why) => {
    const draft = resolveDraft({ ...OPTIONS, providers: [...providers] }, { topic: "Route review" });
    expect(draft).toMatchObject({ provider: null, model: null, problem: `Codex default unknown: ${why}. Choose a model for this lead.` });
  });

  it("never takes a provider's first model as its default", () => {
    expect(resolveDraft(OPTIONS, { topic: "Route review", provider: "opencode" })).toMatchObject({ model: null, problem: "OpenCode flags no default model. Choose a model for this lead." });
  });
});

describe("New lead form", () => {
  it("shows the model chip, the permissions, Plan and route row, and Cancel left of Create lead", async () => {
    const f = mount(NewLsaForm, formProps());
    await settle(); f.render();
    const chip = byTestId(f.tree, "kontor-new-lead-model")!;
    expect(text(elements(chip.props.children as ReactNode))).toBe("A6 ▾\nXHigh · Normal");
    expect(byTestId(f.tree, "kontor-new-lead-model-tag")?.props.children).toBe("Codex default");
    expect(chip.props.accessibilityLabel).toBe("Model: GPT-6 Astra, gpt-6-astra, XHigh · Normal, Codex default. Change model");
    expect(byTestId(f.tree, "kontor-new-lead-settings")?.props.children).toBe("Default permissions | Plan off | via Codex");
    expect(footer(f.tree, "kontor-new-lsa-create")).toEqual(["Cancel", "kontor-new-lsa-create"]);
    expect(text(f.tree)).toContain("Create lead");
    expect(text(f.tree)).not.toMatch(/New session|Create session/);
  });

  it("sets the model chip at the right of the surface's heading, outside the scrolling form (board B1)", async () => {
    const f = mount(NewLsaForm, { ...formProps(), header: createElement("Text", { testID: "lead-heading" }, "New lead") });
    await settle(); f.render();
    const row = f.tree.find((node) => { const first: unknown = [node.props.children].flat()[0]; return isValidElement<{ testID?: string }>(first) && first.props.testID === "lead-heading"; })!;
    expect(byTestId(elements([row.props.children].flat()[1] as ReactNode), "kontor-new-lead-model")).toBeDefined();
    expect(byTestId(elements(byTestId(f.tree, "kontor-new-lsa")!.props.children as ReactNode), "kontor-new-lead-model")).toBeUndefined();
  });

  it("orders Role, Desk, Start, Topic, Source and the title preview, one line each, with persona and binding under Details (board B1)", async () => {
    const f = mount(NewLsaForm, formProps());
    await settle(); f.render();
    const at = (match: (node: Element) => boolean) => f.tree.findIndex(match);
    const order = [
      at((node) => node.props.accessibilityLabel === "Role" && node.props.accessibilityRole === "radiogroup"),
      at((node) => node.props.testID === "kontor-new-lead-desk"),
      at((node) => node.props.accessibilityLabel === "Start" && node.props.accessibilityRole === "radiogroup"),
      at((node) => node.props.testID === "kontor-new-lsa-topic"),
      at((node) => node.type === Action && node.props.label === "Choose source session"),
      at((node) => node.props.testID === "kontor-new-lsa-title"),
    ];
    expect(order.every((index) => index >= 0)).toBe(true);
    expect([...order].sort((a, b) => a - b)).toEqual(order);
    expect(byTestId(f.tree, "kontor-choice-caption-Role")?.props.children).toBe("Lead Software Architect");
    expect(byLabel(f.tree, "LSA, Lead Software Architect")?.props["aria-checked"]).toBe(true);
    // One desk is a fact, not a choice; Refresh stays one compact control.
    expect(text(elements(byTestId(f.tree, "kontor-new-lead-desk")!.props.children as ReactNode))).toBe("Desk\nDESK • ADAM");
    expect(f.tree.filter((node) => node.props.label === "Refresh choices")).toHaveLength(1);
    expect(text(f.tree)).not.toMatch(/Persona:|stays ad hoc/);
    press(opener(f.tree, "kontor-new-lead-details")); f.render();
    expect(text(f.tree)).toContain("Persona: sent as the system prompt");
    expect(text(f.tree)).toContain("The lead starts ad hoc");
    // The key-bound note under Start keeps its space for LSA, unseen and unspoken, so switching roles moves nothing below it.
    const startNote = () => byTestId(f.tree, "kontor-choice-caption-Start")!.props as { children: string; "aria-hidden": boolean; style: { opacity: number } };
    expect(startNote()).toMatchObject({ children: "TPM is key-bound: it starts from a Jira ticket.", "aria-hidden": true, style: { opacity: 0 } });
    press(byLabel(f.tree, "TPM, Technical Program Manager")); f.render();
    expect(byTestId(f.tree, "kontor-choice-caption-Role")?.props.children).toBe("Technical Program Manager");
    expect(startNote()).toMatchObject({ children: "TPM is key-bound: it starts from a Jira ticket.", "aria-hidden": false, style: { opacity: 1 } });
  });

  it("offers several desks side by side under Desk, in order and short, chosen in one press", async () => {
    const desk = (projectId: string, name: string, kind: "pr-review" | "tickets") => ({ ...OPTIONS.desks[0]!, projectId, project: `DESK • ${name}`, desk: name, kind });
    const desks = [...OPTIONS.desks, desk("prj-pr", "PR REVIEW", "pr-review"), desk("prj-tickets", "TICKETS", "tickets")];
    const f = mount(NewLsaForm, formProps({ options: { status: "ready", options: { ...OPTIONS, desks } } }));
    const group = () => f.tree.find((node) => node.props.accessibilityRole === "radiogroup" && node.props.accessibilityLabel === "Desk");
    const radios = () => f.tree.filter((node) => node.props.accessibilityRole === "radio" && ["ADAM", "PR REVIEW", "TICKETS"].includes(node.props.accessibilityLabel as string))
      .map((node) => [node.props.accessibilityLabel, node.props["aria-checked"]]);
    expect(group()).toBeDefined();
    expect(opener(f.tree, "kontor-new-lead-desk")).toBeUndefined();
    expect(f.tree.some((node) => node.props["aria-expanded"] !== undefined && node.props.testID === "kontor-new-lead-desk")).toBe(false);
    expect(radios()).toEqual([["ADAM", true], ["PR REVIEW", false], ["TICKETS", false]]);
    press(byLabel(f.tree, "TICKETS")); f.render();
    expect(radios()).toEqual([["ADAM", false], ["PR REVIEW", false], ["TICKETS", true]]);
    expect(f.props.onRefresh).toHaveBeenLastCalledWith(expect.objectContaining({ projectId: "prj-tickets", role: "LSA" }));
  });

  it("keeps the one remaining desk choosable when the chosen desk vanished, as its problem asks", async () => {
    const second = { ...OPTIONS.desks[0]!, projectId: "prj-tickets", project: "DESK • TICKETS", desk: "TICKETS", kind: "tickets" as const };
    const f = mount(NewLsaForm, formProps({ options: { status: "ready", options: { ...OPTIONS, desks: [...OPTIONS.desks, second] } } }));
    press(byLabel(f.tree, "TICKETS")); f.render();
    f.props.options = { status: "ready", options: OPTIONS }; f.render();
    // The remaining desk is offered, never chosen for the operator.
    expect(byLabel(f.tree, "ADAM")?.props["aria-checked"]).toBe(false);
    expect(text(f.tree)).toContain("Choose a listed desk");
    press(byLabel(f.tree, "ADAM")); f.render();
    // Still the choice it became.
    expect(byLabel(f.tree, "ADAM")?.props).toMatchObject({ accessibilityRole: "radio", "aria-checked": true });
  });

  it("never reads a vanished desk again: Refresh desks asks for the desks that remain", async () => {
    const second = { ...OPTIONS.desks[0]!, projectId: "prj-tickets", project: "DESK • TICKETS", desk: "TICKETS", kind: "tickets" as const };
    const f = mount(NewLsaForm, formProps({ options: { status: "ready", options: { ...OPTIONS, desks: [...OPTIONS.desks, second] } } }));
    // The opening desk, DESK • ADAM, is gone on the next read, and so is its catalog.
    f.props.options = { status: "ready", options: { ...OPTIONS, desks: [second], catalogProjectId: null } }; f.render();
    press(f.tree.find((node) => node.props.label === "Refresh desks")!);
    expect(f.props.onRefresh).toHaveBeenLastCalledWith(expect.objectContaining({ projectId: undefined, role: "LSA" }));
  });

  it("carries the picker's Speed and Plan into the request and onto the chip", async () => {
    const f = mount(NewLsaForm, formProps());
    press(byTestId(f.tree, "kontor-new-lead-model")); f.render();
    const picker = f.tree[0]!;
    expect(picker.type).toBe(ModelPicker);
    (picker.props.onUse as (choice: PickedModel) => void)({ provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: "auto", featureValues: { service_tier: "priority", plan_mode: true } });
    f.render(); await settle(); f.render();
    (byTestId(f.tree, "kontor-new-lsa-topic")!.props.onChangeText as (value: string) => void)("Route review"); f.render();
    expect(text(elements(byTestId(f.tree, "kontor-new-lead-model")!.props.children as ReactNode))).toContain("XHigh · Fast");
    expect(byTestId(f.tree, "kontor-new-lead-settings")?.props.children).toBe("Default permissions | Plan on | via Codex");
    press(byTestId(f.tree, "kontor-new-lsa-create"));
    expect(model.rpc["sessions.create"]).toHaveBeenCalledWith(expect.objectContaining({ provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: "auto", featureValues: { service_tier: "priority", plan_mode: true } }));
    expect(model.settings.save).not.toHaveBeenCalled();
  });

  it("sends no feature values when none were chosen", () => {
    const f = mount(NewLsaForm, formProps());
    (byTestId(f.tree, "kontor-new-lsa-topic")!.props.onChangeText as (value: string) => void)("Route review"); f.render();
    press(byTestId(f.tree, "kontor-new-lsa-create"));
    expect(model.rpc["sessions.create"]!.mock.calls[0]![0]).not.toHaveProperty("featureValues");
  });

  it("persists Make default and clears it on Reset through the organization settings", () => {
    const f = mount(NewLsaForm, formProps());
    press(byTestId(f.tree, "kontor-new-lead-model")); f.render();
    const defaults = f.tree[0]!.props.defaults as { name: string; make: (choice: PickedModel) => void; reset: () => void };
    expect(defaults.name).toBe("Codex default (GPT-6 Astra · XHigh)");
    defaults.make({ provider: "codex", model: "gpt-6.1-sol", effort: "high", modeId: "auto", featureValues: { service_tier: "priority" } });
    expect(model.settings.save).toHaveBeenLastCalledWith({ ...DEFAULT_ORGANIZATION_SETTINGS, newLeadDefault: { provider: "codex", model: "gpt-6.1-sol", effort: "high", featureValues: { service_tier: "priority" } } }, "rev-1");
    defaults.reset();
    expect(model.settings.save).toHaveBeenLastCalledWith({ ...DEFAULT_ORGANIZATION_SETTINGS, newLeadDefault: null }, "rev-1");
  });

  it("starts on a saved default and says when a saved default is gone", () => {
    model.settings = ready({ provider: "codex", model: "gpt-6.1-sol", effort: "xhigh" });
    const saved = mount(NewLsaForm, formProps()).tree;
    expect(text(elements(byTestId(saved, "kontor-new-lead-model")!.props.children as ReactNode))).toBe("S6.1 ▾\nXHigh");
    expect(byTestId(saved, "kontor-new-lead-model-tag")?.props.children).toBe("New-lead default");
    model.unmount(); model.slots = []; model.deps = [];
    model.settings = ready({ provider: "codex", model: "gpt-5-retired", effort: "xhigh" });
    const f = mount(NewLsaForm, formProps());
    expect(byTestId(f.tree, "kontor-new-lead-default-note")?.props.children).toContain("gpt-5-retired is not offered here now");
    expect(byTestId(f.tree, "kontor-new-lead-model-tag")?.props.children).toBe("Codex default");
  });

  it("blocks Create until a model is chosen when the Codex default is unknown", () => {
    const f = mount(NewLsaForm, formProps({ options: { status: "ready", options: { ...OPTIONS, providers: [CLAUDE, { ...CODEX, defaultModel: null }] } } }));
    (byTestId(f.tree, "kontor-new-lsa-topic")!.props.onChangeText as (value: string) => void)("Route review"); f.render();
    expect(byTestId(f.tree, "kontor-new-lead-model-tag")?.props.children).toBe("Codex default unknown");
    expect(byTestId(f.tree, "kontor-new-lsa-create")?.props.disabled).toBe(true);
    expect(byTestId(f.tree, "kontor-new-lsa-problem")?.props.children).toBe("Codex default unknown: Codex flags no default model here. Choose a model for this lead.");
  });

  it("sends the source session alone, never a pin, when this host has no saved summaries", async () => {
    const source = sessionEntry("agent-src", { title: "Route review" });
    const f = mount(NewLsaForm, formProps({ sources: [source] }));
    press(f.tree.find((node) => node.type === Action && node.props.label === "Choose source session")); f.render();
    expect(f.tree[0]!.type).toBe(LeadSourcePicker);
    (f.tree[0]!.props.onUse as (choice: { entry: typeof source; summary: null }) => void)({ entry: source, summary: null }); f.render();
    (byTestId(f.tree, "kontor-new-lsa-topic")!.props.onChangeText as (value: string) => void)("Route review"); f.render();
    expect(text(f.tree)).not.toContain("Source summary is attached");
    press(byTestId(f.tree, "kontor-new-lsa-create")); await settle();
    const request = model.rpc["sessions.create"]!.mock.calls[0]![0] as Record<string, unknown>;
    expect(request.sourceAgentId).toBe("agent-src");
    expect(request).not.toHaveProperty("sourceSummary");
  });

  it("pins a saved summary revision and says it is attached before the first task", async () => {
    const source = sessionEntry("agent-src", { title: "Route review" });
    const f = mount(NewLsaForm, formProps({ sources: [source] }));
    press(f.tree.find((node) => node.type === Action && node.props.label === "Choose source session")); f.render();
    (f.tree[0]!.props.onUse as (choice: { entry: typeof source; summary: SummaryRevision }) => void)({ entry: source, summary: SUMMARY }); f.render();
    (byTestId(f.tree, "kontor-new-lsa-topic")!.props.onChangeText as (value: string) => void)("Route review"); f.render();
    expect(text(f.tree)).toContain("Saved summary r3");
    expect(text(f.tree)).toContain("Source summary is attached before the first task.");
    press(byTestId(f.tree, "kontor-new-lsa-create")); await settle();
    expect(model.readSource).not.toHaveBeenCalled();
    expect(model.rpc["sessions.create"]).toHaveBeenCalledWith(expect.objectContaining({ sourceAgentId: null, sourceSummary: { summaryId: SUMMARY.summaryId, revision: 3 } }));
  });
});

describe("Model & settings", () => {
  it("leads with the choice it opened with and keeps that order while picking (board B2)", () => {
    const rows = (p: { tree: Element[] }) => p.tree.filter((node) => String(node.props.testID ?? "").startsWith("kontor-model-row-")).map((node) => String(node.props.testID).slice(17));
    const vendors = (p: { tree: Element[] }) => p.tree.filter((node) => String(node.props.testID ?? "").startsWith("kontor-model-vendor-")).map((node) => String(node.props.testID).slice(20));
    const p = mount(ModelPicker, pickerProps());
    expect(vendors(p)).toEqual(["openai", "anthropic", "deepseek"]);
    expect(rows(p)[0]).toBe("codex/gpt-6-astra");
    const order = rows(p);
    press(byTestId(p.tree, "kontor-model-vendor-anthropic")); p.render();
    press(byTestId(p.tree, "kontor-model-row-claude/claude-opus-5-5")); p.render();
    expect(vendors(p)).toEqual(["openai", "anthropic", "deepseek"]);
    press(byTestId(p.tree, "kontor-model-vendor-openai")); p.render();
    expect(rows(p)).toEqual(order);
  });

  it("ends a long list on a row's lower edge and says it scrolls; a short list keeps its height", () => {
    const rows = (count: number) => ({ rows: Array.from({ length: count }) });
    // One heading (17), then rows of 52 with 2 between: 71, 125, 179, 233; a fifth would end at 287, past the bound.
    expect(modelListHeight([rows(57)], 52)).toEqual({ height: 233, scrolls: true });
    // A second provider adds its gap and heading first: 63, 109, then 8+17 and rows to 180, 226; the next would end at 272.
    expect(modelListHeight([rows(2), rows(9)], 44)).toEqual({ height: 226, scrolls: true });
    // Rows that all fit still get their whole height, so the column never squeezes the list through a row.
    expect(modelListHeight([rows(2), rows(1)], 52)).toEqual({ height: 204, scrolls: false });
    // Auto leads: its row and the gap come first, 52 + 8 + 17, then rows to 131, 185, 239.
    expect(modelListHeight([rows(57)], 52, 264, 52)).toEqual({ height: 239, scrolls: true });
    const p = mount(ModelPicker, pickerProps({ providers: [{ ...OPTIONS.providers[1]!, models: Array.from({ length: 9 }, (_, index) => ({ id: `gpt-6.${index}-sol`, label: `Sol ${index}`, efforts: [], defaultEffort: null })) }] }));
    expect(byTestId(p.tree, "kontor-model-list")?.props.style).toMatchObject({ flexShrink: 0, height: 233 });
    expect(byTestId(p.tree, "kontor-model-more")?.props.children).toBe("9 models · scroll the list for more");
    const short = mount(ModelPicker, pickerProps({ providers: [{ ...OPTIONS.providers[1]!, models: OPTIONS.providers[1]!.models.slice(0, 2) }] }));
    expect(byTestId(short.tree, "kontor-model-list")?.props.style).toMatchObject({ flexShrink: 0, height: 125 });
    expect(byTestId(short.tree, "kontor-model-more")).toBeUndefined();
  });

  it("gives up whole rows while the settings are cramped, and never takes them back at the same window height", () => {
    const p = mount(ModelPicker, pickerProps({ providers: [{ ...OPTIONS.providers[1]!, models: Array.from({ length: 9 }, (_, index) => ({ id: `gpt-6.${index}-sol`, label: `Sol ${index}`, efforts: [], defaultEffort: null })) }] }));
    const height = () => (byTestId(p.tree, "kontor-model-list")!.props.style as { height: number }).height;
    const settings = () => byTestId(p.tree, "kontor-model-settings")!.props as { onLayout(event: { nativeEvent: { layout: { height: number } } }): void; onContentSizeChange(width: number, height: number): void };
    // Settings with little content that all shows are not cramped, however short.
    settings().onContentSizeChange(390, 60); settings().onLayout({ nativeEvent: { layout: { height: 60 } } }); p.render();
    expect(height()).toBe(233);
    // Cramped to 40 of the 88 they need: the list drops 48 to the next row edge below, 179.
    settings().onContentSizeChange(390, 300); settings().onLayout({ nativeEvent: { layout: { height: 40 } } }); p.render();
    expect(height()).toBe(179);
    settings().onLayout({ nativeEvent: { layout: { height: 300 } } }); p.render();
    expect(height()).toBe(179);
  });

  it("shows every effort level first and open, with no visible Effort label; Permissions stays one line until opened, with Speed and Plan inline (board B2)", async () => {
    const p = mount(ModelPicker, pickerProps());
    await settle(); p.render();
    const levels = () => elements(byLabel(p.tree, "Effort")!.props.children as ReactNode).filter((node) => node.props.accessibilityRole === "radio")
      .map((node) => `${String(node.props.accessibilityLabel)}${node.props["aria-checked"] ? " ✓" : ""}`);
    expect(byLabel(p.tree, "Effort")?.props.accessibilityRole).toBe("radiogroup");
    expect(levels()).toEqual(["High", "XHigh ✓"]);
    expect(byTestId(p.tree, "kontor-model-effort")).toBeUndefined();
    expect(text(p.tree).split("\n")).not.toContain("Effort");
    expect(opener(p.tree, "kontor-model-permissions")?.props).toMatchObject({ accessibilityLabel: "Permissions: Default permissions", "aria-expanded": false });
    expect(byLabel(p.tree, "Speed")?.props.accessibilityRole).toBe("radiogroup");
    expect(byLabel(p.tree, "Fast")).toBeDefined();
    press(byLabel(p.tree, "High")); p.render();
    expect(levels()).toEqual(["High ✓", "XHigh"]);
    press(opener(p.tree, "kontor-model-permissions")); p.render();
    expect(levels()).toEqual(["High ✓", "XHigh"]);
    expect(byLabel(p.tree, "Full access ⚠")).toBeDefined();
    const order = p.tree.filter((node) => (node.props.accessibilityRole === "radiogroup" && ["Effort", "Speed"].includes(String(node.props.accessibilityLabel)))
      || (node.props.accessibilityRole === "button" && node.props.testID === "kontor-model-permissions") || node.props.accessibilityRole === "switch")
      .map((node) => String(node.props.testID ?? node.props.accessibilityLabel));
    expect(order).toEqual(["Effort", "Speed", "kontor-model-permissions", "Plan"]);
  });

  it("keeps the catalog's order when it opens with no choice", () => {
    const p = mount(ModelPicker, pickerProps({ value: null }));
    expect(p.tree.find((node) => String(node.props.testID ?? "").startsWith("kontor-model-row-"))?.props.testID).toBe("kontor-model-row-claude/claude-opus-5-5");
  });

  it("lists the browsed vendor's models with route, alias and full ID, searches within that vendor, and browsing changes no choice", () => {
    const onUse = vi.fn();
    const p = mount(ModelPicker, pickerProps({ onUse }));
    const listed = () => p.tree.filter((node) => String(node.props.testID ?? "").startsWith("kontor-model-row-")).map((node) => node.props.testID);
    const tile = (vendor: string) => byTestId(p.tree, `kontor-model-vendor-${vendor}`)!;
    expect(tile("openai").props).toMatchObject({ accessibilityRole: "tab", "aria-selected": true, accessibilityLabel: "OpenAI, 2 models, has the selected model" });
    expect(tile("deepseek").props).toMatchObject({ "aria-selected": false, accessibilityLabel: "DeepSeek, 1 model" });
    // OpenCode is a route, never a vendor.
    expect(byTestId(p.tree, "kontor-model-vendor-opencode")).toBeUndefined();
    expect(listed()).toEqual(["kontor-model-row-codex/gpt-6-astra", "kontor-model-row-codex/gpt-6.1-sol"]);
    expect(byTestId(p.tree, "kontor-model-row-codex/gpt-6-astra")?.props.accessibilityLabel).toBe("GPT-6 Astra, gpt-6-astra, via Codex, Codex default");
    // A search narrows within the browsed vendor; tapping another vendor then shows that vendor's matches.
    (byTestId(p.tree, "kontor-model-search")!.props.onChangeText as (value: string) => void)("deepseek/"); p.render();
    expect(listed()).toEqual([]);
    expect(text(p.tree)).toContain("No OpenAI model matches “deepseek/”.");
    press(tile("deepseek")); p.render();
    expect([tile("openai").props["aria-selected"], tile("deepseek").props["aria-selected"]]).toEqual([false, true]);
    expect(listed()).toEqual(["kontor-model-row-opencode/deepseek/deepseek-flash"]);
    const row = byTestId(p.tree, "kontor-model-row-opencode/deepseek/deepseek-flash")!;
    expect(row.props.accessibilityLabel).toBe("DeepSeek Flash, deepseek/deepseek-flash, via OpenCode");
    expect(elements(row.props.children as ReactNode).find((node) => typeof node.type === "function" && node.type.name === "VendorMark")?.props.vendor).toBe("deepseek");
    // Browsing and searching kept the chosen model, its summary and its levels.
    expect(text(elements(byTestId(p.tree, "kontor-model-selected")!.props.children as ReactNode))).toBe("GPT-6 Astra\nvia Codex · A6\nSelected");
    expect(tile("openai").props.accessibilityLabel).toBe("OpenAI, 2 models, has the selected model");
    expect(byTestId(p.tree, "kontor-model-search")?.props).toMatchObject({ accessibilityLabel: "Search DeepSeek models", placeholder: "Search DeepSeek models…" });
    expect(byLabel(p.tree, "XHigh")?.props["aria-checked"]).toBe(true);
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).toHaveBeenCalledExactlyOnceWith({ provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: "auto", featureValues: {} });
  });

  it("labels the unknown-vendor tile Other under its mark and speaks the full name", () => {
    const p = mount(ModelPicker, pickerProps({ providers: [CLAUDE, CODEX, { ...OPENCODE, models: [...OPENCODE.models, { id: "nemo/mystery-1", label: "Mystery 1", efforts: [], defaultEffort: null }] }] }));
    const tile = byTestId(p.tree, "kontor-model-vendor-unknown")!;
    expect(tile.props.accessibilityLabel).toBe("Unknown vendor, 1 model");
    expect(text(elements(tile.props.children as ReactNode))).toBe("Other");
  });

  it("hands Speed, Plan, effort and permissions to Use, with Cancel left of the primary", async () => {
    const onUse = vi.fn();
    const p = mount(ModelPicker, pickerProps({ onUse }));
    await settle(); p.render();
    press(byLabel(p.tree, "Fast")); p.render();
    press(p.tree.find((node) => node.props.accessibilityRole === "switch" && node.props.accessibilityLabel === "Plan")); p.render();
    expect(byLabel(p.tree, "Full access ⚠")).toBeUndefined();
    press(opener(p.tree, "kontor-model-permissions")); p.render();
    press(byLabel(p.tree, "Full access ⚠")); p.render();
    expect(byTestId(p.tree, "kontor-model-danger")).toBeDefined();
    expect(footer(p.tree, "kontor-model-use")).toEqual(["Cancel", "kontor-model-use"]);
    expect(byTestId(p.tree, "kontor-model-use")?.props.accessibilityLabel).toBe("Use A6 · XHigh");
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).toHaveBeenCalledExactlyOnceWith({ provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: "full-access", featureValues: { service_tier: "priority", plan_mode: true } });
  });

  it("keeps Speed, Plan and a non-default effort when the checked model is chosen again, and on another model that lists them", async () => {
    const onUse = vi.fn();
    const p = mount(ModelPicker, pickerProps({ onUse }));
    await settle(); p.render();
    press(byLabel(p.tree, "Fast")); p.render();
    press(p.tree.find((node) => node.props.accessibilityRole === "switch" && node.props.accessibilityLabel === "Plan")); p.render();
    press(byLabel(p.tree, "High")); p.render();
    press(byTestId(p.tree, "kontor-model-row-codex/gpt-6-astra")); p.render(); await settle(); p.render();
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).toHaveBeenLastCalledWith({ provider: "codex", model: "gpt-6-astra", effort: "high", modeId: "auto", featureValues: { service_tier: "priority", plan_mode: true } });
    press(byTestId(p.tree, "kontor-model-row-codex/gpt-6.1-sol")); p.render(); await settle(); p.render();
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).toHaveBeenLastCalledWith({ provider: "codex", model: "gpt-6.1-sol", effort: "high", modeId: "auto", featureValues: { service_tier: "priority", plan_mode: true } });
  });

  it("draws the off Plan switch with a muted border and knob, and the on switch in the accent", async () => {
    const p = mount(ModelPicker, pickerProps());
    await settle(); p.render();
    const track = () => byTestId(p.tree, "kontor-toggle-track-Plan")!;
    const knob = () => (track().props.children as Element).props.style as { backgroundColor: string };
    expect(track().props.style).toMatchObject({ borderWidth: 1, borderColor: "#555", backgroundColor: "#ddd" });
    expect(knob().backgroundColor).toBe("#555");
    press(p.tree.find((node) => node.props.accessibilityRole === "switch" && node.props.accessibilityLabel === "Plan")); p.render();
    expect(track().props.style).toMatchObject({ borderColor: "#00f", backgroundColor: "#00f" });
    expect(knob().backgroundColor).toBe("#fff");
  });

  it("offers Speed and Plan only when the host lists them, and names a failed read", async () => {
    const p = mount(ModelPicker, pickerProps({ value: { provider: "claude", model: "claude-opus-5-5", effort: "max", modeId: "default", featureValues: {} } }));
    await settle(); p.render();
    expect(byLabel(p.tree, "Speed")).toBeUndefined();
    expect(p.tree.some((node) => node.props.accessibilityRole === "switch")).toBe(false);
    model.rpc["sessions.create-features"]!.mockResolvedValue({ features: [], problem: "provider offline" });
    press(byTestId(p.tree, "kontor-model-vendor-openai")); p.render();
    press(byTestId(p.tree, "kontor-model-row-codex/gpt-6.1-sol")); p.render(); await settle(); p.render();
    expect(byTestId(p.tree, "kontor-model-settings-problem")?.props.children).toEqual(["Model settings could not be read: ", "provider offline"]);
    expect(byLabel(p.tree, "Speed")).toBeUndefined();
    expect(p.tree.some((node) => node.props.accessibilityRole === "switch")).toBe(false);
  });

  it("saves a default only when Make default is checked, and Reset returns to the Codex default", () => {
    const defaults = { name: "Codex default (GPT-6 Astra · XHigh)", make: vi.fn(), reset: vi.fn() };
    const onUse = vi.fn();
    const p = mount(ModelPicker, pickerProps({ defaults, onUse }));
    expect(text(p.tree)).toContain("New-lead default: Codex default (GPT-6 Astra · XHigh)");
    press(byTestId(p.tree, "kontor-model-row-codex/gpt-6.1-sol")); p.render();
    press(byTestId(p.tree, "kontor-model-use"));
    // GPT-6.1 Sol offers the XHigh the default runs at, so the effort is kept.
    expect(onUse).toHaveBeenLastCalledWith(expect.objectContaining({ model: "gpt-6.1-sol", effort: "xhigh" }));
    expect(defaults.make).not.toHaveBeenCalled();
    press(byTestId(p.tree, "kontor-model-make-default")); p.render();
    press(byTestId(p.tree, "kontor-model-use"));
    expect(defaults.make).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ provider: "codex", model: "gpt-6.1-sol", effort: "xhigh" }));
    press(byTestId(p.tree, "kontor-model-reset")); p.render();
    expect(defaults.reset).toHaveBeenCalledTimes(1);
    expect(byTestId(p.tree, "kontor-model-row-codex/gpt-6-astra")?.props["aria-checked"]).toBe(true);
    expect(byTestId(p.tree, "kontor-model-make-default")?.props["aria-checked"]).toBe(false);
  });

  it("re-tapping the Codex default model keeps its settings, and Reset on that same model restores the Codex default's", async () => {
    const defaults = { name: "GPT-6.1 Sol · High", make: vi.fn(), reset: vi.fn() };
    const onUse = vi.fn();
    const p = mount(ModelPicker, pickerProps({ defaults, onUse }));
    await settle(); p.render();
    press(byLabel(p.tree, "High")); p.render();
    press(byLabel(p.tree, "Fast")); p.render();
    press(p.tree.find((node) => node.props.accessibilityRole === "switch" && node.props.accessibilityLabel === "Plan")); p.render();
    press(opener(p.tree, "kontor-model-permissions")); p.render();
    press(byLabel(p.tree, "Full access ⚠")); p.render();
    press(byTestId(p.tree, "kontor-model-row-codex/gpt-6-astra")); p.render(); await settle(); p.render();
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).toHaveBeenLastCalledWith({ provider: "codex", model: "gpt-6-astra", effort: "high", modeId: "full-access", featureValues: { service_tier: "priority", plan_mode: true } });
    press(byTestId(p.tree, "kontor-model-reset")); p.render(); await settle(); p.render();
    expect(defaults.reset).toHaveBeenCalledTimes(1);
    expect(["XHigh", "Normal"].map((label) => byLabel(p.tree, label)?.props["aria-checked"])).toEqual([true, true]);
    expect(p.tree.find((node) => node.props.accessibilityRole === "switch" && node.props.accessibilityLabel === "Plan")?.props["aria-checked"]).toBe(false);
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).toHaveBeenLastCalledWith({ provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: "auto", featureValues: {} });
    expect(defaults.make).not.toHaveBeenCalled();
  });

  it("resets to the supplied Codex default's effort and settings, not to the model's own", async () => {
    const onUse = vi.fn();
    const p = mount(ModelPicker, pickerProps({ onUse, defaults: { name: "Codex default", make: vi.fn(), reset: vi.fn() },
      codex: { provider: "codex", model: "gpt-6-astra", effort: "high", featureValues: { service_tier: "priority" } } }));
    await settle(); p.render();
    press(byTestId(p.tree, "kontor-model-reset")); p.render(); await settle(); p.render();
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).toHaveBeenLastCalledWith({ provider: "codex", model: "gpt-6-astra", effort: "high", modeId: "auto", featureValues: { service_tier: "priority" } });
  });

  // R11b: Reset never clears a saved choice while the Codex default it would restore cannot be chosen here.
  it.each([["absent", null], ["not offered here", { provider: "codex", model: "gpt-5-retired", effort: "xhigh" }]] as const)("keeps Reset unavailable while the Codex default is %s, clearing nothing", async (_, codex) => {
    const defaults = { name: "GPT-6.1 Sol · High", make: vi.fn(), reset: vi.fn() }, onUse = vi.fn();
    const p = mount(ModelPicker, pickerProps({ codex, defaults, onUse, value: { provider: "codex", model: "gpt-6.1-sol", effort: "high", modeId: "auto", featureValues: {} } }));
    await settle(); p.render();
    const reason = "The Codex default is not available on this host now, so Reset would leave a different model chosen.";
    expect(byTestId(p.tree, "kontor-model-reset")!.props).toMatchObject({ disabled: true, accessibilityLabel: `Reset to Codex default. ${reason}` });
    expect(byTestId(p.tree, "kontor-model-reset-reason")?.props.children).toBe(reason);
    press(byTestId(p.tree, "kontor-model-reset")); p.render();
    expect(defaults.reset).not.toHaveBeenCalled();
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).toHaveBeenLastCalledWith(expect.objectContaining({ provider: "codex", model: "gpt-6.1-sol", effort: "high" }));
  });

  it("offers neither Make default nor Reset without a defaults owner", () => {
    const p = mount(ModelPicker, pickerProps());
    expect(byTestId(p.tree, "kontor-model-make-default")).toBeUndefined();
    expect(byTestId(p.tree, "kontor-model-reset")).toBeUndefined();
  });

  it("shows inherited permissions read-only and emits no mode", () => {
    const onUse = vi.fn();
    const p = mount(ModelPicker, pickerProps({ onUse, permissions: { inherited: "full-access", provider: "codex" }, value: { provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: null, featureValues: {} } }));
    const block = elements(byTestId(p.tree, "kontor-model-permissions-inherited")!.props.children as ReactNode);
    expect(text(block)).toBe("Permissions\nFull access ⚠ · Codex\nInherited from the seat; takeover never changes permissions.");
    expect(byLabel(p.tree, "Permissions")).toBeUndefined();
    expect(p.tree.some((node) => node.props.accessibilityRole === "radio" && ["Default permissions", "Full access ⚠"].includes(String(node.props.accessibilityLabel)))).toBe(false);
    press(byTestId(p.tree, "kontor-model-row-codex/gpt-6.1-sol")); p.render();
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ model: "gpt-6.1-sol", modeId: null }));
  });

  // R1b: a mode the seat never reported is unknown, not inherited; Use stays unavailable and says why.
  it("refuses a seat whose permission mode was never reported", () => {
    const onUse = vi.fn();
    const p = mount(ModelPicker, pickerProps({ onUse, permissions: { inherited: null, provider: "codex" }, value: { provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: null, featureValues: {} } }));
    expect(text(elements(byTestId(p.tree, "kontor-model-permissions-inherited")!.props.children as ReactNode))).toBe("Permissions\nInherited (unknown) · Codex\nInherited from the seat; takeover never changes permissions.");
    expect(byTestId(p.tree, "kontor-model-problem")?.props.children).toBe("Codex cannot keep this seat's permissions: its permission mode was not reported, so the new session's permissions cannot be confirmed.");
    press(byTestId(p.tree, "kontor-model-use"));
    expect(onUse).not.toHaveBeenCalled();
  });

  it("keeps permissions editable for a new lead", () => {
    const p = mount(ModelPicker, pickerProps());
    expect(byTestId(p.tree, "kontor-model-permissions-inherited")).toBeUndefined();
    press(opener(p.tree, "kontor-model-permissions")); p.render();
    expect(byLabel(p.tree, "Permissions")?.props.accessibilityRole).toBe("radiogroup");
    expect(byLabel(p.tree, "Default permissions")?.props["aria-checked"]).toBe(true);
  });

  it("bounds a large catalog's list so the chosen model's settings follow it, not inside it", async () => {
    const many = { ...CODEX, models: Array.from({ length: 60 }, (_, index) => ({ id: `gpt-6-m${index}`, label: `Model ${index}`, efforts: EFFORTS, defaultEffort: "high" })) };
    const p = mount(ModelPicker, pickerProps({ providers: [many], value: null }));
    press(byTestId(p.tree, "kontor-model-row-codex/gpt-6-m57")); p.render(); await settle(); p.render();
    const root = p.tree[0]!, children = [root.props.children].flat(Infinity) as Element[];
    const list = byTestId(p.tree, "kontor-model-list")!, settings = byTestId(p.tree, "kontor-model-settings")!;
    expect((list.props.style as { height?: number }).height).toBeGreaterThan(0);
    expect(children.indexOf(list)).toBeGreaterThan(-1);
    expect(children.indexOf(settings)).toBeGreaterThan(children.indexOf(list));
    const inList = elements(list.props.children as ReactNode), inSettings = elements(settings.props.children as ReactNode);
    expect(inList.filter((node) => node.props.accessibilityRole === "radio")).toHaveLength(60);
    // The levels stand above the list, in one Selected group with the chosen model; the other settings follow the list.
    const chosen = byTestId(p.tree, "kontor-model-chosen")!, inChosen = elements(chosen.props.children as ReactNode);
    expect(chosen.props).toMatchObject({ role: "group", accessibilityLabel: "Selected model" });
    expect(children.indexOf(chosen)).toBeLessThan(children.indexOf(list));
    expect(inChosen.findIndex((node) => node.props.label === "Effort")).toBeGreaterThan(inChosen.indexOf(byTestId(inChosen, "kontor-model-selected")!));
    for (const label of ["Speed", "Permissions: Default permissions"]) {
      expect(inList.some((node) => node.props.accessibilityLabel === label)).toBe(false);
      expect(inSettings.some((node) => node.props.accessibilityLabel === label)).toBe(true);
    }
    expect(inSettings.some((node) => node.props.accessibilityRole === "switch" && node.props.accessibilityLabel === "Plan")).toBe(true);
    expect(byTestId(inList, "kontor-model-row-codex/gpt-6-m57")?.props["aria-checked"]).toBe(true);
  });
});

describe("Choose source", () => {
  const sources = [sessionEntry("agent-src", { title: "Route review", projectName: "DESK • ADAM", workspaceName: "ADAM" }), sessionEntry("agent-other", { title: "Billing" })];
  const sourceProps = (onUse = vi.fn()) => ({ colors: COLORS, compact: true, sources, selected: null, onUse, onCancel: vi.fn() });

  it("names why summaries are unsupported, reads none and pins none", async () => {
    const onUse = vi.fn();
    const p = mount(LeadSourcePicker, sourceProps(onUse));
    await settle(); p.render();
    // The shared session row in pick mode: a radio at its right, nothing destructive (board B3).
    expect(byTestId(p.tree, "kontor-lead-source-agent-src")?.props.mode).toEqual({ kind: "pick", selected: false });
    press(byTestId(p.tree, "kontor-lead-source-agent-src")); p.render(); await settle(); p.render();
    expect(text(p.tree)).toContain(UNSUPPORTED.summaries.reason);
    expect(text(p.tree)).not.toContain("Saved summary ·");
    expect(model.rpc["sessions.summary-latest"]).not.toHaveBeenCalled();
    expect(footer(p.tree, "kontor-lead-source-use")).toEqual(["Cancel", "kontor-lead-source-use"]);
    press(byTestId(p.tree, "kontor-lead-source-use"));
    expect(onUse).toHaveBeenCalledExactlyOnceWith({ entry: sources[0], summary: null });
  });

  it("saves a first revision on Use when the session has none, and pins it", async () => {
    model.rpc["sessions.navigator-capabilities"]!.mockResolvedValue({ ...UNSUPPORTED, summaries: { supported: true } });
    model.rpc["sessions.summary-latest"]!.mockResolvedValue({});
    model.rpc["sessions.summary-save"] = vi.fn(async (input: { agentId: string }) => ({ ...SUMMARY, revision: 1, sourceAgentId: input.agentId }));
    const onUse = vi.fn();
    const p = mount(LeadSourcePicker, sourceProps(onUse));
    await settle(); p.render();
    press(byTestId(p.tree, "kontor-lead-source-agent-src")); p.render(); await settle(); p.render();
    expect(text(p.tree)).toContain("No saved summary yet. Use saves one and pins it for this lead.");
    press(byTestId(p.tree, "kontor-lead-source-use")); await settle();
    expect(model.rpc["sessions.summary-save"]).toHaveBeenCalledExactlyOnceWith({ agentId: "agent-src" });
    expect(onUse).toHaveBeenCalledExactlyOnceWith({ entry: sources[0], summary: expect.objectContaining({ revision: 1, sourceAgentId: "agent-src" }) });
  });

  it("offers a new revision when the session moved on after the saved one, and never pins on a failed save", async () => {
    model.rpc["sessions.navigator-capabilities"]!.mockResolvedValue({ ...UNSUPPORTED, summaries: { supported: true } });
    model.rpc["sessions.summary-save"] = vi.fn(async () => { throw new Error("store offline"); });
    const moved = [sessionEntry("agent-src", { title: "Route review", activeTurnStartedAt: "2026-10-25T11:00:00Z" })];
    const onUse = vi.fn();
    const p = mount(LeadSourcePicker, { ...sourceProps(onUse), sources: moved });
    await settle(); p.render();
    press(byTestId(p.tree, "kontor-lead-source-agent-src")); p.render(); await settle(); p.render();
    expect(text(p.tree)).toContain("The session moved on after r3.");
    press(p.tree.find((node) => node.type === Action && node.props.label === "Save r4")); await settle(); p.render();
    expect(text(p.tree)).toContain("The saved summary could not be read or saved: store offline");
    expect(byTestId(p.tree, "kontor-lead-source-use")?.props.disabled).toBe(true);
    expect(onUse).not.toHaveBeenCalled();
  });

  it("pins the latest saved summary revision where the host keeps them", async () => {
    model.rpc["sessions.navigator-capabilities"]!.mockResolvedValue({ ...UNSUPPORTED, summaries: { supported: true } });
    const onUse = vi.fn();
    const p = mount(LeadSourcePicker, sourceProps(onUse));
    await settle(); p.render();
    press(byTestId(p.tree, "kontor-lead-source-agent-src")); p.render(); await settle(); p.render();
    expect(text(p.tree)).toContain("Saved summary · r3");
    press(byTestId(p.tree, "kontor-lead-source-use"));
    expect(onUse).toHaveBeenCalledExactlyOnceWith({ entry: sources[0], summary: { ...SUMMARY, sourceAgentId: "agent-src" } });
  });
});
