import type { ReactElement } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NavigatorCapabilities, SummaryRevision } from "../shared/session-navigator";

// Each test gets a fresh renderer, so one host's sheets and runs never carry into another test.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const host = vi.hoisted(() => ({ rpc: {} as Record<string, ReturnType<typeof vi.fn>>, toast: { show: () => {} } }));
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, useWindowDimensions: () => ({ width: 390, height: 844 }), Pressable: "Pressable", Text: "Text", View: "View", ScrollView: "ScrollView", Image: "Image",
  BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => host.rpc[contract.name] }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", TextInput: "TextInput", ScrollView: "ScrollView", copyText: async () => {}, useToast: () => host.toast,
  Modal: Object.assign(function Modal() { return null; }, { Content: function ModalContent() { return null; } }) }));

import { useSessionOperations } from "../client/session-operations";
import { AttachSheet } from "../client/session-summary";
import { TAKEOVER_EFFECTS, TakeoverResults } from "../client/session-takeover";
import { moveMenuFocus, type SessionAction } from "../client/session-actions-menu";
import type { DirectoryEntry } from "../client/session-directory";
import { sessionEntry } from "./session-fixtures";

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const READING = "Reading what this host supports…", OFFLINE = "This host is offline.";
const ok = { supported: true } as const;
const off = (reason: string) => ({ supported: false as const, reason });
const SIMULATED = "SIMULATED: nothing leaves this page.";
const SUPPORTED: NavigatorCapabilities = { usageTotals: ok, summaries: ok, draftAttach: ok, takeover: { supported: true, limits: [SIMULATED] }, takeoverBulk: ok, takeoverRoute: ok };
const A = sessionEntry("agent-a", { title: "Route review" }), B = sessionEntry("agent-b", { title: "Billing" });
const SUMMARY: SummaryRevision = { summaryId: "00000000-0000-4000-8000-0000000000aa", revision: 3, sourceAgentId: "agent-a", sourceTitle: "Route review", cutoff: "2026-10-25T10:00:00Z",
  savedAt: "2026-10-25T10:05:00Z", sections: { task: "Fictional task", decisions: "", currentState: "", nextStep: "" }, text: "Fictional summary" };

beforeEach(() => {
  harness.fresh();
  host.toast = { show: vi.fn() };
  host.rpc = {
    "sessions.procedural-takeover.capability": vi.fn(async () => ({ supported: true, limits: [] })),
    "sessions.navigator-capabilities": vi.fn(async () => SUPPORTED),
    "sessions.takeover-review": vi.fn(async (input: { agentId: string }) => ({ reviewId: "00000000-0000-4000-8000-0000000000b1", revision: 1, agentId: input.agentId, checkedAt: "2026-10-26T09:00:00Z",
      state: "blocked", message: "No route has fresh evidence.", predecessor: null, ownership: null, association: { bindingKey: null, source: null, choices: [] }, route: null, successor: null, limits: [], digest: null })),
    "sessions.takeover-apply": vi.fn(), "sessions.takeover-check": vi.fn(),
  };
});

type Ops = ReturnType<typeof useSessionOperations>;
let hosts = 0;
/** Mounts the hook on a host of its own and lets the capability read answer; `capabilities` replaces the host's answer. */
async function mount(patch: Partial<Parameters<typeof useSessionOperations>[0]> = {}, capabilities: NavigatorCapabilities | "pending" = SUPPORTED) {
  host.rpc["sessions.navigator-capabilities"] = vi.fn(capabilities === "pending" ? () => new Promise(() => {}) : async () => capabilities);
  const props = { hostId: `host-${++hosts}`, online: true, colors: COLORS, compact: true, entries: [A, B], opened: new Map<string, number>(), newLead: vi.fn(), onTakeoverStart: vi.fn(), ...patch };
  const render = (): Ops => harness.current.render(() => useSessionOperations(props));
  render(); await settle();
  return { props, render };
}
const action = (ops: Ops, entry: DirectoryEntry, key: string) => ops.actions(entry).find((one) => one.key === key)! as SessionAction & { option: NonNullable<SessionAction["option"]> };
const layer = (ops: Ops) => ops.layer as ReactElement<Record<string, unknown>> | null;
const layerName = (ops: Ops) => (layer(ops)?.type as { name?: string } | undefined)?.name ?? null;

