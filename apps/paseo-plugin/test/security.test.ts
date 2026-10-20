import { readdirSync, readFileSync } from "node:fs";
import { URL } from "node:url";
import { describe, expect, it } from "vitest";
import { DEFAULT_CREDENTIAL_TIER } from "../server/config";

const files = [
  "../index.server.ts",
  "../server/agent-labels.ts",
  "../server/config.ts",
  "../server/floor.ts",
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
  "../client/mobile-filters.tsx",
  "../client/anchored-menu.tsx",
  "../client/graph-page.tsx",
  "../client/kontor-screen.tsx",
  "../client/native-freshness.tsx",
  "../client/navigation-state.ts",
  "../client/organization-row.tsx",
  "../client/overview-page.tsx",
  "../client/page-controls.tsx",
  "../client/page-data.ts",
  "../client/plan-document.tsx",
  "../client/plan-links.ts",
  "../client/plan-reader.tsx",
  "../client/read-only-pages.tsx",
  "../client/source-details.tsx",
  "../client/teams-binding-detail.tsx",
  "../client/teams-role-cards.tsx",
  "../client/teams-source-page.tsx",
  "../index.client.tsx",
  "../client/return-control.ts",
  "../client/session-actions.ts",
  "../client/session-directory.ts",
  "../client/session-organization.ts",
  "../client/session-view-store.ts",
  "../client/session-view.ts",
  "../client/sessions-screen.tsx",
  "../client/toolbar-visibility.ts",
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

describe("floor switch", () => {
  const pluginSources = ["../index.server.ts", "../index.client.tsx",
    ...["server", "shared", "client"].flatMap((dir) =>
      readdirSync(new URL(`../${dir}/`, import.meta.url)).filter((name) => /\.tsx?$/.test(name)).map((name) => `../${dir}/${name}`))];

  it("never reads the superseded environment name", () => {
    expect(pluginSources.length).toBeGreaterThan(20);
    for (const file of pluginSources) {
      expect(source(file), file).not.toContain("KONTOR_PLUGIN_FLOOR");
    }
  });

  it("opens floor.json read-only and never changes it", () => {
    const text = source("../server/floor.ts");
    expect(text).toContain("openSync(file, constants.O_RDONLY | constants.O_NONBLOCK)");
    expect(text).not.toMatch(/O_WRONLY|O_RDWR|O_CREAT|O_TRUNC|O_APPEND|unlink|rename|rmSync|chmod|truncate|mkdir|copyFile/);
  });
});
