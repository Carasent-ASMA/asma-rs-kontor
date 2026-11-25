import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { evaluatePlacement, type WorkspaceCreateLike } from "../server/placement";

const rules = {
  paseoWorktreesRoot: "/Users/test/.paseo/worktrees",
  placementRoots: [],
};

const binding = {
  projectId: "prj_7409d2c6aff8e21c",
  directory: "/Users/test/carasent/asma-modules/.worktrees/asma-8507/asma-rs-kontor",
};
const bound = { projects: [binding.projectId], directory: binding.directory };
/** A caller's own project with no canonical directory: a Paseo-direct seat. */
const direct = { projects: ["prj_paseo_direct"] };

function create(source: WorkspaceCreateLike["source"]): WorkspaceCreateLike {
  return { source, callerAgentId: "agent-1" };
}

describe("evaluatePlacement", () => {
  it("accepts a canonical directory registered under the bound project", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/carasent/asma-modules/.worktrees/asma-8507/asma-rs-kontor",
          projectId: "prj_7409d2c6aff8e21c",
        }),
        rules,
        bound,
      ),
    ).toEqual({ allowed: true });
  });

  it("refuses a Paseo worktree source", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "worktree",
          cwd: "/Users/test/carasent/asma-modules",
          projectId: "prj_7409d2c6aff8e21c",
        }),
        rules,
        bound,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("worktree") });
  });

  it("refuses a directory without an explicit project", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/carasent/asma-modules/.worktrees/asma-8507/asma-rs-kontor",
        }),
        rules,
        bound,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("projectId") });
  });

  it("refuses a relative directory path", () => {
    expect(
      evaluatePlacement(
        create({ kind: "directory", path: ".worktrees/asma-8507", projectId: "prj_1" }),
        rules,
        bound,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("absolute") });
  });

  it("refuses a directory inside Paseo's own worktree root", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/.paseo/worktrees/0vl4ss0m/asma-7681-authoring-surfaces",
          projectId: "prj_1",
        }),
        rules,
        bound,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("Paseo") });
  });

  it("refuses another epic's project even under the canonical directory", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: binding.directory,
          projectId: "prj_epic_b",
        }),
        rules,
        bound,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("project") });
  });

  it("refuses another epic's directory under the bound project", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/carasent/asma-modules/.worktrees/asma-other/epic-b",
          projectId: binding.projectId,
        }),
        rules,
        bound,
      ),
    ).toMatchObject({
      allowed: false,
      reason: expect.stringContaining("canonical worktree"),
    });
  });

  it("refuses an unresolved binding", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: binding.directory,
          projectId: binding.projectId,
        }),
        rules,
        { projects: [""], directory: "" },
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("binding") });
  });

  it("accepts an explicit directory in the caller's own project without a canonical directory", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/carasent/asma-modules/workspaces/asma-8507",
          projectId: "prj_paseo_direct",
        }),
        rules,
        direct,
      ),
    ).toEqual({ allowed: true });
  });

  it("keeps the source checks without a canonical directory", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/carasent/asma-modules/workspaces/asma-8507",
        }),
        rules,
        direct,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("projectId") });
    expect(
      evaluatePlacement(
        create({ kind: "worktree", cwd: "/Users/test/carasent/asma-modules", projectId: "prj_1" }),
        rules,
        direct,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("worktree") });
  });

  it("refuses an empty scope and a valid id outside the caller's project (no unscoped pass)", () => {
    const request = create({ kind: "directory", path: "/Users/test/carasent/asma-modules/workspaces/asma-8507", projectId: "prj_other" });
    expect(evaluatePlacement(request, rules, { projects: [] })).toMatchObject({ allowed: false, reason: expect.stringContaining("binding") });
    expect(evaluatePlacement(request, rules, direct)).toMatchObject({ allowed: false, reason: expect.stringContaining("project outside") });
    expect(evaluatePlacement(request, rules, { projects: ["prj_paseo_direct", "prj_other"] })).toEqual({ allowed: true });
    expect(evaluatePlacement(request, rules, { projects: ["prj_other"], directory: "relative/dir" })).toMatchObject({ allowed: false, reason: expect.stringContaining("binding") });
  });

  it("refuses a relative canonical directory instead of resolving it against the daemon's working directory", () => {
    const here = create({ kind: "directory", path: resolve("relative/dir"), projectId: "prj_other" });
    expect(evaluatePlacement(here, rules, { projects: ["prj_other"], directory: "relative/dir" })).toMatchObject({ allowed: false, reason: expect.stringContaining("absolute canonical directory") });
  });

  it("applies configured placement roots as extra containment", () => {
    const contained = {
      ...rules,
      placementRoots: ["/Users/test/carasent/asma-modules"],
    };
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/carasent/asma-modules/.worktrees/asma-8507/asma-rs-kontor",
          projectId: "prj_7409d2c6aff8e21c",
        }),
        contained,
        bound,
      ),
    ).toEqual({ allowed: true });
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/carasent/asma-modules/.worktrees/task",
          projectId: "prj_7409d2c6aff8e21c",
        }),
        contained,
        bound,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("canonical worktree") });
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/elsewhere/task",
          projectId: "prj_1",
        }),
        contained,
        bound,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("placement roots") });
  });
});
