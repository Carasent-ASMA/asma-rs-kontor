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
vi.mock("react-native", () => ({ ScrollView: "ScrollView", View: "View", Text: "Text" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
import { OrganizationSettingsPage } from "../client/organization-settings-page";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationSettingsSchema, type OrganizationSettings } from "../shared/organization";
import { sessionEntry } from "./session-fixtures";
function elements(node: ReactNode): ReactElement<Record<string, unknown>>[] {
  if (Array.isArray(node)) return node.flatMap(elements);
  if (!isValidElement<Record<string, unknown>>(node)) return [];
  return [node, ...elements(node.props.children as ReactNode)];
}
function displayedText(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(displayedText).join("");
  return isValidElement<Record<string, unknown>>(node) ? displayedText(node.props.children as ReactNode) : "";
}
function mount() {
  const values: OrganizationSettings = { ...DEFAULT_ORGANIZATION_SETTINGS, displayMappings: { "session:native": { workstream: "Adam", epic: "ASMA-1" }, "workspace:other": { workstream: "QA" } } };
  let persisted = { values, revision: "rev-a" };
  let generation = 1;
  let saveError: string | null = null;
  const nextRevision = () => `rev-${String.fromCharCode(97 + generation++)}`;
  // Model the SDK compare-and-set boundary; a newer remote revision rejects an old draft.
  const save = vi.fn(async (next: OrganizationSettings, expectedRevision: string) => {
    z.json().parse(next); const parsed = organizationSettingsSchema.parse(next);
    if (expectedRevision !== persisted.revision) { saveError = "conflict"; return false; }
    persisted = { values: parsed, revision: nextRevision() }; saveError = null; return true;
  });
  type Props = Parameters<typeof OrganizationSettingsPage>[0];
  const readyState = (): Props["state"] => ({ status: "ready", ...persisted, saving: false, saveError, save, reset: async () => false, reload: async () => {} });
  const props: Props = { state: readyState(), colors: {} as Props["colors"], entries: [sessionEntry("native")], workspaces: [], sources: [], reads: {}, sourceError: null, refresh: vi.fn() };
  let tree: ReactNode;
  const render = () => { props.state = readyState(); model.cursor = 0; tree = OrganizationSettingsPage(props); };
  const action = (label: string) => elements(tree).find((element) => element.props.label === label)!;
  const input = (label: string) => elements(tree).find((element) => element.props.accessibilityLabel === label)!;
  const click = (label: string) => { (action(label).props.onPress as () => void)(); render(); };
  render(); click("Edit presentation"); click(`Map session: ${props.entries[0]!.title}`);
  return { save, props, click, action, input, text: () => displayedText(tree),
    saved: () => ({ revision: persisted.revision, values: organizationSettingsSchema.parse(persisted.values) }),
    publish: (remote: OrganizationSettings) => { persisted = { values: organizationSettingsSchema.parse(remote), revision: nextRevision() }; render(); },
    settle: async () => { await Promise.resolve(); render(); },
    clear(label: string, value = "") {
    (input(label).props.onChangeText as (value: string) => void)(value); render();
  } };
}
beforeEach(() => { model.slots = []; model.cursor = 0; });
describe("Settings page draft callbacks", () => {
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
  it("rejects an opening revision after a remote publication, retains the draft and explicitly reapplies onto the new revision", async () => {
    const screen = mount();
    screen.clear("Display workstream", "Local review desk"); screen.click("role");
    expect(screen.text()).toContain("Editing revision rev-a");
    const remote: OrganizationSettings = { ...screen.saved().values, appearance: "dark", favorites: ["remote-session"], expandedGroups: { "remote-group": true }, siblingOrder: { "remote-parent": ["second", "first"] }, planAssociations: { "remote-session": ["remote-plan"] }, displayMappings: { "session:native": { workstream: "Adam", epic: "ASMA-2" }, "workspace:other": { workstream: "Remote QA", epic: "ASMA-3" } } };
    // Rerender the actual page with B while its existing hook slots retain the local A draft.
    screen.publish(remote);
    expect(screen.text()).toContain("Saved revision rev-b");
    expect(screen.text()).toContain("Editing revision rev-a · newer saved values exist; this draft must be reviewed");
    expect(screen.input("Display workstream").props.value).toBe("Local review desk");
    expect(screen.input("Display epic").props.value).toBe("ASMA-1");
    expect(screen.action("role").props.selected).toBe(true); expect(screen.action("system").props.selected).toBe(true);

    screen.click("Save preferences"); await screen.settle();
    expect(screen.text()).toContain("Save failed: conflict");
    expect(screen.text()).toContain("Preferences were not saved. Draft retained");
    expect(screen.text()).toContain("Editing revision rev-a");
    expect(screen.input("Display workstream").props.value).toBe("Local review desk");
    expect(screen.input("Display epic").props.value).toBe("ASMA-1");
    expect(screen.action("role").props.selected).toBe(true);
    expect(screen.save).toHaveBeenCalledTimes(1); expect(screen.save.mock.calls[0]![1]).toBe("rev-a");
    expect(await screen.save.mock.results[0]!.value).toBe(false);
    expect(screen.saved()).toEqual({ revision: "rev-b", values: remote });

    screen.click("Reapply local changes to latest saved values");
    expect(screen.text()).toContain("Editing revision rev-b");
    expect(screen.text()).toContain("Local edits reapplied. Review the draft, then save against the latest revision.");
    expect(screen.input("Display workstream").props.value).toBe("Local review desk");
    expect(screen.input("Display epic").props.value).toBe("ASMA-2");
    expect(screen.action("role").props.selected).toBe(true); expect(screen.action("dark").props.selected).toBe(true);
    expect(screen.saved()).toEqual({ revision: "rev-b", values: remote });
    expect(screen.save).toHaveBeenCalledTimes(1);

    screen.click("Save preferences"); await screen.settle();
    expect(screen.text()).toContain("Preferences saved on this host."); expect(screen.text()).not.toContain("Saved revision");
    expect(screen.input("Saved revision rev-c. Grouping role · appearance dark · 1 conversation with explicit plan links · 2 display mappings")).toBeDefined();
    expect(screen.text()).not.toContain("Editing revision"); expect(screen.text()).not.toContain("Save failed:");
    expect(screen.save).toHaveBeenCalledTimes(2); expect(screen.save.mock.calls[1]![1]).toBe("rev-b");
    expect(await screen.save.mock.results[1]!.value).toBe(true);
    const saved = screen.saved();
    expect(saved.revision).toBe("rev-c");
    expect(saved.values).toEqual({ ...remote, grouping: "role", displayMappings: { ...remote.displayMappings, "session:native": { workstream: "Local review desk", epic: "ASMA-2" } } });
    screen.click("Edit presentation"); screen.click(`Map session: ${screen.props.entries[0]!.title}`);
    expect(screen.input("Display workstream").props.value).toBe("Local review desk");
    expect(screen.input("Display epic").props.value).toBe("ASMA-2");
    expect(screen.action("role").props.selected).toBe(true); expect(screen.action("dark").props.selected).toBe(true);
  });
});
