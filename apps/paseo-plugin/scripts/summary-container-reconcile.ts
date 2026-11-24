// Read-only dry run over the legacy per-job summary containers: prints each with its proof and the supported closeout
// command. It runs no command and changes nothing; the closeout is a separate gated step.
// bun scripts/summary-container-reconcile.ts <daemon-port> [summary-state-root] > reconcile.json
import { createPaseoClient } from "@getpaseo/client";
import { summaryStateRoot } from "../server/session-summaries";
import { reconcileSummaryContainers } from "../server/summary-container-reconcile";

const [port, root = summaryStateRoot()] = process.argv.slice(2);
if (!port) throw new Error("Usage: bun scripts/summary-container-reconcile.ts <daemon-port> [summary-state-root]");
const host = `127.0.0.1:${port}`;
const paseo = createPaseoClient({ url: `ws://${host}/ws`, clientId: "kontor-summary-reconcile-dry-run", reconnect: { enabled: false } });
try {
  await paseo.connect();
  process.stdout.write(JSON.stringify(await reconcileSummaryContainers(paseo, root, host), null, 2) + "\n");
} finally { await paseo.close(); }
