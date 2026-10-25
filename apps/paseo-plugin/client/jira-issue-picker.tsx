import { useEffect, useState } from "react";
import { View } from "react-native";
import { useRpc } from "@getpaseo/plugin/client";
import { issueDesk, readCreateOptions, type CreateOptions, type CreateScope, type JiraIssue } from "../shared/session-create";
import { Action, type Colors } from "./page-controls";
import { SearchPicker } from "./search-picker";

export type IssueFilter = "all" | "epic" | "non-epic";
/** Uses actual type metadata only. These host glyphs are not Jira's configurable icon images. */
export function issueTypeIcon(type: string): string {
  return ({ epic: "Zap", bug: "Bug", story: "BookOpen", "user story": "BookOpen", task: "SquareCheck", "sub-task": "ListTree", subtask: "ListTree" } as Record<string, string>)[type.trim().toLocaleLowerCase()] ?? "Tag";
}
export function jiraPickerRows(issues: readonly JiraIssue[], filter: IssueFilter, query = ""): JiraIssue[] {
  // Backend ordering/paging and type-before-limit remain a backend obligation; never sort by key as a substitute for createdAt.
  const key = query.trim().toUpperCase(), exact = /^ASMA-[1-9]\d*$/.test(key);
  return issues.filter((issue) => (!exact || issue.key === key) && (filter === "all" || issue.epic === (filter === "epic"))).slice(0, 50);
}

export function JiraIssuePicker({ colors, scope, desks, initialQuery, selected, onSelect, onBack }: {
  colors: Colors; scope: CreateScope; desks: CreateOptions["desks"]; initialQuery: string; selected: string | null;
  onSelect: (issue: JiraIssue, query: string) => void; onBack: () => void;
}) {
  const read = useRpc(readCreateOptions);
  const [query, setQuery] = useState(initialQuery), [filter, setFilter] = useState<IssueFilter>("all"), [revision, setRevision] = useState(0);
  const [result, setResult] = useState<{ query: string; issues: JiraIssue[]; error: string | null } | null>(null);
  const [busy, setBusy] = useState(true);
  useEffect(() => {
    let live = true;
    setBusy(true);
    const timer = setTimeout(() => {
      read({ ...scope, start: "from-jira", issueKey: query.trim() }).then(
        (options) => { if (live) { setResult({ query, issues: options.issues, error: options.jiraProblem }); setBusy(false); } },
        (error: unknown) => { if (live) { setResult({ query, issues: [], error: `Jira could not be read: ${error instanceof Error ? error.message : String(error)}` }); setBusy(false); } });
    }, 250);
    return () => { live = false; clearTimeout(timer); };
  }, [query, revision, scope.projectId, scope.role]);
  const current = result?.query === query ? result : null;
  const issues = jiraPickerRows(current?.issues ?? [], filter, query);
  const pick = (key: string) => {
    const issue = current?.issues.find((entry) => entry.key === key);
    if (issue && !busy && !issueDesk(desks, issue).problem) onSelect(issue, query);
  };
  return <SearchPicker name="kontor-jira-picker" label="Choose a Jira issue" placeholder="ASMA key or title words" query={query}
    onQuery={(value) => { setBusy(true); setQuery(value); }} onSearch={() => { setBusy(true); setRevision((value) => value + 1); }}
    selected={selected} busy={busy || !current} error={current?.error}
    message={issues.length ? `${issues.length} results. Choose an issue; its type decides the desk.` : "No open ASMA issue matches."}
    filters={<View role="group" accessibilityLabel="Issue type" style={{ flexDirection: "row", gap: 6 }}>
      {([ ["all", "All"], ["epic", "Epics"], ["non-epic", "Tickets"] ] as const).map(([id, label]) => <Action key={id} label={label} colors={colors} selected={filter === id} onPress={() => setFilter(id)} />)}
    </View>}
    rows={issues.map((issue) => ({ id: issue.key, title: `${issue.key} · ${issue.summary}`, detail: `${issue.type} · ${issue.status}`, icon: issueTypeIcon(issue.type),
      problem: issueDesk(desks, issue).problem,
      action: issue.parent?.open ? <Action label={`Start from ${issue.parent.key}`} colors={colors} onPress={() => { setFilter("all"); setBusy(true); setQuery(issue.parent!.key); }} /> : null }))}
    onSelect={pick} onRetry={() => { setBusy(true); setRevision((value) => value + 1); }} onBack={onBack} colors={colors} />;
}
