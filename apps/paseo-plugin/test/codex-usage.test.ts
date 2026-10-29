import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { appendFile, mkdir, mkdtemp, readFile, readdir, rm, stat, symlink, writeFile } from "node:fs/promises";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { createCodexUsageReader } from "../server/codex-usage";
const ID = "00000000-0000-4000-8000-000000000001", OTHER = "00000000-0000-4000-8000-000000000002";
let root: string, cache: string, source: string;
const meta = (id = ID, patch = {}) => JSON.stringify({ type: "session_meta", payload: { id, session_id: id, cli_version: "0.160.0", originator: "codex_cli_rs", source: "cli", ...patch } });
const event = (input: number, output: number, patch = {}) => JSON.stringify({ type: "event_msg", timestamp: "2026-01-01T00:00:00.000Z", payload: { type: "token_count", info: { total_token_usage: { input_tokens: input, output_tokens: output, total_tokens: input + output, cached_input_tokens: 0, reasoning_output_tokens: 0, ...patch } } } });
const lines = (...values: string[]) => values.join("\n") + "\n";
beforeEach(async () => { root = await mkdtemp(join(tmpdir(), "kontor-codex-usage-")); cache = join(root, "cache"); const dir = join(root, "sessions", "2026", "01", "01"); await mkdir(dir, { recursive: true }); source = join(dir, `rollout-fixture-${ID}.jsonl`); });
afterEach(async () => { await rm(root, { recursive: true, force: true }); });
const reader = () => createCodexUsageReader(root, cache);

