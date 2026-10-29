import { isValidElement } from "react";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { SummaryRevision } from "../shared/session-navigator";

// Each test gets a fresh renderer, so a saved summary never carries into another test.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const host = vi.hoisted(() => ({ nativeAvailable: true, rpc: {} as Record<string, ReturnType<typeof vi.fn>>, copyText: null as unknown as Mock<(text: string) => Promise<void>> }));
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, useWindowDimensions: () => ({ width: 390, height: 844 }), Pressable: "Pressable", Text: "Text", View: "View", ScrollView: "ScrollView", Image: "Image",
  BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => host.rpc[contract.name] }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", TextInput: "TextInput", ScrollView: "ScrollView", copyText: (text: string) => host.copyText(text),
  Modal: Object.assign(function Modal() { return null; }, { Content: function ModalContent() { return null; } }) }));

vi.mock("../client/summary-attachments", async (original) => ({ ...(await original<typeof import("../client/summary-attachments")>()), useSummaryAttachmentAvailability: () => host.nativeAvailable }));

import { SummaryAttachmentHelp } from "../client/summary-attachments";
import { AttachSheet, SummarySheet } from "../client/session-summary";
import { Choices } from "../client/model-picker";
import { DenseSessionRow } from "../client/dense-session-row";
import { sessionEntry } from "./session-fixtures";

type Element = { type: unknown; props: Record<string, unknown> };
/** The sheet and its footer are hook-free, so they are drawn in place and their buttons are visible. */
const DRAWN = new Set(["OperationSheet", "SheetFooter", "FooterButton"]);
const walk = (node: unknown): Element[] => Array.isArray(node) ? node.flatMap(walk) : !isValidElement<Record<string, unknown>>(node) ? []
  : typeof node.type === "function" && DRAWN.has(node.type.name) ? [node, ...walk((node.type as (props: object) => unknown)(node.props))] : [node, ...walk(node.props.children)];
const text = (tree: Element[]) => tree.filter((node) => node.type === "Text").map((node) => [node.props.children].flat(Infinity).join("")).join("\n");
/** A footer button: the drawn Pressable, not the FooterButton element that carries the same testID. */
const button = (tree: Element[], id: string) => tree.find((node) => node.type === "Pressable" && node.props.testID === id);
const footer = (tree: Element[]) => walk(tree.find((node) => node.props.testID === "operation-sheet-footer")).filter((node) => node.type === "Pressable").map((node) => node.props.accessibilityLabel);
const banner = (tree: Element[]) => text(walk(tree.find((node) => node.props.testID === "summary-banner")));
const press = (node: Element | undefined) => (node!.props.onPress as () => void)();
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const SOURCE = sessionEntry("agent-src", { title: "Route review" });
const SUMMARY: SummaryRevision = { summaryId: "00000000-0000-4000-8000-0000000000aa", revision: 3, sourceAgentId: "agent-src", sourceTitle: "Route review", cutoff: "2026-10-25T10:00:00Z",
  savedAt: "2026-10-25T10:05:00Z", sections: { task: "Fictional task", decisions: "Fictional decision", currentState: "Fictional state", nextStep: "Fictional next step" }, text: "Fictional summary" };

beforeEach(() => {
  harness.fresh();
  host.nativeAvailable = true;
  host.rpc = { "sessions.summary-save": vi.fn(async () => SUMMARY), "sessions.summary-read": vi.fn(async () => SUMMARY), "sessions.summary-latest": vi.fn(async () => ({ [SOURCE.id]: SUMMARY })), "sessions.summary-attach": vi.fn() };
  host.copyText = vi.fn(async () => {});
});

