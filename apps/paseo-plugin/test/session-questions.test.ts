import { createHash } from "node:crypto";
import { describe, expect, it } from "vitest";
import {
  acceptQuestionsInputSchema, questionHashInput, questionRequestHash, recommendPermission, stableJson,
  type PermissionRequestLike,
} from "../shared/session-questions";

const sha256 = (value: string) => createHash("sha256").update(value).digest("hex");
const hex = (value: string) => sha256(value);

const question = (options: readonly string[], patch: Record<string, unknown> = {}): PermissionRequestLike => ({
  kind: "question",
  name: "AskUserQuestion",
  title: "Choose one",
  input: {
    questions: [{ header: "Release", question: "Which way?", options: options.map((label) => ({ label })), ...patch }],
  },
});

const tool = (actions: PermissionRequestLike["actions"]): PermissionRequestLike => ({
  kind: "tool", name: "bash", title: "Run command", input: { command: "npm test" }, actions,
});

describe("permission recommendation", () => {
  it("chooses the recommended question option", () => {
    const result = recommendPermission(question(["Wait", "Ship it (Recommended)"]));
    expect(result).toEqual({
      supported: true,
      response: { behavior: "allow", updatedInput: { questions: [{ header: "Release", question: "Which way?", options: [{ label: "Wait" }, { label: "Ship it (Recommended)" }] }], answers: { Release: "Ship it (Recommended)" } } },
      label: "Release: Ship it (Recommended)",
      reason: null,
    });
  });

  it("falls back to the first option when nothing says recommended", () => {
    expect(recommendPermission(question(["Alpha", "Beta"]))).toMatchObject({
      supported: true,
      response: { behavior: "allow", updatedInput: { answers: { Release: "Alpha" } } },
      label: "Release: Alpha",
    });
  });

  it("joins every recommended option on a multi-select and only the first on a single select", () => {
    expect(recommendPermission(question(["Red", "Blue (recommended)", "Green (Recommended)"], { multiSelect: true }))).toMatchObject({
      supported: true,
      response: { behavior: "allow", updatedInput: { answers: { Release: "Blue (recommended), Green (Recommended)" } } },
      label: "Release: Blue (recommended), Green (Recommended)",
    });
    expect(recommendPermission(question(["Red (Recommended)", "Blue (recommended)"]))).toMatchObject({
      supported: true,
      response: { behavior: "allow", updatedInput: { answers: { Release: "Red (Recommended)" } } },
    });
  });

  it("never answers a question that requires a written answer", () => {
    const result = recommendPermission(question([]));
    expect(result).toMatchObject({ supported: false, response: null, label: "Needs the operator" });
    expect(result.reason).toContain("written answer is required");
  });

  it("never answers malformed or empty question content", () => {
    expect(recommendPermission({ kind: "question", name: "AskUserQuestion", input: { questions: "nope" } })).toMatchObject({
      supported: false, response: null, reason: expect.stringContaining("could not be read"),
    });
    expect(recommendPermission({ kind: "question", name: "AskUserQuestion", input: { questions: [] } })).toMatchObject({
      supported: false, response: null, reason: expect.stringContaining("no questions"),
    });
    expect(recommendPermission({ kind: "question", name: "AskUserQuestion" })).toMatchObject({ supported: false, response: null });
  });

  it("refuses a repeated question header because one answer cannot name both", () => {
    const request: PermissionRequestLike = {
      kind: "question", name: "AskUserQuestion",
      input: { questions: [
        { header: "Release", question: "One", options: [{ label: "Yes" }] },
        { header: "Release", question: "Two", options: [{ label: "No" }] },
      ] },
    };
    expect(recommendPermission(request)).toMatchObject({ supported: false, response: null, reason: expect.stringContaining("share this header") });
  });

  it("chooses the first primary allow action", () => {
    const result = recommendPermission(tool([
      { id: "allow-secondary", label: "Allow once", behavior: "allow", variant: "secondary" },
      { id: "allow-primary", label: "Always allow", behavior: "allow", variant: "primary" },
      { id: "deny", label: "Deny", behavior: "deny", variant: "danger" },
    ]));
    expect(result).toEqual({
      supported: true,
      response: { behavior: "allow", selectedActionId: "allow-primary" },
      label: "Always allow · bash",
      reason: null,
    });
  });

  it("falls back to a non-danger allow action", () => {
    expect(recommendPermission(tool([
      { id: "danger-allow", label: "Allow everything", behavior: "allow", variant: "danger" },
      { id: "safe-allow", label: "Allow once", behavior: "allow", variant: "secondary" },
    ]))).toEqual({
      supported: true,
      response: { behavior: "allow", selectedActionId: "safe-allow" },
      label: "Allow once · bash",
      reason: null,
    });
  });

  it("never chooses a dangerous allow or a deny action", () => {
    expect(recommendPermission(tool([{ id: "danger", label: "Allow all", behavior: "allow", variant: "danger" }]))).toMatchObject({
      supported: false, response: null, reason: expect.stringContaining("Only a dangerous allow action"),
    });
    expect(recommendPermission(tool([{ id: "deny", label: "Deny", behavior: "deny", variant: "danger" }]))).toMatchObject({
      supported: false, response: null, reason: expect.stringContaining("No allow action"),
    });
  });

  it("allows the host's plain allow when no actions are offered", () => {
    expect(recommendPermission(tool(undefined))).toEqual({
      supported: true,
      response: { behavior: "allow" },
      label: "Allow · bash",
      reason: null,
    });
  });
});

