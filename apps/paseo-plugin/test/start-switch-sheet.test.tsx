import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { CreateOptions, CreateOutcome } from "../shared/session-create";
import type { DirectoryEntry, DirectoryState } from "../client/session-directory";

const model = vi.hoisted(() => ({
  slots: [] as unknown[], cursor: 0, effect: 0, deps: [] as (readonly unknown[] | undefined)[],
  rpc: {} as Record<string, ReturnType<typeof vi.fn>>,
  focusControl: vi.fn(),
  useState<T>(initial: T | (() => T)): [T, (next: T | ((previous: T) => T)) => void] {
    const index = this.cursor++;
    if (!(index in this.slots)) this.slots[index] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [this.slots[index] as T, (next) => { this.slots[index] = typeof next === "function" ? (next as (previous: T) => T)(this.slots[index] as T) : next; }];
  },
  /** Runs an effect when its dependencies change, as React would after a render. */
  useEffect(effect: () => void, deps?: readonly unknown[]) {
    const index = this.effect++, previous = this.deps[index];
    if (previous && deps && deps.length === previous.length && deps.every((value, at) => Object.is(value, previous[at]))) return;
    this.deps[index] = deps;
    effect();
  },
}));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  useState: <T,>(initial: T | (() => T)) => model.useState(initial),
  useRef: <T,>(initial: T) => model.useState(() => ({ current: initial }))[0],
  useEffect: (effect: () => void, deps?: readonly unknown[]) => model.useEffect(effect, deps) }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", View: "View", Text: "Text", Pressable: "Pressable", BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => model.rpc[contract.name] }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
vi.mock("../client/mobile-filters", () => ({ focusControl: model.focusControl }));

import { createAvailability, StartSwitchSheet, switchRows, type CreateOptionsState } from "../client/start-switch-sheet";
import { NewLsaForm, newRequestId, personaLine, resolveDraft } from "../client/new-lsa-form";
import { configureRecentStorage, recordVisit } from "../client/recent-sessions";
import { Action, Copy } from "../client/page-controls";
import { sessionEntry } from "./session-fixtures";

type Element = ReactElement<Record<string, unknown>>;
/** The element tree; the hook-free `Choices` and `SwitchRowItem` are drawn so their semantics are visible. */
function elements(value: ReactNode): Element[] {
  if (Array.isArray(value)) return value.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(value)) return [];
  const drawn = typeof value.type === "function" && ["Choices", "SwitchRowItem"].includes(value.type.name) ? (value.type as (props: object) => ReactNode)(value.props) : null;
  return [value, ...elements(value.props.children as ReactNode), ...elements(value.props.header as ReactNode), ...elements(drawn)];
}
const text = (tree: Element[]) => tree.filter((node) => node.type === "Text" || node.type === Copy).map((node) => [node.props.children].flat(Infinity).join("")).join("\n");
const byTestId = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id);
const action = (tree: Element[], label: string) => tree.find((node) => node.type === Action && node.props.label === label);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const NOW = Date.parse("2026-10-22T12:00:00.000Z");
const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const OPTIONS: CreateOptions = {
  enabled: true, disabledReason: null, pending: null, notes: [],
  desks: [{ workspaceId: "wks-adam", projectId: "prj-adam", desk: "ADAM", project: "DESK • ADAM" }, { workspaceId: "wks-pr", projectId: "prj-pr", desk: "PR REVIEW", project: "DESK • PR REVIEW" }],
  providers: [
    { id: "claude", label: "Claude", available: true, note: null, persona: "supported", defaultModel: "opus", defaultMode: "default",
      models: [{ id: "opus", label: "Opus", efforts: [{ id: "high", label: "High" }, { id: "max", label: "Max" }], defaultEffort: "high" }, { id: "sonnet", label: "Sonnet", efforts: [], defaultEffort: null }],
      modes: [{ id: "default", label: "Default", dangerous: false }, { id: "bypassPermissions", label: "Bypass", dangerous: true }] },
    { id: "gemini", label: "Gemini", available: false, note: "Provider error: not signed in.", persona: "unverified", defaultModel: null, defaultMode: null, models: [], modes: [] },
    { id: "opencode", label: "OpenCode", available: true, note: null, persona: "unverified", defaultModel: "big", defaultMode: null, models: [{ id: "big", label: "Big", efforts: [], defaultEffort: null }], modes: [] },
  ],
  profile: { name: "lsa", provider: "claude", model: "opus", modeId: "default", effort: "max" },
};
const outcome = (state: CreateOutcome["state"], patch: Partial<CreateOutcome> = {}): CreateOutcome => ({ requestId: "00000000-0000-4000-8000-000000000001", state, title: "LSA • ADHOC • Route review",
  agentId: state === "verified" || state === "differences" ? "agent-new" : null, workspaceId: "wks-adam", message: `Fixture ${state}.`, differences: [], persona: "supported", checkedAt: "2026-10-22", ...patch });
