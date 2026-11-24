import { createHash, randomUUID } from "node:crypto";
import { mkdir, mkdtemp, readdir, readFile, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { afterEach, describe, expect, it } from "vitest";
import { reconcileSummaryContainers } from "../server/summary-container-reconcile";

const SECTIONS = { task: "t", decisions: "d", currentState: "c", nextStep: "n" };
const dirs: string[] = [];
afterEach(async () => { for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true }); });

type Agent = { id: string; workspaceId?: string; cwd: string; archivedAt: string | null };

/** Real job, capture and summary-store files; a read-only host fake with no mutation method at all. */
async function fixture() {
  const root = await mkdtemp(join(tmpdir(), "summary-reconcile-")); dirs.push(root);
  const jobs = join(root, "generation-jobs");
  await mkdir(jobs, { recursive: true });
  const workspaces: Array<Record<string, unknown>> = [];
  const agents: Agent[] = [];
  const terminals = new Map<string, number>();
  const reads: string[] = [];
  /** One legacy container: per-job project and workspace rooted at generation-jobs/<helper>, its helper archived inside. */
  async function container(options: { state?: string; cleaned?: boolean | null; saved?: boolean; helper?: "archived" | "active" | "absent"; job?: boolean; previous?: boolean } = {}) {
    const helperId = randomUUID(), jobKey = createHash("sha256").update(helperId).digest("hex"), source = `source-${helperId}`;
    const directory = join(jobs, helperId), workspaceId = `wks_${helperId.slice(0, 8)}`, projectId = `prj_${helperId.slice(0, 8)}`;
    const snapshotHash = createHash("sha256").update(source).digest("hex");
    const boundary = { epoch: "e", fromSeq: 1, toSeq: 2, snapshotHash, partial: false, provider: "codex", providerSessionId: null };
    if (options.job !== false) {
      const workerId = options.previous ? randomUUID() : helperId;
      await writeFile(join(jobs, `${jobKey}.json`), JSON.stringify({ workerId, state: options.state ?? "complete", previousWorkers: options.previous ? [helperId] : [],
        clientMessageId: randomUUID(), promptHash: "h", ...((options.state ?? "complete") === "complete" ? { sections: SECTIONS } : {}),
        ...(options.cleaned === null ? {} : { cleaned: options.cleaned ?? true }), promptVersion: 2 }));
      await writeFile(join(jobs, `${jobKey}.capture.json`), JSON.stringify({ sourceAgentId: source, historyBoundary: boundary }));
    }
    if (options.saved !== false) {
      const summaryId = randomUUID();
      const revision = { summaryId, revision: 1, sourceAgentId: source, sourceTitle: "S", cutoff: "2026-11-01T00:00:00Z", savedAt: "2026-11-01T00:00:01Z", historyBoundary: boundary, sections: SECTIONS, text: "x" };
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
      list: async (options: { filter: { projectKeys: string[]; includeArchived: boolean; includeBackground: boolean } }) => {
        reads.push(`agents:${options.filter.projectKeys.join(",")}:${options.filter.includeArchived}:${options.filter.includeBackground}`);
        const ids = new Set(workspaces.filter(workspace => options.filter.projectKeys.includes(workspace.projectId as string)).map(workspace => workspace.id));
        return page(agents.filter(agent => ids.has(agent.workspaceId ?? "") || [...ids].some(id => workspaces.find(w => w.id === id)?.workspaceDirectory === agent.cwd)).map(agent => ({ agent, project: null })));
      },
    },
  };
  const run = () => reconcileSummaryContainers(paseo as never, root, "127.0.0.1:6767");
  return { root, jobs, workspaces, agents, terminals, reads, container, run };
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
    expect(f.reads).toEqual(["workspaces:true", `agents:${ok.projectId}:true:true`, `terminals:${ok.workspaceId}`]);
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
      agents: { list: async () => { throw new Error("down"); } } } as never, g.root);
    expect((await run).rows[0]).toMatchObject({ closeout: null, occupants: null, terminals: null, holds: ["Its occupants could not be read.", "Its terminals could not be read."] });
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

  it("treats an unreadable job record as naming no helper", async () => {
    const f = await fixture();
    const held = await f.container();
    const name = (await readdir(f.jobs)).find(file => file.endsWith(".json") && !file.endsWith(".capture.json"))!;
    await writeFile(join(f.jobs, name), "{" + await readFile(join(f.jobs, name), "utf8"));
    const [row] = (await f.run()).rows;
    expect(row).toMatchObject({ helperId: held.helperId, jobState: "unknown", closeout: null });
  });
});
