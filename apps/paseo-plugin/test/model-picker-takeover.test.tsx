import { isValidElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CreateProvider } from "../shared/session-create";

// Each test gets a fresh renderer, so one picker's choice never carries into another test.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const rpc = vi.hoisted(() => ({}) as Record<string, ReturnType<typeof vi.fn>>);
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, useWindowDimensions: () => ({ width: 390, height: 844 }), Pressable: "Pressable", Text: "Text", View: "View", ScrollView: "ScrollView", Image: "Image" }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => rpc[contract.name] }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", TextInput: "TextInput" }));

import { ModelPicker } from "../client/model-picker";

type Element = { type: unknown; props: Record<string, unknown> };
/** The shared footer is hook-free and drawn in place, so its buttons and their order are visible. */
const DRAWN = new Set(["SheetFooter", "FooterButton"]);
const walk = (node: unknown): Element[] => Array.isArray(node) ? node.flatMap(walk) : !isValidElement<Record<string, unknown>>(node) ? []
  : typeof node.type === "function" && DRAWN.has(node.type.name) ? [node, ...walk((node.type as (props: object) => unknown)(node.props))] : [node, ...walk(node.props.children)];
const text = (tree: Element[]) => tree.filter((node) => node.type === "Text").map((node) => [node.props.children].flat(Infinity).join("")).join("\n");
const pressable = (tree: Element[], id: string) => tree.find((node) => node.type === "Pressable" && node.props.testID === id);
const press = (node: Element | undefined) => (node!.props.onPress as () => void)();
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const EFFORTS = [{ id: "high", label: "High" }, { id: "xhigh", label: "XHigh" }];
const CODEX: CreateProvider = { id: "codex", label: "Codex", status: "ready", available: true, note: null, persona: "supported", defaultModel: "gpt-6-astra", defaultMode: "auto",
  models: [{ id: "gpt-6-astra", label: "GPT-6 Astra", efforts: EFFORTS, defaultEffort: "xhigh" }, { id: "gpt-6-sol", label: "GPT-6 Sol", efforts: EFFORTS, defaultEffort: "high" }],
  modes: [{ id: "auto", label: "Default permissions", dangerous: false }, { id: "full-access", label: "Full access", dangerous: true }] };

beforeEach(() => {
  harness.fresh();
  rpc["sessions.create-features"] = vi.fn(async () => ({ features: [], problem: null }));
});

/** The picker as Take over opens it for three seats: Auto, a per-seat permissions line, a one-operation note and a counted verb. */
function picker(patch: Partial<Parameters<typeof ModelPicker>[0]> = {}) {
  const props: Parameters<typeof ModelPicker>[0] = { colors: COLORS, compact: true, providers: [CODEX], projectId: "prj-adam", value: null, codex: { provider: "codex", model: "gpt-6-astra", effort: "xhigh" },
    onUse: vi.fn(), onCancel: vi.fn(), onAuto: vi.fn(), permissions: { seats: 3 }, note: "Only this takeover · Defaults stay unchanged.", subtitle: "For 3 selected seats",
    action: (alias) => `Take over 3${alias ? ` with ${alias}` : ""}`, ...patch };
  let tree: Element[] = [];
  const render = () => (tree = walk(harness.current.render(() => ModelPicker(props))));
  render();
  return { props, render, get tree() { return tree; } };
}
const auto = (tree: Element[]) => pressable(tree, "kontor-model-auto");
const use = (tree: Element[]) => pressable(tree, "kontor-model-use")!;

