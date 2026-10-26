import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

// Each test gets a fresh renderer: a dialog's refs and state must never carry over from another test.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, useWindowDimensions: () => ({ width: 390, height: 844, fontScale: 1 }), Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon", Modal: Object.assign(function Modal() { return null; }, { Content: "Modal.Content" }) }));
vi.mock("../client/anchored-menu", () => ({ AnchoredMenu: "AnchoredMenu" }));
vi.mock("../client/control-help", () => ({ useActionHelp: () => ({ bindings: {}, dismiss() {} }) }));

import { ArchiveRowFeedback, BulkArchiveDialog, BulkArchiveToolbar, archiveIssue, useArchiveSelection } from "../client/bulk-archive";
import type { useSessionLifecycle } from "../client/use-session-lifecycle";
import { SheetFooter } from "../client/operation-sheet";
import { Action, Disclosure } from "../client/page-controls";
import { SelectionBar, SelectionHeader } from "../client/session-selection";
import type { LifecycleRow } from "../shared/session-lifecycle";

type Element = { type?: unknown; props: Record<string, unknown> };
/** Every element under `node`; a sheet footer draws its buttons in place, so their labels and testIDs are visible. */
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk)
  : [node as Element, ...walk((node as Element).props.children), ...walk((node as Element).props.header),
    ...((node as Element).type === SheetFooter ? walk(SheetFooter((node as Element).props as Parameters<typeof SheetFooter>[0])) : [])];