const prompted = (id: string, at: number, overrides: Parameters<typeof sessionEntry>[1] = {}) => ({ ...sessionEntry(id, overrides), lastUserMessageAt: new Date(at).toISOString() });
const directory = (entries: DirectoryEntry[], live = true): DirectoryState => ({ entries, status: "ready", live, lastUpdatedAt: NOW, error: null, staleReason: null, pages: 1, includeArchived: false });

beforeEach(() => {
  model.slots = []; model.cursor = 0; model.effect = 0; model.deps = [];
  model.focusControl.mockClear();
  model.rpc = { "sessions.create-options": vi.fn(async () => OPTIONS), "sessions.create": vi.fn(), "sessions.create-check": vi.fn() };
  configureRecentStorage(null);
});

describe("Start or switch rows", () => {
  it("lists up to eight Recent rows with their place and reason, and none archived", () => {
    const entries: DirectoryEntry[] = Array.from({ length: 10 }, (_, index) => prompted(`agent-${index}`, NOW - (index + 1) * 60_000, { projectName: "DESK • ADAM", workspaceName: "ADAM" }));
    entries.push(sessionEntry("agent-archived", { archivedAt: "2026-10-22" }));
    recordVisit("agent-archived", NOW); recordVisit("agent-9", NOW - 1000);
    const { rows, total } = switchRows(entries, "", NOW);
    expect(total).toBe(8);
    expect(rows.map((row) => row.entry.id)).toEqual(["agent-9", "agent-0", "agent-1", "agent-2", "agent-3", "agent-4", "agent-5", "agent-6"]);
    expect(rows[0]!.detail).toMatch(/^Opened .+ · DESK • ADAM \/ ADAM$/);
    expect(rows[1]!.detail).toMatch(/^Prompted .+ · DESK • ADAM \/ ADAM$/);
  });

  it("matches every token across the directory, Recent first, at most 20 shown with the full count", () => {
    const entries = Array.from({ length: 25 }, (_, index) => sessionEntry(`agent-${index}`, { title: `Route review ${index}` }));
    entries.push(sessionEntry("agent-other", { title: "Billing" }), sessionEntry("agent-archived", { title: "Route review old", archivedAt: "2026-10-22" }));
    recordVisit("agent-24", NOW);
    const { rows, total } = switchRows(entries, "  review   ROUTE ", NOW);
    expect(total).toBe(25);
    expect(rows).toHaveLength(20);
    expect(rows[0]!.entry.id).toBe("agent-24");
    expect(rows.map((row) => row.entry.id)).not.toContain("agent-archived");
  });

  it("says why New LSA is not offered", () => {
    expect(createAvailability({ status: "offline" })).toContain("host offline");
    expect(createAvailability({ status: "loading" })).toBeNull();
    expect(createAvailability({ status: "error", message: "timeout" })).toBe("New LSA unavailable: Kontor could not read this host's options (timeout).");
    expect(createAvailability({ status: "ready", options: { ...OPTIONS, enabled: false, disabledReason: "New LSA is off on this host." } })).toBe("New LSA is off on this host.");
    expect(createAvailability({ status: "ready", options: OPTIONS })).toBeNull();
  });
});