describe("useSessionOperations gating", () => {
  it("says it is still reading what the host supports for every capability-gated action", async () => {
    const ops = (await mount({}, "pending")).render();
    expect(action(ops, A, "summarize").unavailable).toBe(READING);
    expect(action(ops, A, "attach").unavailable).toBe(READING);
    expect(action(ops, A, "takeover").option.unavailable).toBe(READING);
    expect(ops.takeover([A, B])).toMatchObject({ unavailable: READING, auto: READING });
  });

  it("refuses every action on an offline host", async () => {
    const ops = (await mount({ online: false })).render();
    expect(["takeover", "summarize", "attach"].map((key) => action(ops, A, key).unavailable)).toEqual([OFFLINE, OFFLINE, OFFLINE]);
    expect(ops.takeover([A]).unavailable).toBe(OFFLINE);
    expect(ops.takeover([A, B]).unavailable).toBe(OFFLINE);
  });

  it("gates taking over several seats on the host's bulk capability, not a single seat", async () => {
    const reason = "Take over of several sessions is not qualified on this host; take over one session at a time.";
    const ops = (await mount({}, { ...SUPPORTED, takeoverBulk: off(reason) })).render();
    expect(ops.takeover([A, B]).unavailable).toBe(reason);
    expect(ops.takeover([A]).unavailable).toBeNull();
    expect(action(ops, A, "takeover").unavailable).toBeNull();
  });

  // R6: one seat's Take over asks the host as several seats do; no answer is not support.
  it("gates taking over one seat on the host's takeover capability, and reads a missing answer as unsupported", async () => {
    const reason = "Take over is not qualified on this host: no owner here creates and verifies the new session, so nothing would continue the task.";
    const ops = (await mount({}, { ...SUPPORTED, takeover: off(reason) })).render();
    expect(action(ops, A, "takeover").unavailable).toBe(reason);
    expect(ops.takeover([A]).unavailable).toBe(reason);
    expect(ops.takeover([A, B]).unavailable).toBe(reason);
    harness.fresh();
    host.rpc["sessions.navigator-capabilities"] = vi.fn(async () => { throw new Error("no such RPC"); });
    const failed = { hostId: `host-${++hosts}`, online: true, colors: COLORS, compact: true, entries: [A, B], opened: new Map<string, number>(), newLead: vi.fn() };
    harness.current.render(() => useSessionOperations(failed)); await settle();
    expect(action(harness.current.render(() => useSessionOperations(failed)), A, "takeover").unavailable).toBe("Kontor could not read what this host supports (no such RPC).");
  });

  // R5: the surface that starts one-tap Take over shows what it does and the host's stated limits, and claims nothing else.
  it("shows what Take over does and the host's limits where it starts, never a blanket same-permissions claim", async () => {
    const ops = (await mount()).render();
    const footer = action(ops, A, "takeover").footer!;
    expect(footer).toBe(`Saves a compact handoff, archives this session, then starts a new one that continues the task. ${SIMULATED}`);
    expect(footer).not.toMatch(/Same (place|personality|permissions)/);
    expect(ops.takeover([A, B]).disclosure).toBe(footer);
  });

  it("confirms at once a review whose limits the surface showed, and holds one with any other", async () => {
    const ready = (limits: string[]) => vi.fn(async (input: { agentId: string }) => ({ reviewId: "00000000-0000-4000-8000-0000000000c1", revision: 1, agentId: input.agentId, checkedAt: "2026-10-26T09:00:00Z",
      state: "ready", message: "Ready.", predecessor: null, ownership: "ordinary", association: { bindingKey: null, source: null, choices: [] }, route: null,
      successor: { requestId: "00000000-0000-4000-8000-0000000000f1", title: "Route review", workspaceId: "wks", cwd: "/fixture", provider: "codex", model: "gpt-6-sol", effort: null, mode: null, persona: null }, limits, digest: "e".repeat(64) }));
    host.rpc["sessions.takeover-apply"] = vi.fn(async () => ({ reviewId: "00000000-0000-4000-8000-0000000000c1", revision: 1, agentId: "agent-a", state: "succeeded", phase: "done", message: "Done.", successor: null, next: "none", checkedAt: "2026-10-26T09:01:00Z" }));
    host.rpc["sessions.takeover-review"] = ready([SIMULATED]);
    const shown = await mount();
    action(shown.render(), A, "takeover").perform(); await settle(); await settle();
    expect(host.rpc["sessions.takeover-apply"]).toHaveBeenCalledTimes(1);
    harness.fresh();
    host.rpc["sessions.takeover-review"] = ready([SIMULATED, "The supervisor is not told."]);
    const extra = await mount();
    action(extra.render(), A, "takeover").perform(); await settle(); await settle();
    expect(host.rpc["sessions.takeover-apply"]).toHaveBeenCalledTimes(1);
    expect((layer(extra.render())!.props.run as { seats: { state: string }[] }).seats[0]!.state).toBe("review");
  });

  it("refuses a single archived seat", async () => {
    const ops = (await mount()).render();
    expect(ops.takeover([sessionEntry("agent-old", { archivedAt: "2026-10-20T09:00:00Z" })]).unavailable).toBe("Archived: there is no seat to take over.");
  });

  it.each(["running", "initializing"] as const)("refuses a single %s seat until it is stopped", async (status) => {
    const ops = (await mount()).render();
    expect(action(ops, sessionEntry("agent-busy", { status }), "takeover").unavailable).toBe("Running. Stop it in the session, then take it over.");
  });

  it("gates the Auto model choice on the host's route capability", async () => {
    const reason = "This host takes over only on the seat's configured chain; choosing a model for one takeover is not supported.";
    const ops = (await mount({}, { ...SUPPORTED, takeoverRoute: off(reason) })).render();
    expect(action(ops, A, "takeover")).toMatchObject({ unavailable: null, option: { unavailable: reason } });
    expect(ops.takeover([A, B])).toMatchObject({ unavailable: null, auto: reason });
  });

  it("gates Summarize on saved summaries and Attach on saved summaries and draft attach", async () => {
    const noStore = "This host has no saved summary store.", noDraft = "This Paseo app gives plugins no way to place a draft.";
    const stored = (await mount({}, { ...SUPPORTED, summaries: off(noStore) })).render();
    expect([action(stored, A, "summarize").unavailable, action(stored, A, "attach").unavailable]).toEqual([noStore, noStore]);
    harness.fresh();
    const drafted = (await mount({}, { ...SUPPORTED, draftAttach: off(noDraft) })).render();
    expect([action(drafted, A, "summarize").unavailable, action(drafted, A, "attach").unavailable]).toEqual([null, noDraft]);
  });
});

