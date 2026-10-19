import { useCallback } from "react";
import type { SettingsState } from "@getpaseo/plugin/client";
import { useSettings } from "@getpaseo/plugin/client";
import {
  organizationSettings,
  organizationSettingsSchema,
  type OrganizationSettings,
} from "../shared/organization";

export type OrganizationSettingsState = SettingsState<
  typeof organizationSettingsSchema
>;

export async function saveOrganizationPatch(
  state: OrganizationSettingsState,
  patch: Partial<OrganizationSettings>,
): Promise<boolean> {
  if (state.status !== "ready") {
    return false;
  }
  return state.save({ ...state.values, ...patch }, state.revision);
}

export function useOrganizationSettings(): {
  readonly state: OrganizationSettingsState;
  save(patch: Partial<OrganizationSettings>): Promise<boolean>;
} {
  const state = useSettings(organizationSettings);
  const save = useCallback(
    (patch: Partial<OrganizationSettings>) => saveOrganizationPatch(state, patch),
    [state],
  );
  return { state, save };
}
