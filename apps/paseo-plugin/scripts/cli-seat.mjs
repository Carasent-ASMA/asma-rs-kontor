import { createPaseoClient } from "@getpaseo/client";

const port = process.env.PASEO_CLOSURE_PORT ?? process.argv[2];
const repo = process.argv[3];
if (!port || !repo) {
  throw new Error("Usage: node scripts/cli-seat.mjs <daemon-port> <git-repo>");
}

const provider = process.env.PASEO_CLOSURE_PROVIDER ?? "opencode/deepseek/deepseek-flash";
const client = createPaseoClient({
  url: `ws://127.0.0.1:${port}/ws`,
  appVersion: "0.11.2",
});

await client.connect();
try {
  const workspace = await client.workspaces.create({
    source: { kind: "directory", path: repo },
  });
  const agent = await workspace.agents.create({
    config: { provider },
    labels: {
      "kontor.seat_binding_id": "seat-cli-closure",
      "kontor.hosted_seat": "true",
      "kontor.workspace_id": workspace.id,
      "kontor.worktree": repo,
    },
  });
  console.log(JSON.stringify({ agentId: agent.id, workspaceId: workspace.id }));
} finally {
  await client.close();
}
