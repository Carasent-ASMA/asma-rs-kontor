import { describe, expect, it, vi } from "vitest";
import { rowIntersectsBody, useRowVisibility, visibleRowIds, type WindowBounds } from "../client/row-visibility";

const body: WindowBounds = { top: 300, bottom: 500 };

// Drive the real hook outside React: a single state cell the mock's setter writes (functional updates
// included), fresh per test, so measure/settle lifecycle behavior is observable.
const hookState = vi.hoisted(() => ({ value: new Set<string>() }));
vi.mock("react", () => ({
  useRef: (value: unknown) => ({ current: value }),
  useCallback: (fn: unknown) => fn,
  useState: () => [hookState.value, (next: Set<string> | ((previous: Set<string>) => Set<string>)) => { hookState.value = typeof next === "function" ? next(hookState.value) : next; }],
}));

describe("native row visibility hook lifecycle", () => {
  const settledBody = { measureInWindow: (cb: (x: number, y: number, width: number, height: number) => void) => cb(0, 100, 0, 400) };
  const liveRow = () => ({ measureInWindow: (cb: (x: number, y: number, width: number, height: number) => void) => cb(0, 120, 0, 40) });

  it("settles measured rows into the visible set, excludes offscreen rows, and evicts rows that leave", () => {
    hookState.value = new Set();
    const gate = useRowVisibility();
    const row = liveRow();
    const offscreenRow = { measureInWindow: (cb: (x: number, y: number, width: number, height: number) => void) => cb(0, 900, 0, 40) };
    gate.register("a", row);
    gate.register("b", offscreenRow);
    gate.measure(settledBody);
    expect([...hookState.value]).toEqual(["a"]);
    // The row scrolls out; the next settled pass evicts it (a never-un-settling mutant fails here).
    row.measureInWindow = (cb: (x: number, y: number, width: number, height: number) => void) => cb(0, 900, 0, 40);
    gate.measure(settledBody);
    expect([...hookState.value]).toEqual([]);
  });

  it("coalesces overlapping measure passes into one trailing evaluation", () => {
    hookState.value = new Set();
    const gate = useRowVisibility();
    let deliver: ((x: number, y: number, width: number, height: number) => void) | null = null;
    const inFlightBody = { measureInWindow: (cb: (x: number, y: number, width: number, height: number) => void) => { deliver = cb; } };
    gate.register("a", liveRow());
    gate.measure(inFlightBody);
    gate.measure(inFlightBody); // overlaps the in-flight pass: coalesced into the trailing evaluation
    expect(hookState.value.size).toBe(0); // nothing applies before the pass settles
    deliver!(0, 100, 0, 400);
    expect([...hookState.value]).toEqual(["a"]);
  });

  it("leaves the gate empty when a pass never settles, so nothing animates on unknown", () => {
    // An undelivered body callback (an RN contract violation, and the r5 mutant's symptom) must never
    // promote rows to visible: conservative static beats a wrong animation.
    hookState.value = new Set();
    const gate = useRowVisibility();
    const silentBody = { measureInWindow: () => undefined };
    gate.register("a", liveRow());
    gate.measure(silentBody);
    expect(hookState.value.size).toBe(0);
  });
});

describe("native row visibility gate", () => {
  it("animates only rows whose measured window bounds intersect the scroll body", () => {
    expect(rowIntersectsBody({ top: 320, bottom: 360 }, body)).toBe(true);
    expect(rowIntersectsBody({ top: 480, bottom: 520 }, body)).toBe(true); // straddles the bottom edge
    expect(rowIntersectsBody({ top: 280, bottom: 320 }, body)).toBe(true); // straddles the top edge
    expect(rowIntersectsBody({ top: 100, bottom: 140 }, body)).toBe(false);
    expect(rowIntersectsBody({ top: 600, bottom: 640 }, body)).toBe(false);
    expect(rowIntersectsBody({ top: 500, bottom: 540 }, body)).toBe(false); // starts exactly at the edge
    expect(rowIntersectsBody({ top: 260, bottom: 300 }, body)).toBe(false); // ends exactly at the edge
  });

  it("keeps unmeasured rows static and needs a measured body", () => {
    const rows = new Map<string, WindowBounds | undefined>([
      ["measured-visible", { top: 320, bottom: 360 }],
      ["measured-offscreen", { top: 900, bottom: 940 }],
      ["unmeasured", undefined],
    ]);
    expect(visibleRowIds(rows, body)).toEqual(new Set(["measured-visible"]));
    expect(visibleRowIds(rows, undefined)).toEqual(new Set());
    expect(visibleRowIds(new Map(), body)).toEqual(new Set());
  });
});