describe("useSessionOperations takeover start", () => {
  it("reports a start when Auto runs at once and opens the results", async () => {
    const h = await mount();
    action(h.render(), A, "takeover").perform();
    expect(h.props.onTakeoverStart).toHaveBeenCalledTimes(1);
    expect(layer(h.render())?.type).toBe(TakeoverResults);
    expect(host.rpc["sessions.takeover-review"]).toHaveBeenCalledWith({ agentId: "agent-a", bindingKey: null });
  });

  it("reports no start while the picker opens or is cancelled, and one once a model is used", async () => {
    const h = await mount();
    h.render().takeover([A, B]).choose();
    expect(layerName(h.render())).toBe("TakeoverPicker");
    (layer(h.render())!.props.onCancel as () => void)();
    expect(layer(h.render())).toBeNull();
    expect(h.props.onTakeoverStart).not.toHaveBeenCalled();
    action(h.render(), A, "takeover").option.perform();
    const route = { provider: "codex", model: "gpt-6-sol", effort: "high", featureValues: {} };
    (layer(h.render())!.props.onUse as (picked: typeof route) => void)(route);
    expect(h.props.onTakeoverStart).toHaveBeenCalledTimes(1);
    expect(host.rpc["sessions.takeover-review"]).toHaveBeenCalledWith({ agentId: "agent-a", bindingKey: null, route });
  });

  // R1: one seat's picker shows that seat's own mode on its own provider; several keep theirs each. R5: it also states what the host disclosed.
  it("hands the picker one seat's own permission mode and provider, several seats' count, and the host's limits", async () => {
    host.rpc["sessions.create-options"] = vi.fn(async () => ({ providers: [], catalogProjectId: null }));
    const one: DirectoryEntry = { ...sessionEntry("agent-c", { title: "Claude seat" }), provider: "claude-b", mode: "bypassPermissions" };
    const tree = (node: unknown): ReactElement<Record<string, unknown>>[] => !node || typeof node !== "object" || !("props" in node) ? [] : [node as ReactElement<Record<string, unknown>>, ...[(node as ReactElement<{ children?: unknown }>).props.children].flat().flatMap(tree)];
    /** The picker the sheet draws once the host's models are read, in a renderer of its own. */
    const permissions = async (seats: DirectoryEntry[]) => {
      const mounted = await mount();
      mounted.render().takeover(seats).choose();
      const sheet = layer(mounted.render())!;
      harness.fresh();
      const draw = () => harness.current.render(() => (sheet.type as (props: object) => ReactElement)(sheet.props));
      draw(); await settle();
      const { props } = tree(draw()).find((node) => (node.type as { name?: string }).name === "ModelPicker")!;
      expect(props.note).toBe(`Only this takeover · Defaults stay unchanged. ${TAKEOVER_EFFECTS} ${SIMULATED}`);
      return props.permissions;
    };
    expect(await permissions([one])).toEqual({ inherited: "bypassPermissions", provider: "claude-b" });
    expect(await permissions([A, B])).toEqual({ seats: 2 });
  });

  it("starts nothing and reports no start for a refused takeover", async () => {
    const h = await mount({ online: false });
    h.render().takeover([A, B]).start();
    h.render().takeover([A, B]).choose();
    expect(layer(h.render())).toBeNull();
    expect(h.props.onTakeoverStart).not.toHaveBeenCalled();
    expect(host.rpc["sessions.takeover-review"]).not.toHaveBeenCalled();
  });
});

