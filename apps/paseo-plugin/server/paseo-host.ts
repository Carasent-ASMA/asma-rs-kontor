import type { KontorPluginConfig } from "./config";

/** One complete daemon schedule inventory from the public host connector; no schedule contents leave this module. */
export type HostSchedule = {
  target: { type: "agent"; agentId: string } | { type: "self"; agentId: string } | { type: "new-agent" };
  status: string;
};
export type CompleteScheduleRead = { complete: true; hostId: string; schedules: HostSchedule[] } | { complete: false; gap: string };
export type CompleteScheduleReader = () => Promise<CompleteScheduleRead>;

type HostClient = {
  connect(): Promise<void>;
  close(): Promise<void>;
  getLastServerInfoMessage?(): { serverId?: string } | null;
  scheduleList(): Promise<{ schedules?: unknown; error?: string | null }>;
};
type HostConnector = { createHostClient(connection: string, config: Record<string, unknown>): HostClient };
type LoadHostConnector = () => Promise<HostConnector | null>;
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

/** Only the supported public node connector; never a plugin-private object, credential store or arbitrary RPC proxy. */
const loadPublicHostConnector: LoadHostConnector = async () => {
  // A variable specifier keeps the plugin's 0.11.4 type graph from inventing the 0.13.2 subpath. If the installed
  // host runtime does not provide it, the reader returns the exact version/connector gap rather than a false clear.
  const specifier = "@getpaseo/client/node";
  try {
    const module = await import(specifier) as { createHostClient?: unknown };
    return typeof module.createHostClient === "function" ? module as HostConnector : null;
  } catch { return null; }
};

function exactSchedules(value: unknown): HostSchedule[] | null {
  if (!Array.isArray(value)) return null;
  const schedules: HostSchedule[] = [];
  for (const entry of value) {
    if (!entry || typeof entry !== "object" || Array.isArray(entry)) return null;
    const row = entry as Record<string, unknown>, target = row.target;
    if (!target || typeof target !== "object" || Array.isArray(target) || typeof row.status !== "string") return null;
    const type = (target as Record<string, unknown>).type, agentId = (target as Record<string, unknown>).agentId;
    if (type === "new-agent") schedules.push({ target: { type }, status: row.status });
    else if ((type === "agent" || type === "self") && typeof agentId === "string" && agentId.trim())
      schedules.push({ target: { type, agentId }, status: row.status });
    else return null;
  }
  return schedules;
}

/**
 * Qualifies the daemon's unfiltered schedule/list through `createHostClient()` on the server-selected paired host.
 * The returned inventory is complete only when the connector reports a stable daemon server ID and a parseable list.
 */
export function createCompleteScheduleReader(
  config: Pick<KontorPluginConfig, "paseoHost">,
  load: LoadHostConnector = loadPublicHostConnector,
): CompleteScheduleReader {
  return async () => {
    if (!config.paseoHost?.trim()) return { complete: false, gap: "No server-selected paired host is configured for a complete schedule inventory." };
    const connector = await load();
    if (!connector) return { complete: false, gap: "The installed @getpaseo/client/node does not export createHostClient. Complete schedule inventory requires the public node connector (0.13.2 or later); the plugin's pinned 0.11.4 client has no public node entry point." };
    let client: HostClient | null = null;
    try {
      client = connector.createHostClient(config.paseoHost, { clientId: "kontor-paseo-plugin", connectTimeoutMs: 10_000 });
      await client.connect();
      const before = client.getLastServerInfoMessage?.()?.serverId?.trim();
      if (!before) return { complete: false, gap: "The public host connector did not report the daemon server identity, so its schedule list is not host-qualified." };
      const page = await client.scheduleList();
      if (page?.error || !Array.isArray(page?.schedules)) return { complete: false, gap: "The public host connector schedule list was unreadable, so it is not a complete inventory." };
      const after = client.getLastServerInfoMessage?.()?.serverId?.trim();
      if (!after || after !== before) return { complete: false, gap: "The daemon server identity changed during the schedule read, so the inventory is not host-qualified." };
      const schedules = exactSchedules(page.schedules);
      return schedules ? { complete: true, hostId: before, schedules }
        : { complete: false, gap: "The public host connector returned an unrecognized schedule target or status, so the inventory is not complete." };
    } catch (error) {
      return { complete: false, gap: `The public host connector schedule read failed: ${errorText(error)}.` };
    } finally { await client?.close().catch(() => {}); }
  };
}
