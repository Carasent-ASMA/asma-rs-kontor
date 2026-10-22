import { afterEach, describe, expect, it, vi } from "vitest";
import { deviceProfileStorage, loadWorkingAs, profileStorageKey, saveWorkingAs, type ProfileStorage } from "../client/viewer-profile";

/** A disposable Web Storage: the app's own local storage is never touched. */
function webStorage(entries = new Map<string, string>()) {
  return { entries, getItem: (key: string) => entries.get(key) ?? null, setItem: (key: string, value: string) => { entries.set(key, value); }, removeItem: (key: string) => { entries.delete(key); } };
}

/**
 * React Native's iOS Settings over a disposable user-defaults map: get reads the snapshot taken when the
 * module loaded, set merges into it and persists, and a null value removes the key.
 */
function iosSettings(defaults: Map<string, string>) {
  const snapshot: Record<string, unknown> = Object.fromEntries(defaults);
  return {
    get: (key: string) => snapshot[key],
    set: (values: Record<string, string | null>) => {
      Object.assign(snapshot, values);
      for (const [key, value] of Object.entries(values)) if (value === null) defaults.delete(key); else defaults.set(key, value);
    },
  };
}

const global = globalThis as { localStorage?: unknown };
afterEach(() => { delete global.localStorage; vi.resetModules(); });

/** A new plugin context: fresh module instances, so nothing survives except the device store. */
async function freshPluginContext(runtime: Parameters<typeof deviceProfileStorage>[0]) {
  vi.resetModules();
  const store = await import("../client/session-view-store");
  const profile = await import("../client/viewer-profile");
  store.configureWorkingAsStorage(profile.deviceProfileStorage(runtime));
  return store;
}

describe("Working as across a fresh plugin context", () => {
  it("restores on web and desktop from local storage at the first read, and Forget survives a reload", async () => {
    const local = webStorage();
    global.localStorage = local;
    const first = await freshPluginContext(() => ({ platform: "web" }));
    expect(first.readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: null, persistence: "device" });
    first.writeWorkingAs("host-a", "tracker.example", "account-a");
    first.writeWorkingAs("host-b", "tracker.example", "account-b");
    first.writeWorkingAs("host-a", "other.example", "account-c");

    const second = await freshPluginContext(() => ({ platform: "web" }));
    expect(second).not.toBe(first);
    // Synchronous: the choice is there before the first render evaluates My work's assignment filter.
    expect(second.readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: "account-a", persistence: "device" });
    expect(second.readWorkingAs("host-b", "tracker.example")).toBe("account-b");
    expect(second.readWorkingAs("host-a", "other.example")).toBe("account-c");
    expect(second.readWorkingAs("host-b", "other.example")).toBeNull();
    // A new app session view is My work, Any assignment: remembering a person never turns a filter on.
    const view = second.readSessionView("host-a");
    expect([second.workScopeOf(view), second.assignmentOf(view)]).toEqual(["mine", "any"]);
    second.writeWorkingAs("host-a", "tracker.example", null);

    const third = await freshPluginContext(() => ({ platform: "web" }));
    expect(third.readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: null, persistence: "device" });
    expect(third.readWorkingAs("host-b", "tracker.example")).toBe("account-b");
    expect([...local.entries.keys()].every((key) => key.startsWith("kontor.working-as@1 "))).toBe(true);
  });

  it("restores on iOS from user defaults through React Native Settings, never from a web global", async () => {
    const defaults = new Map<string, string>();
    global.localStorage = webStorage(new Map([[profileStorageKey("host-a", "tracker.example"), "account-web"]]));
    const first = await freshPluginContext(() => ({ platform: "ios", settings: iosSettings(defaults) }));
    expect(first.readWorkingAs("host-a", "tracker.example")).toBeNull();
    first.writeWorkingAs("host-a", "tracker.example", "account-a");
    const second = await freshPluginContext(() => ({ platform: "ios", settings: iosSettings(defaults) }));
    expect(second.readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: "account-a", persistence: "device" });
    second.writeWorkingAs("host-a", "tracker.example", null);
    expect(defaults.size).toBe(0);
  });

  it("keeps a choice for this app session only where no device store is reachable", async () => {
    global.localStorage = webStorage();
    const android = await freshPluginContext(() => ({ platform: "android", settings: { get: () => null, set: () => { throw new Error("SettingsFallback"); } } }));
    android.writeWorkingAs("host-a", "tracker.example", "account-a");
    expect(android.readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: "account-a", persistence: "session" });
    const reloaded = await freshPluginContext(() => ({ platform: "android" }));
    expect(reloaded.readRememberedWorkingAs("host-a", "tracker.example")).toEqual({ accountId: null, persistence: "session" });
  });
});

describe("device store adapters", () => {
  it("finds none when reaching the runtime, the native module or the web store throws", () => {
    expect(deviceProfileStorage(() => { throw new Error("no export"); })).toBeNull();
    expect(deviceProfileStorage(() => ({ platform: "ios", settings: { get: () => { throw new TypeError("no native module"); }, set: () => {} } }))).toBeNull();
    Object.defineProperty(globalThis, "localStorage", { configurable: true, get: () => { throw new Error("SecurityError"); } });
    expect(deviceProfileStorage(() => ({ platform: "web" }))).toBeNull();
    delete global.localStorage;
    expect(deviceProfileStorage(() => ({ platform: "web" }))).toBeNull();
  });

  it("keys one value per trimmed host and exact realm, so same-named people and hosts never collide", () => {
    expect(profileStorageKey(" host-a ", "tracker.example")).toBe(profileStorageKey("host-a", "tracker.example"));
    const keys = new Set([profileStorageKey("host-a", "tracker.example"), profileStorageKey("host-b", "tracker.example"), profileStorageKey("host-a", "other.example"),
      profileStorageKey("host-a\",\"tracker", "example")]);
    expect(keys.size).toBe(4);
  });

  it("binds nobody from an unreadable or foreign value, and reading never writes", () => {
    const write = vi.fn();
    const storage = (value: unknown): ProfileStorage => ({ read: () => value, write });
    expect(loadWorkingAs(storage("Alex Example"), "host-a", "tracker.example")).toEqual({ accountId: null, persistence: "failed" });
    expect(loadWorkingAs(storage(42), "host-a", "tracker.example")).toEqual({ accountId: null, persistence: "failed" });
    expect(loadWorkingAs({ read: () => { throw new Error("io"); }, write }, "host-a", "tracker.example")).toEqual({ accountId: null, persistence: "failed" });
    expect(loadWorkingAs(storage(undefined), "host-a", "tracker.example")).toEqual({ accountId: null, persistence: "device" });
    expect(loadWorkingAs(null, "host-a", "tracker.example")).toEqual({ accountId: null, persistence: "session" });
    expect(write).not.toHaveBeenCalled();
    expect(saveWorkingAs({ read: () => null, write: () => { throw new Error("quota"); } }, "host-a", "tracker.example", "account-a")).toEqual({ accountId: "account-a", persistence: "failed" });
  });
});
