import type { DirectoryEntry } from "./session-directory";
import type { ReadSourceSummary } from "../shared/read-sources";
export type PlanLink = { kind: "session"; id: string } | { kind: "source"; id: string } | { kind: "url"; url: string } | { kind: "blocked"; reason: string };
export function resolvePlanLink(target: string, current: ReadSourceSummary, sources: readonly ReadSourceSummary[], entries: readonly DirectoryEntry[]): PlanLink {
  const blocked: PlanLink = { kind: "blocked", reason: "This link is not a configured plan, observed native session or safe HTTPS URL." };
  if (/[\u0000-\u0020\u007f\\]/.test(target)) return blocked;
  try {
    // RN 0.81's built-in URL does not implement file-base/paseo path semantics.
    // Recognize only the explicit read-only targets needed here; no URL polyfill.
    const session = target.match(/^paseo:\/\/agents\/([^/?#]+)$/i);
    if (session) {
      const id = decodeURIComponent(session[1]!);
      return entries.some((entry) => entry.id === id) ? { kind: "session", id } : { kind: "blocked", reason: "This exact native session is not in the current host directory." };
    }
    const https = target.match(/^https:\/\/([^/?#]+)(?:[/?#].*)?$/i);
    if (https) {
      const authority = https[1]!;
      if (!/^(?:[a-z0-9](?:[a-z0-9.-]*[a-z0-9])?|\[[a-f0-9:]+\])(?::\d{1,5})?$/i.test(authority)) return blocked;
      const port = authority.match(/:(\d+)$/)?.[1];
      if (port && (Number(port) < 1 || Number(port) > 65535)) return blocked;
      return { kind: "url", url: target };
    }
    if (/^[a-z][a-z0-9+.-]*:/i.test(target) && !target.startsWith("file:///")) return blocked;
    const path = decodeURIComponent(target.replace(/^file:\/\//, "").split("#")[0]!);
    if (path.startsWith("//") || /[?\u0000-\u001f\u007f\\]/.test(path)) return blocked;
    const absolute = path.startsWith("/") ? path : current.path.slice(0, current.path.lastIndexOf("/") + 1) + path;
    const parts: string[] = [];
    for (const part of absolute.split("/")) { if (part === "..") parts.pop(); else if (part && part !== ".") parts.push(part); }
    const matches = sources.filter((source) => source.kind === "plan" && source.path === `/${parts.join("/")}`);
    if (matches.length > 1) return { kind: "blocked", reason: "This path identifies several configured plans. Choose an exact source in the context picker." };
    return matches.length === 1 ? { kind: "source", id: matches[0]!.id } : blocked;
  } catch { return blocked; }
  return blocked;
}
