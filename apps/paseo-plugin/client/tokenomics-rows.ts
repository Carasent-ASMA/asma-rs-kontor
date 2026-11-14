import { useInfiniteQuery } from "@tanstack/react-query";
import { useRpc } from "@getpaseo/plugin/client";
import { readTokenomics, type TokenomicsQuery, type TokenomicsTable, type TokenomicsView } from "../shared/tokenomics";

export function tokenomicsRowChunk(view: TokenomicsView, tableId: string, revision: string | null, offset: number): TokenomicsTable {
  if (view.revision !== revision || view.status === "unavailable") throw new Error("Report changed or became unavailable. Refresh before continuing.");
  const table = view.tables.find((t) => t.id === tableId);
  if (!table || offset < table.total && !table.rows.length) throw new Error("The requested rows are unavailable. Retry or refresh; no rows were skipped.");
  return { ...table, plot: undefined };
}

/** Transport stays bounded; the table scrolls through successive validated projections. */
export function useTokenomicsRows(table: TokenomicsTable, query: TokenomicsQuery, revision: string | null, hostId: string) {
  const read = useRpc(readTokenomics);
  return useInfiniteQuery({
    queryKey: ["tokenomics-rows", hostId, revision, query, table.id],
    initialPageParam: 0,
    initialData: { pages: [table], pageParams: [0] },
    enabled: false,
    retry: false,
    queryFn: async ({ pageParam }) => tokenomicsRowChunk(await read({ ...query, offset: 0, offsets: { ...query.offsets, [table.id]: pageParam } }), table.id, revision, pageParam),
    getNextPageParam: (last, _pages, offset) => offset + last.rows.length < last.total ? offset + last.rows.length : undefined,
  });
}
