import { describe, expect, it } from "vitest";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { rebaseOrganizationDraft } from "../client/organization-draft";
describe("two-client presentation draft reconciliation", () => {
  it("retains concurrent unrelated preferences, favorites and explicit associations", () => {
    const base = DEFAULT_ORGANIZATION_SETTINGS;
    const local = { ...base, appearance: "dark" as const, favorites: ["local"], planAssociations: { native: ["plan-local"] }, displayMappings: { "session:native": { workstream: "Local" } } };
    const remote = { ...base, grouping: "role" as const, favorites: ["remote"], expandedGroups: { group: false }, planAssociations: { native: ["plan-remote"] }, displayMappings: { "session:native": { epic: "ASMA-2" } } };
    const merged = rebaseOrganizationDraft(base, local, remote);
    expect(merged).toMatchObject({ appearance: "dark", grouping: "role", favorites: ["remote", "local"], expandedGroups: { group: false }, planAssociations: { native: ["plan-remote", "plan-local"] }, displayMappings: { "session:native": { epic: "ASMA-2", workstream: "Local" } } });
  });
  it("preserves removals and independently edited sibling parents", () => {
    const base = { ...DEFAULT_ORGANIZATION_SETTINGS, favorites: ["remove", "keep"], siblingOrder: { one: ["a", "b"], two: ["c", "d"] } };
    const merged = rebaseOrganizationDraft(base, { ...base, favorites: ["keep"], siblingOrder: { ...base.siblingOrder, one: ["b", "a"] } }, { ...base, favorites: ["remove", "keep", "remote"], siblingOrder: { ...base.siblingOrder, two: ["d", "c"] } });
    expect(merged.favorites).toEqual(["keep", "remote"]); expect(merged.siblingOrder).toEqual({ one: ["b", "a"], two: ["d", "c"] });
  });
});
