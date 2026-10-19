// Explicit offline field export only. Does not collect data, contact Jira, or write files.
// bun scripts/export-jira-history.ts <consumption-data.json> ASMA-8539 [ASMA-8371 ...]
import { createHash } from "node:crypto";
import { open, realpath } from "node:fs/promises";
import { projectJiraHistory } from "../server/jira-history";
const [inputPath, ...epics] = process.argv.slice(2);
if (!inputPath || !epics.length) throw new Error("Provide a historical consumption/data@2 file and explicit ASMA epic keys");
const path = await realpath(inputPath);
const handle = await open(path, "r");
try {
  const limit = 16 * 1024 * 1024;
  const info = await handle.stat();
  if (!info.isFile() || info.size > limit) throw new Error("Historical export input must be a regular file of at most 16 MiB");
  const buffer = Buffer.alloc(limit + 1);
  const { bytesRead } = await handle.read(buffer, 0, buffer.length, 0);
  if (bytesRead > limit) throw new Error("Historical export input exceeds 16 MiB");
  const bytes = buffer.subarray(0, bytesRead);
  const projection = projectJiraHistory(JSON.parse(bytes.toString("utf8")), epics, { path, sha256: createHash("sha256").update(bytes).digest("hex") });
  const output = JSON.stringify(projection) + "\n";
  if (Buffer.byteLength(output) > 2 * 1024 * 1024) throw new Error("Select fewer epics: projection exceeds the plugin 2 MiB source cap");
  process.stdout.write(output);
} finally { await handle.close(); }
