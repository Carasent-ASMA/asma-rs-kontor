import { describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({ BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, Pressable: "Pressable", Text: "Text", View: "View" }));

import { containedTab, measureMenu, menuFrame, type FocusTarget, type MenuFrame } from "../client/anchored-menu";

const root = { width: 1000, height: 700 };

describe("anchored menu placement", () => {
  it("opens below the trigger, start- or end-aligned to it", () => {
    const anchor = { x: 100, y: 40, width: 200, height: 44 };
    expect(menuFrame(anchor, root, { align: "start", width: 380 })).toEqual({ left: 100, width: 380, top: 88, bottom: null, maxHeight: 606 });
    expect(menuFrame(anchor, root, { align: "end", width: 190 })).toEqual({ left: 110, width: 190, top: 88, bottom: null, maxHeight: 606 });
  });

  it("clamps the panel inside the root margins and to the root width", () => {
    expect(menuFrame({ x: 900, y: 10, width: 80, height: 30 }, root, { align: "start", width: 380 }).left).toBe(1000 - 6 - 380);
    expect(menuFrame({ x: 2, y: 10, width: 40, height: 30 }, root, { align: "end", width: 190 }).left).toBe(6);
    expect(menuFrame({ x: 0, y: 10, width: 40, height: 30 }, { width: 300, height: 700 }, { align: "start", width: 380 }).width).toBe(288);
  });

  it("stretches across the root on compact layouts and honours the height cap", () => {
    expect(menuFrame({ x: 300, y: 620, width: 60, height: 48 }, { width: 400, height: 700 }, { align: "stretch", width: 0, gap: 6, maxHeight: 420 })).toEqual({
      left: 6, width: 388, top: null, bottom: 700 - 620 + 6, maxHeight: 420,
    });
  });

  it("covers the root's height inside its margins when asked, wherever the trigger sits", () => {
    const short = { width: 390, height: 210 };
    expect(menuFrame({ x: 330, y: 150, width: 44, height: 44 }, short, { align: "stretch", width: 0, cover: true })).toEqual({ left: 6, width: 378, top: 6, bottom: null, maxHeight: 198 });
    expect(menuFrame({ x: 330, y: 10, width: 44, height: 44 }, short, { align: "stretch", width: 0, cover: true, maxHeight: 120 })).toEqual({ left: 6, width: 378, top: 6, bottom: null, maxHeight: 120 });
  });

  it("opens upward only when the space below is the smaller and too short", () => {
    const low = menuFrame({ x: 10, y: 560, width: 100, height: 44 }, root, { align: "start", width: 200 });
    expect(low).toMatchObject({ top: null, bottom: 700 - 560 + 4, maxHeight: 560 - 4 - 6 });
    const enough = menuFrame({ x: 10, y: 400, width: 100, height: 44 }, root, { align: "start", width: 200, maxHeight: 200 });
    expect(enough).toMatchObject({ top: 448, bottom: null, maxHeight: 200 });
  });

  it("applies a measurement relative to the root, and discards a stale or empty one", () => {
    const measurable = (x: number, y: number, width: number, height: number) => ({
      measureInWindow: (callback: (x: number, y: number, width: number, height: number) => void) => callback(x, y, width, height),
    });
    const applied: MenuFrame[] = [];
    measureMenu(measurable(50, 100, 1000, 700), measurable(150, 140, 200, 44), { align: "start", width: 300 }, () => true, (frame) => applied.push(frame));
    expect(applied).toEqual([{ left: 100, width: 300, top: 88, bottom: null, maxHeight: 606 }]);
    measureMenu(measurable(0, 0, 1000, 700), measurable(150, 140, 200, 44), { align: "start", width: 300 }, () => false, (frame) => applied.push(frame));
    measureMenu(measurable(0, 0, 1000, 700), measurable(150, 140, 0, 0), { align: "start", width: 300 }, () => true, (frame) => applied.push(frame));
    measureMenu(measurable(0, 0, 0, 0), measurable(150, 140, 200, 44), { align: "start", width: 300 }, () => true, (frame) => applied.push(frame));
    measureMenu(null, measurable(150, 140, 200, 44), { align: "start", width: 300 }, () => true, (frame) => applied.push(frame));
    expect(applied).toHaveLength(1);
  });
});

