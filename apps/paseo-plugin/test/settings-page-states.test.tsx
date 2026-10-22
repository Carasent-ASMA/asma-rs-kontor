import { readFileSync } from "node:fs";
import { fileURLToPath, URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { sourceCatalog } from "../server/read-sources";
import { createTeamsSourceAdapter } from "../server/teams-source-adapter";
import { teamsSourceBindings } from "../shared/teams-source-bindings";
import { jiraHistorySchema, type ReadSourceSummary, type SourceRead } from "../shared/read-sources";
import type { TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
import { sessionEntry } from "./session-fixtures";
const hooks = vi.hoisted(() => ({ values: [] as unknown[], cursor: 0 }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useContext: () => null, useRef: (current: unknown) => ({ current }), useState: <T,>(initial: T) => {
  const index = hooks.cursor++; if (!(index in hooks.values)) hooks.values[index] = initial;
  return [hooks.values[index] as T, (value: T) => { hooks.values[index] = value; }];
} }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", View: "View", Text: "Text", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { OrganizationSettingsPage } from "../client/organization-settings-page";
import { Card, Copy } from "../client/page-controls";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
type Element = ReactElement<Record<string, unknown>>;
type Props = Parameters<typeof OrganizationSettingsPage>[0];
const catalog = sourceCatalog(JSON.stringify([
  ["authoring", "authoring-fleet.yml"], ["operator", "operator-fleet.yml"], ["roster", "core-team.yml"], ["templates", "mvp-team-pack.json"],
].map(([id, filename]) => ({ id, title: id, kind: "teams", path: fileURLToPath(new URL(`./fixtures/teams-source/${filename}`, import.meta.url)) }))));
const summary = (id: string, kind: ReadSourceSummary["kind"]): ReadSourceSummary => ({ id, title: `${id} title`, kind, path: `/configured/${id}.md`, workstream: null, epic: null, projectId: null, revisionKind: "approved revision" });
const plan = summary("roadmap", "plan");
const decisions = summary("adr-log", "decisions");
const planRead: SourceRead = { source: plan, status: "ready", revision: "a".repeat(64), readAt: "2026-10-19T08:00:00Z", modifiedAt: null, content: "# Roadmap", graph: null, records: null, diagnostics: ["plan diagnostic hidden until details"] };
const OFFLINE = "Paseo host offline. Retained reads are stale.";
let snapshots: Record<string, TeamsSourceSnapshot>;
beforeAll(async () => {
  const adapter = createTeamsSourceAdapter(catalog, teamsSourceBindings(catalog.sources).formats);
  snapshots = Object.fromEntries(await Promise.all(catalog.sources.map(async (source) => [source.id, await adapter.read({ sourceId: source.id })])));
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
function state(status: "loading" | "error" | "invalid" | "ready", extra: Partial<Props["state"]> = {}): Props["state"] {
  const base = { saving: false, saveError: null, save: vi.fn(async () => true), reset: vi.fn(async () => true), reload: vi.fn(async () => {}) };
  const values = { ...DEFAULT_ORGANIZATION_SETTINGS, planAssociations: { "native-a": ["roadmap"], "native-b": [] }, displayMappings: { "session:native-a": { workstream: "Adam" }, "workspace:w1": { epic: "ASMA-1" } } };
  const shape = status === "loading" ? { status } : status === "error" ? { status, error: "settings RPC failed" } : status === "invalid" ? { status, error: "grouping: invalid enum", revision: "rev-bad" } : { status, values, revision: "rev-a" };
  return { ...base, ...shape, ...extra } as Props["state"];
}
function mount(overrides: Partial<Props> = {}) {
  const props: Props = { state: state("ready"), colors: {} as Props["colors"], entries: [sessionEntry("native-a")], workspaces: [], sources: [], reads: {}, sourceError: null, sourcesAvailable: true, refresh: vi.fn(), ...overrides };
  // Every traversal re-renders from the first hook, so nested disclosure state keeps its slot.
  hooks.values = [];
  const render = () => { hooks.cursor = 0; return OrganizationSettingsPage(props); };
  const nodes = () => expand(render());
  const find = (label: string) => { const node = nodes().find((element) => element.props.accessibilityLabel === label); if (!node) throw new Error(`Missing ${label}`); return node; };
  return { props, text: () => visible(render()), nodes, labels: () => nodes().map((node) => node.props.accessibilityLabel).filter(Boolean) as string[],
    find, press(label: string) { (find(label).props.onPress as () => void)(); } };
}
describe("Settings source catalog states", () => {
  it("separates reading, empty and offline instead of claiming an empty host", () => {
    const reading = mount({ sourcesAvailable: false }).text();
    expect(reading).toContain("Reading configured sources…");
    expect(reading).not.toContain("No sources are configured on this host."); expect(reading).not.toContain("Read-only source catalog available.");
    const empty = mount().text();
    expect(empty).not.toContain("Read-only source catalog available."); expect(empty).toContain("No sources are configured on this host.");
    const offline = mount({ sourcesAvailable: false, sourceError: OFFLINE }).text();
    expect(offline).toContain(OFFLINE);
    expect(offline).not.toContain("No sources are configured on this host."); expect(offline).not.toContain("Reading configured sources…");
    // The catalog statement and the kontord line are one short disclosure, visible once opened.
    for (const overrides of [{ sourcesAvailable: false }, {}, { sourcesAvailable: false, sourceError: OFFLINE }, { kontord: "kontord on this host: not answering." }] as Partial<Props>[]) {
      const screen = mount(overrides);
      const note = screen.nodes().find((node) => node.props.testID === "settings-catalog-note")!;
      expect(visible(note)).toBe("▸ Read-only · configured on the server");
      expect(screen.text()).not.toContain("kontord");
      screen.press("Read-only · configured on the server");
      expect(screen.text()).toContain("no path, title or renamed ID is inferred.");
      expect(screen.text()).toContain(`Governed scheduler, gates and team activation: not shown here. ${overrides.kontord ?? "kontord on this host: unknown."} Console owns policy and administration.`);
    }
  });
  it("groups sources by kind in compact rows and keeps provenance and diagnostics behind details", () => {
    const operator = { ...snapshots.operator!, status: "partial" as const, diagnostics: [{ code: "read-probe", pointer: "/chains/probe", message: "Definition unavailable in this snapshot" }] };
    const screen = mount({ sources: [...catalog.sources, decisions, plan], reads: { roadmap: planRead }, teamsSnapshots: { ...snapshots, operator } });
    const text = screen.text();
    const headings = ["Plans (1)", "Decision records (1)", "Teams configuration (4)"];
    expect(headings.map((heading) => text.indexOf(heading))).toEqual(headings.map((heading) => text.indexOf(heading)).toSorted((a, b) => a - b));
    expect(headings.every((heading) => text.includes(heading))).toBe(true);
    expect(text).not.toContain("Jira history ("); expect(text).not.toContain("Delivery records (");
    expect(text).toContain("roadmap title"); expect(text).not.toContain("roadmap title · ready"); expect(text).not.toContain(planRead.readAt);
    expect(screen.labels()).toContain(`roadmap title · ready · approved revision. Read ${planRead.readAt}`);
    expect(text).toContain("adr-log title · Not read · observed time Unknown");
    expect(text).toContain("operator · partial"); expect(text).not.toContain("authoring · ready");
    for (const hidden of [plan.path, "plan diagnostic hidden until details", snapshots.operator!.source.path, "Definition unavailable in this snapshot"]) expect(text).not.toContain(hidden);
    expect(screen.find("Source details: roadmap title").props.accessibilityState).toMatchObject({ expanded: false });
    screen.press("Source details: roadmap title");
    expect(screen.text()).toContain(plan.path); expect(screen.text()).toContain(`Read ${planRead.readAt}`); expect(screen.find("Source details: roadmap title").props.accessibilityState).toMatchObject({ expanded: true });
    const teamDetails = () => screen.nodes().filter((node) => node.props.accessibilityLabel === "Source details");
    expect(teamDetails().map((node) => (node.props.accessibilityState as { expanded: boolean }).expanded)).toEqual([false, false, false, false]);
    (teamDetails()[1]!.props.onPress as () => void)();
    const expanded = screen.text();
    expect(expanded).toContain("read-probe · /chains/probe: Definition unavailable in this snapshot"); expect(expanded).toContain(snapshots.operator!.source.path);
    expect(expanded).not.toContain(snapshots.authoring!.source.path);
  });
  it("shows each source title exactly and adds the Jira capture day only when the title does not already name it", () => {
    const history = jiraHistorySchema.parse(JSON.parse(readFileSync(new URL("./fixtures/jira-history.json", import.meta.url), "utf8")));
    const titled = (id: string, kind: ReadSourceSummary["kind"], title: string) => ({ ...summary(id, kind), title });
    const sources = [titled("stamped", "decisions", "ASMA-8539 decisions — captured 2026-10-19T10:59:22.131040+00:00"),
      titled("jira-short", "jira", "Historical Jira · 18 Oct 2026"), titled("jira-iso", "jira", "Jira export 2026-10-18"), titled("jira-plain", "jira", "Historical Jira")];
    const reads = Object.fromEntries(sources.map((source) => [source.id, { ...planRead, source, content: null, diagnostics: [], ...(source.kind === "jira" ? { jiraHistory: history } : {}) }]));
    const screen = mount({ sources, reads });
    const line = (source: ReadSourceSummary) => visible(screen.find(source.kind === "jira"
      ? `${source.title} · ready · Historical Jira snapshot. Observed ${history.generated} · not live` : `${source.title} · ready · approved revision. Read ${planRead.readAt}`));
    expect(sources.map(line)).toEqual([
      "ASMA-8539 decisions — captured 2026-10-19T10:59:22.131040+00:00", "Historical Jira · 18 Oct 2026", "Jira export 2026-10-18", "Historical Jira · captured 18 Oct"]);
  });
  it("offers the Teams browser only when the host can navigate there", () => {
    expect(mount().labels()).not.toContain("Browse configured teams & templates");
    const onOpenTeams = vi.fn();
    const screen = mount({ sources: catalog.sources, teamsSnapshots: snapshots, onOpenTeams });
    screen.press("Browse configured teams & templates");
    expect(onOpenTeams).toHaveBeenCalledOnce(); expect(screen.props.refresh).not.toHaveBeenCalled();
  });
});
describe("Settings presentation states", () => {
  it("places the Keyboard card, kept in its own settings document, between the preferences and the sources", () => {
    const keyboard = <Card title="Keyboard" colors={{} as Props["colors"]}><Copy colors={{} as Props["colors"]}>Keyboard slot</Copy></Card>;
    const headers = (screen: ReturnType<typeof mount>) => screen.nodes().filter((node) => node.props.accessibilityRole === "header").map((node) => visible(node));
    const plain = headers(mount());
    expect(plain).not.toContain("Keyboard");
    const titles = headers(mount({ keyboard }));
    expect(titles.indexOf("Keyboard")).toBe(titles.indexOf("Presentation and associations") + 1);
    expect(titles[titles.indexOf("Keyboard") + 1]).toBe("Configured sources");
    expect(titles.filter((title) => title !== "Keyboard")).toEqual(plain);
  });
  it("shows loading, unavailable and invalid preferences with only their honest actions", () => {
    const loading = mount({ state: state("loading") });
    expect(loading.text()).toContain("Reading saved preferences…");
    expect(loading.labels()).toContain("Reload saved values"); expect(loading.labels()).not.toContain("Edit presentation");
    const failed = mount({ state: state("error") });
    expect(failed.text()).toContain("Preferences unavailable: settings RPC failed"); expect(failed.labels()).not.toContain("Edit presentation");
    const invalid = mount({ state: state("invalid", { saving: true }) });
    expect(invalid.text()).toContain("Saved preferences are invalid and not applied: grouping: invalid enum · Saving…");
    expect(invalid.find("Reset invalid preferences to defaults").props.disabled).toBe(true); expect(invalid.labels()).not.toContain("Edit presentation");
    for (const screen of [loading, failed, invalid]) expect(screen.text()).not.toContain("Grouping hierarchy");
  });
  it("summarizes saved values in compact rows until a draft is opened, without saving anything", () => {
    const ready = state("ready");
    const screen = mount({ state: ready });
    // Not editing: the revision is in the summary's label, not on the page.
    expect(screen.text()).not.toContain("Saved revision");
    expect(screen.labels()).toContain("Saved revision rev-a. Layout grouped · grouping hierarchy · appearance system · 1 conversation with explicit plan links · 2 display mappings");
    const hashed = mount({ state: { ...ready, revision: "b079931e8f788330f264a50cfa7948a9fd1e87aebd41172a73ff9fdf6ed547db" } as typeof ready });
    expect(hashed.labels().some((label) => label.startsWith("Saved revision b079931e8f78…. Layout"))).toBe(true); expect(hashed.labels().join(" ")).not.toContain("2a73ff9f");
    expect(hashed.text()).not.toContain("b079931e8f78");
    expect(mount({ state: state("ready", { values: DEFAULT_ORGANIZATION_SETTINGS } as Partial<Props["state"]>) }).text()).not.toMatch(/0 (conversations|display mappings)/);
    expect(screen.text()).toContain("Layout grouped · grouping hierarchy · appearance system");
    expect(screen.text()).toContain("1 conversation with explicit plan links · 2 display mappings");
    expect(screen.text()).not.toContain("Changes apply only after Save.");
    screen.press("Edit presentation");
    expect(screen.text()).toContain("Editing revision rev-a"); expect(screen.text()).not.toContain("Layout grouped · grouping hierarchy · appearance system");
    expect(screen.text()).toContain("Saved revision rev-a"); expect(screen.text()).toContain("Shared by every client of this host. Changes apply only after Save.");
    expect(ready.save).not.toHaveBeenCalled(); expect(ready.reset).not.toHaveBeenCalled();
  });
  it("names empty choices in the editor instead of rendering blank lists", () => {
    const screen = mount({ entries: [] });
    screen.press("Edit presentation");
    expect(screen.text()).toContain("No conversations in the current native inventory.");
    expect(screen.text()).toContain("No workspaces or conversations to map.");
    const linked = mount();
    linked.press("Edit presentation"); linked.press(`${sessionEntry("native-a").title} · native-a`);
    expect(linked.text()).toContain("No configured plan sources on this host.");
  });
});
