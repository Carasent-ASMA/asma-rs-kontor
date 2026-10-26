import { describe, expect, it } from "vitest";
import { placeAbove, surfaceHeight } from "../src/host/plugin-popover";

// The composer pill's popover follows Paseo's anchored surface: side top, align start, offset 12, edge padding 8.
describe("composer popover placement", () => {
  const window = { width: 1280, height: 800 }, pill = { x: 300, y: 700, width: 90, height: 32 };

  it("sits 12 above the pill at its start edge", () => {
    expect(placeAbove(pill, { width: 420, height: 440 }, window)).toEqual({ side: "top", x: 300, y: 248 });
  });

  it("stays inside the window by 8 at the right edge", () => {
    expect(placeAbove({ ...pill, x: 1100 }, { width: 420, height: 300 }, window)).toEqual({ side: "top", x: 852, y: 388 });
  });

  it("flips below only when it does not fit above and below has more room, then clamps", () => {
    expect(placeAbove({ ...pill, y: 100 }, { width: 300, height: 440 }, window)).toEqual({ side: "bottom", x: 300, y: 144 });
    // A surface taller than either side stays above and is pulled down to the edge.
    expect(placeAbove({ ...pill, y: 390 }, { width: 300, height: 784 }, { width: 1280, height: 800 })).toEqual({ side: "top", x: 300, y: 8 });
  });

  it("shows its content up to 440 tall and the window less 16", () => {
    expect([surfaceHeight(300, 800), surfaceHeight(900, 800), surfaceHeight(900, 400)]).toEqual([300, 440, 384]);
  });
});
