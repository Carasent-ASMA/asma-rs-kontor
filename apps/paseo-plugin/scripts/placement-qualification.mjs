// Placement qualification on a disposable Paseo daemon. Never uses ~/.paseo or port 6767, and runs only inside the
// no-live-provider envelope below. It starts a daemon in a temporary home, installs this plugin there with floor.json
// off, creates synthetic desk and epic projects and agents (no prompt, so no turn), and runs each creation route with
// placement.json absent (before), on (after) and off (rollback). With placement on, it also drives the stock summary helper's
// creation call twice (slice A absent): each job must still be created, and the project and workspace it adds are
// recorded as the open residual ASMA-8580-summary-placement-pending, not as correct placement. Every agent and
// workspace it made is archived and read back, then the daemon is stopped and the home and fixtures are removed.
//   node scripts/placement-qualification.mjs <report.json> [--paseo <cli>] [--envelope-only]
// `--paseo` names the Paseo CLI (default: the first `paseo` on PATH); `--envelope-only` checks the envelope and stops.
import { createPaseoClient } from "@getpaseo/client";
import assert from "node:assert/strict";
import { execFileSync, spawnSync } from "node:child_process";
import { createHash, randomUUID } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { connect, createServer } from "node:net";
import { tmpdir, userInfo } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const [out, ...flags] = process.argv.slice(2);
assert.ok(out, "Usage: node scripts/placement-qualification.mjs <report.json> [--paseo <cli>] [--envelope-only]");
const option = (name) => (flags.includes(name) ? flags[flags.indexOf(name) + 1] : undefined);
const envelopeOnly = flags.includes("--envelope-only");

// The no-live-provider envelope (AUD-B-R5-02). The script runs itself again under sandbox-exec with an explicit
// environment and nothing else from the caller's: an isolated OS HOME, XDG roots and TMPDIR in a new directory, and
// a PATH of stubs before the system directories only. A provider stub answers `--version` and refuses anything else;
// `security` (the Keychain reader in Paseo 0.17's bundled claude-usage-source) is always refused; each call is logged.
// Provider config roots (CLAUDE_CONFIG_DIR, CODEX_HOME, ...) and executable overrides (PI_COMMAND, OMP_COMMAND) stay
// unset, so they fall back under the isolated HOME and PATH, and the isolated daemon config holds no provider
// override. The sandbox refuses every connection that is not loopback, which covers in-process HTTPS as well. The
// inner run checks all of this before it starts anything, and refuses otherwise.
const STUBS = ["claude", "codex", "copilot", "cursor-agent", "opencode", "pi", "omp", "agy", "fx", "goose", "hermes", "hoplite", "muse"];
const DENIED = ["security"];
const SYSTEM_PATH = "/usr/bin:/bin:/usr/sbin:/sbin";
const ALLOWED_ENV = ["HOME", "PATH", "TMPDIR", "XDG_CONFIG_HOME", "XDG_CACHE_HOME", "XDG_DATA_HOME", "XDG_STATE_HOME", "KONTOR_PLACEMENT_ENVELOPE", "__CF_USER_TEXT_ENCODING"];
const boxEnv = (box) => ({
  HOME: join(box, "home"), PATH: `${join(box, "bin")}:${SYSTEM_PATH}`, TMPDIR: join(box, "tmp"),
  XDG_CONFIG_HOME: join(box, "xdg", "config"), XDG_CACHE_HOME: join(box, "xdg", "cache"), XDG_DATA_HOME: join(box, "xdg", "data"), XDG_STATE_HOME: join(box, "xdg", "state"),
  KONTOR_PLACEMENT_ENVELOPE: box,
});
const sandbox = (box) => `(version 1)(allow default)(deny network-outbound)(allow network-outbound (remote ip "localhost:*"))(allow network-outbound (remote unix-socket))(deny file-write* (literal "${join(box, "sandbox-witness")}"))`;
const stub = (name, log, answers) => `#!/bin/sh
# Kontor placement qualification stub: no ${name} code runs in the envelope.
if [ "$#" -eq 1 ] && [ "$1" = "--version" ] && [ ${answers ? 1 : 0} -eq 1 ]; then
  echo '{"tool":"${name}","op":"version"}' >> '${log}'; echo "99.0.0 (Kontor qualification stub)"; exit 0
fi
echo '{"tool":"${name}","op":"denied","argc":'"$#"'}' >> '${log}'; echo "${name} is disabled in the Kontor qualification envelope" >&2; exit 86
`;

