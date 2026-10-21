import { isMap, parseDocument } from "yaml";

export const GRAPH_FENCE = /^ {0,3}(?:`{3,}|~{3,})plan-graph\s*$/m;
export function planMetadata(content: string) {
  const result = { workstream: null as string | null, epic: null as string | null, diagnostics: [] as string[] };
  const opening = /^\uFEFF?---[ \t]*\r?\n/.exec(content);
  if (!opening) return result;
  const rest = content.slice(opening[0].length);
  const closing = /^(?:---|\.\.\.)[ \t]*\r?$/m.exec(rest);
  if (!closing) { result.diagnostics.push("Unterminated frontmatter; plan metadata remains unknown."); return result; }
  const header = parseDocument(rest.slice(0, closing.index));
  if (header.errors.length || header.warnings.length || !isMap(header.contents)) {
    result.diagnostics.push("Unsupported or invalid frontmatter; plan metadata remains unknown."); return result;
  }
  // Explicit scalars only: aliases, merges, titles and nested fields never establish bindings.
  const value = header.get("workstream");
  if (value !== undefined && value !== null) {
    if (typeof value === "string" && /^[a-z][a-z0-9]*(?:-[a-z0-9]+)*$/.test(value)) result.workstream = value;
    else result.diagnostics.push("Unsupported workstream declaration; plan workstream remains unknown.");
  }
  const epics = ["epic", "jira_epic", "jira"].map((key) => header.get(key)).filter((value) => value !== undefined && value !== null);
  if (epics.length && epics.every((value) => typeof value === "string" && /^ASMA-[1-9]\d*$/.test(value)) && new Set(epics).size === 1) result.epic = epics[0] as string;
  else if (epics.length) result.diagnostics.push("Invalid or conflicting frontmatter epic bindings; epic remains unknown.");
  return result;
}
