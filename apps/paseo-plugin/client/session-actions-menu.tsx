import { useState, type Ref } from "react";
import { Platform, Pressable, Text, View } from "react-native";
import { copyText, Icon, Modal } from "@getpaseo/plugin/client/react-native";
import { useActionHelp } from "./control-help";
import { cancelDelayedOpen } from "./dense-session-row";
import type { Colors } from "./page-controls";
import type { DirectoryEntry } from "./session-directory";
import { readableInk } from "./session-status-ink";

/**
 * One action: shown with the reason it cannot run instead of hiding it, so the menu teaches what exists.
 * - `detail`: a second line under the label while it can run.
 * - `option`: a pill at the row's end that runs a variant of the action, such as Take over's Auto ▾.
 * - `caption` and `footer`: supporting text attached to this action.
 * - `opens`: it leads to another screen, so it carries a chevron.
 */
export type SessionAction = {
  readonly key: string; readonly label: string; readonly icon: string; readonly unavailable?: string | null; readonly perform: () => unknown;
  readonly detail?: string; readonly caption?: string; readonly footer?: string; readonly opens?: boolean;
  readonly option?: { readonly label: string; readonly accessibilityLabel: string; readonly unavailable?: string | null; readonly perform: () => unknown };
};

const focusRing = (colors: Colors, focused?: boolean) => focused && Platform.OS === "web" ? { outlineStyle: "solid" as const, outlineWidth: 2, outlineColor: colors.accent } : {};

// React Native Web writes aria-disabled only from `disabled`, which also drops the item from Tab; tabIndex keeps a refused item reachable so its reason is heard.
function ActionRow({ action, colors, onDone }: { action: SessionAction; colors: Colors; onDone: () => void }) {
  const blocked = !!action.unavailable, option = action.option, optionBlocked = blocked || !!option?.unavailable;
  const row = <Pressable testID={`session-action-${action.key}`} role="menuitem" accessibilityLabel={[action.label, blocked ? action.unavailable : action.detail].filter(Boolean).join(". ")}
    aria-disabled={blocked || undefined} disabled={blocked} {...(Platform.OS === "web" ? { tabIndex: 0 } : {})} accessibilityState={{ disabled: blocked }} onPress={() => { if (blocked) return; onDone(); void action.perform(); }}
    style={({ pressed, hovered, focused }: { pressed: boolean; hovered?: boolean; focused?: boolean }) => ({ flex: 1, minWidth: 0, minHeight: 44, flexDirection: "row", alignItems: "center", gap: 10, paddingHorizontal: 10,
      borderRadius: 6, borderWidth: 1, borderColor: focused ? colors.foreground : "transparent", backgroundColor: pressed || hovered ? colors.surface2 : undefined, ...focusRing(colors, focused) })}>
    {/* The grouped primary action carries the accent, as board C1 draws it. */}
    <Icon name={action.icon} size={18} color={blocked ? colors.foregroundMuted : option ? readableInk(colors.accent, colors) : colors.foreground} />
    <View style={{ flex: 1, minWidth: 0 }}>
      <Text style={{ color: blocked ? colors.foregroundMuted : option ? readableInk(colors.accent, colors, 4.5) : colors.foreground, fontSize: 14, fontWeight: option ? "600" : "400" }}>{action.label}</Text>
      {blocked || action.detail ? <Text style={{ color: colors.foregroundMuted, fontSize: 11 }}>{blocked ? action.unavailable : action.detail}</Text> : null}
    </View>
    {action.opens ? <Icon name="ChevronRight" size={16} color={colors.foregroundMuted} /> : null}
  </Pressable>;
  if (!option && !action.caption && !action.footer) return row;
  return <View style={{ gap: 2, paddingBottom: 4, borderRadius: 8, borderWidth: option ? 1 : 0, borderColor: blocked ? colors.border : colors.accent }}>
    <View style={{ flexDirection: "row", alignItems: "center", gap: 4, paddingRight: 6 }}>
      {row}
      {option ? <Pressable testID={`session-action-${action.key}-option`} role="menuitem" accessibilityLabel={optionBlocked ? `${option.accessibilityLabel}. ${action.unavailable ?? option.unavailable}` : option.accessibilityLabel}
        aria-haspopup="dialog" aria-disabled={optionBlocked || undefined} disabled={optionBlocked} {...(Platform.OS === "web" ? { tabIndex: 0 } : {})} accessibilityState={{ disabled: optionBlocked }} onPress={() => { if (optionBlocked) return; onDone(); void option.perform(); }}
        style={({ pressed, focused }: { pressed: boolean; focused?: boolean }) => ({ minHeight: 44, minWidth: 64, paddingHorizontal: 10, flexDirection: "row", alignItems: "center", justifyContent: "center", gap: 2,
          borderRadius: 8, borderWidth: 1, borderColor: focused ? colors.foreground : colors.border, backgroundColor: pressed ? colors.surface2 : colors.surface1, opacity: optionBlocked ? 0.5 : 1, ...focusRing(colors, focused) })}>
        <Text style={{ color: colors.foreground, fontSize: 12, fontWeight: "600" }}>{option.label}</Text>
        <Icon name="ChevronDown" size={14} color={colors.foreground} />
      </Pressable> : null}
    </View>
    {action.footer ? <Text testID={`session-action-${action.key}-disclosure`} style={{ color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 10 }}>{action.footer}</Text> : null}
    {action.caption ? <Text style={{ color: colors.foregroundMuted, fontSize: 11, paddingHorizontal: 38 }}>{action.caption}</Text> : null}
  </View>;
}

