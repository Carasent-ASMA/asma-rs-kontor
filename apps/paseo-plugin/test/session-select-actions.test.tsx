import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const help = vi.hoisted(() => ({ hold: vi.fn(), dismiss: vi.fn(), texts: [] as string[] }));
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
vi.mock("../client/anchored-menu", () => ({ AnchoredMenu: "AnchoredMenu" }));
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
      expect(states.archive.enabled).toBe(archivedAt === null);
      expect(states.takeOver.enabled).toBe(false);
      expect(states.retry.enabled).toBe(false);
    }
  });

  it("names why each action waits when nothing is selected, and never takes over or retries a batch", () => {
    const none = selectActions("host-a", [], { takeOver: lookup(), retry: lookup() });
    for (const state of Object.values(none)) expect(state).toEqual({ enabled: false, line: "Select a session first." });
    const two = selectActions("host-a", [session("a"), session("b")], { takeOver: lookup(yes("a"), yes("b")), retry: lookup(yes("a"), yes("b")) });
    expect(two.takeOver).toEqual({ enabled: false, line: "Take over needs exactly one selected session." });
    expect(two.retry).toEqual({ enabled: false, line: "Retry in this session needs exactly one selected session." });
  });

  it("keeps Take over unavailable with an exact reason while no owner can pick a route from the seat's chain", () => {
    expect(selectActions("host-a", [session("a", { status: "error" })], {}).takeOver)
      .toEqual({ enabled: false, line: "Take over is unavailable: no owner here can pick a route from this seat's model chain." });
    expect(selectActions("host-a", [session("a", { status: "error" })], { takeOver: lookup() }).takeOver)
      .toEqual({ enabled: false, line: "Checking whether it can be taken over." });
    expect(selectActions("host-a", [session("a", { status: "error" })], { takeOver: lookup(no("a", "No model chain is bound to this seat.")) }).takeOver)
      .toEqual({ enabled: false, line: "No model chain is bound to this seat." });
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
    expect(selectActions("host-a", one, { retry: only(yes("a", { hostId: "host-b" })) }).retry).toEqual({ enabled: false, line: "Checking whether it can retry in this session." });
  });

  it("offers Retry in this session only on the owner's answer, and never for an archived session", () => {
    const one = [session("a", { status: "error" })];
    expect(selectActions("host-a", one, {}).retry).toEqual({ enabled: false, line: "Retry in this session is not available here." });
    expect(selectActions("host-a", one, { retry: lookup(no("a", "Waiting for permission/input.")) }).retry).toEqual({ enabled: false, line: "Waiting for permission/input." });
    expect(selectActions("host-a", one, { retry: lookup(yes("a")) }).retry).toMatchObject({ enabled: true, line: "Consequence for a.", ids: ["a"] });
    expect(selectActions("host-a", [session("a", { archivedAt: ARCHIVED, status: "error" })], { retry: lookup(yes("a")) }).retry)
      .toEqual({ enabled: false, line: "Archived: Retry never revives an archived session." });
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

  it("tells assistive tech whether the menu it opens is expanded", () => {
    const element = SelectActionButton({ testID: "t", icon: "Ellipsis", name: "Actions", line: "Line.", enabled: true, expanded: true, colors, onPress: vi.fn() }) as Element;
    expect(element.props).toMatchObject({ "aria-expanded": true, accessibilityState: { disabled: false, expanded: true } });
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
  const anchorRef = { current: null }, root = { current: null };
  const setup = (patch: { selected?: SelectedSession[]; owners?: Owners; hostId?: string; blocked?: string; refusal?: RefusalView | "unreadable" | null; selectAll?: unknown } = {}) => {
    const props = { hostId: "host-a", selected: [session("a", { status: "error" })], owners: {} as Owners, zone: "UTC", colors, anchorRef, root, rootSize: { width: 800, height: 600 },
      onArchive: vi.fn(), onClear: vi.fn(), ...patch } as Parameters<typeof SelectActionBar>[0];
    const render = () => harness.current.render(() => SelectActionBar(props)) as Element;
    return { props, render };
  };
  const byTestID = (tree: Element, testID: string) => walk(tree).find((node) => node.props.testID === testID);
  const trigger = (tree: Element) => byTestID(tree, "sessions-select-menu-trigger")!;
  const menu = (tree: Element) => walk(tree).find((node) => node.type === "AnchoredMenu");
  /** The drawn menu row for an action, or undefined while the menu or that row is not shown. */
  const item = (tree: Element, name: string) => { const row = walk(menu(tree)?.props.children).find((node) => node.props.action === name); return row && draw(row); };
  const press = (element: Element) => (element.props.onPress as () => void)();
  const textButton = (tree: Element, label: string) => walk(tree).find((node) => node.props.label === label)!;
  const choose = (render: () => Element, name: string) => { press(trigger(render())); press(item(render(), name)!); };
  const confirmShown = (tree: Element) => !!byTestID(tree, "sessions-select-confirm");

  it("is one composite Select: the count, Select all, one Actions menu and Clear, with no Resume", () => {
    const tree = setup({ selectAll: { type: "SelectAll", props: { testID: "sessions-select-all" } } }).render();
    expect(tree.props).toMatchObject({ role: "toolbar", accessibilityLabel: "Selected session actions" });
    expect(texts(tree)).toContain("1 selected");
    expect(byTestID(tree, "sessions-select-all")).toBeDefined();
    expect(trigger(tree).props).toMatchObject({ icon: "Ellipsis", name: "Actions for 1 selected session", line: "Archive or take over the selection.", enabled: true, expanded: false, ref: anchorRef });
    expect(byTestID(tree, "sessions-select-action-clear")).toBeDefined();
    expect(menu(tree)).toBeUndefined();
    expect(JSON.stringify(walk(tree).map((node) => node.props.name ?? node.props.label ?? ""))).not.toMatch(/Resume/);
  });

  it("opens one menu with Archive and Take over, Take over unavailable with its exact reason, and no Retry without an owner", () => {
    const { props, render } = setup();
    press(trigger(render()));
    const tree = render(), open = menu(tree)!;
    expect(trigger(tree).props.expanded).toBe(true);
    expect(open.props).toMatchObject({ testID: "sessions-select-menu", label: "Actions for 1 selected session", anchor: anchorRef, root, placement: { align: "end", width: 320 } });
    expect(item(tree, "archive")!.props).toMatchObject({ role: "menuitem", accessibilityLabel: "Archive…", "aria-disabled": false });
    const takeOver = item(tree, "takeOver")!;
    expect(takeOver.props).toMatchObject({ role: "menuitem", accessibilityLabel: "Take over…", "aria-disabled": true, accessibilityState: { disabled: true },
      accessibilityHint: "Take over is unavailable: no owner here can pick a route from this seat's model chain." });
    expect(texts(takeOver)).toContain("Take over is unavailable: no owner here can pick a route from this seat's model chain.");
    expect((takeOver.props.style as (state: object) => object)({ pressed: false })).toMatchObject({ minHeight: 44 });
    press(takeOver);
    expect(confirmShown(render())).toBe(false);
    expect(menu(render())).toBeDefined();
    expect(item(tree, "retry")).toBeUndefined();
    expect(props.onArchive).not.toHaveBeenCalled();
  });

  it("shows Retry in this session only as a secondary row when its owner says the session can retry", () => {
    for (const [owner, shown] of [[lookup(), false], [lookup(no("a", "Closed or stopped.")), false], [lookup(yes("a")), true]] as const) {
      const { render } = setup({ owners: { retry: owner } });
      press(trigger(render()));
      const row = item(render(), "retry");
      expect(!!row).toBe(shown);
      if (row) expect(texts(row)).toEqual(["Retry in this session…", "Consequence for a."]);
    }
  });

  it("shows why nothing can start while blocked, and opens nothing", () => {
    const { props, render } = setup({ owners: { takeOver: lookup(yes("a")) }, blocked: "Wait for the archive review to close." });
    const tree = render();
    for (const name of ["sessions-select-menu-trigger", "sessions-select-action-clear"]) {
      const button = byTestID(tree, name)!;
      expect(button.props).toMatchObject({ enabled: false, line: "Wait for the archive review to close." });
      press(draw(button));
    }
    expect(help.hold).toHaveBeenCalledTimes(2);
    expect(props.onClear).not.toHaveBeenCalled();
    expect(menu(render())).toBeUndefined();
  });

  it("archives through the existing review and closes the menu", () => {
    const { props, render } = setup({ selected: [session("a"), session("b", { archivedAt: ARCHIVED })] });
    choose(render, "archive");
    expect(props.onArchive).toHaveBeenCalledExactlyOnceWith(["a"]);
    expect(menu(render())).toBeUndefined();
    press(byTestID(render(), "sessions-select-action-clear")!);
    expect(props.onClear).toHaveBeenCalledTimes(1);
  });

  it("asks once with the owner's consequence and sends nothing before Confirm", async () => {
    const { render } = setup({ owners: { takeOver: lookup(yes("a")) } });
    run.mockResolvedValue([{ hostId: "host-a", agentId: "a", text: "Taken over." }]);
    choose(render, "takeOver");
    const confirm = render();
    expect(menu(confirm)).toBeUndefined();
    expect(texts(confirm)).toEqual(expect.arrayContaining(["Take over Session a?", "Consequence for a."]));
    expect(run).not.toHaveBeenCalled();
    press(textButton(confirm, "Cancel"));
    expect(confirmShown(render())).toBe(false);
    expect(run).not.toHaveBeenCalled();
    choose(render, "takeOver");
    press(textButton(render(), "Take over"));
    expect(run).toHaveBeenCalledExactlyOnceWith([{ hostId: "host-a", agentId: "a", revision: "rev-a" }]);
    await Promise.resolve(); await Promise.resolve();
    const done = render();
    expect(byTestID(done, "sessions-select-result")!.props).toMatchObject({ role: "status", children: "Taken over." });
    expect(confirmShown(done)).toBe(false);
  });

  it("retries in this session with its exact revision after its own confirmation", () => {
    const { render } = setup({ owners: { retry: lookup(yes("a")) } });
    choose(render, "retry");
    expect(texts(render())).toEqual(expect.arrayContaining(["Retry Session a in this session?", "Consequence for a."]));
    press(textButton(render(), "Retry"));
    expect(run).toHaveBeenCalledExactlyOnceWith([{ hostId: "host-a", agentId: "a", revision: "rev-a" }]);
  });

  it("never shows a reply for another session or host, and keeps an unknown outcome unknown", async () => {
    const { render } = setup({ owners: { takeOver: lookup(yes("a")) } });
    run.mockResolvedValueOnce([{ hostId: "host-b", agentId: "a", text: "Wrong host." }, { hostId: "host-a", agentId: "b", text: "Wrong session." }]);
    choose(render, "takeOver"); press(textButton(render(), "Take over"));
    await Promise.resolve(); await Promise.resolve();
    expect(texts(render())).toContain("No result reported. Check the session before trying again.");
    run.mockRejectedValueOnce(new Error("reply lost"));
    choose(render, "takeOver"); press(textButton(render(), "Take over"));
    await Promise.resolve(); await Promise.resolve();
    expect(texts(render())).toContain("Result unknown: reply lost. Check the session before trying again.");
  });

  it.each([
    ["the same session starts running", { selected: [session("a", { status: "running" })] }, "This session is running; v1 cannot take it over. Stop it in the session or archive it first."],
    ["the same session starts initializing", { selected: [session("a", { status: "initializing" })] }, "This session is starting; v1 cannot take it over."],
    ["the host goes offline", { blocked: "This host is offline." }, "This host is offline."],
    ["the archive review opens", { blocked: "Close the archive review first." }, "Close the archive review first."],
    ["the owner goes away", { owners: {} }, "Take over is unavailable: no owner here can pick a route from this seat's model chain."],
    ["the owner withdraws its answer", { owners: { takeOver: lookup() } }, "Checking whether it can be taken over."],
    ["the owner now refuses", { owners: { takeOver: lookup(no("a", "No route in this seat's chain is available.")) } }, "No route in this seat's chain is available."],
    ["the owner answers a new revision", { owners: { takeOver: lookup(yes("a", { revision: "rev-a2" })) } }, "This changed after you opened it. Cancel and choose the action again."],
    ["the owner picks another route", { owners: { takeOver: lookup(yes("a", { consequence: "Another route." })) } }, "This changed after you opened it. Cancel and choose the action again."],
  ] as const)("rechecks an open confirmation when %s, shows the current reason and calls no owner", (_case, patch, reason) => {
    const { props, render } = setup({ selected: [session("a")], owners: { takeOver: lookup(yes("a")) } });
    choose(render, "takeOver");
    Object.assign(props, patch);
    const tree = render();
    expect(confirmShown(tree)).toBe(true);
    expect(texts(tree)).toContain(reason);
    expect(textButton(tree, "Take over").props.disabled).toBe(true);
    press(textButton(tree, "Take over"));
    expect(run).not.toHaveBeenCalled();
  });

  it.each([
    ["answers a new revision", yes("a", { revision: "rev-a2" }), "Nothing was sent. This changed after you opened it; choose the action again to review it."],
    ["withdraws its answer", undefined, "Nothing was sent. Checking whether it can be taken over."],
    ["now refuses", no("a", "No route in this seat's chain is available."), "Nothing was sent. No route in this seat's chain is available."],
    ["picks another route", yes("a", { consequence: "Another route." }), "Nothing was sent. This changed after you opened it; choose the action again to review it."],
  ] as const)("asks the owner again at Confirm: when it %s with no redraw, nothing runs and the reason shows at once", (_case, later, line) => {
    let answer: OwnerAnswer | undefined = yes("a");
    const { render } = setup({ selected: [session("a")], owners: { takeOver: { answer: () => answer, run } } });
    choose(render, "takeOver");
    const drawn = render();
    expect(textButton(drawn, "Take over").props.disabled).toBe(false);
    answer = later;
    press(textButton(drawn, "Take over"));
    expect(run).not.toHaveBeenCalled();
    const after = render();
    expect(confirmShown(after)).toBe(false);
    expect(byTestID(after, "sessions-select-result")!.props).toMatchObject({ role: "status", children: line });
  });

  it("keeps several replies for one session unknown rather than picking one", async () => {
    const { render } = setup({ owners: { takeOver: lookup(yes("a")) } });
    run.mockResolvedValueOnce([{ hostId: "host-a", agentId: "a", text: "Taken over." }, { hostId: "host-a", agentId: "a", text: "Refused." }]);
    choose(render, "takeOver"); press(textButton(render(), "Take over"));
    await Promise.resolve(); await Promise.resolve();
    const shown = texts(render());
    expect(shown).toContain("Result unknown: more than one reply for this session. Check the session before trying again.");
    expect(shown).not.toContain("Taken over.");
  });

  it("drops an open menu, an open confirmation and a late reply when the selection changes", async () => {
    let reply!: (value: readonly OwnerReply[]) => void;
    const { props, render } = setup({ owners: { takeOver: lookup(yes("a")) } });
    run.mockImplementation(() => new Promise((resolve) => { reply = resolve; }));
    choose(render, "takeOver"); press(textButton(render(), "Take over"));
    expect(textButton(render(), "Sending…").props.disabled).toBe(true);
    expect(trigger(render()).props.enabled).toBe(false);
    props.selected = [session("b")];
    render();
    reply([{ hostId: "host-a", agentId: "a", text: "Taken over." }]);
    await Promise.resolve(); await Promise.resolve();
    const tree = render();
    expect(texts(tree)).not.toContain("Taken over.");
    expect(confirmShown(tree)).toBe(false);
    press(trigger(tree));
    expect(menu(render())).toBeDefined();
    props.selected = [session("c")];
    render();
    expect(menu(render())).toBeUndefined();
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