describe("Start or switch sheet", () => {
  function sheet(props: Partial<Parameters<typeof StartSwitchSheet>[0]> = {}) {
    const onOpen = vi.fn(), onShowAll = vi.fn(), onClose = vi.fn();
    const all = { anchor: { current: null }, root: { current: null }, rootSize: { width: 1440, height: 900 }, compact: false, colors: COLORS, hostStatus: "online" as const,
      directory: directory([prompted("agent-a", Date.now() - 60_000, { title: "LSA • ASMA-1 • Route" })]), canOpen: true, onOpen, onShowAll, onClose, ...props };
    let tree: Element[] = [];
    const render = () => { model.cursor = 0; model.effect = 0; tree = elements(StartSwitchSheet(all)); return tree; };
    render();
    return { render, onOpen, onShowAll, onClose, get tree() { return tree; } };
  }

  it("is a labelled dialog whose rows are menu items that open through the screen, with no archive control", () => {
    const s = sheet();
    const root = s.tree[0]!;
    expect(root.props).toMatchObject({ testID: "kontor-switch-sheet", dialog: true, label: "Start or switch session", placement: { align: "end", width: 400, maxHeight: 560 } });
    expect(byTestId(s.tree, "kontor-switch-list")?.props).toMatchObject({ role: "menu", accessibilityLabel: "Recent sessions" });
    const row = byTestId(s.tree, "kontor-switch-row-agent-a")!;
    expect(row.props).toMatchObject({ role: "menuitem", accessibilityLabel: expect.stringMatching(/^LSA • ASMA-1 • Route, Prompted .+ · /) });
    (row.props.onPress as () => void)();
    expect(s.onOpen).toHaveBeenCalledWith(expect.objectContaining({ id: "agent-a" }));
    expect(text(s.tree)).not.toMatch(/Archive|✕|×/);
    expect(byTestId(s.tree, "kontor-switch-search")?.props).toMatchObject({ accessibilityLabel: "Jump to any session", autoFocus: true });
  });

  it("covers the screen on a phone and does not steal focus there", () => {
    const s = sheet({ compact: true, rootSize: { width: 320, height: 640 } });
    expect(s.tree[0]!.props.placement).toEqual({ align: "stretch", width: 0, cover: true });
    expect(byTestId(s.tree, "kontor-switch-search")?.props.autoFocus).toBe(false);
  });

  it("restores trigger focus through the shared native helper on Close and panel dismissal", async () => {
    const s = sheet();
    (action(s.tree, "Close")!.props.onPress as () => void)();
    expect(s.onClose).toHaveBeenCalledTimes(1);
    await settle();
    expect(model.focusControl).toHaveBeenCalledTimes(1);
    (s.tree[0]!.props.onClose as () => void)();
    await settle();
    expect(model.focusControl).toHaveBeenCalledTimes(2);
  });

  it("offers New LSA only after the host's options say it is on, and opens the form in place", async () => {
    const s = sheet();
    expect(action(s.tree, "New LSA")).toBeUndefined();
    await settle(); s.render();
    expect(model.rpc["sessions.create-options"]).toHaveBeenCalledTimes(1);
    (action(s.tree, "New LSA")!.props.onPress as () => void)(); s.render();
    expect(s.tree[0]!.props.label).toBe("New LSA session");
    const form = s.tree.find((node) => node.type === NewLsaForm)!;
    expect(form.props).toMatchObject({ options: { status: "ready", options: OPTIONS }, canOpen: true });
    expect((form.props.listed as (id: string) => boolean)("agent-a")).toBe(true);
    expect((form.props.listed as (id: string) => boolean)("agent-unknown")).toBe(false);
    (form.props.onCancel as () => void)(); s.render();
    expect(byTestId(s.tree, "kontor-switch-list")).toBeDefined();
  });

  it.each([
    ["the switch is off", async () => ({ ...OPTIONS, enabled: false, disabledReason: "New LSA is off on this host. The operator turns it on at the host computer." }), "New LSA is off on this host. The operator turns it on at the host computer."],
    ["the options read failed", async () => { throw new Error("handler crashed"); }, "New LSA unavailable: Kontor could not read this host's options (handler crashed)."],
  ])("states why New LSA is absent when %s", async (_label, read, message) => {
    model.rpc["sessions.create-options"] = vi.fn(read);
    const s = sheet();
    await settle(); s.render();
    expect(action(s.tree, "New LSA")).toBeUndefined();
    expect(byTestId(s.tree, "kontor-switch-create-status")?.props.children).toBe(message);
  });

  it("asks nothing of an offline host and shows the last known list", () => {
    const s = sheet({ hostStatus: "offline", directory: directory([prompted("agent-a", Date.now() - 60_000)], false) });
    expect(model.rpc["sessions.create-options"]).not.toHaveBeenCalled();
    expect(byTestId(s.tree, "kontor-switch-create-status")?.props.children).toContain("host offline");
    expect(byTestId(s.tree, "kontor-switch-freshness")).toBeDefined();
    expect(byTestId(s.tree, "kontor-switch-row-agent-a")).toBeDefined();
  });

  it("keeps Check reachable for a pending creation after the operator turns creation off", async () => {
    model.rpc["sessions.create-options"]!.mockResolvedValue({ ...OPTIONS, enabled: false, disabledReason: "New LSA is off on this host.", pending: outcome("unknown") });
    const s = sheet();
    await settle(); s.render();
    expect(action(s.tree, "New LSA")).toBeUndefined();
    (action(s.tree, "Check creation")!.props.onPress as () => void)(); s.render();
    expect(s.tree.find((node) => node.type === NewLsaForm)?.props.options).toMatchObject({ status: "ready", options: { enabled: false, pending: { state: "unknown" } } });
  });

  it("searches, hands a long result to Sessions, and says when nothing matches", () => {
    const entries = Array.from({ length: 22 }, (_, index) => sessionEntry(`agent-${index}`, { title: `Route review ${index}` }));
    const s = sheet({ directory: directory(entries) });
    (byTestId(s.tree, "kontor-switch-search")!.props.onChangeText as (value: string) => void)(" route "); s.render();
    expect(byTestId(s.tree, "kontor-switch-list")?.props.accessibilityLabel).toBe("Matching sessions");
    (action(s.tree, "Show all 22 in Sessions")!.props.onPress as () => void)();
    expect(s.onShowAll).toHaveBeenCalledWith("route");
    (byTestId(s.tree, "kontor-switch-search")!.props.onChangeText as (value: string) => void)("nothing like it"); s.render();
    expect(byTestId(s.tree, "kontor-switch-empty")?.props.children).toBe("No session matches “nothing like it”.");
  });

  it("clears Recent on this device and says so", () => {
    recordVisit("agent-a", Date.now());
    const s = sheet({ directory: directory([sessionEntry("agent-a")]) });
    expect(byTestId(s.tree, "kontor-switch-row-agent-a")).toBeDefined();
    (action(s.tree, "Clear recent on this device")!.props.onPress as () => void)(); s.render();
    expect(byTestId(s.tree, "kontor-switch-row-agent-a")).toBeUndefined();
    expect(String(byTestId(s.tree, "kontor-switch-empty")?.props.children)).toMatch(/^Cleared on this device at /);
    expect(text(s.tree)).toContain("Remembered until Paseo restarts on this device.");
  });

  it("lists without opening when this Paseo app gives plugins no navigation", () => {
    expect(text(sheet({ canOpen: false }).tree)).toContain("does not let plugins open sessions");
  });
});