const colors = { foreground: "#111", foregroundMuted: "#666", surface0: "#fff", border: "#ccc", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
type Lifecycle = ReturnType<typeof useSessionLifecycle>;
// The real Paseo theme values used by session-state-marker.test.
const dark = { surface0: "#181B1A", surface1: "#1E2120", surface2: "#272A29", border: "#252B2A", foreground: "#fafafa", foregroundMuted: "#A1A5A4", accent: "#20744A", accentForeground: "#ffffff", statusSuccess: "#6cb17b", statusWarning: "#c09664", statusDanger: "#d8847b" };
const light = { surface0: "#ffffff", surface1: "#fafafa", surface2: "#f4f4f5", border: "#e4e4e7", foreground: "#1a1a1e", foregroundMuted: "#71717a", accent: "#20744A", accentForeground: "#ffffff", statusSuccess: "#3e704a", statusWarning: "#7b5d39", statusDanger: "#9d433b" };
const luminance = (hex: string) => {
  const [r, g, b] = hex.match(/[a-f\d]{2}/gi)!.map((value) => parseInt(value, 16) / 255).map((channel) => channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4);
  return 0.2126 * r! + 0.7152 * g! + 0.0722 * b!;
};
const contrast = (a: string, b: string) => { const [low, high] = [luminance(a), luminance(b)].sort((x, y) => x - y); return (high! + 0.05) / (low! + 0.05); };
const row = (id: string, patch: Partial<LifecycleRow> = {}): LifecycleRow => ({ id, title: `Session ${id}`, workspaceId: null, cwd: "/fixture", provider: "codex", model: null, effort: null, mode: null, nativeStatus: "idle", fingerprint: id,
  category: "candidate", eligible: true, reason: "Archive stops this session.", children: [], outcome: "pending", message: null, ...patch });
const child = (id: string, status = "idle") => ({ id, title: `Child ${id}`, status, workspaceId: null });
/** Renders a component element one level, as the host would. */
const draw = (element: Element) => (element.type as (props: Record<string, unknown>) => Element)(element.props);
const texts = (tree: unknown) => walk(tree).filter((element) => element.type === "Text").map((element) => [element.props.children].flat().join(""));
const styleOf = (element: Element, state: object = { pressed: false }) => (element.props.style as (value: object) => Record<string, unknown>)(state);
const byId = (tree: unknown, testID: string) => walk(tree).find((element) => element.props.testID === testID)!;
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
  // Rows are their own checkboxes while selecting (dense-session-row.test); board D1's header holds Select all.
  const header = (patch: Partial<Parameters<typeof SelectionHeader>[0]> = {}) => SelectionHeader({ count: 1, total: 2, hidden: 0, colors: dark as unknown as PluginTheme["colors"], onDone: vi.fn(),
    selectAll: { checked: false, unavailable: null, onPress: vi.fn() }, ...patch }) as Element;
  it("draws D1's Select all as a labelled 44pt checkbox that is checked, clear or refused with its reason, and presses only when allowed", () => {
    const press = vi.fn(), off = byId(header({ selectAll: { checked: false, unavailable: null, onPress: press } }), "selection-all");
    expect(off.props).toMatchObject({ accessibilityRole: "checkbox", accessibilityLabel: "Select all 2 matching sessions", accessibilityState: { checked: false, disabled: false }, "aria-checked": false });
    expect(styleOf(off)).toMatchObject({ width: 44, height: 44, borderColor: "transparent", backgroundColor: undefined, opacity: 1 });
    expect(styleOf(off, { pressed: false, focused: true })).toMatchObject({ borderColor: dark.foreground });
    (off.props.onPress as () => void)(); expect(press).toHaveBeenCalledTimes(1);
    // Checked inverts to a foreground fill with a surface mark, at least 3:1 on every sheet surface in both themes.
    const on = byId(header({ count: 2, selectAll: { checked: true, unavailable: null, onPress: press } }), "selection-all");
    expect(on.props).toMatchObject({ accessibilityLabel: "Clear the 2 matching sessions", accessibilityState: { checked: true }, "aria-checked": true });
    expect(styleOf(on)).toMatchObject({ borderColor: "transparent", backgroundColor: dark.foreground });
    expect([on, off].map((element) => walk(element).find((node) => node.type === "Icon")!.props)).toMatchObject([{ name: "ListChecks", color: dark.surface0 }, { name: "ListChecks", color: dark.foreground }]);
    for (const theme of [dark, light]) {
      const drawn = byId(SelectionHeader({ count: 2, total: 2, hidden: 0, colors: theme as unknown as PluginTheme["colors"], onDone: vi.fn(), selectAll: { checked: true, unavailable: null, onPress: press } }), "selection-all");
      const fill = styleOf(drawn).backgroundColor as string, mark = walk(drawn).find((node) => node.type === "Icon")!.props.color as string;
      for (const surface of [theme.surface0, theme.surface1, theme.surface2]) expect(contrast(fill, surface)).toBeGreaterThanOrEqual(3);
      expect(contrast(mark, fill)).toBeGreaterThanOrEqual(3);
    }
    const reason = "The session list is not complete yet; select sessions one by one.";
    const refused = header({ selectAll: { checked: false, unavailable: reason, onPress: press } }), box = byId(refused, "selection-all");
    expect(box.props).toMatchObject({ accessibilityHint: reason, accessibilityState: { checked: false, disabled: true }, "aria-disabled": true });
    expect(styleOf(box)).toMatchObject({ opacity: 0.4 });
    (box.props.onPress as () => void)(); expect(press).toHaveBeenCalledTimes(1);
    expect(texts(refused)).toContain(reason);
  });
  it("counts only the shown selection over every match, names the hidden selection apart and ends with Done", () => {
    const done = vi.fn(), tree = header({ count: 3, total: 2, hidden: 2, onDone: done });
    expect(byId(tree, "selection-count").props).toMatchObject({ role: "status", accessibilityLiveRegion: "polite", children: "1 / 2 selected" });
    expect(byId(tree, "selection-hidden").props.children).toBe("2 selected sessions not shown by the current filters; they stay selected.");
    expect(byId(header({ count: 2, total: 5, hidden: 1 }), "selection-hidden").props.children).toBe("1 selected session not shown by the current filters; it stays selected.");
    expect(walk(header()).some((element) => element.props.testID === "selection-hidden")).toBe(false);
    const finish = byId(tree, "selection-done");
    expect(finish.props.accessibilityLabel).toBe("Done selecting"); expect(styleOf(finish)).toMatchObject({ minHeight: 44 });
    (finish.props.onPress as () => void)(); expect(done).toHaveBeenCalledTimes(1);
  });
  it("caps individual selection at 50, freezes it across filter changes, unions Select all within one batch, removes exact IDs and resets on host switch", () => {
    const shown = Array.from({ length: 51 }, (_, n) => `fixture-${n}`);
    let scope = shown, host = "host-a";
    const draw = () => harness.current.render(() => useArchiveSelection(host, scope));
    draw(); draw().start(); draw().selectShown(); draw().toggle("outside-filtered-scope"); expect(draw().selected).toEqual([]);
    for (const id of shown) draw().toggle(id);
    expect(draw().selected).toHaveLength(50); draw().toggle(shown[0]!); expect(draw().selected).toHaveLength(49);
    draw().remove([shown[1]!, shown[2]!, "not-selected"]); expect(draw().selected).toHaveLength(47); expect(draw().selected).not.toContain(shown[1]);
    // A narrower view drops nothing: what it hides is counted, any selected session can still be removed, and only a shown one can be added.
    scope = [shown[3]!, "fresh"]; draw(); expect(draw()).toMatchObject({ hidden: 46 }); expect(draw().selected).toHaveLength(47);
    draw().toggle(shown[4]!); expect(draw().selected).not.toContain(shown[4]); expect(draw().hidden).toBe(45);
    draw().toggle(shown[0]!); expect(draw().selected).not.toContain(shown[0]);
    draw().selectAll(["fresh", shown[3]!]); expect(draw().selected).toHaveLength(47); expect(draw().selected).toContain("fresh");
    draw().selectAll(["x1", "x2", "x3", "x4"]); expect(draw().selected).toHaveLength(47);
    draw().selectAll(["x1", "x2", "x3"]); expect(draw().selected).toHaveLength(50);
    host = "host-b"; draw(); expect(draw()).toMatchObject({ selected: [], selecting: false, hidden: 0 });
  });
  it("offers Take over N, its Auto picker and Archive N on D1's bar, each refused with one reason under the bar", () => {
    const control = (unavailable: string | null = null) => ({ unavailable, onPress: vi.fn() });
    const bar = (count: number, takeover = control(), auto = control(), archive = control()) => SelectionBar({ count, takeover, auto, archive, colors: dark as unknown as PluginTheme["colors"] }) as Element;
    const buttons = (tree: Element) => walk(tree).filter((element) => typeof element.props.label === "string").map((element) => draw(element));
    const archive = control(), takeover = control(), tree = bar(2, takeover, control(), archive);
    expect(buttons(tree).map((button) => [button.props.testID, button.props.accessibilityLabel])).toEqual([["selection-takeover", "Take over 2 selected sessions on each seat's chain"],
      ["selection-takeover-auto", "Choose a model for taking over 2 selected sessions"], ["selection-archive", "Review archive of 2 selected sessions"]]);
    expect(walk(tree).filter((element) => typeof element.props.label === "string").map((element) => element.props.label)).toEqual(["Take over 2", "Auto", "Archive 2"]);
    for (const button of buttons(tree)) { expect(button.props.accessibilityState).toEqual({ disabled: false }); expect(styleOf(button)).toMatchObject({ minHeight: 44 }); }
    (buttons(tree)[2]!.props.onPress as () => void)(); (buttons(tree)[0]!.props.onPress as () => void)();
    expect(archive.onPress).toHaveBeenCalledTimes(1); expect(takeover.onPress).toHaveBeenCalledTimes(1);
    expect(texts(tree)).toEqual([]);
    // Nothing selected refuses every action with one reason; a refused press does nothing.
    const none = bar(0, takeover, control(), archive);
    for (const button of buttons(none)) { expect(button.props).toMatchObject({ accessibilityHint: "Select sessions first.", accessibilityState: { disabled: true } }); (button.props.onPress as () => void)(); }
    expect(archive.onPress).toHaveBeenCalledTimes(1); expect(takeover.onPress).toHaveBeenCalledTimes(1);
    expect(texts(none)).toEqual(["Select sessions first."]);
    const busy = "A takeover is running; wait for its results.", waits = "Archive waits for the host and any running archive.", route = "Choosing a model for one takeover is not supported on this fixture host.";
    expect(texts(bar(1, control(busy), control(route), control(waits)))).toEqual([busy, waits]);
    const free = bar(1, control(), control(route), control());
    expect(texts(free)).toEqual([route]); expect(buttons(free).map((button) => button.props.accessibilityHint)).toEqual([undefined, route, undefined]);
  });
  it("leaves selecting to D1 and shows only a batch's progress, Stop and one polite announcement", () => {
    const selection = { selecting: true, selected: ["a"], remove: vi.fn() } as unknown as ReturnType<typeof useArchiveSelection>;
    expect(walk(BulkArchiveToolbar({ selection, lifecycle: { running: false, batch: null } as unknown as Lifecycle, colors })).map((element) => element.type)).toEqual(["ScrollView"]);
    const running = BulkArchiveToolbar({ selection, colors, lifecycle: { running: true, batch: batch({ completed: 2, total: 4, announcement: "Archiving 4 selected sessions." }), stop: vi.fn(),
      review: review([row("a", { outcome: "archived", runtime: "closed" }), row("b", { outcome: "blocked" }), row("c"), row("d", { eligible: false })]) } as unknown as Lifecycle });
    expect(texts(running)).toContain("Archiving 3 of 3");
    const live = walk(running).filter((element) => element.props.accessibilityLiveRegion);
    expect(live).toHaveLength(1); expect(live[0]!.props).toMatchObject({ role: "status", accessibilityLiveRegion: "polite", children: "Archiving 4 selected sessions." });
    expect(labelled(running, "Stop before next session")).toHaveLength(1); expect(labelled(running, "Dismiss")).toHaveLength(0);
  });
  it("after a batch, deselects only blocked selected rows and dismisses results without archiving", () => {
    const chosen = { selecting: true, selected: ["blocked", "unconfirmed"], remove: vi.fn(), selectShown: vi.fn() } as unknown as ReturnType<typeof useArchiveSelection>;
    const lifecycle = { running: false, clear: vi.fn(), stop: vi.fn(), batch: batch({ announcement: "Archived 1 · 1 not confirmed · 2 blocked", summary: "x" }),
      review: review([row("done", { outcome: "archived", runtime: "closed" }), row("blocked", { outcome: "blocked" }), row("other-blocked", { outcome: "blocked" }), row("unconfirmed", { outcome: "uncertain" })]) } as unknown as Lifecycle;
    const tree = BulkArchiveToolbar({ selection: chosen, lifecycle, colors });
    const deselect = labelled(tree, "Deselect blocked")[0]!;
    expect(deselect.props.accessibilityLabel).toBe("Deselect 1 blocked session");
    (deselect.props.onPress as () => void)(); expect(chosen.remove).toHaveBeenCalledExactlyOnceWith(["blocked"]);
    (labelled(tree, "Dismiss")[0]!.props.onPress as () => void)(); expect(lifecycle.clear).toHaveBeenCalledTimes(1);
    expect(labelled(BulkArchiveToolbar({ selection: chosen, lifecycle, colors, announce: false }), "Dismiss")).toHaveLength(0);
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
      .toMatchObject({ text: "Archived · runtime stop not confirmed", check: true });
    expect(archiveIssue(row("l", { outcome: "uncertain", message: "Archive returned but exact-ID terminal retirement readback is unavailable. Check native outcome; do not retry the request." })))
      .toMatchObject({ text: "Archive result unknown", check: true });
    expect(archiveIssue(row("n", { outcome: "blocked", message: "Native read failed: socket closed. Absence is not confirmed." })).text).toBe("Native read failed: socket closed.");
  });
  it("keeps the sessions-list marker to one short line and only for blocked or unconfirmed rows", () => {
    const line = (value: LifecycleRow) => texts(ArchiveRowFeedback({ row: value, colors }));
    expect(line(row("p", { outcome: "blocked", eligible: false, children: [child("a"), child("b")] }))).toEqual(["Archive blocked · Has 2 active children"]);
    expect(line(row("u", { outcome: "uncertain", message: "Lost acknowledgement; do not resend" }))).toEqual(["Archive result unknown"]);
    expect(line(row("v", { outcome: "archived", runtime: "unconfirmed" }))).toEqual(["Archived · runtime stop not confirmed"]);
    for (const settled of [row("a", { outcome: "archived", runtime: "not-observed" }), row("b", { outcome: "absent" }), row("c", { outcome: "already_archived" }), row("d")]) expect(ArchiveRowFeedback({ row: settled, colors })).toBeNull();
  });
});

