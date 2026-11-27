import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const pan = vi.hoisted(() => ({ config: null as null | Record<string, (...args: never[]) => unknown>, spring: [] as number[] }));
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "ios" }, Pressable: "Pressable", Text: "Text", View: "View", ScrollView: "ScrollView", ActivityIndicator: "ActivityIndicator",
  PanResponder: { create: (config: Record<string, (...args: never[]) => unknown>) => { pan.config = config; return { panHandlers: { fixture: true } }; } },
  Animated: { View: "Animated.View", Value: class { value = 0; setValue(next: number) { this.value = next; } }, spring: (_value: unknown, to: { toValue: number }) => ({ start: () => { pan.spring.push(to.toValue); } }) } }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: vi.fn() }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon", Modal: Object.assign(function Modal() { return null; }, { Content: "Modal.Content" }) }));
vi.mock("../client/control-help", () => ({ useActionHelp: () => ({ bindings: {}, dismiss() {} }), useControlHelp: () => ({ bindings: {}, hold() {}, dismiss() {} }) }));

import { ArchiveChildrenDialog, BRANCH_CHANGED, branchChanged, branchChildren, branchProblem } from "../client/archive-children";
import { OperationSheet, SheetFooter } from "../client/operation-sheet";
import { Action } from "../client/page-controls";
import { SwipeToArchive, archivesOnRelease, claimsSwipe } from "../client/swipe-archive";
import { branchDispositions } from "../client/start-switch-archive";
import { withLocalDispositions } from "../client/use-session-availability";
import { useSessionLifecycle } from "../client/use-session-lifecycle";
import { useRpc } from "@getpaseo/plugin/client";
import { DEFAULT_RECOVERY_TARGET, KEEP_CHILDREN_UNAVAILABLE, type LifecycleReview, type LifecycleRow } from "../shared/session-lifecycle";
import { fixtureLifecycle } from "../../kontor-prototype/src/host/session-lifecycle";
import { recoverySessions } from "../../kontor-prototype/src/fixtures/recovery";
import { sessionEntry } from "./session-fixtures";

