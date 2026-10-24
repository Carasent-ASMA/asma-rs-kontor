import { createHash } from "node:crypto";
import { copyFile, mkdtemp, readFile, rm, symlink, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath, URL } from "node:url";
import { afterEach, describe, expect, it } from "vitest";
import { createTeamsSourceAdapter } from "../server/teams-source-adapter";
import { sourceCatalog } from "../server/read-sources";
import { decodeTeamsConfigYaml } from "../server/teams-source-yaml";
import { composeTeamsSourceView, readTeamsSource, teamsSourceSnapshotSchema,
  type ObservedTeamsSession, type TeamsSourceFormat, type TeamsSourceSnapshot } from "../shared/teams-source-snapshot";

// Copies of the authorized files, with the owner identity replaced by a
// synthetic label, pinned to LSA-SOURCE-02 (not hand-authored records or a
// runtime-policy fixture).
const sources = [
  { id: "authoring", filename: "authoring-fleet.yml", format: "fleet-authoring",
    hash: "fa82d7a49d12a01a30f0a3b939d605aecf04d784fb80e53b88fcc99ff1f211f7" },
  { id: "operator", filename: "operator-fleet.yml", format: "fleet-operator",
    hash: "5349f628b9de00e9f62ef9d69b4e8977d3c6a28c582f3b5497ed961c77060487" },
  { id: "roster", filename: "core-team.yml", format: "core-roster",
    hash: "329f5fb70d34cb51409f32588533c1b200bd5bb434341a35d9d66fa142fd6cfc" },
  { id: "templates", filename: "mvp-team-pack.json", format: "team-templates",
    hash: "2a5a0aed9c23450f984fd3d1f92c5824f4640a71b2bc9697349b0632fed8b3cc" },
] as const;
const temporary: string[] = [];
afterEach(async () => { await Promise.all(temporary.splice(0).map((path) => rm(path, { recursive: true, force: true }))); });
const hash = (bytes: Buffer) => createHash("sha256").update(bytes).digest("hex");
const fixturePath = (filename: string) => fileURLToPath(new URL(`./fixtures/teams-source/${filename}`, import.meta.url));

async function isolatedSources(pin = true) {
  const directory = await mkdtemp(join(tmpdir(), "asma-8550-teams-source-"));
  temporary.push(directory);
  const entries = [];
  const formats: Record<string, TeamsSourceFormat> = {};
  for (const source of sources) {
    const bytes = await readFile(fixturePath(source.filename));
    expect(hash(bytes)).toBe(source.hash);
    const path = join(directory, source.filename);
    await copyFile(fixturePath(source.filename), path);
    entries.push({ id: source.id, title: source.id, kind: "teams", path,
      expectedSha256: pin ? source.hash : null, revisionKind: "working copy" });
    formats[source.id] = source.format;
  }
  const catalog = sourceCatalog(JSON.stringify(entries));
  expect(catalog.diagnostics).toEqual([]);
  const adapter = createTeamsSourceAdapter(catalog, formats);
  return { directory, catalog, formats, adapter, readAll: () => Promise.all(sources.map(({ id }) => adapter.read({ sourceId: id }))) };
}

function fleet(snapshot: TeamsSourceSnapshot) {
  expect(snapshot.data?.kind, JSON.stringify(snapshot.diagnostics)).toBe("fleet");
  if (snapshot.data?.kind !== "fleet") throw new Error("Expected fleet data");
  return snapshot.data;
}
const observed: readonly ObservedTeamsSession[] = [{ nativeSessionId: "native-exact-id", provider: "codex-a",
  model: "user-selected-model", effort: null, observedAt: "2026-10-19T10:00:00.000Z" }];

