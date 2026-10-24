import { fileURLToPath, URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { sourceCatalog } from "../server/read-sources";
import { createTeamsSourceAdapter } from "../server/teams-source-adapter";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import type { TeamsFleetData, TeamsSourceSnapshot, TeamsTemplatesData } from "../shared/teams-source-snapshot";
import { sessionEntry } from "./session-fixtures";
const hooks = vi.hoisted(() => ({ values: [] as unknown[], cursor: 0 }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useContext: () => null, useRef: (current: unknown) => ({ current }), useState: <T,>(initial: T) => {
  const index = hooks.cursor++; if (!(index in hooks.values)) hooks.values[index] = initial;
  return [hooks.values[index] as T, (value: T) => { hooks.values[index] = value; }];
} }));
vi.mock("react-native", () => ({ Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", TextInput: "TextInput" }));
import { DEFAULT_TEAMS_SOURCE_VIEW, DEFAULT_TEAMS_VIEW, TeamsSourcePage, type TeamsView } from "../client/teams-source-page";
type Element = ReactElement<Record<string, unknown>>;
type Props = Parameters<typeof TeamsSourcePage>[0];
const catalog = sourceCatalog(JSON.stringify([
  ["authoring", "authoring-fleet.yml"], ["operator", "operator-fleet.yml"], ["roster", "core-team.yml"], ["templates", "mvp-team-pack.json"],
].map(([id, filename]) => ({ id, title: id, kind: "teams", path: fileURLToPath(new URL(`./fixtures/teams-source/${filename}`, import.meta.url)) }))));
const CONTEXT = "Sessions in this context: native-exact";
const FILTER = "Filter configured teams, roles and templates";
const colors = {} as Props["colors"];
let snapshots: Record<string, TeamsSourceSnapshot>; let fleet: TeamsFleetData; let templates: TeamsTemplatesData;
beforeAll(async () => {
  const adapter = createTeamsSourceAdapter(catalog, teamsSourceBindings(catalog.sources).formats);
  snapshots = Object.fromEntries(await Promise.all(catalog.sources.map(async (source) => [source.id, await adapter.read({ sourceId: source.id })])));
  if (snapshots.authoring!.data?.kind !== "fleet" || snapshots.templates!.data?.kind !== "templates") throw new Error("Expected actual hashed fixtures");
  fleet = snapshots.authoring!.data; templates = snapshots.templates!.data;
});
beforeEach(() => { hooks.values = []; hooks.cursor = 0; });
function expand(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(expand);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return expand((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return [node, ...expand(node.props.children as ReactNode)];
}
function visible(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(visible).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return visible((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return visible(node.props.children as ReactNode);
}
/** Controlled by default, as the route store will be; `controlled: false` exercises the page's own state. */
function mount(overrides: Partial<Props> = {}, initial: TeamsView = DEFAULT_TEAMS_VIEW, controlled = true) {
  let saved = initial; const changes: TeamsView[] = [];
  let tree: ReactNode = null;
  const render = () => {
    hooks.cursor = 0;
    tree = TeamsSourcePage({ sources: catalog.sources, snapshots, available: true, reads: {}, entries: [sessionEntry("native-exact")], observedAt: "2026-10-18T10:00:00Z", colors, children: CONTEXT,
      ...(controlled ? { view: saved, onView: (next: TeamsView) => { saved = next; changes.push(next); } } : {}), ...overrides });
  };
  render();
  const find = (label: string) => { const node = expand(tree).find((element) => element.props.accessibilityLabel === label); if (!node) throw new Error(`Missing ${label}`); return node; };
  return { changes, view: () => saved, text: () => visible(tree), nodes: () => expand(tree),
    labels: () => expand(tree).map((element) => element.props.accessibilityLabel).filter((label): label is string => typeof label === "string"),
    expanded: (label: string) => (find(label).props.accessibilityState as { expanded?: boolean }).expanded,
    press(label: string) { const node = find(label); expect(node.props.disabled).not.toBe(true); (node.props.onPress as () => void)(); render(); },
    type(label: string, value: string) { (find(label).props.onChangeText as (value: string) => void)(value); render(); } };
}
const groupKey = (binding: TeamsFleetData["bindings"][number]) => `${binding.groupKind}/${binding.groupId}`;
const configured = (sources = 4) => `Configured teams & templates (${sources} sources)`;
describe("AI teams & sessions leads with the project context (TEAMS-01)", () => {
  it("renders the context sessions first and keeps configuration collapsed with expanded semantics", () => {
    const screen = mount();
    const text = screen.text();
    expect(text.indexOf(CONTEXT)).toBe(0);
    // The observation statements open with the configuration, before its snapshots; they are not on the collapsed page.
    expect(text).not.toContain("Native observations"); expect(text).not.toContain("Planned or governed assignment");
    expect(text).not.toContain("Source state:"); expect(text).not.toContain("Fleet schema");
    expect(screen.labels().some((label) => label.startsWith("Roles: ") || label.startsWith("Template: "))).toBe(false);
    expect(screen.expanded(configured())).toBe(false);
    screen.press(configured());
    expect(screen.changes).toHaveLength(1); expect(screen.changes[0]!.configured).toBe(true);
    expect(screen.expanded(configured())).toBe(true);
    expect(screen.text()).toContain("Source state: ready"); expect(screen.labels()).toContain(FILTER);
    // Group, role and template cards are one step in, not behind further disclosures.
    expect(screen.labels()).toContain(`Roles: ${groupKey(fleet.bindings[0]!)}`);
    expect(screen.labels()).toContain(`Template: ${templates.templates[0]!.id} v${templates.templates[0]!.version}`);
    expect(screen.text()).toContain("No configuration-to-session assignment is inferred");
    const opened = screen.text();
    expect(opened.indexOf(configured())).toBeLessThan(opened.indexOf("Native observations: 1 sessions · observed 2026-10-18T10:00:00Z"));
    expect(opened.indexOf("Native sessions and labels are observations, not activated configured teams. Planned or governed assignment: Unknown.")).toBeLessThan(opened.indexOf("Host configuration snapshots"));
    expect(opened).toContain("Governed fleet: not shown here. kontord on this host: unknown.");
  });
  it("keeps the observation statements visible when no configured source gives them a disclosure", () => {
    const text = mount({ sources: [] }).text();
    expect(text).toContain("Native observations: 1 sessions"); expect(text).toContain("Planned or governed assignment: Unknown.");
  });
  it("shows a failed source as a visible one-line error while configuration stays collapsed", () => {
    const { operator: _failed, ...rest } = snapshots;
    const screen = mount({ snapshots: rest, errors: { operator: "operator unavailable: rejected request" } });
    expect(screen.text()).toContain("operator · fleet-operator: adapter request failed: operator unavailable: rejected request");
    for (const healthy of ["authoring · fleet-authoring:", "roster · core-roster:", "templates · team-templates:"]) expect(screen.text()).not.toContain(healthy);
    expect(screen.text()).not.toContain(catalog.sources[1]!.path);
    screen.press(configured());
    // Expanded, the failure is on the source card itself, not inside its closed Source details.
    expect(screen.text()).toContain("Adapter request failed: operator unavailable: rejected request");
    expect(screen.text()).not.toContain(catalog.sources[1]!.path);
  });
  it("narrows group, role and template cards with one filter and restarts paging", () => {
    const sources = [catalog.sources[0]!, catalog.sources[3]!];
    const screen = mount({ sources }, { ...DEFAULT_TEAMS_VIEW, configured: true, sources: { authoring: { ...DEFAULT_TEAMS_SOURCE_VIEW, fleet: { group: null, reference: null, groupPage: 1, rolePage: 0 } } } });
    const groups = () => screen.labels().filter((label) => label.startsWith("Roles: "));
    const cards = () => screen.labels().filter((label) => label.startsWith("Template: "));
    expect(screen.labels()).toContain("Previous groups"); expect(cards()).toHaveLength(templates.templates.length);
    screen.type(FILTER, "judge");
    expect(screen.changes.at(-1)!.filter).toBe("judge");
    expect(screen.changes.at(-1)!.sources.authoring!.fleet.groupPage).toBe(0);
    const matching = [...new Set(fleet.bindings.filter((binding) => [groupKey(binding), binding.reference, binding.slotId, binding.chainName].some((value) => value?.includes("judge"))).map(groupKey))];
    expect(matching.length).toBeGreaterThan(1); expect(matching.length).toBeLessThan(new Set(fleet.bindings.map(groupKey)).size);
    expect(groups()).toEqual(matching.map((group) => `Roles: ${group}`));
    const judged = templates.templates.filter((template) => template.slots.some((slot) => slot.id.includes("judge") || slot.role.role.includes("judge")));
    expect(judged.length).toBeLessThan(templates.templates.length); expect(cards()).toEqual(judged.map((template) => `Template: ${template.id} v${template.version}`));
    const bindings = fleet.bindings.filter((binding) => [binding.reference, binding.slotId, binding.chainName].some((value) => value?.includes("judge")));
    expect(screen.labels()).toContain(`Configured bindings (${bindings.length} of ${fleet.bindings.length})`);
    screen.press("Roles: team/pr-review-desk");
    expect(screen.text()).toContain("team/pr-review-desk · 1 of 7 declared roles");
    expect(screen.labels().filter((label) => label.startsWith("Role: "))).toEqual(["Role: team/pr-review-desk/judge → desk-judge"]);
    const spike = judged.find((template) => template.version === 1)!;
    screen.press(`Template: ${spike.id} v1`);
    expect(screen.labels().filter((label) => label.startsWith("Template role: "))).toEqual(["Template role: judge · judge v1"]);
    // A name match shows every role of that template.
    screen.type(FILTER, "research spike");
    expect(screen.labels().filter((label) => label.startsWith("Template role: "))).toHaveLength(spike.slots.length);
    screen.press("Clear filter");
    expect(screen.view().filter).toBe(""); expect(screen.labels()).toContain(`Configured bindings (${fleet.bindings.length})`);
  });
});
describe("Teams drill-down survives leaving and Back (STATE-01)", () => {
  it("reports every change as a serializable view and restores the same drill-down from it", () => {
    const sources = [catalog.sources[0]!, catalog.sources[3]!];
    const screen = mount({ sources });
    const template = templates.templates.find((item) => item.version === 2)!;
    const slot = template.slots.find((item) => item.id === "builder")!;
    const steps: Array<[string, string?]> = [[configured(2)], ["Next groups"], ["Roles: team/pr-review-desk"], ["Role: team/pr-review-desk/rev-a → desk-review-a"],
      [`Template: ${template.id} v2`], [`Template role: ${slot.id} · ${slot.role.role} v${slot.role.version}`],
      [`Configured bindings (${fleet.bindings.length})`], ["Next bindings"], [`${fleet.bindings[8]!.reference} → ${fleet.bindings[8]!.chainName}`],
      [`Configured chains (${fleet.chains.length})`], ["Source details"], [`Unresolved template/leadership details (${fleet.bindings.length})`], [FILTER, "pr-review"]];
    for (const [label, typed] of steps) {
      if (typed === undefined) screen.press(label); else screen.type(label, typed);
    }
    expect(screen.changes).toHaveLength(steps.length);
    for (const change of screen.changes) expect(JSON.parse(JSON.stringify(change))).toEqual(change);
    const left = screen.text();
    expect(left).toContain("groupId: pr-review-desk · slotId: rev-a · declared chainName: desk-review-a");
    expect(left).toContain("Default rung 1:"); expect(left).toContain(snapshots.authoring!.source.path);
    expect(left).toContain(`Configured bindings (7 of ${fleet.bindings.length})`);
    const saved = JSON.parse(JSON.stringify(screen.view())) as TeamsView;
    expect(saved.sources.authoring!.openBindings).toEqual([fleet.bindings[8]!.reference]);
    // Leaving remounts the page with fresh hook state; Back supplies the stored view.
    hooks.values = [];
    const returned = mount({ sources }, saved);
    expect(returned.text()).toBe(left);
    expect(returned.expanded(configured(2))).toBe(true); expect(returned.changes).toHaveLength(0);
  });
  it("keeps the page's own state when no view is supplied, and still reports changes to onView alone", () => {
    const local = mount({}, DEFAULT_TEAMS_VIEW, false);
    local.press(configured()); expect(local.expanded(configured())).toBe(true);
    local.press("Next groups"); local.press("Roles: team/pr-review-desk"); expect(local.text()).toContain("team/pr-review-desk · 7 declared roles");
    hooks.values = [];
    const onView = vi.fn();
    const observed = mount({ onView }, DEFAULT_TEAMS_VIEW, false);
    observed.press(configured()); expect(observed.expanded(configured())).toBe(true);
    expect(onView).toHaveBeenCalledExactlyOnceWith(expect.objectContaining({ configured: true }));
  });
  it("drops selections whose IDs the current snapshot no longer declares", () => {
    const kept = fleet.bindings[0]!.reference;
    const saved: TeamsView = { ...DEFAULT_TEAMS_VIEW, configured: true, sources: {
      authoring: { ...DEFAULT_TEAMS_SOURCE_VIEW, bindings: true, openBindings: ["team/removed/slot", kept], fleet: { group: "team/removed", reference: "team/removed/slot", groupPage: 0, rolePage: 3 } },
      templates: { ...DEFAULT_TEAMS_SOURCE_VIEW, templates: { template: "removed@9", slot: "ghost", templatePage: 0, slotPage: 2 } },
      operator: { ...DEFAULT_TEAMS_SOURCE_VIEW, fleet: { group: "team/unproven", reference: null, groupPage: 0, rolePage: 0 } },
      "renamed-source": { ...DEFAULT_TEAMS_SOURCE_VIEW, details: true },
    } };
    const { operator: _unread, ...rest } = snapshots;
    const screen = mount({ snapshots: rest, errors: { operator: "operator unavailable" } }, saved);
    expect(screen.labels()).not.toContain("All configured groups"); expect(screen.labels()).not.toContain("All template versions");
    expect(screen.labels()).toContain(`Roles: ${groupKey(fleet.bindings[0]!)}`);
    expect(screen.labels()).toContain(`Template: ${templates.templates[0]!.id} v${templates.templates[0]!.version}`);
    screen.press(`Configured chains (${fleet.chains.length})`);
    const next = screen.changes.at(-1)!;
    expect(Object.keys(next.sources).sort()).toEqual(["authoring", "operator", "templates"]);
    expect(next.sources.authoring).toMatchObject({ chains: true, openBindings: [kept], fleet: { group: null, reference: null, rolePage: 0 } });
    expect(next.sources.templates!.templates).toEqual({ template: null, slot: null, templatePage: 0, slotPage: 0 });
    // An unread source cannot prove its selection is gone, so it is retained.
    expect(next.sources.operator!.fleet.group).toBe("team/unproven");
  });
});
describe("Teams disclosures and targets (A11Y-01)", () => {
  it("exposes expanded state on every disclosure and 44px card and filter targets", () => {
    const all: TeamsView = { ...DEFAULT_TEAMS_VIEW, configured: true, unresolved: true, sources: Object.fromEntries(Object.keys(snapshots).map((id) => [id, { ...DEFAULT_TEAMS_SOURCE_VIEW, details: true, bindings: true, chains: true, openBindings: [fleet.bindings[0]!.reference] }])) };
    const screen = mount({}, all);
    const disclosures = screen.nodes().filter((node) => typeof (node.props.accessibilityState as { expanded?: unknown } | undefined)?.expanded === "boolean");
    const open = (label: string) => disclosures.filter((node) => node.props.accessibilityLabel === label).map((node) => (node.props.accessibilityState as { expanded: boolean }).expanded);
    for (const label of [configured(), `Configured bindings (${fleet.bindings.length})`, `Configured chains (${fleet.chains.length})`, "Unresolved template/leadership details (76)", `${fleet.bindings[0]!.reference} → ${fleet.bindings[0]!.chainName}`]) expect(new Set(open(label))).toEqual(new Set([true]));
    expect(open("Source details")).toEqual([true, true, true, true]);
    // Rows outside the saved openBindings report collapsed, not a missing state.
    const closed = disclosures.filter((node) => !(node.props.accessibilityState as { expanded: boolean }).expanded).map((node) => String(node.props.accessibilityLabel));
    expect(closed).toContain(`${fleet.bindings[1]!.reference} → ${fleet.bindings[1]!.chainName}`); expect(closed.every((label) => label.includes(" → "))).toBe(true);
    const targets = screen.nodes().filter((node) => /^(Roles|Template): /.test(String(node.props.accessibilityLabel)) || node.props.accessibilityLabel === FILTER);
    expect(targets.length).toBeGreaterThan(4);
    for (const target of targets) expect((target.props.style as { minHeight: number }).minHeight).toBeGreaterThanOrEqual(44);
  });
});