describe("SummarySheet", () => {
  const sheet = (attachUnavailable: string | null = null) => {
    const onAttach = vi.fn(), onClose = vi.fn();
    const render = () => walk(harness.current.render(() => SummarySheet({ source: SOURCE, colors: COLORS, attachUnavailable, onAttach, onClose })));
    return { onAttach, onClose, render };
  };

  it("saves once and says Copied only after the clipboard took the summary", async () => {
    let release!: () => void;
    host.copyText = vi.fn(() => new Promise<void>((resolve) => { release = resolve; }));
    const s = sheet();
    expect(banner(s.render())).toBe("Saving a summary of Route review…");
    await settle();
    expect(banner(s.render())).toBe("Saved r3 · Copying…");
    await settle();
    expect(host.copyText).toHaveBeenCalledExactlyOnceWith("Fictional summary");
    await settle();
    expect(banner(s.render())).toBe("Saved r3 · Copying…");
    release(); await settle();
    const tree = s.render();
    expect(banner(tree)).toBe("Copied to clipboard · Saved r3 · Source linked");
    expect(text(tree)).toContain("A saved summary is ready to paste.");
    expect(host.rpc["sessions.summary-save"]).toHaveBeenCalledExactlyOnceWith({ agentId: "agent-src" });
  });

  it("keeps the saved summary on screen with Copy again when the clipboard refuses it", async () => {
    host.copyText = vi.fn(async () => { throw new Error("permission denied"); });
    const s = sheet();
    s.render(); await settle(); s.render(); await settle();
    const tree = s.render();
    expect(banner(tree)).toBe("Saved r3 · Not copied: permission denied. Use Copy again, or select the text below.");
    expect(text(tree)).toContain("Fictional task");
    expect(text(tree)).not.toContain("Copied to clipboard");
    host.copyText = vi.fn(async () => {});
    press(button(tree, "summary-copy-again")); await settle();
    expect(host.copyText).toHaveBeenCalledExactlyOnceWith("Fictional summary");
    expect(banner(s.render())).toBe("Copied to clipboard · Saved r3 · Source linked");
  });

  it("says a failed save copied nothing and offers Try again", async () => {
    host.rpc["sessions.summary-save"] = vi.fn(async () => { throw new Error("store offline"); });
    const s = sheet();
    s.render(); await settle();
    const tree = s.render();
    expect(banner(tree)).toBe("The summary was not saved (store offline). Nothing was copied.");
    expect(host.copyText).not.toHaveBeenCalled();
    expect(footer(tree)).toEqual(["Close", "Try again"]);
    host.rpc["sessions.summary-save"] = vi.fn(async () => SUMMARY);
    press(tree.find((node) => node.type === "Pressable" && node.props.accessibilityLabel === "Try again"));
    s.render(); await settle(); s.render(); await settle();
    expect(banner(s.render())).toBe("Copied to clipboard · Saved r3 · Source linked");
  });

  it("puts Copy again left and destination instructions right", async () => {
    const s = sheet();
    s.render(); await settle(); s.render(); await settle();
    const tree = s.render();
    expect(footer(tree)).toEqual(["Copy again", "Attach summary…"]);
    press(button(tree, "summary-attach"));
    expect(s.onAttach).toHaveBeenCalledExactlyOnceWith(SUMMARY);
  });

  it("keeps Attach visible but disabled with its reason where this host cannot attach", async () => {
    const reason = "This Paseo app gives plugins no way to place a draft in another session for review.";
    const s = sheet(reason);
    s.render(); await settle(); s.render(); await settle();
    const tree = s.render();
    expect(button(tree, "summary-attach")!.props).toMatchObject({ accessibilityLabel: `Attach summary…. ${reason}`, disabled: true, tabIndex: 0 });
    expect(text(tree)).toContain(reason);
  });
  it("rereads the exact pin before every copy and never regenerates on Copy again", async () => {
    const s = sheet(); s.render(); await settle(); s.render(); await settle();
    expect(host.rpc["sessions.summary-read"]).toHaveBeenCalledExactlyOnceWith({ summaryId: SUMMARY.summaryId, revision: 3 });
    press(button(s.render(), "summary-copy-again")); await settle(); s.render();
    expect(host.rpc["sessions.summary-read"]).toHaveBeenCalledTimes(2);
    expect(host.rpc["sessions.summary-save"]).toHaveBeenCalledTimes(1);
    expect(host.copyText.mock.calls).toEqual([[SUMMARY.text], [SUMMARY.text]]);
  });

  it("preserves the saved revision when its exact read fails and lets Copy again recover", async () => {
    host.rpc["sessions.summary-read"] = vi.fn(async () => ({ ...SUMMARY, revision: 4 }));
    const s = sheet(); s.render(); await settle(); s.render(); await settle();
    expect(banner(s.render())).toContain("saved revision readback did not match");
    expect(host.copyText).not.toHaveBeenCalled();
    host.rpc["sessions.summary-read"] = vi.fn(async () => SUMMARY);
    press(button(s.render(), "summary-copy-again")); await settle();
    expect(banner(s.render())).toContain("Copied to clipboard");
    expect(host.rpc["sessions.summary-save"]).toHaveBeenCalledTimes(1);
  });

  it("never copies a save response for a different source", async () => {
    host.rpc["sessions.summary-save"] = vi.fn(async () => ({ ...SUMMARY, sourceAgentId: "another-source" }));
    const s = sheet(); s.render(); await settle();
    expect(banner(s.render())).toContain("belongs to another source");
    expect(host.rpc["sessions.summary-read"]).not.toHaveBeenCalled();
    expect(host.copyText).not.toHaveBeenCalled();
  });

  it("copies the saved revision without provider-specific claims or paste-verification chores", async () => {
    const s = sheet(); s.render(); await settle(); s.render(); await settle();
    const tree = s.render();
    expect(text(tree)).toContain("Copy uses the saved revision");
    expect(text(tree)).not.toContain("read-only Codex");
    expect(tree.some(node => node.props.testID === "summary-paste-verification")).toBe(false);
    expect(host.copyText).toHaveBeenCalledWith(SUMMARY.text);
  });

  it("ignores a late stored read after the selected source changes", async () => {
    let finishRead!: (revision: SummaryRevision) => void;
    host.rpc["sessions.summary-read"] = vi.fn(() => new Promise<SummaryRevision>(resolve => { finishRead = resolve; }));
    let source = SOURCE;
    const render = () => walk(harness.current.render(() => SummarySheet({ source, colors: COLORS, attachUnavailable: "Not supported", onAttach: vi.fn(), onClose: vi.fn() })));
    render(); await settle(); render(); await settle();
    source = sessionEntry("other-source");
    host.rpc["sessions.summary-save"] = vi.fn(() => new Promise<SummaryRevision>(() => {}));
    render(); finishRead(SUMMARY); await settle();
    expect(host.copyText).not.toHaveBeenCalled();
    expect(banner(render())).toContain("Saving a summary");
  });

});

