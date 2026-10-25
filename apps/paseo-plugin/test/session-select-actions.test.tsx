import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const help = vi.hoisted(() => ({ hold: vi.fn(), dismiss: vi.fn(), texts: [] as string[] }));
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
vi.mock("../client/anchored-menu", () => ({ AnchoredMenu: "AnchoredMenu" }));
vi.mock("../client/control-help", () => ({ useActionHelp: (text: string) => { help.texts.push(text); return { bindings: { "aria-describedby": "help" }, hold: help.hold, dismiss: help.dismiss }; } }));

import { ProviderStateLine, SelectActionBar, SelectActionButton, providerStateText, selectActions, type OwnerAnswer, type OwnerReply, type OwnerRequest, type Owners, type RefusalView, type RetryOwner, type SelectedSession, type TakeOverOwner } from "../client/session-select-actions";

type Element = { type?: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk) : [node as Element, ...walk((node as Element).props.children)];
const colors = { foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface1: "#fafafa", surface2: "#eee", border: "#ccc", accent: "#20744A", accentForeground: "#fff", statusWarning: "#c80", statusDanger: "#d00" } as unknown as PluginTheme["colors"];
const draw = (element: Element) => (element.type as (props: Record<string, unknown>) => Element)(element.props);
const texts = (tree: unknown) => walk(tree).filter((element) => element.type === "Text").map((element) => [element.props.children].flat().join(""));
const session = (id: string, patch: Partial<SelectedSession> = {}): SelectedSession => ({ id, title: `Session ${id}`, status: "idle", archivedAt: null, ...patch });
const yes = (agentId: string, patch: Partial<OwnerAnswer> = {}) => ({ hostId: "host-a", agentId, reviewId: `review-${agentId}`, revision: `rev-${agentId}`, supported: true, consequence: `Consequence for ${agentId}.`, ...patch }) as OwnerAnswer;
const no = (agentId: string, reason: string) => ({ hostId: "host-a", agentId, reviewId: `review-${agentId}`, revision: `rev-${agentId}`, supported: false, reason }) as OwnerAnswer;
const done = (agentId: string, patch: Partial<OwnerReply> = {}): OwnerReply => ({ hostId: "host-a", agentId, outcome: "succeeded", successorId: `${agentId}-next`, text: "Archived it and started the new session.", ...patch });
const apply = vi.fn<(request: OwnerRequest) => Promise<OwnerReply>>();
const review = vi.fn<(hostId: string, agentId: string) => Promise<OwnerAnswer>>();
/** Take over's owner reviews only when asked. */
const taker = (): TakeOverOwner => ({ review, apply });
/** Retry's owner answers from the row it has already read. */
const lookup = (...answers: OwnerAnswer[]): RetryOwner => ({ answer: (id: string) => answers.find((answer) => answer.agentId === id), apply });
const only = (answer: OwnerAnswer): RetryOwner => ({ answer: () => answer, apply });
const flush = async () => { for (let n = 0; n < 3; n++) await Promise.resolve(); };
const ARCHIVED = "2026-10-20T21:31:50Z";
const NO_TAKEOVER = "Take over is not available yet: no qualified owner on this host can pick its route and start the new session.";
const REVIEW = "Review the route its model chain picks, then confirm.";

beforeEach(() => {
  harness.fresh(); apply.mockReset(); review.mockReset(); help.hold.mockReset(); help.dismiss.mockReset(); help.texts.length = 0;
  review.mockImplementation(async (hostId, agentId) => yes(agentId, { hostId }));
});

