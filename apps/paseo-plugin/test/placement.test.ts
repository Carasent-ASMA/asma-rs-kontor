import { resolve } from "node:path";
import { describe, expect, it, vi } from "vitest";
import { evaluatePlacement, within, type WorkspaceCreateLike } from "../server/placement";

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

  it("reads a `..<name>` directory under a placement root as inside it, and the root's parent as outside (AUD-R3-04)", () => {
    const contained = { ...rules, placementRoots: ["/Users/test/carasent/asma-modules"] };
    const at = (path: string) => evaluatePlacement(create({ kind: "directory", path, projectId: "prj_paseo_direct" }), contained, direct);
    expect(at("/Users/test/carasent/asma-modules/..scratch/task")).toEqual({ allowed: true });
    expect(at("/Users/test/carasent")).toMatchObject({ allowed: false, reason: expect.stringContaining("placement roots") });
    expect(at("/Users/test/carasent/asma-modules-other/task")).toMatchObject({ allowed: false, reason: expect.stringContaining("placement roots") });
    expect(at("/Users/test/.paseo/worktrees/..evil")).toMatchObject({ allowed: false, reason: expect.stringContaining("Paseo") });
  });

  it("keeps the containment on Windows paths: another drive and a sibling are outside, a `..<name>` child is inside", async () => {
    vi.resetModules();
    vi.doMock("node:path", async () => {
      const { win32 } = await vi.importActual<typeof import("node:path")>("node:path");
      return { ...win32, default: win32 };
    });
    try {
      const { evaluatePlacement: evaluate } = await import("../server/placement");
      const windows = { paseoWorktreesRoot: "C:\\paseo\\worktrees", placementRoots: ["C:\\work"] };
      const at = (path: string) => evaluate(create({ kind: "directory", path, projectId: "prj_paseo_direct" }), windows, direct);
      expect(at("C:\\work\\..scratch\\task")).toEqual({ allowed: true });
      expect(at("D:\\work\\task")).toMatchObject({ allowed: false, reason: expect.stringContaining("placement roots") });
      expect(at("C:\\work-sibling\\task")).toMatchObject({ allowed: false, reason: expect.stringContaining("placement roots") });
      expect(at("C:\\paseo\\worktrees\\..evil")).toMatchObject({ allowed: false, reason: expect.stringContaining("Paseo") });
    } finally {
      vi.doUnmock("node:path");
      vi.resetModules();
    }
  });

  it("refuses a Windows sibling and another drive when Paseo's worktree root is on a different drive (AUD-B-R5-03)", async () => {
    vi.resetModules();
    vi.doMock("node:path", async () => {
      const { win32 } = await vi.importActual<typeof import("node:path")>("node:path");
      return { ...win32, default: win32 };
    });
    try {
      const { evaluatePlacement: evaluate } = await import("../server/placement");
      // Paseo's root on D does not contain these paths, so only the placement-root check can refuse them.
      const split = { paseoWorktreesRoot: "D:\\paseo\\worktrees", placementRoots: ["C:\\work"] };
      const at = (path: string) => evaluate(create({ kind: "directory", path, projectId: "prj_paseo_direct" }), split, direct);
      expect(at("C:\\work-sibling\\task").allowed).toBe(false);
      expect(at("D:\\work\\task").allowed).toBe(false);
      expect(at("C:\\work-sibling\\task")).toMatchObject({ reason: expect.stringContaining("placement roots") });
      expect(at("D:\\work\\task")).toMatchObject({ reason: expect.stringContaining("placement roots") });
      expect(at("C:\\work\\..scratch\\task")).toEqual({ allowed: true });
    } finally {
      vi.doUnmock("node:path");
      vi.resetModules();
    }
  });
});

describe("within", () => {
  it("treats only a .. segment as leaving the parent; ..checkout is a name inside it (AUD-R3-04)", () => {
    expect(within("/x/..checkout", "/x")).toBe(true);
    expect(within("/x/..checkout/repo", "/x")).toBe(true);
    expect(within("/x", "/x")).toBe(true);
    expect(within("/x/y", "/x")).toBe(true);
    expect(within("/x/..", "/x")).toBe(false);
    expect(within("/x/../y", "/x")).toBe(false);
    expect(within("/xy", "/x")).toBe(false);
    expect(within("/", "/x")).toBe(false);
  });
});
