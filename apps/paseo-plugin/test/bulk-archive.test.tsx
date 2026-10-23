import type { PluginTheme } from "@getpaseo/plugin";
import { describe, expect, it, vi } from "vitest";

const harness = await vi.hoisted(async () => (await import("./hermes-scoping")).deferredReact());
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...harness.react }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon" }));
vi.mock("../client/anchored-menu", () => ({ AnchoredMenu: "AnchoredMenu" }));
vi.mock("../client/control-help", () => ({ useActionHelp: () => ({ bindings: {}, dismiss() {} }) }));

import { ArchiveSelectionCheckbox, ArchiveSelectionToggle, BulkArchiveDialog, BulkArchiveToolbar, useArchiveSelection } from "../client/bulk-archive";
import type { useSessionLifecycle } from "../client/use-session-lifecycle";
import { Action, Disclosure } from "../client/page-controls";

type Element = { type?: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk)
  : [node as Element, ...walk((node as Element).props.children), ...walk((node as Element).props.header)];
const colors = { foreground: "#111", foregroundMuted: "#666", surface0: "#fff", border: "#ccc", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
type Lifecycle = ReturnType<typeof useSessionLifecycle>;

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
  it("renders a labelled 44pt checkbox with checked/busy states and supports Space once", () => {
    const press = vi.fn(), element = ArchiveSelectionCheckbox({ session: { id: "fixture-a", title: "Duplicate title" }, checked: true, disabled: false, pending: true, colors, onPress: press });
    expect(element.props).toMatchObject({ accessibilityRole: "checkbox", accessibilityLabel: "Select Duplicate title (fixture-a) for archive", accessibilityState: { checked: true, disabled: false, busy: true }, style: { minWidth: 44, minHeight: 44 } });
    const event = { key: " ", repeat: false, preventDefault: vi.fn(), stopPropagation: vi.fn() };
    element.props.onKeyDown(event); element.props.onKeyDown({ ...event, repeat: true });
    expect(press).toHaveBeenCalledTimes(1); expect(event.preventDefault).toHaveBeenCalledTimes(2);
    const disabled = ArchiveSelectionCheckbox({ session: { id: "a", title: "A" }, checked: false, disabled: true, pending: false, colors, onPress: press });
    disabled.props.onKeyDown(event); expect(press).toHaveBeenCalledTimes(1);
  });
  it("caps individual selection at 50, refuses Select all above 50, prunes changed scope and resets on host switch", () => {
    // This hook has only state/effects; use a fresh renderer so other component tests cannot share its state.
    const shown = Array.from({ length: 51 }, (_, n) => `fixture-${n}`);
    let scope = shown, host = "host-a";
    const draw = () => harness.render(() => useArchiveSelection(host, scope));
    draw(); draw().start(); draw().selectShown(); draw().toggle("outside-filtered-scope"); expect(draw().selected).toEqual([]);
    for (const id of shown) draw().toggle(id);
    expect(draw().selected).toHaveLength(50); draw().toggle(shown[0]!); expect(draw().selected).toHaveLength(49);
    scope = [shown[1]!]; draw(); expect(draw().selected).toEqual([shown[1]]);
    host = "host-b"; draw(); expect(draw()).toMatchObject({ selected: [], selecting: false });
  });
  it("disables select-all above the cap and exposes one polite batch announcement", () => {
    const lifecycle = { running: true, batch: { completed: 2, total: 4, announcement: "Archiving 4 selected sessions." }, stop: vi.fn() } as unknown as Lifecycle;
    const selection = { selecting: true, selected: ["a"], clear: vi.fn(), start: vi.fn(), selectShown: vi.fn() } as unknown as ReturnType<typeof useArchiveSelection>;
    const tree = BulkArchiveToolbar({ selection, shownCount: 51, disabled: false, lifecycle, colors, onReview: vi.fn() });
    expect(walk(tree).find((element) => element.props.label === "Select all shown (51)")!.props.disabled).toBe(true);
    const live = walk(tree).filter((element) => element.props.accessibilityLiveRegion);
    expect(live).toHaveLength(1); expect(live[0]!.props).toMatchObject({ role: "status", accessibilityLiveRegion: "polite", children: "Archiving 4 selected sessions." });
  });
  it("discloses direct request scope and the concurrent-child cascade risk before one explicit confirmation", async () => {
    const onFinished = vi.fn(), run = vi.fn(async () => "Archived 1"), lifecycle = { review: { rows: [{ id: "a", title: "A", eligible: true, outcome: "pending", reason: "Ready" }] }, busy: false, running: false, batch: null, stop: vi.fn(), run } as unknown as Lifecycle;
    const anchor = { current: null };
    const tree = harness.render(() => BulkArchiveDialog({ lifecycle, root: { current: null }, anchor, rootSize: { width: 390, height: 800 }, colors, compact: true, onClose: vi.fn(), onFinished }));
    expect(tree.props.anchor).toBe(anchor);
    const copy = walk(tree).filter((element) => element.type === "Text").map((element) => element.props.children).join("\n");
    expect(copy).toContain("Only selected session IDs are directly requested for archive.");
    expect(copy).toContain("At preflight, a parent is blocked unless every active managed child is selected.");
    expect(copy).toContain("The child check and parent archive are not atomic: a new child created between the final check and parent archive may be archived by the host with its parent, even if unselected.");
    expect(copy).not.toContain("Unselected sessions are never archived");
    const list = walk(tree).find((element) => element.type === Disclosure)!;
    expect(list.props).toMatchObject({ label: "Selected sessions (1)" }); expect(list.props.open).toBeUndefined();
    expect((list.props.colors as PluginTheme["colors"]).accent).toBe(colors.foreground);
    const confirm = walk(tree).filter((element) => element.type === Action && element.props.label === "Archive 1 session");
    expect(confirm).toHaveLength(1); expect(run).not.toHaveBeenCalled(); await (confirm[0]!.props.onPress as () => Promise<void>)(); expect(run).toHaveBeenCalledTimes(1); expect(onFinished).toHaveBeenCalledExactlyOnceWith("Archived 1");
    expect(walk(tree).some((element) => element.props.label === "Retry Archive")).toBe(false);
  });
});
