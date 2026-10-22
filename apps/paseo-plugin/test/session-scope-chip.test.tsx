import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactElement } from "react";
const help = vi.hoisted(() => ({ show: vi.fn(), hide: vi.fn(), focus: vi.fn(), blur: vi.fn(), dismiss: vi.fn(), text: "" }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useMemo: (fn: () => unknown) => fn() }));
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View", PanResponder: { create: () => ({ panHandlers: {} }) } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
vi.mock("../client/control-help", () => ({ useControlHelp: (text: string) => { help.text = text; return {
  bindings: { "aria-describedby": "scope-help", onHoverIn: help.show, onHoverOut: help.hide, onFocus: help.focus, onBlur: help.blur }, dismiss: help.dismiss,
}; } }));
import { SESSION_SHORTCUTS, SessionScopeChip } from "../client/session-scope-chip";
import { buildOrganizationTree, nativeDesk, matchesScope, flattenOrganizationTree, scopeKeepsGroup, filterOrganizationTree, aggregateState } from "../client/session-organization";
import { deskIcon, organizationIcon } from "../client/desk-icon";
import { sessionEntry } from "./session-fixtures";
import { groupStateHelp, groupStateSummary, OrganizationRowView, StateBadge } from "../client/organization-row";

type Element = ReactElement<Record<string, any>>;
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk) : [node as Element, ...walk((node as Element).props.children)];
const colors = { foreground: "#111", accent: "#075", border: "#ccc", surface1: "#fff", surface2: "#eee" } as Parameters<typeof SessionScopeChip>[0]["colors"];
beforeEach(() => vi.clearAllMocks());

describe("scope icon controls", () => {
  it.each(SESSION_SHORTCUTS)("keeps $label identifiable without hover and exposes full count and toggle state", (choice) => {
    const onPress = vi.fn(), onFocus = vi.fn(), onBlur = vi.fn();
    const tree = SessionScopeChip({ choice, count: 679, active: true, colors, onPress, onFocus, onBlur });
    const props = tree.props;
    expect(props.accessibilityRole).toBe("button");
    expect(props.accessibilityLabel).toBe(`${choice.label}, 679 sessions`);
    expect(props.accessibilityHint).toBe(`${choice.help} Search and other filters still apply.`);
    expect(props["aria-pressed"]).toBe(true);
    expect(walk(tree).some((node) => node.props.children === choice.caption)).toBe(true);
    expect(walk(tree).find((node) => node.type === "Icon")?.props.name).toBe(choice.icon);
    expect(help.text).toContain("Counts are before search and other filters.");
    props.onHoverIn!({} as never); expect(help.show).toHaveBeenCalledOnce();
    props.onFocus!({} as never); expect(help.focus).toHaveBeenCalledOnce(); expect(onFocus).toHaveBeenCalledOnce();
    props.onBlur!({} as never); expect(help.blur).toHaveBeenCalledOnce(); expect(onBlur).toHaveBeenCalledOnce();
    props.onPress!({} as never); expect(help.dismiss).toHaveBeenCalledOnce(); expect(onPress).toHaveBeenCalledOnce();
  });

  it("keeps 44pt targets, zero/large counts, pressed feedback, focus and disabled states", () => {
    for (const count of [0, 1, 10000]) {
      const tree = SessionScopeChip({ choice: SESSION_SHORTCUTS[0], count, active: false, disabled: true, colors, onPress() {} });
      const style = tree.props.style as (state: Record<string, boolean>) => Record<string, any>;
      expect(tree.props.disabled).toBe(true); expect(tree.props["aria-pressed"]).toBe(false);
      expect(tree.props.accessibilityLabel).toContain(String(count));
      if (count === 1) expect(tree.props.accessibilityLabel).toBe("All sessions, 1 session");
      expect(style({}).minHeight).toBeGreaterThanOrEqual(44); expect(style({}).minWidth).toBeGreaterThanOrEqual(44);
      expect(style({ focused: true }).borderColor).toBe(colors.foreground);
      expect(style({ pressed: true }).backgroundColor).toBe(colors.surface2);
      expect(style({}).opacity).toBe(0.5);
    }
  });
});