describe("ModelPicker in takeover mode", () => {
  it("checks Auto while no model is picked, and Use then takes over on each seat's chain", () => {
    const p = picker();
    expect(auto(p.tree)!.props).toMatchObject({ accessibilityRole: "radio", accessibilityState: { checked: true }, "aria-checked": true, accessibilityLabel: "Auto, use each seat's chain" });
    expect(use(p.tree).props).toMatchObject({ accessibilityLabel: "Take over 3", disabled: false });
    press(use(p.tree));
    expect(p.props.onAuto).toHaveBeenCalledTimes(1);
    expect(p.props.onUse).not.toHaveBeenCalled();
  });

  it("names the picked model in the primary and hands back that model with no permission mode", async () => {
    const p = picker();
    press(pressable(p.tree, "kontor-model-row-codex/gpt-6-sol")); p.render(); await settle(); p.render();
    expect(auto(p.tree)!.props.accessibilityState).toEqual({ checked: false });
    expect(use(p.tree).props.accessibilityLabel).toBe("Take over 3 with S6");
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenCalledExactlyOnceWith({ provider: "codex", model: "gpt-6-sol", effort: "high", modeId: null, featureValues: {} });
    expect(p.props.onAuto).not.toHaveBeenCalled();
  });

  it("chooses a model and Auto again on Space, as a press does", () => {
    const space = (node: Element | undefined) => (node!.props.onKeyDown as (event: object) => void)({ key: " ", preventDefault: () => {}, stopPropagation: () => {} });
    const p = picker();
    space(pressable(p.tree, "kontor-model-row-codex/gpt-6-sol")); p.render();
    expect(pressable(p.tree, "kontor-model-row-codex/gpt-6-sol")!.props.accessibilityState).toEqual({ checked: true });
    space(auto(p.tree)); p.render();
    expect(auto(p.tree)!.props.accessibilityState).toEqual({ checked: true });
  });

  it("draws the checked mark in the green on light surfaces and in the foreground on the dark ones, where the green is under 3:1", () => {
    const check = (colors: object) => walk(auto(picker({ colors: colors as never }).tree)).find((node) => node.props.name === "CircleCheck")!.props.color;
    expect(check({ ...(COLORS as object), surface0: "#ffffff", surface1: "#fafafa", surface2: "#f4f4f5", accent: "#20744A" })).toBe("#20744A");
    expect(check({ ...(COLORS as object), surface0: "#181B1A", surface1: "#1E2120", surface2: "#272A29", foreground: "#fafafa", accent: "#20744A" })).toBe("#fafafa");
  });

  it("returns to Auto when its row is pressed after a model was picked", () => {
    const p = picker();
    press(pressable(p.tree, "kontor-model-row-codex/gpt-6-sol")); p.render();
    press(auto(p.tree)); p.render();
    expect(auto(p.tree)!.props.accessibilityState).toEqual({ checked: true });
    expect(use(p.tree).props.accessibilityLabel).toBe("Take over 3");
  });

  it("writes the operation's note above the footer", () => {
    expect(picker().tree.find((node) => node.props.testID === "kontor-model-note")!.props.children).toBe("Only this takeover · Defaults stay unchanged.");
  });

  it("shows permissions inherited per seat, read-only, for a picked model", async () => {
    const p = picker();
    press(pressable(p.tree, "kontor-model-row-codex/gpt-6-sol")); p.render(); await settle(); p.render();
    expect(text(p.tree)).toContain("Permissions inherited per seat");
    expect(text(p.tree)).toContain("Inherited from the seat; takeover never changes permissions.");
    expect(p.tree.find((node) => node.props.testID === "kontor-model-permissions")).toBeUndefined();
  });

  it("ends in the shared footer, Cancel left and the primary right", () => {
    const p = picker();
    const footer = walk(p.tree.find((node) => node.props.testID === "operation-sheet-footer")).filter((node) => node.type === "Pressable");
    expect(footer.map((node) => node.props.accessibilityLabel)).toEqual(["Cancel model choice", "Take over 3"]);
    expect(footer[1]!.props.testID).toBe("kontor-model-use");
    press(footer[0]);
    expect(p.props.onCancel).toHaveBeenCalledTimes(1);
  });

  it("keeps the new-lead picker without Auto or the shared footer", () => {
    const p = picker({ onAuto: undefined, action: undefined, permissions: undefined, note: undefined, value: { provider: "codex", model: "gpt-6-sol", effort: "high", modeId: "auto", featureValues: {} } });
    expect(auto(p.tree)).toBeUndefined();
    expect(p.tree.find((node) => node.props.testID === "operation-sheet-footer")).toBeUndefined();
    expect(use(p.tree).props.accessibilityLabel).toBe("Use S6 · High");
  });
});

const CLAUDE: CreateProvider = { id: "claude", label: "Claude Code", status: "ready", available: true, note: null, persona: "supported", defaultModel: "claude-opus-5-5", defaultMode: "default",
  models: [{ id: "claude-opus-5-5", label: "Claude Opus 5.5", efforts: [{ id: "max", label: "Max" }], defaultEffort: "max" }],
  modes: [{ id: "default", label: "Default", dangerous: false }, { id: "auto", label: "Auto", dangerous: false }, { id: "bypassPermissions", label: "Bypass permissions", dangerous: true }] };
