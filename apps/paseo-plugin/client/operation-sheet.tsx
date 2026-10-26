import type { ReactNode } from "react";
import { Pressable, Text, View } from "react-native";
import { Modal, ScrollView } from "@getpaseo/plugin/client/react-native";
import { COARSE_POINTER, type Colors } from "./page-controls";

/**
 * One footer button; `primary` fills with the accent. A disabled primary keeps its label and says why in `hint`, spoken
 * with the label (React Native Web writes no hint), and stays in Tab so the reason is reachable.
 */
function FooterButton({ testID, label, accessibilityLabel, hint, primary = false, disabled = false, colors, onPress }: {
  testID?: string; label: string; accessibilityLabel?: string; hint?: string | null; primary?: boolean; disabled?: boolean; colors: Colors; onPress: () => void;
}) {
  return <Pressable testID={testID} accessibilityRole="button" accessibilityLabel={[accessibilityLabel ?? label, disabled ? hint : null].filter(Boolean).join(". ")} accessibilityState={{ disabled }} aria-disabled={disabled}
    // The primary takes the wider share, as the boards draw it, so its longer verb fits on a phone.
    disabled={disabled} tabIndex={0} onPress={onPress} style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ flex: primary ? 1.5 : 1, minWidth: 0, minHeight: 44, paddingHorizontal: 12, borderRadius: 8,
      alignItems: "center", justifyContent: "center", borderWidth: primary ? 2 : 1, opacity: pressed ? 0.85 : 1,
      borderColor: focused ? colors.foreground : primary ? disabled ? colors.surface2 : colors.accent : colors.border,
      backgroundColor: primary ? disabled ? colors.surface2 : colors.accent : undefined })}>
    {/* A long label wraps to a second line on a narrow phone rather than losing its end. */}
    <Text numberOfLines={2} style={{ color: primary ? disabled ? colors.foregroundMuted : colors.accentForeground : colors.foreground, fontSize: 13, fontWeight: "600", textAlign: "center" }}>{label}</Text>
  </Pressable>;
}

export type FooterAction = { readonly label: string; readonly accessibilityLabel?: string; readonly hint?: string | null; readonly disabled?: boolean; readonly testID?: string; readonly onPress: () => void };

/** Every confirmation and result footer: dismissal or the secondary action LEFT, the primary RIGHT, one row of 44pt targets. */
export function SheetFooter({ secondary, primary, note, colors }: { secondary?: FooterAction | null; primary?: FooterAction | null; note?: string | null; colors: Colors }) {
  return <View style={{ gap: 6, paddingTop: 8, borderTopWidth: 1, borderColor: colors.border }}>
    <View testID="operation-sheet-footer" style={{ flexDirection: "row", gap: 8 }}>
      {secondary ? <FooterButton {...secondary} colors={colors} /> : null}
      {primary ? <FooterButton {...primary} primary colors={colors} /> : null}
    </View>
    {note ? <Text style={{ color: colors.foregroundMuted, fontSize: 11, lineHeight: 15, textAlign: "center" }}>{note}</Text> : null}
  </View>;
}

/**
 * A sheet for one operation on sessions: the host's modal titled for it, a scrolling body and the footer pinned under
 * it. Closing it from the host's chrome is the same as the footer's dismissal.
 */
export function OperationSheet({ testID, title, subtitle, colors, onClose, footer, children }: {
  testID: string; title: string; subtitle?: string | null; colors: Colors; onClose: () => void; footer: ReactNode; children: ReactNode;
}) {
  return <Modal title={title} open onOpenChange={(open) => { if (!open) onClose(); }}>
    <Modal.Content scrollable={false} contentContainerStyle={{ padding: COARSE_POINTER ? 12 : 16, gap: 8 }}>
      <View testID={testID} style={{ flexShrink: 1, minHeight: 0, gap: 8 }}>
        {subtitle ? <Text numberOfLines={2} style={{ color: colors.foregroundMuted, fontSize: 12, lineHeight: 17 }}>{subtitle}</Text> : null}
        <ScrollView style={{ flexGrow: 0, flexShrink: 1 }} contentContainerStyle={{ gap: 10 }} keyboardShouldPersistTaps="handled">{children}</ScrollView>
        {footer}
      </View>
    </Modal.Content>
  </Modal>;
}
