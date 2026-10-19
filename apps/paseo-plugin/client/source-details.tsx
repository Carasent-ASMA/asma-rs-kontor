import { useState } from "react";
import { View } from "react-native";
import type { SourceRead } from "../shared/read-sources";
import { Action, Copy, type Colors } from "./page-controls";
/** `bare` drops the summary lines when the caller already shows the source title, status and read time. */
export function SourceDetails({ read, colors, expanded, bare = false }: { read: SourceRead; colors: Colors; expanded?: boolean; bare?: boolean }) {
  const [open, setOpen] = useState(false);
  return <View style={{ gap: 2 }}>
    {bare ? null : <>
      <Copy colors={colors} muted small>{read.source.title} · {read.status} · {read.source.kind === "jira" ? "Historical Jira snapshot" : read.source.revisionKind}</Copy>
      <Copy colors={colors} muted small>{read.source.kind === "jira" ? `Observed ${read.jiraHistory?.generated ?? "Unknown"} · not live` : `Read ${read.readAt}`}</Copy>
    </>}
    {read.status !== "ready" ? <Copy colors={colors} small>Source unavailable or partial. Open Source details for diagnostics.</Copy> : null}
    {expanded === undefined ? <View style={{ alignSelf: "flex-start" }}><Action small colors={colors} label="Source details" accessibilityLabel={`Source details: ${read.source.title}`} expanded={open} onPress={() => setOpen(!open)} /></View> : null}
    {(expanded ?? open) ? <>
      <Copy colors={colors} small>{read.source.path}</Copy>
      <Copy colors={colors} small>SHA-256 {read.revision ?? "Unknown"} · read {read.readAt} · modified {read.modifiedAt ?? "Unknown"}</Copy>
      {read.jiraHistory ? <>
        <Copy colors={colors} small>{read.jiraHistory.schema} · original capture contains {read.jiraHistory.provenance.capturedEpics} epics; this projection has {read.jiraHistory.epics.length}. Neither count establishes complete inventory.</Copy>
        <Copy colors={colors} small>{read.jiraHistory.provenance.path} · SHA-256 {read.jiraHistory.provenance.sha256}</Copy>
      </> : null}
      {read.diagnostics.map((diagnostic, index) => <Copy key={index} colors={colors} small>{diagnostic}</Copy>)}
    </> : null}
  </View>;
}
