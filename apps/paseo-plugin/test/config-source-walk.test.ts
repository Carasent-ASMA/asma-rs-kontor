import { beforeEach, describe, expect, it, vi } from "vitest";

/**
 * The source-walk branches that a real filesystem cannot easily produce (an ancestor whose lstat succeeds but
 * whose stat follows a link to a file, and a root that cannot be resolved) are exercised through a synthetic
 * virtual root. The loader's own imported fs functions are mocked for this file only; no real path is read.
 */
const state = vi.hoisted(() => ({ mode: "root" as string, roots: 0 }));

vi.mock("node:fs/promises", async (original) => {
  const actual: any = await original();
  const absent = () => Object.assign(new Error("absent"), { code: "ENOENT" });
  return {
    ...actual,
    lstat: async (path: string) => {
      if (path === "/__walk__/a/b/opencode.json" || path === "/__walk__/a/b" || path === "/__walk__/a/opencode.json") throw absent();
      if (path === "/__walk__/a") {
        if (state.mode === "file" || state.mode === "loop") return {};
        if (state.mode === "access") throw Object.assign(new Error("denied"), { code: "EACCES" });
        throw absent();
      }
      if (path === "/__walk__") throw absent();
      if (path === "/") {
        state.roots += 1;
        if (state.mode === "missing") {
          if (state.roots > 3) throw Object.assign(new Error("bounded"), { code: "EIO" });
          throw absent();
        }
        return {};
      }
      return actual.lstat(path);
    },
    stat: async (path: string) => {
      if (path === "/__walk__/a") {
        if (state.mode === "loop") throw Object.assign(new Error("loop"), { code: "ELOOP" });
        return { isDirectory: () => state.mode !== "file" };
      }
      if (path === "/") return { isDirectory: () => true };
      return actual.stat(path);
    },
  };
});

import { readOpenCodeConfig } from "../server/opencode-config";

beforeEach(() => { state.mode = "root"; state.roots = 0; });

const run = (mode: string) => {
  state.mode = mode;
  return readOpenCodeConfig("/__walk__/a/b", {
    env: {}, home: "/__walk__/home", system: false, stopAt: "/__walk__",
    readFile: async () => { throw Object.assign(new Error("absent"), { code: "ENOENT" }); },
  });
};

describe("configuration source walk", () => {
  it("keeps a genuinely missing chain under a real root absent", async () => {
    expect(await run("root")).toEqual({ documents: [] });
  });

  it("rejects an access error during the walk", async () => {
    const read = await run("access");
    expect("problem" in read).toBe(true);
    if ("problem" in read) expect(read.problem).toContain("/__walk__/a/opencode.json");
  });

  it("rejects a file ancestor", async () => {
    expect("problem" in await run("file")).toBe(true);
  });

  it("rejects a directory loop during the walk", async () => {
    expect("problem" in await run("loop")).toBe(true);
  });

  it("terminates once at an unresolvable root", async () => {
    const read = await run("missing");
    expect("problem" in read).toBe(true);
    expect(state.roots).toBe(1);
  });
});