const SPEED_PLAN = [{ type: "select", id: "service_tier", label: "Speed", value: null, options: [{ id: "default", label: "Normal", isDefault: true }, { id: "priority", label: "Fast", isDefault: false }] },
  { type: "toggle", id: "plan_mode", label: "Plan", value: false }];
const problemText = (tree: Element[]) => tree.find((node) => node.props.testID === "kontor-model-problem")?.props.children ?? null;
const changedText = (tree: Element[]) => tree.find((node) => node.props.testID === "kontor-model-changed")?.props.children ?? null;

// R1: one seat's permissions are its own provider's mode; no other provider is shown to keep them.
describe("ModelPicker inherited permissions for one seat", () => {
  const seat = (inherited: string | null) => picker({ providers: [CODEX, CLAUDE], permissions: { inherited, provider: "claude" }, subtitle: "For Claude seat", action: (alias) => `Take over${alias ? ` with ${alias}` : ""}` });

  it("shows the seat's actual mode by its own provider, read-only", async () => {
    const p = seat("bypassPermissions");
    press(pressable(p.tree, "kontor-model-vendor-anthropic")); p.render();
    press(pressable(p.tree, "kontor-model-row-claude/claude-opus-5-5")); p.render(); await settle(); p.render();
    expect(p.tree.find((node) => node.props.testID === "kontor-model-permissions-mode")!.props.children).toBe("Bypass permissions ⚠ · Claude Code");
    expect(p.tree.find((node) => node.props.testID === "kontor-model-permissions")).toBeUndefined();
    expect(use(p.tree).props.disabled).toBe(false);
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenCalledExactlyOnceWith({ provider: "claude", model: "claude-opus-5-5", effort: "max", modeId: null, featureValues: {} });
  });

  // R1b: a mode the seat never reported is missing evidence, even on its own provider.
  it("refuses the seat's own provider when its permission mode was never reported", async () => {
    const p = seat(null);
    press(pressable(p.tree, "kontor-model-vendor-anthropic")); p.render();
    press(pressable(p.tree, "kontor-model-row-claude/claude-opus-5-5")); p.render(); await settle(); p.render();
    expect(problemText(p.tree)).toBe("Claude Code cannot keep this seat's permissions: its permission mode was not reported, so the new session's permissions cannot be confirmed.");
    expect(use(p.tree).props.disabled).toBe(true);
    press(use(p.tree));
    expect(p.props.onUse).not.toHaveBeenCalled();
  });

  it.each(["bypassPermissions", "auto"])("refuses a Codex model for a Claude seat in %s, even where Codex lists the same mode id", async (mode) => {
    const p = seat(mode);
    press(pressable(p.tree, "kontor-model-row-codex/gpt-6-sol")); p.render(); await settle(); p.render();
    expect(problemText(p.tree)).toBe(`Codex cannot keep this seat's permissions (${mode} on claude): no permission mapping between providers is qualified.`);
    expect(use(p.tree).props.disabled).toBe(true);
    press(use(p.tree));
    expect(p.props.onUse).not.toHaveBeenCalled();
  });
});

// R3: another model keeps what it also offers; only what it lacks resets, and the picker says so.
describe("ModelPicker settings across a change of model", () => {
  const FAST_PLAN = { service_tier: "priority", plan_mode: true };
  function lead() {
    rpc["sessions.create-features"] = vi.fn(async (input: { provider: string }) => ({ features: input.provider === "codex" ? SPEED_PLAN : [], problem: null }));
    return picker({ providers: [CODEX, CLAUDE], onAuto: undefined, action: undefined, permissions: undefined, note: undefined,
      value: { provider: "codex", model: "gpt-6-astra", effort: "high", modeId: "auto", featureValues: FAST_PLAN }, defaults: { name: "GPT-6 Astra", make: vi.fn(), reset: vi.fn() } });
  }
  const read = async (p: ReturnType<typeof picker>) => { p.render(); await settle(); p.render(); };

  it("keeps effort, Speed Fast and Plan on from GPT-6 Astra to GPT-6 Sol, which offers them", async () => {
    const p = lead(); await read(p);
    press(pressable(p.tree, "kontor-model-row-codex/gpt-6-sol")); await read(p);
    expect(changedText(p.tree)).toBeNull();
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenCalledExactlyOnceWith({ provider: "codex", model: "gpt-6-sol", effort: "high", modeId: "auto", featureValues: FAST_PLAN });
  });

  it("drops only what the new model lacks, says so, and uses an effort it offers", async () => {
    const p = lead(); await read(p);
    press(pressable(p.tree, "kontor-model-vendor-anthropic")); p.render();
    press(pressable(p.tree, "kontor-model-row-claude/claude-opus-5-5")); await read(p);
    expect(changedText(p.tree)).toBe("Claude Opus 5.5 does not offer effort High; it uses Max. Claude Opus 5.5 does not offer Speed Fast, Plan on, so they are not applied.");
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenCalledExactlyOnceWith({ provider: "claude", model: "claude-opus-5-5", effort: "max", modeId: "default", featureValues: {} });
  });

  it("changes nothing when the checked model is chosen again", async () => {
    const p = lead(); await read(p);
    press(pressable(p.tree, "kontor-model-row-codex/gpt-6-astra")); await read(p);
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenCalledExactlyOnceWith({ provider: "codex", model: "gpt-6-astra", effort: "high", modeId: "auto", featureValues: FAST_PLAN });
  });

  it("starts Reset from the Codex default's own settings, carrying none", async () => {
    const p = lead(); await read(p);
    press(pressable(p.tree, "kontor-model-row-codex/gpt-6-sol")); await read(p);
    press(pressable(p.tree, "kontor-model-reset")); await read(p);
    expect(p.props.defaults!.reset).toHaveBeenCalledTimes(1);
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenCalledExactlyOnceWith({ provider: "codex", model: "gpt-6-astra", effort: "xhigh", modeId: "auto", featureValues: {} });
  });
});