describe("ASMA-8550 actual Teams/chains source adapter", () => {
  it("reads the exact four hashed files through the existing source catalog", async () => {
    const { readAll } = await isolatedSources();
    const snapshots = await readAll();
    for (const [index, snapshot] of snapshots.entries()) {
      expect(snapshot.status, JSON.stringify(snapshot.diagnostics)).toBe("ready");
      expect(snapshot.sha256).toBe(sources[index]?.hash);
      expect(snapshot.activation).toBe("not-attested");
      expect(Number.isFinite(Date.parse(snapshot.readAt))).toBe(true);
      expect(Number.isFinite(Date.parse(snapshot.modifiedAt!))).toBe(true);
      expect(teamsSourceSnapshotSchema.safeParse(snapshot).success).toBe(true);
    }
    const authoring = fleet(snapshots[0]!);
    const operator = fleet(snapshots[1]!);
    expect(snapshots[0]?.configurationState).toBe("unactivated-authoring");
    expect(snapshots[1]?.configurationState).toBe("operator-file-snapshot");
    expect([authoring.schemaVersion, authoring.chains.length, authoring.models.length, authoring.bindings.length]).toEqual([2, 18, 20, 40]);
    expect([operator.schemaVersion, operator.chains.length, operator.models.length, operator.bindings.length]).toEqual([1, 18, 20, 36]);
    expect(authoring.domains.find((domain) => domain.alias === "deepseek")?.accounts).toEqual(["opencode-deepseek"]);
    expect(operator.domains.find((domain) => domain.alias === "deepseek")?.accounts).toEqual(["opencode"]);
    expect(authoring.bindings.filter((binding) => binding.groupKind === "leadership")).toHaveLength(2);
    expect(operator.bindings.some((binding) => binding.groupKind === "leadership")).toBe(false);
    const roster = snapshots[2]?.data;
    expect(roster).toMatchObject({ kind: "roster", version: 1, roleCatalog: { version: 1,
      content_hash: "6dcecf8df91b64a64b5561454fbe09aae791107b0578df3611c2bc9bc85232df" } });
    if (roster?.kind === "roster") expect(roster.seats.map((seat) => seat.role_slot_id)).toEqual(["lsa", "tpm"]);
  });

  it("preserves rung order, alternatives, selected effort and reference/model/domain provenance", async () => {
    const { adapter } = await isolatedSources();
    const data = fleet(await adapter.read({ sourceId: "authoring" }));
    const hardDecision = data.chains.find((chain) => chain.name === "hard-decision")!;
    expect(hardDecision.rungs.map((rung) => rung.alternatives.map((alternative) => alternative.reference))).toEqual([
      ["fable-5.1@xhigh", "opus-5.5@xhigh"], ["glm-5.3@max"], ["sol@xhigh"], ["deepseek-flash@max"],
      ["inkling-free@high", "nemotron-free@high", "laguna-free@high", "north-code-free@high", "nemotron-lightning-free@high"],
    ]);
    expect(hardDecision.rungs[0]?.alternatives[0]).toMatchObject({ selectedEffort: "xhigh",
      model: { alias: "fable-5.1", id: "claude-fable-5-1", vendor: "anthropic", calibrated: false,
        supportedEfforts: ["low", "medium", "high", "xhigh", "max", "ultracode"],
        provenance: { sourceId: "authoring", pointer: "/models/fable-5.1" } },
      domain: { provider: "claude", accounts: ["claude-b", "claude-a"],
        provenance: { sourceId: "authoring", pointer: "/domains/claude" } },
      provenance: { sourceId: "authoring", pointer: "/chains/hard-decision/0/0" },
    });
    const coordinator = data.chains.find((chain) => chain.name === "coordinator")!;
    expect(coordinator.rungs[4]?.alternatives[0]).toMatchObject({ modelAlias: "composer-2.5", selectedEffort: null });
    expect(coordinator.rungs.at(-1)?.alternatives.map((alternative) => alternative.reference))
      .toEqual(hardDecision.rungs.at(-1)?.alternatives.map((alternative) => alternative.reference));
    expect(data.rules.vision_required).toEqual(["team/01936f5a-0000-7000-8000-000000000103/browser-verify"]);
  });

  it("retains both template versions and repeated role slots without applying defaults to observations", async () => {
    const { readAll } = await isolatedSources();
    const snapshots = await readAll();
    const templates = snapshots[3]?.data;
    expect(templates?.kind).toBe("templates");
    if (templates?.kind !== "templates") throw new Error("Expected templates");
    expect(templates.templates.map((template) => [template.id, template.version])).toEqual([
      ["01936f5a-0000-7000-8000-000000000001", 1], ["01936f5a-0000-7000-8000-000000000002", 1],
      ["01936f5a-0000-7000-8000-000000000001", 2], ["01936f5a-0000-7000-8000-000000000002", 2],
    ]);
    expect(templates.templates[1]?.slots.filter((slot) => slot.role.role === "researcher").map((slot) => slot.id)).toEqual(["researcher-a", "researcher-b"]);
    expect(templates.templates[0]?.slots.find((slot) => slot.id === "architect")?.defaultRungs)
      .toEqual([{ provider: "codex", model: "gpt-5.6-sol", effort: "xhigh" }]);
    expect(templates.templates[0]?.slots[0]?.autonomy).toBeNull();
    expect(templates.templates[2]?.slots[0]?.autonomy).toBe("bounded");
    const view = composeTeamsSourceView(snapshots, observed);
    expect(view.observedSessions).toBe(observed);
    expect(view.observedSessions[0]?.model).toBe("user-selected-model");
    expect(view.metadata).toHaveLength(76);
    const architect = view.metadata.find((entry) => entry.sourceId === "authoring" && entry.bindingReference.endsWith("000001/architect"))!;
    expect(architect.state).toBe("unresolved");
    expect(architect.reason).toContain("does not pin a template version");
    expect(architect.templateCandidates.map((candidate) => candidate.version)).toEqual([1, 2]);
    const unknown = view.metadata.find((entry) => entry.bindingReference.endsWith("000101/scope"))!;
    expect(unknown.templateCandidates).toEqual([]);
    expect(unknown.reason).toContain("No authorized definition");
    expect(view.metadata.find((entry) => entry.bindingReference.startsWith("leadership/"))?.reason).toContain("slot alone is insufficient");
  });

  it("keeps native observations and valid sources available when a config is absent or invalid", async () => {
    const { adapter, directory } = await isolatedSources(false);
    await rm(join(directory, "authoring-fleet.yml"));
    await writeFile(join(directory, "operator-fleet.yml"), "schema_version: 99\n");
    const missing = await adapter.read({ sourceId: "authoring" });
    const invalid = await adapter.read({ sourceId: "operator" });
    expect(missing).toMatchObject({ status: "unavailable", sha256: null, data: null });
    expect(missing.diagnostics[0]?.message).toContain("ENOENT");
    expect(invalid.status).toBe("unavailable");
    expect(invalid.sha256).toMatch(/^[a-f0-9]{64}$/);
    expect(invalid.data).toBeNull();
    const templates = await adapter.read({ sourceId: "templates" });
    expect(templates.status).toBe("ready");
    expect(composeTeamsSourceView([missing, invalid, templates], observed).observedSessions).toEqual(observed);
    expect(composeTeamsSourceView([], observed).observedSessions).toEqual(observed);
    expect(JSON.stringify([missing, invalid])).not.toMatch(/kontord.*(?:needed|required|connect)/i);
  });

  it.each([
    ["unknown-chain", "000001/architect: codex-first", "000001/architect: undeclared-chain"],
    ["unknown-model", "- [sol@xhigh]", "- [undeclared-model@xhigh]"],
    ["unknown-domain", "domain: codex,", "domain: undeclared-domain,"],
    ["invalid-effort", "- [sol@xhigh]", "- [sol@undeclared-effort]"],
  ])("reports %s precisely without inventing a model or chain", async (code, before, after) => {
    const { adapter, directory } = await isolatedSources(false);
    const path = join(directory, "authoring-fleet.yml");
    const text = await readFile(path, "utf8");
    expect(text).toContain(before);
    await writeFile(path, text.replace(before, after));
    const snapshot = await adapter.read({ sourceId: "authoring" });
    expect(snapshot.status, JSON.stringify(snapshot.diagnostics)).toBe("partial");
    expect(snapshot.diagnostics.some((entry) => entry.code === code && entry.pointer.startsWith("/"))).toBe(true);
    const data = fleet(snapshot);
    if (code === "unknown-chain") expect(data.bindings.find((binding) => binding.chainName === "undeclared-chain")?.chainResolved).toBe(false);
    if (code === "unknown-model") expect(data.chains[0]?.rungs[0]?.alternatives[0]?.model).toBeNull();
  });

  it("leaves a configured unknown slot unresolved and rejects malformed binding references", async () => {
    const { adapter, directory } = await isolatedSources(false);
    const path = join(directory, "authoring-fleet.yml");
    const original = await readFile(path, "utf8");
    await writeFile(path, original.replace("000001/architect: codex-first", "000001/unknown-slot: codex-first"));
    const snapshot = await adapter.read({ sourceId: "authoring" });
    const templates = await adapter.read({ sourceId: "templates" });
    const metadata = composeTeamsSourceView([snapshot, templates], observed).metadata;
    expect(metadata.find((entry) => entry.bindingReference.endsWith("/unknown-slot"))).toMatchObject({ state: "unresolved", templateCandidates: [],
      reason: "Unknown slot unknown-slot in the available versions of template 01936f5a-0000-7000-8000-000000000001." });
    await writeFile(path, original.replace("000001/architect: codex-first", "000001/architect/extra: codex-first"));
    const malformed = await adapter.read({ sourceId: "authoring" });
    expect(malformed.status).toBe("unavailable");
    expect(malformed.diagnostics[0]?.message).toContain("Malformed");
  });

  it("rejects duplicate template versions/slots and reports a handoff to an unknown slot", async () => {
    const { adapter, directory } = await isolatedSources(false);
    const path = join(directory, "mvp-team-pack.json");
    const original = await readFile(path, "utf8");
    await writeFile(path, original.replace('"version": 2', '"version": 1'));
    expect((await adapter.read({ sourceId: "templates" })).diagnostics[0]?.message).toContain("Duplicate team template ID/version");
    await writeFile(path, original.replace('"id": "researcher-b"', '"id": "researcher-a"'));
    expect((await adapter.read({ sourceId: "templates" })).diagnostics[0]?.message).toContain("Duplicate template slot");
    await writeFile(path, original.replace('"to_slot": "builder"', '"to_slot": "unknown-slot"'));
    const snapshot = await adapter.read({ sourceId: "templates" });
    expect(snapshot.status).toBe("partial");
    expect(snapshot.diagnostics[0]?.code).toBe("unknown-slot");
  });

  it("enforces source ID only requests and host-selected catalog IDs/formats", async () => {
    const { adapter, catalog, formats } = await isolatedSources();
    for (const input of [{ sourceId: "/tmp/credentials" }, { sourceId: "authoring", path: "/tmp/credentials" },
      { sourceId: "authoring", format: "fleet-operator" }, { sourceId: "missing" }, { sourceId: "toString" }]) {
      await expect(adapter.read(input)).rejects.toThrow();
      expect(readTeamsSource.input.safeParse(input).success && input.sourceId === "authoring").toBe(false);
    }
    expect(() => createTeamsSourceAdapter(catalog, { undeclared: "fleet-authoring" })).toThrow("Expected one configured");
    expect(() => createTeamsSourceAdapter({ ...catalog, sources: [...catalog.sources, catalog.sources[0]!] }, formats)).toThrow("Expected one configured");
    expect(() => createTeamsSourceAdapter({ ...catalog, sources: catalog.sources.map((source) => ({ ...source, kind: "plan" })) }, formats)).toThrow();
    await expect(createTeamsSourceAdapter(catalog, {}).read({ sourceId: "authoring" })).rejects.toThrow("not an authorized");
    // Adapter freezes a copy; caller mutation cannot redirect a later read to another file.
    catalog.sources[0]!.path = "/tmp/not-a-teams-source";
    expect((await adapter.read({ sourceId: "authoring" })).status).toBe("ready");
  });

  it("hashes bytes, refuses revision drift/oversize/non-file/invalid UTF-8 and permits declared symlinks", async () => {
    const { adapter, directory, catalog, formats } = await isolatedSources();
    const path = join(directory, "authoring-fleet.yml");
    await writeFile(path, "changed\n");
    const drift = await adapter.read({ sourceId: "authoring" });
    expect(drift).toMatchObject({ status: "unavailable", data: null, sha256: hash(Buffer.from("changed\n")) });
    expect(drift.diagnostics[0]?.code).toBe("revision-mismatch");
    await writeFile(path, "x".repeat(2 * 1024 * 1024 + 1));
    expect((await adapter.read({ sourceId: "authoring" })).diagnostics[0]?.code).toBe("source-size-or-type");
    const entry = catalog.sources[0]!;
    entry.expectedSha256 = null;
    const unpinned = createTeamsSourceAdapter(catalog, formats);
    await writeFile(path, Buffer.from([0xff]));
    expect((await unpinned.read({ sourceId: "authoring" })).data).toBeNull();
    await rm(path);
    await symlink(directory, path);
    expect((await unpinned.read({ sourceId: "authoring" })).diagnostics[0]?.code).toBe("source-size-or-type");
    await rm(path);
    await symlink(fixturePath("authoring-fleet.yml"), path);
    expect((await unpinned.read({ sourceId: "authoring" })).status).toBe("ready");
    entry.revisionKind = "approved revision";
    expect((await createTeamsSourceAdapter(catalog, formats).read({ sourceId: "authoring" })).diagnostics[0]?.code).toBe("revision-unpinned");
  });
});

