// Offline, read-only discovery. Output is a candidate, never an installed catalog.
// bun scripts/discover-plan-sources.ts <documentation-checkout> [--catalog]
import { discoverPlanSources } from "../server/plan-discovery";
const [checkout, option, ...extra] = process.argv.slice(2);
if (!checkout || (option && option !== "--catalog") || extra.length) throw new Error("Usage: discover-plan-sources.ts <documentation-checkout> [--catalog]");
const report = await discoverPlanSources(checkout);
process.stdout.write(JSON.stringify(option === "--catalog" ? report.catalog : report, null, 2) + "\n");
for (const diagnostic of report.diagnostics) process.stderr.write(diagnostic + "\n");
if (report.coverage.some((row) => row.status === "invalid")) {
  process.stderr.write("Discovery includes unreadable or invalid sources; inspect the coverage report before installation.\n");
  process.exitCode = 1;
}
