import { describe, expect, it } from "vitest";
import {
  cameraAtGraphPoint, clampGraphScale, fitPlanGraph, graphScrollEnvelope, zoomPlanGraph,
  MAX_GRAPH_SCALE, MIN_GRAPH_SCALE, GRAPH_SCALE_STEP,
} from "../client/plan-graph/layout";

describe("graph camera invariants", () => {
  const bounds = { width: 4000, height: 12000 };
  const viewport = { width: 800, height: 500 };

  it("uses the admitted scale range and quarter-step controls", () => {
    expect([MIN_GRAPH_SCALE, MAX_GRAPH_SCALE, GRAPH_SCALE_STEP]).toEqual([0.5, 2, 0.25]);
    expect([-10, 0.5, 0.75, 1, 2, 10, NaN, Infinity].map(clampGraphScale)).toEqual([0.5, 0.5, 0.75, 1, 2, 2, 1, 1]);
  });

  it.each([0.5, 0.75, 1, 1.25, 1.5, 1.75, 2])("preserves the viewport-centre graph point at scale %s when unclamped", (scale) => {
    const before = { scale: 1, x: 800, y: 3000 };
    const after = zoomPlanGraph(before, scale, bounds, viewport);
    expect((after.x + viewport.width / 2) / scale).toBe((before.x + viewport.width / 2) / before.scale);
    expect((after.y + viewport.height / 2) / scale).toBe((before.y + viewport.height / 2) / before.scale);
    expect(zoomPlanGraph(after, 1, bounds, viewport)).toEqual(before);
  });

  it("clamps offsets to scaled content and leaves small content in a viewport-sized envelope", () => {
    expect(graphScrollEnvelope({ width: 300, height: 200 }, viewport, 0.5)).toEqual(viewport);
    expect(graphScrollEnvelope(bounds, viewport, 1.5)).toEqual({ width: 6000, height: 18000 });
    expect(cameraAtGraphPoint(2, { x: 999999, y: 999999 }, bounds, viewport)).toEqual({ scale: 2, x: 7200, y: 23500 });
    expect(cameraAtGraphPoint(0.5, { x: 0, y: 0 }, bounds, viewport)).toEqual({ scale: 0.5, x: 0, y: 0 });
  });

  it("centres a selected logical node correctly at a non-unit scale", () => {
    expect(cameraAtGraphPoint(1.5, { x: 1010, y: 3000 }, bounds, viewport)).toEqual({ scale: 1.5, x: 1115, y: 4250 });
  });

  it("fits the whole graph at an exact admissible scale, including the 0.5 boundary", () => {
    const before = { scale: 1.25, x: 50, y: 100 };
    expect(fitPlanGraph(before, { width: 1000, height: 800 }, viewport)).toEqual({ scale: 0.625, x: 0, y: 0, constrained: false });
    expect(fitPlanGraph(before, { width: 1600, height: 1000 }, viewport)).toEqual({ scale: 0.5, x: 0, y: 0, constrained: false });
    expect(fitPlanGraph(before, { width: 100, height: 100 }, viewport)).toEqual({ scale: 2, x: 0, y: 0, constrained: false });
  });

  it("reports constrained fit and preserves the pan anchor instead of pretending everything fits", () => {
    const before = { scale: 1, x: 800, y: 3000 };
    const fit = fitPlanGraph(before, bounds, viewport);
    expect(fit).toEqual({ ...zoomPlanGraph(before, 0.5, bounds, viewport), constrained: true });
    expect(fit.y).toBeGreaterThan(0);
    expect(graphScrollEnvelope(bounds, viewport, fit.scale).height).toBeGreaterThan(viewport.height);
  });

  it("keeps reset deterministic and does not produce invalid offsets from unavailable bounds", () => {
    const reset = zoomPlanGraph({ scale: 0.75, x: 800, y: 3000 }, 1, bounds, viewport);
    expect(zoomPlanGraph(reset, 1, bounds, viewport)).toEqual(reset);
    const invalid = fitPlanGraph({ scale: NaN, x: Infinity, y: NaN }, { width: 0, height: NaN }, { width: 0, height: Infinity });
    expect(invalid.constrained).toBe(true);
    expect([invalid.scale, invalid.x, invalid.y].every(Number.isFinite)).toBe(true);
  });
});
