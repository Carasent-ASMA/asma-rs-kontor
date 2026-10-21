import { execFileSync } from "node:child_process";
import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it, vi } from "vitest";
import { indexResponsibility, responsibilityExpired, rosterChoices, sessionResponsibility } from "../client/responsibility";
import { sessionEpic } from "../client/session-organization";
import { responsibilitySchema, type Responsibility } from "../shared/responsibility";
import { readResponsibilityProjection, responsibilityPath } from "../server/responsibility";
import { sessionEntry } from "./session-fixtures";

/** Synthetic realm, people and keys only. */
function projection(overrides: Partial<Responsibility> = {}): Responsibility {
  return responsibilitySchema.parse({
    schema: "kontor/responsibility@1",
    realm: "tracker.example",
    generatedAt: "2026-10-21T12:00:00Z",
    expiresAt: "2026-10-22T12:00:00Z",
    provenance: { collector: "collect-responsibility@1", selection: ["ASMA-100", "ASMA-200", "ASMA-300"], complete: true, workstreams: { path: "/operator/workstreams.json", sha256: "a".repeat(64) } },
    people: [
      { accountId: "account-a", displayName: "Alex Example", active: true, accountType: "atlassian" },
      { accountId: "account-b", displayName: "Blair Example", active: true, accountType: "atlassian" },
      { accountId: "account-c", displayName: "Alex Example", active: false, accountType: "atlassian" },
      { accountId: "account-d", displayName: "Alex Example", active: false, accountType: "atlassian" },
      { accountId: "account-svc", displayName: "Collector", active: true, accountType: "app" },
    ],
    issues: [
      { key: "ASMA-100", type: "Epic", level: 1, parent: null, assignee: "account-a", workstreams: ["alpha"] },
      { key: "ASMA-101", type: "Story", level: 0, parent: "ASMA-100", assignee: "account-b" },
      { key: "ASMA-102", type: "Story", level: 0, parent: "ASMA-100", assignee: null },
      { key: "ASMA-103", type: "Sub-task", level: -1, parent: "ASMA-101", assignee: "account-c" },
      { key: "ASMA-200", type: "Epic", level: 1, parent: null, workstreams: ["beta"] },
      { key: "ASMA-300", type: "Epic", level: 1, parent: null, assignee: null, workstreams: [] },
    ],
    workstreams: [{ key: "alpha", responsible: ["account-b"], participants: ["account-c"] }],
    ...overrides,
  });
}
const match = (labels: Record<string, string>, person: string, index = indexResponsibility(projection())) =>
  sessionResponsibility(sessionEntry("s", { labels }), index, person);

describe("responsibility projection schema", () => {
  it("keeps an absent assignee apart from an explicit null", () => {
    const parsed = projection();
    expect("assignee" in parsed.issues.find((issue) => issue.key === "ASMA-200")!).toBe(false);
    expect(parsed.issues.find((issue) => issue.key === "ASMA-102")!.assignee).toBeNull();
  });

  it.each([
    ["an assignee outside people", { issues: [{ key: "ASMA-1", type: "Story", level: 0, parent: null, assignee: "account-x" }] }],
    ["a duplicate issue", { issues: [{ key: "ASMA-1", type: "Story", level: 0, parent: null }, { key: "ASMA-1", type: "Story", level: 0, parent: null }] }],
    ["workstream facts with no declaration", { provenance: { collector: "c", selection: ["ASMA-1"], complete: true, workstreams: null } }],
    ["a workstream naming someone outside people", { workstreams: [{ key: "alpha", responsible: ["account-x"], participants: [] }] }],
    ["an expiry before the observation", { expiresAt: "2026-10-20T12:00:00Z" }],
    ["an unknown field", { owner: "account-a" }],
    ["a declared workstream that names nobody", { workstreams: [{ key: "alpha", responsible: [], participants: [] }] }],
    ["a declaration with no workstream", { workstreams: [] }],
  ])("rejects %s", (_name, overrides) => {
    expect(() => projection(overrides as Partial<Responsibility>)).toThrow();
  });
});

