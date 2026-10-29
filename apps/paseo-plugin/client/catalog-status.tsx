import { Text, View } from "react-native";
import type { WorkCatalogRead, WorkCatalogSource } from "../shared/work-catalog";
import { Copy, type Colors } from "./page-controls";

const count = (value: number, noun: string) => `${value} ${noun}${value === 1 ? "" : "s"}`;

function stateWord(source: WorkCatalogSource): string {
  switch (source.state) {
    case "not_configured": return "not configured";
    case "never_collected": return "never collected";
    case "complete": return "complete";
    case "incomplete": return "incomplete";
    case "failed": return "failed";
  }
}

/**
 * One source's freshness, coverage, retry deadline, conflicts and pending
 * verification. `asOf` is the generation's completion instant; the plugin's own
 * check time is never substituted for it.
 */
function sourceLine(source: WorkCatalogSource): string {
  const parts = [source.state === "not_configured" ? "not configured" : `${stateWord(source)} · generation ${source.generation}`];
  if (source.asOf !== null) parts.push(`as of ${source.asOf}`);
  else if (source.state !== "not_configured") parts.push("no complete observation");
  if (source.attemptedGeneration !== source.generation) parts.push(`attempt ${source.attemptedGeneration} not complete`);
  const collection = source.coverage.find((line) => ["collection", "scope", "archive coverage"].includes(line.subject));
  if (collection) parts.push(collection.detail);
  const schedule = source.coverage.find((line) => line.subject === "schedule");
  if (schedule) parts.push(`${schedule.state.replaceAll("_", " ")} (${schedule.detail})`);
  if (source.retryAt !== null) parts.push(`retry after ${source.retryAt}`);
  if (source.conflicts.length) parts.push(count(source.conflicts.length, "conflict"));
  if (source.pendingVerification) parts.push(`${count(source.pendingVerification, "association")} pending verification`);
  return parts.join(" · ");
}

function SourceBlock({ source, colors }: { source: WorkCatalogSource; colors: Colors }) {
  const conflicts = source.conflicts.slice(0, 3);
  return <View testID={`work-catalog-source-${source.source}`} style={{ gap: 2 }}>
    <Copy colors={colors} small>{`${source.source === "plan" ? "Plans" : source.source === "jira" ? "Jira" : "Native Paseo"} · ${sourceLine(source)}`}</Copy>
    {source.error !== null ? <Copy colors={colors} small>{`Error: ${source.error}`}</Copy> : null}
    {conflicts.map((conflict, index) => <Copy key={index} colors={colors} muted small>{`Conflict (${conflict.kind}${conflict.subject ? `, ${conflict.subject}` : ""}): ${conflict.message}`}</Copy>)}
    {source.conflicts.length > conflicts.length ? <Copy colors={colors} muted small>{`${source.conflicts.length - conflicts.length} more conflicts in this source.`}</Copy> : null}
  </View>;
}

/**
 * The shared-catalog line inside Sources and coverage. It names the mode, the
 * per-source freshness and coverage, and every visible conflict or pending
 * verification. A snapshot is labelled as last-known and keeps the source's own
 * as-of instant; a request/check time is never presented as source freshness.
 */
export function WorkCatalogStatus({ state, colors }: { state: WorkCatalogRead | null; colors: Colors }) {
  if (state === null) {
    return <View testID="work-catalog-status" style={{ gap: 2 }}>
      <Text style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>Shared catalog: not read on this screen yet. Direct source readers remain active.</Text>
    </View>;
  }
  if (state.view === null) {
    return <View testID="work-catalog-status" style={{ gap: 2 }}>
      <Text style={{ color: colors.foreground, fontSize: 12, lineHeight: 18, fontWeight: "600" }}>Shared catalog unavailable. Direct source readers remain active.</Text>
      {state.error !== null ? <Text style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>{state.error}</Text> : null}
    </View>;
  }
  const snapshot = state.mode === "snapshot";
  const freshness = state.sourceAsOf === null ? "no complete source observation" : `oldest source as of ${state.sourceAsOf}`;
  return <View testID="work-catalog-status" style={{ gap: 4 }}>
    <Text style={{ color: colors.foreground, fontSize: 12, lineHeight: 18, fontWeight: "600" }}>
      {snapshot ? "Shared catalog: last-known snapshot. Direct source readers remain active." : "Shared catalog: live through the observer credential."}
    </Text>
    <Text style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18 }}>
      {snapshot && state.snapshot !== null
        ? `Snapshot saved ${state.snapshot.savedAt}; ${freshness}.`
        : freshness.charAt(0).toUpperCase() + freshness.slice(1) + "."}
      {state.error !== null ? ` ${state.error}` : ""}
    </Text>
    {state.view.sources.map((source) => <SourceBlock key={source.source} source={source} colors={colors} />)}
    {state.native.mode === "catalog" && state.native.view !== null
      ? <Copy colors={colors} small>{`Native inventory: shared catalog ${state.native.runtimeInstance} · generation ${state.native.view.generation} · as of ${state.native.view.asOf ?? "Unknown"}${state.native.view.truncated ? " · bounded" : ""}${state.native.view.archiveScope ? "" : " · archived sessions not collected"}.`}</Copy>
      : state.native.mode === "snapshot" && state.native.view !== null
        ? <Copy colors={colors} small>{`Native inventory: last-known snapshot${state.native.snapshotSavedAt ? ` saved ${state.native.snapshotSavedAt}` : ""} · ${state.native.runtimeInstance ?? "runtime Unknown"} · as of ${state.native.view.asOf ?? "Unknown"}${state.native.view.truncated ? " · bounded" : ""}.`}</Copy>
        : <Copy colors={colors} muted small>{`Native inventory unavailable: ${state.native.error ?? "no catalog runtime matched this host"}. Direct Paseo readers remain active.`}</Copy>}
    {state.view.entriesTruncated ? <Copy colors={colors} muted small>Entry pages are bounded; the catalog holds more entries than this view.</Copy> : null}
  </View>;
}
