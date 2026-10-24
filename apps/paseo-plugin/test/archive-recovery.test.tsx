import type { PluginTheme } from "@getpaseo/plugin";
import { beforeEach, describe, expect, it, vi } from "vitest";

const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, Pressable: "Pressable", Text: "Text", View: "View" }));
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
  const recovery = { items: [], error: null, refreshing: null, notes: {}, persistence: "device", refresh: vi.fn(async () => {}), ...patch } as unknown as Recovery;
  const props = { recovery, root: { current: null }, rootSize: { width: 390, height: 800 }, colors, compact: extra.compact ?? true,
    canRefresh: extra.canRefresh ?? true, hostOnline: extra.hostOnline ?? true, onOpen: vi.fn(), onClose: vi.fn() };
  return { props, tree: harness.current.render(() => ArchiveRecoverySheet(props)) as Element };
};
const actions = (tree: unknown, label: string) => walk(tree).filter((element) => element.type === Action && element.props.label === label);

beforeEach(() => { harness.fresh(); vi.useFakeTimers({ toFake: ["Date"] }); vi.setSystemTime(NOW); });

describe("U4 Review archive sheet", () => {
  it("lists every unresolved item with its state, plain reason, observed facts and both actions", () => {
    const unknown = item("lost", "unknown", { message: "Archive returned but exact-ID terminal retirement readback is unavailable. Check native outcome; do not retry the request.", availability: undefined });
    const unconfirmed = item("aud", "unconfirmed", { outcome: "archived", runtime: "unconfirmed" });
    const { tree, props } = sheet({ items: [unknown, unconfirmed] });
    expect(tree.props).toMatchObject({ dialog: true, label: "Review archive", testID: "archive-recovery-sheet", placement: { align: "stretch", cover: true } });
    expect(texts(tree.props.header)).toContain("2 archives need attention");
    const listed = walk(tree).filter((element) => element.props.role === "listitem");
    expect(listed.map((element) => element.props.testID)).toEqual(["archive-recovery-item-lost", "archive-recovery-item-aud"]);
    expect(texts(listed[0])).toEqual(expect.arrayContaining(["SWE • DEMO • lost", "Archive result unknown", "Not checked yet",
      "The archive request was sent, but the host has not confirmed whether the session was archived. Another archive request will not be sent while its outcome is uncertain."]));
    expect(texts(listed[1])).toEqual(expect.arrayContaining(["Archived · runtime stop not confirmed", "Last checked 5 minutes ago · archived 10 minutes ago · host reports idle"]));
    // Jargon and IDs wait in Technical details.
    const outside = [listed[0]!.props.children].flat().filter((child) => (child as Element | null)?.type !== Disclosure);
    expect(texts(outside).join(" ")).not.toMatch(/terminal retirement|Check native|fence|Session ID/);
    const details = walk(listed[0]).find((element) => element.type === Disclosure)!;
    expect(details.props).toMatchObject({ label: "Technical details" });
    expect(texts(details)).toEqual(expect.arrayContaining(["Session ID lost", `Review ID ${unknown.ref.reviewId}`, unknown.row!.message]));
    const refresh = actions(listed[1], "Refresh status")[0]!, open = actions(listed[1], "Open session")[0]!;
    expect(refresh.props).toMatchObject({ accessibilityLabel: "Refresh archive status: SWE • DEMO • aud", disabled: false });
    expect(refresh.props.small).toBeUndefined(); expect(open.props.small).toBeUndefined();
    (refresh.props.onPress as () => void)(); (open.props.onPress as () => void)();
    expect(props.recovery.refresh).toHaveBeenCalledExactlyOnceWith(unconfirmed.ref); expect(props.onOpen).toHaveBeenCalledExactlyOnceWith(unconfirmed);
    (actions(tree.props.header, "Close")[0]!.props.onPress as () => void)(); expect(props.onClose).toHaveBeenCalledTimes(1);
  });
  it("disables Refresh status while another action runs or the host is offline, and says why", () => {
    const one = item("aud", "unconfirmed");
    const offline = sheet({ items: [one] }, { canRefresh: false, hostOnline: false }).tree;
    expect(actions(offline, "Refresh status")[0]!.props.disabled).toBe(true);
    expect(texts(offline)).toContain("The host is offline. Refresh status works again when it reconnects.");
    const { tree, props } = sheet({ items: [one] }, { canRefresh: false });
    (actions(tree, "Refresh status")[0]!.props.onPress as () => void)(); expect(props.recovery.refresh).not.toHaveBeenCalled();
    expect(actions(tree, "Open session")[0]!.props.disabled).toBeUndefined();
  });
  it("U5 shows checking, unchanged, changed, couldn't check and a cleared confirmation as status text", () => {
    const [a, b, c] = [item("a", "unknown"), item("b", "unconfirmed"), item("c", "unavailable", {}, "Fixture host offline.")];
    const notes = { [recoveryKey(a.ref)]: { at: NOW - 14_000, outcome: "unchanged", title: "A", state: "unknown" }, [recoveryKey(b.ref)]: { at: NOW, outcome: "changed", title: "B", state: "unconfirmed" },
      [recoveryKey(c.ref)]: { at: NOW, outcome: "failed", title: "C", error: "Fixture host read failed." }, "r:gone": { at: NOW, outcome: "confirmed", title: "SWE • DEMO • gone" } } as const;
    const { tree } = sheet({ items: [a, b, c], notes, refreshing: recoveryKey(a.ref) });
    expect(walk(tree).filter((element) => element.props.role === "status").map((element) => element.props.children)).toEqual([
      "SWE • DEMO • gone: confirmed archived · notice cleared", "Checked just now · unchanged", "Checked just now · now Archived · runtime stop not confirmed", "Couldn't check: Fixture host read failed."]);
    expect(actions(tree, "Checking…")).toHaveLength(1); expect(actions(tree, "Refresh status")).toHaveLength(2);
    expect(texts(tree)).toContain("Its latest record could not be read: Fixture host offline.");
    const all = sheet({ items: [a, b], refreshing: "all" }).tree; expect(actions(all, "Checking…")).toHaveLength(2);
  });
  it("U3 says when hiding lasts only this app session, and an empty sheet says so", () => {
    for (const persistence of ["session", "failed", "unreadable"] as const)
      expect(texts(sheet({ items: [item("a", "unknown")], persistence }).tree)).toContain("Hidden archive notices are kept for this app session only on this device.");
    expect(texts(sheet({ items: [item("a", "unknown")] }).tree)).not.toContain("Hidden archive notices are kept for this app session only on this device.");
    const empty = sheet({ error: "Fixture list failed." }, { compact: false }).tree;
    expect(empty.props.placement).toMatchObject({ align: "end", width: 660 });
    expect(texts(empty)).toEqual(expect.arrayContaining(["No archives need attention", "Archive checks could not be read: Fixture list failed."]));
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
