import { readFileSync } from "node:fs";
import { URL } from "node:url";
import ts from "typescript";

// Hermes (RN 0.81.5) evaluates plugin client bundles with function-scoped let/const: one binding per function call,
// and `let x;` resets it to undefined. A closure made in a loop therefore sees the loop's latest values on iOS.
export function hermesScoped(source: string, fileName: string) {
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
  return ts.transpileModule(source, { fileName, compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2020, jsx: ts.JsxEmit.ReactJSX },
    transformers: { before: [scoping] } }).outputText;
}
/** Runs a client module's own source as the iOS app does; `modules` answers its imports. */
export function loadHermesScoped<T>(path: string, modules: Record<string, unknown>): T {
  const file = new URL(path, import.meta.url), module = { exports: {} as T };
  const require = (name: string) => { if (!(name in modules)) throw new Error(`No test module for ${name}`); return modules[name]; };
  new Function("require", "module", "exports", hermesScoped(readFileSync(file, "utf8"), file.pathname))(require, module, module.exports);
  return module.exports;
}
/** React runs a queued updater when it next renders, which can be after a hook's loop has moved on. */
export function deferredReact() {
  const slots: Array<{ value?: unknown; queue?: Array<(value: unknown) => unknown>; deps?: unknown[]; cleanup?: (() => void) | void }> = [];
  const effects: Array<() => void> = []; let cursor = 0;
  const slot = () => slots[cursor] ?? (slots[cursor] = {});
  const react = {
    useState: (initial: unknown) => { const s = slot(); cursor++; s.queue ??= []; if (!("value" in s)) s.value = typeof initial === "function" ? initial() : initial;
      for (const update of s.queue.splice(0)) s.value = update(s.value);
      return [s.value, (next: unknown) => s.queue!.push(typeof next === "function" ? next as (value: unknown) => unknown : () => next)]; },
    useRef: (initial: unknown) => { const s = slot(); cursor++; return s.value ?? (s.value = { current: initial }); },
    useMemo: (fn: () => unknown) => fn(), useCallback: (fn: unknown) => fn,
    useEffect: (fn: () => (() => void) | void, deps: unknown[]) => { const s = slot(); cursor++; if (s.deps && deps.every((d, i) => Object.is(d, s.deps![i]))) return; s.deps = deps; effects.push(() => { s.cleanup?.(); s.cleanup = fn(); }); },
  };
  const render = <T>(draw: () => T) => { cursor = 0; const value = draw(); for (const run of effects.splice(0)) run(); return value; };
  return { react, render };
}
