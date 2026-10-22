import { readFileSync } from "node:fs";
import { URL } from "node:url";
import ts from "typescript";
import { describe, expect, it } from "vitest";
import * as directory from "../client/session-directory";
import type { useSessionAvailability as Hook } from "../client/use-session-availability";
import * as lifecycle from "../shared/session-lifecycle";
import type { SessionAvailability } from "../shared/session-lifecycle";
import { sessionEntry } from "./session-fixtures";

// Hermes (RN 0.81.5) evaluates plugin client bundles with function-scoped let/const: one binding per function call,
// and `let x;` resets it to undefined. Run the hook's own source that way, as the iOS app does.
function hermesScoped(source: string) {
  const scoping: ts.TransformerFactory<ts.SourceFile> = (context) => {
    const { factory } = context;
    const toVar = (list: ts.VariableDeclarationList, reset: boolean) => factory.createVariableDeclarationList(list.declarations.map((d) =>
      reset && !d.initializer ? factory.updateVariableDeclaration(d, d.name, d.exclamationToken, d.type, factory.createVoidZero()) : d));
    const visit = (node: ts.Node): ts.Node => {
      if (ts.isVariableStatement(node) && node.declarationList.flags & ts.NodeFlags.BlockScoped) node = factory.updateVariableStatement(node, node.modifiers, toVar(node.declarationList, true));
      else if (ts.isVariableDeclarationList(node) && node.flags & ts.NodeFlags.BlockScoped) node = toVar(node, false);
      return ts.visitEachChild(node, visit, context);
    };
    return (file) => ts.visitNode(file, visit) as ts.SourceFile;
  };
  return ts.transpileModule(source, { compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020 }, transformers: { before: [scoping] } }).outputText;
}
type Rpc = (name: string, input: { agentIds: string[] }) => Promise<Record<string, SessionAvailability>>;
/** React runs a queued updater when it next renders, which can be after the hook's read loop has moved on. */
function load(rpc: Rpc) {
  const slots: Array<{ value?: unknown; queue?: Array<(value: unknown) => unknown>; deps?: unknown[]; cleanup?: (() => void) | void }> = [];
  const effects: Array<() => void> = []; let cursor = 0;
  const slot = () => slots[cursor] ?? (slots[cursor] = {});
  const react = {
    useState: (initial: unknown) => { const s = slot(); cursor++; s.queue ??= []; if (!("value" in s)) s.value = typeof initial === "function" ? initial() : initial;
      for (const update of s.queue.splice(0)) s.value = update(s.value);
      return [s.value, (next: unknown) => s.queue!.push(typeof next === "function" ? next as (value: unknown) => unknown : () => next)]; },
    useRef: (initial: unknown) => { const s = slot(); cursor++; return s.value ?? (s.value = { current: initial }); },
    useMemo: (fn: () => unknown) => fn(),
    useEffect: (fn: () => (() => void) | void, deps: unknown[]) => { const s = slot(); cursor++; if (s.deps && deps.every((d, i) => Object.is(d, s.deps![i]))) return; s.deps = deps; effects.push(() => { s.cleanup?.(); s.cleanup = fn(); }); },
  };
  const modules: Record<string, unknown> = { react, "@getpaseo/plugin/client": { useRpc: (contract: { name: string }) => (input: { agentIds: string[] }) => rpc(contract.name, input) },
    "../shared/session-lifecycle": lifecycle, "./session-directory": directory };
  const module = { exports: {} as { useSessionAvailability: typeof Hook } };
  new Function("require", "module", "exports", hermesScoped(readFileSync(new URL("../client/use-session-availability.ts", import.meta.url), "utf8")))((name: string) => modules[name], module, module.exports);
  return <T>(draw: (hook: typeof Hook) => T) => { cursor = 0; const value = draw(module.exports.useSessionAvailability); for (const run of effects.splice(0)) run(); return value; };
}
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const read = (agentId: string, state: SessionAvailability["state"], extra: Partial<SessionAvailability> = {}): SessionAvailability =>
  ({ agentId, state, reason: "fictional native read", checkedAt: new Date().toISOString(), revision: sessionEntry(agentId).updatedAt, ...extra });

describe("session availability under the iOS (Hermes) scoping of plugin bundles", () => {
  it("settles each disposition batch when React renders while the next batch is read", async () => {
    const entries = Array.from({ length: 150 }, (_, n) => sessionEntry(`fixture-${n}`));
    const pending: Array<{ ids: string[]; resolve: (value: Record<string, SessionAvailability>) => void }> = [];
    const render = load((_name, input) => new Promise((resolve) => pending.push({ ids: input.agentIds, resolve })));
    const draw = () => render((hook) => hook("host", entries, false));
    draw(); pending[0]!.resolve({ "fixture-7": read("fixture-7", "missing", { localRemoval: true }) }); await settle();
    expect(pending.map((call) => call.ids.length)).toEqual([100, 50]);
    expect(() => draw()).not.toThrow();
    pending[1]!.resolve({ "fixture-107": read("fixture-107", "missing", { localRemoval: true }), "fixture-109": read("fixture-109", "readable", { archivedAt: "2026-10-21" }) }); await settle();
    const shown = draw().entries;
    expect(shown).toHaveLength(148); expect(shown.map((entry) => entry.id)).not.toContain("fixture-7");
    expect(shown.map((entry) => entry.id)).not.toContain("fixture-107"); expect(shown.find((entry) => entry.id === "fixture-109")?.archivedAt).toBe("2026-10-21");
  });
  it("keeps every availability batch a probe worker reads before React renders", async () => {
    const entries = Array.from({ length: 100 }, (_, n) => sessionEntry(`fixture-${n}`, { requiresAttention: true, attentionReason: "finished" }));
    const render = load(async (name, input) => name === "sessions.availability" ? Object.fromEntries(input.agentIds.map((id) => [id, read(id, "readable")])) : {});
    const draw = () => render((hook) => hook("host", entries, true));
    draw(); await settle();
    expect(draw().entries.filter((entry) => entry.availability?.state === "readable")).toHaveLength(100);
  });
});
