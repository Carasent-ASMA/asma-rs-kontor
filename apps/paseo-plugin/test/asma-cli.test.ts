import { chmod, mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, isAbsolute, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";

import { ASMA_COMMAND_ENV, resolveAsmaCommand } from "../server/asma-cli";

const temporary: string[] = [];
afterEach(async () => { await Promise.all(temporary.splice(0).map((path) => rm(path, { recursive: true, force: true }))); });

async function home(): Promise<string> {
  const directory = await mkdtemp(join(tmpdir(), "kontor-asma-cli-"));
  temporary.push(directory);
  return directory;
}

async function executable(path: string): Promise<void> {
  await mkdir(dirname(path), { recursive: true });
  await writeFile(path, "#!/bin/sh\nexit 0\n");
  await chmod(path, 0o755);
}

describe("trusted asma executable resolution", () => {
  it("finds the absolute user-local install even when PATH cannot resolve asma", async () => {
    const directory = await home();
    const command = join(directory, ".local/bin/asma");
    await executable(command);
    expect(await resolveAsmaCommand({ env: { HOME: directory, PATH: "/usr/bin:/bin" }, home: directory }))
      .toBe(command);
  });

  it("fails closed without testing a bare command when no trusted path exists", async () => {
    const directory = await home();
    const seen: string[] = [];
    await expect(resolveAsmaCommand({
      env: { HOME: directory, PATH: "/usr/bin:/bin" },
      home: directory,
      inspect: async (path) => { seen.push(path); return false; },
    })).rejects.toThrow("trusted absolute path");
    expect(seen.length).toBeGreaterThan(0);
    expect(seen.every(isAbsolute)).toBe(true);
    expect(seen).not.toContain("asma");
  });

  it("uses the configured workspace .venv when no user-local install exists", async () => {
    const directory = await home();
    const command = join(directory, "workspace/.venv/bin/asma");
    await executable(command);
    expect(await resolveAsmaCommand({
      env: { HOME: directory, PATH: "/usr/bin:/bin", KONTOR_PLUGIN_ASMA_WORKSPACE: join(directory, "workspace") },
      home: directory,
    })).toBe(command);
  });

  it("fails closed on a relative or missing absolute override instead of choosing another binary", async () => {
    const directory = await home();
    await executable(join(directory, ".local/bin/asma"));
    await expect(resolveAsmaCommand({
      env: { HOME: directory, PATH: "/usr/bin:/bin", [ASMA_COMMAND_ENV]: "asma" },
      home: directory,
    })).rejects.toThrow("absolute path");
    await expect(resolveAsmaCommand({
      env: { HOME: directory, PATH: "/usr/bin:/bin", [ASMA_COMMAND_ENV]: join(directory, "missing/asma") },
      home: directory,
    })).rejects.toThrow("executable asma file");
  });
});
