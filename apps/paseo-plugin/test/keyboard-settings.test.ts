import { describe, expect, it, vi } from "vitest";
import {
  BACK_SHORTCUTS,
  DEFAULT_KEYBOARD_SETTINGS,
  KEYBOARD_SETTINGS_ID,
  KEYBOARD_SETTINGS_VERSION,
  keyboardSettings,
  keyboardSettingsSchema,
} from "../shared/keyboard";
import { ORGANIZATION_SETTINGS_ID } from "../shared/organization";
import { saveKeyboardPatch } from "../client/use-keyboard-settings";

describe("keyboard settings contract", () => {
  it("registers its own host-scoped versioned document with the shortcut off", () => {
    expect(keyboardSettings.id).toBe(KEYBOARD_SETTINGS_ID);
    expect(keyboardSettings.id).not.toBe(ORGANIZATION_SETTINGS_ID);
    expect(keyboardSettings.scope).toBe("host");
    expect(keyboardSettings.version).toBe(KEYBOARD_SETTINGS_VERSION);
    expect(BACK_SHORTCUTS).toEqual(["off", "cmd-backquote"]);
    expect(keyboardSettingsSchema.parse({})).toEqual(DEFAULT_KEYBOARD_SETTINGS);
    expect(DEFAULT_KEYBOARD_SETTINGS).toEqual({ backShortcut: "off" });
  });

  it("accepts the listed shortcuts only, and no other field", () => {
    expect(keyboardSettingsSchema.parse({ backShortcut: "cmd-backquote" })).toEqual({ backShortcut: "cmd-backquote" });
    expect(keyboardSettingsSchema.safeParse({ backShortcut: "cmd-escape" }).success).toBe(false);
    expect(keyboardSettingsSchema.safeParse({ backShortcut: "off", grouping: "role" }).success).toBe(false);
  });

  it("saves a patch over the read values at the read revision", async () => {
    const save = vi.fn(async () => false);
    const result = await saveKeyboardPatch(
      { status: "ready", values: { backShortcut: "off" }, revision: "rev-3", saving: false, saveError: null, save, reset: async () => false, reload: async () => undefined },
      { backShortcut: "cmd-backquote" },
    );
    expect(result).toBe(false);
    expect(save).toHaveBeenCalledExactlyOnceWith({ backShortcut: "cmd-backquote" }, "rev-3");
    // An empty patch keeps the read values rather than saving an empty document.
    save.mockClear();
    await saveKeyboardPatch(
      { status: "ready", values: { backShortcut: "cmd-backquote" }, revision: "rev-4", saving: false, saveError: null, save, reset: async () => false, reload: async () => undefined },
      {},
    );
    expect(save).toHaveBeenCalledExactlyOnceWith({ backShortcut: "cmd-backquote" }, "rev-4");
  });

  it("refuses to save while loading, invalid or errored", async () => {
    const save = vi.fn(async () => true);
    const rest = { saving: false, saveError: null, save, reset: async () => false, reload: async () => undefined };
    for (const state of [
      { status: "loading" as const, ...rest },
      { status: "invalid" as const, error: "bad", revision: "rev-1", ...rest },
      { status: "error" as const, error: "offline", ...rest },
    ]) {
      expect(await saveKeyboardPatch(state, { backShortcut: "cmd-backquote" })).toBe(false);
    }
    expect(save).not.toHaveBeenCalled();
  });
});