describe("one person's relation to one session", () => {
  it("matches the epic assignee, the workstream responsible and a participant, each with its reason", () => {
    expect(match({ epic: "ASMA-100" }, "account-a")).toEqual({ status: "match", reasons: ["epic-assignee"], unknown: [] });
    // A supervisor matches through the workstream although the epic is assigned to someone else.
    expect(match({ epic: "ASMA-100" }, "account-b")).toEqual({ status: "match", reasons: ["workstream-responsible"], unknown: [] });
    expect(match({ epic: "ASMA-100" }, "account-c")).toEqual({ status: "match", reasons: ["workstream-participant"], unknown: [] });
    expect(match({ epic: "ASMA-100" }, "account-svc")).toEqual({ status: "other" });
  });

  it("matches an exact task assignee only on that task's sessions, reading the epic from the task's parents", () => {
    const undeclared = indexResponsibility(projection({ provenance: { collector: "c", selection: ["ASMA-100"], complete: true, workstreams: null }, workstreams: [] }));
    expect(match({ "asma.task": "ASMA-101" }, "account-b", undeclared)).toEqual({ status: "match", reasons: ["task-assignee"], unknown: ["workstream responsibility not declared"] });
    expect(match({ "asma.task": "ASMA-102" }, "account-b", undeclared)).toEqual({ status: "unknown", unknown: ["workstream responsibility not declared"] });
    expect(match({ "jira.issue": "ASMA-103" }, "account-c", undeclared)).toMatchObject({ status: "match", reasons: ["task-assignee"] });
    // The epic assignee includes the epic's task sessions.
    expect(match({ "asma.task": "ASMA-103" }, "account-a", undeclared)).toMatchObject({ status: "match", reasons: ["epic-assignee"] });
  });

  it("normalizes epic and task claims the way the resolver does, so case is never a conflict", () => {
    expect(match({ epic: "asma-100", task: " asma-101 " }, "account-b")).toEqual({ status: "match", reasons: ["task-assignee", "workstream-responsible"], unknown: [] });
  });

  it("refuses disagreeing epic sources; the resolver's conflict wording is pinned here", () => {
    const disputed = sessionEntry("d", { labels: { "jira.epic": "ASMA-8399", "origin.epic": "ASMA-8371" } });
    expect(sessionEpic(disputed, undefined).note.startsWith("sources disagree")).toBe(true);
    expect(sessionResponsibility(disputed, indexResponsibility(projection()), "account-a").status).toBe("unknown");
    // A label and the projection's parent of an exact task disagree.
    expect(match({ epic: "ASMA-200", "asma.task": "ASMA-101" }, "account-b")).toEqual({ status: "unknown", unknown: ["epic sources disagree: ASMA-200 / ASMA-100"] });
  });

  it("keeps uncovered relations unknown, never someone else's", () => {
    expect(match({ epic: "ASMA-200" }, "account-a")).toEqual({ status: "unknown", unknown: ["ASMA-200 assignee not read", "workstream beta not declared"] });
    expect(match({ epic: "ASMA-999" }, "account-a")).toEqual({ status: "unknown", unknown: ["ASMA-999 not in this projection"] });
    expect(match({}, "account-a")).toEqual({ status: "unknown", unknown: ["no epic or task label"] });
    expect(match({ "asma.task": "ASMA-404" }, "account-a").status).toBe("unknown");
    expect(match({ epic: "ASMA-100" }, "account-x")).toEqual({ status: "unknown", unknown: ["person not in this projection"] });
    // An explicit null assignee and an explicitly empty workstream list are read facts.
    expect(match({ epic: "ASMA-300" }, "account-a")).toEqual({ status: "other" });
  });

  it("uses the session's own workstream label before its epic's, and never titles, owners or names", () => {
    expect(match({ epic: "ASMA-300", workstream: "Alpha" }, "account-b")).toMatchObject({ status: "match", reasons: ["workstream-responsible"] });
    const named = sessionEntry("n", { title: "Alex Example • ASMA-100", labels: { owner: "account-a", creator: "account-a" } });
    expect(sessionResponsibility(named, indexResponsibility(projection()), "account-a").status).toBe("unknown");
  });

  it("honors the producer's expiry", () => {
    const index = indexResponsibility(projection());
    expect(responsibilityExpired(index, Date.parse("2026-10-22T11:59:59Z"))).toBe(false);
    expect(responsibilityExpired(index, Date.parse("2026-10-22T12:00:00Z"))).toBe(true);
  });

  it("tells same-named people apart without showing account IDs", () => {
    const labels = rosterChoices(indexResponsibility(projection())).map(({ label }) => label);
    expect(labels).toEqual(["Alex Example", "Alex Example (inactive) · 1 of 2", "Alex Example (inactive) · 2 of 2", "Blair Example", "Collector (app)"]);
    expect(labels.join(" ")).not.toContain("account-");
  });

  it("keeps every label unique when a real name reads like a numbered one", () => {
    const people = [
      { accountId: "account-c", displayName: "Alex", active: false, accountType: "atlassian" },
      { accountId: "account-d", displayName: "Alex", active: false, accountType: "atlassian" },
      { accountId: "account-e", displayName: "Alex (inactive) · 1 of 2", active: true, accountType: "atlassian" },
      { accountId: "account-f", displayName: "Alex (inactive) · 1 of 2 · 1 of 1", active: true, accountType: "atlassian" },
    ];
    const roster = rosterChoices(indexResponsibility(projection({ people, issues: [], workstreams: [], provenance: { collector: "c", selection: ["ASMA-100"], complete: true, workstreams: null } })));
    expect(roster.map(({ label }) => label)).toEqual(["Alex (inactive) · 1 of 2", "Alex (inactive) · 2 of 2",
      "Alex (inactive) · 1 of 2 · 1 of 1", "Alex (inactive) · 1 of 2 · 1 of 1 · 1 of 1"]);
    expect(new Set(roster.map(({ label }) => label)).size).toBe(people.length);
  });

  it("never reads an unknown relation as someone else's through an undeclared workstream", () => {
    // A declaration names only decided workstreams; beta is undeclared, and the epic has no assignee.
    expect(match({ epic: "ASMA-200" }, "account-b")).toEqual({ status: "unknown", unknown: ["ASMA-200 assignee not read", "workstream beta not declared"] });
    const unassigned = indexResponsibility(projection({ issues: [{ key: "ASMA-200", type: "Epic", level: 1, parent: null, assignee: null, workstreams: ["beta"] }] }));
    expect(match({ epic: "ASMA-200" }, "account-b", unassigned)).toEqual({ status: "unknown", unknown: ["workstream beta not declared"] });
  });
});

