import { isValidElement } from "react";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { AttachOutcome, SummaryRevision } from "../shared/session-navigator";

// Each test gets a fresh renderer, so a saved summary never carries into another test.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const host = vi.hoisted(() => ({ rpc: {} as Record<string, ReturnType<typeof vi.fn>>, copyText: null as unknown as Mock<(text: string) => Promise<void>> }));
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, useWindowDimensions: () => ({ width: 390, height: 844 }), Pressable: "Pressable", Text: "Text", View: "View", ScrollView: "ScrollView", Image: "Image",
  BackHandler: { addEventListener: () => ({ remove() {} }) } }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => host.rpc[contract.name] }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", TextInput: "TextInput", ScrollView: "ScrollView", copyText: (text: string) => host.copyText(text),
  Modal: Object.assign(function Modal() { return null; }, { Content: function ModalContent() { return null; } }) }));

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
  host.rpc = { "sessions.summary-save": vi.fn(async () => SUMMARY), "sessions.summary-read": vi.fn(async () => SUMMARY), "sessions.summary-attach": vi.fn() };
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

  it("puts Copy again left and Attach to another seat right", async () => {
    const s = sheet();
    s.render(); await settle(); s.render(); await settle();
    const tree = s.render();
    expect(footer(tree)).toEqual(["Copy again", "Attach to another seat…"]);
    press(button(tree, "summary-attach"));
    expect(s.onAttach).toHaveBeenCalledExactlyOnceWith(SUMMARY);
  });

  it("keeps Attach visible but disabled with its reason where this host cannot attach", async () => {
    const reason = "This Paseo app gives plugins no way to place a draft in another session for review.";
    const s = sheet(reason);
    s.render(); await settle(); s.render(); await settle();
    const tree = s.render();
    expect(button(tree, "summary-attach")!.props).toMatchObject({ accessibilityLabel: `Attach to another seat…. ${reason}`, disabled: true, tabIndex: 0 });
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

  it("compares explicitly pasted text with the saved revision without claiming automatic clipboard readback", async () => {
    const s = sheet(); s.render(); await settle(); s.render(); await settle();
    let tree = s.render();
    expect(text(tree)).toContain("Clipboard readback is not automatic");
    const paste = (value: string) => (tree.find(node => node.props.testID === "summary-paste-verification")!.props.onChangeText as (text: string) => void)(value);
    paste("different text"); tree = s.render();
    expect(text(tree)).toContain("does not match this saved revision");
    paste(SUMMARY.text); tree = s.render();
    expect(text(tree)).toContain("Pasted text matches saved r3.");
    expect(host.copyText).toHaveBeenCalledTimes(1);
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

describe("AttachSheet", () => {
  const DESTINATIONS = [SOURCE, sessionEntry("agent-b", { title: "Billing" }), sessionEntry("agent-old", { title: "Old billing", archivedAt: "2026-10-20T09:00:00Z" }), sessionEntry("agent-c", { title: "Release notes" })];
  const sheet = (newLead: Parameters<typeof AttachSheet>[0]["newLead"] = vi.fn<(revision: SummaryRevision) => void>()) => {
    let tree: Element[] = [];
    const render = () => (tree = walk(harness.current.render(() => AttachSheet({ source: SOURCE, revision: SUMMARY, destinations: DESTINATIONS, opened: new Map(), colors: COLORS, compact: true, newLead, onClose: vi.fn() }))));
    render();
    return { render, get tree() { return tree; } };
  };
  const rows = (tree: Element[]) => tree.filter((node) => node.type === DenseSessionRow).map((node) => (node.props.entry as { id: string }).id);
  const pick = (s: ReturnType<typeof sheet>, id: string) => { press(s.tree.find((node) => node.type === DenseSessionRow && node.props.testID === `attach-destination-${id}`)); s.render(); };
  const target = (s: ReturnType<typeof sheet>, id: "existing" | "new") => { (s.tree.find((node) => node.type === Choices)!.props.onSelect as (id: string) => void)(id); s.render(); };

  it("offers every other live seat, never the source or an archived one", () => {
    expect(rows(sheet().tree)).toEqual(["agent-b", "agent-c"]);
  });

  it("previews the draft with the source and revision once a destination is chosen", () => {
    const s = sheet();
    expect(s.tree.find((node) => node.props.testID === "attach-preview")).toBeUndefined();
    expect(button(s.tree, "attach-confirm")!.props).toMatchObject({ disabled: true, accessibilityLabel: "Attach to draft. Choose a destination." });
    pick(s, "agent-b");
    const preview = text(walk(s.tree.find((node) => node.props.testID === "attach-preview")));
    expect(preview).toContain("Draft preview · Billing");
    expect(preview).toContain("Context from Route review (r3)");
  });

  it("attaches the pinned revision to the chosen draft, shows the outcome and sends nothing", async () => {
    const drafted: AttachOutcome = { state: "drafted", message: "The summary is in Billing's draft for its operator to review.", destinationAgentId: "agent-b", summaryId: SUMMARY.summaryId, revision: 3 };
    host.rpc["sessions.summary-attach"] = vi.fn(async () => drafted);
    const s = sheet();
    pick(s, "agent-b");
    press(button(s.tree, "attach-confirm")); await settle(); s.render();
    expect(host.rpc["sessions.summary-attach"]).toHaveBeenCalledExactlyOnceWith({ summaryId: SUMMARY.summaryId, revision: 3, destinationAgentId: "agent-b" });
    expect(text(walk(s.tree.find((node) => node.props.testID === "attach-result")))).toBe(drafted.message);
    expect(button(s.tree, "attach-confirm")!.props).toMatchObject({ accessibilityLabel: "Attached", disabled: true });
    // A given revision is never saved again, and attaching is the only call: nothing is sent to the destination.
    expect(host.rpc["sessions.summary-save"]).not.toHaveBeenCalled();
    expect(text(s.tree)).toContain("Review before sending. Nothing is sent automatically.");
  });

  it("asks to check the destination's draft when the attach outcome is unknown", async () => {
    host.rpc["sessions.summary-attach"] = vi.fn(async (): Promise<AttachOutcome> => ({ state: "unknown", message: "No reply from the draft owner.", destinationAgentId: "agent-b", summaryId: SUMMARY.summaryId, revision: 3 }));
    const s = sheet();
    pick(s, "agent-b");
    press(button(s.tree, "attach-confirm")); await settle(); s.render();
    expect(text(walk(s.tree.find((node) => node.props.testID === "attach-result")))).toBe("No reply from the draft owner. Check the destination's draft before trying again.");
  });

  it("opens New lead with the pinned revision", () => {
    const newLead = vi.fn();
    const s = sheet(newLead);
    target(s, "new");
    const start = button(s.tree, "attach-new-lead")!;
    expect(start.props).toMatchObject({ accessibilityLabel: "New lead with r3", disabled: false });
    press(start);
    expect(newLead).toHaveBeenCalledExactlyOnceWith(SUMMARY);
    expect(host.rpc["sessions.summary-attach"]).not.toHaveBeenCalled();
  });

  it("shows why New lead is not offered when the surface gives a reason instead", () => {
    const reason = "New lead opens only from Kontor's Sessions.";
    const s = sheet(reason);
    target(s, "new");
    expect(button(s.tree, "attach-new-lead")!.props).toMatchObject({ disabled: true, accessibilityLabel: expect.stringContaining(`. ${reason}`) });
    expect(text(s.tree)).toContain(reason);
  });
});