// Synthetic routes shaped as a host reports them: each route lists its own effort IDs, and one model lists none.
const levels = (...ids: string[]) => ids.map((id) => ({ id, label: id === "xhigh" ? "XHigh" : `${id[0]!.toUpperCase()}${id.slice(1)}` }));
const ROUTES: CreateProvider[] = [
  { ...CLAUDE, models: [{ id: "claude-opus-5-5", label: "Claude Opus 5.5", efforts: levels("low", "medium", "high", "xhigh", "max", "ultracode"), defaultEffort: "high" },
    { id: "claude-haiku-4-5", label: "Claude Haiku 4.5", efforts: [], defaultEffort: null }] },
  { id: "opencode", label: "OpenCode", status: "ready", available: true, note: null, persona: "unverified", defaultModel: null, defaultMode: null, modes: [],
    models: [{ id: "deepseek/deepseek-v4", label: "DeepSeek V4", efforts: levels("default", "low", "high", "max"), defaultEffort: "default" },
      { id: "zai/glm-5.3", label: "GLM 5.3", efforts: levels("default", "low", "high", "max"), defaultEffort: "default" }] },
  { id: "glm-agent", label: "GLM Agent", status: "ready", available: true, note: null, persona: "unverified", defaultModel: "glm-5.3", defaultMode: null, modes: [],
    models: [{ id: "glm-5.3", label: "GLM 5.3", efforts: levels("minimal", "low", "medium", "high", "xhigh", "max"), defaultEffort: "medium" }] },
];
const OPUS = { provider: "claude", model: "claude-opus-5-5", effort: "xhigh", modeId: "default", featureValues: {} };
/** The levels the Selected group offers, as exact IDs with the checked one marked, or null when it offers none. */
const offered = (tree: Element[]) => {
  const group = tree.find((node) => node.props.label === "Effort");
  return group ? (group.props.options as { id: string }[]).map((option) => `${option.id}${option.id === group.props.selected ? " ✓" : ""}`) : null;
};
const level = (tree: Element[], id: string) => (tree.find((node) => node.props.label === "Effort")!.props.onSelect as (next: string) => void)(id);
const selected = (tree: Element[]) => text(walk(tree.find((node) => node.props.testID === "kontor-model-selected")!.props.children));

