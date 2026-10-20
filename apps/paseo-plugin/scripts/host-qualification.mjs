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

const [planPath, outputPath, resources, jiraPath, teamsCatalogPath] = process.argv.slice(2);
assert.ok(planPath && outputPath && resources, "Usage: host-qualification.mjs <canonical-plan> <evidence-dir> <Paseo Resources> [historical-Jira] [isolated-Teams-catalog.json]");
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
const originalPaseoHome = process.env.PASEO_HOME;
let cleanup;
try {
  await mkdir(output, { recursive: true });
  const expectedSourceHash = hash(await readFile(planPath));
  const catalog = [{ id: "qualification-plan", title: "Qualification plan", kind: "plan", path: resolve(planPath), expectedSha256: expectedSourceHash }];
  if (jiraPath) catalog.push({ id: "qualification-jira", title: "Historical Jira", kind: "jira", path: resolve(jiraPath), expectedSha256: hash(await readFile(jiraPath)) });
  // Explicit local qualification inputs only. This changes this disposable process,
  // never a running host's catalog or configuration.
  const teamsCatalog = teamsCatalogPath ? JSON.parse(await readFile(teamsCatalogPath, "utf8")) : [];
  assert.ok(Array.isArray(teamsCatalog));
  const authorizedIds = ["authoring", "operator", "roster", "templates"];
  assert.equal(new Set(teamsCatalog.map((entry) => entry.id)).size, teamsCatalog.length);
  for (const entry of teamsCatalog) {
    assert.ok(authorizedIds.includes(entry.id) && entry.kind === "teams");
    assert.match(entry.expectedSha256, /^[a-f0-9]{64}$/);
    assert.equal(hash(await readFile(entry.path)), entry.expectedSha256);
    catalog.push(entry);
  }
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
  // floor.json is read from this disposable home, never the runner's ~/.paseo.
  const paseoHome = join(work, "paseo-home");
  const floorPath = join(paseoHome, "plugin-config/kontor/floor.json");
  process.env.PASEO_HOME = paseoHome;
  // The server's floor lines go to stdout/stderr; record them while it starts.
  const floorLines = (start) => {
    const lines = { info: [], errors: [] };
    const { log, error } = console;
    console.log = (line) => lines.info.push(JSON.parse(line));
    console.error = (line) => lines.errors.push(JSON.parse(line));
    try { return { ...lines, stop: start() }; } finally { console.log = log; console.error = error; }
  };
  let definition;
  const absent = floorLines(() => serverModule.default({
    registerSettings(value) { if (value.id === "organization") definition = value; const registration = store.register(value); settings.set(value.id, registration); return registration.settings; },
    handle(contract, handler) { handlers.set(contract.name, handler); },
    before(name) { hooks.push(name); return () => {}; },
  }));
  cleanup = absent.stop;
  assert.deepEqual(hooks, ["agent.create", "agent.session_open", "workspace.create"]);
  assert.deepEqual(absent.info, [{ event: "kontor.floor.mode", floor: "enabled", reason: "not-configured", file: floorPath }]);
  assert.deepEqual(absent.errors, []);
  // The same server bundle in a new start with the operator's file off, then non-conforming.
  const floorStart = async (label, content) => {
    await mkdir(dirname(floorPath), { recursive: true });
    await writeFile(floorPath, content, { mode: 0o600 });
    const floorStore = new PluginSettingsStore(join(work, `floor-${label}`), () => {});
    const started = { hooks: [], handlers: [] };
    const lines = floorLines(() => serverModule.default({
      registerSettings: (value) => floorStore.register(value).settings,
      handle(contract) { started.handlers.push(contract.name); },
      before(name) { started.hooks.push(name); return () => {}; },
    }));
    lines.stop();
    return { ...started, info: lines.info, errors: lines.errors };
  };
  const floorOff = await floorStart("off", '{\n  "floor": "off"\n}\n');
  assert.deepEqual(floorOff.hooks, []);
  assert.deepEqual(floorOff.handlers, [...handlers.keys()]);
  assert.deepEqual(floorOff.info, [{ event: "kontor.floor.mode", floor: "disabled", reason: "explicit-off", file: floorPath }]);
  assert.deepEqual(floorOff.errors, []);
  const floorInvalid = await floorStart("invalid", '{"floor":"OFF"}');
  assert.deepEqual(floorInvalid.hooks, ["agent.create", "agent.session_open", "workspace.create"]);
  assert.deepEqual(floorInvalid.errors.map((line) => [line.event, line.floor]), [["kontor.floor.invalid", "enabled"]]);
  await rm(floorPath);
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
  // The Back shortcut lives in its own file: off by default, closed to other combos, and saving it leaves organization.json alone.
  const keyboard = settings.get("keyboard");
  assert.ok(keyboard);
  const keyboardDefault = await keyboard.read.handle();
  assert.equal(keyboardDefault.status, "ready"); assert.deepEqual(keyboardDefault.values, { backShortcut: "off" });
  assert.equal((await keyboard.write.handle({ revision: keyboardDefault.revision, values: { backShortcut: "cmd-k" } })).status, "invalid");
  const organizationBytes = await readFile(join(work, "organization.json"));
  const keyboardSaved = await keyboard.write.handle({ revision: keyboardDefault.revision, values: { backShortcut: "cmd-backquote" } });
  assert.equal(keyboardSaved.status, "saved");
  assert.ok((await readFile(join(work, "organization.json"))).equals(organizationBytes));
  assert.deepEqual((await registration.read.handle()).values, clearedEpic.values);
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
  assert.ok(handlers.has("teams.sources.read"));
  await assert.rejects(() => handlers.get("teams.sources.read")({ sourceId: "not-configured" }));
  const teamSnapshots = [];
  for (const entry of teamsCatalog) {
    const snapshot = await handlers.get("teams.sources.read")({ sourceId: entry.id });
    assert.equal(snapshot.status, "ready", JSON.stringify(snapshot.diagnostics));
    assert.equal(snapshot.sha256, entry.expectedSha256);
    assert.equal(snapshot.activation, "not-attested");
    teamSnapshots.push(snapshot);
  }
  const compositionPath = join(work, "teams-composition.cjs");
  execFileSync("bun", ["build", join(pluginRoot, "shared/teams-source-snapshot.ts"), "--target=node", "--format=cjs", "--outfile", compositionPath], { encoding: "utf8" });
  const { composeTeamsSourceView } = pluginRequire(compositionPath);
  const teamsView = composeTeamsSourceView(teamSnapshots, []);
  assert.ok(teamsView.metadata.every((item) => item.state === "unresolved"));
  const teams = { scope: "isolated read-only qualification catalog; production host catalog not attested",
    nativeObservedSessions: 0, activation: "not-attested", unknownIdRejected: true,
    sources: teamSnapshots.map((snapshot) => ({ id: snapshot.source.id, path: snapshot.source.path, format: snapshot.format,
      status: snapshot.status, sha256: snapshot.sha256, readAt: snapshot.readAt, modifiedAt: snapshot.modifiedAt,
      sourceObservedAt: null, configurationState: snapshot.configurationState, diagnostics: snapshot.diagnostics })),
    unresolvedBindings: teamsView.metadata.length, metadata: teamsView.metadata };
  stopNotices();
  const report = { hostVersion: JSON.parse(await readFile(join(hostRoot, "package.json"), "utf8")).version,
    installation: "none; offline compiler and disposable settings store only", hooks,
    floor: { absent: hooks, off: floorOff.hooks, invalid: floorInvalid.hooks, offModeLine: floorOff.info[0], invalidDiagnostic: floorInvalid.errors[0] },
    clientBytes: Buffer.byteLength(compiled.clientBundle), clientSha256: hash(compiled.clientBundle),
    serverBytes: Buffer.byteLength(compiled.serverBundle), serverSha256: hash(compiled.serverBundle),
    hermesExit: 0, bytecodeVersion: 96, bytecodeBytes: bytecode.length, bytecodeSha256: hash(bytecode), bytecodeSourcePath: clientPath,
    settings: { twoClientConflict: conflict.status, reloadPreservesUnrelatedChanges: true, rootOrderPreserved: true, bothMappingClearsSaved: true, restartPersistence: true, invalidStoredDataPreserved: true, successfulNotifications: notices.length, changes: changes.length },
    keyboard: { defaultBackShortcut: keyboardDefault.values.backShortcut, otherComboRefused: true, saved: keyboardSaved.values.backShortcut, organizationBytesUnchanged: true },
    historicalJira, teams,
    plan: { path: resolve(planPath), sourceSha256: expectedSourceHash, graphSha256: source.graph.graph_sha256, nodes: source.graph.nodes.length, edges: source.graph.edges.length, status: source.status },
  };
  await writeFile(join(output, "qualification.json"), JSON.stringify(report, null, 2) + "\n");
  console.log(JSON.stringify(report, null, 2));
} finally {
  cleanup?.();
  if (originalCatalog === undefined) delete process.env.KONTOR_PLUGIN_READ_SOURCES; else process.env.KONTOR_PLUGIN_READ_SOURCES = originalCatalog;
  if (originalPaseoHome === undefined) delete process.env.PASEO_HOME; else process.env.PASEO_HOME = originalPaseoHome;
  await rm(work, { recursive: true, force: true });
}
