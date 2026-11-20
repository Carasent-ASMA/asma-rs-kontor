import { tokenomicsFixture, tokenomicsLiveFixture } from "../../../paseo-plugin/test/fixtures/tokenomics";
import { tokenomicsQuerySchema } from "../../../paseo-plugin/shared/tokenomics";
import { buildTokenomics, paginateTokenomics, pivotRows, tokenomicsCsv, tokenomicsOptions, tokenomicsNotices, tokenomicsSummary, usageModel } from "../../../paseo-plugin/shared/tokenomics-model";

export function tokenomicsRpc(method: string, input: unknown, manyRows: boolean) {
  const data = tokenomicsFixture(), live = tokenomicsLiveFixture(), query = tokenomicsQuerySchema.parse(input);
  if (manyRows) live.delivery!.prs = Array.from({ length: 73 }, (_, i) => ({ ...live.delivery!.prs[0]!, ref: `demo-repo#${i + 1}` }));
  if (query.compare === "previous") query.compare = data.revs.at(-2)?.id ?? null;
  if (query.rev === null) query.rev = data.revs.at(-1)?.id ?? null;
  const revision = manyRows ? "synthetic-tokenomics-scroll-1" : "synthetic-tokenomics-1";
  const result = buildTokenomics(data, live, query);
  if (method === "tokenomics.export") return { filename: `tokenomics-${query.page}.csv`, content: tokenomicsCsv(result), revision };
  const model = usageModel(data, query), options = tokenomicsOptions(data, model);
  if (query.page === "pivot") options.pivotNodes = pivotRows(model, query).filter((r) => r.hasChildren).map((r) => ({ value: r.key, label: r.key }));
  return { status: "ready", readAt: "2026-11-03T13:00:00Z", generated: data.generated, revision, diagnostics: ["Synthetic fixture snapshot; no collection or live source is read."], query,
    options, notices: tokenomicsNotices(data, live, query, model, false), summary: tokenomicsSummary(model), tiles: result.tiles, tables: paginateTokenomics(result, query), notes: result.notes };
}
