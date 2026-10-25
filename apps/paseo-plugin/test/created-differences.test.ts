import { beforeEach, describe, expect, it, vi } from "vitest";
import { configureRecentStorage, readRecent } from "../client/recent-sessions";
import { openCreatedSession, type SessionNavigation } from "../client/session-actions";
import type { CreateOutcome } from "../shared/session-create";

// A readback with differences names its exact session so the operator can inspect it. Opening it is navigation
// only: it releases no task, sends nothing and does not turn the differences into a verified creation.
const outcome = (): CreateOutcome => ({ requestId: "00000000-0000-4000-8000-000000000002", state: "differences", title: "LSA • ADHOC • Fixture", agentId: "created-with-differences",
  workspaceId: "wks-adam", message: "Created; the readback differs.", differences: ["cwd: expected /fixture/a, read /fixture/b"], persona: "supported", checkedAt: "2026-10-24" });

describe("a created session whose readback differs", () => {
  beforeEach(() => configureRecentStorage(null));

  it("opens its exact host/native ID for inspection and nothing else", () => {
    const calls: string[] = [];
    const navigation = new Proxy({}, { get: (_target, name) => (...args: unknown[]) => { calls.push(`${String(name)} ${JSON.stringify(args)}`); } }) as SessionNavigation;
    const created = outcome(), before = structuredClone(created);
    expect(openCreatedSession({ outcome: created, serverId: "host-a", navigation })).toEqual({ ok: true });
    expect(calls).toEqual([`openAgent ${JSON.stringify([{ agentId: "created-with-differences", serverId: "host-a" }])}`]);
    expect(created).toEqual(before);
    expect(readRecent("host-a").record.visits.map((visit) => visit.id)).toEqual(["created-with-differences"]);
  });

  it("does not authorize task release: the open result carries no creation, verification or release claim", () => {
    const navigation = { openAgent: vi.fn() } as unknown as SessionNavigation;
    const result = openCreatedSession({ outcome: outcome(), serverId: "host-a", navigation });
    expect(Object.keys(result)).toEqual(["ok"]);
    expect(outcome().state).toBe("differences");
    expect(outcome().differences).not.toEqual([]);
  });
});
