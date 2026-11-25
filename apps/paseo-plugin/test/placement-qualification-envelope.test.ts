import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

// The deny-tool contract of the placement qualification envelope, written here independently of the harness's own
// STUBS and DENIED lists, which both build the stubs and check them: a name dropped from those lists is dropped from
// both, and the envelope check still passes (AUD-B-R6-01). Traced in installed Paseo 0.17.4
// (app.asar 74294de565e88be702d1e8b6a4f4107747e0c10be1fa8b40a8eddc212c200f4e):
// - core providers: server/agent/provider-registry.js:53-91 (claude, codex, copilot, cursor as `cursor-agent`,
//   opencode, pi, omp);
// - bundled provider plugins: server/plugins/builtin/index.js:5-21 (antigravity as `agy`, fx, goose, hermes,
//   hoplite, muse);
// - builtin-plugins/claude-usage-source/server/usage.ts:308 runs bare `security` to read a Keychain item.
// Each provider stub may answer `--version` only; `security` is always refused.
const REQUIRED_PROVIDER_STUBS = ["agy", "claude", "codex", "copilot", "cursor-agent", "fx", "goose", "hermes", "hoplite", "muse", "omp", "opencode", "pi"];
const REQUIRED_DENIED = ["security"];

const source = readFileSync(resolve(__dirname, "../scripts/placement-qualification.mjs"), "utf8");
function list(name: string): string[] {
  const match = new RegExp(`^const ${name} = (\\[[^\\]]*\\]);$`, "m").exec(source);
  expect(match, `${name} is declared as one array literal`).not.toBeNull();
  return JSON.parse(match![1]!) as string[];
}
/** The source between two markers, each found exactly once. */
function block(from: string, to: string): string {
  expect(source.split(from)).toHaveLength(2);
  const start = source.indexOf(from), end = source.indexOf(to, start);
  expect(end).toBeGreaterThan(start);
  return source.slice(start, end);
}

describe("placement qualification envelope deny-tool contract", () => {
  it("stubs exactly the 13 provider executables and refuses security", () => {
    expect([...list("STUBS")].sort()).toEqual(REQUIRED_PROVIDER_STUBS);
    expect([...list("DENIED")].sort()).toEqual(REQUIRED_DENIED);
  });

  it("builds a stub for every listed name, and only providers answer --version", () => {
    const builder = block("if (!box) {", "const inner = spawnSync(");
    expect(builder).toContain('for (const name of [...STUBS, ...DENIED]) writeFileSync(join(made, "bin", name), stub(name, join(made, "provider-invocations.jsonl"), STUBS.includes(name))');
  });

  it("checks every listed name resolves first to its stub, and each provider only once", () => {
    const checker = block("const faults = Object.keys(process.env)", "if (faults.length)");
    expect(checker).toContain("for (const name of [...STUBS, ...DENIED]) {");
    expect(checker).toContain('const found = spawnSync("/usr/bin/which", ["-a", name]');
    expect(checker).toContain('if (found[0] !== join(box, "bin", name) || (STUBS.includes(name) && found.length !== 1)) faults.push(');
  });
});
