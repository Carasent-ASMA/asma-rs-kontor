import { describe, expect, it } from "vitest";
import { epicScopeChoice, epicScopeChoices, epicScopeKey, matchesEpicScope } from "../client/session-scope";
import { sessionEntry } from "./session-fixtures";

describe("exact resolved epic scope", () => {
  const labelled = sessionEntry("labelled", { labels: { epic: "ASMA-10" } });
  const captured = { ...sessionEntry("captured", { labels: { "asma.task": "ASMA-11" } }), jira: { observed: ["2026-10-21"], epics: new Map([["ASMA-11", "ASMA-10"]]) } };
  const conflict = { ...captured, id: "conflict", labels: { ...captured.labels, epic: "ASMA-20" } };
  const titleOnly = sessionEntry("title-only", { title: "SWE • ASMA-10 • title is not membership" });
  const known = { kind: "known", key: "ASMA-10" } as const;
  const unknown = { kind: "unknown" } as const;

  it("uses the accepted resolver for labels, dated capture, conflict and unknown", () => {
    expect([labelled, captured, conflict, titleOnly].filter((entry) => matchesEpicScope(entry, known, {})).map((entry) => entry.id)).toEqual(["labelled", "captured"]);
    expect([labelled, captured, conflict, titleOnly].filter((entry) => matchesEpicScope(entry, unknown, {})).map((entry) => entry.id)).toEqual(["conflict", "title-only"]);
    expect(matchesEpicScope(labelled, { kind: "known", key: "ASMA-1" }, {})).toBe(false);
    expect(matchesEpicScope(conflict, null, {})).toBe(true);
  });

  it("applies session and workspace display mappings with the existing precedence", () => {
    const mappings = { "session:conflict": { epic: "ASMA-30" }, "workspace:wks_1": { epic: "ASMA-40" } };
    expect(matchesEpicScope(conflict, { kind: "known", key: "ASMA-30" }, mappings)).toBe(true);
    expect(matchesEpicScope(labelled, { kind: "known", key: "ASMA-40" }, mappings)).toBe(true);
    expect(matchesEpicScope(conflict, unknown, mappings)).toBe(false);
  });

  it("offers distinct canonical keys, including explicit Unknown, and round-trips selection", () => {
    const choices = epicScopeChoices([labelled, captured, conflict, titleOnly, sessionEntry("case", { labels: { epic: "asma-10" } })], {});
    expect(choices).toEqual([{ key: "known:ASMA-10", label: "ASMA-10" }, { key: "unknown", label: "No epic linked" }]);
    expect(epicScopeChoices([], {})).toEqual([{ key: "unknown", label: "No epic linked" }]);
    for (const scope of [null, known, unknown]) expect(epicScopeChoice(epicScopeKey(scope))).toEqual(scope);
  });
});