describe("operator-owned projection reader", () => {
  let root: string | undefined;
  afterEach(async () => { vi.unstubAllEnvs(); if (root) await rm(root, { recursive: true, force: true }); root = undefined; });
  async function home() {
    root = await mkdtemp(join(tmpdir(), "kontor-responsibility-"));
    vi.stubEnv("PASEO_HOME", root);
    await mkdir(join(root, "plugin-config/kontor"), { recursive: true });
    return responsibilityPath();
  }

  it("reads the fixed path beside the registry, with a content revision", async () => {
    const file = await home();
    expect(file).toBe(join(root!, "plugin-config/kontor/responsibility.json"));
    await writeFile(file, JSON.stringify(projection()));
    const read = await readResponsibilityProjection();
    expect(read).toMatchObject({ status: "ready", diagnostics: [] });
    expect(read.revision).toMatch(/^[a-f0-9]{64}$/);
    expect(read.projection?.people).toHaveLength(5);
  });

  it("reports an absent projection as unavailable, not as no work", async () => {
    await home();
    expect(await readResponsibilityProjection()).toMatchObject({ status: "unavailable", projection: null, diagnostics: ["No responsibility projection is installed on this host."] });
  });

  it.each(["broken", "[]", JSON.stringify({ ...projection(), schema: "kontor/responsibility@2" }), '{"schema":"kontor/responsibility@1","schema":"kontor/responsibility@1"}'])("reports an invalid projection: %s", async (raw) => {
    await writeFile(await home(), raw);
    const read = await readResponsibilityProjection();
    expect(read).toMatchObject({ status: "invalid", projection: null, revision: null });
    expect(read.diagnostics[0]).toMatch(/^Responsibility projection invalid/);
  });
});

describe("collector contract", () => {
  it("emits what the reader accepts, keeping absent and null assignees apart", () => {
    const output = execFileSync("python3", [join(__dirname, "../scripts/collect-responsibility.py"), "--self-check"], { encoding: "utf8" });
    const parsed = responsibilitySchema.parse(JSON.parse(output));
    expect(parsed.issues.map((issue) => ["assignee" in issue ? issue.assignee : "not read", issue.workstreams ?? "not read"])).toEqual([
      ["account-a", ["alpha"]], [null, []], ["not read", "not read"]]);
  });
});
