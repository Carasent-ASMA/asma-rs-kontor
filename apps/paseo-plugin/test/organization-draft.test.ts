import { describe, expect, it } from "vitest";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationSettingsSchema } from "../shared/organization";
import { rebaseOrganizationDraft, setDisplayMappingField } from "../client/organization-draft";
describe("two-client presentation draft reconciliation", () => {
  it("retains concurrent unrelated preferences, favorites and explicit associations", () => {
    const base = DEFAULT_ORGANIZATION_SETTINGS;
    const local = { ...base, appearance: "dark" as const, favorites: ["local"], planAssociations: { native: ["plan-local"] }, displayMappings: { "session:native": { workstream: "Local" } } };
    const remote = { ...base, grouping: "role" as const, favorites: ["remote"], expandedGroups: { group: false }, planAssociations: { native: ["plan-remote"] }, displayMappings: { "session:native": { epic: "ASMA-2" } } };
    const merged = rebaseOrganizationDraft(base, local, remote);
    expect(merged).toMatchObject({ appearance: "dark", grouping: "role", favorites: ["remote", "local"], expandedGroups: { group: false }, planAssociations: { native: ["plan-remote", "plan-local"] }, displayMappings: { "session:native": { epic: "ASMA-2", workstream: "Local" } } });
  });
  it("reapplies mapping clears as JSON-safe removals while preserving concurrent fields", () => {
    const base = { ...DEFAULT_ORGANIZATION_SETTINGS, displayMappings: { "session:native": { workstream: "Local", epic: "ASMA-1" } } };
    const partial = { ...base, displayMappings: setDisplayMappingField(base.displayMappings, "session:native", "workstream", "") };
    const cleared = { ...base, displayMappings: setDisplayMappingField(partial.displayMappings, "session:native", "epic", "") };
    const remote = { ...base, displayMappings: { "session:native": { workstream: "Remote", epic: "ASMA-2" }, "workspace:other": { workstream: "Other" } } };
    const merged = rebaseOrganizationDraft(base, partial, remote);
    expect(merged.displayMappings).toEqual({ "session:native": { epic: "ASMA-2" }, "workspace:other": { workstream: "Other" } });
    expect(organizationSettingsSchema.parse(merged)).toEqual(merged);
    const removed = rebaseOrganizationDraft(base, cleared, remote);
    expect(removed.displayMappings).toEqual({ "workspace:other": { workstream: "Other" } });
    expect(JSON.parse(JSON.stringify(removed))).toEqual(removed);
  });
  it("preserves removals and independently edited sibling parents", () => {
    const base = { ...DEFAULT_ORGANIZATION_SETTINGS, favorites: ["remove", "keep"], siblingOrder: { one: ["a", "b"], two: ["c", "d"] } };
    const merged = rebaseOrganizationDraft(base, { ...base, favorites: ["keep"], siblingOrder: { ...base.siblingOrder, one: ["b", "a"] } }, { ...base, favorites: ["remove", "keep", "remote"], siblingOrder: { ...base.siblingOrder, two: ["d", "c"] } });
    expect(merged.favorites).toEqual(["keep", "remote"]); expect(merged.siblingOrder).toEqual({ one: ["b", "a"], two: ["d", "c"] });
  });
});