describe("desk glyphs preserve native meaning", () => {
  it("gives known desks distinct SDK glyphs and unknown desks a stable fallback", () => {
    expect(["pr-review", "Adam", "QA", "QSW"].map(deskIcon)).toEqual(["GitPullRequest", "Bot", "FlaskConical", "Zap"]);
    expect(deskIcon("future desk")).toBe("Armchair");
    expect(deskIcon("future desk")).toBe(deskIcon(""));
  });

  it.each(["hierarchy", "placement"] as const)("keeps empty QSW and unknown desks under native IDs in %s", (grouping) => {
    const projects = [{ id: "qsw", name: "DESK • QSW" }, { id: "future", name: "DESK • FUTURE" }];
    const tree = buildOrganizationTree({ entries: [], projects, grouping });
    expect(tree.map(organizationIcon)).toEqual(["Armchair", "Zap"]);
    expect(tree.find((group) => group.project?.id === "qsw")).toMatchObject({ detail: "0 workspaces", desk: true, totalCount: 0 });
    const filtered = filterOrganizationTree(tree, () => false, (group) => scopeKeepsGroup(group, "Desks"));
    expect(filtered).toHaveLength(2);
    expect(flattenOrganizationTree(filtered, {}, true).filter((row) => row.kind === "empty")).toHaveLength(2);
    expect(nativeDesk("QSW")).toBeNull(); // A code alone never declares a desk.
    expect(nativeDesk("DESK • QSW")).toBe("qsw");
  });

  it("changes neither desk membership nor the outside-desks scope for conflicting associations", () => {
    const entries = [sessionEntry("adam", { projectName: "DESK • ADAM", labels: { desk: "pr-review" } }),
      sessionEntry("review", { labels: { desk: "pr-review" } }), sessionEntry("qsw", { projectName: "DESK • QSW" }),
      sessionEntry("stream", { labels: { workstream: "alpha" } }), sessionEntry("unknown")];
    expect(SESSION_SHORTCUTS.map(({ id }) => entries.filter((entry) => matchesScope(entry, id, {})).map((entry) => entry.id)))
      .toEqual([["adam", "review", "qsw", "stream", "unknown"], ["review"], ["adam"], ["stream", "unknown"]]);
  });
});


