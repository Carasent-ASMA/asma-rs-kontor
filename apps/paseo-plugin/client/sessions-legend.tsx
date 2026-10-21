import { Text, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import { KIND_ICONS, KIND_NAMES, WORKSPACE_KINDS } from "./organization-row";
import { Disclosure, type Colors } from "./page-controls";

/** Static reference: the same icons and workspace definitions as the tree. */
export function SessionsLegend({ colors, compact, open, onToggle }: {
  colors: Colors; compact: boolean; open: boolean; onToggle: (open: boolean) => void;
}) {
  const referenceColors = { ...colors, accent: colors.foreground };
  const section = { flexGrow: 1, flexBasis: compact ? "auto" as const : 220, minWidth: 0, gap: 8 };
  const heading = { color: colors.foreground, fontSize: 13, fontWeight: "600" as const, lineHeight: 20 };
  const text = { color: colors.foreground, fontSize: 14, lineHeight: 21, flexShrink: 1, minWidth: 0 };
  return <Disclosure label="Legend" colors={referenceColors} open={open} onToggle={onToggle}>
    <View testID="sessions-legend" style={{ flexDirection: compact ? "column" : "row", flexWrap: "wrap", gap: 20, paddingVertical: 8 }}>
      <View style={section}>
        <Text accessibilityRole="header" style={heading}>Tree icons</Text>
        {(["workstream", "epic", "workspace", "project"] as const).map((kind) => <View key={kind} style={{ flexDirection: "row", alignItems: "flex-start", gap: 8 }}>
          <View accessible={false} accessibilityElementsHidden importantForAccessibility="no-hide-descendants" aria-hidden style={{ paddingTop: 2 }}>
            <Icon name={KIND_ICONS[kind]!} size={16} color={colors.foregroundMuted} />
          </View>
          <Text style={text}>{KIND_NAMES[kind]}</Text>
        </View>)}
      </View>
      <View style={section}>
        <Text accessibilityRole="header" style={heading}>Workspace codes</Text>
        {Object.entries(WORKSPACE_KINDS).map(([code, meaning]) => <View key={code} style={{ flexDirection: "row", alignItems: "flex-start", gap: 8 }}>
          <Text style={{ ...text, flexShrink: 0, width: 48, fontFamily: "monospace", fontWeight: "600" }}>{code}</Text>
          <Text style={{ ...text, flex: 1 }}>{meaning}</Text>
        </View>)}
      </View>
      <View style={section}>
        <Text accessibilityRole="header" style={heading}>Grouping</Text>
        <Text style={text}>Role view groups sessions by their displayed role. Within a workspace, repeated title prefixes can also form groups.</Text>
        <Text style={text}>These groups and workspace codes are for display only. They do not authorize a role.</Text>
        <Disclosure label="Role label rules" colors={referenceColors}>
          <Text style={text}>Role view uses the kontor.role label, then the role label, then the title prefix before •.</Text>
        </Disclosure>
      </View>
    </View>
  </Disclosure>;
}
