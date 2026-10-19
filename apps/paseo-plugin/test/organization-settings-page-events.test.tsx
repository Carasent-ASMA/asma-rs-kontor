import { isValidElement, type ReactElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { z } from "zod";
const model = vi.hoisted(() => ({ slots: [] as unknown[], cursor: 0,
  useState<T>(initial: T): [T, (next: T) => void] {
    const index = this.cursor++; if (!(index in this.slots)) this.slots[index] = initial;
    return [this.slots[index] as T, (next) => { this.slots[index] = next; }];
  },
}));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(), useState: <T,>(initial: T) => model.useState(initial) }));
vi.mock("react-native", () => ({ ScrollView: "ScrollView", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { OrganizationSettingsPage } from "../client/organization-settings-page";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationSettingsSchema, type OrganizationSettings } from "../shared/organization";
import { sessionEntry } from "./session-fixtures";
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...elements(node.props.children as ReactNode)];
}
function mount() {
  const values: OrganizationSettings = { ...DEFAULT_ORGANIZATION_SETTINGS, displayMappings: { "session:native": { workstream: "Adam", epic: "ASMA-1" }, "workspace:other": { workstream: "QA" } } };
  const save = vi.fn(async (next: OrganizationSettings) => {
    z.json().parse(next); organizationSettingsSchema.parse(next); return true;
  });
  const props: Parameters<typeof OrganizationSettingsPage>[0] = { state: { status: "ready", values, revision: "rev-a", saving: false, saveError: null, save, reset: async () => false, reload: async () => {} }, colors: {} as Parameters<typeof OrganizationSettingsPage>[0]["colors"], entries: [sessionEntry("native")], workspaces: [], sources: [], reads: {}, sourceError: null, refresh: vi.fn() };
  let tree: ReactNode;
  const render = () => { model.cursor = 0; tree = OrganizationSettingsPage(props); };
  const action = (label: string) => elements(tree).find((element) => element.props.label === label)!;
  const click = (label: string) => { (action(label).props.onPress as () => void)(); render(); };
  render(); click("Edit presentation"); click(`Map session: ${props.entries[0]!.title}`);
  return { save, props, click, clear(label: string, value = "") {
    const input = elements(tree).find((element) => element.props.accessibilityLabel === label)!;
    (input.props.onChangeText as (value: string) => void)(value); render();
  } };
}
beforeEach(() => { model.slots = []; model.cursor = 0; });
describe("mapping clear callbacks", () => {
  it.each(["workstream", "epic"] as const)("saves a JSON-valid cleared %s while retaining the other field", async (field) => {
    const screen = mount(); screen.clear(`Display ${field}`, "   "); screen.click("Save preferences");
    await Promise.resolve();
    const saved = screen.save.mock.calls[0]![0];
    expect(screen.save).toHaveBeenCalledWith(expect.anything(), "rev-a");
    expect(saved.displayMappings["session:native"]).toEqual(field === "workstream" ? { epic: "ASMA-1" } : { workstream: "Adam" });
    expect(saved.displayMappings["workspace:other"]).toEqual({ workstream: "QA" });
    expect(JSON.parse(JSON.stringify(saved))).toEqual(saved);
  });
  it("removes a mapping once both inputs are cleared", async () => {
    const screen = mount(); screen.clear("Display workstream"); screen.clear("Display epic"); screen.click("Save preferences");
    await Promise.resolve();
    expect(screen.save.mock.calls[0]![0].displayMappings).toEqual({ "workspace:other": { workstream: "QA" } });
  });
});
