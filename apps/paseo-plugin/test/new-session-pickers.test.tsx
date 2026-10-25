import { beforeEach, afterEach, describe, expect, it, vi } from "vitest";
import type { PaseoApi } from "@getpaseo/client";
import type { ReactElement } from "react";
import { sessionEntry } from "./session-fixtures";
import type { JiraIssue } from "../shared/session-create";

const hooks = vi.hoisted(() => ({
  slots: [] as unknown[], cursor: 0, effect: 0, deps: [] as (readonly unknown[] | undefined)[], cleanups: [] as (void | (() => void))[],
  read: vi.fn(), client: null as unknown, clients: [] as string[],
  state<T>(initial: T | (() => T)): [T, (value: T | ((before: T) => T)) => void] {
    const at = this.cursor++;
    if (!(at in this.slots)) this.slots[at] = typeof initial === "function" ? (initial as () => T)() : initial;
    return [this.slots[at] as T, (next) => { this.slots[at] = typeof next === "function" ? (next as (before: T) => T)(this.slots[at] as T) : next; }];
  },
  run(effect: () => void | (() => void), deps?: readonly unknown[]) {
    const at = this.effect++, previous = this.deps[at];
    if (previous && deps && deps.length === previous.length && deps.every((v, i) => Object.is(v, previous[i]))) return;
    this.cleanups[at]?.(); this.deps[at] = deps; this.cleanups[at] = effect();
  },
  dispose() { this.cleanups.forEach((cleanup) => cleanup?.()); this.cleanups = []; },
}));
vi.mock("react", async (original) => ({ ...await original<typeof import("react")>(),
  useState: <T,>(initial: T | (() => T)) => hooks.state(initial),
  useRef: <T,>(initial: T) => hooks.state(() => ({ current: initial }))[0],
  useEffect: (effect: () => void | (() => void), deps?: readonly unknown[]) => hooks.run(effect, deps),
}));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, useWindowDimensions: () => ({ width: 390, height: 844 }), View: "View", Text: "Text", Pressable: "Pressable", ScrollView: "ScrollView" }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: () => hooks.read, getPaseoClient: (host: string) => { hooks.clients.push(host); return hooks.client; } }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ Icon: "Icon", TextInput: "TextInput" }));
vi.mock("../client/page-controls", () => ({ Action: "Action", NO_PRESS_DELAY: {}, pressFeedback: () => ({}) }));

import { isSourceId, readSource, readSourceCatalog, sourceRows, sourceTokenMatches } from "../client/source-picker";
import { issueTypeIcon, jiraPickerRows, JiraIssuePicker } from "../client/jira-issue-picker";
import { SourceSessionPicker } from "../client/source-session-picker";
import { reconcilePickerActive, SearchPicker } from "../client/search-picker";

const colors = { foreground: "#111", foregroundMuted: "#555", surface2: "#eee", border: "#ddd", accent: "#00f", statusWarning: "#850" } as never;
const id = "00000000-0000-4000-8000-000000000001";
const native = (key: string, patch = {}) => ({ agent: { ...sessionEntry(key), provider: "fixture", ...patch }, project: null });
const tick = async () => { for (let i = 0; i < 8; i++) await Promise.resolve(); };
const deferred = <T,>() => { let resolve!: (value: T) => void; const promise = new Promise<T>((done) => { resolve = done; }); return { promise, resolve }; };
const render = <T,>(draw: () => T) => { hooks.cursor = 0; hooks.effect = 0; return draw(); };
function sourceClient(value: unknown = native(id), timeline: unknown = { entries: [{ item: { type: "assistant_message", text: "last assistant message" } }] }) {
  const refresh = vi.fn().mockResolvedValue(value), refetch = vi.fn().mockResolvedValue(timeline);
  const list = vi.fn().mockResolvedValue({ entries: [], pageInfo: { hasMore: false, nextCursor: null } });
  const ref = vi.fn(() => ({ refresh, timeline: { refetch } }));
  return { client: { agents: { ref, list } } as unknown as PaseoApi, refresh, refetch, ref, list };
}
beforeEach(() => { vi.useFakeTimers(); hooks.slots = []; hooks.cursor = 0; hooks.effect = 0; hooks.deps = []; hooks.cleanups = []; hooks.read = vi.fn(); hooks.clients = []; });
afterEach(() => { hooks.dispose(); vi.useRealTimers(); });

