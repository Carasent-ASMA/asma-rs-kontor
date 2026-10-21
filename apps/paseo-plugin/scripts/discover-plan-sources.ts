// Offline candidate only; the integrator owns catalog installation.
import { readFile } from "node:fs/promises";
import { discoverPlanCatalog } from "../server/plan-discovery";
import { readWorkstreamRegistry, sourceCatalog } from "../server/read-sources";
const args = process.argv.slice(2);
const roots: string[] = [];
let catalogOnly = false, existingPath: string | undefined;
for (let i = 0; i < args.length; i++) {
  const arg = args[i]!;
  if (arg === "--catalog") catalogOnly = true;
  else if (arg === "--existing" && args[i + 1]) existingPath = args[++i];
  else if (arg.startsWith("--")) throw new Error("Usage: discover-plan-sources.ts <checkout> [<checkout>...] [--existing <catalog>] [--catalog]");
  else roots.push(arg);
}
const existing = existingPath ? sourceCatalog(await readFile(existingPath, "utf8")) : { sources: [], diagnostics: [] };
if (existing.diagnostics.length) throw new Error(existing.diagnostics.join(" "));
const report = await discoverPlanCatalog(roots, existing.sources);
const registry = await readWorkstreamRegistry(roots[0]);
process.stdout.write(JSON.stringify(catalogOnly ? report.catalog : { ...report, registry }, null, 2) + "\n");
for (const diagnostic of report.diagnostics) process.stderr.write(diagnostic + "\n");
if (report.reports.some((row) => row.coverage.some((source) => source.status === "invalid"))) process.exitCode = 1;
