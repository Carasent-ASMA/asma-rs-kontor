import type { PluginClientContext } from "@getpaseo/plugin/client";
import { describe, expect, it, vi } from "vitest";
import { linkedPlans } from "../client/navigation-state";
import type { contributePlanControl as Contribute } from "../client/plan-control";
import * as directory from "../client/session-directory";
import * as readSources from "../shared/read-sources";
import { loadHermesScoped } from "./hermes-scoping";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const agent = (id: string) => ({ agent: { id, workspaceId: `workspace-${id}`, status: "idle", title: id, cwd: "/fixture", provider: "codex",
  createdAt: "2026-10-22T00:00:00.000Z", updatedAt: "2026-10-22T00:00:00.000Z" }, project: null });
const source = (id: string) => ({ id, title: id, kind: "plan", path: `/plans/${id}.md`, epic: null, projectId: null, workstream: null, revisionKind: "working copy" });

describe("Open plan under the iOS (Hermes) scoping of plugin bundles", () => {
  it("opens each composer pill's own session and plan", async () => {
    const { contributePlanControl } = loadHermesScoped<{ contributePlanControl: typeof Contribute }>("../client/plan-control.ts", {
      "../shared/read-sources": readSources, "./session-directory": directory, "./navigation-state": { linkedPlans } });
    const pills: Array<{ agentId: string; button: { behavior: { onPress: () => void } } }> = [], openScreen = vi.fn();
    const client = {
      rpc: async () => ({ status: "ready", associations: { "fixture-a": ["plan-a"], "fixture-b": ["plan-b", "plan-c"] }, sources: ["plan-a", "plan-b", "plan-c"].map(source), error: null }),
      paseo: { agents: { list: async () => ({ entries: ["fixture-a", "fixture-b", "fixture-c"].map(agent), pageInfo: { hasMore: false, nextCursor: null } }) } },
      addComposerPill: (pill: (typeof pills)[number]) => { pills.push(pill); return { update: vi.fn(), remove: vi.fn() }; },
      openScreen,
    } as unknown as PluginClientContext;
    const stop = contributePlanControl(client, "fixture-screen"); await settle(); stop();
    expect(pills.map((pill) => pill.agentId)).toEqual(["fixture-a", "fixture-b"]);
    pills[0]!.button.behavior.onPress();
    expect(openScreen).toHaveBeenCalledExactlyOnceWith({ screenId: "fixture-screen", params: { tab: "Plan", agentId: "fixture-a", sourceId: "plan-a" } });
  });
});