describe("source picker data", () => {
  it("filters archived scope before the newest10 limit, includes errors, and uses created time with stable ID ties", () => {
    const rows = Array.from({ length: 35 }, (_, i) => sessionEntry(`session-${String(i).padStart(2, "0")}`, {
      createdAt: new Date(1000 * (i < 2 ? 1 : i)).toISOString(), updatedAt: new Date(1000 * (100 - i)).toISOString(),
      archivedAt: i >= 20 ? "2026-10-20" : null, status: i === 19 ? "error" : "idle",
    }));
    expect(sourceRows(rows.reverse(), "", false).map((entry) => entry.id)).toEqual(Array.from({ length: 10 }, (_, i) => `session-${19 - i}`));
    expect(sourceRows(rows, "", true).map((entry) => entry.id)).toEqual(Array.from({ length: 10 }, (_, i) => `session-${34 - i}`));
    expect(sourceRows(rows, "session-0", false).map((entry) => entry.id).slice(-2)).toEqual(["session-00", "session-01"]);
  });
  it("searches all title/ID matches beyond the initial10 without imposing a search cap", () => {
    const rows = Array.from({ length: 70 }, (_, i) => sessionEntry(`id-${i}`, { title: `Human title ${i}`, createdAt: new Date(i * 1000).toISOString() }));
    expect(sourceRows(rows, "", false)).toHaveLength(10);
    expect(sourceRows(rows, "Human title", false)).toHaveLength(70);
    expect(sourceRows(rows, "title 0", false).map((entry) => entry.id)).toContain("id-0");
    expect(sourceRows(rows, "id-0", false).map((entry) => entry.id)).toEqual(["id-0"]);
    expect(isSourceId(` ${id} `)).toBe(true);
  });
  it("matches title words without numeric substring false positives across the full catalog", () => {
    const sid = (n: number) => `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`;
    const rows = Array.from({ length: 70 }, (_, i) => sessionEntry(sid(i + 1), {
      title: `Human context ${i + 1} with a long readable session name`, createdAt: new Date(1000000 * (i + 1)).toISOString(),
    }));
    expect(sourceRows(rows, "Human context 1 with", false)).toHaveLength(1);
    expect(sourceRows(rows, "Human context 1 with", false)[0]!.id).toBe(sid(1));
    expect(sourceTokenMatches("Human context 10 with", "1")).toBe(false);
    expect(sourceTokenMatches("Human context 1 with", "1")).toBe(true);
  });
  it("collects every host page including archived and rejects incomplete paging", async () => {
    const f = sourceClient();
    f.list.mockResolvedValueOnce({ entries: [native("a")], pageInfo: { hasMore: true, nextCursor: "page2" } })
      .mockResolvedValueOnce({ entries: [native("b", { archivedAt: "2026-10-20" })], pageInfo: { hasMore: false, nextCursor: null } });
    expect((await readSourceCatalog(f.client, new AbortController().signal)).map((entry) => entry.id)).toEqual(["a", "b"]);
    expect(f.list.mock.calls[1]?.[0]).toMatchObject({ filter: { includeArchived: true }, page: { cursor: "page2" } });
    f.list.mockResolvedValue({ entries: [], pageInfo: { hasMore: true, nextCursor: null } });
    await expect(readSourceCatalog(f.client, new AbortController().signal)).rejects.toThrow("no next cursor");
  });
  it("resolves archived/broken by exact ID through read APIs and preserves context-unavailable titles", async () => {
    const f = sourceClient(native(id, { title: "Readable human title", status: "error", archivedAt: "2026-10-20" }));
    expect(await readSource(f.client, id)).toMatchObject({ entry: { id, title: "Readable human title", status: "error", archivedAt: "2026-10-20" }, contextProblem: null });
    expect(f.ref).toHaveBeenCalledWith(id);
    expect(f.refetch).toHaveBeenCalledWith({ limit: 100, direction: "tail" });
    f.refetch.mockRejectedValue(new Error("timeline access denied"));
    expect(await readSource(f.client, id)).toMatchObject({ entry: { title: "Readable human title" }, contextProblem: "Context could not be read: timeline access denied" });
    expect(f.list).not.toHaveBeenCalled();
  });
  it("distinguishes genuine absence, read failure, wrong IDs and no assistant message", async () => {
    const f = sourceClient(null);
    expect(await readSource(f.client, id)).toBeNull();
    f.refresh.mockRejectedValueOnce(new Error("transport timeout"));
    await expect(readSource(f.client, id)).rejects.toThrow("transport timeout");
    f.refresh.mockResolvedValueOnce(native("another-id"));
    await expect(readSource(f.client, id)).rejects.toThrow("different session ID");
    f.refresh.mockResolvedValueOnce(native(id)); f.refetch.mockResolvedValue({ entries: [{ item: { type: "user_message", text: "not context" } }] });
    expect(await readSource(f.client, id)).toMatchObject({ contextProblem: expect.stringContaining("No assistant message") });
  });
});