describe("question content hash", () => {
  it("matches SHA-256 over the stable pin, including multi-block and non-ASCII content", () => {
    const requests: PermissionRequestLike[] = [
      question(["Alpha", "Beta (Recommended)"]),
      { kind: "plan", name: "ExitPlanMode", title: "Ready to implement?", input: { plan: "£".repeat(80), note: "Résumé — 日本語" } },
      { kind: "tool", name: "hämtning", title: "Run", input: { nested: { z: 1, a: [true, null, "x"] } }, actions: [] },
    ];
    for (const request of requests) {
      expect(questionRequestHash(request)).toBe(sha256(questionHashInput(request)));
      expect(questionRequestHash(request)).toMatch(/^[a-f0-9]{64}$/);
    }
    expect(questionRequestHash({ kind: "tool", name: "bash" })).toBe(sha256(questionHashInput({ kind: "tool", name: "bash" })));
    expect(hex("abc")).toBe("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
  });

  it("pins content independent of object key order and changes when content changes", () => {
    const left: PermissionRequestLike = { kind: "tool", name: "bash", title: "Run", input: { b: 2, a: { d: 4, c: 3 } } };
    const right: PermissionRequestLike = { kind: "tool", name: "bash", title: "Run", input: { a: { c: 3, d: 4 }, b: 2 } };
    expect(questionHashInput(left)).toBe(questionHashInput(right));
    expect(questionRequestHash(left)).toBe(questionRequestHash(right));
    expect(questionRequestHash({ ...left, title: "Run again" })).not.toBe(questionRequestHash(left));
    expect(stableJson({ b: 1, a: { d: 2, c: [3, { f: 6, e: 5 }] } })).toBe('{"a":{"c":[3,{"e":5,"f":6}],"d":2},"b":1}');
  });

  it("keeps the accept input to the review's three fields", () => {
    const item = { agentId: "a", requestId: "r", hash: "a".repeat(64) };
    expect(acceptQuestionsInputSchema.safeParse({ items: [item] }).success).toBe(true);
    expect(acceptQuestionsInputSchema.safeParse({ items: [{ ...item, response: { behavior: "allow" } }] }).success).toBe(false);
    expect(acceptQuestionsInputSchema.safeParse({ items: [{ ...item, hash: "not-a-digest" }] }).success).toBe(false);
  });
});
