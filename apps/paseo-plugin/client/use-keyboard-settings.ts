import { useCallback } from "react";
import type { SettingsState } from "@getpaseo/plugin/client";
import { useSettings } from "@getpaseo/plugin/client";
import {
  keyboardSettings,
  keyboardSettingsSchema,
  type KeyboardSettings,
} from "../shared/keyboard";

export type KeyboardSettingsState = SettingsState<typeof keyboardSettingsSchema>;

export async function saveKeyboardPatch(
  state: KeyboardSettingsState,
  patch: Partial<KeyboardSettings>,
): Promise<boolean> {
  if (state.status !== "ready") {
    return false;
  }
  return state.save({ ...state.values, ...patch }, state.revision);
}

export function useKeyboardSettings(): {
  readonly state: KeyboardSettingsState;
  save(patch: Partial<KeyboardSettings>): Promise<boolean>;
} {
  const state = useSettings(keyboardSettings);
  const save = useCallback(
    (patch: Partial<KeyboardSettings>) => saveKeyboardPatch(state, patch),
    [state],
  );
  return { state, save };
}