type Element = { type?: unknown; props: Record<string, unknown> };
const DRAWN: unknown[] = [OperationSheet, SheetFooter];
/** Every element under `node`, drawing the sheet and its footer in place so their buttons are visible. */
const walk = (node: unknown): Element[] => {
  if (!node || typeof node !== "object") return [];
  if (Array.isArray(node)) return node.flatMap(walk);
  const element = node as Element;
  if (DRAWN.includes(element.type)) return [element, ...walk((element.type as (props: unknown) => unknown)(element.props))];
  return [element, ...walk(element.props.children), ...walk(element.props.footer)];
};
const texts = (tree: Element[]) => tree.filter((element) => element.type === "Text").map((element) => [element.props.children].flat().join(""));
const byId = (tree: Element[], testID: string) => tree.find((element) => element.props.testID === testID);
const keep = (tree: Element[]) => tree.find((element) => element.type === Action && element.props.label === "Keep children alive")!;
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const colors = { foreground: "#111", foregroundMuted: "#666", surface0: "#fff", surface2: "#eee", border: "#ccc", accent: "#080", accentForeground: "#fff", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
type Lifecycle = ReturnType<typeof useSessionLifecycle>;

const row = (id: string, children: string[] = [], patch: Partial<LifecycleRow> = {}): LifecycleRow => ({ id, title: `Session ${id}`, workspaceId: "wks", cwd: "/fixture", provider: "codex", model: null, effort: null, mode: null,
  nativeStatus: "idle", fingerprint: `fp-${id}`, category: "candidate", eligible: true, reason: "Fixture eligible.", outcome: "pending", message: null,
  children: children.map((child) => ({ id: child, title: `Session ${child}`, status: "idle", workspaceId: "wks" })), ...patch });
/** Parent p with children c1 (running) and c2, and c1's child g; child-first as the host orders it. */
const review = (rows: LifecycleRow[] = [row("g"), row("c1", ["g"], { nativeStatus: "running" }), row("c2"), row("p", ["c1", "c2"])], id = "00000000-0000-4000-8000-00000000000a"): LifecycleReview =>
  ({ id, createdAt: "2026-11-27", action: "archive", scope: "filtered", target: { provider: "codex", model: "m", effort: "e" }, limitation: "Fixture.", rows, keepChildren: { available: false, reason: KEEP_CHILDREN_UNAVAILABLE } });

beforeEach(() => { harness.fresh(); pan.config = null; pan.spring = []; });

// Fixed point distances: the row takes a drag from 12 points left and archives from 96, whatever the constants say.
describe("swipe classification", () => {
  it.each([
    ["a tap wobble", -11, 0, false, false],
    ["the shortest drag the row takes", -12, 0, true, false],
    ["a short leftward drag", -95, 0, true, false],
    ["a deliberate leftward swipe", -96, 0, true, true],
    ["a leftward swipe at exactly the steepest allowed angle", -96, 48, true, true],
    ["a leftward swipe just steeper than allowed", -96, 49, false, false],
    ["a rightward swipe", 192, 0, false, false],
    ["a vertical scroll", -20, 120, false, false],
    ["a mostly vertical drag that ends far left", -106, -80, false, false],
  ])("%s: claims %#", (_name, dx, dy, claims, archives) => {
    expect(claimsSwipe({ dx, dy })).toBe(claims);
    expect(archivesOnRelease({ dx, dy })).toBe(archives);
  });
});

describe("SwipeToArchive", () => {
  const mount = (enabled = true) => {
    const onArchive = vi.fn();
    const tree = harness.current.render(() => SwipeToArchive({ enabled, colors, onArchive, children: "row" })) as Element;
    const config = pan.config!;
    const move = (dx: number, dy = 0) => (config.onMoveShouldSetPanResponderCapture as (event: unknown, travel: object) => boolean)({}, { dx, dy });
    const release = (dx: number, dy = 0) => (config.onPanResponderRelease as (event: unknown, travel: object) => void)({}, { dx, dy });
    return { tree, config, onArchive, move, release };
  };
  it("takes only a leftward horizontal drag and archives once on a deliberate release", () => {
    const s = mount();
    expect(s.move(-11)).toBe(false);
    expect(s.move(-12)).toBe(true);
    expect(s.move(36)).toBe(false);
    expect(s.move(-12, 12)).toBe(false);
    expect((s.config.onPanResponderTerminationRequest as () => boolean)()).toBe(false);
    s.release(-95);
    expect(s.onArchive).not.toHaveBeenCalled();
    s.release(-96);
    expect(s.onArchive).toHaveBeenCalledOnce();
    s.release(-100, 3);
    expect(s.onArchive).toHaveBeenCalledTimes(2);
    expect(pan.spring).toEqual([0, 0, 0]);
  });
  it("does nothing for a cancelled, rightward or vertical drag", () => {
    const s = mount();
    (s.config.onPanResponderTerminate as (event: unknown, travel: object) => void)({}, { dx: -192, dy: 0 });
    s.release(192);
    s.release(-10, 192);
    expect(s.onArchive).not.toHaveBeenCalled();
    expect(pan.spring).toEqual([0, 0, 0]);
  });
  it("stays still and never archives while Archive is unavailable", () => {
    const s = mount(false);
    expect(s.move(-192)).toBe(false);
    s.release(-192);
    expect(s.onArchive).not.toHaveBeenCalled();
  });
  it("passes the row through unchanged and hides the swipe hint from assistive technology", () => {
    const s = mount(), body = walk(s.tree).find((element) => element.type === "Animated.View")!;
    expect(body.props).toMatchObject({ fixture: true, children: "row" });
    (s.config.onPanResponderGrant as () => void)();
    const dragging = walk(harness.current.render(() => SwipeToArchive({ enabled: true, colors, onArchive: s.onArchive, children: "row" })));
    expect(dragging.find((element) => element.props.accessibilityElementsHidden)?.props.importantForAccessibility).toBe("no-hide-descendants");
  });
});

describe("branch review helpers", () => {
  it("lists descendants top-down with their depth, while the review stays child-first", () => {
    expect(branchChildren(review(), "p").map(({ row: value, depth }) => [value.id, depth])).toEqual([["c1", 1], ["g", 2], ["c2", 1]]);
  });
  it("treats a new session, a changed child set or a changed identity as a change, and a status change as none", () => {
    const base = review();
    expect(branchChanged(base, review())).toBe(false);
    expect(branchChanged(base, review(base.rows.map((value) => value.id === "c1" ? { ...value, nativeStatus: "idle" } : value)))).toBe(false);
    expect(branchChanged(base, review([row("late"), ...base.rows.slice(0, 3), row("p", ["c1", "c2", "late"])]))).toBe(true);
    expect(branchChanged(base, review(base.rows.map((value) => value.id === "g" ? { ...value, fingerprint: "moved" } : value)))).toBe(true);
  });
  it("says why a branch cannot be archived: missing parent, no children, or a blocked session", () => {
    expect(branchProblem(review(), "p")).toBeNull();
    expect(branchProblem(review(), "other")).toMatch(/not in the host's review/);
    expect(branchProblem(review([row("p")]), "p")).toMatch(/no active child sessions/);
    expect(branchProblem(review([row("g", [], { eligible: false, reason: "Native read failed." }), row("p", ["g"])]), "p")).toBe("Session g cannot be archived: Native read failed. Nothing was archived.");
  });
});

describe("Archive with children dialog", () => {
  const entry = sessionEntry("p", { title: "Parent session" });
  const open = (lifecycle: Partial<Lifecycle> = {}, online = true) => {
    const props = { entry, colors, online, onClose: vi.fn(), onArchive: vi.fn(),
      lifecycle: { busy: false, running: false, error: null, branch: vi.fn(async () => review()), run: vi.fn(), ...lifecycle } as unknown as Lifecycle };
    const render = () => walk(harness.current.render(() => ArchiveChildrenDialog(props)));
    return { props, render };
  };
  const ready = async (lifecycle: Partial<Lifecycle> = {}, online = true) => { const s = open(lifecycle, online); s.render(); await settle(); return { ...s, tree: s.render() }; };
  const confirm = (tree: Element[]) => byId(tree, "archive-children-confirm")!;
  const FAILED = "Incomplete native inventory; no lifecycle action is allowed.";
  /** The dialog before a usable review: the read is still open, the host refused it, or the host went offline mid-read. */
  const unreviewed = {
    loading: { lifecycle: () => ({ branch: vi.fn(() => new Promise<never>(() => {})) }), online: true, why: "Reading this session's child sessions on the host…" },
    failed: { lifecycle: () => ({ branch: vi.fn(async () => FAILED), error: FAILED }), online: true,
      why: "The host did not list this session's child sessions, so nothing can be archived here. Close this and archive the session again." },
    offline: { lifecycle: () => ({ branch: vi.fn(() => new Promise<never>(() => {})) }), online: false, why: "The host is offline. Nothing can be archived until it reconnects." },
    "offline after its review": { lifecycle: () => ({}), online: false, why: "The host is offline. Nothing can be archived until it reconnects." },
  } as const;
  const escape = (tree: Element[]) => (tree.find((element) => typeof element.props.onOpenChange === "function")!.props.onOpenChange as (open: boolean) => void)(false);

  it("reads the exact branch once on open and archives nothing while it reads", () => {
    const s = open(), tree = s.render();
    expect(s.props.lifecycle.branch).toHaveBeenCalledExactlyOnceWith("p");
    expect(texts(tree)).toContain("Reading this session's child sessions on the host…");
    expect(confirm(tree).props.disabled).toBe(true);
    expect(s.props.lifecycle.run).not.toHaveBeenCalled(); expect(s.props.onArchive).not.toHaveBeenCalled();
  });
  it("lists every reviewed child, names the running one and says archive stops its work", async () => {
    const s = await ready();
    expect(byId(s.tree, "archive-children-row-g")!.props.style).toMatchObject({ paddingLeft: 24 });
    expect(texts(s.tree)).toEqual(expect.arrayContaining(["3 active child sessions belong to this session. Choose what happens to them.", "Session c1", "running", "Session g", "idle · nested", "Session c2"]));
    expect(byId(s.tree, "archive-children-running")!.props.children).toBe("Running now: Session c1. Archive stops their active work.");
    expect(texts(s.tree).some((value) => value.startsWith("Archive children too archives these sessions first") && value.includes("Archive stops any active work"))).toBe(true);
    expect(texts(s.tree)).toContain("A new child session started just before this session archives may be archived with it by the host, even though it is not listed here.");
    expect(confirm(s.tree).props).toMatchObject({ label: "Archive children too", disabled: false });
  });
  it("shows Keep children alive disabled with the host's cause, and choosing it does nothing", async () => {
    const s = await ready(), option = keep(s.tree);
    expect(option.props).toMatchObject({ disabled: true, accessibilityLabel: `Keep children alive, unavailable. ${KEEP_CHILDREN_UNAVAILABLE}` });
    expect(byId(s.tree, "archive-children-keep-reason")!.props.children).toBe(KEEP_CHILDREN_UNAVAILABLE);
    (option.props.onPress as () => unknown)();
    expect(s.props.lifecycle.branch).toHaveBeenCalledOnce(); expect(s.props.lifecycle.run).not.toHaveBeenCalled();
    expect(s.props.onArchive).not.toHaveBeenCalled(); expect(s.props.onClose).not.toHaveBeenCalled();
  });
  it.each(["loading", "failed", "offline", "offline after its review"] as const)("shows both choices while the branch is %s: Archive children too disabled, Keep children alive with the host limitation, and neither sends anything", async (state) => {
    const s = await ready(unreviewed[state].lifecycle(), unreviewed[state].online);
    const primary = confirm(s.tree), option = keep(s.tree);
    expect(primary.props).toMatchObject({ label: "Archive children too", disabled: true, hint: unreviewed[state].why });
    expect(option.props).toMatchObject({ disabled: true, accessibilityLabel: `Keep children alive, unavailable. ${KEEP_CHILDREN_UNAVAILABLE}` });
    expect(byId(s.tree, "archive-children-keep-reason")!.props.children).toBe(KEEP_CHILDREN_UNAVAILABLE);
    await (primary.props.onPress as () => unknown)(); (option.props.onPress as () => unknown)(); await settle();
    expect(s.props.lifecycle.branch).toHaveBeenCalledOnce(); expect(s.props.lifecycle.run).not.toHaveBeenCalled();
    expect(s.props.onArchive).not.toHaveBeenCalled(); expect(s.props.onClose).not.toHaveBeenCalled();
  });
  it.each([["available", () => ({}), true, false], ["unavailable (offline)", () => ({}), false, true],
    ["loading", unreviewed.loading.lifecycle, true, true], ["unread (failed)", unreviewed.failed.lifecycle, true, true]] as const)("Cancel and Escape change nothing while Archive children too is %s", async (_state, lifecycle, online, disabled) => {
    for (const exit of ["cancel", "escape"] as const) {
      harness.fresh();
      const s = await ready(lifecycle(), online);
      expect(confirm(s.tree).props.disabled).toBe(disabled);
      if (exit === "cancel") (byId(s.tree, "archive-children-cancel")!.props.onPress as () => void)(); else escape(s.tree);
      await settle();
      expect(s.props.onClose).toHaveBeenCalledOnce();
      expect(s.props.lifecycle.branch).toHaveBeenCalledOnce();
      expect(s.props.lifecycle.run).not.toHaveBeenCalled(); expect(s.props.onArchive).not.toHaveBeenCalled();
    }
  });
  it("re-reads the exact branch before any archive and hands over the unchanged review", async () => {
    const fresh = review(undefined, "00000000-0000-4000-8000-00000000000b"), branch = vi.fn().mockResolvedValueOnce(review()).mockResolvedValueOnce(fresh);
    const s = await ready({ branch });
    await (confirm(s.tree).props.onPress as () => Promise<void>)(); await settle();
    expect(branch).toHaveBeenCalledTimes(2); expect(branch).toHaveBeenLastCalledWith("p");
    expect(s.props.onArchive).toHaveBeenCalledExactlyOnceWith(fresh);
  });
  it("refuses a changed branch, archives nothing and shows the updated list", async () => {
    const changed = review([row("late"), row("g"), row("c1", ["g"]), row("c2"), row("p", ["c1", "c2", "late"])]);
    const branch = vi.fn().mockResolvedValueOnce(review()).mockResolvedValueOnce(changed);
    const s = await ready({ branch });
    await (confirm(s.tree).props.onPress as () => Promise<void>)(); await settle();
    const tree = s.render();
    expect(s.props.onArchive).not.toHaveBeenCalled(); expect(s.props.lifecycle.run).not.toHaveBeenCalled();
    expect(byId(tree, "archive-children-refusal")!.props).toMatchObject({ accessibilityRole: "alert", children: BRANCH_CHANGED });
    expect(byId(tree, "archive-children-row-late")).toBeDefined();
  });
  it("refuses when the re-read finds a session that cannot be archived, and when the dialog closed during the read", async () => {
    const blocked = review([row("g", [], { eligible: false, reason: "Native read failed." }), row("c1", ["g"], { nativeStatus: "running" }), row("c2"), row("p", ["c1", "c2"])]);
    const s = await ready({ branch: vi.fn().mockResolvedValueOnce(review()).mockResolvedValueOnce(blocked) });
    await (confirm(s.tree).props.onPress as () => Promise<void>)(); await settle();
    expect(s.props.onArchive).not.toHaveBeenCalled();
    expect(byId(s.render(), "archive-children-refusal")!.props.children).toBe("Session g cannot be archived: Native read failed. Nothing was archived.");
    harness.fresh();
    let finish!: (value: LifecycleReview) => void;
    const late = await ready({ branch: vi.fn().mockResolvedValueOnce(review()).mockReturnValueOnce(new Promise((resolve) => { finish = resolve; })) });
    const pending = (confirm(late.tree).props.onPress as () => Promise<void>)();
    (byId(late.tree, "archive-children-cancel")!.props.onPress as () => void)();
    finish(review()); await pending; await settle();
    expect(late.props.onArchive).not.toHaveBeenCalled();
  });
  /** The dialog on the actual useSessionLifecycle over a synthetic preview RPC; `host.online` is read on every render. */
  const actual = async (preview: ReturnType<typeof vi.fn>) => {
    vi.mocked(useRpc).mockImplementation(((contract: { name: string }) => contract.name === "sessions.lifecycle-preview" ? preview : vi.fn()) as never);
    const host = { online: true }, onArchive = vi.fn(), onClose = vi.fn(); let error: string | null = null;
    const render = () => walk(harness.current.render(() => {
      const lifecycle = useSessionLifecycle("host-a", () => {}); error = lifecycle.error;
      return ArchiveChildrenDialog({ entry, colors, online: host.online, lifecycle, onClose, onArchive });
    }));
    render(); await settle(); render(); await settle();
    return { host, onArchive, onClose, render, tree: render(), error: () => error };
  };
  const press = (element: Element) => (element.props.onPress as () => Promise<void> | undefined)();
  const INTERRUPTED = "The host's availability changed while Archive children too was checking these sessions. Nothing was archived.";
  it("leaves both choices unavailable after a failed fresh read, sends no third read and never hands over (AUD-9118-04)", async () => {
    const preview = vi.fn().mockResolvedValueOnce(review()).mockRejectedValueOnce(new Error("Incomplete native inventory.")).mockResolvedValue(review());
    const s = await actual(preview);
    expect(confirm(s.tree).props.disabled).toBe(false);
    await press(confirm(s.tree)); await settle();
    const tree = s.render(), primary = confirm(tree), option = keep(tree);
    expect(s.error()).toBe("Incomplete native inventory.");
    await press(primary); (option.props.onPress as () => unknown)(); await settle(); s.render();
    expect(s.onArchive).not.toHaveBeenCalled(); expect(preview).toHaveBeenCalledTimes(2);
    expect(primary.props).toMatchObject({ disabled: true, hint: unreviewed.failed.why });
    expect(option.props.disabled).toBe(true);
    expect(byId(tree, "archive-children-row-c1")).toBeUndefined();
  });
  it("never hands over a fresh read started online once the host goes offline (AUD-9118-05)", async () => {
    let finish!: (value: LifecycleReview) => void;
    const preview = vi.fn().mockResolvedValueOnce(review()).mockReturnValueOnce(new Promise<LifecycleReview>((resolve) => { finish = resolve; }));
    const s = await actual(preview);
    const pending = press(confirm(s.tree));
    s.host.online = false; const tree = s.render();
    expect(confirm(tree).props).toMatchObject({ disabled: true, hint: unreviewed.offline.why });
    finish(review()); await pending; await settle(); s.render();
    expect(s.onArchive).not.toHaveBeenCalled(); expect(preview).toHaveBeenCalledTimes(2);
  });
  it("ends the displayed review once Archive children too is chosen: while the fresh read is open, no child is listed and nothing can be chosen", async () => {
    const preview = vi.fn().mockResolvedValueOnce(review()).mockReturnValueOnce(new Promise<never>(() => {}));
    const s = await actual(preview);
    expect(byId(s.tree, "archive-children-row-c1")).toBeDefined();
    void press(confirm(s.tree));
    const tree = s.render();
    expect(byId(tree, "archive-children-row-c1")).toBeUndefined(); expect(texts(tree)).toContain(unreviewed.loading.why);
    expect(confirm(tree).props.disabled).toBe(true);
    await press(confirm(tree)); await settle();
    expect(preview).toHaveBeenCalledTimes(2); expect(s.onArchive).not.toHaveBeenCalled();
  });
  it("never hands over a fresh read when the host went offline and back online during it, and asks for a fresh review", async () => {
    let finish!: (value: LifecycleReview) => void;
    const preview = vi.fn().mockResolvedValueOnce(review()).mockReturnValueOnce(new Promise<LifecycleReview>((resolve) => { finish = resolve; }));
    const s = await actual(preview);
    const pending = press(confirm(s.tree));
    s.host.online = false; s.render(); s.host.online = true; s.render();
    finish(review()); await pending; await settle();
    const tree = s.render();
    expect(s.onArchive).not.toHaveBeenCalled();
    expect(confirm(tree).props).toMatchObject({ disabled: true, hint: unreviewed.failed.why });
    expect(byId(tree, "archive-children-refusal")!.props.children).toBe(INTERRUPTED);
    await press(confirm(tree)); await settle();
    expect(preview).toHaveBeenCalledTimes(2); expect(s.onArchive).not.toHaveBeenCalled();
  });
  it.each(["cancel", "escape"] as const)("never hands over a fresh read that settles after %s", async (exit) => {
    let finish!: (value: LifecycleReview) => void;
    const preview = vi.fn().mockResolvedValueOnce(review()).mockReturnValueOnce(new Promise<LifecycleReview>((resolve) => { finish = resolve; }));
    const s = await actual(preview);
    const pending = press(confirm(s.tree));
    const tree = s.render();
    if (exit === "cancel") (byId(tree, "archive-children-cancel")!.props.onPress as () => void)(); else escape(tree);
    finish(review()); await pending; await settle();
    expect(s.onClose).toHaveBeenCalledOnce(); expect(preview).toHaveBeenCalledTimes(2); expect(s.onArchive).not.toHaveBeenCalled();
  });
  it("hands an unchanged fresh read over exactly once, child first (control)", async () => {
    const fresh = review(undefined, "00000000-0000-4000-8000-00000000000b");
    const preview = vi.fn().mockResolvedValueOnce(review()).mockResolvedValueOnce(fresh);
    const s = await actual(preview);
    await press(confirm(s.tree)); await settle();
    expect(preview).toHaveBeenCalledTimes(2);
    expect(s.onArchive).toHaveBeenCalledExactlyOnceWith(fresh);
    expect((s.onArchive.mock.calls[0]![0] as LifecycleReview).rows.map((value) => value.id)).toEqual(["g", "c1", "c2", "p"]);
  });
  it("shows a changed fresh read for a new choice and archives nothing until it is chosen", async () => {
    const changed = review([row("late"), row("g"), row("c1", ["g"]), row("c2"), row("p", ["c1", "c2", "late"])], "00000000-0000-4000-8000-00000000000c");
    const preview = vi.fn().mockResolvedValueOnce(review()).mockResolvedValue(changed);
    const s = await actual(preview);
    await press(confirm(s.tree)); await settle();
    let tree = s.render();
    expect(s.onArchive).not.toHaveBeenCalled();
    expect(byId(tree, "archive-children-row-late")).toBeDefined();
    expect(byId(tree, "archive-children-refusal")!.props.children).toBe(BRANCH_CHANGED);
    expect(confirm(tree).props).toMatchObject({ disabled: false, hint: null });
    await press(confirm(tree)); await settle(); tree = s.render();
    expect(preview).toHaveBeenCalledTimes(3); expect(s.onArchive).toHaveBeenCalledExactlyOnceWith(changed);
  });
  // AUD-9118-06: a host press handler can still hold an earlier render's callback; it confirms only its own shown review and view.
  it("never lets an earlier enabled callback start a confirm after the view goes offline (AUD-9118-06 entry race)", async () => {
    const preview = vi.fn().mockResolvedValue(review());
    const s = await actual(preview), queued = confirm(s.tree);
    s.host.online = false; const offline = s.render();
    expect(confirm(offline).props.disabled).toBe(true);
    await press(queued); await settle(); s.render();
    expect(s.onArchive).not.toHaveBeenCalled();
    expect(preview).toHaveBeenCalledTimes(1);
  });
  it("never lets an earlier callback reuse a review whose confirm failed (AUD-9118-06 stale callback)", async () => {
    const preview = vi.fn().mockResolvedValueOnce(review()).mockRejectedValueOnce(new Error("unread confirm")).mockResolvedValue(review());
    const s = await actual(preview), queued = confirm(s.tree);
    await press(queued); await settle(); const failed = s.render();
    expect(s.error()).toBe("unread confirm"); expect(confirm(failed).props.disabled).toBe(true);
    await press(queued); await settle(); s.render();
    expect(s.onArchive).not.toHaveBeenCalled();
    expect(preview).toHaveBeenCalledTimes(2);
  });
  it("never lets a callback from a replaced review confirm, while the shown review's own callback still can", async () => {
    const changed = review([row("late"), row("g"), row("c1", ["g"]), row("c2"), row("p", ["c1", "c2", "late"])], "00000000-0000-4000-8000-00000000000c");
    const preview = vi.fn().mockResolvedValueOnce(review()).mockResolvedValue(changed);
    const s = await actual(preview), queued = confirm(s.tree);
    await press(queued); await settle(); const shown = s.render();
    expect(byId(shown, "archive-children-row-late")).toBeDefined();
    await press(queued); await settle(); s.render();
    expect(preview).toHaveBeenCalledTimes(2); expect(s.onArchive).not.toHaveBeenCalled();
    await press(confirm(shown)); await settle();
    expect(preview).toHaveBeenCalledTimes(3); expect(s.onArchive).toHaveBeenCalledExactlyOnceWith(changed);
  });
  it.each(["cancel", "escape"] as const)("never lets an earlier callback read or archive after %s", async (exit) => {
    const preview = vi.fn().mockResolvedValue(review());
    const s = await actual(preview), queued = confirm(s.tree);
    if (exit === "cancel") (byId(s.tree, "archive-children-cancel")!.props.onPress as () => void)(); else escape(s.tree);
    await press(queued); await settle();
    expect(s.onClose).toHaveBeenCalledOnce(); expect(preview).toHaveBeenCalledTimes(1); expect(s.onArchive).not.toHaveBeenCalled();
  });
  it("never hands over through several availability flaps during one confirm", async () => {
    let finish!: (value: LifecycleReview) => void;
    const preview = vi.fn().mockResolvedValueOnce(review()).mockReturnValueOnce(new Promise<LifecycleReview>((resolve) => { finish = resolve; }));
    const s = await actual(preview), pending = press(confirm(s.tree));
    for (const online of [false, true, false, true]) { s.host.online = online; s.render(); }
    finish(review()); await pending; await settle(); const tree = s.render();
    await press(confirm(tree)); expect(s.onArchive).not.toHaveBeenCalled(); expect(preview).toHaveBeenCalledTimes(2);
    expect(confirm(tree).props.disabled).toBe(true);
  });
  it("requires a fresh review after a changed review's second confirm fails", async () => {
    const changed = review([row("late"), row("g"), row("c1", ["g"]), row("c2"), row("p", ["c1", "c2", "late"])]);
    const preview = vi.fn().mockResolvedValueOnce(review()).mockResolvedValueOnce(changed).mockRejectedValueOnce(new Error("second confirm unread"));
    const s = await actual(preview); await press(confirm(s.tree)); await settle(); let tree = s.render();
    expect(byId(tree, "archive-children-row-late")).toBeDefined(); expect(confirm(tree).props.disabled).toBe(false);
    await press(confirm(tree)); await settle(); tree = s.render();
    expect(confirm(tree).props.disabled).toBe(true); await press(confirm(tree));
    expect(s.onArchive).not.toHaveBeenCalled(); expect(preview).toHaveBeenCalledTimes(3);
  });
  it.each(["cancel", "escape"] as const)("discards a late result when %s ends the second confirm", async (exit) => {
    const changed = review([row("late"), row("g"), row("c1", ["g"]), row("c2"), row("p", ["c1", "c2", "late"])]);
    let finish!: (value: LifecycleReview) => void;
    const preview = vi.fn().mockResolvedValueOnce(review()).mockResolvedValueOnce(changed).mockReturnValueOnce(new Promise<LifecycleReview>((resolve) => { finish = resolve; }));
    const s = await actual(preview); await press(confirm(s.tree)); await settle(); const changedTree = s.render();
    const pending = press(confirm(changedTree)), tree = s.render();
    if (exit === "cancel") (byId(tree, "archive-children-cancel")!.props.onPress as () => void)(); else escape(tree);
    finish(changed); await pending; await settle(); s.render();
    expect(s.onArchive).not.toHaveBeenCalled(); expect(s.onClose).toHaveBeenCalledOnce(); expect(preview).toHaveBeenCalledTimes(3);
  });
  it("recovers from an interrupted confirm by closing and reopening for a fresh choice (control)", async () => {
    let finish!: (value: LifecycleReview) => void;
    const preview = vi.fn().mockResolvedValueOnce(review()).mockReturnValueOnce(new Promise<LifecycleReview>((resolve) => { finish = resolve; })).mockResolvedValue(review());
    const s = await actual(preview), pending = press(confirm(s.tree));
    s.host.online = false; s.render(); s.host.online = true; s.render(); finish(review()); await pending; await settle();
    const interrupted = s.render(); expect(confirm(interrupted).props.disabled).toBe(true);
    expect(texts(interrupted)).toContain(unreviewed.failed.why);
    (byId(interrupted, "archive-children-cancel")!.props.onPress as () => void)(); expect(s.onClose).toHaveBeenCalledOnce();
    harness.fresh(); const reopened = await actual(preview); expect(confirm(reopened.tree).props.disabled).toBe(false);
    await press(confirm(reopened.tree)); await settle();
    expect(s.onArchive).not.toHaveBeenCalled(); expect(reopened.onArchive).toHaveBeenCalledOnce(); expect(preview).toHaveBeenCalledTimes(4);
  });
  it("shows a host read failure and keeps Archive children too unavailable", async () => {
    const s = await ready({ branch: vi.fn(async () => "Incomplete native inventory; no lifecycle action is allowed."), error: "Incomplete native inventory; no lifecycle action is allowed." });
    expect(confirm(s.tree).props.disabled).toBe(true);
    expect(texts(s.tree)).toContain("Incomplete native inventory; no lifecycle action is allowed.");
    expect(texts(s.tree)).not.toContain("Reading this session's child sessions on the host…");
  });
});

describe("Archive with children dialog against the kontor-prototype host", () => {
  it("lists the child from the prototype's branch review and hands over child, then parent, with nothing archived", async () => {
    const rows = recoverySessions(), update = vi.fn(), invoke = fixtureLifecycle(() => rows, update);
    const branch = vi.fn((id: string) => invoke("sessions.lifecycle-preview", { action: "archive", scope: "filtered", agentIds: [id], target: DEFAULT_RECOVERY_TARGET, descendants: true }));
    const props = { entry: sessionEntry("agt_recovery_parent", { title: "Supervisor" }), colors, online: true, onClose: vi.fn(), onArchive: vi.fn(),
      lifecycle: { busy: false, running: false, error: null, branch, run: vi.fn() } as unknown as Lifecycle };
    const render = () => walk(harness.current.render(() => ArchiveChildrenDialog(props)));
    render(); await settle(); const tree = render();
    expect(byId(tree, "archive-children-row-agt_recovery_child")).toBeDefined();
    expect(byId(tree, "archive-children-running")!.props.children).toBe("Running now: SWE • Running child · preserve work. Archive stops their active work.");
    expect(byId(tree, "archive-children-keep-reason")!.props.children).toBe(KEEP_CHILDREN_UNAVAILABLE);
    expect(byId(tree, "archive-children-confirm")!.props).toMatchObject({ disabled: false, hint: null });
    await (byId(tree, "archive-children-confirm")!.props.onPress as () => Promise<void>)(); await settle();
    expect(branch).toHaveBeenCalledTimes(2);
    expect((props.onArchive.mock.calls[0]![0] as LifecycleReview).rows.map((row) => row.id)).toEqual(["agt_recovery_child", "agt_recovery_parent"]);
    expect(update).not.toHaveBeenCalled();
  });
});

describe("branch results without live native updates", () => {
  const archived = (id: string, revision = "r1") => row(id, [], { outcome: "archived", runtime: "closed", availability: { agentId: id, state: "readable", archivedAt: "2026-11-27", checkedAt: "2026-11-27", revision, reason: "Exact read." } });
  const entries = ["g", "c1", "p"].map((id) => sessionEntry(id, { updatedAt: "r1" }));
  const revisions = { g: "r1", c1: "r1", p: "r1" };
  /** Which sessions the stale list now shows archived. */
  const archivedIds = (list: readonly { id: string; archivedAt: string | null }[]) => list.filter((entry) => entry.archivedAt !== null).map((entry) => entry.id);
  it("marks every confirmed archived session of a successful branch", () => {
    expect(archivedIds(withLocalDispositions(entries, branchDispositions(entries, [archived("g"), archived("c1"), archived("p")], revisions)))).toEqual(["g", "c1", "p"]);
  });
  it("keeps the uncertain and blocked sessions of a partial branch, and a newer native row, unarchived", () => {
    const rows = [archived("g"), row("c1", ["g"], { outcome: "uncertain", availability: { agentId: "c1", state: "readable", archivedAt: null, checkedAt: "2026-11-27", revision: "r1", reason: "Not archived on this read." } }),
      row("p", ["c1"], { outcome: "blocked", message: "Selected children have not all archived with confirmed outcomes." })];
    expect(archivedIds(withLocalDispositions(entries, branchDispositions(entries, rows, revisions)))).toEqual(["g"]);
    const newer = entries.map((entry) => entry.id === "g" ? { ...entry, updatedAt: "r2" } : entry);
    expect(archivedIds(withLocalDispositions(newer, branchDispositions(newer, rows, revisions)))).toEqual([]);
    // A listed session outside the confirmed branch never binds through it, even at a matching revision.
    expect(branchDispositions([...entries, sessionEntry("outside", { updatedAt: "r1" })], [archived("outside")], revisions)).toEqual({});
  });
});
