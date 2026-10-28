import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, ActivityIndicator: "ActivityIndicator", Pressable: "Pressable", Text: "Text", View: "View" }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ ScrollView: "ScrollView", Icon: "Icon" }));
vi.mock("../client/anchored-menu", () => ({ AnchoredMenu: "AnchoredMenu" }));
vi.mock("../client/control-help", () => ({ useControlHelp: () => ({ bindings: {}, dismiss() {} }) }));

import { ArchiveRecoverySheet, noteLine, observedFacts } from "../client/archive-recovery";
import { recoveryKey, type RecoveryItem, type useArchiveRecovery } from "../client/archive-feedback";
import { Action, Disclosure } from "../client/page-controls";
import type { LifecycleRow } from "../shared/session-lifecycle";

type Element = { type?: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => !node || typeof node !== "object" ? [] : Array.isArray(node) ? node.flatMap(walk)
  : [node as Element, ...walk((node as Element).props.children), ...walk((node as Element).props.header)];
const texts = (tree: unknown) => walk(tree).filter((element) => element.type === "Text").map((element) => [element.props.children].flat().join(""));
const colors = { foreground: "#111", foregroundMuted: "#666", surface0: "#fff", border: "#ccc", statusWarning: "#c80" } as unknown as PluginTheme["colors"];
const NOW = Date.parse("2026-10-24T08:10:00.000Z");
const row = (id: string, patch: Partial<LifecycleRow> = {}) => ({ id, title: `SWE • DEMO • ${id}`, eligible: true, outcome: "uncertain", reason: "Fixture reason.", message: null,
  availability: { agentId: id, state: "readable", nativeStatus: "idle", archivedAt: "2026-10-24T08:00:00.000Z", reason: "Exact-ID read.", checkedAt: "2026-10-24T08:05:00.000Z", revision: "r1" }, ...patch }) as LifecycleRow;
const item = (id: string, state: RecoveryItem["state"], patch: Partial<LifecycleRow> = {}, error: string | null = null): RecoveryItem =>
  ({ ref: { reviewId: `00000000-0000-4000-8000-0000000000${id.length}${id.length}`, agentId: id }, state, row: row(id, patch), error });
type Recovery = ReturnType<typeof useArchiveRecovery>;
const sheet = (patch: Partial<Recovery> = {}, extra: { canRefresh?: boolean; hostOnline?: boolean; compact?: boolean } = {}) => {
  const recovery = { items: [], error: null, refreshing: null, notes: {}, hidden: [], persistence: "device", refresh: vi.fn(async () => {}), ...patch } as unknown as Recovery;
  const props = { recovery, root: { current: null }, rootSize: { width: 390, height: 800 }, colors, compact: extra.compact ?? true,
    canRefresh: extra.canRefresh ?? true, hostOnline: extra.hostOnline ?? true, onOpen: vi.fn(), onHide: vi.fn(), onClose: vi.fn() };
  return { props, tree: harness.current.render(() => ArchiveRecoverySheet(props)) as Element };
};
const actions = (tree: unknown, label: string) => walk(tree).filter((element) => element.type === Action && element.props.label === label);

beforeEach(() => { harness.fresh(); vi.useFakeTimers({ toFake: ["Date"] }); vi.setSystemTime(NOW); });

describe("U4 Review archive sheet", () => {
  it("keeps the archived notice to one sentence and actions while diagnostics stay collapsed", () => {
    const one = item("aud", "unconfirmed", { outcome: "archived", runtime: "unconfirmed", message: "RAW_DIAGNOSTIC" });
    const { tree } = sheet({ items: [one], notes: { [recoveryKey(one.ref)]: { at: NOW, outcome: "unchanged", title: "A", state: "unconfirmed" } } });
    const listed = walk(tree).find((element) => element.props.role === "listitem")!;
    const outside = [listed.props.children].flat().filter((child) => (child as Element | null)?.type !== Disclosure);
    expect(texts(outside)).toEqual(["SWE • DEMO • aud", "Archived. Paseo hasn’t confirmed the session has stopped."]);
    expect(actions(listed, "Refresh status")).toHaveLength(1);
    expect(actions(listed, "Hide notice")).toHaveLength(1);
    const details = walk(listed).find((element) => element.type === Disclosure)!;
    expect(details.props.open).toBeUndefined();
    expect(texts(details).join(" ")).toMatch(/RAW_DIAGNOSTIC/);
    expect(texts(details).join(" ")).toMatch(/host reports idle/);
    expect(texts(details).join(" ")).toMatch(/unchanged/);
  });
  it("keeps raw read failures in details and does not call an unknown result archived", () => {
    const one = item("lost", "unknown", { availability: undefined }, "RAW_READ_ERROR");
    const { tree } = sheet({ items: [one] });
    const listed = walk(tree).find((element) => element.props.role === "listitem")!;
    const outside = [listed.props.children].flat().filter((child) => (child as Element | null)?.type !== Disclosure);
    expect(texts(outside)).toContain("Archive result unknown");
    expect(texts(outside).join(" ")).not.toMatch(/RAW_READ_ERROR|Archived\. Paseo/);
    expect(texts(outside)).toContain("Couldn’t refresh status. Try again when the host is available.");
    expect(texts(listed).join(" ")).toContain("RAW_READ_ERROR");
  });

  it("lists every unresolved item with its state, plain reason, observed facts and its actions", () => {
    const unknown = item("lost", "unknown", { message: "Archive returned but exact-ID terminal retirement readback is unavailable. Check native outcome; do not retry the request.", availability: undefined });
    const unconfirmed = item("aud", "unconfirmed", { outcome: "archived", runtime: "unconfirmed" });
    const { tree, props } = sheet({ items: [unknown, unconfirmed] });
    expect(tree.props).toMatchObject({ dialog: true, label: "Review archive", testID: "archive-recovery-sheet", placement: { align: "stretch", cover: true } });
    expect(texts(tree.props.header)).toContain("2 archives need attention");
    const listed = walk(tree).filter((element) => element.props.role === "listitem");
    expect(listed.map((element) => element.props.testID)).toEqual(["archive-recovery-item-lost", "archive-recovery-item-aud"]);
    expect(texts(listed[0])).toEqual(expect.arrayContaining(["SWE • DEMO • lost", "Archive result unknown", "Not checked yet",
      "The archive request was sent, but the host has not confirmed whether the session was archived. Another archive request will not be sent while its outcome is uncertain."]));
    // Membership and the runtime stop are separate facts.
    expect(texts(listed[1])).toEqual(expect.arrayContaining(["Archived. Paseo hasn’t confirmed the session has stopped.", "Last checked 5 minutes ago · archived 10 minutes ago · host reports idle",
      "The host lists it in the archive but has not reported its runtime stopped, so it is not called stopped. Refresh status asks again; no archive request is sent again."]));
    // Jargon and IDs wait in Technical details.
    const outside = [listed[0]!.props.children].flat().filter((child) => (child as Element | null)?.type !== Disclosure);
    expect(texts(outside).join(" ")).not.toMatch(/terminal retirement|Check native|fence|Session ID/);
    const details = walk(listed[0]).find((element) => element.type === Disclosure)!;
    expect(details.props).toMatchObject({ label: "Technical details" });
    expect(texts(details)).toEqual(expect.arrayContaining(["Session ID lost", `Review ID ${unknown.ref.reviewId}`, unknown.row!.message]));
    const refresh = actions(listed[1], "Refresh status")[0]!, open = actions(listed[0], "Open session")[0]!;
    expect(refresh.props).toMatchObject({ accessibilityLabel: "Refresh archive status: SWE • DEMO • aud", disabled: false });
    expect(refresh.props.small).toBeUndefined(); expect(open.props.small).toBeUndefined();
    (refresh.props.onPress as () => void)(); (open.props.onPress as () => void)();
    expect(props.recovery.refresh).toHaveBeenCalledExactlyOnceWith(unconfirmed.ref); expect(props.onOpen).toHaveBeenCalledExactlyOnceWith(unknown);
    (actions(tree.props.header, "Close")[0]!.props.onPress as () => void)(); expect(props.onClose).toHaveBeenCalledTimes(1);
  });
  it.each(["resolve", "reject"])("keeps the refresh Action pending until its promise settles (%s)", async (outcome) => {
    let resolve!: () => void, reject!: (error: Error) => void;
    const pending = new Promise<void>((yes, no) => { resolve = yes; reject = no; });
    const refresh = vi.fn(() => pending);
    const { tree } = sheet({ items: [item("aud", "unconfirmed")], refresh });
    const props = actions(tree, "Refresh status")[0]!.props as Parameters<typeof Action>[0];
    // Give the control its own hook slots, as React does for a child component.
    harness.fresh();
    const draw = () => harness.current.render(() => Action(props)) as Element;
    let control = draw();
    (control.props.onPress as () => void)(); (control.props.onPress as () => void)();
    expect(refresh).toHaveBeenCalledTimes(1);
    control = draw();
    expect(control.props).toMatchObject({ "aria-busy": true, accessibilityState: { busy: true } });
    expect(walk(control).some((node) => node.type === "ActivityIndicator")).toBe(true);
    if (outcome === "resolve") resolve(); else reject(new Error("Fixture read failed."));
    await Promise.resolve(); await Promise.resolve();
    control = draw();
    expect(control.props["aria-busy"]).toBeUndefined();
    expect(walk(control).some((node) => node.type === "ActivityIndicator")).toBe(false);
    (control.props.onPress as () => void)();
    expect(refresh).toHaveBeenCalledTimes(2);
    await Promise.resolve();
  });
  it("disables Refresh status while another action runs or the host is offline, and says why", () => {
    const one = item("aud", "unconfirmed");
    const offline = sheet({ items: [one] }, { canRefresh: false, hostOnline: false }).tree;
    expect(actions(offline, "Refresh status")[0]!.props.disabled).toBe(true);
    expect(texts(offline)).toContain("The host is offline. Refresh status works again when it reconnects.");
    const { tree, props } = sheet({ items: [one, item("lost", "unknown", { availability: undefined })] }, { canRefresh: false });
    (actions(tree, "Refresh status")[0]!.props.onPress as () => void)(); expect(props.recovery.refresh).not.toHaveBeenCalled();
    expect(actions(tree, "Open session")[0]!.props.disabled).toBeUndefined();
  });
  it("U5 shows checking, unchanged, changed, couldn't check and a cleared confirmation as status text", () => {
    const [a, b, c] = [item("a", "unknown"), item("b", "unconfirmed"), item("c", "unavailable", {}, "Fixture host offline.")];
    const notes = { [recoveryKey(a.ref)]: { at: NOW - 14_000, outcome: "unchanged", title: "A", state: "unknown" }, [recoveryKey(b.ref)]: { at: NOW, outcome: "changed", title: "B", state: "unconfirmed" },
      [recoveryKey(c.ref)]: { at: NOW, outcome: "failed", title: "C", error: "Fixture host read failed." }, "r:gone": { at: NOW, outcome: "confirmed", title: "SWE • DEMO • gone" } } as const;
    const { tree } = sheet({ items: [a, b, c], notes, refreshing: recoveryKey(a.ref) });
    expect(walk(tree).filter((element) => element.props.role === "status").map((element) => element.props.children)).toEqual([
      "SWE • DEMO • gone: confirmed archived · notice cleared", "Couldn’t refresh status. Try again when the host is available."]);
    expect(actions(tree, "Checking…")).toHaveLength(1); expect(actions(tree, "Refresh status")).toHaveLength(2);
    expect(texts(tree)).toContain("Its latest record could not be read: Fixture host offline.");
    const all = sheet({ items: [a, b], refreshing: "all" }).tree; expect(actions(all, "Checking…")).toHaveLength(2);
  });
  it("AR-01 offers no Open for a session its exact-ID read shows archived, says why in one line, and keeps Refresh status", () => {
    const NOT_OPENED = "Opening an archived session asks Paseo to restore its workspace. Use the Archived list to open it.";
    // An archived parent, a workspace Paseo cannot restore or a restore path that diverged all start here: an archived read.
    const archived = [item("aud", "unconfirmed", { outcome: "archived", runtime: "unconfirmed" }), item("legacy", "unconfirmed"), item("unknown-archived", "unknown")];
    for (const entry of archived) {
      const { tree } = sheet({ items: [entry] });
      expect(actions(tree, "Open session")).toEqual([]); expect(actions(tree, "Refresh status")).toHaveLength(1);
      expect(texts(tree)).toContain(NOT_OPENED);
    }
    // Uncertain, not yet read, or read without archive membership: Open stays, and so does its exact-ID guard downstream.
    for (const entry of [item("lost", "unknown", { availability: undefined }), item("live", "unknown", { availability: { ...row("live").availability!, archivedAt: null } }),
      item("down", "unavailable", { availability: { ...row("down").availability!, state: "unknown", archivedAt: undefined } }, "Fixture host offline.")]) {
      const { tree } = sheet({ items: [entry] });
      expect(actions(tree, "Open session")).toHaveLength(1); expect(texts(tree)).not.toContain(NOT_OPENED);
    }
  });
  it("AR-02 offers Hide notice only after an unchanged Refresh status of an archived session", () => {
    const one = item("aud", "unconfirmed", { outcome: "archived", runtime: "unconfirmed" }), key = recoveryKey(one.ref);
    const ACK = "Hiding this notice leaves the archive unresolved. It returns if its state changes.";
    const unchanged = { [key]: { at: NOW, outcome: "unchanged", title: "A", state: "unconfirmed" } } as const;
    const { tree, props } = sheet({ items: [one], notes: unchanged });
    expect(texts(tree)).toEqual(expect.arrayContaining(["Checked just now · unchanged", ACK]));
    const hide = actions(tree, "Hide notice")[0]!;
    expect(hide.props).toMatchObject({ accessibilityLabel: "Hide archive notice: SWE • DEMO • aud" });
    (hide.props.onPress as () => void)();
    expect(props.onHide).toHaveBeenCalledExactlyOnceWith(one); expect(props.recovery.refresh).not.toHaveBeenCalled(); expect(props.onOpen).not.toHaveBeenCalled();
    // Not before a Refresh, not after a failed or changed one, and never for a result that is still unknown.
    for (const [entry, notes] of [[one, {}], [one, { [key]: { at: NOW, outcome: "failed", title: "A", error: "Fixture read failed." } }],
      [one, { [key]: { at: NOW, outcome: "changed", title: "A", state: "unconfirmed" } }],
      [item("u", "unknown"), { [recoveryKey(item("u", "unknown").ref)]: { at: NOW, outcome: "unchanged", title: "U", state: "unknown" } }]] as const) {
      const { tree: other } = sheet({ items: [entry], notes: notes as Recovery["notes"] });
      expect(actions(other, "Hide notice")).toEqual([]); expect(texts(other)).not.toContain(ACK);
    }
  });
  it("U3 says when hiding lasts only this app session, and an empty sheet says so", () => {
    for (const persistence of ["session", "failed", "unreadable"] as const)
      expect(texts(sheet({ items: [item("a", "unknown")], persistence }).tree)).toContain("Hidden archive notices are kept for this app session only on this device.");
    expect(texts(sheet({ items: [item("a", "unknown")] }).tree)).not.toContain("Hidden archive notices are kept for this app session only on this device.");
    const empty = sheet({ error: "Fixture list failed." }, { compact: false }).tree;
    expect(empty.props.placement).toMatchObject({ align: "end", width: 660 });
    expect(texts(empty)).toEqual(expect.arrayContaining(["No archives need attention", "Couldn’t read archive checks. Try again when the host is available.", "Fixture list failed."]));
  });
});

describe("note and fact lines", () => {
  it("never infers state from age: no read says so, and a clock ahead is named", () => {
    expect(noteLine({ at: NOW - 90_000, outcome: "unchanged", title: "A", state: "unknown" }, NOW)).toBe("Checked 1 minute ago · unchanged");
    expect(noteLine({ at: NOW, outcome: "failed", title: "A" }, NOW)).toBe("Couldn't check: no reply");
    expect(observedFacts(item("a", "unknown", { availability: undefined }), NOW)).toBe("Not checked yet");
    expect(observedFacts({ ...item("a", "unavailable"), row: null }, NOW)).toBe("Not checked yet");
  });
});
