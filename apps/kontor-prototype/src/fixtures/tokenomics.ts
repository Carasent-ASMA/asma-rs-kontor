import { tokenomicsFixture, tokenomicsLiveFixture } from "../../../paseo-plugin/test/fixtures/tokenomics";
import { tokenomicsQuerySchema } from "../../../paseo-plugin/shared/tokenomics";
import { buildTokenomics, paginateTokenomics, pivotRows, tokenomicsCsv, tokenomicsOptions, usageModel } from "../../../paseo-plugin/shared/tokenomics-model";

export function tokenomicsRpc(method: string, input: unknown) {
  const data = tokenomicsFixture(), live = tokenomicsLiveFixture(), query = tokenomicsQuerySchema.parse(input);
  const result = buildTokenomics(data, live, query);
  if (method === "tokenomics.export") return { filename: `tokenomics-${query.page}.csv`, content: tokenomicsCsv(result), revision: "synthetic-tokenomics-1" };
  const model = usageModel(data, query), options = tokenomicsOptions(data, model);
  if (query.page === "pivot") options.pivotNodes = pivotRows(model, query).filter((r) => r.hasChildren).map((r) => ({ value: r.key, label: r.key }));
  return { status: "ready", readAt: "2026-11-03T13:00:00Z", generated: data.generated, revision: "synthetic-tokenomics-1", diagnostics: ["Synthetic fixture snapshot; no collection or live source is read."], query,
    options, tiles: result.tiles, tables: paginateTokenomics(result, query), notes: result.notes };
}
