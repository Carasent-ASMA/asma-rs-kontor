import { defineSettings } from "@getpaseo/plugin";
import { z } from "zod";

export const KEYBOARD_SETTINGS_ID = "keyboard";
export const KEYBOARD_SETTINGS_VERSION = 1;

/**
 * The Kontor Back shortcut. "cmd-backquote" is Cmd with the key at the Backquote position
 * (macOS keycode 50): ` on a US layout, < on a Norwegian one. Off until chosen, because
 * macOS uses the same keys for "Move focus to next window".
 */
export const BACK_SHORTCUTS = ["off", "cmd-backquote"] as const;
export type BackShortcut = (typeof BACK_SHORTCUTS)[number];

// Kept apart from the organization preferences, so a release without it leaves them valid.
export const keyboardSettingsSchema = z.object({
  backShortcut: z.enum(BACK_SHORTCUTS).default("off"),
}).strict();

export type KeyboardSettings = z.infer<typeof keyboardSettingsSchema>;

export const DEFAULT_KEYBOARD_SETTINGS: KeyboardSettings = keyboardSettingsSchema.parse({});

export const keyboardSettings = defineSettings({
  id: KEYBOARD_SETTINGS_ID,
  scope: "host",
  version: KEYBOARD_SETTINGS_VERSION,
  schema: keyboardSettingsSchema,
});
