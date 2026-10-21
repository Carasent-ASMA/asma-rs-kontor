import type { PluginTheme } from "@getpaseo/plugin";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

type Slot = { value?: unknown; deps?: readonly unknown[]; cleanup?: () => void };
type Key = { key: string; preventDefault(): void; stopPropagation(): void };

// Retain each real component's hooks across renders and model RN 0.81's newest-first Back dispatch.
const rig = vi.hoisted(() => ({
  owner: "page", cursor: 0,
  slots: new Map<string, Slot[]>(),
  effects: [] as (() => void)[],
  handlers: [] as (() => boolean)[],
  keys: [] as ((event: Key) => void)[],
  backRegistrations: 0, keyRegistrations: 0,
  slot(): Slot {
    const slots = this.slots.get(this.owner) ?? [];
    this.slots.set(this.owner, slots);
    const index = this.cursor++;
    return slots[index] ?? (slots[index] = {});
  },
  begin(owner: string) { this.owner = owner; this.cursor = 0; },
  commit() { for (const effect of this.effects.splice(0)) effect(); },
  unmount(owner: string) {
    for (const slot of this.slots.get(owner) ?? []) slot.cleanup?.();
    this.slots.delete(owner);
  },
  back() {
    for (let index = this.handlers.length - 1; index >= 0; index--) {
      if (this.handlers[index]!()) return;
    }
  },
}));

vi.mock("react", async (original) => ({
  ...await original<typeof import("react")>(),
  useRef: (initial: unknown) => {
    const slot = rig.slot();
    if (!("value" in slot)) slot.value = { current: initial };
    return slot.value;
  },
  useState: (initial: unknown) => {
    const slot = rig.slot();
    if (!("value" in slot)) slot.value = typeof initial === "function" ? initial() : initial;
    return [slot.value, () => {}];
  },
  useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) => {
    const slot = rig.slot();
    const changed = !deps || !slot.deps || slot.deps.length !== deps.length || deps.some((value, index) => !Object.is(value, slot.deps![index]));
    if (!changed) return;
    slot.deps = deps;
    rig.effects.push(() => {
      slot.cleanup?.();
      const cleanup = effect();
      slot.cleanup = typeof cleanup === "function" ? cleanup : undefined;
    });
  },
}));

vi.mock("react-native", () => ({
  AccessibilityInfo: { setAccessibilityFocus: () => {} }, findNodeHandle: () => null, Platform: { OS: "android" },
  BackHandler: { addEventListener: (_name: string, handler: () => boolean) => {
    rig.backRegistrations++;
    rig.handlers.push(handler);
    return { remove() { const index = rig.handlers.indexOf(handler); if (index >= 0) rig.handlers.splice(index, 1); } };
  } },
  Pressable: "Pressable", Text: "Text", View: "View",
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon" }));

import { MobileFiltersPage } from "../client/mobile-filters";
import { AnchoredMenu } from "../client/anchored-menu";

const colors = {} as PluginTheme["colors"];
function page(onBack: () => void, menuOpen = false) {
  rig.begin("page");
  MobileFiltersPage({ colors, onBack, menuOpen, children: null });
  rig.commit();
}
function more(onClose: () => void) {
  const ref = { current: null };
  const panel = AnchoredMenu({ anchor: ref, root: ref, rootSize: { width: 390, height: 844 }, placement: { align: "end", width: 190 }, onClose, colors, label: "More Kontor views", children: null });
  rig.begin("more");
  (panel.type as (props: unknown) => unknown)(panel.props);
  rig.commit();
}

beforeEach(() => {
  rig.slots.clear(); rig.effects = []; rig.handlers = []; rig.keys = [];
  rig.backRegistrations = 0; rig.keyRegistrations = 0;
  vi.stubGlobal("document", {
    activeElement: null, body: {},
    addEventListener: (type: string, listener: (event: Key) => void) => {
      if (type === "keydown") { rig.keys.push(listener); rig.keyRegistrations++; }
    },
    removeEventListener: (type: string, listener: (event: Key) => void) => {
      const index = rig.keys.indexOf(listener);
      if (type === "keydown" && index >= 0) rig.keys.splice(index, 1);
    },
  });
});
afterEach(() => {
  for (const owner of [...rig.slots.keys()]) rig.unmount(owner);
  vi.unstubAllGlobals();
});

describe("mobile Filters Back order", () => {
  it("closes outer More before Filters after a parent/inventory rerender, then uses the latest page callback", () => {
    const oldPageBack = vi.fn(), latestPageBack = vi.fn(), closeMore = vi.fn();
    page(oldPageBack);
    more(closeMore);
    page(latestPageBack);

    rig.back();
    expect(closeMore).toHaveBeenCalledOnce();
    expect(oldPageBack).not.toHaveBeenCalled();
    expect(latestPageBack).not.toHaveBeenCalled();

    rig.unmount("more");
    rig.back();
    expect(closeMore).toHaveBeenCalledOnce();
    expect(latestPageBack).toHaveBeenCalledOnce();
    expect(oldPageBack).not.toHaveBeenCalled();
  });

  it("keeps one Back and Escape listener across callback-only rerenders and removes both on leaving", () => {
    const oldPageBack = vi.fn(), latestPageBack = vi.fn();
    page(oldPageBack);
    page(vi.fn()); page(vi.fn()); page(latestPageBack);
    expect([rig.handlers.length, rig.keys.length, rig.backRegistrations, rig.keyRegistrations]).toEqual([1, 1, 1, 1]);
    rig.back();
    rig.keys[0]!({ key: "Escape", preventDefault: () => {}, stopPropagation: () => {} });
    expect(latestPageBack).toHaveBeenCalledTimes(2);
    expect(oldPageBack).not.toHaveBeenCalled();
    rig.unmount("page");
    expect([rig.handlers.length, rig.keys.length]).toEqual([0, 0]);
  });

  it("yields to a local select while it is open and resumes once with the latest callback", () => {
    const oldPageBack = vi.fn(), latestPageBack = vi.fn();
    page(oldPageBack);
    page(oldPageBack, true);
    page(latestPageBack, true);
    expect([rig.handlers.length, rig.keys.length]).toEqual([0, 0]);
    page(latestPageBack, false);
    page(latestPageBack, false);
    expect([rig.handlers.length, rig.keys.length, rig.backRegistrations, rig.keyRegistrations]).toEqual([1, 1, 2, 2]);
    rig.back();
    expect(latestPageBack).toHaveBeenCalledOnce();
    expect(oldPageBack).not.toHaveBeenCalled();
  });
});
