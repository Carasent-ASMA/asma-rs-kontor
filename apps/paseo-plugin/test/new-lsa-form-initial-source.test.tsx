import { isValidElement } from "react";
import { beforeEach, describe, expect, it, vi, type Mock } from "vitest";
import type { CreateOptions, CreateOutcome } from "../shared/session-create";
import type { SummaryRevision } from "../shared/session-navigator";

// Each test gets a fresh renderer, so one form's draft never carries into another test.
const harness = await vi.hoisted(async () => { const { deferredReact } = await import("./hermes-scoping"); const box = { current: deferredReact(), fresh: () => { box.current = deferredReact(); } }; return box; });
const host = vi.hoisted(() => ({ rpc: {} as Record<string, ReturnType<typeof vi.fn>>, settings: null as unknown, readSource: null as unknown as Mock<(...args: unknown[]) => Promise<unknown>> }));
vi.mock("react", async (original) => ({ ...(await original<typeof import("react")>()), ...Object.fromEntries((["useState", "useRef", "useMemo", "useCallback", "useEffect"] as const).map((name) => [name, (...args: unknown[]) => (harness.current.react[name] as (...values: unknown[]) => unknown)(...args)])) }));
vi.mock("react-native", () => ({ Platform: { OS: "web" }, useWindowDimensions: () => ({ width: 390, height: 844 }), ScrollView: "ScrollView", View: "View", Text: "Text", Pressable: "Pressable", Image: "Image" }));
vi.mock("@getpaseo/plugin/client", () => ({ useRpc: (contract: { name: string }) => host.rpc[contract.name], useSettings: () => host.settings, getPaseoClient: () => ({}) }));
vi.mock("@getpaseo/plugin/client/react-native", () => ({ TextInput: "TextInput", Icon: "Icon" }));
vi.mock("../client/mobile-filters", () => ({ focusControl: () => {} }));
vi.mock("../client/source-picker", async (original) => ({ ...await original<typeof import("../client/source-picker")>(), readSource: (...args: unknown[]) => host.readSource(...args) }));

import { NewLsaForm } from "../client/new-lsa-form";
import { DenseSessionRow } from "../client/dense-session-row";
import { DEFAULT_ORGANIZATION_SETTINGS } from "../shared/organization";
import { sessionEntry } from "./session-fixtures";

type Element = { type: unknown; props: Record<string, unknown> };
const walk = (node: unknown): Element[] => Array.isArray(node) ? node.flatMap(walk) : !isValidElement<Record<string, unknown>>(node) ? [] : [node, ...walk(node.props.children)];
const text = (tree: Element[]) => tree.filter((node) => node.type === "Text").map((node) => [node.props.children].flat(Infinity).join("")).join("\n");
const byTestId = (tree: Element[], id: string) => tree.find((node) => node.props.testID === id);
const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

const COLORS = { surface0: "#fff", surface1: "#eee", surface2: "#ddd", foreground: "#111", foregroundMuted: "#555", border: "#aaa", accent: "#00f", accentForeground: "#fff", statusSuccess: "#080", statusWarning: "#850", statusDanger: "#b00" } as never;
const OPTIONS: CreateOptions = {
  enabled: true, disabledReason: null, pending: null, notes: [], catalogProjectId: "prj-adam", issues: [], jiraProblem: null,
  desks: [{ projectId: "prj-adam", project: "DESK • ADAM", desk: "ADAM", kind: "adam", cwd: "/fixtures/adam", lead: { ADHOC: "wks-adhoc", LSA: "wks-lsa", TPM: "wks-tpm" } }],
  providers: [{ id: "codex", label: "Codex", status: "ready", available: true, note: null, persona: "supported", defaultModel: "gpt-6-astra", defaultMode: "auto",
    models: [{ id: "gpt-6-astra", label: "GPT-6 Astra", efforts: [], defaultEffort: null }], modes: [{ id: "auto", label: "Default permissions", dangerous: false }] }],
  profile: { name: "lsa", provider: "codex", model: "gpt-6-astra", modeId: "auto", effort: null },
};
const SOURCE = sessionEntry("agent-src", { title: "Route review" });
const SUMMARY: SummaryRevision = { summaryId: "00000000-0000-4000-8000-0000000000aa", revision: 3, sourceAgentId: "agent-src", sourceTitle: "Route review", cutoff: "2026-10-25T10:00:00Z",
  savedAt: "2026-10-25T10:05:00Z", sections: { task: "Fictional task", decisions: "", currentState: "", nextStep: "" }, text: "Fictional summary" };
