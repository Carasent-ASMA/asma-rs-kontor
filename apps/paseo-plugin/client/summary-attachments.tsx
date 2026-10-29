import { createContext, useContext, useState } from "react";
import { Pressable, Text, View } from "react-native";
import type { PluginClientContext } from "@getpaseo/plugin/client";
import { savedSummaryAttachments, SUMMARY_ATTACHMENT_LIMIT, SUMMARY_ATTACHMENT_STEPS } from "../shared/summary-attachments";
import type { Colors } from "./page-controls";

export const SummaryAttachmentAvailability = createContext(false);
export const useSummaryAttachmentAvailability = () => useContext(SummaryAttachmentAvailability);
export const SUMMARY_ATTACHMENT_UNAVAILABLE = "This app cannot register Kontor's native summary attachment source. No draft was changed here; use a supported stock Paseo app.";

export function registerSummaryAttachments(client: PluginClientContext): { available: boolean; remove(): void } {
  if (typeof client.addAttachmentSource !== "function" || typeof client.addComposerPill !== "function") return { available: false, remove() {} };
  try { return { available: true, remove: client.addAttachmentSource(savedSummaryAttachments) }; }
  catch { return { available: false, remove() {} }; }
}

/** Uses the existing destination-side Kontor composer pill; it never operates on a draft. */
export function SummaryAttachmentHelp({ available, colors }: { available: boolean; colors: Colors }) {
  const [open, setOpen] = useState(false);
  return <View style={{ gap: 6 }}>
    <Pressable testID="summary-native-help" accessibilityRole="button" accessibilityState={{ expanded: open }} onPress={() => setOpen(!open)} style={{ minHeight: 44, justifyContent: "center" }}>
      <Text style={{ color: colors.foreground, fontSize: 12 }}>{open ? "Close attachment instructions" : "Attach a saved summary…"}</Text>
    </Pressable>
    {open ? <Text testID="summary-native-instructions" style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>{available ? `${SUMMARY_ATTACHMENT_STEPS} ${SUMMARY_ATTACHMENT_LIMIT}` : SUMMARY_ATTACHMENT_UNAVAILABLE}</Text> : null}
  </View>;
}
