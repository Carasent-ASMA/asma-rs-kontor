import { describe, expect, it } from "vitest";
import { evaluatePlacement, type WorkspaceCreateLike } from "../server/placement";

const rules = {
  paseoWorktreesRoot: "/Users/test/.paseo/worktrees",
  placementRoots: [],
};

function create(source: WorkspaceCreateLike["source"]): WorkspaceCreateLike {
  return { source, callerAgentId: "agent-1" };
}

describe("evaluatePlacement", () => {
  it("accepts a canonical directory registered under an explicit project", () => {
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/carasent/asma-modules/.worktrees/asma-8507/asma-rs-kontor",
          projectId: "prj_7409d2c6aff8e21c",
        }),
        rules,
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
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("projectId") });
  });

  it("refuses a relative directory path", () => {
    expect(
      evaluatePlacement(
        create({ kind: "directory", path: ".worktrees/asma-8507", projectId: "prj_1" }),
        rules,
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
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("Paseo") });
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
          path: "/Users/test/carasent/asma-modules/.worktrees/task",
          projectId: "prj_1",
        }),
        contained,
      ),
    ).toEqual({ allowed: true });
    expect(
      evaluatePlacement(
        create({
          kind: "directory",
          path: "/Users/test/elsewhere/task",
          projectId: "prj_1",
        }),
        contained,
      ),
    ).toMatchObject({ allowed: false, reason: expect.stringContaining("placement roots") });
  });
});
