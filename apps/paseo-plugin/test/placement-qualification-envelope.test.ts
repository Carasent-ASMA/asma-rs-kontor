import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import ts from "typescript";
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
const REQUIRED: Record<string, string[]> = {
  STUBS: ["agy", "claude", "codex", "copilot", "cursor-agent", "fx", "goose", "hermes", "hoplite", "muse", "omp", "opencode", "pi"],
  DENIED: ["security"],
};
const BUILDER = 'for (const name of [...STUBS, ...DENIED]) writeFileSync(join(made, "bin", name), stub(name, join(made, "provider-invocations.jsonl"), STUBS.includes(name)), { mode: 0o700 });';
const CHECKER_BODY = [
  'const found = spawnSync("/usr/bin/which", ["-a", name], { encoding: "utf8" }).stdout.split("\\n").filter(Boolean);',
  'if (found[0] !== join(box, "bin", name) || (STUBS.includes(name) && found.length !== 1)) faults.push(`${name} resolves to ${found.join(", ") || "nothing"}`);',
];

/**
 * What breaks the deny-tool contract in the harness's active code; empty when it holds. The source is parsed, not
 * searched: comments are trivia to the parser and a string is not a statement, so a commented-out or quoted copy
 * counts for nothing (AUD-B-R7-01). A list must be one top-level const array of strings that is otherwise only read.
 * The builder and the checker loop must each be one direct statement of their block, with no earlier statement able
 * to end the run other than by a non-zero `process.exit` (the outer run and the inner check are exclusive branches).
 * Not covered: exits by other means (an alias of process.exit, a signal, a hang) and changes to `faults` itself.
 */
function contractFaults(source: string): string[] {
  const file = ts.createSourceFile("placement-qualification.mjs", source, ts.ScriptTarget.Latest, true, ts.ScriptKind.JS);
  const text = (node: ts.Node) => node.getText(file);
  const faults: string[] = [];
  const lists: Record<string, ts.Expression | undefined> = {};
  const scan = (node: ts.Node): void => {
    if (ts.isIdentifier(node) && node.text in REQUIRED) {
      const p = node.parent;
      const declared = ts.isVariableDeclaration(p) && p.name === node && ts.isVariableStatement(p.parent.parent) && p.parent.parent.parent === file && (p.parent.flags & ts.NodeFlags.Const) !== 0 && !(node.text in lists);
      const read = ts.isSpreadElement(p) || (ts.isPropertyAssignment(p) && p.initializer === node) || (ts.isForOfStatement(p) && p.expression === node)
        || (ts.isPropertyAccessExpression(p) && p.expression === node && p.name.text === "includes" && ts.isCallExpression(p.parent) && p.parent.expression === p);
      if (declared) lists[node.text] = p.initializer;
      else if (!read) faults.push(`${node.text} is bound or changed: ${text(p)}`);
    }
    ts.forEachChild(node, scan);
  };
  scan(file);
  for (const [name, required] of Object.entries(REQUIRED)) {
    const list = lists[name];
    if (!list || !ts.isArrayLiteralExpression(list) || !list.elements.every(ts.isStringLiteral)) faults.push(`${name} is not a top-level const array of strings`);
    else {
      const names = list.elements.map((element) => (element as ts.StringLiteral).text).sort();
      if (JSON.stringify(names) !== JSON.stringify(required)) faults.push(`${name} lists ${JSON.stringify(names)}, the contract is ${JSON.stringify(required)}`);
    }
  }
  const exits = (node: ts.Node, label: string): void => {
    if (ts.isCallExpression(node) && text(node.expression) === "process.exit") {
      const [code] = node.arguments;
      if (!(code && ts.isNumericLiteral(code) && Number(code.text) !== 0)) faults.push(`${label} is not reached: ${text(node)} runs first`);
    }
    ts.forEachChild(node, (child) => exits(child, label));
  };
  const reached = (label: string, body: readonly ts.Statement[], is: (statement: ts.Statement) => boolean, exclusive?: ts.Statement) => {
    const found = body.filter(is);
    if (found.length !== 1) return void faults.push(`${label}: ${found.length} active statements, expected 1`);
    for (const statement of body.slice(0, body.indexOf(found[0]!))) if (statement !== exclusive) exits(statement, label);
    return found[0];
  };
  const guard = reached("the outer run's `if (!box)` block", file.statements, (s) => ts.isIfStatement(s) && text(s.expression) === "!box") as ts.IfStatement | undefined;
  if (guard) reached("the stub builder", ts.isBlock(guard.thenStatement) ? guard.thenStatement.statements : [], (s) => text(s) === BUILDER);
  const check = reached("the envelope check block", file.statements, (s) => ts.isBlock(s) && !!s.statements[0] && text(s.statements[0]).startsWith("const faults = "), guard) as ts.Block | undefined;
  const loop = check && reached("the checker loop", check.statements, (s) => ts.isForOfStatement(s) && text(s.initializer) === "const name" && text(s.expression) === "[...STUBS, ...DENIED]") as ts.ForOfStatement | undefined;
  if (check && loop) {
    if (!ts.isBlock(loop.statement) || JSON.stringify(loop.statement.statements.map(text)) !== JSON.stringify(CHECKER_BODY)) faults.push("the checker loop does not look up and check each name");
    const refusal = reached("the refusal on faults", check.statements.slice(check.statements.indexOf(loop) + 1), (s) => ts.isIfStatement(s) && text(s.expression) === "faults.length") as ts.IfStatement | undefined;
    if (refusal) reached("the refusal's exit", ts.isBlock(refusal.thenStatement) ? refusal.thenStatement.statements : [], (s) => text(s) === "process.exit(3);");
  }
  return faults;
}

