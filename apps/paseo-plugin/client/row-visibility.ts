import { useCallback, useRef, useState } from "react";

/** Window bounds of one gated row, and of its scroll body, as measureInWindow reports them. */
export interface WindowBounds { top: number; bottom: number }

/** A view whose window position can be measured; react-native's View satisfies this on every platform. */
export interface MeasurableView {
  measureInWindow?(callback: (x: number, y: number, width: number, height: number) => void): void;
}

/** Motion gate decision: a row animates only while its window bounds intersect the scroll body's. */
export function rowIntersectsBody(row: WindowBounds, body: WindowBounds): boolean {
  return row.bottom > body.top && row.top < body.bottom;
}

/**
 * Native scroll bodies give marks no IntersectionObserver, so each row's measured window bounds decide
 * whether its mark may animate. Rows that have not measured (or whose node is gone) stay static until they
 * do: motion is a progressive enhancement, and a static rainbow never claims more than the row shows.
 */
export function visibleRowIds(rows: ReadonlyMap<string, WindowBounds | undefined>, body: WindowBounds | undefined): Set<string> {
  const next = new Set<string>();
  if (!body) return next;
  for (const [id, row] of rows) if (row && rowIntersectsBody(row, body)) next.add(id);
  return next;
}

/** Shared gate for one scroll body: register row nodes, then measure the body and its rows against the window. */
export function useRowVisibility() {
  const rows = useRef(new Map<string, MeasurableView | null>());
  const [visible, setVisible] = useState<ReadonlySet<string>>(() => new Set());
  const evaluating = useRef(false);
  const dirty = useRef(false);
  const apply = useCallback((measured: ReadonlyMap<string, WindowBounds | undefined>, body: WindowBounds | undefined) => {
    const next = visibleRowIds(measured, body);
    setVisible((previous) => previous.size === next.size && [...next].every((id) => previous.has(id)) ? previous : next);
  }, []);
  /** Measures the body and every registered row; overlapping calls coalesce into one trailing pass. */
  const measure = useCallback((bodyNode: MeasurableView | null) => {
    if (!bodyNode || typeof bodyNode.measureInWindow !== "function") return;
    if (evaluating.current) { dirty.current = true; return; }
    evaluating.current = true;
    const settle = (measured: ReadonlyMap<string, WindowBounds | undefined>, body: WindowBounds | undefined) => {
      evaluating.current = false;
      apply(measured, body);
      if (dirty.current) { dirty.current = false; measure(bodyNode); }
    };
    bodyNode.measureInWindow((_, top, __, height) => {
      const body = height > 0 ? { top, bottom: top + height } : undefined;
      const registered = [...rows.current.entries()];
      const measured = new Map<string, WindowBounds | undefined>();
      if (!registered.length) { settle(measured, body); return; }
      let pending = registered.length;
      const done = () => { if (--pending === 0) settle(measured, body); };
      for (const [id, node] of registered) {
        if (!node || typeof node.measureInWindow !== "function") { measured.set(id, undefined); done(); continue; }
        node.measureInWindow((_, y, __, rowHeight) => { measured.set(id, { top: y, bottom: y + rowHeight }); done(); });
      }
    });
  }, [apply]);
  const register = useCallback((id: string, node: MeasurableView | null) => {
    rows.current.set(id, node);
  }, []);
  const forget = useCallback((id: string) => {
    rows.current.delete(id);
  }, []);
  return { visible, measure, register, forget };
}