describe("atomic AttachSheet", () => {
  const sheet = (given: SummaryRevision | null = SUMMARY, capability = { supported: true } as import("../shared/session-navigator").Capability) => {
    const onClose = vi.fn();
    return { onClose, render: () => walk(harness.current.render(() => AttachSheet({ source: SOURCE, revision: given, entries: [SOURCE, sessionEntry("destination", { title: "Review destination" })], capability, colors: COLORS, onClose }))) };
  };
  const choose = (tree: Element[]) => {
    const rows = tree.filter(node => node.type === DenseSessionRow);
    expect(rows.map(row => (row.props.entry as { id: string }).id)).toEqual(["destination"]);
    (rows[0]!.props.onPress as () => void)();
  };
  it.each(["drafted", "refused", "unknown"])("places the exact pin into the chosen destination and reports %s", async state => {
    host.rpc["sessions.summary-attach"] = vi.fn(async pin => ({ ...pin, state, message: "Exact server outcome" }));
    const s = sheet(); s.render(); await settle(); choose(s.render());
    press(button(s.render(), "attach-confirm")); await settle();
    const tree = s.render();
    expect(host.rpc["sessions.summary-attach"]).toHaveBeenCalledExactlyOnceWith({ summaryId: SUMMARY.summaryId, revision: 3, destinationAgentId: "destination" });
    expect(text(tree)).toContain(`${state} · Destination destination · r3`);
    expect(text(tree)).toContain("Review destination (destination)");
    expect(host.rpc["sessions.summary-save"]).not.toHaveBeenCalled();
    expect(host.copyText).not.toHaveBeenCalled();
    press(button(tree, "attach-cancel")); expect(s.onClose).toHaveBeenCalledOnce();
  });
  it("shows the exact unsupported capability reason and performs no RPC", async () => {
    const reason = "Exact host atomic attachment gap";
    const s = sheet(SUMMARY, { supported: false, reason }); s.render(); await settle();
    expect(text(s.render())).toContain(reason);
    press(button(s.render(), "attach-confirm")); await settle();
    for (const rpc of Object.values(host.rpc)) expect(rpc).not.toHaveBeenCalled();
  });
  it("cancel does not attach or generate", async () => {
    const s = sheet(); s.render(); await settle(); choose(s.render()); press(button(s.render(), "attach-cancel"));
    expect(s.onClose).toHaveBeenCalledOnce();
    expect(host.rpc["sessions.summary-attach"]).not.toHaveBeenCalled();
    expect(host.rpc["sessions.summary-save"]).not.toHaveBeenCalled();
  });
  it("suppresses duplicate presses while placement is awaiting readback", async () => {
    let finish!: (value: unknown) => void;
    host.rpc["sessions.summary-attach"] = vi.fn(() => new Promise(resolve => { finish = resolve; }));
    const s = sheet(); s.render(); await settle(); choose(s.render());
    const confirm = button(s.render(), "attach-confirm"); press(confirm); press(confirm);
    expect(host.rpc["sessions.summary-attach"]).toHaveBeenCalledTimes(1);
    finish({ summaryId: SUMMARY.summaryId, revision: 3, destinationAgentId: "destination", state: "drafted", message: "Draft read back" }); await settle();
    expect(text(s.render())).toContain("drafted · Destination destination");
  });
  it("reports lost transport as unknown without a second request", async () => {
    host.rpc["sessions.summary-attach"] = vi.fn(async () => { throw new Error("Reply lost"); });
    const s = sheet(); s.render(); await settle(); choose(s.render()); press(button(s.render(), "attach-confirm")); await settle();
    expect(text(s.render())).toContain("unknown · Destination destination");
    expect(text(s.render())).toContain("Reply lost");
    expect(host.rpc["sessions.summary-attach"]).toHaveBeenCalledTimes(1);
  });
  it("holds mismatched readback as unknown for the requested destination", async () => {
    host.rpc["sessions.summary-attach"] = vi.fn(async pin => ({ ...pin, state: "drafted", destinationAgentId: "foreign", message: "Wrong destination" }));
    const s = sheet(); s.render(); await settle(); choose(s.render()); press(button(s.render(), "attach-confirm")); await settle();
    expect(text(s.render())).toContain("unknown · Destination destination");
    expect(text(s.render())).not.toContain("drafted · Destination foreign");
  });
  it("reads latest without generating and reports an absent summary", async () => {
    host.rpc["sessions.summary-latest"] = vi.fn(async () => ({ [SOURCE.id]: null }));
    const s = sheet(null); s.render(); await settle();
    expect(text(s.render())).toContain("Use Summarize & copy first");
    expect(host.rpc["sessions.summary-save"]).not.toHaveBeenCalled();
  });
  it.each(["empty", "malformed", "wrong source", "changed text"])("refuses %s summary data", async kind => {
    host.rpc["sessions.summary-read"] = vi.fn(async () => kind === "malformed" ? {} : { ...SUMMARY, text: kind === "empty" ? "" : kind === "changed text" ? "different" : SUMMARY.text, sourceAgentId: kind === "wrong source" ? "elsewhere" : SOURCE.id });
    const s = sheet(); s.render(); await settle(); const tree = s.render();
    expect(tree.some(node => node.props.testID === "attach-revision-identifier")).toBe(false);
    expect(host.rpc["sessions.summary-attach"]).not.toHaveBeenCalled();
  });
});


describe("destination composer instructions", () => {
  it.each([true, false])("opens and dismisses instructions with source availability %s without any RPC or send", available => {
    const render = () => walk(harness.current.render(() => SummaryAttachmentHelp({ available, colors: COLORS })));
    let tree = render(); expect(tree.some(node => node.props.testID === "summary-native-instructions")).toBe(false);
    press(button(tree, "summary-native-help")); tree = render();
    expect(text(tree)).toContain(available ? "Only your native picker selection" : "cannot register");
    press(button(tree, "summary-native-help")); tree = render();
    expect(tree.some(node => node.props.testID === "summary-native-instructions")).toBe(false);
    for (const rpc of Object.values(host.rpc)) expect(rpc).not.toHaveBeenCalled();
    expect(host.copyText).not.toHaveBeenCalled();
  });
});
