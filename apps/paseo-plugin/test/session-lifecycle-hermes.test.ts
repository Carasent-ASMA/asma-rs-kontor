import { describe, expect, it } from "vitest";
import type { useSessionLifecycle as Hook } from "../client/use-session-lifecycle";
import * as lifecycle from "../shared/session-lifecycle";
import type { LifecycleReview, LifecycleRow } from "../shared/session-lifecycle";
import { deferredReact, loadHermesScoped } from "./hermes-scoping";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const row = (id: string, outcome: LifecycleRow["outcome"]) => ({ id, eligible: true, outcome, message: null }) as LifecycleRow;

describe("session lifecycle under the iOS (Hermes) scoping of plugin bundles", () => {
  it.each([["archive", "archived"], ["resume", "submitted"]] as const)("shows each %s result on its own row when React renders between items", async (action, outcome) => {
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
