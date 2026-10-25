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
  "../client/session-compact-content.tsx",
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
  "../client/kontord-copy.ts",
  "../client/kontord-banner.tsx",
  "../client/use-kontor-status.ts",
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
  "../client/start-switch-sheet.tsx",
  "../client/start-switch-archive.ts",
  "../client/start-switch-statuses.tsx",
  "../client/session-state-shortcuts.ts",
  "../client/new-lsa-form.tsx",
  "../client/recent-sessions.ts",
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

describe("kontord status probe", () => {
  const text = source("../server/kontor-status.ts");

  it("starts only the configured kontor-mcp with fixed arguments, and reads one observer tool", () => {
    expect(text.match(/spawn\(/g)).toHaveLength(1);
    expect(text).toContain(`["--state-root", config.stateRoot, "--credential-tier", DEFAULT_CREDENTIAL_TIER, ...(config.serveProfile ? ["--serve-profile", config.serveProfile] : [])]`);
    expect(text).toContain("spawn(config.mcpCommand, args, {");
    expect(text).not.toMatch(/shell:/);
    expect([...new Set(text.match(/kontor_[a-z_]+/g))]).toEqual(["kontor_realm_get"]);
  });

  it("never handles a credential, writes or opens its own connection", () => {
    expect(text).not.toMatch(/[\"'](?:admin|operator)[\"']|credentials\.json|KONTOR_AUTH|daemon-keypair/);
    expect(text).not.toMatch(/node:fs|writeFile|appendFile|createWriteStream|execFile|exec\(|fetch\(|node:http|node:net/);
    // The child sees PATH and HOME only, its standard error is never read, and it is killed outright.
    expect(text).toContain(`env: { PATH: process.env.PATH ?? "", HOME: process.env.HOME ?? "" }`);
    expect(text).toContain(`stdio: ["pipe", "pipe", "ignore"]`);
    expect(text).toContain(`child.kill("SIGKILL")`);
  });

  it("is registered whatever the floor, reports it, and is stopped with the worker", () => {
    const entry = source("../index.server.ts");
    expect(entry).toContain("const stopStatus = contributeKontorStatus(server, floor.enabled);");
    expect(entry).toContain("const stopHooks = floor.enabled ? contribute(server) : () => {};");
    expect(entry).toContain("return () => { stopSources(); stopStatus(); stopHooks(); };");
  });
});

describe("bounded session create", () => {
  const text = source("../server/session-create.ts");

  it("creates through one idempotent native call and one lead workspace, and never archives, renames or patches", () => {
    expect(text.match(/agents\.create\(/g)).toHaveLength(1);
    expect(text).toContain("agents.create({ idempotencyKey: requestId,");
    expect(text.match(/workspaces\.create\(/g)).toHaveLength(1);
    expect(text).toContain("client.workspaces.create({ source:");
    expect(text).not.toMatch(/\.send\(|\.archive\(|ref\([^)]*\)\.update\(|setTitle|respondToPermission|config\.patch|initialPrompt/);
  });

  it("only reads the operator's create switch", () => {
    expect(text).toContain('readFloor(file, "create")');
    expect(text).not.toMatch(/(?:open|writeFile|rename|rm)\((?:file|switchFile)\b/);
  });
});

describe("bounded fleet selection read", () => {
  const text = source("../server/session-takeover.ts");

  it("runs only the configured kontor command with one fixed argument vector and the operator tier for that child alone", () => {
    expect(text.match(/execFile\(/g)).toHaveLength(1);
    expect(text).toContain("execFile(command, args, { env: { PATH: process.env.PATH ?? \"\", HOME: process.env.HOME ?? \"\" },");
    expect(text).toContain("const args = [\"--state-root\", config.stateRoot, \"--tier\", \"operator\", \"fleet-policy-resolve\", `--binding-key=${key}`, `--route-health=${JSON.stringify(evidence.data)}`];");
    expect(text).toContain("await run(config.cliCommand, args)");
    expect(text.match(/"operator"/g)).toHaveLength(1);
    expect([...new Set(text.match(/fleet-policy-resolve|kontor_[a-z_]+/g))]).toEqual(["kontor_fleet_policy_resolve", "fleet-policy-resolve"]);
  });

  it("raises no tier, handles no credential and opens no other surface", () => {
    expect(text).not.toMatch(/shell:|spawn\(|exec\(|process\.env\.[A-Z_]*=|KONTOR_TIER|"admin"|credentials\.json|KONTOR_AUTH|daemon-keypair/);
    expect(text).not.toMatch(/node:fs|writeFile|appendFile|createWriteStream|fetch\(|node:http|node:net|kontor-mcp/);
  });
});
