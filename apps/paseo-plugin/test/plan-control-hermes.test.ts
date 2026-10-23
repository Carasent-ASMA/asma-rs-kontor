import type { PluginClientContext } from "@getpaseo/plugin/client";
import { describe, expect, it, vi } from "vitest";
import { linkedPlans } from "../client/navigation-state";
import type { contributePlanControl as Contribute } from "../client/plan-control";
import * as directory from "../client/session-directory";
import type { DirectoryState } from "../client/session-directory";
import * as readSources from "../shared/read-sources";
import { loadHermesScoped } from "./hermes-scoping";
import { sessionEntry } from "./session-fixtures";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const source = (id: string) => ({ id, title: id, kind: "plan", path: `/plans/${id}.md`, epic: null, projectId: null, workstream: null, revisionKind: "working copy" });

describe("Open plan under the iOS (Hermes) scoping of plugin bundles", () => {
  it("opens each composer pill's own session and plan from the shared directory", async () => {
    const state: DirectoryState = {
      ...directory.emptyDirectoryState(false),
      entries: ["fixture-a", "fixture-b", "fixture-c"].map((id) => sessionEntry(id, { workspaceId: `workspace-${id}` })),
      status: "ready",
      live: true,
      inventoryComplete: true,
      lastUpdatedAt: 1,
    };
    const bindHostDirectory = (listener: (state: DirectoryState) => void) => {
      listener(state);
      return () => undefined;
    };
    const { contributePlanControl } = loadHermesScoped<{ contributePlanControl: typeof Contribute }>("../client/plan-control.ts", {
      "../shared/read-sources": readSources, "./session-directory": directory, "./navigation-state": { linkedPlans },
      "./use-session-directory": { bindHostDirectory } });
    const pills: Array<{ agentId: string; button: { behavior: { onPress: () => void } } }> = [], openScreen = vi.fn();
    const list = vi.fn(async () => { throw new Error("plan control must not walk the agent directory"); });
    const client = {
      rpc: async () => ({ status: "ready", associations: { "fixture-a": ["plan-a"], "fixture-b": ["plan-b", "plan-c"] }, sources: ["plan-a", "plan-b", "plan-c"].map(source), error: null }),
      paseo: { agents: { list } },
      addComposerPill: (pill: (typeof pills)[number]) => { pills.push(pill); return { update: vi.fn(), remove: vi.fn() }; },
      openScreen,
    } as unknown as PluginClientContext;
    const stop = contributePlanControl(client, "fixture-screen"); await settle();
    expect(pills.map((pill) => pill.agentId)).toEqual(["fixture-a", "fixture-b"]);
    expect(list).not.toHaveBeenCalled();
    pills[0]!.button.behavior.onPress();
    expect(openScreen).toHaveBeenCalledExactlyOnceWith({ screenId: "fixture-screen", params: { tab: "Plan", agentId: "fixture-a", sourceId: "plan-a" } });
    stop();
  });
});
