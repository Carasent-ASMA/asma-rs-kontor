import { useState } from "react";
import { View } from "react-native";
import type { SourceRead } from "../shared/read-sources";
import { Action, Copy, type Colors } from "./page-controls";
export function SourceDetails({ read, colors }: { read: SourceRead; colors: Colors }) {
  const [open, setOpen] = useState(false);
  return <View style={{ gap: 2 }}>
    <Copy colors={colors} muted>{read.source.title} · {read.status} · {read.source.kind === "jira" ? "Historical Jira snapshot" : read.source.revisionKind}</Copy>
    <Copy colors={colors} muted>{read.source.kind === "jira" ? `Observed ${read.jiraHistory?.generated ?? "Unknown"} · not live` : `Read ${read.readAt}`}</Copy>
    {read.status !== "ready" ? <Copy colors={colors}>Source unavailable or partial. Open Source details for diagnostics.</Copy> : null}
    <Action colors={colors} label="Source details" accessibilityLabel={`Source details: ${read.source.title}`} selected={open} onPress={() => setOpen(!open)} />
    {open ? <>
      <Copy colors={colors}>{read.source.path}</Copy>
      <Copy colors={colors}>SHA-256 {read.revision ?? "Unknown"} · read {read.readAt} · modified {read.modifiedAt ?? "Unknown"}</Copy>
      {read.jiraHistory ? <>
        <Copy colors={colors}>{read.jiraHistory.schema} · original capture contains {read.jiraHistory.provenance.capturedEpics} epics; this projection has {read.jiraHistory.epics.length}. Neither count establishes complete inventory.</Copy>
        <Copy colors={colors}>{read.jiraHistory.provenance.path} · SHA-256 {read.jiraHistory.provenance.sha256}</Copy>
      </> : null}
      {read.diagnostics.map((diagnostic, index) => <Copy key={index} colors={colors}>{diagnostic}</Copy>)}
    </> : null}
  </View>;
}