// R2: after Close, an unfinished seat's menu and the surface's notice lead back to Check or Finish.
describe("useSessionOperations unfinished takeover", () => {
  const REVIEW = "00000000-0000-4000-8000-0000000000d1";
  const archived = sessionEntry("agent-a", { title: "Route review", archivedAt: "2026-10-26T09:00:30Z" });
  function host2(outcome: { state: string; phase: string; next: string }) {
    host.rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => ({ reviewId: REVIEW, revision: 1, agentId: input.agentId, checkedAt: "2026-10-26T09:00:00Z",
      state: "ready", message: "Ready.", predecessor: null, ownership: "ordinary", association: { bindingKey: null, source: null, choices: [] }, route: null,
      successor: { requestId: "00000000-0000-4000-8000-0000000000f1", title: "Route review", workspaceId: "wks", cwd: "/fixture", provider: "codex", model: "gpt-6-sol", effort: null, mode: null, persona: null }, limits: [SIMULATED], digest: "e".repeat(64) }));
    host.rpc["sessions.takeover-apply"] = vi.fn(async () => ({ reviewId: REVIEW, revision: 1, agentId: "agent-a", message: "Fixture.", successor: null, checkedAt: "2026-10-26T09:01:00Z", ...outcome }));
    host.rpc["sessions.takeover-check"] = vi.fn(async () => ({ reviewId: REVIEW, revision: 1, agentId: "agent-a", state: "succeeded", phase: "done", message: "Checked.", successor: null, next: "none", checkedAt: "2026-10-26T09:02:00Z" }));
  }
  const close = (ops: Ops) => (layer(ops)!.props.onClose as () => void)();

  it("offers Finish takeover on the archived predecessor after Close, and a notice that reopens the results", async () => {
    host2({ state: "partial", phase: "create", next: "review" });
    const h = await mount();
    action(h.render(), A, "takeover").perform(); await settle(); await settle();
    close(h.render());
    expect(layer(h.render())).toBeNull();
    const finish = action(h.render(), archived, "takeover");
    expect(finish).toMatchObject({ label: "Finish takeover", unavailable: null });
    expect(finish.option).toBeUndefined();
    const notice = h.render().notice as ReactElement<Record<string, unknown>>;
    expect(notice.props).toMatchObject({ testID: "takeover-unfinished", accessibilityLabel: "1 recovery pending. Open takeover results" });
    (notice.props.onPress as () => void)();
    expect(layer(h.render())?.type).toBe(TakeoverResults);
    close(h.render());
    finish.perform(); await settle(); await settle();
    expect(host.rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(2);
    expect(host.rpc["sessions.takeover-apply"]).toHaveBeenCalledTimes(2);
  });

  it("offers Check takeover for an unconfirmed effect after Close, reading it by its earlier review with nothing resent", async () => {
    host2({ state: "uncertain", phase: "create", next: "check" });
    const h = await mount();
    action(h.render(), A, "takeover").perform(); await settle(); await settle();
    close(h.render());
    const check = action(h.render(), A, "takeover");
    expect(check.label).toBe("Check takeover");
    check.perform(); await settle(); await settle();
    expect(host.rpc["sessions.takeover-check"]).toHaveBeenCalledExactlyOnceWith({ reviewId: REVIEW });
    expect(host.rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(1);
    expect(host.rpc["sessions.takeover-apply"]).toHaveBeenCalledTimes(1);
    close(h.render());
    expect(action(h.render(), A, "takeover").label).toBe("Take over");
    expect(h.render().notice).toBeNull();
  });

  // R2b: while a seat is unresolved, no new Take over starts on this host; its own Check, Finish or review stays.
  const BUSY = "Finish or check the unfinished takeover first.";
  it("refuses another seat's Take over and the selection bar's while one is unfinished, keeping its Finish", async () => {
    host2({ state: "partial", phase: "create", next: "review" });
    const h = await mount();
    action(h.render(), A, "takeover").perform(); await settle(); await settle();
    close(h.render());
    expect([action(h.render(), B, "takeover").unavailable, h.render().takeover([B]).unavailable, h.render().takeover([A, B]).unavailable]).toEqual([BUSY, BUSY, BUSY]);
    h.render().takeover([B]).start();
    h.render().takeover([A, B]).choose();
    expect(layer(h.render())).toBeNull();
    expect(host.rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(1);
    expect(action(h.render(), archived, "takeover")).toMatchObject({ label: "Finish takeover", unavailable: null });
  });

  it("refuses a start from a picker opened before a seat became unfinished, saying why and reopening the results", async () => {
    host2({ state: "partial", phase: "create", next: "review" });
    const h = await mount();
    action(h.render(), B, "takeover").option.perform();
    const onUse = layer(h.render())!.props.onUse as (route: object) => void;
    action(h.render(), A, "takeover").perform(); await settle(); await settle();
    close(h.render());
    vi.mocked(h.props.onTakeoverStart).mockClear();
    onUse({ provider: "codex", model: "gpt-6-sol", effort: "high", featureValues: {} }); await settle();
    expect(host.toast.show).toHaveBeenCalledWith(BUSY);
    expect(h.props.onTakeoverStart).not.toHaveBeenCalled();
    expect(host.rpc["sessions.takeover-review"]).toHaveBeenCalledTimes(1);
    expect(layer(h.render())?.type).toBe(TakeoverResults);
  });

  it("leads a held review's seat back to its results to read the limits, never confirming from the menu", async () => {
    host2({ state: "succeeded", phase: "done", next: "none" });
    const ready = host.rpc["sessions.takeover-review"]!.getMockImplementation() as (input: { agentId: string }) => Promise<object>;
    host.rpc["sessions.takeover-review"] = vi.fn(async (input: { agentId: string }) => ({ ...await ready(input), limits: [SIMULATED, "The supervisor is not told."] }));
    const h = await mount();
    action(h.render(), A, "takeover").perform(); await settle(); await settle();
    close(h.render());
    const held = action(h.render(), A, "takeover");
    expect(held).toMatchObject({ label: "Review takeover", unavailable: null });
    expect(action(h.render(), B, "takeover").unavailable).toBe(BUSY);
    expect((h.render().notice as ReactElement<Record<string, unknown>>).props.accessibilityLabel).toBe("1 review needed. Open takeover results");
    held.perform(); await settle();
    expect(layer(h.render())?.type).toBe(TakeoverResults);
    expect(host.rpc["sessions.takeover-apply"]).not.toHaveBeenCalled();
    (layer(h.render())!.props.onDecline as (id: string) => void)("agent-a");
    close(h.render());
    expect(action(h.render(), A, "takeover").label).toBe("Take over");
    expect(h.render().notice).toBeNull();
  });
});

describe("useSessionOperations attach", () => {
  it("passes the surface's reason for no New lead through to Attach", async () => {
    const reason = "New lead opens only from Kontor's Sessions.";
    const h = await mount({ newLead: reason });
    action(h.render(), A, "attach").perform();
    expect(layer(h.render())).toMatchObject({ type: AttachSheet, props: { newLead: reason, revision: null } });
  });

  it("closes Attach and opens New lead with its source and revision", async () => {
    const h = await mount();
    action(h.render(), A, "attach").perform();
    (layer(h.render())!.props.newLead as (revision: SummaryRevision) => void)(SUMMARY);
    expect(h.props.newLead).toHaveBeenCalledExactlyOnceWith(A, SUMMARY);
    expect(layer(h.render())).toBeNull();
  });
});

describe("the actions menu's keys", () => {
  it("moves focus down and up through its items, wrapping, to either end on Home and End, and leaves other keys alone", () => {
    const items = ["takeover", "favorite", "archive"].map((key) => ({ key, focus: vi.fn() }));
    const menu = { ownerDocument: { activeElement: items[0] as unknown }, querySelectorAll: () => items };
    const press = (key: string) => { const preventDefault = vi.fn(); moveMenuFocus({ key, preventDefault, currentTarget: menu }); return preventDefault.mock.calls.length; };
    const focused = () => items.findIndex((item) => item.focus.mock.calls.length);
    press("ArrowDown"); expect(focused()).toBe(1);
    items[1]!.focus.mockClear(); press("ArrowUp"); expect(focused()).toBe(2);
    items[2]!.focus.mockClear(); press("End"); expect(focused()).toBe(2);
    items[2]!.focus.mockClear(); menu.ownerDocument.activeElement = items[2]; press("Home"); expect(focused()).toBe(0);
    expect(press("Tab")).toBe(0);
    // From outside the items, such as the sheet itself, Down reaches the first and Up the last.
    items[0]!.focus.mockClear(); menu.ownerDocument.activeElement = null; press("ArrowUp"); expect(focused()).toBe(2);
    items[2]!.focus.mockClear(); press("ArrowDown"); expect(focused()).toBe(0);
  });
});

describe("prepared takeover action", () => {
  it("offers a separate import while legacy takeover remains refused", async () => {
    const { UNSUPPORTED } = await import("../shared/session-navigator");
    const m = await mount({}, UNSUPPORTED), ops = m.render();
    expect(action(ops, A, "takeover").unavailable).toBeTruthy();
    expect(action(ops, A, "prepared-takeover").unavailable).toBeNull();
    action(ops, A, "prepared-takeover").perform();
    expect(layerName(m.render())).toBe("PreparedTakeoverSheet");
    expect(host.rpc["sessions.takeover-review"]).not.toHaveBeenCalled();
  });
  it("keeps prepared import unavailable when its own capability read fails", async () => {
    host.rpc["sessions.procedural-takeover.capability"] = vi.fn(async () => { throw new Error("unavailable"); });
    const m = await mount();
    expect(action(m.render(), A, "prepared-takeover").unavailable).toContain("unavailable");
  });
});

describe("prepared import client recovery", () => {
  it("validates SHA without RPC and confirms only once after lost reply", async () => {
    const { preparedState } = await import("../client/prepared-takeover-state");
    const receipt = { sourceId: A.id, successorId: B.id, operationId: "operation", admissionHash: "a".repeat(64), phase: "prepared" } as import("../shared/procedural-takeover").ProceduralReceipt;
    const calls = { import: vi.fn(async () => ({ receipt, limits: ["Owner reviewed"], canExecute: true })), execute: vi.fn(async () => { throw new Error("lost reply"); }), check: vi.fn(async () => ({ ...receipt, phase: "held" as const })) };
    const state = preparedState(A.id);
    await state.import(calls, { path: "/admission", sha256: "bad" }); expect(calls.import).not.toHaveBeenCalled();
    await state.import(calls, { path: "/admission", sha256: "a".repeat(64) }); expect(state.limits).toEqual(["Owner reviewed"]);
    await state.act(calls, true); await state.act(calls, true);
    expect(calls.execute).toHaveBeenCalledTimes(1); expect(state.problem).toContain("Unconfirmed");
    await state.act(calls, false); expect(calls.check).toHaveBeenCalledWith({ operationId: "operation" }); expect(state.receipt?.phase).toBe("held");
  });
  it("never confirms a recovered execution or wrong row", async () => {
    const { preparedState } = await import("../client/prepared-takeover-state");
    const receipt = { sourceId: A.id, successorId: B.id, operationId: "operation", admissionHash: "a".repeat(64), phase: "prepared" } as import("../shared/procedural-takeover").ProceduralReceipt;
    const calls = { import: vi.fn(async () => ({ receipt, limits: [], canExecute: false })), execute: vi.fn(async () => receipt), check: vi.fn(async () => receipt) };
    const state = preparedState(A.id); await state.import(calls, { path: "/admission", sha256: "a".repeat(64) }); await state.act(calls, true); expect(calls.execute).not.toHaveBeenCalled();
    const other = preparedState(B.id); await other.import(calls, { path: "/admission", sha256: "a".repeat(64) }); expect(other.receipt).toBeNull(); expect(other.problem).toContain("another session");
  });
});

describe("prepared takeover review sheet", () => {
  it("reviews exact identities/config/limits then checks a lost dispatch after reopening", async () => {
    const { PreparedTakeoverSheet } = await import("../client/prepared-takeover");
    const receipt = { sourceId: A.id, successorId: B.id, operationId: "stable-operation", admissionHash: "a".repeat(64), revision: 0, phase: "prepared", message: "Prepared only", admission: { mode: { workflow: "free", orchestration: "paseo" }, config: { provider: "codex", model: "fixture-model", effort: "low", mode: "full-access" }, cwd: "/fixture", checkout: { head: "1".repeat(40) } } } as import("../shared/procedural-takeover").ProceduralReceipt;
    host.rpc["sessions.procedural-takeover.import"] = vi.fn(async () => ({ receipt, canExecute: true, limits: ["No cross-client fence"] }));
    host.rpc["sessions.procedural-takeover.execute"] = vi.fn(async () => { throw new Error("lost reply"); });
    host.rpc["sessions.procedural-takeover.check"] = vi.fn(async () => ({ ...receipt, phase: "held" }));
    const props = { hostId: `review-${++hosts}`, source: A, colors: COLORS, onClose: vi.fn() };
    const draw = () => harness.current.render(() => PreparedTakeoverSheet(props)) as ReactElement<Record<string, unknown>>;
    const tree = (node: unknown): ReactElement<Record<string, unknown>>[] => !node || typeof node !== "object" || !("props" in node) ? [] : [node as ReactElement<Record<string, unknown>>, ...[(node as ReactElement<{ children?: unknown }>).props.children].flat().flatMap(tree)];
    const footer = () => (draw().props.footer as ReactElement<{ primary: { disabled: boolean; label: string; onPress: () => void } }>).props.primary;
    expect(footer().disabled).toBe(true);
    (tree(draw()).find(n => n.props.accessibilityLabel === "Admission host path")!.props.onChangeText as (s: string) => void)("/admission");
    (tree(draw()).find(n => n.props.accessibilityLabel === "Admission SHA-256")!.props.onChangeText as (s: string) => void)("a".repeat(64));
    footer().onPress(); await settle();
    const content = JSON.stringify(draw().props.children);
    for (const expected of [A.id, B.id, "stable-operation", "free / paseo", "codex / fixture-model / low / full-access", "No cross-client fence"]) expect(content).toContain(expected);
    expect(footer().label).toBe("Confirm procedural takeover");
    footer().onPress(); await settle(); expect(footer().label).toBe("Check same operation");
    harness.fresh(); expect(footer().label).toBe("Check same operation"); footer().onPress(); await settle();
    expect(host.rpc["sessions.procedural-takeover.execute"]).toHaveBeenCalledTimes(1);
    expect(host.rpc["sessions.procedural-takeover.check"]).toHaveBeenCalledWith({ operationId: "stable-operation" });
  });
});
