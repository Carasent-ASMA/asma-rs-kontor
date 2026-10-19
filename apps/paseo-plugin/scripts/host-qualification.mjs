// Run with the installed Paseo Electron helper in ELECTRON_RUN_AS_NODE mode.
// Compiles local source and exercises host settings in a disposable directory;
// it never installs a plugin, connects to a daemon, or creates native sessions.
import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { mkdtemp, mkdir, readFile, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const [planPath, outputPath, resources, jiraPath] = process.argv.slice(2);
assert.ok(planPath && outputPath && resources, "Usage: host-qualification.mjs <canonical-plan> <evidence-dir> <Paseo Resources>");
const pluginRoot = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const hostRoot = join(resources, "app.asar/node_modules/@getpaseo/server");
const hostPlugins = join(hostRoot, "dist/server/server/plugins");
const { compilePlugin } = await import(pathToFileURL(join(hostPlugins, "compiler.js")).href);
const { PluginSettingsStore } = await import(pathToFileURL(join(hostPlugins, "settings/index.js")).href);
const hostRequire = createRequire(join(hostRoot, "package.json"));
const pluginRequire = createRequire(join(pluginRoot, "package.json"));
const work = await mkdtemp(join(tmpdir(), "kontor-host-settings-"));
const output = resolve(outputPath);
const hash = (value) => createHash("sha256").update(value).digest("hex");
const originalCatalog = process.env.KONTOR_PLUGIN_READ_SOURCES;
let cleanup;
try {
  await mkdir(output, { recursive: true });
  const expectedSourceHash = hash(await readFile(planPath));
  const catalog = [{ id: "qualification-plan", title: "Qualification plan", kind: "plan", path: resolve(planPath), expectedSha256: expectedSourceHash }];
  if (jiraPath) catalog.push({ id: "qualification-jira", title: "Historical Jira", kind: "jira", path: resolve(jiraPath), expectedSha256: hash(await readFile(jiraPath)) });
  process.env.KONTOR_PLUGIN_READ_SOURCES = JSON.stringify(catalog);
  const compiled = await compilePlugin({ client: join(pluginRoot, "index.client.tsx"), server: join(pluginRoot, "index.server.ts") });
  assert.ok(compiled.clientBundle && compiled.serverBundle);
  const clientPath = join(output, "client.js");
  const serverPath = join(output, "server.js");
  const bytecodePath = join(output, "client.hbc");
  await writeFile(clientPath, compiled.clientBundle);
  await writeFile(serverPath, compiled.serverBundle);
  const hermesc = join(dirname(pluginRequire.resolve("react-native/package.json")), "sdks/hermesc/osx-bin/hermesc");
  execFileSync(hermesc, ["-emit-binary", "-out", bytecodePath, clientPath], { encoding: "utf8", maxBuffer: 2 * 1024 * 1024 });
  const bytecode = await readFile(bytecodePath);
  assert.equal(bytecode.readUInt32LE(8), 96, "Expected HBC96");
  const handlers = new Map();
  const settings = new Map();
  const changes = [];
  const hooks = [];
  const store = new PluginSettingsStore(work, (id) => changes.push(id));
  const evaluate = (0, eval)(compiled.serverBundle);
  const serverModule = evaluate(hostRequire);
  let definition;
  cleanup = serverModule.default({
    registerSettings(value) { definition = value; const registration = store.register(value); settings.set(value.id, registration); return registration.settings; },
    handle(contract, handler) { handlers.set(contract.name, handler); },
    before(name) { hooks.push(name); return () => {}; },
  });
  assert.deepEqual(hooks, ["agent.create", "agent.session_open", "workspace.create"]);
  const registration = settings.get("organization");
  assert.ok(registration);
  const notices = [];
  const stopNotices = registration.settings.subscribe((state) => notices.push(state));
  const desktop = await registration.read.handle();
  const phone = await registration.read.handle();
  assert.equal(desktop.status, "ready"); assert.equal(phone.status, "ready");
  const orderPath = join(work, "organization-order.cjs");
  execFileSync("bun", ["build", join(pluginRoot, "client/organization-order.ts"), "--target=node", "--format=cjs", "--outfile", orderPath], { encoding: "utf8" });
  const { reorderSavedSiblingOrder } = pluginRequire(orderPath);
  const fullOrder = ["Z", "Y", "X", "W", "Adam"];
  const first = await registration.write.handle({ revision: desktop.revision, values: { ...desktop.values, favorites: ["native-desktop"], siblingOrder: { root: fullOrder }, planAssociations: { "native-conversation": ["qualification-plan"] } } });
  assert.equal(first.status, "saved");
  const conflict = await registration.write.handle({ revision: phone.revision, values: { ...phone.values, appearance: "dark" } });
  assert.equal(conflict.status, "conflict");
  assert.equal(notices.length, 1, "A conflict must not notify as a successful save");
  const reloadedPhone = await registration.read.handle();
  const reordered = reorderSavedSiblingOrder(reloadedPhone.values.siblingOrder.root, fullOrder, ["Adam"], "Adam", -1);
  const second = await registration.write.handle({ revision: reloadedPhone.revision, values: { ...reloadedPhone.values, appearance: "dark", siblingOrder: { ...reloadedPhone.values.siblingOrder, root: reordered } } });
  assert.deepEqual(second.values.siblingOrder.root, fullOrder, "A successful partition move must preserve the full root order");
  assert.equal(second.status, "saved");
  assert.deepEqual(second.values.favorites, ["native-desktop"]);
  const invalid = await registration.write.handle({ revision: second.revision, values: { ...second.values, grouping: "invalid grouping" } });
  assert.equal(invalid.status, "invalid"); assert.equal(notices.length, 2);
  const draftPath = join(work, "organization-draft.cjs");
  execFileSync("bun", ["build", join(pluginRoot, "client/organization-draft.ts"), "--target=node", "--format=cjs", "--outfile", draftPath], { encoding: "utf8" });
  const { setDisplayMappingField } = pluginRequire(draftPath);
  const mapped = await registration.write.handle({ revision: second.revision, values: { ...second.values, displayMappings: { "session:native": { workstream: "Adam", epic: "ASMA-1" }, "workspace:other": { workstream: "QA" } } } });
  assert.equal(mapped.status, "saved");
  const clearedWorkstream = await registration.write.handle({ revision: mapped.revision, values: { ...mapped.values, displayMappings: setDisplayMappingField(mapped.values.displayMappings, "session:native", "workstream", "") } });
  assert.equal(clearedWorkstream.status, "saved");
  assert.deepEqual(clearedWorkstream.values.displayMappings["session:native"], { epic: "ASMA-1" });
  const clearedEpic = await registration.write.handle({ revision: clearedWorkstream.revision, values: { ...clearedWorkstream.values, displayMappings: setDisplayMappingField(clearedWorkstream.values.displayMappings, "session:native", "epic", "") } });
  assert.equal(clearedEpic.status, "saved");
  assert.deepEqual(clearedEpic.values.displayMappings, { "workspace:other": { workstream: "QA" } });
  const associations = await handlers.get("sources.associations")({});
  assert.equal(associations.status, "ready");
  assert.deepEqual(associations.associations, { "native-conversation": ["qualification-plan"] });
  const restart = new PluginSettingsStore(work, () => {});
  const restarted = restart.register(definition);
  assert.deepEqual((await restarted.read.handle()).values, clearedEpic.values);
  const settingsPath = join(work, "organization.json");
  const corrupt = "{ invalid stored JSON";
  await writeFile(settingsPath, corrupt);
  assert.equal((await restarted.read.handle()).status, "invalid");
  assert.equal(await readFile(settingsPath, "utf8"), corrupt);
  const source = await handlers.get("sources.read")({ sourceId: "qualification-plan" });
  assert.equal(source.status, "ready", source.diagnostics.join("\n"));
  assert.equal(source.revision, expectedSourceHash);
  assert.equal(source.graph.source_sha256, expectedSourceHash);
  let historicalJira = null;
  if (jiraPath) {
    const projectedBytes = await readFile(jiraPath);
    const projected = JSON.parse(projectedBytes);
    const historical = await handlers.get("sources.read")({ sourceId: "qualification-jira" });
    assert.equal(historical.status, "ready", historical.diagnostics.join("\n"));
    assert.equal(historical.jiraHistory.generated, projected.generated);
    assert.equal(historical.content, null);
    assert.ok(projectedBytes.length <= 2 * 1024 * 1024);
    historicalJira = { schema: historical.jiraHistory.schema, observedAt: historical.jiraHistory.generated,
      readAt: historical.readAt, projectionBytes: projectedBytes.length, originalSha256: historical.jiraHistory.provenance.sha256,
      capturedEpics: historical.jiraHistory.provenance.capturedEpics,
      selected: historical.jiraHistory.epics.map((epic) => ({ key: epic.key, capturedChildren: epic.children.length })), status: historical.status };
  }
  stopNotices();
  const report = { hostVersion: JSON.parse(await readFile(join(hostRoot, "package.json"), "utf8")).version,
    installation: "none; offline compiler and disposable settings store only", hooks,
    clientBytes: Buffer.byteLength(compiled.clientBundle), clientSha256: hash(compiled.clientBundle),
    serverBytes: Buffer.byteLength(compiled.serverBundle), serverSha256: hash(compiled.serverBundle),
    hermesExit: 0, bytecodeVersion: 96, bytecodeBytes: bytecode.length, bytecodeSha256: hash(bytecode), bytecodeSourcePath: clientPath,
    settings: { twoClientConflict: conflict.status, reloadPreservesUnrelatedChanges: true, rootOrderPreserved: true, bothMappingClearsSaved: true, restartPersistence: true, invalidStoredDataPreserved: true, successfulNotifications: notices.length, changes: changes.length },
    historicalJira,
    plan: { path: resolve(planPath), sourceSha256: expectedSourceHash, graphSha256: source.graph.graph_sha256, nodes: source.graph.nodes.length, edges: source.graph.edges.length, status: source.status },
  };
  await writeFile(join(output, "qualification.json"), JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify(report, null, 2));
} finally {
  cleanup?.();
  if (originalCatalog === undefined) delete process.env.KONTOR_PLUGIN_READ_SOURCES; else process.env.KONTOR_PLUGIN_READ_SOURCES = originalCatalog;
  await rm(work, { recursive: true, force: true });
}
