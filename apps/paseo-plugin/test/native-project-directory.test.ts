import type { PaseoProject, PaseoProjectListResult, PaseoWorkspaceListResult } from "@getpaseo/client";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const model = vi.hoisted(() => ({
  state: undefined as unknown,
  effect: null as (() => () => void) | null,
  cleanup: null as (() => void) | null,
  projects: vi.fn<() => Promise<PaseoProjectListResult>>(),
  workspaces: vi.fn<() => Promise<PaseoWorkspaceListResult>>(),
  client: vi.fn(),
}));
vi.mock("react", () => ({
  useState: <T,>(initial: T) => {
    if (model.state === undefined) model.state = initial;
    return [model.state as T, (update: (current: T) => T) => { model.state = update(model.state as T); }];
  },
  useEffect: (effect: () => () => void) => { model.effect = effect; },
}));
vi.mock("@getpaseo/plugin/client", () => ({ getPaseoClient: (hostId: string) => model.client(hostId) }));
import { forgetWorkspaceDirectories, useWorkspaceDirectory } from "../client/use-workspace-directory";

const desk: PaseoProject = {
  projectId: "prj_empty_desk", projectKey: "fixture:empty-desk", projectDisplayName: "Desk", projectCustomName: "DESK • RESEARCH",
  projectRootPath: "/fixture/desk", projectKind: "non_git", projectCustomIconRevision: null, projectIconRevision: "automatic:none:v1",
};
function read() { return useWorkspaceDirectory("host-fixture", true); }
function mount(online = true) {
  useWorkspaceDirectory("host-fixture", online);
  model.cleanup = model.effect!();
}
beforeEach(() => {
  vi.useFakeTimers(); vi.setSystemTime(new Date("2026-10-20T12:00:00Z"));
  model.state = undefined; model.effect = null; model.cleanup = null; forgetWorkspaceDirectories();
  model.projects.mockReset().mockResolvedValue({ requestId: "projects", projects: [desk] });
  model.workspaces.mockReset().mockResolvedValue({ requestId: "workspaces", entries: [], pageInfo: { hasMore: false, nextCursor: null, prevCursor: null } });
  model.client.mockReset().mockReturnValue({ projects: { list: model.projects }, workspaces: { list: model.workspaces } });
});
afterEach(() => { model.cleanup?.(); vi.useRealTimers(); });
async function settle() { await vi.advanceTimersByTimeAsync(0); }

describe("native project inventory independent from workspaces", () => {
  it("reads a desk with no workspace through the supported project feed and refreshes its rename", async () => {
    mount(); await settle();
    expect(model.client).toHaveBeenCalledWith("host-fixture");
    expect(read()).toMatchObject({ entries: [], projects: [desk], projectError: null, readAt: "2026-10-20T12:00:00.000Z", projectsReadAt: "2026-10-20T12:00:00.000Z" });
    model.projects.mockResolvedValueOnce({ requestId: "rename", projects: [{ ...desk, projectCustomName: "DESK • DOCS" }] });
    await vi.advanceTimersByTimeAsync(30_000);
    expect(read().projects[0]).toMatchObject({ projectId: desk.projectId, projectCustomName: "DESK • DOCS" });
    expect(model.projects).toHaveBeenCalledTimes(2);
  });

  it("retains last project data with an explicit failure while publishing a fresh workspace read", async () => {
    mount(); await settle();
    model.projects.mockRejectedValueOnce(new Error("Project feed disconnected"));
    await vi.advanceTimersByTimeAsync(30_000);
    expect(read()).toMatchObject({ projects: [desk], error: null, readAt: "2026-10-20T12:00:30.000Z", projectsReadAt: "2026-10-20T12:00:00.000Z" });
    expect(read().projectError).toContain("Project feed disconnected");
  });

  it("still discovers empty desks if the workspace feed fails", async () => {
    model.workspaces.mockRejectedValueOnce(new Error("Workspace feed disconnected"));
    mount(); await settle();
    expect(read()).toMatchObject({ entries: [], readAt: null, projects: [desk], projectError: null, projectsReadAt: "2026-10-20T12:00:00.000Z" });
    expect(read().error).toContain("Workspace feed disconnected");
  });

  it("does not request an offline host and names both inventories as stale", () => {
    mount(false);
    expect(read()).toMatchObject({ entries: [], projects: [], readAt: null, projectsReadAt: null });
    expect(read().error).toContain("host offline"); expect(read().projectError).toContain("host offline");
    expect(model.client).not.toHaveBeenCalled();
  });

  it("discards a pending result after unmount, with no subsequent timer", async () => {
    let complete!: (value: PaseoProjectListResult) => void;
    model.projects.mockImplementationOnce(() => new Promise((resolve) => { complete = resolve; }));
    mount(); model.cleanup!(); model.cleanup = null;
    complete({ requestId: "late", projects: [desk] }); await settle();
    expect(read()).toMatchObject({ entries: [], projects: [], readAt: null, projectsReadAt: null });
    expect(vi.getTimerCount()).toBe(0);
  });
});
