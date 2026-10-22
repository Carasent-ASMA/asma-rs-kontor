import { describe, expect, it } from "vitest";
import { DEFAULT_ORGANIZATION_SETTINGS, organizationSettingsSchema } from "../shared/organization";
import { rebaseOrganizationDraft, setDisplayMappingField } from "../client/organization-draft";
describe("two-client presentation draft reconciliation", () => {
  it("reapplies layout while retaining a concurrent grouping and unrelated fields", () => {
    const base = DEFAULT_ORGANIZATION_SETTINGS;
    const local = { ...base, layout: "flat" as const };
    const current = { ...base, grouping: "role" as const, appearance: "dark" as const, favorites: ["remote"] };
    expect(rebaseOrganizationDraft(base, local, current)).toEqual({ ...current, layout: "flat" });
  });
  it("reapplies grouping while retaining an unedited concurrent layout", () => {
    const base = DEFAULT_ORGANIZATION_SETTINGS;
    const local = { ...base, grouping: "role" as const };
    const current = { ...base, layout: "flat" as const, expandedGroups: { remote: false } };
    expect(rebaseOrganizationDraft(base, local, current)).toEqual({ ...current, grouping: "role" });
  });
  it("reapplies an explicit layout conflict without replacing concurrent grouping", () => {
    const base = DEFAULT_ORGANIZATION_SETTINGS;
    const local = { ...base, layout: "grouped" as const };
    const current = { ...base, layout: "flat" as const, grouping: "role" as const };
    expect(rebaseOrganizationDraft(base, local, current)).toEqual({ ...current, layout: "grouped" });
  });
  it("omits a removed layout and keeps legacy drafts JSON-safe", () => {
    const legacy = DEFAULT_ORGANIZATION_SETTINGS;
    const base = { ...legacy, layout: "flat" as const };
    const current = { ...base, layout: "grouped" as const, grouping: "role" as const };
    const removed = rebaseOrganizationDraft(base, legacy, current);
    expect(removed).toEqual({ ...legacy, grouping: "role" });
    expect(removed).not.toHaveProperty("layout");
    expect(JSON.parse(JSON.stringify(removed))).toEqual(removed);
    expect(organizationSettingsSchema.parse(removed)).toEqual(removed);
    const unchanged = rebaseOrganizationDraft(legacy, legacy, legacy);
    expect(unchanged).not.toHaveProperty("layout");
    expect(JSON.parse(JSON.stringify(unchanged))).toEqual(unchanged);
  });
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