const box = process.env.KONTOR_PLACEMENT_ENVELOPE;
if (!box) {
  if (process.platform !== "darwin" || !existsSync("/usr/bin/sandbox-exec")) {
    console.error("Kontor placement qualification refused: the no-live-provider envelope needs macOS sandbox-exec.");
    process.exit(3);
  }
  const made = realpathSync(mkdtempSync(join(tmpdir(), "kontor-placement-envelope-")));
  for (const dir of ["home", "tmp", "bin", "xdg/config", "xdg/cache", "xdg/data", "xdg/state"]) mkdirSync(join(made, dir), { recursive: true, mode: 0o700 });
  for (const name of [...STUBS, ...DENIED]) writeFileSync(join(made, "bin", name), stub(name, join(made, "provider-invocations.jsonl"), STUBS.includes(name)), { mode: 0o700 });
  const cli = option("--paseo") ?? spawnSync("/usr/bin/which", ["paseo"], { env: { PATH: process.env.PATH ?? "" }, encoding: "utf8" }).stdout.trim();
  const inner = spawnSync("/usr/bin/sandbox-exec", ["-p", sandbox(made), process.execPath, fileURLToPath(import.meta.url), resolve(out), ...(cli ? ["--paseo", realpathSync(cli)] : []), ...(envelopeOnly ? ["--envelope-only"] : [])], { env: boxEnv(made), stdio: "inherit" });
  rmSync(made, { recursive: true, force: true });
  process.exit(inner.status ?? 1);
}
{
  const faults = Object.keys(process.env).filter((key) => !ALLOWED_ENV.includes(key)).map((key) => `${key} is outside the allow-list`);
  for (const [key, value] of Object.entries(boxEnv(box))) if (process.env[key] !== value) faults.push(`${key} is not the envelope's`);
  if (process.env.HOME === userInfo().homedir) faults.push("HOME is the account home");
  for (const name of [...STUBS, ...DENIED]) {
    const found = spawnSync("/usr/bin/which", ["-a", name], { encoding: "utf8" }).stdout.split("\n").filter(Boolean);
    if (found[0] !== join(box, "bin", name) || (STUBS.includes(name) && found.length !== 1)) faults.push(`${name} resolves to ${found.join(", ") || "nothing"}`);
  }
  try { writeFileSync(join(box, "sandbox-witness"), ""); faults.push("the sandbox is not applied"); } catch (error) { if (error.code !== "EPERM") faults.push(`sandbox witness: ${error.code}`); }
  // 192.0.2.1 is a documentation address (RFC 5737): outside the sandbox the probe goes nowhere.
  const network = await new Promise((done) => { const s = connect(9, "192.0.2.1").on("error", (e) => done(e.code)).on("connect", () => (s.destroy(), done("connected"))); setTimeout(() => (s.destroy(), done("timeout")), 3000); });
  if (network !== "EPERM") faults.push(`an outside connection was not refused (${network})`);
  if (faults.length) {
    console.error(`Kontor placement qualification refused: the no-live-provider envelope is not in place: ${faults.join("; ")}`);
    process.exit(3);
  }
  if (envelopeOnly) {
    console.log(JSON.stringify({ envelope: "in place", stubs: STUBS, denied: DENIED, path: process.env.PATH, env: Object.keys(process.env).sort() }));
    process.exit(0);
  }
}
const cli = option("--paseo");
assert.ok(cli, "no Paseo CLI: pass --paseo <cli>");
const app = join(dirname(cli), "..", "app.asar");
const pluginDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const home = mkdtempSync(join(tmpdir(), "kontor-placement-home-"));
const root = mkdtempSync(join(tmpdir(), "kontor-placement-fixtures-"));
const port = await new Promise((done) => { const s = createServer().listen(0, "127.0.0.1", () => { const p = s.address().port; s.close(() => done(p)); }); });
assert.notEqual(port, 6767);
const switches = join(home, "plugin-config", "kontor");
// Children get the envelope's environment only, so no live caller identity leaks into a case; a case adds its own.
const paseo = (args, env = {}) => {
  const run = spawnSync(cli, [...args, "--home", home], { env: { ...process.env, ...env }, encoding: "utf8" });
  return { status: run.status, stdout: run.stdout.trim(), stderr: run.stderr.trim() };
};
const git = (...args) => spawnSync("git", ["-C", pluginDir, ...args], { encoding: "utf8" }).stdout.trim();
const report = {
  paseo: null, paseoCli: cli, paseoAppAsarSha256: existsSync(app) ? createHash("sha256").update(readFileSync(app)).digest("hex") : null, port,
  candidate: { head: git("rev-parse", "HEAD"), base: git("merge-base", "HEAD", "origin/master"), dirty: git("status", "--porcelain", "--", ".").split("\n").filter(Boolean) },
  envelope: { stubs: STUBS, denied: DENIED, path: process.env.PATH, env: Object.keys(process.env).sort(), network: "loopback only (sandbox-exec)", invocations: null, configAgents: null },
  installed: null, checks: [], detections: [], summaryResidual: [], modes: [], cleanup: null,
};
const made = { workspaces: new Set(), agents: new Set() };
let client;