describe("ModelPicker effort levels per route", () => {
  const modes = { "new lead": { onAuto: undefined, action: undefined, permissions: undefined, note: undefined },
    takeover: { permissions: { inherited: "default", provider: "claude" }, subtitle: "For Claude seat", action: (alias: string | null) => `Take over${alias ? ` with ${alias}` : ""}` } } as const;
  const open = (mode: keyof typeof modes, value: Parameters<typeof ModelPicker>[0]["value"] = OPUS) => picker({ providers: ROUTES, value, ...modes[mode] });
  const read = async (p: ReturnType<typeof picker>) => { p.render(); await settle(); p.render(); };

  it.each([["new lead", "default"], ["takeover", null]] as const)("in %s, browsing another vendor changes neither the selected model nor its levels", async (mode, modeId) => {
    const p = open(mode); await read(p);
    expect(selected(p.tree)).toBe("Claude Opus 5.5\nvia Claude Code · O5.5\nSelected");
    expect(offered(p.tree)).toEqual(["low", "medium", "high", "xhigh ✓", "max", "ultracode"]);
    press(pressable(p.tree, "kontor-model-vendor-deepseek")); await read(p);
    expect(pressable(p.tree, "kontor-model-vendor-deepseek")!.props["aria-selected"]).toBe(true);
    expect(pressable(p.tree, "kontor-model-vendor-anthropic")!.props).toMatchObject({ "aria-selected": false, accessibilityLabel: "Anthropic, 2 models, has the selected model" });
    expect(p.tree.find((node) => node.props.testID === "kontor-model-search")!.props.accessibilityLabel).toBe("Search DeepSeek models");
    expect(pressable(p.tree, "kontor-model-row-opencode/deepseek/deepseek-v4")!.props["aria-checked"]).toBe(false);
    expect(selected(p.tree)).toBe("Claude Opus 5.5\nvia Claude Code · O5.5\nSelected");
    expect(offered(p.tree)).toEqual(["low", "medium", "high", "xhigh ✓", "max", "ultracode"]);
    expect(changedText(p.tree)).toBeNull();
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenCalledExactlyOnceWith({ ...OPUS, modeId });
  });

  it("offers each route only its own effort IDs, keeps a level the next model lists and names the one it drops", async () => {
    const p = open("new lead"); await read(p);
    press(pressable(p.tree, "kontor-model-vendor-deepseek")); p.render();
    press(pressable(p.tree, "kontor-model-row-opencode/deepseek/deepseek-v4")); await read(p);
    expect(offered(p.tree)).toEqual(["default ✓", "low", "high", "max"]);
    expect(changedText(p.tree)).toBe("DeepSeek V4 does not offer effort XHigh; it uses Default.");
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenLastCalledWith({ provider: "opencode", model: "deepseek/deepseek-v4", effort: "default", modeId: null, featureValues: {} });
    level(p.tree, "high"); p.render();
    // The same model on two routes: GLM Agent lists its own six levels and keeps High; OpenCode lists four and drops XHigh.
    press(pressable(p.tree, "kontor-model-vendor-zai")); p.render();
    press(pressable(p.tree, "kontor-model-row-glm-agent/glm-5.3")); await read(p);
    expect(selected(p.tree)).toContain("via GLM Agent");
    expect(offered(p.tree)).toEqual(["minimal", "low", "medium", "high ✓", "xhigh", "max"]);
    expect(changedText(p.tree)).toBeNull();
    level(p.tree, "xhigh"); p.render();
    press(pressable(p.tree, "kontor-model-row-opencode/zai/glm-5.3")); await read(p);
    expect(selected(p.tree)).toContain("via OpenCode");
    expect(offered(p.tree)).toEqual(["default ✓", "low", "high", "max"]);
    expect(changedText(p.tree)).toBe("GLM 5.3 does not offer effort XHigh; it uses Default.");
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenLastCalledWith({ provider: "opencode", model: "zai/glm-5.3", effort: "default", modeId: null, featureValues: {} });
    // A model that lists no levels shows none, says so, and sends no effort.
    press(pressable(p.tree, "kontor-model-vendor-anthropic")); p.render();
    press(pressable(p.tree, "kontor-model-row-claude/claude-haiku-4-5")); await read(p);
    expect(offered(p.tree)).toBeNull();
    expect(text(p.tree)).toContain("No effort setting for this model.");
    expect(changedText(p.tree)).toBe("Claude Haiku 4.5 does not offer effort Default; it uses no effort setting.");
    press(use(p.tree));
    expect(p.props.onUse).toHaveBeenLastCalledWith({ provider: "claude", model: "claude-haiku-4-5", effort: null, modeId: "default", featureValues: {} });
  });

  it("never offers or sends a level the route does not list", async () => {
    const p = open("new lead", { provider: "opencode", model: "deepseek/deepseek-v4", effort: "xhigh", modeId: null, featureValues: {} }); await read(p);
    expect(offered(p.tree)).toEqual(["default", "low", "high", "max"]);
    expect(problemText(p.tree)).toBe("Effort xhigh is not offered for DeepSeek V4.");
    press(use(p.tree));
    expect(p.props.onUse).not.toHaveBeenCalled();
  });
});
