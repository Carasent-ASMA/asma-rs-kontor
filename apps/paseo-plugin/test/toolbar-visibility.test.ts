import { describe, expect, it } from "vitest";
import {
  accumulateScrollDelta,
  COLLAPSE_SCROLL_DELTA,
  EMPTY_SCROLL_ACCUMULATOR,
  initialToolbarVisibility,
  reduceToolbarVisibility,
  REVEAL_SCROLL_DELTA,
  toolbarCompensationHeight,
  toolbarLayout,
  toolbarScrollCompensation,
  TOP_REVEAL_OFFSET,
} from "../client/toolbar-visibility";

function context(overrides: {
  offset?: number;
  delta?: number;
  contentHeight?: number;
  viewportHeight?: number;
  headerHeight?: number;
  pinned?: boolean;
  suppressed?: boolean;
} = {}) {
  return {
    offset: overrides.offset ?? 400,
    delta: overrides.delta ?? 0,
    contentHeight: overrides.contentHeight ?? 2000,
    viewportHeight: overrides.viewportHeight ?? 600,
    headerHeight: overrides.headerHeight ?? 0,
    pinned: overrides.pinned ?? false,
    suppressed: overrides.suppressed ?? false,
  };
}

describe("compact toolbar visibility", () => {
  it("accumulates a slow same-direction scroll but resets on a reversal", () => {
    let accumulator = accumulateScrollDelta(EMPTY_SCROLL_ACCUMULATOR, 8);
    accumulator = accumulateScrollDelta(accumulator, 8);
    accumulator = accumulateScrollDelta(accumulator, 8);
    expect(accumulator).toEqual({ direction: 1, distance: 24 });

    accumulator = accumulateScrollDelta(accumulator, -3);
    expect(accumulator).toEqual({ direction: -1, distance: -3 });
    accumulator = accumulateScrollDelta(accumulator, -8);
    expect(accumulator).toEqual({ direction: -1, distance: -11 });

    expect(accumulateScrollDelta(accumulator, 0)).toEqual(accumulator);
  });

  it("collapses only after a deliberate downward delta", () => {
    expect(
      reduceToolbarVisibility("expanded", context({ delta: COLLAPSE_SCROLL_DELTA })),
    ).toBe("expanded");
    expect(
      reduceToolbarVisibility("expanded", context({ delta: COLLAPSE_SCROLL_DELTA + 1 })),
    ).toBe("collapsed");
    expect(reduceToolbarVisibility("collapsed", context({ delta: 4 }))).toBe(
      "collapsed",
    );
  });

  it("reveals on a short reverse scroll at the current position", () => {
    expect(
      reduceToolbarVisibility("collapsed", context({ delta: -REVEAL_SCROLL_DELTA })),
    ).toBe("collapsed");
    expect(
      reduceToolbarVisibility(
        "collapsed",
        context({ delta: -REVEAL_SCROLL_DELTA - 1 }),
      ),
    ).toBe("expanded");
    expect(
      reduceToolbarVisibility(
        "collapsed",
        context({ offset: 1800, delta: -11, contentHeight: 2000 }),
      ),
    ).toBe("expanded");
  });

  it("does not collapse before the toolbar can be compensated away", () => {
    expect(
      reduceToolbarVisibility(
        "expanded",
        context({ offset: 200, delta: 100, headerHeight: 300 }),
      ),
    ).toBe("expanded");
    expect(
      reduceToolbarVisibility(
        "expanded",
        context({ offset: 400, delta: 100, headerHeight: 300 }),
      ),
    ).toBe("collapsed");
    expect(
      reduceToolbarVisibility(
        "collapsed",
        context({ offset: 200, delta: 100, headerHeight: 300 }),
      ),
    ).toBe("collapsed");
  });

  it("always reveals at the top and ignores overscroll or non-scrollable content", () => {
    expect(
      reduceToolbarVisibility(
        "collapsed",
        context({ offset: TOP_REVEAL_OFFSET, delta: 100 }),
      ),
    ).toBe("expanded");
    expect(
      reduceToolbarVisibility("expanded", context({ offset: -20, delta: 100 })),
    ).toBe("expanded");
    expect(
      reduceToolbarVisibility(
        "expanded",
        context({ offset: 900, delta: 100, contentHeight: 1000, viewportHeight: 600 }),
      ),
    ).toBe("expanded");
    expect(
      reduceToolbarVisibility(
        "expanded",
        context({ offset: 300, delta: 100, contentHeight: 600, viewportHeight: 600 }),
      ),
    ).toBe("expanded");
  });

  it("keeps focused, filter-active and accessibility contexts pinned open", () => {
    expect(
      reduceToolbarVisibility("collapsed", context({ delta: 100, pinned: true })),
    ).toBe("expanded");
  });

  it("ignores programmatic restoration and zero-delta reorders", () => {
    expect(
      reduceToolbarVisibility(
        "collapsed",
        context({ delta: 100, suppressed: true }),
      ),
    ).toBe("collapsed");
    expect(reduceToolbarVisibility("collapsed", context({ delta: 0 }))).toBe(
      "collapsed",
    );
    expect(reduceToolbarVisibility("expanded", context({ delta: 0 }))).toBe(
      "expanded",
    );
  });

  it("starts collapsed for a restored deep list and expanded at the top", () => {
    expect(initialToolbarVisibility(0)).toBe("expanded");
    expect(initialToolbarVisibility(240)).toBe("collapsed");
  });

  it("uses one slim row when collapsed and never leaves a spacer gap", () => {
    expect(toolbarLayout({ collapsed: true, compact: false })).toEqual({
      showSlimRow: true,
      showFullToolbar: false,
      spacerHeight: 0,
    });
    expect(toolbarLayout({ collapsed: false, compact: true })).toEqual({
      showSlimRow: true,
      showFullToolbar: true,
      spacerHeight: 0,
    });
    expect(toolbarLayout({ collapsed: false, compact: false })).toEqual({
      showSlimRow: false,
      showFullToolbar: true,
      spacerHeight: 0,
    });
  });

  it("uses the net header height when the wide slim row appears or disappears", () => {
    expect(
      toolbarCompensationHeight({ fullHeight: 260, slimHeight: 44, compact: true }),
    ).toBe(260);
    expect(
      toolbarCompensationHeight({ fullHeight: 260, slimHeight: 44, compact: false }),
    ).toBe(216);
    expect(
      toolbarCompensationHeight({ fullHeight: 30, slimHeight: 44, compact: false }),
    ).toBe(0);
  });

  it("compensates the visible position when layout space is reclaimed", () => {
    expect(
      toolbarScrollCompensation({
        currentOffset: 500,
        headerHeight: 120,
        direction: "collapse",
        maxOffset: 2000,
      }),
    ).toBe(380);
    expect(
      toolbarScrollCompensation({
        currentOffset: 380,
        headerHeight: 120,
        direction: "reveal",
        maxOffset: 2000,
      }),
    ).toBe(500);
    expect(
      toolbarScrollCompensation({
        currentOffset: 4,
        headerHeight: 120,
        direction: "reveal",
        maxOffset: 2000,
      }),
    ).toBe(4);
    expect(
      toolbarScrollCompensation({
        currentOffset: 1950,
        headerHeight: 120,
        direction: "reveal",
        maxOffset: 2000,
      }),
    ).toBe(2000);
    expect(
      toolbarScrollCompensation({
        currentOffset: 500,
        headerHeight: 0,
        direction: "collapse",
        maxOffset: 2000,
      }),
    ).toBe(500);
  });
});