/** A focusable node; one that is not drawn has no client rects, as under display: none. */
const stop = (drawn = true): FocusTarget => ({ focus: () => {}, getClientRects: () => (drawn ? [{}] : []) });
/** A modal panel whose stops are read afresh on every Tab, so a control it adds or drops counts at once. */
function modal(stops: FocusTarget[], drawn = true) {
  const panel: FocusTarget = { focus: () => {}, getClientRects: () => (drawn ? [{}] : []), querySelectorAll: () => stops, contains: (node) => node === panel || stops.includes(node as FocusTarget) };
  return panel;
}

describe("modal Tab containment", () => {
  const trigger = stop();
  const body = {};
  const screenOf = (...nodes: FocusTarget[]): FocusTarget => ({ contains: (node) => nodes.includes(node as FocusTarget) });

  it("holds focus on the panel itself when it has nothing to reach", () => {
    const panel = modal([]);
    for (const back of [false, true]) {
      expect(containedTab(panel, screenOf(trigger), panel, body, back)).toBe(panel);
      expect(containedTab(panel, screenOf(trigger), body, body, back)).toBe(panel);
    }
  });

  it("keeps a single stop focused both ways", () => {
    const close = stop();
    const panel = modal([close]);
    expect([containedTab(panel, null, close, body, false), containedTab(panel, null, close, body, true)]).toEqual([close, close]);
  });

  it("wraps at either end and leaves a move between stops to the browser", () => {
    const [first, middle, last] = [stop(), stop(), stop()];
    const panel = modal([first!, middle!, last!]);
    expect(containedTab(panel, null, last, body, false)).toBe(first);
    expect(containedTab(panel, null, first, body, true)).toBe(last);
    expect([containedTab(panel, null, first, body, false), containedTab(panel, null, middle, body, false), containedTab(panel, null, middle, body, true), containedTab(panel, null, last, body, true)]).toEqual([null, null, null, null]);
  });

  it("brings focus back in from behind the panel, from the panel itself or from the page body", () => {
    const [first, last] = [stop(), stop()];
    const panel = modal([first!, last!]);
    for (const active of [trigger, panel, body, null, undefined]) {
      expect(containedTab(panel, screenOf(trigger), active, body, false)).toBe(first);
      expect(containedTab(panel, screenOf(trigger), active, body, true)).toBe(last);
    }
  });

  it("counts only the stops drawn at the moment of the Tab", () => {
    const [first, second, hidden] = [stop(), stop(), stop(false)];
    const stops = [first!, second!, hidden!];
    const panel = modal(stops);
    expect(containedTab(panel, null, second, body, false)).toBe(first);
    expect(containedTab(panel, null, first, body, true)).toBe(second);
    const added = stop();
    stops.push(added);
    expect(containedTab(panel, null, second, body, false)).toBeNull();
    expect(containedTab(panel, null, added, body, false)).toBe(first);
    stops.splice(0, stops.length, hidden!);
    expect(containedTab(panel, null, body, body, false)).toBe(panel);
  });

  it("leaves focus in the host app outside the screen, and a panel that is not drawn, alone", () => {
    const close = stop();
    const host = stop();
    expect([containedTab(modal([close]), screenOf(trigger), host, body, false), containedTab(modal([close]), screenOf(trigger), host, body, true)]).toEqual([null, null]);
    expect([containedTab(modal([close], false), screenOf(trigger), trigger, body, false), containedTab(null, screenOf(trigger), trigger, body, false)]).toEqual([null, null]);
  });
});
