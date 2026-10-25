import { describe, expect, it } from "vitest";
import {
  DEFAULT_CREDENTIAL_TIER,
  DEFAULT_MCP_COMMAND,
  readConfig,
} from "../server/config";

describe("readConfig", () => {
  it("defaults to the documented state root and observer authority", () => {
    const config = readConfig({ HOME: "/Users/test" });
    expect(config.stateRoot).toBe("/Users/test/.local/state/kontor/asma");
    expect(config.mcpCommand).toBe(DEFAULT_MCP_COMMAND);
    expect(config.cliCommand).toBe("kontor");
    expect(config.mcpEnabled).toBe(true);
    expect(config.serveProfile).toBeNull();
    expect(config.placementRoots).toEqual([]);
    expect(config.paseoWorktreesRoot).toBe("/Users/test/.paseo/worktrees");
    expect(DEFAULT_CREDENTIAL_TIER).toBe("observer");
  });

  it("follows a custom Paseo home for the worktree root", () => {
    const config = readConfig({ HOME: "/Users/test", PASEO_HOME: "/srv/paseo" });
    expect(config.paseoWorktreesRoot).toBe("/srv/paseo/worktrees");
  });

  it("reads the plugin's read-only overrides", () => {
    const config = readConfig({
      HOME: "/Users/test",
      KONTOR_PLUGIN_STATE_ROOT: "/srv/kontor",
      KONTOR_PLUGIN_MCP_COMMAND: "/opt/kontor-mcp",
      KONTOR_PLUGIN_CLI_COMMAND: "/opt/kontor",
      KONTOR_PLUGIN_MCP: "off",
      KONTOR_PLUGIN_SERVE_PROFILE: "leadership",
      KONTOR_PLUGIN_PLACEMENT_ROOTS: "/Users/test/repos, /Users/test/other",
      KONTOR_PLUGIN_LABEL_TIMEOUT_MS: "250",
    });
    expect(config.stateRoot).toBe("/srv/kontor");
    expect(config.mcpCommand).toBe("/opt/kontor-mcp");
    expect(config.cliCommand).toBe("/opt/kontor");
    expect(config.mcpEnabled).toBe(false);
    expect(config.serveProfile).toBe("leadership");
    expect(config.placementRoots).toEqual(["/Users/test/repos", "/Users/test/other"]);
    expect(config.labelReadTimeoutMs).toBe(250);
  });

  it("carries no credential setting", () => {
    const config = readConfig({ HOME: "/Users/test" });
    expect(Object.keys(config)).not.toContain("credentialTier");
    expect(Object.keys(config)).not.toContain("auth");
    expect(JSON.stringify(config)).not.toContain("KONTOR_AUTH");
  });
});
