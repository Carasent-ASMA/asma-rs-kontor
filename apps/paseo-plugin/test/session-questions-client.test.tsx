import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = await vi.hoisted(async () => {
  const { deferredReact } = await import("./hermes-scoping");
  const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } };
  return box;
});
vi.mock("react", async (original) => ({
  ...(await original<typeof import("react")>()),
  ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])),
}));
vi.mock("react-native", () => ({
  Platform: { OS: "web" },
  Pressable: "Pressable",
  Text: "Text",
  View: "View",
  ScrollView: "ScrollView",
  ActivityIndicator: () => null,
  Settings: {},
}));
vi.mock("@getpaseo/plugin/client/react-native", () => ({
  Modal: Object.assign(function Modal() { return null; }, { Content: "Modal.Content" }),
  ScrollView: "ScrollView",
  Icon: "Icon",
}));
vi.mock("../client/control-help", () => ({
  useActionHelp: () => ({ bindings: {}, dismiss() {} }),
  useControlHelp: () => ({ bindings: {}, hold() {}, dismiss() {} }),
}));

const rpc = vi.hoisted(() => ({ read: vi.fn(), accept: vi.fn() }));
vi.mock("@getpaseo/plugin/client", () => ({
  useRpc: (contract: { name: string }) => contract.name === "sessions.pending-questions" ? rpc.read : rpc.accept,
}));

import { PendingQuestionsButton, SessionQuestionsSheet, pendingPermissionTotal, useSessionQuestions, QUESTION_OUTCOME_LABELS } from "../client/session-questions";
import { SheetFooter } from "../client/operation-sheet";
import type { PendingQuestions, SessionQuestionItem, SessionQuestionResult } from "../shared/session-questions";

type Element = { type?: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk)
  : [node as Element, ...walk((node as Element).props.children), ...walk((node as Element).props.footer), ...walk((node as Element).props.header)];
const draw = (element: Element) => (element.type as (props: Record<string, unknown>) => Element)(element.props);
const texts = (tree: unknown) => walk(tree).filter((element) => element.type === "Text").map((element) => [element.props.children].flat().join(""));
const button = (count: number) => harness.current.render(() => PendingQuestionsButton({ count, colors, compact: false, onPress: vi.fn() })) as Element | null;

const colors = {
  accent: "#20744A", accentForeground: "#ffffff", border: "#cccccc", foreground: "#111111", foregroundMuted: "#666666",
  surface0: "#ffffff", surface1: "#f4f4f4", surface2: "#eeeeee", statusDanger: "#cc0000", statusSuccess: "#008800", statusWarning: "#cc8800",
} as unknown as PluginTheme["colors"];

const item = (patch: Partial<SessionQuestionItem> = {}): SessionQuestionItem => ({
  agentId: "agent-a", agentTitle: "Fix the tests", agentStatus: "running", requestId: "request-1", kind: "tool",
  title: "Run command", label: "Allow once · bash", response: { behavior: "allow", selectedActionId: "allow-once" },
  hash: "a".repeat(64), supported: true, reason: null, ...patch,
});
const unsupportedItem = (patch: Partial<SessionQuestionItem> = {}): SessionQuestionItem => item({
  requestId: "request-2", agentTitle: "Ask the operator", kind: "question", title: "Choose a release", label: "Needs the operator",
  response: null, hash: "b".repeat(64), supported: false, reason: "Release: a written answer is required.", ...patch,
});
const result = (patch: Partial<SessionQuestionResult> = {}): SessionQuestionResult => ({
  agentId: "agent-a", requestId: "request-1", outcome: "answered", message: null, ...patch,
});
const pending = (...items: SessionQuestionItem[]): PendingQuestions => ({ items });
const questions = (patch: Partial<ReturnType<typeof useSessionQuestions>> = {}) => ({
  open: true, review: pending(item()), results: {}, busy: false, running: false, checking: null, error: null,
  begin: vi.fn(), confirm: vi.fn(), check: vi.fn(), close: vi.fn(), ...patch,
}) as ReturnType<typeof useSessionQuestions>;

beforeEach(() => {
  harness.fresh();
  rpc.read.mockReset();
  rpc.accept.mockReset();
});

describe("pending questions button", () => {
  it("is absent at zero and counts the pending requests when present", () => {
    expect(button(0)).toBeNull();
    expect(button(3)).toMatchObject({
      props: { label: "Accept 3", accessibilityLabel: "Accept 3 pending permission requests" },
    });
    expect(button(1)).toMatchObject({ props: { label: "Accept 1", accessibilityLabel: "Accept 1 pending permission request" } });
  });

  it("sums the directory's native count over live rows only", () => {
    expect(pendingPermissionTotal([])).toBe(0);
    expect(pendingPermissionTotal([
      { archivedAt: null, pendingPermissionCount: 2 },
      { archivedAt: null, pendingPermissionCount: 1 },
      { archivedAt: "2026-01-01", pendingPermissionCount: 9 },
    ])).toBe(3);
  });
});