describe("New LSA draft", () => {
  it("starts from the host's LSA profile, then each provider's own defaults", () => {
    const draft = resolveDraft(OPTIONS, { topic: " Route   review " });
    expect(draft).toMatchObject({ desk: { workspaceId: "wks-adam" }, provider: { id: "claude" }, model: { id: "opus" }, effort: { id: "max" }, mode: { id: "default" },
      topic: "Route review", title: "LSA • ADHOC • Route review", problem: null, fromProfile: true });
    expect(resolveDraft(OPTIONS, { topic: "Route review", provider: "opencode" })).toMatchObject({ model: { id: "big" }, effort: null, mode: null, fromProfile: false, problem: null });
    expect(resolveDraft({ ...OPTIONS, profile: null }, { topic: "Route review" })).toMatchObject({ provider: { id: "claude" }, effort: { id: "high" }, fromProfile: false });
  });

  it.each([
    [{ ...OPTIONS, enabled: false, disabledReason: "Switched off." }, "Route review", "Switched off."],
    [{ ...OPTIONS, pending: outcome("unknown") }, "Route review", "An earlier creation, “LSA • ADHOC • Route review”, is not settled. Check it first."],
    [{ ...OPTIONS, desks: [] }, "Route review", "No desk on this host has an ADAM workspace."],
    [{ ...OPTIONS, providers: [OPTIONS.providers[1]!] }, "Route review", "No provider is available on this host."],
    [OPTIONS, "Route review", "Provider error: not signed in.", "gemini"],
    [OPTIONS, "ab", "Type a topic of at least 3 characters."],
    [OPTIONS, "A • B", "A topic cannot contain • or ·, which separate title parts."],
  ])("names the reason Create is disabled", (options, topic, problem, provider?: string) => {
    expect(resolveDraft(options, { topic, provider }).problem).toBe(problem);
  });

  it("makes version-4 request IDs", () => {
    const ids = new Set(Array.from({ length: 50 }, newRequestId));
    expect(ids.size).toBe(50);
    for (const id of ids) expect(id).toMatch(/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/);
  });

  it("promises the persona only where it is confirmed", () => {
    expect(personaLine("supported", "Claude")).toContain("Claude and Codex apply it");
    expect(personaLine("unverified", "OpenCode")).toBe("Persona delivery unverified: OpenCode is not confirmed to apply a system prompt.");
  });
});

