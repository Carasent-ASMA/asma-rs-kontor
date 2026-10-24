import { useState } from "react";
import { Text, View } from "react-native";
import type { KontorStatus } from "../shared/kontor-status";
import { kontordBanner } from "./kontord-copy";
import { NoticeAction } from "./notice-tray";
import type { Colors } from "./page-controls";

/** The Kontor screen's status line. Its details open on tap or keyboard, never on hover alone. */
export function KontordBanner({ status, compact, colors }: { status: KontorStatus | null; compact: boolean; colors: Colors }) {
  const [open, setOpen] = useState(false);
  const copy = kontordBanner(status);
  return <View testID="kontor-kontord-line" style={{ paddingHorizontal: compact ? 8 : 12, paddingVertical: 2 }}>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 8 }}>
      <Text selectable style={{ flex: 1, minWidth: 0, color: colors.foreground, fontSize: 12, lineHeight: 18 }}>
        <Text style={{ fontWeight: "600" }}>{copy.title}</Text>
        {copy.checked ? <Text style={{ color: colors.foregroundMuted }}>{compact ? "\n" : "  "}{copy.checked}</Text> : null}
      </Text>
      <NoticeAction icon="Info" accessibilityLabel="Kontor status details" help="What this Kontor status means." colors={colors} expanded={open} onPress={() => setOpen(!open)} />
    </View>
    {open ? <Text testID="kontor-kontord-details" selectable style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 18, paddingBottom: 4 }}>{copy.details}</Text> : null}
  </View>;
}