const source = readFileSync(resolve(__dirname, "../scripts/placement-qualification.mjs"), "utf8");
const fixture = (name: string) => readFileSync(resolve(__dirname, "fixtures/placement-qualification", name), "utf8");
const mutate = (...edits: [string, string][]) => edits.reduce((text, [from, to]) => {
  expect(text.split(from)).toHaveLength(2);
  return text.replace(from, to);
}, source);
const LOOP = "  for (const name of [...STUBS, ...DENIED]) {\n";

// Inactive or dead code that the earlier text checks accepted. AUD-C1 and AUD-C2 are AUD r7's fixtures, kept byte for
// byte; the SWE mutants are applied to the live harness source. Each must fail for its own reason.
const NEGATIVE: [string, () => string, string[]][] = [
  ["AUD-C1: the checker loop is commented out", () => fixture("AUD-C1-commented-out-checker.mjs"), ["the checker loop: 0 active statements, expected 1"]],
  ["AUD-C2: a commented copy of DENIED precedes an empty live one", () => fixture("AUD-C2-dead-denied-copy.mjs"), ['DENIED lists [], the contract is ["security"]']],
  ["SWE-M1: the checker loop is dead code under if (false)", () => mutate([LOOP, `  if (false)${LOOP.slice(1)}`]), ["the checker loop: 0 active statements, expected 1"]],
  ["SWE-M2: an empty DENIED shadows the list in the builder and the check", () => mutate(["if (!box) {\n", "if (!box) {\n  const DENIED = [];\n"], ["  if (process.env.HOME === userInfo().homedir)", "  const DENIED = [];\n  if (process.env.HOME === userInfo().homedir)"]),
    ["DENIED is bound or changed: DENIED = []", "DENIED is bound or changed: DENIED = []"]],
  ["SWE-M3: the checker header survives as a string; the live loop skips DENIED", () => mutate([LOOP, `  "${LOOP.trim()}";\n  for (const name of STUBS) {\n`]), ["the checker loop: 0 active statements, expected 1"]],
  ["SWE-M4: an early envelope-only report exits 0 before the checker loop", () => mutate([LOOP, `  if (envelopeOnly) { console.log(JSON.stringify({ envelope: "in place", stubs: STUBS, denied: DENIED })); process.exit(0); }\n${LOOP}`]),
    ["the checker loop is not reached: process.exit(0) runs first"]],
];

describe("placement qualification envelope deny-tool contract", () => {
  it("holds in the harness's active code: the 13 provider stubs, security refused, built and checked", () => {
    expect(contractFaults(source)).toEqual([]);
  });

  it.each(NEGATIVE)("rejects %s", (_, text, faults) => {
    expect(contractFaults(text())).toEqual(faults);
  });
});
