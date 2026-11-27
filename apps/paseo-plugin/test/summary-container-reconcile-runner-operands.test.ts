import { mkdtemp, readdir, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";

// Only the client is stubbed and counted; the runner and the recorder are the real modules.
const spy = { clients: 0, connects: 0 };
vi.mock("@getpaseo/client", () => ({
  createPaseoClient: () => {
    spy.clients++;
    return { connect: async () => { spy.connects++; }, close: async () => {}, agents: { ref: () => ({ refresh: async () => null }) } };
  },
}));

const argv = process.argv;
const dirs: string[] = [];
afterEach(async () => {
  process.argv = argv; spy.clients = 0; spy.connects = 0; vi.resetModules(); vi.restoreAllMocks();
  for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true });
});

async function run(root: string, ...args: string[]) {
  process.argv = ["bun", "scripts/summary-container-reconcile.ts", "6767", root, "record-recovery", ...args];
  vi.spyOn(process.stdout, "write").mockImplementation(() => true);
  await import("../scripts/summary-container-reconcile");
}

describe("summary reconcile runner recovery operands (ASMA-8580 round 3, item 4)", () => {
  const JOB = `${"d".repeat(64)}.json`;

  it.each([
    ["a path escape", [`../${JOB}`, "retire-legacy-create", "evidence"], "Name one job file"],
    ["a missing job", [], "Name one job file"],
    ["an unsupported disposition", [JOB, "retry", "evidence"], "retire-legacy-create"],
    ["blank evidence", [JOB, "retire-legacy-create", "   "], "non-blank evidence"],
  ] as const)("refuses %s before any client exists", async (_name, args, message) => {
    const root = await mkdtemp(join(tmpdir(), "summary-operands-")); dirs.push(root);
    await expect(run(root, ...args)).rejects.toThrow(message);
    expect(spy).toEqual({ clients: 0, connects: 0 });
    expect(await readdir(root)).toEqual([]);
  });

  it("reaches the client only with valid operands; the job's state is then read through it", async () => {
    const root = await mkdtemp(join(tmpdir(), "summary-operands-")); dirs.push(root);
    await expect(run(root, JOB, "retire-legacy-create", "evidence")).rejects.toThrow(/ENOENT/);
    expect(spy).toEqual({ clients: 1, connects: 1 });
  });
});
