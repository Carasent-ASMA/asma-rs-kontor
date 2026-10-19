export const COLLAPSE_SCROLL_DELTA = 18;
export const REVEAL_SCROLL_DELTA = 10;
export const TOP_REVEAL_OFFSET = 8;
export const MIN_SCROLLABLE_OVERFLOW = 24;

export type ToolbarVisibility = "expanded" | "collapsed";

export type ScrollAccumulator = {
  readonly direction: -1 | 0 | 1;
  readonly distance: number;
};

export const EMPTY_SCROLL_ACCUMULATOR: ScrollAccumulator = {
  direction: 0,
  distance: 0,
};

/**
 * Accumulates same-direction scroll deltas so a slow deliberate scroll crosses
 * the threshold, while a direction change or jitter resets the count.
 */
export function accumulateScrollDelta(
  accumulator: ScrollAccumulator,
  delta: number,
): ScrollAccumulator {
  if (!Number.isFinite(delta) || delta === 0) {
    return accumulator;
  }
  const direction: -1 | 1 = delta > 0 ? 1 : -1;
  if (direction !== accumulator.direction) {
    return { direction, distance: delta };
  }
  return { direction, distance: accumulator.distance + delta };
}

export type ToolbarScrollContext = {
  readonly offset: number;
  readonly delta: number;
  readonly contentHeight: number;
  readonly viewportHeight: number;
  /** Measured expanded toolbar height; avoids an uncompensatable shallow collapse. */
  readonly headerHeight?: number;
  /** Focused search, active filter interaction, VoiceOver or large-text pin. */
  readonly pinned: boolean;
  /** Programmatic restoration/compensation; never toggles. */
  readonly suppressed?: boolean;
};

export type ToolbarLayout = {
  readonly showSlimRow: boolean;
  readonly showFullToolbar: boolean;
  /** The collapsed state always reclaims the full toolbar height. */
  readonly spacerHeight: number;
};

/**
 * One compact toolbar variant. Deliberate downward scrolling collapses the
 * full controls; a short reverse scroll reveals them at the current position.
 * Tiny deltas, overscroll, no-overflow layouts, pins and programmatic scrolls
 * never toggle.
 */
export function reduceToolbarVisibility(
  current: ToolbarVisibility,
  context: ToolbarScrollContext,
): ToolbarVisibility {
  if (context.suppressed) {
    return current;
  }
  if (context.pinned) {
    return "expanded";
  }
  if (!Number.isFinite(context.offset) || !Number.isFinite(context.delta)) {
    return current;
  }
  const maxOffset = Math.max(0, context.contentHeight - context.viewportHeight);
  if (context.offset <= TOP_REVEAL_OFFSET) {
    return "expanded";
  }
  const minimumCollapseOffset = Math.max(
    TOP_REVEAL_OFFSET + 1,
    context.headerHeight ?? 0,
  );
  if (current === "expanded" && context.offset < minimumCollapseOffset) {
    return "expanded";
  }
  if (maxOffset <= MIN_SCROLLABLE_OVERFLOW) {
    return "expanded";
  }
  if (context.offset < 0 || context.offset > maxOffset) {
    return "expanded";
  }
  if (context.delta > COLLAPSE_SCROLL_DELTA) {
    return "collapsed";
  }
  if (context.delta < -REVEAL_SCROLL_DELTA) {
    return "expanded";
  }
  return current;
}

/** A restored deep list starts collapsed; the top starts expanded. */
export function initialToolbarVisibility(savedOffset: number): ToolbarVisibility {
  return savedOffset > TOP_REVEAL_OFFSET ? "collapsed" : "expanded";
}

export function toolbarLayout(input: {
  readonly collapsed: boolean;
  readonly compact: boolean;
}): ToolbarLayout {
  return {
    showSlimRow: input.compact || input.collapsed,
    showFullToolbar: !input.collapsed,
    spacerHeight: 0,
  };
}

/**
 * On wide layouts the slim row appears only while collapsed, so the net header
 * height removed/added is the full toolbar minus the slim row. On compact the
 * slim row is always present and the full toolbar height is the net change.
 */
export function toolbarCompensationHeight(input: {
  readonly fullHeight: number;
  readonly slimHeight: number;
  readonly compact: boolean;
}): number {
  return Math.max(
    0,
    input.fullHeight - (input.compact ? 0 : input.slimHeight),
  );
}

export type ToolbarScrollCompensationInput = {
  readonly currentOffset: number;
  readonly headerHeight: number;
  readonly direction: "collapse" | "reveal";
  readonly maxOffset: number;
};

/**
 * Collapsing removes a header above the list; scroll up by the same height so
 * the visible row does not move. Revealing deep in the list scrolls down by the
 * same height. At the top, revealing leaves the offset at the top.
 */
export function toolbarScrollCompensation(
  input: ToolbarScrollCompensationInput,
): number {
  if (!Number.isFinite(input.currentOffset) || input.headerHeight <= 0) {
    return Number.isFinite(input.currentOffset) ? input.currentOffset : 0;
  }
  if (input.direction === "collapse") {
    return Math.max(0, input.currentOffset - input.headerHeight);
  }
  if (input.currentOffset <= TOP_REVEAL_OFFSET) {
    return input.currentOffset;
  }
  const target = input.currentOffset + input.headerHeight;
  if (Number.isFinite(input.maxOffset) && input.maxOffset > 0) {
    return Math.min(input.maxOffset, target);
  }
  return target;
}