describe("pending questions review sheet", () => {
  it("lists every request with its session, kind, title and exact label, and marks unsupported ones", () => {
    const tree = draw(SessionQuestionsSheet({ questions: questions({ review: pending(item(), unsupportedItem()) }), colors }) as Element);
    const body = texts(tree);
    expect(body).toEqual(expect.arrayContaining([
      "Fix the tests", "tool · Run command", "Allow once · bash",
      "Ask the operator", "question · Choose a release", "Needs the operator", "Release: a written answer is required.",
    ]));
    expect(body).toContain("1 request will be answered · 1 request needs the operator.");
    expect(tree.props.title).toBe("Review 2 pending permission requests");
  });

  it("cancels without a confirmation and confirms the supported batch once from the footer", () => {
    const confirm = vi.fn();
    const close = vi.fn();
    const tree = draw(SessionQuestionsSheet({ questions: questions({ review: pending(item(), unsupportedItem()), confirm, close }), colors }) as Element);
    const footer = walk(tree).find((element) => element.type === SheetFooter)!;
    const cancel = footer.props.secondary as { label: string; onPress: () => void };
    const accept = footer.props.primary as { label: string; accessibilityLabel: string; disabled: boolean; onPress: () => void };
    expect(accept).toMatchObject({ label: "Accept 1", accessibilityLabel: "Accept 1 pending request", disabled: false });
    cancel.onPress();
    expect(close).toHaveBeenCalledTimes(1);
    expect(confirm).not.toHaveBeenCalled();
    accept.onPress();
    expect(confirm).toHaveBeenCalledTimes(1);
  });

  it("renders each result outcome and offers Check only for an uncertain request", () => {
    const outcomes: SessionQuestionResult[] = [
      result({ requestId: "answered", outcome: "answered" }),
      result({ requestId: "already", outcome: "already-answered", message: "This request is no longer pending." }),
      result({ requestId: "changed", outcome: "changed", message: "The request changed since the review." }),
      result({ requestId: "unsupported", outcome: "unsupported", message: "The operator must answer." }),
      result({ requestId: "uncertain", outcome: "uncertain", message: "Still listed; it is not resent." }),
    ];
    const review = pending(
      item({ requestId: "answered" }), item({ requestId: "already" }), item({ requestId: "changed" }),
      item({ requestId: "unsupported" }), item({ requestId: "uncertain" }), unsupportedItem(),
    );
    const results = Object.fromEntries(outcomes.map((entry) => [`${entry.agentId}\u0000${entry.requestId}`, entry]));
    const tree = draw(SessionQuestionsSheet({ questions: questions({ review, results }), colors }) as Element);
    const body = texts(tree);
    for (const label of Object.values(QUESTION_OUTCOME_LABELS)) {
      expect(body.some((text) => text.includes(label))).toBe(true);
    }
    expect(body).toContain("1 answered · 1 already answered · 1 changed · 1 unsupported · 1 not confirmed · 1 request needs the operator");
    const check = walk(tree).filter((element) => element.props.label === "Check" && typeof element.props.onPress === "function");
    expect(check).toHaveLength(1);
    expect(check[0]!.props.accessibilityLabel).toBe("Check Run command without resending");
  });
});

describe("pending questions hook", () => {
  it("confirms one batch once and never includes an unsupported request", async () => {
    rpc.read.mockResolvedValue({ items: [item(), unsupportedItem()] });
    let release!: (value: unknown) => void;
    rpc.accept.mockImplementation(() => new Promise((resolve) => { release = resolve; }));
    const changed = vi.fn();
    let state = harness.current.render(() => useSessionQuestions("host-a", changed));

    await state.begin();
    state = harness.current.render(() => useSessionQuestions("host-a", changed));
    expect(state.review?.items).toHaveLength(2);

    const first = state.confirm();
    await Promise.resolve();
    const second = state.confirm();
    expect(await second).toBeNull();
    expect(rpc.accept).toHaveBeenCalledTimes(1);
    expect(rpc.accept).toHaveBeenCalledWith({ items: [{ agentId: "agent-a", requestId: "request-1", hash: "a".repeat(64) }] });

    release({ results: [result()] });
    await first;
    state = harness.current.render(() => useSessionQuestions("host-a", changed));
    expect(state.results["agent-a\u0000request-1"]).toMatchObject({ outcome: "answered" });
    expect(changed).toHaveBeenCalledTimes(1);
  });

  it("does not resend a batch whose result was not reported", async () => {
    rpc.read.mockResolvedValue({ items: [item()] });
    rpc.accept.mockRejectedValueOnce(new Error("transport closed"));
    const changed = vi.fn();
    let state = harness.current.render(() => useSessionQuestions("host-a", changed));
    await state.begin();
    state = harness.current.render(() => useSessionQuestions("host-a", changed));

    expect(await state.confirm()).toBeNull();
    state = harness.current.render(() => useSessionQuestions("host-a", changed));
    expect(state.error).toContain("did not report results");
    expect(await state.confirm()).toBeNull();
    expect(rpc.accept).toHaveBeenCalledTimes(1);
    expect(changed).toHaveBeenCalledTimes(1);
  });

  it("checks an uncertain request by reading only, never by sending again", async () => {    rpc.read.mockResolvedValue({ items: [] });
    const changed = vi.fn();
    let state = harness.current.render(() => useSessionQuestions("host-a", changed));
    state = harness.current.render(() => useSessionQuestions("host-a", changed));

    await state.check(item());
    state = harness.current.render(() => useSessionQuestions("host-a", changed));
    expect(rpc.accept).not.toHaveBeenCalled();
    expect(state.results["agent-a\u0000request-1"]).toMatchObject({ outcome: "answered", message: "No longer pending." });
    expect(changed).toHaveBeenCalledTimes(1);

    rpc.read.mockResolvedValue({ items: [item()] });
    await state.check(item());
    state = harness.current.render(() => useSessionQuestions("host-a", changed));
    expect(rpc.accept).not.toHaveBeenCalled();
    expect(state.results["agent-a\u0000request-1"]).toMatchObject({ outcome: "uncertain", message: "Still listed; it is not resent." });
  });
});