type MenuKey = { key: string; preventDefault(): void; currentTarget: { ownerDocument: { activeElement: unknown }; querySelectorAll(selector: string): ArrayLike<{ focus(): void }> } };
/** Up and Down move focus through the menu's items, wrapping, and Home and End go to its ends, as a menu does; from outside the items Down goes to the first and Up to the last. */
export function moveMenuFocus(event: MenuKey) {
  const step = event.key === "ArrowDown" ? 1 : event.key === "ArrowUp" ? -1 : 0;
  if (!step && event.key !== "Home" && event.key !== "End") return;
  const items = Array.from(event.currentTarget.querySelectorAll('[role="menuitem"]')), at = items.indexOf(event.currentTarget.ownerDocument.activeElement as { focus(): void });
  event.preventDefault();
  items[event.key === "Home" ? 0 : event.key === "End" || (at < 0 && step < 0) ? items.length - 1 : (at + step + items.length) % items.length]?.focus();
}

/** Board C1's order on every surface; a surface lists only the actions it has. */
export const ACTION_ORDER = ["takeover", "favorite", "summarize", "attach", "copy-id", "details", "show-in-tree", "select", "archive"];

/**
 * The trailing ellipsis of a session row and the sheet of that session's actions; nothing runs until one is chosen.
 * `ref` reaches the ellipsis, where focus returns after an action that leaves the row, such as Details.
 */
export function SessionActionsMenu({ entry, colors, actions, ref }: { entry: Pick<DirectoryEntry, "id" | "title">; colors: Colors; actions: readonly SessionAction[]; ref?: Ref<View> }) {
  const [open, setOpen] = useState(false);
  const help = useActionHelp(`Actions for ${entry.title}`, ref);
  const ordered = [...actions].sort((a, b) => ACTION_ORDER.indexOf(a.key) - ACTION_ORDER.indexOf(b.key));
  return <>
    <Pressable {...help.bindings} testID={`session-actions-${entry.id}`} accessibilityRole="button" accessibilityLabel={`Actions for ${entry.title}`}
      aria-haspopup="menu" aria-expanded={open} accessibilityState={{ expanded: open }} onPress={() => { help.dismiss(); cancelDelayedOpen(); setOpen(true); }}
      style={({ pressed }: { pressed: boolean }) => ({ width: 44, minHeight: 44, alignItems: "center", justifyContent: "center", borderRadius: 6, backgroundColor: pressed ? colors.surface2 : undefined })}>
      <Icon name="Ellipsis" size={18} color={colors.foreground} />
    </Pressable>
    {open ? <Modal title="Session actions" open onOpenChange={setOpen}>
      <Modal.Content contentContainerStyle={{ padding: 8, gap: 2 }}>
        <Text numberOfLines={2} style={{ color: colors.foregroundMuted, fontSize: 12, paddingHorizontal: 10, paddingBottom: 4 }}>{entry.title}</Text>
        <View role="menu" accessibilityLabel={`Actions for ${entry.title}`} {...(Platform.OS === "web" ? { onKeyDown: moveMenuFocus } : {})} style={{ gap: 2 }}>
          {ordered.map((action) => <ActionRow key={action.key} action={action} colors={colors} onDone={() => setOpen(false)} />)}
        </View>
      </Modal.Content>
    </Modal> : null}
  </>;
}

/** Copies the exact native ID; the toast says so only after the clipboard accepted it. */
export async function copySessionId(entry: DirectoryEntry, report: { show(message: string): void; error(message: string): void }) {
  try { await copyText(entry.id); report.show(`Copied session ID ${entry.id}`); }
  catch (error) { report.error(`The clipboard refused the session ID (${error instanceof Error ? error.message : String(error)}). It is ${entry.id}.`); }
}
