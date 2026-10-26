import { useEffect, useState } from "react";
import { useRpc } from "@getpaseo/plugin/client";
import { readNavigatorCapabilities, readUsageTotals, UNSUPPORTED, type NavigatorCapabilities } from "../shared/session-navigator";
import type { SessionTotals } from "./session-row-format";

const failure = (error: unknown) => error instanceof Error ? error.message : String(error);
const NONE: ReadonlyMap<string, SessionTotals> = new Map();
/** The most IDs one totals read may name. */
const TOTALS_BATCH = 200;

/**
 * What this host's server says the navigator can do; null while reading. A failed read leaves every capability
 * unsupported with that failure as the reason, so no control offers what nobody confirmed.
 */
export function useNavigatorCapabilities(hostId: string): NavigatorCapabilities | null {
  const read = useRpc(readNavigatorCapabilities);
  const [state, setState] = useState<{ hostId: string; value: NavigatorCapabilities } | null>(null);
  useEffect(() => {
    let live = true;
    read({}).then((value) => { if (live) setState({ hostId, value }); }, (error: unknown) => {
      if (!live) return;
      const off = { supported: false as const, reason: `Kontor could not read what this host supports (${failure(error)}).` };
      setState({ hostId, value: Object.fromEntries(Object.keys(UNSUPPORTED).map((key) => [key, off])) as NavigatorCapabilities });
    });
    return () => { live = false; };
  }, [hostId]);
  return state?.hostId === hostId ? state.value : null;
}

/**
 * Whole-session totals for `ids` where the host reports them. A session without a report, a host without the
 * capability and a failed read all leave the row's totals unknown; a latest-usage report never stands in.
 */
export function useUsageTotals(supported: boolean, ids: readonly string[]): ReadonlyMap<string, SessionTotals> {
  const read = useRpc(readUsageTotals);
  const key = supported ? [...new Set(ids)].sort().join("\n") : "";
  const [state, setState] = useState<{ key: string; totals: ReadonlyMap<string, SessionTotals> } | null>(null);
  useEffect(() => {
    if (!key) return;
    let live = true;
    const all = key.split("\n"), batches = Array.from({ length: Math.ceil(all.length / TOTALS_BATCH) }, (_, index) => all.slice(index * TOTALS_BATCH, (index + 1) * TOTALS_BATCH));
    Promise.all(batches.map((agentIds) => read({ agentIds }))).then((answers) => {
      if (live) setState({ key, totals: new Map(answers.flatMap((answer) => Object.entries(answer).flatMap(([id, value]) => value ? [[id, value] as const] : []))) });
    }, () => { if (live) setState({ key, totals: NONE }); });
    return () => { live = false; };
  }, [key]);
  return state?.key === key ? state.totals : NONE;
}
