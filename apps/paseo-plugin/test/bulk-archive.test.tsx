import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Each test gets a fresh renderer: a dialog's refs and state must never carry over from another test.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon" }));
vi.mock("../client/anchored-menu", () => ({ AnchoredMenu: "AnchoredMenu" }));
vi.mock("../client/control-help", () => ({ useActionHelp: () => ({ bindings: {}, dismiss() {} }) }));

import { ArchiveRowFeedback, ArchiveSelectionCheckbox, ArchiveSelectionToggle, BulkArchiveDialog, BulkArchiveToolbar, archiveIssue, useArchiveSelection } from "../client/bulk-archive";
import type { useSessionLifecycle } from "../client/use-session-lifecycle";
import { Action, Disclosure } from "../client/page-controls";
import type { LifecycleRow } from "../shared/session-lifecycle";

type Element = { type?: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk)
  : [node as Element, ...walk((node as Element).props.children), ...walk((node as Element).props.header)];
const colors = { foreground: "#111", foregroundMuted: "#666", surface0: "#fff", border: "#ccc", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
type Lifecycle = ReturnType<typeof useSessionLifecycle>;
// The real Paseo theme values used by session-state-marker.test.
const dark = { surface0: "#181B1A", surface1: "#1E2120", surface2: "#272A29", border: "#252B2A", foreground: "#fafafa", foregroundMuted: "#A1A5A4", accent: "#20744A", accentForeground: "#ffffff", statusSuccess: "#6cb17b", statusWarning: "#c09664", statusDanger: "#d8847b" };
const light = { surface0: "#ffffff", surface1: "#fafafa", surface2: "#f4f4f5", border: "#e4e4e7", foreground: "#1a1a1e", foregroundMuted: "#71717a", accent: "#20744A", accentForeground: "#ffffff", statusSuccess: "#3e704a", statusWarning: "#7b5d39", statusDanger: "#9d433b" };
const luminance = (hex: string) => { const [r, g, b] = hex.match(/[a-f\d]{2}/gi)!.map((value) => parseInt(value, 16) / 255).map((c) => c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4); return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!; };
const contrast = (a: string, b: string) => { const [low, high] = [luminance(a), luminance(b)].sort((x, y) => x - y); return (high! + 0.05) / (low! + 0.05); };
const row = (id: string, patch: Partial<LifecycleRow> = {}): LifecycleRow => ({ id, title: `Session ${id}`, workspaceId: null, cwd: "/fixture", provider: "codex", model: null, effort: null, mode: null, nativeStatus: "idle", fingerprint: id,
  category: "candidate", eligible: true, reason: "Archive stops this session.", children: [], outcome: "pending", message: null, ...patch });
const child = (id: string, status = "idle") => ({ id, title: `Child ${id}`, status, workspaceId: null });
/** Renders a component element one level, as the host would. */
const draw = (element: Element) => (element.type as (props: Record<string, unknown>) => Element)(element.props);
const texts = (tree: unknown) => walk(tree).filter((element) => element.type === "Text").map((element) => [element.props.children].flat().join(""));
const box = (pressable: Element) => walk(pressable).find((element) => String(element.props.testID).endsWith("-box"))!;
const styleOf = (element: Element) => element.props.style as Record<string, unknown>;
const dialog = (lifecycle: Partial<Lifecycle>, extra: { onClose?: () => void; onFinished?: (summary: string) => void } = {}) => {
  const props = { lifecycle: { busy: false, running: false, batch: null, error: null, stop: vi.fn(), run: vi.fn(), prepare: vi.fn(), reconcile: vi.fn(), ...lifecycle } as unknown as Lifecycle,
    root: { current: null }, anchor: { current: null }, rootSize: { width: 390, height: 800 }, colors, compact: true, onClose: extra.onClose ?? vi.fn(), onFinished: extra.onFinished ?? vi.fn() };
  return { props, render: () => harness.current.render(() => BulkArchiveDialog(props)) as Element };
};
const labelled = (tree: unknown, label: string | RegExp) => walk(tree).filter((element) => typeof element.props.label === "string" && (typeof label === "string" ? element.props.label === label : label.test(element.props.label)));
const review = (rows: LifecycleRow[]) => ({ id: "00000000-0000-4000-8000-000000000001", rows });
const batch = (patch: Record<string, unknown> = {}) => ({ currentId: null, completed: 0, total: 1, announcement: "", summary: null, ...patch });

beforeEach(() => harness.fresh());

describe("bulk archive selection accessibility", () => {
  it("keeps Select a labelled button with a 44pt target and help, and only starts or cancels selection", () => {
    for (const selecting of [false, true]) {
      const selection = { selecting, start: vi.fn(), clear: vi.fn() } as unknown as ReturnType<typeof useArchiveSelection>;
      const button = ArchiveSelectionToggle({ selection, disabled: false, colors });
      expect(button.props).toMatchObject({ accessibilityRole: "button", "aria-pressed": selecting,
        accessibilityLabel: selecting ? "Cancel archive selection" : "Select sessions for archive" });
      expect(button.props.accessibilityHint).toContain(selecting ? "without archiving" : "review and confirm");
      expect(walk(button).some(node => node.type === "Text")).toBe(false);
      expect(button.props.style({ pressed: false })).toMatchObject({ minWidth: 44, minHeight: 44 });
      expect(walk(button).some((node) => node.type === "Icon")).toBe(true);
      button.props.onPress();
      expect(selection.start).toHaveBeenCalledTimes(selecting ? 0 : 1);
      expect(selection.clear).toHaveBeenCalledTimes(selecting ? 1 : 0);
    }
  });
  it("draws a 22pt box inside a 44pt target with checked, unchecked, busy and disabled states and supports Space once", () => {
    const press = vi.fn(), element = draw(ArchiveSelectionCheckbox({ session: { id: "fixture-a", title: "Duplicate title" }, checked: true, disabled: false, pending: true, colors, onPress: press }));
    expect(element.props).toMatchObject({ accessibilityRole: "checkbox", accessibilityLabel: "Select Duplicate title (fixture-a) for archive", accessibilityState: { checked: true, disabled: false, busy: true }, "aria-checked": true });
    expect((element.props.style as (state: object) => object)({ pressed: false })).toMatchObject({ minWidth: 44, minHeight: 44, opacity: 1 });
    expect((element.props.style as (state: object) => object)({ pressed: false, focused: true })).toMatchObject({ borderWidth: 2, borderColor: colors.foreground });
    expect(styleOf(box(element))).toMatchObject({ width: 22, height: 22, borderWidth: 2 });
    expect(walk(element).find((node) => node.type === "Icon")!.props.name).toBe("Loader");
    const event = { key: " ", repeat: false, preventDefault: vi.fn(), stopPropagation: vi.fn() };
    (element.props.onKeyDown as (value: typeof event) => void)(event); (element.props.onKeyDown as (value: typeof event) => void)({ ...event, repeat: true });
    expect(press).toHaveBeenCalledTimes(1); expect(event.preventDefault).toHaveBeenCalledTimes(2);
    const disabled = draw(ArchiveSelectionCheckbox({ session: { id: "a", title: "A" }, checked: false, disabled: true, pending: false, colors, onPress: press }));
    (disabled.props.onKeyDown as (value: typeof event) => void)(event); expect(press).toHaveBeenCalledTimes(1);
    expect((disabled.props.style as (state: object) => object)({ pressed: false })).toMatchObject({ opacity: 0.4 });
    expect(walk(disabled).some((node) => node.type === "Icon")).toBe(false);
  });
  it.each([["dark", dark], ["light", light]] as const)("separates checked from unchecked by fill and mark with at least 3:1 contrast in the %s theme", (_name, theme) => {
    const make = (checked: boolean) => draw(ArchiveSelectionCheckbox({ session: { id: "a", title: "A" }, checked, disabled: false, pending: false, colors: theme as unknown as PluginTheme["colors"], onPress: vi.fn() }));
    const on = make(true), off = make(false), onBox = styleOf(box(on)), offBox = styleOf(box(off));
    expect(offBox.backgroundColor).toBeUndefined(); expect(walk(off).some((node) => node.type === "Icon")).toBe(false);
    expect(onBox.backgroundColor).toBe(theme.foreground); expect(walk(on).find((node) => node.type === "Icon")!.props).toMatchObject({ name: "Check", color: theme.surface0 });
    // Rows sit on surface0, a selected or hovered row on surface2, and the dialog on surface1.
    for (const surface of [theme.surface0, theme.surface1, theme.surface2]) {
      expect(contrast(offBox.borderColor as string, surface)).toBeGreaterThanOrEqual(3);
      expect(contrast(onBox.backgroundColor as string, surface)).toBeGreaterThanOrEqual(3);
    }
    expect(contrast(theme.surface0, onBox.backgroundColor as string)).toBeGreaterThanOrEqual(3);
  });
  it("caps individual selection at 50, refuses Select all above 50, prunes changed scope, removes exact IDs and resets on host switch", () => {
    const shown = Array.from({ length: 51 }, (_, n) => `fixture-${n}`);
    let scope = shown, host = "host-a";
    const draw = () => harness.current.render(() => useArchiveSelection(host, scope));
    draw(); draw().start(); draw().selectShown(); draw().toggle("outside-filtered-scope"); expect(draw().selected).toEqual([]);
    for (const id of shown) draw().toggle(id);
    expect(draw().selected).toHaveLength(50); draw().toggle(shown[0]!); expect(draw().selected).toHaveLength(49);
    draw().remove([shown[1]!, shown[2]!, "not-selected"]); expect(draw().selected).toHaveLength(47); expect(draw().selected).not.toContain(shown[1]);
    scope = [shown[3]!]; draw(); expect(draw().selected).toEqual([shown[3]]);
    host = "host-b"; draw(); expect(draw()).toMatchObject({ selected: [], selecting: false });
  });
  it("offers a three-state Select all with a cap hint, an Archive N… review step and one polite batch announcement", () => {
    const selection = (selected: string[]) => ({ selecting: true, selected, clear: vi.fn(), start: vi.fn(), selectShown: vi.fn(), remove: vi.fn() }) as unknown as ReturnType<typeof useArchiveSelection>;
    const idle = { running: false, batch: null } as unknown as Lifecycle, onReview = vi.fn();
    const bar = (chosen: ReturnType<typeof selection>, shownCount: number, lifecycle = idle) => BulkArchiveToolbar({ selection: chosen, shownCount, disabled: false, lifecycle, colors, onReview });
    const all = (tree: unknown) => draw(walk(tree).find((element) => element.props.testID === "sessions-select-all")!);
    const none = selection([]), some = selection(["a"]), every = selection(["a", "b"]);
    expect(all(bar(none, 2)).props).toMatchObject({ accessibilityLabel: "Select all 2 shown sessions", "aria-checked": false, disabled: false });
    expect(all(bar(some, 2)).props["aria-checked"]).toBe("mixed"); expect(walk(all(bar(some, 2))).find((node) => node.type === "Icon")!.props.name).toBe("Minus");
    expect(all(bar(every, 2)).props).toMatchObject({ accessibilityLabel: "Clear selection", "aria-checked": true });
    (all(bar(some, 2)).props.onPress as () => void)(); expect(some.selectShown).toHaveBeenCalledTimes(1);
    (all(bar(every, 2)).props.onPress as () => void)(); expect(every.remove).toHaveBeenCalledExactlyOnceWith(["a", "b"]);
    const capped = bar(some, 51);
    expect(all(capped).props).toMatchObject({ disabled: true, accessibilityHint: expect.stringContaining("narrow the view") });
    expect(texts(capped)).toEqual(expect.arrayContaining(["1 selected", "Max 50 per batch"]));
    expect(texts(bar(some, 2))).not.toContain("Max 50 per batch");
    const open = labelled(bar(some, 2), "Archive 1…")[0]!;
    expect(open.props).toMatchObject({ accessibilityLabel: "Review archive of 1 selected session", disabled: false }); (open.props.onPress as () => void)(); expect(onReview).toHaveBeenCalledTimes(1);
    expect(labelled(bar(none, 2), "Archive 0…")[0]!.props.disabled).toBe(true);
    const running = bar(some, 51, { running: true, batch: batch({ completed: 2, total: 4, announcement: "Archiving 4 selected sessions." }), stop: vi.fn(),
      review: review([row("a", { outcome: "archived", runtime: "closed" }), row("b", { outcome: "blocked" }), row("c"), row("d", { eligible: false })]) } as unknown as Lifecycle);
    expect(texts(running)).toContain("Archiving 3 of 3");
    const live = walk(running).filter((element) => element.props.accessibilityLiveRegion);
    expect(live).toHaveLength(1); expect(live[0]!.props).toMatchObject({ role: "status", accessibilityLiveRegion: "polite", children: "Archiving 4 selected sessions." });
    expect(labelled(running, "Stop before next session")).toHaveLength(1); expect(labelled(running, "Dismiss")).toHaveLength(0);
  });
  it("after a batch, deselects only blocked selected rows and dismisses results without archiving", () => {
    const chosen = { selecting: true, selected: ["blocked", "unconfirmed"], remove: vi.fn(), selectShown: vi.fn() } as unknown as ReturnType<typeof useArchiveSelection>;
    const lifecycle = { running: false, clear: vi.fn(), stop: vi.fn(), batch: batch({ announcement: "Archived 1 · 1 not confirmed · 2 blocked", summary: "x" }),
      review: review([row("done", { outcome: "archived", runtime: "closed" }), row("blocked", { outcome: "blocked" }), row("other-blocked", { outcome: "blocked" }), row("unconfirmed", { outcome: "uncertain" })]) } as unknown as Lifecycle;
    const tree = BulkArchiveToolbar({ selection: chosen, shownCount: 3, disabled: false, lifecycle, colors, onReview: vi.fn() });
    const deselect = labelled(tree, "Deselect blocked")[0]!;
    expect(deselect.props.accessibilityLabel).toBe("Deselect 1 blocked session");
    (deselect.props.onPress as () => void)(); expect(chosen.remove).toHaveBeenCalledExactlyOnceWith(["blocked"]);
    (labelled(tree, "Dismiss")[0]!.props.onPress as () => void)(); expect(lifecycle.clear).toHaveBeenCalledTimes(1);
    expect(labelled(BulkArchiveToolbar({ selection: chosen, shownCount: 3, disabled: false, lifecycle, colors, onReview: vi.fn(), announce: false }), "Dismiss")).toHaveLength(0);
  });
});

describe("bulk archive blocked reasons", () => {
  it("names unselected children exactly, offers only idle ones and never a running child", () => {
    const parent = row("p", { eligible: false, category: "excluded", reason: "Archive blocked: native archive can cascade to these children. Preserve or finish them through the owning workflow first.",
      children: [child("idle-1"), child("busy", "running"), child("selected"), child("starting", "initializing")] });
    const rows = [parent, row("selected")];
    expect(archiveIssue(parent, rows)).toEqual({ text: "3 active children not selected · 2 running", add: ["idle-1"], running: 2, check: false });
    expect(archiveIssue(parent)).toMatchObject({ text: "Has 4 active children", add: [] });
    expect(archiveIssue(row("q", { eligible: false, children: [child("x")] }), [row("q"), row("x")]).text).toBe("Archive stops this session.");
  });
  it("tells a parent waiting on its children, an earlier fence and an unconfirmed archive apart, from fields only", () => {
    const parent = row("p", { outcome: "blocked", message: "Selected children have not all archived with confirmed outcomes. No parent archive was sent.", children: [child("c")] });
    expect(archiveIssue(parent, [parent, row("c", { outcome: "uncertain" })]).text).toBe("Waits for its selected children to archive");
    expect(archiveIssue(parent, [parent, row("c", { outcome: "archived", runtime: "closed" })]).text).toBe("Selected children have not all archived with confirmed outcomes.");
    expect(archiveIssue(row("f", { outcome: "blocked", reconcileReviewId: "00000000-0000-4000-8000-000000000002", message: "An earlier archive owns this session. Check its native outcome; no second dispatch was made." })))
      .toMatchObject({ text: "An earlier archive owns this session.", check: true });
    expect(archiveIssue(row("u", { outcome: "archived", runtime: "unconfirmed", message: "Archived. The archive reply did not confirm completion (lost), so stopping its runtime is not confirmed." })))
      .toMatchObject({ text: "Archived, but stopping its runtime is not confirmed", check: true });
    expect(archiveIssue(row("n", { outcome: "blocked", message: "Native read failed: socket closed. Absence is not confirmed." })).text).toBe("Native read failed: socket closed.");
  });
  it("keeps the sessions-list marker to one short line and only for blocked or unconfirmed rows", () => {
    const line = (value: LifecycleRow) => texts(ArchiveRowFeedback({ row: value, colors }));
    expect(line(row("p", { outcome: "blocked", eligible: false, children: [child("a"), child("b")] }))).toEqual(["Archive blocked · Has 2 active children"]);
    expect(line(row("u", { outcome: "uncertain", message: "Lost acknowledgement; do not resend" }))).toEqual(["Archive not confirmed · Lost acknowledgement; do not resend"]);
    for (const settled of [row("a", { outcome: "archived", runtime: "not-observed" }), row("b", { outcome: "absent" }), row("c", { outcome: "already_archived" }), row("d")]) expect(ArchiveRowFeedback({ row: settled, colors })).toBeNull();
  });
});

describe("bulk archive confirmation", () => {
  const parent = row("parent", { title: "TPM • ASMA-7675 • QNR-P1", eligible: false, category: "excluded", reason: "Archive blocked: native archive can cascade to these children. Preserve or finish them through the owning workflow first.",
    children: [child("idle-child"), child("running-child", "running")] });
  it("opens compact: title, one ready/blocked summary, one consequence, grouped rows and Archive counting only ready rows", () => {
    const view = dialog({ review: review([row("a", { title: "A very long session title that wraps onto a second line on a 320 point phone screen" }), row("b"), parent]) as never });
    const tree = view.render(), copy = texts(tree);
    expect(tree.props).toMatchObject({ label: "Confirm bulk archive", dialog: true });
    expect(copy).toContain("Archive 3 sessions?"); expect(copy).toContain("2 ready · 1 blocked");
    expect(copy.filter((value) => value.includes("no undo") || value.includes("There is no undo"))).toEqual(["Archive stops active work and keeps history and files. There is no undo."]);
    expect(copy.some((value) => value.startsWith("Children archive before"))).toBe(false);
    expect(copy.filter((value) => /^(Blocked|Ready) · \d$/.test(value))).toEqual(["Blocked · 1", "Ready · 2"]);
    expect(copy).toContain("2 active children not selected · 1 running");
    expect(walk(tree).find((element) => element.type === "Text" && String(element.props.children).startsWith("A very long session title"))!.props.numberOfLines).toBe(2);
    // IDs and the full server reason wait in row details; nothing visible repeats the cascade paragraph.
    expect(copy.some((value) => value.includes("(parent)") || value.includes("Preserve or finish them"))).toBe(false);
    expect(walk(tree).filter((element) => element.props.role === "listitem")).toHaveLength(3);
    const archive = labelled(tree, "Archive 2 sessions"); expect(archive).toHaveLength(1); expect(archive[0]!.props.disabled).toBe(false);
    expect(labelled(tree, "Cancel")).toHaveLength(1);
  });
  it("keeps the preflight rule and the non-atomic cascade caveat verbatim in About archive, and adds a child-scope line when a parent is ready", () => {
    const tree = dialog({ review: review([row("c"), row("p", { children: [child("c")], reason: "All active managed children are selected." })]) as never }).render();
    const about = walk(tree).find((element) => element.type === Disclosure && element.props.label === "About archive")!;
    const inside = texts(about).join("\n");
    expect(inside).toContain("Only selected session IDs are directly requested for archive.");
    expect(inside).toContain("At preflight, a parent is blocked unless every active managed child is selected.");
    expect(inside).toContain("The child check and parent archive are not atomic: a new child created between the final check and parent archive may be archived by the host with its parent, even if unselected.");
    expect(texts(tree).join("\n")).not.toContain("Unselected sessions are never archived");
    expect(texts(tree)).toContain("Children archive before their parents. A child started during the run may be archived with its parent.");
    expect(texts(tree)).toContain("Archives after its 1 child");
  });
  it("shows a session's ID, full reason and exact children only in its details", () => {
    const view = dialog({ review: review([parent]) as never });
    let tree = view.render();
    const toggle = walk(tree).find((element) => element.props.accessibilityLabel === `Details for ${parent.title}`)!;
    expect(toggle.props).toMatchObject({ "aria-expanded": false }); expect((toggle.props.style as (state: object) => object)({ pressed: false })).toMatchObject({ width: 44, height: 44 });
    expect(texts(tree).some((value) => value === "ID parent" || value === parent.reason)).toBe(false);
    (toggle.props.onPress as () => void)(); tree = view.render();
    expect(texts(tree)).toEqual(expect.arrayContaining(["ID parent", parent.reason, "Child: Child idle-child · idle · idle-child", "Child: Child running-child · running · running-child"]));
  });
  it("adds only idle children by exact ID for one more review, keeps running ones out and respects the 50 cap", () => {
    const prepare = vi.fn(), tree = dialog({ review: review([row("a"), parent]) as never, prepare }).render();
    const add = labelled(tree, "Add 1 idle child")[0]!;
    expect(add.props.accessibilityLabel).toBe(`Add 1 idle child of ${parent.title} and review again`);
    expect(texts(tree)).toContain("Finish running children in their own workflow, then review again.");
    (add.props.onPress as () => void)();
    expect(prepare).toHaveBeenCalledExactlyOnceWith("archive", "filtered", ["a", "parent", "idle-child"], true);
    const full = dialog({ review: review([...Array.from({ length: 49 }, (_, n) => row(`r${n}`)), parent]) as never, prepare: vi.fn() }).render();
    expect(labelled(full, /^Add /)).toHaveLength(0); expect(texts(full)).toContain("Adding its children would exceed the 50-session batch cap.");
  });
  it("runs once, keeps the toast for a batch that finishes after the dialog closed, and cancels without archiving", async () => {
    const run = vi.fn(async () => "Archived 1"), onFinished = vi.fn(), onClose = vi.fn(), stop = vi.fn();
    const open = dialog({ review: review([row("a")]) as never, run, stop }, { onFinished, onClose });
    await (labelled(open.render(), "Archive 1 session")[0]!.props.onPress as () => Promise<void>)(); await Promise.resolve();
    expect(run).toHaveBeenCalledTimes(1); expect(onFinished).not.toHaveBeenCalled();
    harness.fresh();
    let finish = (_summary: string) => {};
    const later = dialog({ review: review([row("a")]) as never, run: vi.fn(() => new Promise<string>((resolve) => { finish = resolve; })), stop }, { onFinished, onClose });
    const tree = later.render();
    (labelled(tree, "Archive 1 session")[0]!.props.onPress as () => void)();
    (labelled(tree, "Close")[0]!.props.onPress as () => void)(); expect(stop).toHaveBeenCalledTimes(1); expect(onClose).toHaveBeenCalledTimes(1);
    finish("Stopped before the next session. Archived 1"); await new Promise((resolve) => setTimeout(resolve, 0));
    expect(onFinished).toHaveBeenCalledExactlyOnceWith("Stopped before the next session. Archived 1");
    harness.fresh();
    const cancel = vi.fn(), cancelled = dialog({ review: review([row("a")]) as never, run: cancel }, { onClose });
    (labelled(cancelled.render(), "Cancel")[0]!.props.onPress as () => void)(); expect(cancel).not.toHaveBeenCalled();
  });
  it("reports progress over the confirmed sessions in one status line and stops before the next session", () => {
    // The blocked parent's preflight request is in the batch total, but it is not one of the sessions confirmed for archive.
    const stop = vi.fn(), view = dialog({ review: review([row("a", { outcome: "archived", runtime: "closed" }), row("b"), row("c"), parent]) as never, running: true, stop,
      batch: batch({ currentId: "b", completed: 1, total: 4, announcement: "Archiving 4 selected sessions." }) as never });
    let tree = view.render();
    expect(texts(tree)).toEqual(expect.arrayContaining(["Archiving…", "Session 2 of 3", "Queued · 2", "Archiving…"]));
    expect(walk(tree).filter((element) => element.props.accessibilityLiveRegion)).toHaveLength(1);
    expect(labelled(tree, /^Archive \d+ sessions?$/)).toHaveLength(0); expect(labelled(tree, /^(Add|Check|Review)/)).toHaveLength(0);
    (labelled(tree, "Stop before next session")[0]!.props.onPress as () => void)(); tree = view.render();
    expect(stop).toHaveBeenCalledTimes(1); expect(texts(tree)).toContain("Stopping after session 2 of 3");
  });
  it("states a partial result once, keeps runtime and unconfirmed facts apart, checks without resending and reviews only blocked or unattempted rows again", () => {
    const prepare = vi.fn(), reconcile = vi.fn(), run = vi.fn(), onClose = vi.fn();
    const rows = [row("closed", { outcome: "archived", runtime: "closed" }), row("quiet", { outcome: "archived", runtime: "not-observed" }), row("lost", { outcome: "uncertain", message: "The request did not report a result. Read its durable receipt." }),
      row("fence", { outcome: "blocked", reconcileReviewId: "00000000-0000-4000-8000-000000000002", message: "An earlier archive owns this session. Check its native outcome." }),
      { ...parent, outcome: "blocked" as const, message: parent.reason }, row("skipped"), row("gone", { outcome: "absent" }), row("was", { outcome: "already_archived" })];
    const tree = dialog({ review: review(rows) as never, batch: batch({ total: 8, summary: "x", announcement: "x" }) as never, prepare, reconcile, run }, { onClose }).render();
    const copy = texts(tree);
    expect(copy).toContain("Archive finished");
    const summary = walk(tree).filter((element) => element.props.testID === "bulk-archive-summary");
    expect(summary).toHaveLength(1); expect(summary[0]!.props).toMatchObject({ role: "status", children: "Archived 2 · 1 not confirmed · 2 blocked · 1 absent (local removal only) · 1 already archived · 1 not attempted" });
    expect(walk(tree).find((element) => element.type === "Icon" && element.props.name === "CircleAlert")).toBeDefined();
    expect(copy.filter((value) => / · \d+$/.test(value))).toEqual(["Blocked · 2", "Not confirmed · 1", "Not attempted · 1", "Already archived · 1", "Not found on this host · 1"]);
    const archived = walk(tree).find((element) => element.type === Disclosure && element.props.label === "Archived · 2")!;
    expect(archived.props.open).toBeUndefined(); expect(texts(archived)).toEqual(expect.arrayContaining(["Runtime not reported closed"]));
    expect(texts(archived).filter((value) => value === "Runtime not reported closed")).toHaveLength(1);
    expect(copy.filter((value) => value === "archived" || value === "blocked")).toHaveLength(0);
    const checks = labelled(tree, "Check"); expect(checks).toHaveLength(2);
    for (const check of checks) (check.props.onPress as () => void)();
    expect(reconcile.mock.calls).toEqual([["fence"], ["lost"]]); expect(run).not.toHaveBeenCalled();
    (labelled(tree, "Review 2 again")[0]!.props.onPress as () => void)();
    expect(prepare).toHaveBeenCalledExactlyOnceWith("archive", "filtered", ["parent", "skipped"], true);
    expect(labelled(tree, /^Archive \d+ sessions?$/)).toHaveLength(0);
    (labelled(tree, "Done")[0]!.props.onPress as () => void)(); expect(onClose).toHaveBeenCalledTimes(1);
  });
  it("keeps results on screen during a Check, shows a fresh read only while previewing, and reports a failed read", () => {
    const rows = [row("lost", { outcome: "uncertain" })];
    const checking = dialog({ review: review(rows) as never, busy: true, batch: batch({ summary: "x" }) as never }).render();
    expect(texts(checking)).not.toContain("Reading selected sessions and managed children…"); expect(labelled(checking, "Check")[0]!.props.disabled).toBe(true);
    const reading = dialog({ review: review(rows) as never, busy: true }).render();
    expect(texts(reading).sort()).toEqual(["Archive selected sessions", "Reading selected sessions and managed children…"]); expect(labelled(reading, /^Archive \d/)).toHaveLength(0);
    const failed = dialog({ review: null, error: "Archive requires 1–50 explicitly selected session IDs in filtered scope." }).render();
    expect(walk(failed).find((element) => element.props.accessibilityRole === "alert")!.props.children).toContain("1–50");
    expect(walk(failed).some((element) => element.type === Action && element.props.label === "Close")).toBe(true);
  });
});