describe("bounded configuration YAML dialect", () => {
  it("handles actual anchor forms, quoted comments, YAML 1.2 off and indentationless roster lists", () => {
    expect(decodeTeamsConfigYaml('x: &free [one@high, two@max]\ny: *free\nz: { text: "# literal", efforts: ["off", off], flag: false }\nseats:\n- id: lsa\n  presence: required\n'))
      .toEqual({ x: ["one@high", "two@max"], y: ["one@high", "two@max"], z: { text: "# literal", efforts: ["off", "off"], flag: false }, seats: [{ id: "lsa", presence: "required" }] });
  });
  it.each(["x: 1\nx: 2", "x: *forward", "x: &loop [*loop]", "x: &nested [[a]]", "x: !!js/function evil", "x: |\n  multiline", "x: { <<: *a }",
    "x: [a,,b]", "x: [a}", 'x: "unterminated', "x:\n\tbad: true", "---\nx: 1", "x: {a: 1, a: 2}"])("refuses unsupported or ambiguous syntax: %s", (text) => {
    expect(() => decodeTeamsConfigYaml(text)).toThrow();
  });
  it("bounds nesting and alias expansion", () => {
    expect(() => decodeTeamsConfigYaml(`x: ${"[".repeat(40)}a${"]".repeat(40)}`)).toThrow("limit");
    expect(() => decodeTeamsConfigYaml(`x: &a [${Array(500).fill("m").join(", ")}]\n${Array.from({ length: 250 }, (_, index) => `s${index}: *a`).join("\n")}`)).toThrow("limit");
  });
});
