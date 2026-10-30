#!/usr/bin/env node
// A procedural client of the existing owner, never another archive/create/send writer.
import { createHash } from 'node:crypto';
import { constants } from 'node:fs';
import { open, readFile } from 'node:fs/promises';
import { dirname, isAbsolute, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';

function requireFact(value, message) { if (!value) throw new Error(message); }
async function immutable(path, value) {
  const file = await open(path, 'wx', 0o600);
  try { await file.writeFile(JSON.stringify(value, null, 2) + '\n'); await file.sync(); }
  finally { await file.close(); }
  const directory = await open(dirname(path), 'r');
  try { await directory.sync(); } finally { await directory.close(); }
}
async function briefUnchanged(ref) {
  requireFact(ref && isAbsolute(ref.path) && /^[a-f0-9]{64}$/.test(ref.sha256), 'An exact prepared brief path and SHA256 are required');
  const file = await open(ref.path, constants.O_RDONLY | constants.O_NOFOLLOW);
  try {
    const stat = await file.stat();
    requireFact(stat.isFile() && stat.nlink === 1 && stat.size <= 8_000_000, 'Brief is not a bounded regular file');
    requireFact(createHash('sha256').update(await file.readFile()).digest('hex') === ref.sha256, 'Prepared brief changed');
  } finally { await file.close(); }
}
export async function connectPublicHost(host) {
  const specifier = '@getpaseo/client/node';
  let createHostClient;
  try { ({ createHostClient } = await import(specifier)); }
  catch { throw new Error('The procedural CLI requires the supported @getpaseo/client/node connector (0.13.2+); the plugin development SDK 0.11.4 does not provide it. No private SDK fallback is used.'); }
  requireFact(typeof createHostClient === 'function', 'Public createHostClient is unavailable; no private SDK fallback');
  const client = createHostClient(host, { clientId: 'kontor-seat-takeover', connectTimeoutMs: 10000 });
  try { await client.connect(); return client; }
  catch (error) { await client.close(); throw error; }
}
/** A repeated apply resolves the original operation; no second apply dispatch after an ambiguous reply. */
export async function driveTakeover(command, inputPath, checkpointPath, connect = connectPublicHost) {
  requireFact(['review', 'apply', 'check'].includes(command), 'Use review, apply or check');
  const record = JSON.parse(await readFile(inputPath, 'utf8'));
  for (const field of ['host', 'hostId', 'pluginId', 'sourceId']) requireFact(typeof record[field] === 'string' && record[field].trim(), `Missing exact ${field}`);
  requireFact(!/password|token|secret/i.test(record.host) && !record.host.includes('@'), 'Use a credential-free selected host reference');
  await briefUnchanged(record.preparedBrief);
  const client = await connect(record.host);
  const verifyHost = () => requireFact(client.getLastServerInfoMessage()?.serverId === record.hostId, 'Selected daemon identity changed or is unavailable');
  try {
    verifyHost();
    if (command === 'review') {
      requireFact(checkpointPath && isAbsolute(checkpointPath), 'Review needs an absolute new checkpoint path');
      const review = await client.invokePluginRpc(record.pluginId, 'sessions.takeover-review', {
        agentId: record.sourceId, bindingKey: record.bindingKey ?? null,
        ...(record.route ? { route: record.route } : {}), preparedBrief: record.preparedBrief,
      });
      verifyHost();
      requireFact(review?.agentId === record.sourceId && typeof review.reviewId === 'string', 'Review identifies another source');
      const checkpoint = { version: 1, host: record.host, hostId: record.hostId, pluginId: record.pluginId,
        sourceId: record.sourceId, preparedBrief: record.preparedBrief, review };
      await immutable(checkpointPath, checkpoint);
      return checkpoint;
    }
    const review = record.review;
    requireFact(record.version === 1 && review?.agentId === record.sourceId && typeof review.reviewId === 'string', 'Checkpoint source/review mismatch');
    let method = 'sessions.takeover-check', args = { reviewId: review.reviewId };
    if (command === 'apply') {
      requireFact(review.state === 'ready' && /^[a-f0-9]{64}$/.test(review.digest) && Number.isInteger(review.revision), 'This review is not confirmable');
      try {
        await immutable(`${inputPath}.apply-intent`, { reviewId: review.reviewId, sourceId: record.sourceId, hostId: record.hostId, digest: review.digest });
        method = 'sessions.takeover-apply'; args = { reviewId: review.reviewId, revision: review.revision, digest: review.digest };
      } catch (error) {
        if (error?.code !== 'EEXIST') throw error;
        const prior = JSON.parse(await readFile(`${inputPath}.apply-intent`, 'utf8'));
        requireFact(prior.reviewId === review.reviewId && prior.sourceId === record.sourceId && prior.hostId === record.hostId && prior.digest === review.digest, 'Apply intent belongs to another checkpoint');
      }
    }
    verifyHost();
    const result = await client.invokePluginRpc(record.pluginId, method, args);
    verifyHost();
    requireFact(result?.agentId === record.sourceId && result?.reviewId === review.reviewId, 'Outcome identifies another operation');
    return result;
  } finally { await client.close(); }
}
if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  const [command, input, checkpoint] = process.argv.slice(2);
  try { console.log(JSON.stringify(await driveTakeover(command, resolve(input ?? ''), checkpoint && resolve(checkpoint)), null, 2)); }
  catch (error) { console.error(error instanceof Error ? error.message : String(error)); process.exitCode = 1; }
}
