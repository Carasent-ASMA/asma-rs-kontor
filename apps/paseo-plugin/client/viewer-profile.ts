import { ACCOUNT_ID } from "../shared/responsibility";

/**
 * Working as, remembered on this device: a viewing profile chosen once from the roster for assignment
 * filters, never sign-in and never a detected identity. One value per host and tracker realm, holding only
 * the tracker account ID. iOS keeps it in the app's user defaults (React Native Settings), web and desktop in
 * the app's local storage. Elsewhere, or when the store fails, a choice lasts this app session and says so.
 */
export type WorkingAsPersistence = "device" | "session" | "failed";
export type RememberedWorkingAs = { readonly accountId: string | null; readonly persistence: WorkingAsPersistence };
export type ProfileStorage = { read(key: string): unknown; write(key: string, value: string | null): void };
type NativeSettings = { get(key: string): unknown; set(values: Record<string, string | null>): void };
type WebStorage = { getItem(key: string): string | null; setItem(key: string, value: string): void; removeItem(key: string): void };

const PREFIX = "kontor.working-as@1 ";

export function profileStorageKey(hostId: string, realm: string): string {
  return PREFIX + JSON.stringify([hostId.trim(), realm]);
}

/**
 * This runtime's device store, or null. Reaching it can throw (no native settings module, a stub runtime),
 * which also means none; local storage is read only on web, never as a global on native.
 */
export function deviceProfileStorage(runtime: () => { readonly platform: string; readonly settings?: NativeSettings }): ProfileStorage | null {
  try {
    const { platform, settings } = runtime();
    if (platform === "ios" && settings) {
      settings.get(PREFIX);
      // A null value removes the key from user defaults.
      return { read: (key) => settings.get(key), write: (key, value) => settings.set({ [key]: value }) };
    }
    const local = platform === "web" ? (globalThis as { localStorage?: WebStorage }).localStorage : undefined;
    if (local) {
      local.getItem(PREFIX);
      return { read: (key) => local.getItem(key), write: (key, value) => value === null ? local.removeItem(key) : local.setItem(key, value) };
    }
  } catch {
    // Present but unreachable: this app session only.
  }
  return null;
}

/** An unreadable or foreign value binds nobody; it stays until a choice or Forget replaces it. */
export function loadWorkingAs(storage: ProfileStorage | null, hostId: string, realm: string): RememberedWorkingAs {
  if (!storage) return { accountId: null, persistence: "session" };
  try {
    const value = storage.read(profileStorageKey(hostId, realm));
    if (value === null || value === undefined) return { accountId: null, persistence: "device" };
    if (typeof value === "string" && ACCOUNT_ID.test(value)) return { accountId: value, persistence: "device" };
  } catch {
    // Unreadable, like a value this version does not accept.
  }
  return { accountId: null, persistence: "failed" };
}

/** Null forgets. A failed write keeps the choice for this app session and says it is not remembered. */
export function saveWorkingAs(storage: ProfileStorage | null, hostId: string, realm: string, accountId: string | null): RememberedWorkingAs {
  if (!storage) return { accountId, persistence: "session" };
  try {
    storage.write(profileStorageKey(hostId, realm), accountId);
    return { accountId, persistence: "device" };
  } catch {
    return { accountId, persistence: "failed" };
  }
}
