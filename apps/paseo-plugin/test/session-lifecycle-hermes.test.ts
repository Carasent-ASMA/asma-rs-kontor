import { describe, expect, it } from "vitest";
import type { useSessionLifecycle as Hook } from "../client/use-session-lifecycle";
import * as lifecycle from "../shared/session-lifecycle";
import type { LifecycleReview, LifecycleRow } from "../shared/session-lifecycle";
import { deferredReact, loadHermesScoped } from "./hermes-scoping";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const row = (id: string, outcome: LifecycleRow["outcome"]) => ({ id, eligible: true, outcome, message: null }) as LifecycleRow;

describe("session lifecycle under the iOS (Hermes) scoping of plugin bundles", () => {
  function fixture(ids: string[], execute: (id: string) => Promise<LifecycleRow>) {
    const review = { id: "fixture-review", action: "archive", scope: "filtered", rows: ids.map((id) => row(id, "pending")) } as LifecycleReview;
    const calls: string[] = [], changed: LifecycleRow[] = [];
    const { react, render } = deferredReact();
    const { useSessionLifecycle } = loadHermesScoped<{ useSessionLifecycle: typeof Hook }>("../client/use-session-lifecycle.ts", { react,
      "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => contract.name === "sessions.lifecycle-preview" ? async () => review : async (input: { agentId: string }) => { calls.push(input.agentId); return execute(input.agentId); } },
      "../shared/session-lifecycle": lifecycle });
    let hostId = "host";
    const draw = () => render(() => useSessionLifecycle(hostId, (rows) => changed.push(...rows)));
    draw(); draw();
    return { draw, calls, changed, switchHost: (id: string) => { hostId = id; } };
  }
  it("keeps a Resume receipt read back from before Resume was removed as history: it never runs", async () => {
    const review = { id: "fixture-resume", action: "resume", scope: "filtered", rows: [row("a", "pending")] } as LifecycleReview, sent: string[] = [];
    const { react, render } = deferredReact();
    const { useSessionLifecycle } = loadHermesScoped<{ useSessionLifecycle: typeof Hook }>("../client/use-session-lifecycle.ts", { react,
      "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => async () => { sent.push(contract.name); return review; } }, "../shared/session-lifecycle": lifecycle });
    const draw = () => render(() => useSessionLifecycle("host", () => {}));
    draw(); await draw().restore(); await settle();
    expect(draw().review?.id).toBe("fixture-resume");
    expect(await draw().run()).toBeNull(); await settle();
    expect(draw().error).toBe(lifecycle.RESUME_REMOVED);
    expect(sent).toEqual(["sessions.lifecycle-review"]);
  });
  it("requires explicit confirmation even for one session selected in bulk mode", async () => {
    const f = fixture(["a"], async (id) => row(id, "archived"));
    await f.draw().prepare("archive", "filtered", ["a"], true);
    expect(f.calls).toEqual([]);
    expect(await f.draw().run()).toBe("Archived 1"); expect(f.calls).toEqual(["a"]);
    expect(f.draw().batch?.summary).toBe("Archived 1");
  });
  it("stops before the next row while settling the in-flight row, with no optimistic archive", async () => {
    let complete!: (row: LifecycleRow) => void;
    const f = fixture(["a", "b"], () => new Promise((resolve) => { complete = resolve; }));
    await f.draw().prepare("archive", "filtered", ["a", "b"], true);
    const running = f.draw().run();
    expect(f.draw().review?.rows.map((row) => row.outcome)).toEqual(["pending", "pending"]);
    expect(f.draw().batch?.currentId).toBe("a"); f.draw().stop(); expect(f.draw().batch?.announcement).toContain("Stopping after the current session"); complete(row("a", "archived"));
    expect(await running).toBe("Stopped before the next session. Archived 1 · 1 not attempted");
    expect(f.calls).toEqual(["a"]); expect(f.draw().review?.rows[1]?.outcome).toBe("pending");
  });
  it("keeps a lost transport reply uncertain and stops without dispatching or hiding the next row", async () => {
    const f = fixture(["a", "b"], async () => { throw new Error("Lost reply"); });
    await f.draw().prepare("archive", "filtered", ["a", "b"], true);
    expect(await f.draw().run()).toBe("Archived 0 · 1 not confirmed · 1 not attempted");
    expect(f.calls).toEqual(["a"]); expect(f.draw().review?.rows.map((row) => row.outcome)).toEqual(["uncertain", "pending"]);
    expect(f.changed.at(-1)).toMatchObject({ id: "a", outcome: "uncertain" });
  });
  it("discards an old host's late archive result and final summary and never dispatches its next row", async () => {
    let complete!: (row: LifecycleRow) => void;
    const f = fixture(["a", "b"], () => new Promise((resolve) => { complete = resolve; }));
    await f.draw().prepare("archive", "filtered", ["a", "b"], true);
    const running = f.draw().run(); f.switchHost("host-b"); f.draw(); f.draw();
    complete(row("a", "archived")); expect(await running).toBeNull();
    expect(f.draw()).toMatchObject({ batch: null, review: null, running: false }); expect(f.calls).toEqual(["a"]);
    expect(f.changed.some((row) => row.outcome === "archived")).toBe(false);
  });
  it("updates the batch summary when Check resolves an unconfirmed archive", async () => {
    let calls = 0;
    const f = fixture(["a"], async (id) => ({ ...row(id, "archived"), runtime: ++calls === 1 ? "unconfirmed" : "closed" }));
    await f.draw().prepare("archive", "filtered", ["a"], true);
    expect(await f.draw().run()).toBe("Archived 0 · 1 not confirmed");
    await f.draw().reconcile("a"); expect(f.draw().batch?.summary).toBe("Archived 1");
  });
  it.each([["archive", "archived"]] as const)("shows each %s result on its own row when React renders between items", async (action, outcome) => {
    const ids = ["fixture-a", "fixture-b", "fixture-c"], executing: Array<() => void> = [];
    const review = { id: "fixture-review", action, scope: "filtered", rows: ids.map((id) => row(id, "pending")) } as LifecycleReview;
    const rpc: Record<string, (input: { agentId?: string }) => Promise<unknown>> = {
      "sessions.lifecycle-preview": async () => review,
      "sessions.lifecycle-execute-item": (input) => new Promise((resolve) => executing.push(() => resolve(row(input.agentId!, outcome)))),
    };
    const { react, render } = deferredReact();
    const { useSessionLifecycle } = loadHermesScoped<{ useSessionLifecycle: typeof Hook }>("../client/use-session-lifecycle.ts", { react,
      "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => rpc[contract.name] }, "../shared/session-lifecycle": lifecycle });
    const draw = () => render(() => useSessionLifecycle("host", () => {}));
    await draw().prepare(action, "filtered", ids);
    const running = draw().run();
    for (let n = 0; n < ids.length; n++) { executing.shift()!(); await settle(); draw(); }
    await running;
    expect(draw().review?.rows.map((entry) => [entry.id, entry.outcome])).toEqual(ids.map((id) => [id, outcome]));
  });
});