describe("qualified Codex cumulative snapshots", () => {
  it("takes the latest cumulative report, including explicit zeros, without summing caches or reasoning", async () => {
    await writeFile(source, lines(meta(), event(100, 5), event(200, 7, { cached_input_tokens: 150, reasoning_output_tokens: 4, cache_write_input_tokens: 0 })));
    expect(await reader().read(ID)).toMatchObject({ input: 200, output: 7, scope: "provider-cumulative" });
    await writeFile(source, lines(meta(), event(0, 0)));
    expect(await createCodexUsageReader(root, join(root, "fresh-cache")).read(ID)).toMatchObject({ input: 0, output: 0 });
  });
  it("keeps duplicate/replayed snapshots idempotent across append, reconnect and reader restart", async () => {
    await writeFile(source, lines(meta(), event(100, 5), event(100, 5)));
    const first = reader(); expect(await first.read(ID)).toMatchObject({ input: 100, output: 5 });
    expect(await first.read(ID)).toMatchObject({ input: 100, output: 5 });
    expect(await reader().read(ID)).toMatchObject({ input: 100, output: 5 });
    await appendFile(source, lines(event(200, 8)));
    expect(await reader().read(ID)).toMatchObject({ input: 200, output: 8 });
  });
  it("joins exact metadata IDs even when sessions have the same title, and rejects a wrong ID in a matching filename", async () => {
    await writeFile(source, lines(meta(ID, { title: "Same" }), event(100, 5)));
    const second = source.replace(ID, OTHER); await writeFile(second, lines(meta(OTHER, { title: "Same" }), event(900, 40)));
    expect(await reader().read(OTHER)).toMatchObject({ input: 900, output: 40 });
    await writeFile(source, lines(meta(OTHER), event(999, 99)));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null, source: expect.stringContaining("identity mismatch") });
  });
  it.each([{ session_id: OTHER }, { forked_from_id: OTHER }, { parent_thread_id: OTHER }, { source: { subagent: "fixture" } }])("refuses inherited/forked scope %j", async (patch) => {
    await writeFile(source, lines(meta(ID, patch), event(100, 5)));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null, source: expect.stringContaining("unqualified") });
  });
  it.each([{ cli_version: "0.159.0" }, { originator: "other" }])("rejects an unqualified format %j", async (patch) => {
    await writeFile(source, lines(meta(ID, patch), event(100, 5)));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null });
  });
  it.each([{ id: OTHER }, { cli_version: "0.159.0" }, { forked_from_id: OTHER }])("rejects later contradictory metadata before and after a cached restart %j", async (patch) => {
    await writeFile(source, lines(meta(), event(100, 5)));
    expect(await reader().read(ID)).toMatchObject({ input: 100 });
    await appendFile(source, lines(meta(ID, patch), event(900, 40)));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null });
    expect(await createCodexUsageReader(root, join(root, "cold-cache")).read(ID)).toMatchObject({ input: null, output: null });
  });
  it("validates escaped JSON metadata types instead of relying on a literal-line filter", async () => {
    const escaped = meta(OTHER).replace("session_meta", "session\\u005fmeta");
    await writeFile(source, lines(meta(), event(100, 5), escaped, event(900, 40)));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null });
  });
  it.each([{ id: OTHER }, { cli_version: "0.159.0" }, { parent_thread_id: OTHER }])("discards v1 offsets that already skipped contradictory metadata %j", async (patch) => {
    await writeFile(source, lines(meta(), event(100, 5))); await reader().read(ID);
    const path = join(cache, (await readdir(cache))[0]!);
    await appendFile(source, lines(meta(ID, patch), event(900, 40)));
    const old = JSON.parse(await readFile(path, "utf8")), current = await stat(source);
    Object.assign(old, { version: 1, size: current.size, modified: current.mtimeMs, offset: current.size, counts: { input_tokens: 900, output_tokens: 40, total_tokens: 940 }, baseline: { input_tokens: 900, output_tokens: 40, total_tokens: 940 } });
    await writeFile(path, JSON.stringify(old));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null });
    expect(JSON.parse(await readFile(path, "utf8")).version).toBe(3);
  });
  it("requalifies valid v1 sources from zero and resumes a partial consistent metadata append", async () => {
    await writeFile(source, lines(meta(), event(100, 5))); await reader().read(ID);
    const path = join(cache, (await readdir(cache))[0]!), old = JSON.parse(await readFile(path, "utf8"));
    old.version = 1; old.counts.input_tokens = 999; old.counts.total_tokens = 1004; old.baseline = old.counts;
    await writeFile(path, JSON.stringify(old));
    expect(await reader().read(ID)).toMatchObject({ input: 100 });
    const repeated = meta(); await appendFile(source, repeated.slice(0, 30));
    expect(await reader().read(ID)).toMatchObject({ input: null });
    await appendFile(source, repeated.slice(30) + "\n" + lines(event(200, 8)));
    expect(await reader().read(ID)).toMatchObject({ input: 200, output: 8 });
    expect(await reader().read(ID)).toMatchObject({ input: 200, output: 8 });
  });
  it("refuses reset counters, including source truncation between restarts", async () => {
    await writeFile(source, lines(meta(), event(200, 8))); expect(await reader().read(ID)).toMatchObject({ input: 200 });
    await writeFile(source, lines(meta(), event(100, 4))); expect(await reader().read(ID)).toMatchObject({ input: null, source: expect.stringContaining("reset") });
  });
  it("does not mistake a partial re-read after replacement for a reset, but retains a real reset", async () => {
    await writeFile(source, lines(meta(), event(100, 5), event(200, 8)));
    expect(await reader().read(ID)).toMatchObject({ input: 200 });
    const replacement = source + ".replacement";
    await writeFile(replacement, lines(meta(), event(100, 5), event(300, 9)));
    await import("node:fs/promises").then((fs) => fs.rename(replacement, source));
    const firstPart = Buffer.byteLength(lines(meta(), event(100, 5)));
    expect(await reader().read(ID, { bytes: firstPart })).toMatchObject({ input: null });
    expect(await reader().read(ID)).toMatchObject({ input: 300 });
    await writeFile(source, lines(meta(), event(50, 1)));
    expect(await reader().read(ID)).toMatchObject({ input: null, source: expect.stringContaining("reset") });
  });
  it("refuses resets within one source and never exposes a partial lower counter as a session total", async () => {
    await writeFile(source, lines(meta(), event(200, 8), event(100, 4)));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null, source: expect.stringContaining("reset") });
  });
  it.each([{ input_tokens: null }, { input_tokens: -1 }, { total_tokens: 1 }, { cached_input_tokens: 200 }, { reasoning_output_tokens: 6 }])("refuses missing/invalid counts %j", async (patch) => {
    await writeFile(source, lines(meta(), event(100, 5, patch)));
    expect(await reader().read(ID)).toMatchObject({ input: null, output: null });
  });
  it("keeps missing, malformed and partially-written usage unknown, then reads a completed append", async () => {
    await writeFile(source, lines(meta())); expect(await reader().read(ID)).toMatchObject({ input: null });
    await appendFile(source, event(100, 5)); expect(await reader().read(ID)).toMatchObject({ input: null, source: expect.stringContaining("incomplete") });
    await appendFile(source, "\n"); expect(await reader().read(ID)).toMatchObject({ input: 100 });
    await appendFile(source, '{"token_count":bad}\n'); expect(await reader().read(ID)).toMatchObject({ input: null });
  });
  it("fails closed on duplicate exact sources, symlinks and absent IDs", async () => {
    await writeFile(source, lines(meta(), event(100, 5)));
    await mkdir(join(root, "archived_sessions")); await writeFile(join(root, "archived_sessions", `rollout-duplicate-${ID}.jsonl`), lines(meta(), event(300, 9)));
    expect(await reader().read(ID)).toMatchObject({ input: null, source: expect.stringContaining("ambiguous") });
    await rm(source); await symlink(join(root, "archived_sessions", `rollout-duplicate-${ID}.jsonl`), source);
    expect(await reader().read(ID)).toMatchObject({ input: 300 }); // symlink is not treated as a second provider source
    expect(await reader().read(OTHER)).toMatchObject({ input: null });
  });
  it("bounds incremental reads, persists numeric-only state, and does not retain transcript or home paths", async () => {
    const secret = "PRIVATE_TRANSCRIPT_FIXTURE";
    await writeFile(source, lines(meta(), JSON.stringify({ type: "response_item", payload: { content: secret } }), event(100, 5)));
    const budget = { bytes: 50 }; expect(await reader().read(ID, budget)).toMatchObject({ input: null }); expect(budget.bytes).toBe(0);
    expect(await reader().read(ID)).toMatchObject({ input: 100 });
    const files = await import("node:fs/promises").then((fs) => fs.readdir(cache));
    const stored = await readFile(join(cache, files[0]!), "utf8");
    expect(stored).not.toContain(secret); expect(stored).not.toContain(root);
    expect(stored).not.toContain("base_instructions");
  });
});

it("recomputes v2 failures and continues past a large record", async () => {
 await writeFile(source, lines(meta(), event(100, 5))); await reader().read(ID);
 const path = join(cache, (await readdir(cache))[0]!);
 const old = JSON.parse(await readFile(path, "utf8"));
 Object.assign(old, { version: 2, meta: false, counts: null, offset: 37874, problem: "record exceeds the source bound" });
 await writeFile(path, JSON.stringify(old));
 await writeFile(source, lines(meta(), JSON.stringify({ type: "response_item", payload: "x".repeat(5 * 1024 * 1024) }), event(200, 8)));
 expect(await reader().read(ID, { bytes: 1024 * 1024 })).toMatchObject({ input: null });
 expect(await reader().read(ID)).toMatchObject({ input: 200, output: 8 });
});

it("fails closed above the 16MiB record bound without exposing partial totals", async () => {
 await writeFile(source, lines(meta(), event(100, 5), JSON.stringify({ type: "response_item", payload: "x".repeat(17 * 1024 * 1024) }), event(200, 8)));
 expect(await reader().read(ID)).toMatchObject({ input: null, source: expect.stringContaining("record exceeds") });
});