describe("New LSA form", () => {
  function form(options: CreateOptionsState = { status: "ready", options: OPTIONS }, props: Partial<Parameters<typeof NewLsaForm>[0]> = {}) {
    const onRefresh = vi.fn(), onOpen = vi.fn(), onCancel = vi.fn();
    const all = { colors: COLORS, compact: false, options, onRefresh, canOpen: true, listed: () => true, onOpen, onCancel, ...props };
    let tree: Element[] = [];
    const render = () => { model.cursor = 0; model.effect = 0; tree = elements(NewLsaForm(all)); return tree; };
    render();
    const type = (topic: string) => { (byTestId(tree, "kontor-new-lsa-topic")!.props.onChangeText as (value: string) => void)(topic); render(); };
    const press = (id: string) => { ((byTestId(tree, id) ?? action(tree, id))!.props.onPress as () => void)(); render(); };
    return { render, type, press, onRefresh, onOpen, onCancel, setOptions: (options: CreateOptionsState) => { all.options = options; render(); }, get tree() { return tree; } };
  }

  it.each([
    [{ status: "loading" } as const, "Reading this host's desks and providers…"],
    [{ status: "offline" } as const, "Host offline. Nothing can be created until it reconnects."],
  ])("waits truthfully while options are %s", (state, message) => {
    expect(text(form(state).tree)).not.toContain("Create LSA");
    expect(text(form(state).tree)).toBe(message);
  });

  it("offers Try again when the options read failed", () => {
    const f = form({ status: "error", message: "timeout" });
    f.press("Try again");
    expect(f.onRefresh).toHaveBeenCalledTimes(1);
  });

  it("disables Create with a visible, announced reason until the topic is valid", () => {
    const f = form();
    const create = () => byTestId(f.tree, "kontor-new-lsa-create")!;
    expect(create().props).toMatchObject({ disabled: true, accessibilityState: { disabled: true }, accessibilityHint: "Type a topic of at least 3 characters." });
    expect(byTestId(f.tree, "kontor-new-lsa-problem")?.props.children).toBe("Type a topic of at least 3 characters.");
    f.type("Route review");
    expect(create().props).toMatchObject({ disabled: false, accessibilityLabel: "Create LSA • ADHOC • Route review", accessibilityHint: undefined });
    expect(text(f.tree)).toContain("Title: LSA • ADHOC • Route review.");
  });

  it("offers each choice as a radio with its state, and keeps an unavailable provider visible with its reason", () => {
    const f = form();
    const radios = f.tree.filter((node) => node.props.accessibilityRole === "radio");
    expect(f.tree.filter((node) => node.props.accessibilityRole === "radiogroup").map((node) => node.props.accessibilityLabel)).toEqual(["Desk", "Provider", "Model", "Effort", "Mode"]);
    expect(radios.find((node) => node.props.accessibilityLabel === "DESK • ADAM / ADAM")?.props.accessibilityState).toEqual({ checked: true, disabled: false });
    expect(radios.find((node) => node.props.accessibilityLabel === "Gemini, Provider error: not signed in.")?.props).toMatchObject({ disabled: true, accessibilityState: { checked: false, disabled: true } });
    expect(radios.every((node) => (node.props.style as { minHeight: number }).minHeight >= 44)).toBe(true);
  });

  it("warns about a dangerous mode before Create", () => {
    const f = form();
    (f.tree.find((node) => node.props.accessibilityLabel === "Bypass ⚠")!.props.onPress as () => void)(); f.render();
    expect(byTestId(f.tree, "kontor-new-lsa-danger")?.props.children).toEqual(["⚠ ", "Bypass", " is a dangerous permission mode on this host."]);
  });

  it("sends one request, waits for the host, and offers Open only for a listed, verified session", async () => {
    const f = form();
    let answer: (value: CreateOutcome) => void = () => {};
    model.rpc["sessions.create"]!.mockImplementation(() => new Promise((resolve) => { answer = resolve; }));
    f.type(" Route   review ");
    f.press("kontor-new-lsa-create");
    expect(model.rpc["sessions.create"]).toHaveBeenCalledWith({ requestId: expect.stringMatching(/^[0-9a-f-]{36}$/), topic: "Route review", workspaceId: "wks-adam", provider: "claude", model: "opus", effort: "max", modeId: "default" });
    expect(text(f.tree)).toContain("Nothing is reported until the host answers.");
    expect(byTestId(f.tree, "kontor-new-lsa-create")).toBeUndefined();
    answer(outcome("verified")); await settle(); f.render();
    expect(byTestId(f.tree, "kontor-new-lsa-verified")?.props).toMatchObject({ accessibilityLiveRegion: "polite" });
    expect(text(f.tree)).toContain("Created and verified");
    f.press("Open session");
    expect(f.onOpen).toHaveBeenCalledWith("agent-new");
    expect(f.onRefresh).toHaveBeenCalled();
  });

  it("does not offer Open before the session list shows the new session", async () => {
    model.rpc["sessions.create"]!.mockResolvedValue(outcome("differences", { differences: ["Model reads sonnet."] }));
    const f = form(undefined, { listed: () => false });
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(text(f.tree)).toContain("Created with differences");
    expect(text(f.tree)).toContain("Model reads sonnet.");
    expect(text(f.tree)).toContain("Open is offered once the session list shows it.");
    expect(action(f.tree, "Open session")).toBeUndefined();
  });

  it("ignores a second submit before React has rendered the busy state", async () => {
    model.rpc["sessions.create"]!.mockResolvedValue(outcome("verified"));
    const f = form();
    f.type("Route review");
    const submit = byTestId(f.tree, "kontor-new-lsa-create")!.props.onPress as () => void;
    submit(); submit();
    expect(model.rpc["sessions.create"]).toHaveBeenCalledTimes(1);
    await settle(); f.render();
    expect(text(f.tree)).toContain("Created and verified");
  });

  it("keeps the confirmed result visible while options refresh fails or the host disconnects", async () => {
    model.rpc["sessions.create"]!.mockResolvedValue(outcome("verified"));
    const f = form();
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    for (const options of [{ status: "loading" }, { status: "error", message: "read failed" }, { status: "offline" }] as const) {
      f.setOptions(options);
      expect(text(f.tree)).toContain("Created and verified");
      expect(text(f.tree)).not.toContain("Nothing was created");
    }
  });

  it("turns an unknown outcome into a Check, never a retry", async () => {
    model.rpc["sessions.create"]!.mockResolvedValue(outcome("unknown"));
    model.rpc["sessions.create-check"]!.mockResolvedValue(outcome("unknown", { message: "No session is visible yet. Creation may still complete." }));
    const f = form();
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(text(f.tree)).toContain("Outcome unknown");
    expect(action(f.tree, "Back to the form")).toBeUndefined();
    f.press("Check"); await settle(); f.render();
    expect(model.rpc["sessions.create-check"]).toHaveBeenCalledWith({ requestId: outcome("unknown").requestId });
    expect(text(f.tree)).toContain("Outcome unknown");
    expect(text(f.tree)).toContain("Creation may still complete");
    expect(model.rpc["sessions.create"]).toHaveBeenCalledTimes(1);
  });

  it("treats a lost reply as unknown with Check, using the same request", async () => {
    model.rpc["sessions.create"]!.mockRejectedValue(new Error("socket closed"));
    model.rpc["sessions.create-check"]!.mockResolvedValue(outcome("verified"));
    const f = form();
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(byTestId(f.tree, "kontor-new-lsa-lost")).toBeDefined();
    expect(action(f.tree, "Back to the form")).toBeUndefined();
    expect(text(f.tree)).toContain("may or may not have reached the host");
    const requestId = model.rpc["sessions.create"]!.mock.calls[0]![0].requestId;
    f.press("Check"); await settle(); f.render();
    expect(model.rpc["sessions.create-check"]).toHaveBeenCalledWith({ requestId });
    expect(text(f.tree)).toContain("Created and verified");
  });

  it("returns a refusal to the form with the draft kept", async () => {
    model.rpc["sessions.create"]!.mockResolvedValue(outcome("refused", { message: "Mode plan is not offered by Claude. Nothing was created." }));
    const f = form();
    f.type("Route review"); f.press("kontor-new-lsa-create"); await settle(); f.render();
    expect(text(f.tree)).toContain("Not created");
    f.press("Back to the form");
    expect(byTestId(f.tree, "kontor-new-lsa-topic")?.props.value).toBe("Route review");
  });

  it("shows an unsettled earlier creation with Check instead of the form", () => {
    const f = form({ status: "ready", options: { ...OPTIONS, pending: outcome("unknown") } });
    expect(byTestId(f.tree, "kontor-new-lsa-topic")).toBeUndefined();
    expect(byTestId(f.tree, "kontor-new-lsa-unknown")).toBeDefined();
    expect(action(f.tree, "Check")).toBeDefined();
  });
});
