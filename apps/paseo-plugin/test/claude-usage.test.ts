import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { appendFile, mkdir, mkdtemp, readFile, readdir, rm, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { usageTotalsSchema } from "../shared/session-navigator";
import { createClaudeUsageReader } from "../server/claude-usage";

let root: string, projects: string, cache: string;
const ID = "00000000-0000-4000-8000-000000000001";
const usage = (input: number, output: number, cacheRead = 0, cacheCreation = 0) => ({ input_tokens: input, output_tokens: output, cache_read_input_tokens: cacheRead, cache_creation_input_tokens: cacheCreation });
const line = (id: string | null, value = usage(10, 2), sidechain = false) => JSON.stringify({ type: "assistant", isSidechain: sidechain, message: { id, usage: value } });
const lines = (...rows: string[]) => rows.join("\n") + "\n";
const file = (project = "project-a", id = ID) => join(projects, project, `${id}.jsonl`);

beforeEach(async () => {
  root = await mkdtemp(join(tmpdir(), "kontor-claude-usage-"));
  projects = join(root, "projects");
  cache = join(root, "cache");
  await mkdir(join(projects, "project-a"), { recursive: true });
});
afterEach(async () => { await rm(root, { recursive: true, force: true }); });
const reader = () => createClaudeUsageReader(projects, cache);

describe("qualified Claude assistant-message totals", () => {
  it("sums one exact JSONL, excludes sidechains/subagents, and maps cache reads/creation into input", async () => {
    await writeFile(file(), lines(
      line("msg-1", usage(2, 5, 100, 10)),
      JSON.stringify({ type: "user", message: { id: "user-1", usage: usage(9, 9) } }),
      line("msg-2", usage(999, 999), true),
      line("msg-1", usage(2, 5, 100, 10)),
      line("msg-3", usage(7, 3)),
    ));
    const result = await reader().read(ID);
    expect(result).toMatchObject({ input: 119, output: 8, scope: "provider-cumulative" });
    expect(usageTotalsSchema.parse(result)).toEqual(result);
    expect(result.source).toContain("sidechains and subagents are excluded");
    expect(result.source).toContain("cache reads and cache creation");
  });

  it("replaces a repeated message ID with its last usage and replays idempotently across restarts", async () => {
    await writeFile(file(), lines(line("msg-1", usage(10, 1)), line("msg-2", usage(20, 2))));
    expect(await reader().read(ID)).toMatchObject({ input: 30, output: 3 });
    await appendFile(file(), lines(line("msg-1", usage(100, 10)), line("msg-2", usage(20, 2)), line("msg-1", usage(100, 10))));
    expect(await reader().read(ID)).toMatchObject({ input: 120, output: 12 });
    expect(await reader().read(ID)).toMatchObject({ input: 120, output: 12 });
  });

  it("persists only numeric totals and hashed seen IDs, not transcript content", async () => {
    const secret = "PRIVATE_CLAUDE_TRANSCRIPT";
    await writeFile(file(), lines(JSON.stringify({ type: "assistant", isSidechain: false, message: { id: "msg-secret", usage: usage(1, 2) }, payload: { text: secret } })));
    await reader().read(ID);
    const stored = await readFile(join(cache, (await readdir(cache))[0]!), "utf8");
    expect(stored).not.toContain(secret); expect(stored).not.toContain("msg-secret");
    expect(stored).not.toContain(root);
    expect(stored).toContain('"input":1');
  });

  it("returns unknown for a missing or ambiguous exact file", async () => {
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null, source: expect.stringContaining("no exact source") });
    await mkdir(join(projects, "project-b"), { recursive: true });
    await writeFile(file("project-a"), lines(line("msg-1")));
    await writeFile(file("project-b"), lines(line("msg-2")));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null, source: expect.stringContaining("ambiguous exact source") });
  });

  it("keeps invalid identities, malformed records and oversized lines unknown", async () => {
    const cases = [
      { id: "00000000-0000-4000-8000-000000000011", lines: lines(line(null), line("msg-1")), source: "invalid message identity" },
      { id: "00000000-0000-4000-8000-000000000012", lines: lines('{"type":"assistant","message":'), source: "malformed usage record" },
      { id: "00000000-0000-4000-8000-000000000013", lines: lines(line("msg-1", usage(-1, 2))), source: "invalid usage record" },
      { id: "00000000-0000-4000-8000-000000000014", lines: lines(line("msg-1")) + JSON.stringify({ type: "assistant", message: { id: "msg-big", usage: usage(1, 1) }, payload: "x".repeat(17 * 1024 * 1024) }) + "\n", source: "record exceeds" },
    ];
    for (const item of cases) {
      await writeFile(file("project-a", item.id), item.lines);
      expect(await reader().read(item.id)).toMatchObject({ input: null, output: null, source: expect.stringContaining(item.source) });
    }
  });

  it("keeps a saved problem sticky without spending budget or starving an unrelated session", async () => {
    const bad = "00000000-0000-4000-8000-000000000021", good = "00000000-0000-4000-8000-000000000022";
    await writeFile(file("project-a", bad), lines('{"type":"assistant","message":') + "x".repeat(2048));
    await writeFile(file("project-a", good), lines(line("msg-good", usage(7, 3))));
    const instance = reader(), first = { bytes: 512 };
    expect(await instance.read(bad, first)).toMatchObject({ input: null, output: null, source: expect.stringContaining("malformed usage record") });
    expect(first.bytes).toBe(0);
    const replay = { bytes: 512 };
    expect(await instance.read(bad, replay)).toMatchObject({ input: null, output: null, source: expect.stringContaining("malformed usage record") });
    expect(replay.bytes).toBe(512);
    expect(await instance.read(good, replay)).toMatchObject({ input: 7, output: 3 });
  });

  it("resumes a partial append and a bounded read without inventing a total", async () => {
    const first = line("msg-1", usage(10, 2)), second = line("msg-2", usage(20, 4));
    await writeFile(file(), first + "\n" + second.slice(0, 20));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null, source: expect.stringContaining("awaiting source record completion") });
    await appendFile(file(), second.slice(20) + "\n");
    expect(await reader().read(ID)).toMatchObject({ input: 30, output: 6 });
    await writeFile(file(), lines(line("msg-3", usage(7, 3))));
    expect(await reader().read(ID, { bytes: 12 })).toMatchObject({ input: null, output: null, source: expect.stringContaining("incomplete bounded") });
    expect(await reader().read(ID)).toMatchObject({ input: 7, output: 3 });
  });

  it("re-reads a replaced file from zero instead of carrying old totals", async () => {
    await writeFile(file(), lines(line("msg-1", usage(100, 10))));
    expect(await reader().read(ID)).toMatchObject({ input: 100, output: 10 });
    await writeFile(file(), lines(line("msg-2", usage(7, 3))));
    expect(await reader().read(ID)).toMatchObject({ input: 7, output: 3 });
  });
});