const placement = (value) => value === null ? rmSync(join(switches, "placement.json"), { force: true }) : writeFileSync(join(switches, "placement.json"), JSON.stringify({ placement: value }));
const pluginLines = () => {
  const logs = paseo(["plugin", "logs", "kontor", "--json"]);
  const text = `${logs.stdout}\n${logs.stderr}`;
  return [...`${text}\n${text.replace(/\\"/g, '"')}`.matchAll(/\{[^{}]*"event":"kontor\.(?:floor|placement)\.[a-z]+"[^{}]*\}/g)].map((m) => JSON.parse(m[0]));
};
function check(name, route, expected, outcome, detail) {
  report.checks.push({ name, route, expected, outcome, ...detail });
  assert.equal(outcome, expected, `${name}: ${JSON.stringify(detail)}`);
}
/** `paseo workspace create` from a caller's seat environment, or with no caller. */
function cliCreate(name, expected, caller, args, isolation = "local") {
  const env = caller ? { PASEO_AGENT_ID: caller.id, PASEO_WORKSPACE_ID: caller.workspaceId, PASEO_AGENT_CWD: caller.cwd } : {};
  const run = paseo(["workspace", "create", "--isolation", isolation, "--json", ...args], env);
  const id = run.status === 0 ? JSON.parse(run.stdout).workspaceId : null;
  if (id) made.workspaces.add(id);
  const refused = /Kontor workspace guard refused[^"\n]*/.exec(run.stderr)?.[0] ?? null;
  check(name, `cli workspace create --isolation ${isolation}${caller ? " (caller env)" : " (no caller)"}`, expected, id ? "created" : refused ? "refused" : "error", { workspaceId: id, message: refused ?? (id ? undefined : run.stderr.slice(0, 300)) });
}
const dir = (name) => { const path = join(root, name); mkdirSync(path, { recursive: true }); return path; };

try {
  report.paseo = paseo(["--version"]).stdout;
  paseo(["daemon", "config", "set", "daemon.listen", `127.0.0.1:${port}`, "--string"]);
  paseo(["daemon", "config", "set", "pluginsEnabled", "true"]);
  report.relayOff = paseo(["daemon", "config", "set", "daemon.relay.enabled", "false"]).status;
  // No provider override or command replacement in the isolated daemon config: provider launches stay on PATH.
  report.envelope.configAgents = JSON.parse(readFileSync(join(home, "config.json"), "utf8")).agents ?? null;
  assert.equal(report.envelope.configAgents, null, "the isolated daemon config carries agent or provider settings");
  mkdirSync(switches, { recursive: true });
  writeFileSync(join(switches, "floor.json"), '{"floor":"off"}');
  const started = spawnSync(cli, ["daemon", "start", "--home", home], { env: { ...process.env, PASEO_HOME: home, KONTOR_PLUGIN_MCP: "off" }, encoding: "utf8" });
  assert.equal(started.status, 0, started.stderr);
  const installed = paseo(["plugin", "install", pluginDir, "--json"]);
  assert.equal(installed.status, 0, installed.stderr.slice(0, 500));
  const plugins = JSON.parse(paseo(["plugin", "ls", "--json"]).stdout);
  report.installed = plugins.find((p) => p.id === "kontor") ?? null;
  assert.equal(report.installed?.status, "running");
  // The worker must read the isolated home's switches before any case relies on them.
  const startLines = pluginLines();
  assert.ok(startLines.some((l) => l.event === "kontor.floor.mode" && l.file === join(switches, "floor.json") && l.floor === "disabled"), JSON.stringify(startLines));
  assert.ok(startLines.some((l) => l.event === "kontor.placement.mode" && l.file === join(switches, "placement.json") && l.placement === "off"), JSON.stringify(startLines));

  client = createPaseoClient({ url: `ws://127.0.0.1:${port}/ws`, clientId: "kontor-placement-qualification", reconnect: { enabled: false } });
  await client.connect();
  const project = async (path, name) => {
    const ws = await client.workspaces.create({ source: { kind: "directory", path } });
    made.workspaces.add(ws.id);
    if (name) assert.equal(paseo(["project", "rename", ws.projectId, name, "--json"]).status, 0);
    return { workspaceId: ws.id, projectId: ws.projectId, path };
  };
  const desk = await project(dir("desk-projects/ADAM"), "DESK • ADAM");
  const esw1 = await project(dir("epic-projects/ASMA-1"), "ESW • ASMA-1");
  const esw2 = await project(dir("epic-projects/ASMA-2"), "ESW • ASMA-2");
  const repo = dir("repo");
  execFileSync("git", ["init", "-q", repo]);
  execFileSync("git", ["-C", repo, "-c", "user.name=q", "-c", "user.email=q@example.invalid", "commit", "-q", "--allow-empty", "-m", "init"]);
  const linked = join(root, "repo-task");
  execFileSync("git", ["-C", repo, "worktree", "add", "-q", "-b", "task", linked]);
  const plain = await project(repo, null);

  let snapshot = await client.providers.snapshot({ cwd: repo });
  if (!snapshot.entries.length || snapshot.entries.some((e) => e.status === "loading")) snapshot = await client.providers.waitForReady({ cwd: repo, timeoutMs: 90_000 });
  const ready = snapshot.entries.filter((e) => e.status === "ready" && e.models?.length && e.enabled !== false);
  const provider = ready.find((e) => e.provider === "claude") ?? ready[0];
  assert.ok(provider, "no ready provider in the isolated home");
  const config = { provider: `${provider.provider}/${provider.models[0].id}`, ...(provider.defaultModeId ? { modeId: provider.defaultModeId } : {}) };
  const agent = async (where, title, labels) => {
    const handle = client.workspaces.ref(where.workspaceId);
    await handle.refresh();
    const created = await handle.agents.create({ agentId: randomUUID(), title, config, labels });
    made.agents.add(created.id);
    return { id: created.id, workspaceId: where.workspaceId, cwd: where.path };
  };
  const swe = await agent(esw1, "SWE", { orchestration: "paseo-direct", epic: "ASMA-1", task: "ASMA-3", role: "SWE" });
  const bare = await agent(esw1, "unlabelled", {});
  const tpm = await agent(desk, "TPM", { role: "TPM", epic: "ASMA-1" });
  const mislabelled = await agent(esw1, "mislabelled", { "kontor.seat_binding_id": "seat-q", "kontor.workspace_id": esw2.workspaceId, "kontor.worktree": esw2.path });
  const unmarked = await agent(plain, "unmarked", {});
  const legacy = await agent(plain, "legacy review seat", { orchestration: "paseo-direct", epic: "ASMA-1", role: "review-a" });
  const target = (eswProject, name) => ["--path", dir(`targets/${name}`), "--project", eswProject.projectId];

  // Before: placement.json absent follows floor off, so a workflow caller can still misplace.
  cliCreate("before: SWE into another epic project", "created", swe, target(esw2, "before"));

  placement("on");
  cliCreate("wrong project with a valid id", "refused", swe, target(esw2, "a1"));
  cliCreate("own epic project", "created", swe, target(esw1, "a2"));
  cliCreate("(a) unlabelled caller in an epic project, other project", "refused", bare, target(esw2, "a3"));
  cliCreate("(b) workflow caller with no target project", "refused", bare, ["--path", dir("targets/a4")]);
  cliCreate("worktree route from a workflow caller", "refused", swe, ["--path", repo, "--project", plain.projectId], "worktree");
  // LSA-B-01: no source the hook can read proves a desk caller's epic assignment, so its epic label widens nothing.
  cliCreate("desk caller with an epic label into that epic project (no assignment source)", "refused", tpm, target(esw1, "a6"));
  cliCreate("desk caller into another epic project", "refused", tpm, target(esw2, "a7"));
  cliCreate("(d) Kontor binding naming another seat's workspace", "refused", mislabelled, target(esw2, "a8"));
  cliCreate("unlabelled agent outside desk and epic projects (unclassified, unguarded)", "created", unmarked, target(esw2, "a9"));
  cliCreate("(c) no caller and no workflow claim (unclassified, unguarded)", "created", null, target(esw2, "a10"));
  // LSA-B-02 residual: a workflow seat in a misplaced legacy project is held to that project only while its labels remain.
  cliCreate("legacy-misplaced seat with its labels", "refused", legacy, target(esw2, "a13"));
  assert.equal(paseo(["agent", "update", legacy.id, "--label", "orchestration=", "--label", "epic=", "--label", "role="]).status, 0);
  cliCreate("legacy-misplaced seat with its labels removed (unsupported residual, unguarded)", "created", legacy, target(esw2, "a14"));
  {
    const name = "workflow-labelled initial agent with no caller (missing caller)";
    const result = await client.workspaces.create({ agent: { config, cwd: esw2.path, labels: { orchestration: "paseo", workflow: "kontor" } }, source: { kind: "directory", path: dir("targets/a11"), projectId: esw2.projectId } })
      .then((ws) => (made.workspaces.add(ws.id), { outcome: "created", workspaceId: ws.id }), (e) => ({ outcome: /Kontor workspace guard refused/.test(String(e?.message)) ? "refused" : "error", message: String(e?.message ?? e).slice(0, 300) }));
    check(name, "sdk workspaces.create (initial agent, no caller)", "refused", result.outcome, result);
  }
  // Paseo 0.17.4's `run --new-workspace` sends `workspace.create` with its source and, when one resolves, the caller
  // (cli run.js:405-409), never an initial agent: the run's labels go only to the agent it creates afterwards
  // (run.js:510-523). The local source it builds carries no projectId (run.js:61-75, workspace/create.js:12-27).
  {
    // A workflow caller is held to its own project and refused because the request names no project; the run's label
    // never reaches the guard. Refused before any agent or turn starts.
    const run = paseo(["run", "--new-workspace", "local", "--cwd", dir("targets/a12"), "--provider", config.provider, "--label", "orchestration=paseo-direct", "--json", "qualification"], { PASEO_AGENT_ID: swe.id, PASEO_WORKSPACE_ID: swe.workspaceId, PASEO_AGENT_CWD: swe.cwd });
    const id = run.status === 0 ? JSON.parse(run.stdout).agentId : null;
    if (id) made.agents.add(id);
    check("cli run --new-workspace from a workflow caller (refused for the missing projectId)", "cli run --new-workspace (caller env)", "refused", id ? "created" : /Kontor workspace guard refused/.test(run.stderr) ? "refused" : "error", { message: /Kontor workspace guard refused[^"\n]*/.exec(run.stderr)?.[0] ?? run.stderr.slice(0, 300) });
  }
  {
    // With no caller the request carries no caller and no workflow claim, whatever labels the run has, so it is
    // unclassified and unguarded: the workspace is created and the daemon finds or creates a project for the directory
    // (workspace-provisioning-service.js:137-140). The run's agent is then created in it with the workflow labels, and
    // its one turn reaches only the envelope's refusing stub.
    const cwd = dir("targets/a15");
    const run = paseo(["run", "--new-workspace", "local", "--cwd", cwd, "--provider", config.provider, "--label", "orchestration=paseo", "--label", "workflow=kontor", "--no-wait", "--json", "qualification"]);
    const id = run.status === 0 ? JSON.parse(run.stdout).agentId : null;
    if (id) made.agents.add(id);
    const agentRead = id ? (await client.agents.ref(id).refresh())?.agent ?? null : null;
    // The run's workspace, by its agent or, if the agent was not created, by its directory.
    const listed = agentRead?.workspaceId ? { id: agentRead.workspaceId } : (await client.workspaces.list({ includeBackground: true, page: { limit: 200 } })).entries.find((w) => w.workspaceDirectory && resolve(w.workspaceDirectory) === resolve(cwd));
    const workspace = listed ? await client.workspaces.ref(listed.id).refresh() : null;
    if (workspace) made.workspaces.add(workspace.id);
    const labels = agentRead?.labels ?? null;
    const fixtures = [desk, esw1, esw2, plain].map((p) => p.projectId);
    check("cli run --new-workspace with no caller and workflow labels (unclassified, unguarded)", "cli run --new-workspace (no caller)", "created",
      workspace ? "created" : /Kontor workspace guard refused/.test(run.stderr) ? "refused" : "error",
      { workspaceId: workspace?.id ?? null, projectId: workspace?.projectId ?? null, fixtureProject: workspace ? fixtures.includes(workspace.projectId) : null, agentId: id, agentLabels: labels, message: workspace ? undefined : run.stderr.slice(0, 300) });
  }
  // Routes the hook cannot see: the import on agent create and an operator's directory open. Detection only.
  {
    const imported = await client.agents.create({ agentId: randomUUID(), title: "import route", config, cwd: linked });
    made.agents.add(imported.id);
    const read = await client.agents.ref(imported.id).refresh();
    if (read?.agent.workspaceId) made.workspaces.add(read.agent.workspaceId);
    check("agent create with cwd only at a linked worktree (import, no hook)", "sdk agents.create cwd", "created", read?.agent.workspaceId ? "created" : "error", { workspaceId: read?.agent.workspaceId ?? null });
  }
  // Stock summary helper, slice A absent: the generator's own create fields and per-job cwd under the summary state
  // root, without the prompt and output schema, so no model turn runs. The import has no before hook, so placement
  // neither refuses nor places it; each job adds a project and a workspace (the open residual).
  const live = async () => {
    const workspaces = (await client.workspaces.list({ includeBackground: true, page: { limit: 200 } })).entries.filter((w) => !w.archivingAt);
    return { projects: ((await client.projects.list()).projects ?? []).length, workspaces: workspaces.length };
  };
  const jobs = join(home, "plugin-state", "kontor", "session-summaries-v1", "generation-jobs");
  for (const job of [1, 2]) {
    const before = await live();
    const workerId = randomUUID(), cwd = join(jobs, workerId);
    mkdirSync(cwd, { recursive: true, mode: 0o700 });
    const result = await client.agents.create({
      agentId: workerId, clientMessageId: randomUUID(), idempotencyKey: `kontor-summary:${workerId}`, cwd,
      title: `Summary · qualification source ${job}`, config,
      labels: { "kontor.summary.source": swe.id, "kontor.summary.job": `qualification-${job}` },
    }).then((created) => (made.agents.add(created.id), { outcome: "created" }), (e) => ({ outcome: /Kontor workspace guard refused/.test(String(e?.message)) ? "refused" : "error", message: String(e?.message ?? e).slice(0, 300) }));
    const read = result.outcome === "created" ? await client.agents.ref(workerId).refresh() : null;
    const workspace = read?.agent.workspaceId ? await client.workspaces.ref(read.agent.workspaceId).refresh() : null;
    if (workspace) made.workspaces.add(workspace.id);
    const after = await live();
    const residual = { job, workerId, workspaceId: workspace?.id ?? null, projectId: workspace?.projectId ?? null, workspaceDirectory: workspace?.workspaceDirectory ?? null, before, after, added: { projects: after.projects - before.projects, workspaces: after.workspaces - before.workspaces } };
    report.summaryResidual.push(residual);
    check(`stock summary helper job ${job} with placement enforced (slice A absent)`, "sdk agents.create cwd (summary helper import)", "created", result.outcome, residual);
  }
  await new Promise((done) => setTimeout(done, 1500));
  report.detections = pluginLines().filter((l) => l.event === "kontor.placement.detected");
  assert.ok(report.detections.some((l) => l.kind === "project-at-linked-worktree" && resolve(l.projectRootPath) === resolve(linked)), JSON.stringify(report.detections));
  // The detector covers two kinds only; a summary job's container is neither, so it is not detected.
  const helperWorkspaces = new Set(report.summaryResidual.map((r) => r.workspaceId));
  check("summary helper containers are not detected", "workspace.created detector", "not-detected", report.detections.some((l) => helperWorkspaces.has(l.workspaceId)) ? "detected" : "not-detected", { workspaces: [...helperWorkspaces] });

  placement("off");
  cliCreate("rollback: placement off lets the misplacement through again", "created", swe, target(esw2, "rollback"));
  placement(null);
  report.modes = pluginLines().filter((l) => l.event === "kontor.placement.mode");
} finally {
  const cleanup = { agents: [], workspaces: [] };
  if (client) {
    for (const id of made.agents) cleanup.agents.push(await client.agents.ref(id).archive().then((a) => ({ id, archivedAt: a?.archivedAt ?? "archived" }), (e) => ({ id, error: String(e?.message ?? e) })));
    for (const id of made.workspaces) cleanup.workspaces.push(await client.workspaces.archive(id).then(() => ({ id, archived: true }), (e) => ({ id, error: String(e?.message ?? e) })));
    const left = (await client.workspaces.list({ includeBackground: true, page: { limit: 200 } })).entries.filter((w) => !w.archivingAt).map((w) => w.id);
    cleanup.activeWorkspacesAfter = left;
    await client.close().catch(() => {});
  }
  cleanup.daemonStop = paseo(["daemon", "stop"]).status;
  const calls = join(box, "provider-invocations.jsonl");
  report.envelope.invocations = existsSync(calls) ? readFileSync(calls, "utf8").split("\n").filter(Boolean).map((line) => JSON.parse(line)) : [];
  rmSync(home, { recursive: true, force: true });
  rmSync(root, { recursive: true, force: true });
  report.cleanup = cleanup;
  writeFileSync(out, `${JSON.stringify(report, null, 2)}\n`, { mode: 0o600 });
}
const answered = report.envelope.invocations.filter((c) => c.op === "version").length;
console.log(JSON.stringify({ checks: report.checks.length, passed: report.checks.filter((c) => c.outcome === c.expected).length, detections: report.detections.length, summaryResidual: report.summaryResidual.map((r) => r.added), providerCalls: { versionAnswered: answered, refused: report.envelope.invocations.length - answered } }));
