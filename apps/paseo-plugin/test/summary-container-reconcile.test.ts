import { createHash, randomUUID } from "node:crypto";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { afterEach, describe, expect, it } from "vitest";
import { reconcileSummaryContainers, recordSummaryRecovery } from "../server/summary-container-reconcile";
import { summaryJobSchema } from "../server/stock-summary-generator";

const SECTIONS = { task: "t", decisions: "d", currentState: "c", nextStep: "n" };
const dirs: string[] = [];
afterEach(async () => { for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true }); });

type Agent = { id: string; workspaceId?: string; cwd: string; archivedAt: string | null };
type Fixture = Awaited<ReturnType<typeof fixture>>;
type Container = Awaited<ReturnType<Fixture["container"]>>;

/** Real job, capture and summary-store files; a read-only host fake with no mutation method at all. */
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), "summary-reconcile-")); dirs.push(root);
  const jobs = join(root, "generation-jobs");
  await mkdir(jobs, { recursive: true });
  const workspaces: Array<Record<string, unknown>> = [];
  const agents: Agent[] = [];
  const terminals = new Map<string, number>();
  const reads: string[] = [];
  const helperProjects = new Map<string, string>();
  /** One legacy container: per-job project and workspace rooted at generation-jobs/<helper>, its helper archived inside. */
  async function container(options: { state?: string; cleaned?: boolean | null; saved?: boolean; helper?: "archived" | "active" | "absent"; job?: boolean; previous?: boolean;
    placement?: boolean; savedBoundary?: Record<string, unknown>; savedSections?: Record<string, string>; savedCutoff?: string; savedSource?: string; capture?: "missing" | "corrupt" } = {}) {
    const helperId = randomUUID(), source = `source-${helperId}`;
    const directory = join(jobs, helperId), workspaceId = `wks_${helperId.slice(0, 8)}`, projectId = `prj_${helperId.slice(0, 8)}`;
    const snapshotHash = createHash("sha256").update(source).digest("hex"), cutoff = "2026-11-01T00:00:00Z";
    const boundary = { epoch: "e", fromSeq: 1, toSeq: 2, snapshotHash, partial: false, provider: "codex", providerSessionId: null };
    // The generator's key: the job and capture files are named by sha256([source, boundary]).
    const jobKey = createHash("sha256").update(JSON.stringify([source, boundary])).digest("hex");
    if (options.job !== false) {
      const workerId = options.previous ? randomUUID() : helperId;
      await writeFile(join(jobs, `${jobKey}.json`), JSON.stringify({ workerId, state: options.state ?? "complete", previousWorkers: options.previous ? [helperId] : [],
        clientMessageId: randomUUID(), promptHash: "h", ...((options.state ?? "complete") === "complete" ? { sections: SECTIONS } : {}),
        ...(options.placement ? { placement: { workspaceId, projectId, cwd: directory, owned: false } } : {}),
        ...(options.cleaned === null ? {} : { cleaned: options.cleaned ?? true }), promptVersion: 2 }));
      if (options.capture !== "missing") await writeFile(join(jobs, `${jobKey}.capture.json`), options.capture === "corrupt" ? "{\"sourceAgentId\":" : JSON.stringify({ sourceAgentId: source, cutoff, historyBoundary: boundary }));
    }
    if (options.saved !== false) {
      const summaryId = randomUUID();
      const revision = { summaryId, revision: 1, sourceAgentId: options.savedSource ?? source, sourceTitle: "S", cutoff: options.savedCutoff ?? cutoff, savedAt: "2026-11-01T00:00:01Z",
        historyBoundary: { ...boundary, ...options.savedBoundary }, sections: options.savedSections ?? SECTIONS, text: "x" };
      await writeFile(join(root, `source-${createHash("sha256").update(source).digest("hex")}.json`), JSON.stringify({ summaryId, sourceAgentId: source, sourceTitle: "S", latestRevision: 1, revisions: { "1": revision } }));
    }
    workspaces.push({ id: workspaceId, projectId, projectRootPath: directory, workspaceDirectory: directory });
    if ((options.helper ?? "archived") !== "absent") agents.push({ id: helperId, workspaceId, cwd: directory, archivedAt: options.helper === "active" ? null : "2026-11-01T00:00:02Z" });
    return { helperId, workspaceId, projectId, directory };
  }
  const page = <T>(entries: T[]) => ({ requestId: "r", entries, pageInfo: { hasMore: false, nextCursor: null } });
  const paseo = {
    workspaces: {
      list: async (options: { includeBackground?: boolean }) => { reads.push(`workspaces:${options.includeBackground}`); return page(workspaces); },
      ref: (id: string) => ({ terminals: { list: async () => { reads.push(`terminals:${id}`); return { requestId: "r", entries: Array.from({ length: terminals.get(id) ?? 0 }, (_, index) => ({ id: `t${index}` })) }; } } }),
    },
    agents: {
      ref: (id: string) => ({ refresh: async () => {
        reads.push(`helper:${id}`);
        const agent = agents.find(entry => entry.id === id);
        const workspace = workspaces.find(entry => entry.id === agent?.workspaceId);
        return agent ? { agent, project: workspace ? { projectKey: helperProjects.get(id) ?? workspace.projectId } : null } : null;
      } }),
      list: async (options: { filter: { projectKeys: string[]; includeArchived: boolean; includeBackground: boolean } }) => {
        reads.push(`agents:${options.filter.projectKeys.join(",")}:${options.filter.includeArchived}:${options.filter.includeBackground}`);
        const ids = new Set(workspaces.filter(workspace => options.filter.projectKeys.includes(workspace.projectId as string)).map(workspace => workspace.id));
        return page(agents.filter(agent => options.filter.includeArchived || !agent.archivedAt).filter(agent => ids.has(agent.workspaceId ?? "") || [...ids].some(id => workspaces.find(w => w.id === id)?.workspaceDirectory === agent.cwd)).map(agent => ({ agent, project: null })));
      },
    },
  };
  const run = () => reconcileSummaryContainers(paseo as never, root, "127.0.0.1:6767");
  return { root, jobs, workspaces, agents, terminals, reads, helperProjects, container, run };
}

