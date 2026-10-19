import { readFileSync } from "node:fs";
import { URL } from "node:url";
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

const clientFiles = [
  "../client/graph-page.tsx",
  "../client/kontor-screen.tsx",
  "../client/native-freshness.tsx",
  "../client/navigation-state.ts",
  "../client/page-data.ts",
  "../client/plan-document.tsx",
  "../client/plan-links.ts",
  "../client/read-only-pages.tsx",
  "../client/source-details.tsx",
  "../client/teams-binding-detail.tsx",
  "../client/teams-role-cards.tsx",
  "../client/teams-source-page.tsx",
  "../index.client.tsx",
  "../client/return-control.ts",
  "../client/session-actions.ts",
  "../client/session-directory.ts",
  "../client/session-view-store.ts",
  "../client/session-view.ts",
  "../client/sessions-screen.tsx",
  "../client/use-session-directory.ts",
] as const;

describe("client navigation stays on the borrowed host connection", () => {
  it("names no Kontor credential, Kontor daemon or file/process access", () => {
    for (const file of clientFiles) {
      const text = source(file);
      expect(text, file).not.toContain("KONTOR_AUTH");
      expect(text, file).not.toContain("credentials.json");
      expect(text, file).not.toContain("kontor-mcp");
      expect(text, file).not.toMatch(
        /node:child_process|node:fs|node:net|node:http|writeFile|appendFile|fetch\(/,
      );
    }
  });

  it("contains no agent-control or workflow-write call", () => {
    for (const file of clientFiles) {
      const text = source(file);
      expect(text, file).not.toMatch(
        /respondToPermission|agents\.ref\(|\.send\(|\.run\(|\.archive\(|workspaces\.(?:create|archive)|config\.patch/,
      );
    }
  });
});
