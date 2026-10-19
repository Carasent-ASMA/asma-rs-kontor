import { createPaseoClient } from "@getpaseo/client";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const port = process.argv[2];
const home = process.argv[3];
if (!port || !home) {
  throw new Error("Usage: node scripts/navigator-probe.mjs <daemon-port> <daemon-home>");
}

const provider = "opencode/deepseek/deepseek-flash";
const repo = mkdtempSync(join(tmpdir(), "paseo-kontor-navigator-"));
execFileSync("git", ["init", "-q", repo]);
writeFileSync(join(repo, "README.md"), "navigator probe\n");
execFileSync("git", ["-C", repo, "add", "README.md"]);
execFileSync(
  "git",
  ["-C", repo, "-c", "user.name=probe", "-c", "user.email=probe@example.com", "commit", "-qm", "init"],
);

const client = createPaseoClient({
  url: `ws://127.0.0.1:${port}/ws`,
  appVersion: "0.11.4",
});

function waitFor(predicate, timeoutMs = 8_000) {
  const started = Date.now();
  return new Promise((resolve, reject) => {
    const tick = () => {
      if (predicate()) {
        resolve();
        return;
      }
      if (Date.now() - started >= timeoutMs) {
        reject(new Error("Timed out waiting for the native session update"));
        return;
      }
      setTimeout(tick, 100);
    };
    tick();
  });
}

let handle = null;
let subscription = null;
const report = { checks: [] };
try {
  await client.connect();
  handle = await client.agents.create({
    config: { provider },
    cwd: repo,
    title: `navigator-probe-${Date.now()}`,
  });
  assert.ok(handle.id, "the probe agent must carry a native id");

  const subscribed = await client.agents.list({
    filter: { includeArchived: false },
    sort: [{ key: "updated_at", direction: "desc" }],
    page: { limit: 1 },
    subscribe: {},
  });
  subscription = subscribed.subscription;
  assert.ok(subscribed.subscriptionId, "the native directory must return a subscription id");
  const updates = [];
  subscription.subscribe({
    snapshot: () => undefined,
    update: (message) => {
      if (message.type === "agent_update") {
        updates.push(message.payload);
      }
    },
  });
  report.checks.push({
    name: "native session subscription",
    outcome: "subscribed",
    subscriptionId: subscribed.subscriptionId,
    firstPageEntries: subscribed.entries.length,
  });

  let cursor;
  let pages = 0;
  const ids = [];
  do {
    const page = await client.agents.list({
      filter: { includeArchived: false },
      sort: [{ key: "updated_at", direction: "desc" }],
      page: { limit: 1, ...(cursor ? { cursor } : {}) },
    });
    pages += 1;
    ids.push(...page.entries.map((item) => item.agent.id));
    cursor = page.pageInfo.hasMore ? page.pageInfo.nextCursor : undefined;
  } while (cursor);
  assert.ok(ids.includes(handle.id), "the complete page walk must include the probe agent");
  assert.equal(new Set(ids).size, ids.length, "the complete page walk must not duplicate a row");
  report.checks.push({
    name: "complete page walk",
    outcome: "complete",
    pages,
    rows: ids.length,
    limit: 1,
  });

  const updatedTitle = `navigator-probe-updated-${Date.now()}`;
  execFileSync(
    "paseo",
    ["agent", "update", handle.id, "--name", updatedTitle, "--home", home],
    { stdio: "ignore" },
  );
  await waitFor(() =>
    updates.some(
      (update) =>
        update.kind === "upsert" &&
        update.agent.id === handle.id &&
        update.agent.title === updatedTitle,
    ),
  );
  report.checks.push({
    name: "native state update",
    outcome: "observed",
    updateKinds: [...new Set(updates.map((update) => update.kind))],
  });
} finally {
  if (subscription) {
    await subscription.release().catch(() => undefined);
  }
  if (handle) {
    await handle.archive().catch(() => undefined);
  }
  await client.close().catch(() => undefined);
  rmSync(repo, { recursive: true, force: true });
}

console.log(JSON.stringify(report, null, 2));
