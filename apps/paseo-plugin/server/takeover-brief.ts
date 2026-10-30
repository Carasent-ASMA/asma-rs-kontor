import { createHash } from "node:crypto";
import { constants } from "node:fs";
import { open } from "node:fs/promises";
import { isAbsolute, join } from "node:path";
import { execFile } from "node:child_process";
import { promisify, isDeepStrictEqual } from "node:util";
import { z } from "zod";
import { takeoverBriefRefSchema } from "../shared/session-lifecycle";
const exec = promisify(execFile);
function requireFact(value: unknown, message: string): asserts value { if (!value) throw new Error(message); }
export const preparedCheckoutSchema = z.object({ path: z.string().min(1), branch: z.string().min(1), head: z.string().regex(/^[a-f0-9]{40}$/), wip: z.string().length(64) }).strict();
export const preparedTakeoverBriefSchema = z.object({
  version: z.literal(1), sourceId: z.string().min(1), originalTitle: z.string().min(1),
  mode: z.object({ workflow: z.enum(["kontor", "free"]), orchestration: z.literal("paseo") }).strict(),
  supervisorId: z.string().min(1).nullable(), scope: z.string().min(1), criteria: z.array(z.string().min(1)).min(1),
  authorizedNextAction: z.string().min(1), findings: z.array(z.string()), retryCounters: z.record(z.string(), z.number().int().nonnegative()),
  procedureRevision: z.string().min(1), checkout: preparedCheckoutSchema,
  authority: takeoverBriefRefSchema, history: z.array(takeoverBriefRefSchema).min(1),
}).strict();
export type PreparedTakeoverBrief = z.infer<typeof preparedTakeoverBriefSchema>;
export type BriefRef = z.infer<typeof takeoverBriefRefSchema>;
export type BackgroundAccounting = { sourceId: string; sourceRevision: string; requestId: string; observedAt: string; revision: string;
  workers: { id: string; state: "finished" | "checkpointed"; checkpoint: BriefRef }[] };
/** Only a server-configured reader of the provider-owned worker state may supply a complete result. No caller boolean is authority. */
export type BackgroundReader = (input: { sourceId: string; sourceRevision: string; requestId: string }) => Promise<
  { complete: true; accounting: BackgroundAccounting } | { complete: false; gap: string }>;
export async function readTakeoverReference(ref: BriefRef): Promise<string> {
  requireFact(isAbsolute(ref.path), "Prepared evidence requires an absolute path");
  const file = await open(ref.path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const stat = await file.stat();
    requireFact(stat.isFile() && stat.nlink === 1 && stat.size <= 8_000_000, "Prepared evidence is not a bounded regular file");
    const bytes = await file.readFile();
    requireFact(createHash("sha256").update(bytes).digest("hex") === ref.sha256, "Prepared evidence changed");
    requireFact(bytes.toString().trim(), "Prepared evidence is empty");
    return bytes.toString();
  } finally { await file.close(); }
}
export async function takeoverCheckout(path: string): Promise<z.infer<typeof preparedCheckoutSchema>> {
  requireFact(isAbsolute(path), "Checkout requires an absolute path");
  const git = async (...args: string[]) =>
    (await exec("git", ["--no-optional-locks", "-C", path, ...args], { maxBuffer: 16_000_000 }))
      .stdout;
  const branch = (await git("symbolic-ref", "--short", "HEAD")).trim(),
    head = (await git("rev-parse", "HEAD")).trim();
  const parts: (string | Buffer)[] = [
    await git("diff", "--binary", "HEAD", "--"),
    await git("status", "--porcelain=v1", "-z", "--untracked-files=all"),
  ];
  for (const name of (await git("ls-files", "--others", "--exclude-standard", "-z"))
    .split("\0")
    .filter(Boolean)
    .sort()) {
    const file = await open(join(path, name), constants.O_RDONLY | constants.O_NOFOLLOW);
    try {
      const stat = await file.stat();
      requireFact(
        stat.isFile() && stat.size <= 16_000_000,
        "Untracked checkout entry exceeds its bound or is not a regular file",
      );
      parts.push(name, await file.readFile());
    } finally {
      await file.close();
    }
  }
  const hash = createHash("sha256");
  for (const part of parts) {
    hash.update(String(Buffer.byteLength(part)));
    hash.update(":");
    hash.update(part);
  }
  return { path, branch, head, wip: hash.digest("hex") };
}

export async function readPreparedTakeoverBrief(ref: BriefRef, sourceId: string, cwd: string, title: string, supervisorId: string | null): Promise<PreparedTakeoverBrief> {
  const brief = preparedTakeoverBriefSchema.parse(JSON.parse(await readTakeoverReference(ref)));
  requireFact(brief.sourceId === sourceId && brief.originalTitle === title && brief.supervisorId === supervisorId, "Prepared brief source/title/supervisor mismatch");
  requireFact(brief.checkout.path === cwd && isDeepStrictEqual(await takeoverCheckout(cwd), brief.checkout), "Prepared checkout changed");
  for (const entry of [brief.authority, ...brief.history]) await readTakeoverReference(entry);
  return brief;
}
export async function readPreparedBackground(reader: BackgroundReader | undefined, input: Parameters<BackgroundReader>[0]): Promise<BackgroundAccounting> {
  requireFact(reader, "Authoritative provider-native background accounting is unavailable; nothing is retired");
  const result = await reader(input);
  requireFact(result.complete, result.complete ? "" : result.gap);
  const read = result.accounting, at = Date.parse(read.observedAt);
  requireFact(read.sourceId === input.sourceId && read.sourceRevision === input.sourceRevision && read.requestId === input.requestId
    && read.revision && Number.isFinite(at) && at <= Date.now() && Date.now() - at < 60_000 && Array.isArray(read.workers), "Background accounting is unbound, stale or incomplete");
  requireFact(new Set(read.workers.map(worker => worker.id)).size === read.workers.length, "Background worker identities conflict");
  for (const worker of read.workers) {
    requireFact(worker.id && ["finished", "checkpointed"].includes(worker.state), "Background work is not preserved");
    await readTakeoverReference(takeoverBriefRefSchema.parse(worker.checkpoint));
  }
  return read;
}
