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
import { SWIPE_ARCHIVE, SWIPE_CLAIM, SwipeToArchive, archivesOnRelease, claimsSwipe } from "../client/swipe-archive";
import { branchDispositions } from "../client/start-switch-archive";
import { withLocalDispositions } from "../client/use-session-availability";
import type { useSessionLifecycle } from "../client/use-session-lifecycle";
import { KEEP_CHILDREN_UNAVAILABLE, type LifecycleReview, type LifecycleRow } from "../shared/session-lifecycle";
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

describe("swipe classification", () => {
  it.each([
    ["a tap wobble", -SWIPE_CLAIM + 1, 0, false, false],
    ["a short leftward drag", -(SWIPE_ARCHIVE - 1), 0, true, false],
    ["a deliberate leftward swipe", -SWIPE_ARCHIVE, 0, true, true],
    ["a leftward swipe at exactly the steepest allowed angle", -SWIPE_ARCHIVE, SWIPE_ARCHIVE / 2, true, true],
    ["a leftward swipe just steeper than allowed", -SWIPE_ARCHIVE, SWIPE_ARCHIVE / 2 + 1, false, false],
    ["a rightward swipe", SWIPE_ARCHIVE * 2, 0, false, false],
    ["a vertical scroll", -20, 120, false, false],
    ["a mostly vertical drag that ends far left", -(SWIPE_ARCHIVE + 10), -80, false, false],
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
    expect(s.move(-SWIPE_CLAIM)).toBe(true);
    expect(s.move(SWIPE_CLAIM * 3)).toBe(false);
    expect(s.move(-SWIPE_CLAIM, SWIPE_CLAIM)).toBe(false);
    expect((s.config.onPanResponderTerminationRequest as () => boolean)()).toBe(false);
    s.release(-(SWIPE_ARCHIVE - 1));
    expect(s.onArchive).not.toHaveBeenCalled();
    s.release(-SWIPE_ARCHIVE - 4, 3);
    expect(s.onArchive).toHaveBeenCalledOnce();
    expect(pan.spring).toEqual([0, 0]);
  });
  it("does nothing for a cancelled, rightward or vertical drag", () => {
    const s = mount();
    (s.config.onPanResponderTerminate as (event: unknown, travel: object) => void)({}, { dx: -SWIPE_ARCHIVE * 2, dy: 0 });
    s.release(SWIPE_ARCHIVE * 2);
    s.release(-10, SWIPE_ARCHIVE * 2);
    expect(s.onArchive).not.toHaveBeenCalled();
    expect(pan.spring).toEqual([0, 0, 0]);
  });
  it("stays still and never archives while Archive is unavailable", () => {
    const s = mount(false);
    expect(s.move(-SWIPE_ARCHIVE * 2)).toBe(false);
    s.release(-SWIPE_ARCHIVE * 2);
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
  it.each([["available", true], ["unavailable (offline)", false]])("Cancel and Escape change nothing while Archive children too is %s", async (_state, online) => {
    for (const exit of ["cancel", "escape"] as const) {
      harness.fresh();
      const s = await ready({}, online);
      expect(confirm(s.tree).props.disabled).toBe(!online);
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
  it("shows a host read failure and keeps Archive children too unavailable", async () => {
    const s = await ready({ branch: vi.fn(async () => "Incomplete native inventory; no lifecycle action is allowed."), error: "Incomplete native inventory; no lifecycle action is allowed." });
    expect(confirm(s.tree).props.disabled).toBe(true);
    expect(texts(s.tree)).toContain("Incomplete native inventory; no lifecycle action is allowed.");
    expect(texts(s.tree)).not.toContain("Reading this session's child sessions on the host…");
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
    expect(branchDispositions(entries, [archived("outside")], revisions)).toEqual({});
  });
});
