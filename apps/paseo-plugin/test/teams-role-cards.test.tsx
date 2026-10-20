import { fileURLToPath, URL } from "node:url";
import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { sourceCatalog } from "../server/read-sources";
import { createTeamsSourceAdapter } from "../server/teams-source-adapter";
import { composeTeamsSourceView, type TeamsFleetData, type TeamsTemplatesData, type TeamsSourceSnapshot } from "../shared/teams-source-snapshot";
const hooks = vi.hoisted(() => ({ values: [] as unknown[], cursor: 0 }));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: <T,>(initial: T) => {
  const index = hooks.cursor++; if (!(index in hooks.values)) hooks.values[index] = initial;
  return [hooks.values[index] as T, (value: T) => { hooks.values[index] = value; }];
} }));
vi.mock("react-native", () => ({ Text: "Text", View: "View", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon" }));
import { FleetRoleCards, TemplateRoleCards } from "../client/teams-role-cards";
type Element = ReactElement<Record<string, unknown>>;
function elements(node: ReactNode): Element[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  if (typeof node.type === "function") return elements((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return [node, ...elements(node.props.children as ReactNode)];
}
function text(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(text).join("");
  if (!isValidElement<Record<string, unknown>>(node)) return "";
  if (typeof node.type === "function") return text((node.type as (props: Record<string, unknown>) => ReactNode)(node.props));
  return text(node.props.children as ReactNode);
}
let snapshots: TeamsSourceSnapshot[]; let fleet: TeamsFleetData; let templates: TeamsTemplatesData;
const colors = {} as Parameters<typeof FleetRoleCards>[0]["colors"];
beforeAll(async () => {
  const catalog = sourceCatalog(JSON.stringify([
    { id: "authoring", title: "Authoring", kind: "teams", path: fileURLToPath(new URL("./fixtures/teams-source/authoring-fleet.yml", import.meta.url)), expectedSha256: "5e49c621a5ed28a181f7e5f7c5c7c67f7bdc09c8d1258e94d48c60988fa5cff4" },
    { id: "templates", title: "Templates", kind: "teams", path: fileURLToPath(new URL("./fixtures/teams-source/mvp-team-pack.json", import.meta.url)), expectedSha256: "2a5a0aed9c23450f984fd3d1f92c5824f4640a71b2bc9697349b0632fed8b3cc" },
  ]));
  const adapter = createTeamsSourceAdapter(catalog, { authoring: "fleet-authoring", templates: "team-templates" });
  snapshots = await Promise.all(catalog.sources.map((source) => adapter.read({ sourceId: source.id })));
  const first = snapshots[0]!.data; const second = snapshots[1]!.data;
  if (first?.kind !== "fleet" || second?.kind !== "templates") throw new Error("Expected actual hashed fixtures");
  fleet = first; templates = second;
});
beforeEach(() => { hooks.values = []; hooks.cursor = 0; });
function harness(component: () => ReactNode) {
  let tree: ReactNode;
  const render = () => { hooks.cursor = 0; tree = component(); }; render();
  return { text: () => text(tree), labels: () => elements(tree).map((node) => String(node.props.accessibilityLabel)), click(label: string) {
    const action = elements(tree).find((node) => node.props.accessibilityLabel === label);
    if (!action) throw new Error(`Missing ${label}`);
    expect(action.props.disabled).not.toBe(true); (action.props.onPress as () => void)(); render();
  } };
}
describe("typed read-only Teams → role hierarchy", () => {
  it("opens the explicit PR desk group then its exact role/chain with retained ordered rungs and provenance", () => {
    const metadata = composeTeamsSourceView(snapshots, []).metadata;
    const screen = harness(() => FleetRoleCards({ data: fleet, metadata, colors }));
    expect(screen.labels().filter((label) => label.startsWith("Roles: ")).length).toBeLessThanOrEqual(8);
    while (!screen.labels().includes("Roles: team/pr-review-desk")) screen.click("Next groups");
    screen.click("Roles: team/pr-review-desk"); expect(screen.text()).toContain("7 declared roles");
    for (const mapping of ["adam → coordinator", "rev-a → desk-review-a", "swe → desk-fixer"]) expect(screen.labels()).toContain(`Role: team/pr-review-desk/${mapping}`);
    screen.click("Role: team/pr-review-desk/rev-a → desk-review-a");
    expect(screen.text()).toContain("groupId: pr-review-desk · slotId: rev-a · declared chainName: desk-review-a");
    expect(screen.text()).toContain("Rung 1"); expect(screen.text()).toContain("Alternative 1:");
    expect(screen.text()).toContain("account aliases"); expect(screen.text()).toContain("/bindings/team~1pr-review-desk~1rev-a");
    expect(screen.text()).toContain("unresolved"); expect(screen.text()).toContain("No authorized definition identifies");
    screen.click("All group roles"); expect(screen.labels()).toContain("Role: team/pr-review-desk/swe → desk-fixer");
  });
  it("shows template candidates across versions without choosing a latest or assigning sessions", () => {
    const metadata = composeTeamsSourceView(snapshots, []).metadata;
    const candidate = metadata.find((item) => item.templateCandidates.length > 1)!;
    const binding = fleet.bindings.find((item) => item.reference === candidate.bindingReference)!;
    const screen = harness(() => FleetRoleCards({ data: fleet, metadata, colors }));
    const group = `Roles: ${binding.groupKind}/${binding.groupId}`;
    while (!screen.labels().includes(group)) screen.click("Next groups");
    screen.click(group); screen.click(`Role: ${binding.reference} → ${binding.chainName}`);
    expect(screen.text()).toContain("Candidate only:"); expect(screen.text()).toContain(" v1"); expect(screen.text()).toContain(" v2");
    expect(screen.text()).toContain("does not pin a template version");
    expect(screen.text()).not.toContain("latest"); expect(screen.text()).toContain("no native assignment inferred");
  });
  it("opens an explicitly selected bundled template version and its typed role definitions", () => {
    const template = templates.templates.find((item) => item.version === 2)!;
    const slot = template.slots.find((item) => item.id === "builder") ?? template.slots[0]!;
    const screen = harness(() => TemplateRoleCards({ data: templates, colors }));
    expect(screen.labels()).toContain(`Template: ${template.id} v1`); expect(screen.labels()).toContain(`Template: ${template.id} v2`);
    screen.click(`Template: ${template.id} v2`); screen.click(`Template role: ${slot.id} · ${slot.role.role} v${slot.role.version}`);
    expect(screen.text()).toContain(`${template.id} v2`); expect(screen.text()).toContain("Default rung 1:");
    expect(screen.text()).toContain("Skills:"); expect(screen.text()).toContain("Context:"); expect(screen.text()).toContain(slot.provenance.pointer);
    expect(screen.text()).toContain("not execution authority"); expect(screen.labels().some((label) => /publish|activate|assign|duplicate/i.test(label))).toBe(false);
    screen.click("All template roles"); screen.click("All template versions");
    expect(screen.labels()).toContain(`Template: ${template.id} v1`);
  });
});
