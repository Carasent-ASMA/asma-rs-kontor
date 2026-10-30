import { afterEach, describe, expect, it } from 'vitest';
import { mkdtemp, writeFile, rm } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { createHash } from 'node:crypto';
import { driveTakeover } from '../scripts/seat-takeover.mjs';
const dirs = [];
afterEach(async () => { for (const dir of dirs.splice(0)) await rm(dir, { recursive: true, force: true }); });
async function fixture() {
  const dir = await mkdtemp(join(tmpdir(), 'kontor-seat-script-')); dirs.push(dir);
  const briefPath = join(dir, 'brief.json'), brief = JSON.stringify({ sourceId: 'source-1' }); await writeFile(briefPath, brief);
  const request = { host: '127.0.0.1:6767', hostId: 'host-1', pluginId: 'kontor', sourceId: 'source-1',
    preparedBrief: { path: briefPath, sha256: createHash('sha256').update(brief).digest('hex') } };
  const path = join(dir, 'request.json'), checkpoint = join(dir, 'checkpoint.json'); await writeFile(path, JSON.stringify(request));
  const calls = []; let closed = 0, hostId = 'host-1', loseApply = false;
  const review = { reviewId: 'review-1', revision: 1, agentId: 'source-1', state: 'ready', digest: 'a'.repeat(64) };
  const connect = async () => ({ getLastServerInfoMessage: () => ({ serverId: hostId }), close: async () => { closed++; },
    invokePluginRpc: async (plugin, method, input) => {
      calls.push({ plugin, method, input });
      if (method === 'sessions.takeover-review') return review;
      if (loseApply && method === 'sessions.takeover-apply') throw new Error('reply lost');
      return { reviewId: 'review-1', agentId: 'source-1', state: 'uncertain', phase: 'release', successor: { agentId: 'successor-1' } };
    } });
  return { request, path, checkpoint, calls, review, connect, briefPath, setHost: v => { hostId = v; }, loseApply: () => { loseApply = true; }, closed: () => closed };
}
describe('procedural takeover CLI existing owner', () => {
  it('pins exact host/source/brief and delegates only to existing review/apply/check', async () => {
    const f = await fixture(); await driveTakeover('review', f.path, f.checkpoint, f.connect);
    await driveTakeover('apply', f.checkpoint, undefined, f.connect);
    await driveTakeover('check', f.checkpoint, undefined, f.connect);
    expect(f.calls.map(c => c.method)).toEqual(['sessions.takeover-review', 'sessions.takeover-apply', 'sessions.takeover-check']);
    expect(f.calls[0].input).toEqual({ agentId: 'source-1', bindingKey: null, preparedBrief: f.request.preparedBrief });
    expect(f.closed()).toBe(3);
  });
  it('never repeats apply after a lost reply, including a reconstructed client', async () => {
    const f = await fixture(); await driveTakeover('review', f.path, f.checkpoint, f.connect); f.loseApply();
    await expect(driveTakeover('apply', f.checkpoint, undefined, f.connect)).rejects.toThrow('reply lost');
    await driveTakeover('apply', f.checkpoint, undefined, f.connect);
    expect(f.calls.map(c => c.method)).toEqual(['sessions.takeover-review', 'sessions.takeover-apply', 'sessions.takeover-check']);
  });
  it('refuses host drift before RPC and releases its connection', async () => {
    const f = await fixture(); f.setHost('another-host');
    await expect(driveTakeover('review', f.path, f.checkpoint, f.connect)).rejects.toThrow('daemon identity');
    expect(f.calls).toEqual([]); expect(f.closed()).toBe(1);
  });
  it('refuses changed brief or a review naming another selected source', async () => {
    const f = await fixture(); await writeFile(f.briefPath, '{}');
    await expect(driveTakeover('review', f.path, f.checkpoint, f.connect)).rejects.toThrow('brief changed');
    expect(f.calls).toEqual([]);
    const g = await fixture(); g.review.agentId = 'another-source';
    await expect(driveTakeover('review', g.path, g.checkpoint, g.connect)).rejects.toThrow('another source');
  });
});
