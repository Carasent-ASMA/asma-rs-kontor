// Read-only dry run over the legacy per-job summary containers: prints each with its proof and the supported closeout
// command. It runs no command and changes nothing; the closeout is a separate gated step.
// bun scripts/summary-container-reconcile.ts <daemon-port> [summary-state-root] > reconcile.json
// The record-recovery mode records one operator recovery disposition on a held job: a dry run unless --apply.
// bun scripts/summary-container-reconcile.ts <daemon-port> <summary-state-root> record-recovery <job-file> retire-legacy-create <evidence> [--apply <before-sha256>]
import { createPaseoClient } from "@getpaseo/client";
import { summaryStateRoot } from "../server/session-summaries";
import { RECONCILE_USAGE, reconcileEndpoint, reconcileSummaryContainers, recordSummaryRecovery, recoveryOperands } from "../server/summary-container-reconcile";

const [port, explicitRoot, mode, job, disposition, evidence, flag, beforeSha256, ...extra] = process.argv.slice(2);
const { url, host } = reconcileEndpoint(port);
const recording = mode === "record-recovery";
const applying = flag === "--apply" && /^[a-f0-9]{64}$/.test(beforeSha256 ?? "");
if ((mode !== undefined && !recording) || (recording && (!explicitRoot || (flag !== undefined && !applying))) || (!applying && beforeSha256 !== undefined) || extra.length) throw new Error(RECONCILE_USAGE);
// Every refusal that needs no host happens before the client exists.
const operands = recording ? recoveryOperands({ job, disposition, evidence }) : null;
const root = explicitRoot ?? summaryStateRoot();
const paseo = createPaseoClient({ url, clientId: recording ? "kontor-summary-record-recovery" : "kontor-summary-reconcile-dry-run", reconnect: { enabled: false } });
try {
  await paseo.connect();
  const result = recording
    ? await recordSummaryRecovery(paseo, root, { ...operands!, ...(applying ? { applySha256: beforeSha256 } : {}) })
    : await reconcileSummaryContainers(paseo, root, host);
  process.stdout.write(JSON.stringify(result, null, 2) + "\n");
} finally { await paseo.close(); }