const issue = (key: string, epic: boolean, type = epic ? "Epic" : "Task"): JiraIssue => ({
  id: `jira-${key}`, key, created: "2026-01-01T00:00:00.000Z", epic, type, summary: `Title ${key}`, status: "Open", statusCategory: "To Do", open: true, parent: null,
});
describe("search picker highlight", () => {
  it("clears a stale active option when the result set changes", () => {
    const row = (id: string) => ({ id, title: id, detail: "detail" });
    expect(reconcilePickerActive("ASMA-900", [row("ASMA-900"), row("ASMA-899")])).toBe("ASMA-900");
    expect(reconcilePickerActive("ASMA-900", [row("ASMA-899"), row("ASMA-897")])).toBeNull();
    expect(reconcilePickerActive(null, [row("ASMA-900")])).toBeNull();
  });
});

describe("Jira picker", () => {
  it("keeps backend rows intact except exact-key postfilter and maps only explicit type names to host glyphs", () => {
    const rows = Array.from({ length: 100 }, (_, i) => issue(`ASMA-${100 - i}`, i % 2 === 0));
    expect(jiraPickerRows(rows)).toHaveLength(100);
    expect(jiraPickerRows([issue("ASMA-10", true), issue("ASMA-100", true)], "asma-10").map((row) => row.key)).toEqual(["ASMA-10"]);
    expect(issueTypeIcon("Bug")).toBe("Bug"); expect(issueTypeIcon("Epic")).toBe("Zap");
    expect(issueTypeIcon("User Story")).toBe("BookOpen"); expect(issueTypeIcon("Task")).toBe("SquareCheck");
    expect(issueTypeIcon("Sub-task")).toBe("ListTree"); expect(issueTypeIcon("custom issue type")).toBe("Tag");
  });
  it("queries the backend for each debounced term, ignores stale replies, and retries errors without losing the binding", async () => {
    const first = deferred<{ issues: JiraIssue[]; jiraProblem: null }>();
    hooks.read.mockReturnValueOnce(first.promise).mockResolvedValueOnce({ issues: [issue("ASMA-999", true)], jiraProblem: null });
    const props = { colors, desks: [{ project: "DESK • ADAM", kind: "adam" }], scope: { role: "LSA" }, initialQuery: "", selected: "ASMA-1", onSelect: vi.fn(), onBack: vi.fn() } as unknown as Parameters<typeof JiraIssuePicker>[0];
    const draw = () => render(() => JiraIssuePicker(props));
    draw(); await vi.advanceTimersByTimeAsync(250);
    draw().props.onQuery("outside browse window"); draw(); await vi.advanceTimersByTimeAsync(250);
    expect(draw().props.rows.map((row: { id: string }) => row.id)).toEqual(["ASMA-999"]);
    first.resolve({ issues: [issue("ASMA-1", true)], jiraProblem: null }); await tick();
    expect(draw().props.rows.map((row: { id: string }) => row.id)).toEqual(["ASMA-999"]);
    expect(hooks.read).toHaveBeenLastCalledWith({ role: "LSA", start: "from-jira", issueKey: "outside browse window", issueClass: "eligible" });
    expect(draw().props.selected).toBe("ASMA-1");
    hooks.read.mockRejectedValueOnce(new Error("network")); draw().props.onQuery("retry"); draw(); await vi.advanceTimersByTimeAsync(250);
    expect(draw().props.error).toContain("network");
    hooks.read.mockResolvedValueOnce({ issues: [], jiraProblem: null }); draw().props.onRetry(); draw(); await vi.advanceTimersByTimeAsync(250);
    expect(draw().props.message).toContain("No open ASMA issue matches");
  });
});

describe("source picker continuations", () => {
  it("uses the selected host and resolves a pasted archived exact ID with Include archived off", async () => {
    const f = sourceClient(native(id, { title: "Archived context", archivedAt: "2026-10-20", status: "error" })); hooks.client = f.client;
    const props = { hostId: "host-a", colors, selected: null, onSelect: vi.fn(), onBack: vi.fn() };
    const draw = () => render(() => SourceSessionPicker(props));
    draw(); await tick(); draw().props.onQuery(id); draw(); await vi.advanceTimersByTimeAsync(250);
    expect(draw().props.rows[0]).toMatchObject({ title: "Archived context", detail: expect.stringContaining("Archived · error") });
    draw().props.onSelect(id); await tick();
    expect(props.onSelect).toHaveBeenCalledWith(expect.objectContaining({ hostId: "host-a", entry: expect.objectContaining({ id }) }));
    expect(new Set(hooks.clients)).toEqual(new Set(["host-a"]));
  });
  it("rejects a delayed selection after query change, host change or unmount", async () => {
    const f = sourceClient(), pending = deferred<unknown>(); hooks.client = f.client; f.refresh.mockReturnValue(pending.promise);
    const props = { hostId: "host-a", colors, selected: null, onSelect: vi.fn(), onBack: vi.fn() };
    const draw = () => render(() => SourceSessionPicker(props));
    draw(); await tick(); draw().props.onSelect(id); draw().props.onQuery("other"); props.hostId = "host-b"; draw(); hooks.dispose();
    pending.resolve(native(id)); await tick();
    expect(props.onSelect).not.toHaveBeenCalled();
  });
});
