import { describe, expect, it, vi } from "vitest";
import {
  DEFAULT_ORGANIZATION_SETTINGS,
  ORGANIZATION_SETTINGS_ID,
  ORGANIZATION_SETTINGS_VERSION,
  organizationSettings,
  organizationSettingsSchema,
  organizationPresentation,
} from "../shared/organization";
import { saveOrganizationPatch } from "../client/use-organization-settings";

describe("organization settings contract", () => {
  it("registers one host-scoped versioned document with complete defaults", () => {
    expect(organizationSettings.id).toBe(ORGANIZATION_SETTINGS_ID);
    expect(organizationSettings.scope).toBe("host");
    expect(organizationSettings.version).toBe(ORGANIZATION_SETTINGS_VERSION);
    expect(ORGANIZATION_SETTINGS_VERSION).toBe(1);
    expect(organizationSettingsSchema.parse({})).toEqual(
      DEFAULT_ORGANIZATION_SETTINGS,
    );
    expect(DEFAULT_ORGANIZATION_SETTINGS).toEqual({
      grouping: "hierarchy",
      appearance: "system",
      favorites: [],
      expandedGroups: {},
      siblingOrder: {},
      displayMappings: {},
      planAssociations: {},
    });
  });

  it.each([
    ["hierarchy", "grouped", "hierarchy"],
    ["role", "grouped", "role"],
    ["flat", "flat", "hierarchy"],
  ] as const)("resolves legacy %s without adding a persisted layout", (grouping, layout, remembered) => {
    const parsed = organizationSettingsSchema.parse({ grouping });
    expect(parsed).not.toHaveProperty("layout");
    expect(organizationPresentation(parsed)).toEqual({ layout, grouping: remembered });
    expect(organizationSettingsSchema.parse(JSON.parse(JSON.stringify(parsed)))).toEqual(parsed);
    expect(parsed).toEqual({ ...DEFAULT_ORGANIZATION_SETTINGS, grouping });
  });

  it.each(["grouped", "flat"] as const)("keeps explicit %s separate from the remembered grouping", (layout) => {
    const parsed = organizationSettingsSchema.parse({ grouping: "role", layout });
    expect(organizationPresentation(parsed)).toEqual({ layout, grouping: "role" });
    expect(organizationSettingsSchema.parse(JSON.parse(JSON.stringify(parsed)))).toEqual(parsed);
    expect(organizationPresentation({ grouping: "flat", layout })).toEqual({ layout, grouping: "hierarchy" });
  });

  it("rejects invalid layouts and unknown settings fields instead of falling back", () => {
    for (const layout of ["role", "", null, true]) {
      expect(organizationSettingsSchema.safeParse({ layout }).success).toBe(false);
    }
    expect(organizationSettingsSchema.safeParse({ layout: "flat", extra: true }).success).toBe(false);
    expect(organizationSettingsSchema.safeParse({ grouping: "placement" }).success).toBe(false);
  });

  it("accepts only explicit session/workspace display mappings", () => {
    const parsed = organizationSettingsSchema.parse({
      displayMappings: {
        "session:agent-1": { workstream: "Adam", epic: "ASMA-1" },
        "workspace:wks_1": { workstream: "QA" },
      },
    });
    expect(parsed.displayMappings["session:agent-1"]).toEqual({
      workstream: "Adam",
      epic: "ASMA-1",
    });
    expect(() =>
      organizationSettingsSchema.parse({
        displayMappings: { "/tmp/arbitrary": { workstream: "QA" } },
      }),
    ).toThrow();
    expect(() =>
      organizationSettingsSchema.parse({
        displayMappings: { "session:agent-1": { workstream: "QA", extra: true } },
      }),
    ).toThrow();
  });

  it("keeps persisted order, expansion, favorites and appearance typed", () => {
    const parsed = organizationSettingsSchema.parse({
      grouping: "role",
      appearance: "dark",
      favorites: ["agent-1"],
      expandedGroups: { "workstream:pr%20review": false },
      siblingOrder: { "workspace:wks_1": ["session:agent-1"] },
    });
    expect(parsed.grouping).toBe("role");
    expect(parsed.appearance).toBe("dark");
    expect(parsed.favorites).toEqual(["agent-1"]);
    expect(parsed.expandedGroups["workstream:pr%20review"]).toBe(false);
    expect(parsed.siblingOrder["workspace:wks_1"]).toEqual(["session:agent-1"]);
  });

  it("passes the current revision and preserves the local patch on conflict", async () => {
    const save = vi.fn(async () => false);
    const result = await saveOrganizationPatch(
      {
        status: "ready",
        values: { ...DEFAULT_ORGANIZATION_SETTINGS, grouping: "role", favorites: ["agent-1"] },
        revision: "rev-7",
        saving: false,
        saveError: null,
        save,
        reset: async () => false,
        reload: async () => undefined,
      },
      { appearance: "dark", layout: "flat" },
    );
    expect(result).toBe(false);
    expect(save).toHaveBeenCalledWith(
      {
        ...DEFAULT_ORGANIZATION_SETTINGS,
        grouping: "role",
        favorites: ["agent-1"],
        appearance: "dark",
        layout: "flat",
      },
      "rev-7",
    );
  });

  it("refuses to save while loading, invalid or errored", async () => {
    const save = vi.fn(async () => true);
    for (const state of [
      { status: "loading" as const },
      { status: "invalid" as const, error: "bad", revision: "rev-1" },
      { status: "error" as const, error: "offline" },
    ]) {
      const result = await saveOrganizationPatch(
        {
          ...state,
          saving: false,
          saveError: null,
          save,
          reset: async () => false,
          reload: async () => undefined,
        },
        { appearance: "light" },
      );
      expect(result).toBe(false);
    }
    expect(save).not.toHaveBeenCalled();
  });
});
