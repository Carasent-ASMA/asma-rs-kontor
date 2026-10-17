import { createPaseoClient } from "@getpaseo/client";
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

const port = process.env.PASEO_CLOSURE_PORT ?? process.argv[2];
const expected = process.env.PASEO_CLOSURE_EXPECT ?? process.argv[3] ?? "guarded";
if (!port) {
  throw new Error("Usage: node scripts/closure.mjs <daemon-port> [guarded|unguarded]");
}
if (expected !== "guarded" && expected !== "unguarded") {
  throw new Error(`Unknown expectation: ${expected}`);
}

const provider = "opencode/deepseek/deepseek-flash";
const repo = mkdtempSync(join(tmpdir(), "paseo-kontor-closure-"));
execFileSync("git", ["init", "-q", repo]);
writeFileSync(join(repo, "README.md"), "closure\n");
execFileSync("git", ["-C", repo, "add", "README.md"]);
execFileSync(
  "git",
  ["-C", repo, "-c", "user.name=closure", "-c", "user.email=closure@example.com", "commit", "-qm", "init"],
);

const client = createPaseoClient({
  url: `ws://127.0.0.1:${port}/ws`,
  appVersion: "0.11.2",
});

function labelledAgent(labels) {
  return {
    config: { provider, cwd: repo },
    cwd: repo,
    labels,
  };
}

async function expectRefusal(label, promise) {
  try {
    await promise;
  } catch (error) {
    const message = error instanceof Error ? error.message : String(error);
    assert.match(message, /Kontor workspace guard refused/, `${label}: ${message}`);
    return message;
  }
  assert.fail(`${label}: the workspace creation was not refused`);
}

async function expectSuccess(label, promise) {
  const handle = await promise;
  assert.ok(handle?.id, `${label}: no workspace returned`);
  await handle.archive();
  return handle.id;
}

const report = { expected, checks: [], repo };

await client.connect();
try {
  const baseHandle = await client.workspaces.create({
    source: { kind: "directory", path: repo },
  });
  const projectId = baseHandle.projectId;
  assert.ok(projectId, "the base workspace must carry a project id");
  const base = baseHandle.id;
  report.checks.push({ name: "interactive directory create", outcome: "created", workspaceId: base });

  const bindingLabels = {
    "kontor.seat": "seat-closure",
    "kontor.workspace_id": base,
    "kontor.worktree": repo,
  };

  const labelledWorktree = () =>
    client.workspaces.create({
      agent: labelledAgent(bindingLabels),
      source: { kind: "worktree", cwd: repo, projectId },
    });

  const labelledNoProject = () =>
    client.workspaces.create({
      agent: labelledAgent(bindingLabels),
      source: { kind: "directory", path: repo },
    });

  if (expected === "guarded") {
    report.checks.push({
      name: "labelled worktree create",
      outcome: "refused",
      message: await expectRefusal("labelled worktree create", labelledWorktree()),
    });
    report.checks.push({
      name: "labelled directory without projectId",
      outcome: "refused",
      message: await expectRefusal("labelled directory without projectId", labelledNoProject()),
    });
  } else {
    const created = await expectSuccess("labelled worktree create", labelledWorktree());
    report.checks.push({
      name: "labelled worktree create",
      outcome: "created",
      workspaceId: created,
    });
  }

  const plainWorktree = await expectSuccess(
    "interactive worktree create",
    client.workspaces.create({
      source: { kind: "worktree", cwd: repo, projectId, worktreeSlug: `closure-${Date.now()}` },
    }),
  );
  report.checks.push({
    name: "interactive worktree create",
    outcome: "created",
    workspaceId: plainWorktree,
  });

  await baseHandle.archive();
} finally {
  await client.close();
  rmSync(repo, { recursive: true, force: true });
}

console.log(JSON.stringify(report, null, 2));
