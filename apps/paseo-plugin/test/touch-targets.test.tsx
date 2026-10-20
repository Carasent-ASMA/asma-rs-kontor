import { isValidElement, type ReactElement, type ReactNode } from "react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SourceRead } from "../shared/read-sources";
import type { Colors } from "../client/page-controls";
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useContext: () => null, useRef: (current: unknown) => ({ current }), useState: (initial: unknown) => [initial, () => {}], useMemo: (factory: () => unknown) => factory(), useEffect: () => undefined }));
vi.mock("react-native", () => ({ PanResponder: { create: () => ({ panHandlers: {} }) }, Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
vi.mock("@getpaseo/plugin/client", () => ({ useHosts: () => [] }));
type Element = ReactElement<Record<string, unknown>>;
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return elements((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return [node, ...elements(node.props.children as ReactNode)];
}
const colors: Colors = {
  surface0: "#ffffff", surface1: "#fafafa", surface2: "#f4f4f5", border: "#e4e4e7",
  foreground: "#1a1a1e", foregroundMuted: "#71717a", accent: "#20744A", accentForeground: "#ffffff",
  statusSuccess: "#3e704a", statusWarning: "#7b5d39", statusDanger: "#9d433b",
};
const read: SourceRead = { source: { id: "plan", title: "Plan", kind: "plan", path: "/configured/plan.md", projectId: null, epic: null, workstream: null, revisionKind: "working copy" }, status: "ready", revision: "a".repeat(64), readAt: "2026-10-19T10:00:00Z", modifiedAt: null, content: "# Plan", graph: null, records: null, diagnostics: [] };
/** Loads the controls fresh, so the pointer probe runs against the stubbed environment. */
async function heights(pointer: "none" | "coarse" | "fine") {
  vi.resetModules();
  if (pointer !== "none") vi.stubGlobal("matchMedia", (query: string) => ({ matches: query === "(pointer: coarse)" && pointer === "coarse" }));
  const { Disclosure } = await import("../client/page-controls");
  const { SourceDetails } = await import("../client/source-details");
  const height = (tree: ReactNode, label: string) => (elements(tree).find((node) => node.type === "Pressable" && String(node.props.accessibilityLabel).startsWith(label))!.props.style as { minHeight: number }).minHeight;
  return {
    disclosure: height(Disclosure({ label: "Configured chains", colors, children: null }), "Configured chains"),
    small: height(Disclosure({ label: "Legend", colors, children: null, small: true }), "Legend"),
    details: height(SourceDetails({ read, colors }), "Source details"),
  };
}
/** Every Sessions toolbar and Kontor navigation target, under the same pointer probe. */
async function controls(pointer: "none" | "coarse" | "fine") {
  vi.resetModules();
  if (pointer !== "none") vi.stubGlobal("matchMedia", (query: string) => ({ matches: query === "(pointer: coarse)" && pointer === "coarse" }));
  const { Action, IconButton } = await import("../client/page-controls");
  const { MenuItem } = await import("../client/anchored-menu");
  const { ClearFiltersButton, FilterChip, FiltersButton, OptionSelect } = await import("../client/sessions-screen");
  const style = (tree: ReactNode, label: string) => {
    const found = elements(tree).find((node) => node.type === "Pressable" && String(node.props.accessibilityLabel ?? node.props.label).startsWith(label))!.props.style;
    return (typeof found === "function" ? found({ pressed: false }) : found) as { minHeight?: number; minWidth?: number; width?: number; height?: number };
  };
  const onPress = () => {};
  const small = style(Action({ label: "Plan", small: true, colors, onPress }), "Plan");
  const back = style(IconButton({ label: "Back", icon: "ChevronLeft", colors, onPress }), "Back");
  return {
    chip: style(FilterChip({ label: "Running", active: false, colors, onPress }), "Running").minHeight,
    compactChip: style(FilterChip({ label: "Running", active: false, colors, compact: true, onPress }), "Running").minHeight,
    select: style(OptionSelect({ label: "Workspace scope", value: "Durable desks", expanded: false, colors, compact: false, anchor: { current: null }, testID: "sessions-scope", onPress }), "Workspace scope").minHeight,
    filters: style(FiltersButton({ activeCount: 0, expanded: false, colors, compact: false, onPress }), "Filters").minHeight,
    clear: style(ClearFiltersButton({ colors, compact: false, onPress }), "Clear filters").minHeight,
    tab: style(Action({ label: "Overview", tab: true, colors, onPress }), "Overview").minHeight,
    more: style(Action({ label: "More", trailing: "⌄", colors, onPress }), "More").minHeight,
    small: [small.minWidth, small.minHeight],
    menuItem: style(MenuItem({ label: "Durable desks", colors, onPress }), "Durable desks").minHeight,
    back: [back.width, back.height],
  };
}
/** A wide session row's own targets: the row, its details, favorite and Arrange handle, under the same pointer probe. */
async function rowTargets(pointer: "none" | "coarse" | "fine") {
  vi.resetModules();
  if (pointer !== "none") vi.stubGlobal("matchMedia", (query: string) => ({ matches: query === "(pointer: coarse)" && pointer === "coarse" }));
  const { OrganizationRowView } = await import("../client/organization-row");
  const { SessionDetailsButton, SessionRow } = await import("../client/sessions-screen");
  const { sessionEntry } = await import("./session-fixtures");
  const entry = sessionEntry("qa", { title: "QA • review" });
  const onPress = () => {};
  const style = (tree: ReactNode, label: string) => {
    const found = elements(tree).find((node) => node.type === "Pressable" && String(node.props.accessibilityLabel).startsWith(label))!.props.style;
    return (typeof found === "function" ? found({ pressed: false }) : found) as { minHeight?: number; width?: number; height?: number };
  };
  const row = OrganizationRowView({ row: { kind: "session", key: "session:qa", parentKey: "root", depth: 1, entry } as never, colors, compact: false, disabled: false, arranging: true, selected: false, favorite: false,
    details: <SessionDetailsButton title={entry.title} expanded={false} compact={false} colors={colors} onPress={onPress} />, onFavorite: onPress, onToggle: onPress, onMove: onPress, onSelect: onPress,
    children: (SessionRow as unknown as { type: (props: object) => ReactNode }).type({ entry, colors, compact: false, onPress }) });
  const details = style(row, "Details: QA • review"), favorite = style(row, "Favorite QA • review"), arrange = style(row, "Reorder QA • review");
  return { row: style(row, "QA • review.").minHeight, details: [details.width, details.height], favorite: [favorite.width, favorite.height], arrange: [arrange.width, arrange.minHeight] };
}
afterEach(() => { vi.unstubAllGlobals(); vi.resetModules(); });
describe("wide session row touch targets", () => {
  // Real bounds beside each other, so a tap on one never lands on its neighbour; not a hit slop.
  it.each([["none", "a native app"], ["coarse", "a tablet or touch-screen browser"]] as const)("gives the row, its details, favorite and Arrange handle 44pt at any width (%s: %s)", async (pointer, _where) => {
    expect(await rowTargets(pointer)).toEqual({ row: 44, details: [44, 44], favorite: [44, 44], arrange: [44, 44] });
  });
  it("keeps the dense row for a mouse or trackpad", async () => {
    expect(await rowTargets("fine")).toEqual({ row: 40, details: [30, 30], favorite: [30, 30], arrange: [28, 32] });
  });
});
describe("disclosure touch targets", () => {
  it.each([["none", "a native app"], ["coarse", "a phone or tablet browser"]] as const)("gives every disclosure a 44pt row with no web pointer probe or a coarse pointer (%s: %s)", async (pointer, _where) => {
    expect(await heights(pointer)).toEqual({ disclosure: 44, small: 44, details: 44 });
  });
  it("keeps the denser rows for a mouse or trackpad", async () => {
    expect(await heights("fine")).toEqual({ disclosure: 36, small: 30, details: 30 });
  });
});
describe("chip, select, tab, menu and Back touch targets", () => {
  // Real heights, not a hit slop: React Native Web ignores hitSlop.
  it.each([["none", "a native app"], ["coarse", "a phone or tablet browser"]] as const)("gives every one a 44pt target in both layouts (%s: %s)", async (pointer, _where) => {
    expect(await controls(pointer)).toEqual({ chip: 44, compactChip: 44, select: 44, filters: 44, clear: 44, tab: 44, more: 44, small: [44, 44], menuItem: 44, back: [44, 44] });
  });
  it("keeps the denser controls for a mouse or trackpad", async () => {
    expect(await controls("fine")).toEqual({ chip: 30, compactChip: 36, select: 30, filters: 36, clear: 32, tab: 32, more: 32, small: [30, 30], menuItem: 32, back: [34, 34] });
  });
});
