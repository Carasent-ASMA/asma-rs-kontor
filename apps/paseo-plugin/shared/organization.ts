import { defineSettings } from "@getpaseo/plugin";
import { z } from "zod";

export const ORGANIZATION_SETTINGS_ID = "organization";
export const ORGANIZATION_SETTINGS_VERSION = 1;

export type OrganizationGrouping = "hierarchy" | "role" | "flat";
export type OrganizationAppearance = "system" | "light" | "dark";

export type OrganizationDisplayMapping = {
  readonly workstream?: string;
  readonly epic?: string;
};

const displayMappingSchema = z
  .object({
    workstream: z.string().trim().min(1).optional(),
    epic: z.string().trim().min(1).optional(),
  })
  .strict();

const mappingKeySchema = z
  .string()
  .trim()
  .regex(/^(session|workspace):.+$/);

export const organizationSettingsSchema = z.object({
  grouping: z.enum(["hierarchy", "role", "flat"]).default("hierarchy"),
  layout: z.enum(["grouped", "flat"]).optional(),
  appearance: z.enum(["system", "light", "dark"]).default("system"),
  favorites: z.array(z.string().trim().min(1)).default([]),
  expandedGroups: z.record(z.string(), z.boolean()).default({}),
  siblingOrder: z.record(z.string(), z.array(z.string().trim().min(1))).default({}),
  displayMappings: z.record(mappingKeySchema, displayMappingSchema).default({}),
  // Host-local native conversation -> configured source IDs. No filesystem paths.
  planAssociations: z.record(z.string().min(1), z.array(z.string().regex(/^[a-zA-Z0-9_-]{1,128}$/))).default({}),
}).strict();

export type OrganizationSettings = z.infer<typeof organizationSettingsSchema>;

/** Resolve legacy Flat without rewriting the saved document or losing the grouped choice. */
export function organizationPresentation(settings: Pick<OrganizationSettings, "grouping" | "layout">): {
  readonly layout: NonNullable<OrganizationSettings["layout"]>;
  readonly grouping: Exclude<OrganizationGrouping, "flat">;
} {
  return {
    layout: settings.layout ?? (settings.grouping === "flat" ? "flat" : "grouped"),
    grouping: settings.grouping === "flat" ? "hierarchy" : settings.grouping,
  };
}

export const DEFAULT_ORGANIZATION_SETTINGS: OrganizationSettings =
  organizationSettingsSchema.parse({});

export const organizationSettings = defineSettings({
  id: ORGANIZATION_SETTINGS_ID,
  scope: "host",
  version: ORGANIZATION_SETTINGS_VERSION,
  schema: organizationSettingsSchema,
});
