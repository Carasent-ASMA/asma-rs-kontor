import { describe, expect, it, vi } from "vitest";

import { jiraSearchArgs, parseJiraSearchOutput, runJiraSearch } from "../server/session-create";
import { createScopeSchema } from "../shared/session-create";

function output(patch: Record<string, unknown> = {}) {
  return JSON.stringify({
    issues: [{
      id: "10001", key: "ASMA-100", summary: "Deliver the desk layout", created: "2026-10-01T10:00:00.000+0000",
      type: "Epic", epic: true, status: "In Development", statusCategory: "In Progress", open: true, parent: null,
    }],
    hasMore: true,
    nextPageToken: "page-2",
    ...patch,
  });
}

const command = async () => "/opt/asma";

describe("asma jira search server boundary", () => {
  it("asks for the honest 50 cap and passes the selected class", () => {
    expect(jiraSearchArgs({ query: "", issueClass: "eligible", openOnly: true }))
      .toEqual(["jira", "search", "--query", "", "--limit", "50", "--class", "eligible"]);
  });

  it("accepts eligible as a scope class and rejects an unknown class", () => {
    expect(createScopeSchema.parse({ issueClass: "eligible" })).toMatchObject({ issueClass: "eligible" });
    expect(() => createScopeSchema.parse({ issueClass: "all" })).toThrow();
  });

  it("keeps an exact query unchanged and omits --all unless asked", () => {
    expect(jiraSearchArgs({ query: "asma-123", issueClass: null, openOnly: true }))
      .toEqual(["jira", "search", "--query", "asma-123", "--limit", "50"]);
    expect(jiraSearchArgs({ query: "", issueClass: "epic", openOnly: false }))
      .toEqual(["jira", "search", "--query", "", "--limit", "50", "--class", "epic", "--all"]);
  });

  it("parses identity, created, type, hasMore and continuation without stripping", () => {
    const parsed = parseJiraSearchOutput(output());
    expect(parsed.issues[0]).toMatchObject({
      id: "10001", key: "ASMA-100", created: "2026-10-01T10:00:00.000+0000", type: "Epic",
    });
    expect(parsed.hasMore).toBe(true);
    expect(parsed.nextPageToken).toBe("page-2");
  });

  it("treats a missing continuation token as null but rejects unknown fields", () => {
    const parsed = parseJiraSearchOutput(output({ nextPageToken: undefined }));
    expect(parsed.nextPageToken).toBeNull();
    expect(() => parseJiraSearchOutput(output({ unexpected: true }))).toThrow();
  });

  it("keeps an empty page distinct from a later-page failure", async () => {
    const emptyExecute = vi.fn(async () => ({ stdout: JSON.stringify({ issues: [], hasMore: false }) }));
    const empty = await runJiraSearch({ query: "", issueClass: "eligible", openOnly: true }, emptyExecute, command);
    expect(empty).toEqual({ issues: [], hasMore: false, nextPageToken: null, failure: null });
    expect(emptyExecute).toHaveBeenCalledTimes(1);

    const failedExecute = vi.fn(async () => { throw new Error("Jira search failed (http 500: later page)"); });
    const failed = await runJiraSearch({ query: "", issueClass: "eligible", openOnly: true }, failedExecute, command);
    expect(failed.issues).toEqual([]);
    expect(failed.hasMore).toBe(false);
    expect(failed.nextPageToken).toBeNull();
    expect(failed.failure).toContain("later page");
    expect(failedExecute).toHaveBeenCalledTimes(1);
  });

  it("invokes the CLI once with --limit 50 and passes its page through", async () => {
    const execute = vi.fn(async (_file: string, _args: readonly string[], _options: { timeout: number; maxBuffer: number; encoding: "utf8" }) => ({ stdout: output({ nextPageToken: undefined }) }));
    const result = await runJiraSearch({ query: "", issueClass: "epic", openOnly: true }, execute, command);
    expect(result.issues.map((issue) => issue.key)).toEqual(["ASMA-100"]);
    expect(result.hasMore).toBe(true);
    expect(result.nextPageToken).toBeNull();
    expect(result.failure).toBeNull();
    expect(execute).toHaveBeenCalledTimes(1);
    expect(execute.mock.calls[0]?.[0]).toBe("/opt/asma");
    expect(execute.mock.calls[0]?.[2]).toMatchObject({ timeout: 15_000 });
    expect(execute.mock.calls[0]?.[1]).toEqual(["jira", "search", "--query", "", "--limit", "50", "--class", "epic"]);
  });

  it("reports a missing trusted CLI without spawning a bare command", async () => {
    const execute = vi.fn(async () => ({ stdout: output() }));
    const failed = await runJiraSearch({ query: "", issueClass: null, openOnly: true }, execute, async () => {
      throw new Error("The ASMA CLI could not be found at a trusted absolute path.");
    });
    expect(failed.failure).toContain("trusted absolute path");
    expect(execute).not.toHaveBeenCalled();
  });
});
