import { describe, expect, it, vi } from "vitest";

vi.mock("react-native", () => ({ BackHandler: { addEventListener: () => ({ remove: () => undefined }) }, Pressable: "Pressable", Text: "Text", View: "View" }));

import { measureMenu, menuFrame, type MenuFrame } from "../client/anchored-menu";

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