const reply = async (input: { requestId: string }): Promise<CreateOutcome> => ({ requestId: input.requestId, state: "verified", title: "LSA • ADHOC • Route review", agentId: "agent-new",
  workspaceId: "wks-adhoc", message: "Fixture verified.", differences: [], persona: "supported", checkedAt: "2026-10-26" });

beforeEach(() => {
  harness.fresh();
  host.settings = { status: "ready", values: DEFAULT_ORGANIZATION_SETTINGS, revision: "rev-1", save: vi.fn(async () => true) };
  host.readSource = vi.fn(async (_client: unknown, id: unknown) => ({ entry: sessionEntry(String(id), { title: "Route review" }), contextProblem: null }));
  host.rpc = { "sessions.create": vi.fn(reply), "sessions.create-check": vi.fn(), "sessions.create-features": vi.fn(async () => ({ features: [], problem: null })) };
});

/** Opens New lead with a source already chosen, names the topic and presses Create. */
async function create(patch: Partial<Parameters<typeof NewLsaForm>[0]>) {
  const props = { hostId: "host-a", colors: COLORS, compact: true, options: { status: "ready", options: OPTIONS } as const, onRefresh: vi.fn(), canOpen: true,
    onOpenCreated: vi.fn(() => ({ ok: true as const })), onCancel: vi.fn(), ...patch };
  const render = () => walk(harness.current.render(() => NewLsaForm(props)));
  const opened = render();
  (byTestId(render(), "kontor-new-lsa-topic")!.props.onChangeText as (value: string) => void)("Route review");
  (byTestId(render(), "kontor-new-lsa-create")!.props.onPress as () => void)();
  await settle();
  return { opened, request: (host.rpc["sessions.create"]!.mock.calls[0]?.[0] ?? null) as Record<string, unknown> | null };
}

describe("New lead with an initial source", () => {
  it("opens with the source and its saved revision pinned, and sends the pin instead of the session", async () => {
    const { opened, request } = await create({ sources: [SOURCE], initialSource: { entry: SOURCE, summary: SUMMARY } });
    expect(opened.find((node) => node.type === DenseSessionRow && node.props.testID === "kontor-source-selected")!.props.entry).toBe(SOURCE);
    expect(text(opened)).toContain("Saved summary r3");
    expect(text(opened)).toContain("Source summary is attached before the first task.");
    expect(text(opened)).not.toContain("Choose source session");
    expect(host.readSource).not.toHaveBeenCalled();
    expect(request).toMatchObject({ sourceAgentId: null, sourceSummary: { summaryId: SUMMARY.summaryId, revision: 3 } });
  });

  it("seeds the draft's source session when no revision is pinned", async () => {
    const { opened, request } = await create({ initialSource: { entry: SOURCE, summary: null } });
    expect(byTestId(opened, "kontor-source-selected")!.props.children).toBe("Route review");
    expect(text(opened)).toContain("No saved summary: the lead copies this session's last assistant message before its first task.");
    expect(host.readSource).toHaveBeenCalledWith(expect.anything(), "agent-src");
    expect(request).toMatchObject({ sourceAgentId: "agent-src" });
    expect(request).not.toHaveProperty("sourceSummary");
  });
});
