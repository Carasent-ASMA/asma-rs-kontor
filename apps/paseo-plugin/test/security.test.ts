import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { DEFAULT_CREDENTIAL_TIER } from "../server/config";

const files = [
  "../index.server.ts",
  "../server/agent-labels.ts",
  "../server/config.ts",
  "../server/hooks.ts",
  "../server/labels.ts",
  "../server/placement.ts",
  "../server/placement-binding.ts",
] as const;

function source(relative: string): string {
  return readFileSync(new URL(relative, import.meta.url), "utf8");
}

describe("no Kontor write credential", () => {
  it("always composes the Kontor MCP at observer authority", () => {
    expect(DEFAULT_CREDENTIAL_TIER).toBe("observer");
  });

  it("never names a writable tier, a credential file or a seat token", () => {
    for (const file of files) {
      const text = source(file);
      expect(text, file).not.toMatch(/[\"'](?:admin|operator)[\"']/);
      expect(text, file).not.toContain("credentials.json");
      expect(text, file).not.toContain("KONTOR_AUTH");
      expect(text, file).not.toContain("daemon-keypair");
    }
  });

  it("holds no write surface of its own", () => {
    for (const file of files) {
      const text = source(file);
      expect(text, file).not.toMatch(
        /writeFile|appendFile|createWriteStream|execFile|spawn\(|node:child_process|fetch\(|node:http|node:net/,
      );
    }
  });
});