describe("legacy summary container dry-run reconcile (ASMA-8580)", () => {
  it("proposes the supported closeout only for a complete, cleaned, saved container holding only its archived helper", async () => {
    const f = await fixture();
    const ok = await f.container();
    const before = await readdir(f.root);
    const result = await f.run();
    expect(result).toMatchObject({ dryRun: true, executed: false, containers: 1, closeout: 1, held: 0 });
    expect(result.rows[0]).toMatchObject({ workspaceId: ok.workspaceId, helperId: ok.helperId, jobState: "complete", cleaned: true, summarySaved: true, terminals: 0, holds: [],
      occupants: [{ agentId: ok.helperId, own: true, archived: true }], closeout: `paseo workspace archive ${ok.workspaceId} --json --host 127.0.0.1:6767` });
    expect(result.projectResidual).toContain("not proposed");
    expect(await readdir(f.root)).toEqual(before);
    expect(f.reads).toEqual(["workspaces:true", `helper:${ok.helperId}`, `agents:${ok.projectId}:true:true`, `terminals:${ok.workspaceId}`]);
  });

  it("closes out an earlier attempt's container of a complete job", async () => {
    const f = await fixture();
    await f.container({ previous: true });
    expect((await f.run()).rows[0]!.closeout).toMatch(/^paseo workspace archive /);
  });

  it.each([
    ["waiting", { state: "waiting", cleaned: null }, "The job is waiting"],
    ["creating", { state: "creating", cleaned: null, helper: "absent" as const }, "The job is creating"],
    ["failed", { state: "failed" }, "The job is failed"],
    ["unknown", { job: false }, "No job record names this helper (unknown)."],
    ["unconfirmed archive", { cleaned: false }, "no confirmed helper archive"],
    ["unsaved summary", { saved: false }, "No saved summary revision"],
    ["helper still active", { helper: "active" as const }, "is not archived"],
    ["helper missing", { helper: "absent" as const }, "Its helper does not resolve in this container."],
  ])("holds a %s container with no closeout", async (_name, options, reason) => {
    const f = await fixture();
    await f.container(options);
    const [row] = (await f.run()).rows;
    expect(row!.closeout).toBeNull();
    expect(row!.holds.join(" ")).toContain(reason);
  });

  it("holds a cleaned=true container with a foreign occupant, archived or not, and names it", async () => {
    const f = await fixture();
    const live = await f.container(), archived = await f.container();
    f.agents.push({ id: "foreign-live", workspaceId: live.workspaceId, cwd: live.directory, archivedAt: null });
    f.agents.push({ id: "foreign-archived", cwd: archived.directory, archivedAt: "2026-11-02T00:00:00Z" });
    const result = await f.run();
    expect(result).toMatchObject({ containers: 2, closeout: 0, held: 2 });
    expect(result.rows[0]!.holds).toEqual(["Foreign occupant foreign-live."]);
    expect(result.rows[1]!.holds).toEqual(["Foreign occupant foreign-archived."]);
  });

  it("holds a container with an open terminal and one whose occupants or terminals cannot be read", async () => {
    const f = await fixture();
    const terminal = await f.container();
    f.terminals.set(terminal.workspaceId, 1);
    expect((await f.run()).rows[0]!.holds).toEqual(["1 terminal(s) open."]);
    const g = await fixture();
    await g.container();
    (g as unknown as { agents: unknown }).agents = null;
    const run = reconcileSummaryContainers({ workspaces: { list: async () => ({ entries: g.workspaces, pageInfo: { hasMore: false } }), ref: () => ({ terminals: { list: async () => { throw new Error("gone"); } } }) },
      agents: { ref: () => ({ refresh: async () => { throw new Error("down"); } }), list: async () => { throw new Error("down"); } } } as never, g.root);
    expect((await run).rows[0]).toMatchObject({ closeout: null, occupants: null, terminals: null,
      holds: ["The helper's own readback does not bind this workspace and project.", "Its occupants could not be read.", "Its terminals could not be read."] });
  });

  it("holds a container whose occupants alone cannot be read: no closeout is proposed", async () => {
    const f = await fixture();
    await f.container();
    const run = reconcileSummaryContainers({ workspaces: { list: async () => ({ entries: f.workspaces, pageInfo: { hasMore: false } }), ref: () => ({ terminals: { list: async () => ({ entries: [] }) } }) },
      agents: { ref: (id: string) => ({ refresh: async () => ({ agent: f.agents.find(agent => agent.id === id), project: { projectKey: f.workspaces[0]!.projectId } }) }), list: async () => { throw new Error("down"); } } } as never, f.root);
    const [row] = (await run).rows;
    expect(row!.closeout).toBeNull();
    expect(row!.holds).toEqual(["Its occupants could not be read."]);
  });

  it("ignores workspaces outside the job directory, including a nested or sibling path", async () => {
    const f = await fixture();
    f.workspaces.push({ id: "wks_desk", projectId: "prj_desk", projectRootPath: "/fixture/desk", workspaceDirectory: "/fixture/desk" },
      { id: "wks_nested", projectId: "prj_n", projectRootPath: "/x", workspaceDirectory: join(f.jobs, randomUUID(), "nested") },
      { id: "wks_sibling", projectId: "prj_s", projectRootPath: "/x", workspaceDirectory: join(f.root, randomUUID()) });
    expect((await f.run()).containers).toBe(0);
  });

  it("holds a job-directory workspace whose name is not a helper id", async () => {
    const f = await fixture();
    f.workspaces.push({ id: "wks_odd", projectId: "prj_odd", projectRootPath: join(f.jobs, "superseded"), workspaceDirectory: join(f.jobs, "superseded") });
    const [row] = (await f.run()).rows;
    expect(row!.holds).toContain("The directory is not a helper id.");
    expect(row!.closeout).toBeNull();
  });

  it("always holds a job that records a designated placement it does not own (explicit-nonowned-container)", async () => {
    const f = await fixture();
    const held = await f.container({ placement: true });
    const [row] = (await f.run()).rows;
    expect(row).toMatchObject({ workspaceId: held.workspaceId, closeout: null });
    expect(row!.holds).toEqual(["The job records a designated workspace it does not own (owned: false)."]);
  });

  it.each([
    ["in the same project (same-directory-second-workspace)", (projectId: string) => projectId],
    ["in another project", () => "prj_other"],
  ])("proposes only the workspace the helper is bound to when a sibling shares its directory %s", async (_name, siblingProject) => {
    const f = await fixture();
    const owned = await f.container();
    f.workspaces.push({ id: "wks_sibling", projectId: siblingProject(owned.projectId), projectRootPath: owned.directory, workspaceDirectory: owned.directory });
    const result = await f.run();
    expect(result).toMatchObject({ containers: 2, closeout: 1, held: 1 });
    const [bound, sibling] = [result.rows.find(row => row.workspaceId === owned.workspaceId)!, result.rows.find(row => row.workspaceId === "wks_sibling")!];
    expect(bound).toMatchObject({ holds: [], closeout: `paseo workspace archive ${owned.workspaceId} --json --host 127.0.0.1:6767` });
    expect(sibling.closeout).toBeNull();
    expect(sibling.holds).toContain("The helper's own readback does not bind this workspace and project.");
    expect(sibling.occupants).toEqual([{ agentId: owned.helperId, own: false, archived: true }]);
  });

  it.each([
    ["another workspace", (f: Fixture, c: Container) => { f.agents.find(a => a.id === c.helperId)!.workspaceId = "wks_elsewhere"; }],
    ["another project", (f: Fixture, c: Container) => { f.helperProjects.set(c.helperId, "prj_elsewhere"); }],
    ["another directory", (f: Fixture, c: Container) => { f.agents.find(a => a.id === c.helperId)!.cwd = "/fixture/elsewhere"; }],
  ])("holds the container when the helper's own readback names %s", async (_name, mutate) => {
    const f = await fixture();
    const c = await f.container();
    mutate(f, c);
    const row = (await f.run()).rows.find(entry => entry.workspaceId === c.workspaceId)!;
    expect(row.closeout).toBeNull();
    expect(row.holds).toContain("The helper's own readback does not bind this workspace and project.");
  });

  it.each([
    ["a different epoch", { savedBoundary: { epoch: "e2" } }],
    ["a different provider session", { savedBoundary: { providerSessionId: "session-2" } }],
    ["a different sequence range", { savedBoundary: { toSeq: 3 } }],
    ["a partial flag", { savedBoundary: { partial: true } }],
    ["a different cutoff", { savedCutoff: "2026-11-02T00:00:00Z" }],
    ["other sections", { savedSections: { ...SECTIONS, nextStep: "other" } }],
    ["another source on the revision", { savedSource: "another-source" }],
  ])("holds a container whose only saved revision has the same input hash but %s", async (_name, options) => {
    const f = await fixture();
    await f.container(options);
    const [row] = (await f.run()).rows;
    expect(row).toMatchObject({ summarySaved: false, closeout: null });
    expect(row!.holds).toEqual(["No saved summary revision matches the job's capture."]);
  });

  it("accepts the matching full boundary as saved (control)", async () => {
    const f = await fixture();
    await f.container({ savedBoundary: {} });
    expect((await f.run()).rows[0]).toMatchObject({ summarySaved: true, holds: [] });
  });

  it.each([["missing", "missing"], ["unreadable", "corrupt"]] as const)("holds a complete, cleaned container whose capture is %s", async (_name, capture) => {
    const f = await fixture();
    await f.container({ capture });
    const [row] = (await f.run()).rows;
    expect(row).toMatchObject({ summarySaved: false, closeout: null });
    expect(row!.holds).toEqual(["No saved summary revision matches the job's capture."]);
  });

  it("holds a capture that is not the job's own, even when a saved revision matches it", async () => {
    const f = await fixture();
    await f.container({ savedBoundary: { fromSeq: 0 } });
    const name = (await readdir(f.jobs)).find(file => file.endsWith(".capture.json"))!;
    const capture = JSON.parse(await readFile(join(f.jobs, name), "utf8"));
    await writeFile(join(f.jobs, name), JSON.stringify({ ...capture, historyBoundary: { ...capture.historyBoundary, fromSeq: 0 } }));
    expect((await f.run()).rows[0]!.holds).toEqual(["No saved summary revision matches the job's capture."]);
  });

  it("treats an unreadable job record as naming no helper", async () => {
    const f = await fixture();
    const held = await f.container();
    const name = (await readdir(f.jobs)).find(file => file.endsWith(".json") && !file.endsWith(".capture.json"))!;
    await writeFile(join(f.jobs, name), "{" + await readFile(join(f.jobs, name), "utf8"));
    const [row] = (await f.run()).rows;
    expect(row).toMatchObject({ helperId: held.helperId, jobState: "unknown", closeout: null });
  });
});