describe("selection actions: availability comes only from the owner", () => {
  it("lets any native session be selected, archived and running included, and grants nothing for being selected", () => {
    for (const status of ["initializing", "idle", "running", "error", "closed"] as const) for (const archivedAt of [null, ARCHIVED]) {
      const states = selectActions("host-a", [session("a", { status, archivedAt })], {});
      expect(states.archive.enabled).toBe(archivedAt === null);
      expect(states.takeOver.enabled).toBe(false);
      expect(states.retry.enabled).toBe(false);
    }
  });

  it("names why each action waits when nothing is selected, and never takes over or retries a batch", () => {
    const none = selectActions("host-a", [], { takeOver: taker(), retry: lookup() });
    for (const state of Object.values(none)) expect(state).toEqual({ enabled: false, line: "Select a session first." });
    const two = selectActions("host-a", [session("a"), session("b")], { takeOver: taker(), retry: lookup(yes("a"), yes("b")) });
    expect(two.takeOver).toEqual({ enabled: false, line: "Take over needs exactly one selected session." });
    expect(two.retry).toEqual({ enabled: false, line: "Retry in this session needs exactly one selected session." });
  });

  it("keeps Take over unavailable with an exact reason while no qualified owner can pick a route from the session's chain", () => {
    expect(selectActions("host-a", [session("a", { status: "error" })], {}).takeOver).toEqual({ enabled: false, line: NO_TAKEOVER });
  });

  it("offers an owner's Take over as a review to open, for any role, scope or title, and reviews nothing before it is chosen", () => {
    // Synthetic roles and placeholder keys only.
    const titles = ["LSA • DEMO-1 • Lead", "TPM • DEMO-1 • Coordinator", "JUDGE • DEMO-2 • Review", "SWE • DEMO-3 • Delivery", "LSA • ADHOC • Discussion", "Untitled"];
    for (const title of titles) for (const status of ["idle", "error", "closed"] as const) {
      const one = [session("a", { title, status })];
      expect(selectActions("host-a", one, {}).takeOver.enabled).toBe(false);
      expect(selectActions("host-a", one, { takeOver: taker() }).takeOver).toEqual({ enabled: true, line: REVIEW, ids: ["a"] });
    }
    expect(review).not.toHaveBeenCalled();
  });

  it("refuses running and starting sessions even with an owner", () => {
    expect(selectActions("host-a", [session("a", { status: "running" })], { takeOver: taker() }).takeOver)
      .toEqual({ enabled: false, line: "This session is running. Stop it in the session, then take it over." });
    expect(selectActions("host-a", [session("a", { status: "initializing" })], { takeOver: taker() }).takeOver)
      .toEqual({ enabled: false, line: "This session is starting. Wait until it is idle, then take it over." });
  });

  it("counts a Retry answer only for this host and this exact session", () => {
    const one = [session("a")], checking = { enabled: false, line: "Checking whether it can retry in this session." };
    expect(selectActions("host-a", one, { retry: only(yes("a", { hostId: "host-b" })) }).retry).toEqual(checking);
    expect(selectActions("host-a", one, { retry: only(yes("b")) }).retry).toEqual(checking);
  });

  it("offers Retry in this session only on the owner's answer, and never for an archived session", () => {
    const one = [session("a", { status: "error" })];
    expect(selectActions("host-a", one, {}).retry).toEqual({ enabled: false, line: "Retry in this session is not available here." });
    expect(selectActions("host-a", one, { retry: lookup() }).retry).toEqual({ enabled: false, line: "Checking whether it can retry in this session." });
    expect(selectActions("host-a", one, { retry: lookup(no("a", "Waiting for permission/input.")) }).retry).toEqual({ enabled: false, line: "Waiting for permission/input." });
    expect(selectActions("host-a", one, { retry: lookup(yes("a")) }).retry).toEqual({ enabled: true, line: "Consequence for a.", ids: ["a"] });
    expect(selectActions("host-a", [session("a", { archivedAt: ARCHIVED, status: "error" })], { retry: lookup(yes("a")) }).retry)
      .toEqual({ enabled: false, line: "Archived: Retry never revives an archived session." });
  });

  it("archives only unarchived sessions through the existing review", () => {
    const states = selectActions("host-a", [session("a"), session("b", { archivedAt: ARCHIVED })], {});
    expect(states.archive).toEqual({ enabled: true, ids: ["a"], line: "Review archive of 1 session, then confirm · 1 already archived." });
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
      accessibilityHint: "Take over is not available yet: no qualified owner on this host can pick its route and start the new session." });
    expect(texts(takeOver)).toContain("Take over is not available yet: no qualified owner on this host can pick its route and start the new session.");
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
    const { props, render } = setup({ owners: { takeOver: taker() }, blocked: "Wait for the archive review to close." });
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

  const result = (tree: Element) => byTestID(tree, "sessions-select-result")?.props.children;
  /** Chooses Take over, lets its review answer, and confirms what it shows. */
  const takeOver = async (render: () => Element) => { choose(render, "takeOver"); await flush(); press(textButton(render(), "Take over")); await flush(); };

  it("reviews only when Take over is chosen, then applies once exactly what the confirmation showed", async () => {
    const { render } = setup({ owners: { takeOver: taker() } });
    press(trigger(render())); render();
    expect(review).not.toHaveBeenCalled();
    let answer!: (value: OwnerAnswer) => void;
    review.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve; }));
    press(item(render(), "takeOver")!);
    expect(review).toHaveBeenCalledExactlyOnceWith("host-a", "a");
    let tree = render();
    expect(menu(tree)).toBeUndefined();
    expect(texts(tree)).toEqual(expect.arrayContaining(["Take over Session a (a)?", "Reviewing the route its model chain picks…"]));
    expect(textButton(tree, "Take over").props.disabled).toBe(true);
    press(textButton(tree, "Take over"));
    answer(yes("a")); await flush();
    tree = render();
    expect(texts(tree)).toContain("Consequence for a.");
    expect(textButton(tree, "Take over").props.disabled).toBe(false);
    expect(apply).not.toHaveBeenCalled();
    apply.mockResolvedValueOnce(done("a"));
    press(textButton(tree, "Take over"));
    expect(apply).toHaveBeenCalledExactlyOnceWith({ hostId: "host-a", agentId: "a", reviewId: "review-a", revision: "rev-a" });
    await flush();
    const after = render();
    expect(byTestID(after, "sessions-select-result")!.props).toMatchObject({ role: "status", children: "Done: new session a-next. Archived it and started the new session." });
    expect(confirmShown(after)).toBe(false);
    expect(review).toHaveBeenCalledTimes(1);
  });

  it("drops a review that answers after Cancel, and sends nothing", async () => {
    const { render } = setup({ owners: { takeOver: taker() } });
    let answer!: (value: OwnerAnswer) => void;
    review.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve; }));
    choose(render, "takeOver");
    press(textButton(render(), "Cancel"));
    answer(yes("a")); await flush();
    expect(confirmShown(render())).toBe(false);
    expect(apply).not.toHaveBeenCalled();
  });

  it.each([
    ["refuses", () => Promise.resolve(no("a", "No model chain is recorded for this session.")), "No model chain is recorded for this session."],
    ["answers for another session", () => Promise.resolve(yes("b")), "Nothing was sent. The review answered for another session."],
    ["answers for another host", () => Promise.resolve(yes("a", { hostId: "host-b" })), "Nothing was sent. The review answered for another session."],
    ["fails", () => Promise.reject(new Error("resolver unavailable")), "Nothing was sent. The review failed: resolver unavailable."],
  ] as const)("keeps Confirm off with the exact reason when the review %s", async (_case, answer, line) => {
    const { render } = setup({ owners: { takeOver: taker() } });
    review.mockImplementationOnce(answer);
    choose(render, "takeOver"); await flush();
    const tree = render();
    expect(texts(tree)).toContain(line);
    expect(textButton(tree, "Take over").props.disabled).toBe(true);
    press(textButton(tree, "Take over")); await flush();
    expect(apply).not.toHaveBeenCalled();
  });

  it("retries in this session with its exact review and revision after its own confirmation, and reviews nothing", () => {
    const { render } = setup({ owners: { retry: lookup(yes("a")) } });
    choose(render, "retry");
    expect(texts(render())).toEqual(expect.arrayContaining(["Retry Session a (a) in this session?", "Consequence for a."]));
    apply.mockResolvedValueOnce(done("a", { successorId: undefined }));
    press(textButton(render(), "Retry"));
    expect(apply).toHaveBeenCalledExactlyOnceWith({ hostId: "host-a", agentId: "a", reviewId: "review-a", revision: "rev-a" });
    expect(review).not.toHaveBeenCalled();
  });

  it("never shows a reply for another session or host, and keeps an unknown outcome unknown", async () => {
    const { render } = setup({ owners: { takeOver: taker() } });
    for (const reply of [done("a", { hostId: "host-b" }), done("b")]) {
      apply.mockResolvedValueOnce(reply);
      await takeOver(render);
      expect(result(render())).toBe("No result reported. Check the session before trying again.");
    }
    apply.mockRejectedValueOnce(new Error("reply lost"));
    await takeOver(render);
    expect(result(render())).toBe("Result unknown: reply lost. Check the session before trying again.");
  });

  const changed = "This changed after you opened it. Cancel and choose the action again.";
  it.each([
    ["Take over", "the same session starts running", { selected: [session("a", { status: "running" })] }, "This session is running. Stop it in the session, then take it over."],
    ["Take over", "the same session starts initializing", { selected: [session("a", { status: "initializing" })] }, "This session is starting. Wait until it is idle, then take it over."],
    ["Take over", "the host goes offline", { blocked: "This host is offline." }, "This host is offline."],
    ["Take over", "the archive review opens", { blocked: "Close the archive review first." }, "Close the archive review first."],
    ["Take over", "the owner goes away", { owners: {} }, NO_TAKEOVER],
    ["Retry", "the session is archived", { selected: [session("a", { status: "error", archivedAt: ARCHIVED })] }, "Archived: Retry never revives an archived session."],
    ["Retry", "the owner withdraws its answer", { owners: { retry: lookup() } }, "Checking whether it can retry in this session."],
    ["Retry", "the owner now refuses", { owners: { retry: lookup(no("a", "Closed or stopped.")) } }, "Closed or stopped."],
    ["Retry", "the owner answers a new revision", { owners: { retry: lookup(yes("a", { revision: "rev-a2" })) } }, changed],
    ["Retry", "the owner answers from a new review", { owners: { retry: lookup(yes("a", { reviewId: "review-a2" })) } }, changed],
    ["Retry", "the owner states another consequence", { owners: { retry: lookup(yes("a", { consequence: "Another consequence." })) } }, changed],
  ] as const)("rechecks an open %s confirmation when %s, shows the current reason and calls no owner", async (label, _case, patch, reason) => {
    const action = label === "Take over" ? "takeOver" : "retry";
    const { props, render } = setup({ selected: [session("a", { status: "error" })], owners: action === "takeOver" ? { takeOver: taker() } : { retry: lookup(yes("a")) } });
    choose(render, action); await flush();
    expect(textButton(render(), label).props.disabled).toBe(false);
    Object.assign(props, patch);
    const tree = render();
    expect(confirmShown(tree)).toBe(true);
    expect(texts(tree)).toContain(reason);
    expect(textButton(tree, label).props.disabled).toBe(true);
    press(textButton(tree, label)); await flush();
    expect(apply).not.toHaveBeenCalled();
  });

  // Take over's local state changes only with a redraw; drift after its review is the owner's to refuse at apply.
  it.each([
    ["withdraws its answer", undefined, "Nothing was sent. Checking whether it can retry in this session."],
    ["now refuses", no("a", "Closed or stopped."), "Nothing was sent. Closed or stopped."],
    ["answers a new revision", yes("a", { revision: "rev-a2" }), "Nothing was sent. This changed after you opened it; choose the action again to review it."],
    ["answers from a new review", yes("a", { reviewId: "review-a2" }), "Nothing was sent. This changed after you opened it; choose the action again to review it."],
    ["states another consequence", yes("a", { consequence: "Another consequence." }), "Nothing was sent. This changed after you opened it; choose the action again to review it."],
  ] as const)("asks the Retry owner again at Confirm: when it %s with no redraw, nothing is sent and the reason shows at once", async (_case, later, line) => {
    let answer: OwnerAnswer | undefined = yes("a");
    const { render } = setup({ selected: [session("a", { status: "error" })], owners: { retry: { answer: () => answer, apply } } });
    choose(render, "retry");
    const drawn = render();
    expect(textButton(drawn, "Retry").props.disabled).toBe(false);
    answer = later;
    press(textButton(drawn, "Retry")); await flush();
    expect(apply).not.toHaveBeenCalled();
    const after = render();
    expect(confirmShown(after)).toBe(false);
    expect(byTestID(after, "sessions-select-result")!.props).toMatchObject({ role: "status", children: line });
  });

  it.each([
    ["a partial Take over as partly done", "takeOver", done("a", { outcome: "partial", successorId: undefined, text: "Archived it; the new session is not confirmed." }), "Partly done: Archived it; the new session is not confirmed."],
    ["a refusal as refused", "takeOver", done("a", { outcome: "refused", successorId: undefined, text: "No route in this session's chain is available." }), "Refused: No route in this session's chain is available."],
    ["a reply still reconciling as not confirmed", "takeOver", done("a", { outcome: "unconfirmed", text: "Checking the new session." }), "Not confirmed: Checking the new session."],
    ["a Take over success with no new session as unknown", "takeOver", done("a", { successorId: undefined }), "Result unknown: the owner reported success without a new session. Check the session before trying again."],
    ["a Take over success naming the same session as unknown", "takeOver", done("a", { successorId: "a" }), "Result unknown: the owner reported success without a new session. Check the session before trying again."],
    ["an outcome it does not know as unknown", "takeOver", done("a", { outcome: "archived" as OwnerReply["outcome"] }), "Result unknown: the owner reported no known outcome. Check the session before trying again."],
    ["a same-ID retry as done in this session", "retry", done("a", { successorId: undefined, text: "Sent the retry to this session." }), "Done: Sent the retry to this session."],
  ] as const)("reports %s", async (_case, action, reply, line) => {
    const { render } = setup({ owners: action === "takeOver" ? { takeOver: taker() } : { retry: lookup(yes("a")) } });
    apply.mockResolvedValueOnce(reply);
    choose(render, action); await flush();
    press(textButton(render(), action === "takeOver" ? "Take over" : "Retry")); await flush();
    expect(result(render())).toBe(line);
  });

  it("drops an open menu, a review, an open confirmation and a late reply when the selection changes", async () => {
    let reply!: (value: OwnerReply) => void, answer!: (value: OwnerAnswer) => void;
    const { props, render } = setup({ owners: { takeOver: taker() } });
    apply.mockImplementation(() => new Promise((resolve) => { reply = resolve; }));
    choose(render, "takeOver"); await flush(); press(textButton(render(), "Take over"));
    expect(textButton(render(), "Sending…").props.disabled).toBe(true);
    expect(trigger(render()).props.enabled).toBe(false);
    props.selected = [session("b")];
    render();
    reply(done("a")); await flush();
    let tree = render();
    expect(result(tree)).toBeUndefined();
    expect(confirmShown(tree)).toBe(false);
    review.mockImplementationOnce(() => new Promise((resolve) => { answer = resolve; }));
    choose(render, "takeOver");
    props.selected = [session("c")];
    render();
    answer(yes("b")); await flush();
    tree = render();
    expect(confirmShown(tree)).toBe(false);
    press(trigger(tree));
    expect(menu(render())).toBeDefined();
    props.selected = [session("d")];
    render();
    expect(menu(render())).toBeUndefined();
    expect(apply).toHaveBeenCalledTimes(1);
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
