import type { ReactNode } from "react";
import { afterEach, expect, it, vi } from "vitest";

// Reuse the pinned workspace renderer and console jsdom; no feature-specific dependency install.
const runtime = await vi.hoisted(async () => {
  const { createRequire } = await import("node:module");
  const { resolve } = await import("node:path");
  const renderer = createRequire(
    resolve(process.env.RENDERER_PACKAGE_JSON ?? "../kontor-prototype/package.json"),
  );
  const tools = createRequire(resolve(process.env.JSDOM_PACKAGE_JSON ?? "../console/package.json"));
  const React = renderer("react");
  if (React.version !== "19.1.0" || renderer("react-dom/package.json").version !== "19.1.0")
    throw new Error("Use the pinned React/ReactDOM19.1 renderer");
  return {
    React,
    createRoot: renderer("react-dom/client").createRoot,
    JSDOM: tools("jsdom").JSDOM,
    calls: {} as Record<string, ReturnType<typeof vi.fn>>,
    inputs: {} as Record<string, (value: string) => void>,
  };
});
vi.mock("react", () => ({ ...runtime.React, default: runtime.React }));
vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: (rpc: { name: string }) => runtime.calls[rpc.name],
}));
vi.mock("react-native", () => ({
  View: ({ children }: { children: ReactNode }) => <div>{children}</div>,
  Text: ({ children }: { children: ReactNode }) => <span>{children}</span>,
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({
  TextInput: ({
    accessibilityLabel,
    onChangeText,
    value,
  }: {
    accessibilityLabel: string;
    onChangeText: (value: string) => void;
    value: string;
  }) => {
    runtime.inputs[accessibilityLabel] = onChangeText;
    return <input aria-label={accessibilityLabel} value={value} readOnly />;
  },
}));
vi.mock("../client/operation-sheet", () => ({
  OperationSheet: ({ children, footer }: { children: ReactNode; footer: ReactNode }) => (
    <section>
      {children}
      {footer}
    </section>
  ),
  SheetFooter: ({
    primary,
  }: {
    primary: { disabled: boolean; onPress: () => void; label: string };
  }) => (
    <button data-primary disabled={primary.disabled} onClick={primary.onPress}>
      {primary.label}
    </button>
  ),
}));
import React from "react";
import { PreparedTakeoverSheet } from "../client/prepared-takeover";
import type { DirectoryEntry } from "../client/session-directory";
import type { Colors } from "../client/page-controls";

let root: { render: (node: ReactNode) => void; unmount: () => void } | null = null;
let dom: {
  window: {
    document: {
      getElementById: (id: string) => unknown;
      querySelector: (
        selector: string,
      ) => { disabled: boolean; textContent: string | null; click: () => void } | null;
      body: { textContent: string | null };
    };
    HTMLElement: unknown;
    close: () => void;
  };
} | null = null;
afterEach(async () => {
  if (root) await runtime.React.act(async () => root!.unmount());
  root = null;
  dom?.window.close();
  dom = null;
  vi.unstubAllGlobals();
});

it.each(["import", "execute", "check"] as const)(
  "updates a remounted sheet when pending %s resolves or rejects",
  async (stage) => {
    for (const rejected of [false, true]) {
      dom = new runtime.JSDOM("<!doctype html><div id='root'></div>");
      vi.stubGlobal("window", dom!.window);
      vi.stubGlobal("document", dom!.window.document);
      vi.stubGlobal("HTMLElement", dom!.window.HTMLElement);
      vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
      let finish!: (value: unknown) => void, fail!: (reason: Error) => void;
      const pending = new Promise((yes, no) => {
        finish = yes;
        fail = no;
      });
      const sourceId = "00000000-0000-4000-8000-000000000001";
      const receipt = {
        sourceId,
        successorId: "00000000-0000-4000-8000-000000000002",
        operationId: "00000000-0000-4000-8000-000000000003",
        admissionHash: "a".repeat(64),
        revision: 0,
        phase: "prepared",
        message: "Prepared only",
        admission: {
          mode: { workflow: "free", orchestration: "paseo" },
          config: { provider: "codex", model: "fixture", effort: "low", mode: "read-only" },
          cwd: "/fixture",
          checkout: { head: "1".repeat(40) },
        },
      };
      const answer = { receipt, canExecute: true, limits: ["No cross-client fence"] };
      runtime.calls = {
        "sessions.procedural-takeover.import": vi.fn(() =>
          stage === "import" ? pending : Promise.resolve(answer),
        ),
        "sessions.procedural-takeover.execute": vi.fn(() =>
          stage === "execute" ? pending : Promise.reject(new Error("lost execute reply")),
        ),
        "sessions.procedural-takeover.check": vi.fn(() => pending),
      };
      const props = {
        hostId: `lifetime-${stage}-${rejected}`,
        source: { id: sourceId, title: "Synthetic source" } as DirectoryEntry,
        colors: { foreground: "#111", statusWarning: "#900" } as Colors,
        onClose: () => {},
      };
      const mount = async () => {
        root = runtime.createRoot(dom!.window.document.getElementById("root")!);
        await runtime.React.act(async () => root!.render(<PreparedTakeoverSheet {...props} />));
      };
      const button = () => dom!.window.document.querySelector("button[data-primary]")!;
      const click = async () => {
        await runtime.React.act(async () => button().click());
      };
      await mount();
      await runtime.React.act(async () => {
        runtime.inputs["Admission host path"]!("/fixture/admission");
        runtime.inputs["Admission SHA-256"]!("a".repeat(64));
      });
      await click();
      if (stage !== "import") await click();
      if (stage === "check") await click();
      expect(button().disabled).toBe(true);
      await runtime.React.act(async () => root!.unmount());
      root = null;
      await mount();
      expect(button().disabled).toBe(true);
      await runtime.React.act(async () => {
        if (rejected) fail(new Error(`lost ${stage} reply`));
        else
          finish(
            stage === "import"
              ? answer
              : { ...receipt, revision: 1, phase: "held", message: "Observed exact operation" },
          );
        await pending.catch(() => {});
      });
      if (rejected && stage === "import") {
        expect(dom!.window.document.body.textContent).toContain("lost import reply");
        expect(button().textContent).toBe("Read prepared admission");
      } else {
        expect(button().disabled).toBe(false);
        expect(button().textContent).toBe(
          stage === "import" ? "Confirm procedural takeover" : "Check same operation",
        );
      }
      if (rejected && stage !== "import")
        expect(dom!.window.document.body.textContent).toContain("Unconfirmed");
      expect(runtime.calls["sessions.procedural-takeover.import"]).toHaveBeenCalledTimes(1);
      expect(runtime.calls["sessions.procedural-takeover.execute"]).toHaveBeenCalledTimes(
        stage === "import" ? 0 : 1,
      );
      if (stage === "check")
        expect(runtime.calls["sessions.procedural-takeover.check"]).toHaveBeenCalledWith({
          operationId: receipt.operationId,
        });
      await runtime.React.act(async () => root!.unmount());
      root = null;
      dom!.window.close();
      dom = null;
    }
  },
);
