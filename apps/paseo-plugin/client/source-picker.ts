import type { PaseoApi } from "@getpaseo/client";
import { collectDirectory, toDirectoryEntry, type DirectoryEntry } from "./session-directory";

export type SourceSession = Pick<DirectoryEntry, "id" | "title" | "status" | "archivedAt" | "createdAt">;
export type SourceRead = { entry: SourceSession; contextProblem: string | null };
export type SourceChoice = SourceRead & { hostId: string };

export const sourceState = (entry: SourceSession) => `${entry.archivedAt ? "Archived · " : ""}${entry.status}`;
export const isSourceId = (query: string) => /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i.test(query.trim());

/** Scope and match precede the browse limit; a title search covers every collected page. */
export function sourceRows(entries: readonly SourceSession[], query: string, includeArchived: boolean): SourceSession[] {
  const tokens = query.trim().toLocaleLowerCase().split(/\s+/).filter(Boolean);
  const rows = entries.filter((entry) => (includeArchived || !entry.archivedAt)
    && tokens.every((token) => `${entry.title}\n${entry.id}`.toLocaleLowerCase().includes(token)))
    .sort((a, b) => (Date.parse(b.createdAt) || 0) - (Date.parse(a.createdAt) || 0) || a.id.localeCompare(b.id));
  return tokens.length ? rows : rows.slice(0, 10);
}

export async function readSourceCatalog(client: PaseoApi, signal: AbortSignal): Promise<SourceSession[]> {
  return (await collectDirectory(client, true, signal)).entries;
}

/** Supported reads only. Null means an explicit absent record, never a transport failure. */
export async function readSource(client: PaseoApi, id: string): Promise<SourceRead | null> {
  const handle = client.agents.ref(id), found = await handle.refresh();
  if (!found) return null;
  if (found.agent.id !== id) throw new Error("The host returned a different session ID. Retry this exact ID.");
  const entry = toDirectoryEntry(found);
  try {
    const page = await handle.timeline.refetch({ limit: 100, direction: "tail" });
    const hasMessage = page.entries.some(({ item }) => item.type === "assistant_message" && item.text.trim());
    return { entry, contextProblem: hasMessage ? null : "No assistant message is available in the last 100 timeline entries." };
  } catch (error) {
    return { entry, contextProblem: `Context could not be read: ${error instanceof Error ? error.message : String(error)}` };
  }
}