describe("group primary state shares", () => {
  const entries = [sessionEntry("input", { status: "running", needsInput: true, requiresAttention: true }),
    sessionEntry("error", { status: "error", requiresAttention: true, attentionReason: "error" }),
    sessionEntry("running", { status: "running", requiresAttention: true }),
    sessionEntry("attention", { status: "idle", requiresAttention: true, attentionReason: "finished" }),
    sessionEntry("idle", { status: "idle" })];
  const aggregate = (values = entries) => buildOrganizationTree({ entries: values, grouping: "flat" })[0]!.aggregate;

  it("counts overlapping native running/input/attention once by the existing display priority", () => {
    const value = aggregate();
    expect(value).toMatchObject({ total: 5, needsInput: 1, failed: 1, running: 1, attention: 1, idle: 1, native: { running: 2, error: 1, idle: 2 } });
    expect(groupStateSummary(value)).toBe("Needs input: 1 of 5 sessions. Error: 1 of 5 sessions. Running: 1 of 5 sessions. Attention: 1 of 5 sessions. Idle: 1 of 5 sessions");
    expect(groupStateHelp(value)).toContain("Native lifecycle and attention signals can overlap");
    expect(groupStateHelp(value)).toContain("not counts of every attention signal");
    expect(groupStateHelp(value)).toContain("including collapsed branches");
  });

  it.each([true, false])("keeps a state word and ratio at every width, including 0/1/all, compact=%s", (compact) => {
    for (const values of [[], [entries[2]!], [entries[2]!, entries[2]!], entries]) {
      const value = aggregate(values);
      const tree = StateBadge({ state: aggregateState(value), aggregate: value, colors, compact });
      const text = walk(tree).flatMap((node) => typeof node.props.children === "string" ? [node.props.children] : []).join("");
      expect(text).toBe(values.length === 0 ? "No sessions" : values.length === 5 ? "Needs input 1/5" : `Running ${values.length}/${values.length}`);
      expect(text).not.toContain("0/0");
    }
    expect(groupStateSummary(aggregate([entries[2]!]))).toBe("Running: 1 of 1 session");
  });

  it.each([
    [2, 85, "running", "Running 2/85"], [50, 85, "attention", "Attention 50/85"], [85, 85, "running", "Running 85/85"],
  ] as const)("shows %s of %s sessions in %s without borrowing the group total", (count, total, state, expected) => {
    const values = Array.from({ length: total }, (_, index) => sessionEntry(String(index), { status: state === "running" && index < count ? "running" : "idle", requiresAttention: state === "attention" && index < count }));
    const value = aggregate(values);
    const tree = StateBadge({ state: aggregateState(value), aggregate: value, colors, compact: true });
    expect(walk(tree).some(node => node.props.children === expected)).toBe(true);
  });

  it.each([true, false])("puts full state help on the existing group focus target, compact=%s", (compact) => {
    const group = buildOrganizationTree({ entries, grouping: "flat" })[0]!;
    const onToggle = vi.fn();
    const tree = OrganizationRowView({ row: { kind: "group", group, depth: 0, expanded: false }, colors, compact,
      disabled: false, arranging: false, selected: false, favorite: false, onToggle, onFavorite() {}, onMove() {}, onSelect() {} });
    const button = walk(tree).find(node => node.type === "Pressable")!.props;
    expect(button.accessibilityLabel).toContain("Needs input: 1 of 5 sessions");
    expect(button.accessibilityHint).toContain("including collapsed branches");
    expect(button["aria-describedby"]).toBe("scope-help");
    expect(button.style({ focused: true }).minHeight).toBeGreaterThanOrEqual(44);
    expect(button.style({ focused: true }).borderColor).toBe(colors.foreground);
    button.onFocus(); expect(help.focus).toHaveBeenCalledOnce();
    button.onPress(); expect(help.dismiss).toHaveBeenCalledOnce(); expect(onToggle).toHaveBeenCalledOnce();
  });

  it.each(["hierarchy", "placement", "role"] as const)("uses current filtered descendants for every nested %s denominator", (grouping) => {
    const tree = buildOrganizationTree({ entries: entries.map((entry) => ({ ...entry, labels: { workstream: "Kontor", epic: "ASMA-99100", role: "QA" } })), grouping });
    const groups = (values: readonly (typeof tree)[number][]): typeof tree => values.flatMap((group) => [group, ...groups(group.groups)]);
    expect(groups(tree).length).toBeGreaterThan(grouping === "role" ? 0 : 1);
    expect(groups(tree).every((group) => groupStateSummary(group.aggregate).includes("1 of 5 sessions"))).toBe(true);
    const filtered = filterOrganizationTree(tree, (entry) => entry.status === "running");
    expect(groups(filtered).every((group) => group.aggregate.total === 2)).toBe(true);
    expect(groups(filtered).every((group) => groupStateSummary(group.aggregate) === "Needs input: 1 of 2 sessions. Running: 1 of 2 sessions")).toBe(true);
    // Original totals and saved group identity are not rewritten by narrowing.
    expect(groups(filtered).map((group) => group.key)).toEqual(groups(tree).map((group) => group.key));
    expect(groups(filtered).every((group) => group.totalCount === 5 && group.visibleCount === 2)).toBe(true);
  });
});
