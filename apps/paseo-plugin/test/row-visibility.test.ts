import { describe, expect, it } from "vitest";
import { rowIntersectsBody, visibleRowIds, type WindowBounds } from "../client/row-visibility";

const body: WindowBounds = { top: 300, bottom: 500 };

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