describe("bulk archive confirmation", () => {
  const SCOPE = "A new child session created just before a selected session archives may be archived with it, even if unselected.";
  const parent = row("parent", { title: "TPM • ASMA-7675 • QNR-P1", eligible: false, category: "excluded", reason: "Archive blocked: native archive can cascade to these children. Preserve or finish them through the owning workflow first.",
    children: [child("idle-child"), child("running-child", "running")] });
  it("opens compact: title, one ready/blocked summary, one consequence, grouped rows and Archive counting only ready rows", () => {
    const view = dialog({ review: review([row("a", { title: "A very long session title that wraps onto a second line on a 320 point phone screen" }), row("b"), parent]) as never });
    const tree = view.render(), copy = texts(tree);
    expect(tree.props).toMatchObject({ label: "Confirm bulk archive", dialog: true });
    expect(copy).toContain("Archive 3 sessions"); expect(copy).toContain("2 ready · 1 blocked");
    expect(copy).toEqual(expect.arrayContaining(["Only the selected sessions are requested for archive.", "Archived sessions stay available in Kontor."]));
    expect(copy.filter((value) => value.includes("no undo") || value.includes("There is no undo"))).toEqual(["Archive stops active work and keeps history and files. There is no undo."]);
    expect(copy.some((value) => value.startsWith("Children archive before"))).toBe(false);
    expect(copy.filter((value) => /^(Blocked|Ready) · \d$/.test(value))).toEqual(["Blocked · 1", "Ready · 2"]);
    expect(copy).toContain("2 active children not selected · 1 running");
    expect(walk(tree).find((element) => element.type === "Text" && String(element.props.children).startsWith("A very long session title"))!.props.numberOfLines).toBe(2);
    // IDs and the full server reason wait in row details; nothing visible repeats the cascade paragraph.
    expect(copy.some((value) => value.includes("(parent)") || value.includes("Preserve or finish them"))).toBe(false);
    expect(walk(tree).filter((element) => element.props.role === "listitem")).toHaveLength(3);
    const archive = labelled(tree, "Archive 2"); expect(archive).toHaveLength(1); expect(archive[0]!.props).toMatchObject({ accessibilityLabel: "Archive 2 sessions", disabled: false });
    expect(labelled(tree, "Cancel")).toHaveLength(1);
  });
  it("keeps the preflight rule and the non-atomic cascade caveat verbatim in About archive, and leads the scope line with child order when a parent is ready", () => {
    const tree = dialog({ review: review([row("c"), row("p", { children: [child("c")], reason: "All active managed children are selected." })]) as never }).render();
    const about = walk(tree).find((element) => element.type === Disclosure && element.props.label === "About archive")!;
    const inside = texts(about).join("\n");
    expect(inside).toContain("Only selected session IDs are directly requested for archive.");
    expect(inside).toContain("At preflight, a parent is blocked unless every active managed child is selected.");
    expect(inside).toContain("The child check and parent archive are not atomic: a new child created between the final check and parent archive may be archived by the host with its parent, even if unselected.");
    expect(texts(tree).join("\n")).not.toContain("Unselected sessions are never archived");
    expect(texts(tree)).toContain(`Children archive before their parents. ${SCOPE}`);
    expect(texts(tree)).toContain("Archives after its 1 child");
  });
  it("AUD PR476 states the unselected-child scope before confirm with every disclosure closed, even when no selected session has children", () => {
    // Renders each Disclosure as the host does, closed: only what shows before About opens counts as consent.
    const shown = (node: unknown): unknown => Array.isArray(node) ? node.map(shown) : !node || typeof node !== "object" ? node
      : (node as Element).type === Disclosure ? shown(Disclosure((node as Element).props as never))
      : { ...(node as Element), props: { ...(node as Element).props, children: shown((node as Element).props.children), header: shown((node as Element).props.header) } };
    for (const [rows, line] of [[[row("leaf")], SCOPE], [[row("c"), row("p", { children: [child("c")] })], `Children archive before their parents. ${SCOPE}`]] as const) {
      harness.fresh();
      const view = dialog({ review: review([...rows]) as never }), copy = texts(harness.current.render(() => shown(BulkArchiveDialog(view.props))));
      expect(copy).toContain(line); expect(copy).toContain("▸ About archive");
      expect(copy.join("\n")).not.toContain("are not atomic");
      expect(labelled(harness.current.render(() => BulkArchiveDialog(view.props)), `Archive ${rows.length}`)[0]!.props.disabled).toBe(false);
    }
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
    await (labelled(open.render(), "Archive 1")[0]!.props.onPress as () => Promise<void>)(); await Promise.resolve();
    expect(run).toHaveBeenCalledTimes(1); expect(onFinished).not.toHaveBeenCalled();
    harness.fresh();
    let finish = (_summary: string) => {};
    const later = dialog({ review: review([row("a")]) as never, run: vi.fn(() => new Promise<string>((resolve) => { finish = resolve; })), stop }, { onFinished, onClose });
    const tree = later.render();
    (labelled(tree, "Archive 1")[0]!.props.onPress as () => void)();
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
    expect(labelled(tree, /^Archive \d+$/)).toHaveLength(0); expect(labelled(tree, /^(Add|Check|Review)/)).toHaveLength(0);
    (labelled(tree, "Stop before next session")[0]!.props.onPress as () => void)(); tree = view.render();
    expect(stop).toHaveBeenCalledTimes(1); expect(texts(tree)).toContain("Stopping after session 2 of 3");
  });
  it("states a partial result once, keeps runtime and unconfirmed facts apart, checks without resending and reviews only blocked or unattempted rows again", () => {
    const prepare = vi.fn(), reconcile = vi.fn(), run = vi.fn(), onClose = vi.fn();
    const rows = [row("closed", { outcome: "archived", runtime: "closed" }), row("quiet", { outcome: "archived", runtime: "not-observed" }), row("lost", { outcome: "uncertain", message: "The request did not report a result. Read its durable receipt." }),
      row("fence", { outcome: "blocked", reconcileReviewId: "00000000-0000-4000-8000-000000000002", message: "An earlier archive owns this session. Check its native outcome." }),
      { ...parent, outcome: "blocked" as const, message: parent.reason }, row("skipped"), row("gone", { outcome: "absent" }), row("was", { outcome: "already_archived" })];
    const view = dialog({ review: review(rows) as never, batch: batch({ total: 8, summary: "x", announcement: "x" }) as never, prepare, reconcile, run }, { onClose }), tree = view.render();
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
    const checks = labelled(tree, "Refresh status"); expect(checks).toHaveLength(2);
    expect(checks.map((check) => check.props.accessibilityLabel)).toEqual(["Refresh archive status: Session fence", "Refresh archive status: Session lost"]);
    for (const check of checks) (check.props.onPress as () => void)();
    expect(reconcile.mock.calls).toEqual([["fence"], ["lost"]]); expect(run).not.toHaveBeenCalled();
    (labelled(tree, "Review 2 again")[0]!.props.onPress as () => void)();
    expect(prepare).toHaveBeenCalledExactlyOnceWith("archive", "filtered", ["parent", "skipped"], true);
    expect(labelled(tree, /^Archive \d+$/)).toHaveLength(0);
    // Review again replaces the rows; the sessions the first batch archived still leave the caller's selection on Close.
    Object.assign(view.props.lifecycle, { review: review([{ ...parent }, row("skipped")]), batch: null });
    const again = view.render();
    (byId(again, "bulk-archive-cancel").props.onPress as () => void)(); expect(onClose).toHaveBeenCalledExactlyOnceWith(["closed", "quiet", "gone", "was"]);
  });
  it("keeps results on screen during a Refresh status, shows a fresh read only while previewing, and reports a failed read", () => {
    const rows = [row("lost", { outcome: "uncertain" })];
    const checking = dialog({ review: review(rows) as never, busy: true, batch: batch({ summary: "x" }) as never }).render();
    expect(texts(checking)).not.toContain("Reading selected sessions and managed children…"); expect(labelled(checking, "Refresh status")[0]!.props.disabled).toBe(true);
    const reading = dialog({ review: review(rows) as never, busy: true }).render();
    expect(texts(reading).sort()).toEqual(["Archive selected sessions", "Reading selected sessions and managed children…"]); expect(labelled(reading, /^Archive \d/)).toHaveLength(0);
    const failed = dialog({ review: null, error: "Archive requires 1–50 explicitly selected session IDs in filtered scope." }).render();
    expect(walk(failed).find((element) => element.props.accessibilityRole === "alert")!.props.children).toContain("1–50");
    expect(walk(failed).some((element) => element.type === Action && element.props.label === "Close")).toBe(true);
  });
  it("keeps D4's footer to one row: the dismissal LEFT, the wider primary RIGHT, in review, while running and after results", () => {
    const footer = (tree: unknown) => walk(byId(tree, "operation-sheet-footer")).filter((element) => typeof element.props.label === "string");
    const reviewing = footer(dialog({ review: review([row("a"), row("b")]) as never }).render());
    expect(reviewing.map((button) => [button.props.testID, button.props.label])).toEqual([["bulk-archive-cancel", "Cancel"], ["bulk-archive-confirm", "Archive 2"]]);
    expect(reviewing.map((button) => styleOf(draw(button)))).toMatchObject([{ flex: 1, minHeight: 44 }, { flex: 1.5, minHeight: 44 }]);
    harness.fresh();
    const running = footer(dialog({ review: review([row("a"), row("b")]) as never, running: true, batch: batch({ currentId: "a" }) as never }).render());
    expect(running.map((button) => [button.props.testID, button.props.label])).toEqual([["bulk-archive-stop", "Stop before next session"]]);
    harness.fresh();
    const finished = footer(dialog({ review: review([row("a", { outcome: "archived", runtime: "closed" }), row("b", { outcome: "blocked" })]) as never, batch: batch({ summary: "x" }) as never }).render());
    expect(finished.map((button) => [button.props.testID, button.props.label])).toEqual([["bulk-archive-close", "Close"], ["bulk-archive-again", "Review 1 again"]]);
    harness.fresh();
    expect(footer(dialog({ review: review([row("a", { outcome: "archived", runtime: "closed" })]) as never, batch: batch({ summary: "x" }) as never }).render())
      .map((button) => button.props.testID)).toEqual(["bulk-archive-close"]);
  });
  it("opens in the host's modal where a surface has no anchor, with the same title, body and footer, and closes as Cancel does", () => {
    const onClose = vi.fn(), stop = vi.fn(), view = dialog({ review: review([row("a")]) as never, stop }, { onClose });
    const { root: _root, anchor: _anchor, rootSize: _size, ...props } = view.props;
    const tree = harness.current.render(() => BulkArchiveDialog(props)) as Element;
    expect(tree.props).toMatchObject({ title: "Archive 1 session", open: true });
    expect(byId(tree, "bulk-archive-dialog").props.accessibilityLabel).toBe("Confirm bulk archive");
    expect(texts(tree)).toContain("Only the selected sessions are requested for archive.");
    expect(byId(tree, "bulk-archive-confirm").props).toMatchObject({ label: "Archive 1", accessibilityLabel: "Archive 1 session" });
    (tree.props.onOpenChange as (open: boolean) => void)(false);
    expect(stop).toHaveBeenCalledTimes(1); expect(onClose).toHaveBeenCalledTimes(1);
  });
});
