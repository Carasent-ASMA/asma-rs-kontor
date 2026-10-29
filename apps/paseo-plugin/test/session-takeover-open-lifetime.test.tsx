import type { ReactNode } from "react";
import { expect, it, vi } from "vitest";
import type { TakeoverOutcome } from "../shared/session-lifecycle";
const runtime = await vi.hoisted(async () => {
  const { createRequire } = await import("node:module"),
    { resolve } = await import("node:path");
  const renderer = createRequire(
    resolve(process.env.RENDERER_PACKAGE_JSON ?? "../kontor-prototype/package.json"),
  );
  const tools = createRequire(resolve(process.env.JSDOM_PACKAGE_JSON ?? "../console/package.json"));
  const React = renderer("react");
  if (React.version !== "19.1.0" || renderer("react-dom/package.json").version !== "19.1.0")
    throw new Error("Use the pinned React19.1 renderer");
  return {
    React,
    createRoot: renderer("react-dom/client").createRoot,
    JSDOM: tools("jsdom").JSDOM,
    calls: {} as Record<string, ReturnType<typeof vi.fn>>,
  };
});
vi.mock("react", () => ({ ...runtime.React, default: runtime.React }));
vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: (rpc: { name: string }) => runtime.calls[rpc.name],
}));
vi.mock("react-native", () => ({ Text: "Text", View: "View", Pressable: "Pressable" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({
  Icon: "Icon",
  ScrollView: "ScrollView",
  Modal: Object.assign(() => null, { Content: () => null }),
}));
import React from "react";
import { useTakeoverRun } from "../client/session-takeover";
it("suppresses native open after unmount, then reopens only the verified replacement without resending", async () => {
  const dom = new runtime.JSDOM("<!doctype html><div id='root'></div>");
  vi.stubGlobal("window", dom.window);
  vi.stubGlobal("document", dom.window.document);
  vi.stubGlobal("IS_REACT_ACT_ENVIRONMENT", true);
  const open = vi.fn(() => ({ ok: true as const }));
  let finish!: (value: TakeoverOutcome) => void;
  const delayed = new Promise<TakeoverOutcome>((yes) => {
    finish = yes;
  });
  runtime.calls = {
    "sessions.takeover-review": vi.fn(async () => ({
      reviewId: "review",
      agentId: "source",
      revision: 1,
      state: "ready",
      digest: "a".repeat(64),
      limits: [],
      successor: { model: "fixture" },
    })),
    "sessions.takeover-apply": vi.fn(() => delayed),
    "sessions.takeover-check": vi.fn(),
  };
  let current: ReturnType<typeof useTakeoverRun> | null = null;
  function Probe() {
    current = useTakeoverRun("unmounted-open-host", [], undefined, open);
    return null;
  }
  let root: { render: (node: ReactNode) => void; unmount: () => void } | null = runtime.createRoot(
    dom.window.document.getElementById("root"),
  );
  try {
    await runtime.React.act(async () => root!.render(<Probe />));
    let pending!: Promise<string | null>;
    await runtime.React.act(async () => {
      pending = current!.start([{ id: "source", title: "Original title" }], null);
      await Promise.resolve();
    });
    await runtime.React.act(async () => root!.unmount());
    root = null;
    const outcome: TakeoverOutcome = {
      reviewId: "review",
      revision: 1,
      agentId: "source",
      state: "succeeded",
      phase: "done",
      successor: { requestId: "request", agentId: "exact-successor", title: "Original title" },
      next: "none",
      message: "Task continues",
      checkedAt: "2026-10-29",
    };
    await runtime.React.act(async () => {
      finish(outcome);
      await pending;
    });
    expect(open).not.toHaveBeenCalled();
    root = runtime.createRoot(dom.window.document.getElementById("root"));
    await runtime.React.act(async () => root!.render(<Probe />));
    await runtime.React.act(async () => current!.act("source"));
    expect(open).toHaveBeenCalledExactlyOnceWith(outcome);
    expect(runtime.calls["sessions.takeover-apply"]).toHaveBeenCalledTimes(1);
    expect(runtime.calls["sessions.takeover-review"]).toHaveBeenCalledTimes(1);
    expect(runtime.calls["sessions.takeover-check"]).not.toHaveBeenCalled();
  } finally {
    if (root) await runtime.React.act(async () => root!.unmount());
    dom.window.close();
    vi.unstubAllGlobals();
  }
});
