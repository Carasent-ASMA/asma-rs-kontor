import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const here = dirname(fileURLToPath(import.meta.url));
const read = (path: string) => JSON.parse(readFileSync(resolve(here, path), "utf8"));

describe("Paseo version floor", () => {
  it("supports only the current Paseo release in the manifest and the SDK ranges", () => {
    expect(read("../paseo-plugin.json").requirements.paseo).toBe(">=0.17.4");
    const packageJson = read("../package.json");
    for (const name of ["@getpaseo/client", "@getpaseo/plugin", "@getpaseo/protocol"]) {
      expect(packageJson.devDependencies[name]).toBe(">=0.17.4");
    }
  });
});
