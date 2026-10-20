import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { readFloor } from "../server/floor";

// A file can grow between the size check and the read; fstat here reports the size before it grew.
vi.mock("node:fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("node:fs")>();
  return {
    ...actual,
    fstatSync: (descriptor: number) =>
      new Proxy(actual.fstatSync(descriptor), { get: (target, key) => (key === "size" ? 0 : Reflect.get(target, key)) }),
  };
});

describe("readFloor on a file that grows while it is read", () => {
  let dir: string;

  beforeEach(() => {
    dir = mkdtempSync(join(tmpdir(), "kontor-floor-growth-"));
  });

  afterEach(() => {
    rmSync(dir, { recursive: true, force: true });
  });

  it("keeps the floor enabled once more than 2 MiB arrives, even when the bytes read say off", () => {
    const file = join(dir, "floor.json");
    const body = '{"floor":"off"}';
    writeFileSync(file, body + " ".repeat(2 * 1024 * 1024 - body.length + 1));
    expect(readFloor(file)).toEqual({ enabled: true, reason: "invalid", file, error: "exceeds 2 MiB" });
  });
});