describe("summary recovery recording (ASMA-8580 round 2, item 3)", () => {
  const sha = (bytes: Buffer | string) => createHash("sha256").update(bytes).digest("hex");
  const NAME = `${"a".repeat(64)}.json`, WORKER = "0b6f2c1e-5d4a-4c3b-9a8f-7e6d5c4b3a21";
  const EVIDENCE = "evidence/installer/RECEIPT-TRIAGE.json sha256 8a4e531b";
  /** One job file in the earliest released shape, and a host whose agent reads are the only call. */
  async function held(job: Record<string, unknown> = {}, format = (value: object) => JSON.stringify(value), helper: object | null = null) {
    const root = await mkdtemp(join(tmpdir(), "summary-recovery-")); dirs.push(root);
    const jobs = join(root, "generation-jobs"); await mkdir(jobs, { recursive: true });
    const text = format({ workerId: WORKER, state: "creating", previousWorkers: [], clientMessageId: "7f1d2c3b-0a9e-4d8c-b7a6-5f4e3d2c1b0a", promptHash: "p".repeat(64), ...job });
    await writeFile(join(jobs, NAME), text);
    const reads: string[] = [];
    const paseo = { agents: { ref: (id: string) => ({ refresh: async () => { reads.push(id); return helper; } }) } };
    const record = (overrides: Partial<Parameters<typeof recordSummaryRecovery>[2]> = {}) =>
      recordSummaryRecovery(paseo as never, root, { job: NAME, disposition: "retire-legacy-create", evidence: EVIDENCE, now: () => "2026-11-25T08:00:00Z", ...overrides });
    return { root, jobs, file: join(jobs, NAME), text, reads, record };
  }

  it("is a dry run by default: the plan names the bytes it keeps, and nothing changes", async () => {
    const h = await held();
    expect(await h.record()).toEqual({ job: NAME, workerId: WORKER, beforeSha256: sha(h.text), archive: `superseded/${WORKER}.before-recovery.json`,
      recovery: { disposition: "retire-legacy-create", evidence: EVIDENCE, recordedAt: "2026-11-25T08:00:00Z" }, applied: false, unchanged: false });
    expect(await readFile(h.file, "utf8")).toBe(h.text);
    expect(await readdir(h.jobs)).toEqual([NAME]);
    expect(h.reads).toEqual([WORKER]);
  });

  it.each([
    ["compact", (value: object) => JSON.stringify(value)],
    ["compact, in another field order", (value: object) => JSON.stringify({ promptHash: (value as { promptHash: string }).promptHash, ...value })],
    ["pretty-printed", (value: object) => JSON.stringify(value, null, 2) + "\n"],
  ] as const)(
    "on --apply keeps the %s original bytes first, then adds only the recovery; an identical repeat changes nothing", async (_name, format) => {
      const h = await held({}, format);
      const applied = await h.record({ applySha256: sha(h.text) });
      expect(applied).toMatchObject({ applied: true, beforeSha256: sha(h.text) });
      const kept = await readFile(join(h.jobs, "superseded", `${WORKER}.before-recovery.json`));
      expect(sha(kept)).toBe(sha(h.text));
      const after = await readFile(h.file, "utf8");
      // The exact bytes: the original fields in their own order and values, then only the recovery.
      expect(after).toBe(JSON.stringify({ ...JSON.parse(h.text), recovery: { disposition: "retire-legacy-create", evidence: EVIDENCE, recordedAt: "2026-11-25T08:00:00Z" } }));
      expect(sha(after)).toBe((applied as { afterSha256: string }).afterSha256);
      const before = JSON.parse(h.text), now = JSON.parse(after);
      expect(Object.keys(now)).toEqual([...Object.keys(before), "recovery"]);
      for (const key of Object.keys(before)) expect(now[key]).toEqual(before[key]);
      expect(now.recovery).toEqual({ disposition: "retire-legacy-create", evidence: EVIDENCE, recordedAt: "2026-11-25T08:00:00Z" });
      expect(summaryJobSchema.parse(now).recovery).toBeDefined();
      expect(await h.record({ applySha256: sha(h.text), now: () => "2026-11-26T00:00:00Z" })).toMatchObject({ applied: false, unchanged: true });
      expect(await readFile(h.file, "utf8")).toBe(after);
      expect(sha(await readFile(join(h.jobs, "superseded", `${WORKER}.before-recovery.json`)))).toBe(sha(h.text));
      expect((await readdir(h.jobs)).sort()).toEqual([NAME, "superseded"]);
    });

  it.each([
    ["a job that is not creating", { job: { state: "waiting" } }, {}, "only a held creating job"],
    ["a job whose helper reads back", { helper: { agent: { id: WORKER } } }, {}, "reads back on this host"],
    ["blank evidence", {}, { evidence: "  " }, "non-blank evidence"],
    ["another disposition", {}, { disposition: "retry" }, 'only recovery disposition is "retire-legacy-create"'],
    ["a path instead of a job file", {}, { job: `../${"a".repeat(64)}.json` }, "Name one job file"],
    ["bytes that are not the ones the dry run reviewed", {}, { applySha256: "0".repeat(64) }, "is not the bytes its dry run reviewed"],
    ["another recovery already recorded", { job: { recovery: { disposition: "retire-legacy-create", evidence: "other", recordedAt: "2026-11-24T00:00:00Z" } } }, {}, "already records another recovery"],
  ] as const)("refuses %s with --apply and changes nothing", async (_name, setup, overrides, message) => {
    const h = await held((setup as { job?: Record<string, unknown> }).job, undefined, (setup as { helper?: object }).helper ?? null);
    // A valid job one level up: a name that escapes generation-jobs would reach it.
    await writeFile(join(h.root, NAME), h.text);
    const error = await h.record({ applySha256: sha(h.text), ...overrides }).catch((cause: Error) => cause);
    expect(await readFile(h.file, "utf8")).toBe(h.text);
    expect(await readFile(join(h.root, NAME), "utf8")).toBe(h.text);
    expect(await readdir(h.jobs)).toEqual([NAME]);
    expect(String(error)).toContain(message);
  });

  it("holds, and never overwrites, an existing pre-recovery archive with other bytes", async () => {
    const h = await held();
    await mkdir(join(h.jobs, "superseded"));
    await writeFile(join(h.jobs, "superseded", `${WORKER}.before-recovery.json`), "earlier evidence");
    const error = await h.record({ applySha256: sha(h.text) }).catch((cause: Error) => cause);
    expect(await readFile(join(h.jobs, "superseded", `${WORKER}.before-recovery.json`), "utf8")).toBe("earlier evidence");
    expect(await readFile(h.file, "utf8")).toBe(h.text);
    expect(String(error)).toContain("already holds other evidence");
  });
});
