import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const help = vi.hoisted(() => ({ hold: vi.fn(), dismiss: vi.fn(), texts: [] as string[] }));
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
vi.mock("../client/control-help", () => ({ useActionHelp: (text: string) => { help.texts.push(text); return { bindings: { "aria-describedby": "help" }, hold: help.hold, dismiss: help.dismiss }; } }));

import { ProviderStateLine, SelectActionBar, SelectActionButton, providerStateText, selectActions, type Owner, type OwnerAnswer, type OwnerReply, type OwnerRequest, type Owners, type RefusalView, type SelectedSession } from "../client/session-select-actions";

type Element = { type?: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk) : [node as Element, ...walk((node as Element).props.children)];
const colors = { foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#fafafa", surface2: "#eee", border: "#ccc", accent: "#20744A", accentForeground: "#fff", statusWarning: "#c80", statusDanger: "#d00" } as unknown as PluginTheme["colors"];
const draw = (element: Element) => (element.type as (props: Record<string, unknown>) => Element)(element.props);
const texts = (tree: unknown) => walk(tree).filter((element) => element.type === "Text").map((element) => [element.props.children].flat().join(""));
const session = (id: string, patch: Partial<SelectedSession> = {}): SelectedSession => ({ id, title: `Session ${id}`, status: "idle", archivedAt: null, ...patch });
const yes = (agentId: string, patch: Partial<OwnerAnswer> = {}) => ({ hostId: "host-a", agentId, revision: `rev-${agentId}`, supported: true, consequence: `Consequence for ${agentId}.`, ...patch }) as OwnerAnswer;
const no = (agentId: string, reason: string) => ({ hostId: "host-a", agentId, revision: `rev-${agentId}`, supported: false, reason }) as OwnerAnswer;
const run = vi.fn<(requests: readonly OwnerRequest[]) => Promise<readonly OwnerReply[]>>();
const lookup = (...answers: OwnerAnswer[]): Owner => ({ answer: (id: string) => answers.find((answer) => answer.agentId === id), run });
const only = (answer: OwnerAnswer): Owner => ({ answer: () => answer, run });
const ARCHIVED = "2026-10-20T21:31:50Z";

beforeEach(() => { harness.fresh(); run.mockReset(); help.hold.mockReset(); help.dismiss.mockReset(); help.texts.length = 0; });

describe("selection actions: availability comes only from the owner's answer", () => {
  it("lets any native session be selected, archived and running included, and grants nothing for being selected", () => {
    for (const status of ["initializing", "idle", "running", "error", "closed"] as const) for (const archivedAt of [null, ARCHIVED]) {
      const states = selectActions("host-a", [session("a", { status, archivedAt })], {});
      expect(states.open).toMatchObject({ enabled: true, ids: ["a"] });
      expect(states.resume.enabled).toBe(false);
      expect(states.takeOver.enabled).toBe(false);
    }
  });

  it("names why each action waits when nothing is selected or several are", () => {
    const none = selectActions("host-a", [], { resume: lookup(), takeOver: lookup() });
    for (const state of Object.values(none)) expect(state).toEqual({ enabled: false, line: "Select a session first." });
    const two = selectActions("host-a", [session("a"), session("b")], { takeOver: lookup(yes("a"), yes("b")) });
    expect(two.open).toEqual({ enabled: false, line: "Select one session to open." });
    expect(two.takeOver).toEqual({ enabled: false, line: "Take over needs exactly one selected session." });
  });

  it("keeps Take over unavailable with an exact reason while no qualified owner exists", () => {
    expect(selectActions("host-a", [session("a", { status: "error" })], {}).takeOver)
      .toEqual({ enabled: false, line: "Take over needs a qualified handoff profile, and none is available yet." });
    expect(selectActions("host-a", [session("a", { status: "error" })], { takeOver: lookup() }).takeOver)
      .toEqual({ enabled: false, line: "Checking whether it can be taken over." });
  });

  it("never derives Take over support from a role, scope or title, for any profile", () => {
    // Synthetic roles and placeholder keys only.
    const titles = ["LSA • DEMO-1 • Lead", "TPM • DEMO-1 • Coordinator", "JUDGE • DEMO-2 • Review", "SWE • DEMO-3 • Delivery", "LSA • ADHOC • Discussion", "Untitled"];
    for (const title of titles) for (const status of ["idle", "error", "closed"] as const) {
      const one = [session("a", { title, status })];
      expect(selectActions("host-a", one, {}).takeOver.enabled).toBe(false);
      expect(selectActions("host-a", one, { takeOver: lookup() }).takeOver.enabled).toBe(false);
      expect(selectActions("host-a", one, { takeOver: lookup(no("a", "Take over is not supported for this review role yet.")) }).takeOver)
        .toEqual({ enabled: false, line: "Take over is not supported for this review role yet." });
      expect(selectActions("host-a", one, { takeOver: lookup(yes("a")) }).takeOver).toMatchObject({ enabled: true, line: "Consequence for a.", ids: ["a"] });
    }
  });

  it("refuses running and starting sessions in v1 even when an owner answers supported", () => {
    expect(selectActions("host-a", [session("a", { status: "running" })], { takeOver: lookup(yes("a")) }).takeOver)
      .toEqual({ enabled: false, line: "This session is running; v1 cannot take it over. Stop it in the session or archive it first." });
    expect(selectActions("host-a", [session("a", { status: "initializing" })], { takeOver: lookup(yes("a")) }).takeOver)
      .toEqual({ enabled: false, line: "This session is starting; v1 cannot take it over." });
  });

  it("counts an answer only for this host and this exact session", () => {
    const one = [session("a")];
    expect(selectActions("host-a", one, { takeOver: only(yes("a", { hostId: "host-b" })) }).takeOver.enabled).toBe(false);
    expect(selectActions("host-a", one, { takeOver: only(yes("b")) }).takeOver.enabled).toBe(false);
    expect(selectActions("host-a", one, { resume: only(yes("a", { hostId: "host-b" })) }).resume).toEqual({ enabled: false, line: "Checking whether it can resume." });
  });

  it("never offers Resume for an archived session, whatever the owner answers", () => {
    expect(selectActions("host-a", [session("a", { archivedAt: ARCHIVED, status: "error" })], { resume: lookup(yes("a")) }).resume)
      .toEqual({ enabled: false, line: "Archived: Resume never revives an archived session." });
  });

  it("resumes only the owner's eligible sessions and says how many are not", () => {
    const selected = [session("a", { status: "error" }), session("b", { status: "running" }), session("c", { status: "error", archivedAt: ARCHIVED })];
    const owners: Owners = { resume: lookup(yes("a"), no("b", "Running: nothing to retry.")) };
    expect(selectActions("host-a", selected, owners).resume).toMatchObject({ enabled: true, ids: ["a"], line: expect.stringMatching(/^Resume 1 · 2 not eligible\./) });
    expect(selectActions("host-a", [selected[1]!], owners).resume).toEqual({ enabled: false, line: "Running: nothing to retry." });
    expect(selectActions("host-a", selected.slice(1), owners).resume).toEqual({ enabled: false, line: "None of the selected sessions can resume: Running: nothing to retry." });
    expect(selectActions("host-a", [selected[0]!], {}).resume).toEqual({ enabled: false, line: "Resume is not available here." });
  });

  it("archives only unarchived sessions through the existing review", () => {
    const states = selectActions("host-a", [session("a"), session("b", { archivedAt: ARCHIVED })], {});
    expect(states.archive).toMatchObject({ enabled: true, ids: ["a"], line: "Review archive of 1 session, then confirm · 1 already archived." });
    expect(selectActions("host-a", [session("b", { archivedAt: ARCHIVED })], {}).archive).toEqual({ enabled: false, line: "Already archived." });
  });
});

describe("provider state line", () => {
  const refusal = (patch: Partial<RefusalView> = {}): RefusalView => ({ kind: "quota", failureAt: "2026-10-20T21:30:52.000Z", observedAt: "2026-10-24T16:10:00.000Z",
    read: "fresh", state: "current", recovery: "not-confirmed", ...patch });

  it("shows the failure time from the refusal, the read time apart from it, and that recovery is not confirmed", () => {
    const text = providerStateText(refusal(), "UTC")!;
    expect(text).toMatch(/^Last refusal \(quota\): .*21:30:52.* \(observed .*16:10:00.*\) · Recovery not confirmed$/);
    expect(providerStateText(refusal({ failureAt: "unknown" }), "UTC")).toMatch(/^Last refusal \(quota\): time unknown \(observed .*16:10:00.*\)/);
  });

  it("keeps an archived or stale refusal as unrecovered history and never says clear or available", () => {
    for (const patch of [{ state: "history" as const }, { read: "stale" as const, recovery: "confirmed" as const }, { read: "unreadable" as const, recovery: "confirmed" as const }]) {
      const text = providerStateText(refusal(patch), "UTC")!;
      expect(text).toContain("Recovery not confirmed");
      expect(text).not.toMatch(/clear|available/i);
    }
    expect(providerStateText(refusal({ recovery: "confirmed" }), "UTC")).toMatch(/· Recovery confirmed$/);
  });

  it("says unknown, not clear, when the read failed, and shows nothing without a refusal", () => {
    expect(providerStateText("unreadable", "UTC")).toBe("Provider state unknown (read failed)");
    expect(providerStateText(null, "UTC")).toBeNull();
    expect(ProviderStateLine({ refusal: null, zone: "UTC", colors })).toBeNull();
  });

  it("is visible text, danger for a current refusal and warning for history", () => {
    const current = ProviderStateLine({ refusal: refusal(), zone: "UTC", colors }) as Element;
    expect(texts(current)[0]).toContain("Recovery not confirmed");
    expect(walk(current).find((node) => node.type === "Text")!.props.style).toMatchObject({ color: colors.statusDanger });
    const history = ProviderStateLine({ refusal: refusal({ state: "history" }), zone: "UTC", colors }) as Element;
    expect(walk(history).find((node) => node.type === "Text")!.props.style).toMatchObject({ color: colors.statusWarning });
  });
});

describe("select action button", () => {
  const button = (enabled: boolean, onPress = vi.fn()) => SelectActionButton({ testID: "t", icon: "ArrowRightLeft", name: "Take over session", line: "Reason.", enabled, colors, onPress }) as Element;

  it("is a named 44pt icon action with help, and no visible text", () => {
    const element = button(true);
    expect(element.props).toMatchObject({ accessibilityRole: "button", accessibilityLabel: "Take over session", accessibilityHint: "Reason.", "aria-describedby": "help" });
    expect((element.props.style as (state: object) => object)({ pressed: false })).toMatchObject({ width: 44, height: 44 });
    expect(walk(element).some((node) => node.type === "Text")).toBe(false);
    expect(help.texts).toContain("Take over session. Reason.");
  });

  it("stays focusable when unavailable, and a tap shows its reason and runs nothing", () => {
    const onPress = vi.fn(), element = button(false, onPress);
    expect(element.props.disabled).toBeUndefined();
    expect(element.props).toMatchObject({ "aria-disabled": true, accessibilityState: { disabled: true } });
    (element.props.onPress as () => void)();
    expect(help.hold).toHaveBeenCalledTimes(1);
    expect(onPress).not.toHaveBeenCalled();
  });

  it("runs an available action once and closes its help", () => {
    const onPress = vi.fn(), element = button(true, onPress);
    (element.props.onPress as () => void)();
    expect(help.dismiss).toHaveBeenCalledTimes(1);
    expect(onPress).toHaveBeenCalledTimes(1);
    expect(help.hold).not.toHaveBeenCalled();
  });
});

describe("select action bar", () => {
  const setup = (patch: { selected?: SelectedSession[]; owners?: Owners; hostId?: string; blocked?: string; refusal?: RefusalView | "unreadable" | null } = {}) => {
    const props = { hostId: "host-a", selected: [session("a", { status: "error" })], owners: {} as Owners, zone: "UTC", colors,
      onOpen: vi.fn(), onArchive: vi.fn(), onClear: vi.fn(), ...patch };
    const render = () => harness.current.render(() => SelectActionBar(props)) as Element;
    return { props, render };
  };
  const action = (tree: Element, name: string) => walk(tree).find((node) => node.props.testID === `sessions-select-action-${name}`)!;
  const press = (element: Element) => (element.props.onPress as () => void)();
  const textButton = (tree: Element, label: string) => walk(tree).find((node) => node.props.label === label)!;

  it("shows the count, four actions and Clear, with Take over unavailable and its reason", () => {
    const tree = setup().render();
    expect(tree.props).toMatchObject({ role: "toolbar", accessibilityLabel: "Selected session actions" });
    expect(texts(tree)).toContain("1 selected");
    expect(["open", "archive", "resume", "takeOver", "clear"].map((name) => action(tree, name).props.name))
      .toEqual(["Open session", "Archive sessions", "Resume session", "Take over session", "Clear selection"]);
    expect(action(tree, "takeOver").props).toMatchObject({ enabled: false, line: "Take over needs a qualified handoff profile, and none is available yet." });
  });

  it("shows why nothing can start while blocked, and runs nothing", () => {
    const { props, render } = setup({ owners: { takeOver: lookup(yes("a")) }, blocked: "Wait for the archive review to close." });
    const tree = render();
    for (const name of ["open", "archive", "resume", "takeOver", "clear"]) {
      expect(action(tree, name).props).toMatchObject({ enabled: false, line: "Wait for the archive review to close." });
      press(draw(action(tree, name)));
    }
    expect(props.onOpen).not.toHaveBeenCalled(); expect(props.onArchive).not.toHaveBeenCalled(); expect(props.onClear).not.toHaveBeenCalled();
    expect(help.hold).toHaveBeenCalledTimes(5);
    expect(walk(render()).some((node) => node.props.testID === "sessions-select-confirm")).toBe(false);
  });

  it("opens and archives through the existing paths, and never through Take over", () => {
    const { props, render } = setup({ selected: [session("a"), session("b", { archivedAt: ARCHIVED })] });
    const tree = render();
    press(action(tree, "archive")); press(action(tree, "takeOver")); press(action(tree, "clear"));
    expect(props.onArchive).toHaveBeenCalledExactlyOnceWith(["a"]);
    expect(props.onClear).toHaveBeenCalledTimes(1);
    expect(props.onOpen).not.toHaveBeenCalled();
    const single = setup({ selected: [session("z")] });
    press(action(single.render(), "open"));
    expect(single.props.onOpen).toHaveBeenCalledExactlyOnceWith("z");
    const archiveRef = { current: null };
    const anchored = harness.current.render(() => SelectActionBar({ ...single.props, archiveRef })) as Element;
    expect(action(anchored, "archive").props.ref).toBe(archiveRef);
    expect(action(anchored, "takeOver").props.ref).toBeUndefined();
  });

  it("asks once with the owner's consequence and sends nothing before Confirm", async () => {
    const { props, render } = setup({ owners: { takeOver: lookup(yes("a")) } });
    run.mockResolvedValue([{ hostId: "host-a", agentId: "a", text: "Taken over." }]);
    press(action(render(), "takeOver"));
    const confirm = render();
    expect(texts(confirm)).toEqual(expect.arrayContaining(["Take over Session a?", "Consequence for a."]));
    expect(run).not.toHaveBeenCalled();
    press(textButton(confirm, "Cancel"));
    expect(walk(render()).some((node) => node.props.testID === "sessions-select-confirm")).toBe(false);
    expect(run).not.toHaveBeenCalled();
    press(action(render(), "takeOver"));
    press(textButton(render(), "Take over"));
    expect(run).toHaveBeenCalledExactlyOnceWith([{ hostId: "host-a", agentId: "a", revision: "rev-a" }]);
    await Promise.resolve(); await Promise.resolve();
    const done = render();
    expect(walk(done).find((node) => node.props.testID === "sessions-select-result")!.props).toMatchObject({ role: "status", children: "Taken over." });
    expect(walk(done).some((node) => node.props.testID === "sessions-select-confirm")).toBe(false);
  });

  it("never shows a reply for another session or host, and keeps an unknown outcome unknown", async () => {
    const { props, render } = setup({ owners: { takeOver: lookup(yes("a")) } });
    run.mockResolvedValueOnce([{ hostId: "host-b", agentId: "a", text: "Wrong host." }, { hostId: "host-a", agentId: "b", text: "Wrong session." }]);
    press(action(render(), "takeOver")); press(textButton(render(), "Take over"));
    await Promise.resolve(); await Promise.resolve();
    expect(texts(render())).toContain("No result reported. Check the session before trying again.");
    run.mockRejectedValueOnce(new Error("reply lost"));
    press(action(render(), "takeOver")); press(textButton(render(), "Take over"));
    await Promise.resolve(); await Promise.resolve();
    expect(texts(render())).toContain("Result unknown: reply lost. Check the session before trying again.");
  });

  it("drops an open confirmation and a late reply when the selection changes", async () => {
    let reply!: (value: readonly OwnerReply[]) => void;
    const { props, render } = setup({ owners: { takeOver: lookup(yes("a")) } });
    run.mockImplementation(() => new Promise((resolve) => { reply = resolve; }));
    press(action(render(), "takeOver")); press(textButton(render(), "Take over"));
    expect(textButton(render(), "Sending…").props.disabled).toBe(true);
    props.selected = [session("b")];
    render();
    reply([{ hostId: "host-a", agentId: "a", text: "Taken over." }]);
    await Promise.resolve(); await Promise.resolve();
    const tree = render();
    expect(texts(tree)).not.toContain("Taken over.");
    expect(walk(tree).some((node) => node.props.testID === "sessions-select-confirm")).toBe(false);
  });

  it("resumes the owner's eligible sessions with their exact revisions", () => {
    const selected = [session("a", { status: "error" }), session("b", { status: "running" })];
    const { props, render } = setup({ selected, owners: { resume: lookup(yes("a"), no("b", "Running: nothing to retry.")) } });
    press(action(render(), "resume"));
    expect(texts(render())).toEqual(expect.arrayContaining(["Resume Session a?"]));
    press(textButton(render(), "Resume"));
    expect(run).toHaveBeenCalledExactlyOnceWith([{ hostId: "host-a", agentId: "a", revision: "rev-a" }]);
  });

  it("shows the provider line only for one selected session with an exact read", () => {
    const line = (tree: Element) => walk(tree).find((node) => node.type === ProviderStateLine);
    expect(line(setup({ refusal: "unreadable" }).render())!.props.refusal).toBe("unreadable");
    expect(line(setup().render())).toBeUndefined();
    expect(line(setup({ refusal: "unreadable", selected: [session("a"), session("b")] }).render())).toBeUndefined();
  });

  it("shows provider unknown text when rendered", () => {
    expect(texts(draw(walk(setup({ refusal: "unreadable" }).render()).find((node) => node.type === ProviderStateLine)!))).toEqual(["Provider state unknown (read failed)"]);
  });
});
