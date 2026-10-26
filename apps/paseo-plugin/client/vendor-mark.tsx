import { Image, View } from "react-native";
import { Icon } from "@getpaseo/plugin/client/react-native";
import type { Colors } from "./page-controls";
import type { Vendor } from "./session-usage";
import { VENDOR_MARK_PNGS } from "./vendor-mark-assets";

/**
 * Each known vendor's own mark, in its brand color where it has one and the text color otherwise. A vendor with no
 * bundled mark is named under a neutral one; only a truly unknown author is Other.
 */
const MARKS: Record<Vendor, { readonly name: string; readonly short?: string; readonly color?: string }> = {
  openai: { name: "OpenAI" }, anthropic: { name: "Anthropic", color: "#D97757" }, deepseek: { name: "DeepSeek", color: "#4D6BFE" },
  zai: { name: "Z.AI" }, xai: { name: "xAI" }, google: { name: "Google" }, cursor: { name: "Cursor" },
  moonshot: { name: "Moonshot" }, alibaba: { name: "Alibaba" },
  nvidia: { name: "NVIDIA" }, cohere: { name: "Cohere" }, liquid: { name: "Liquid AI", short: "Liquid" }, xiaomi: { name: "Xiaomi" }, minimax: { name: "MiniMax" },
  meituan: { name: "Meituan" }, inclusionai: { name: "inclusionAI", short: "inclusion" }, thinkingmachines: { name: "Thinking Machines", short: "Thinking" },
  unknown: { name: "Unknown vendor", short: "Other" },
};
const PNGS: Partial<Record<Vendor, string>> = VENDOR_MARK_PNGS;

export const vendorName = (vendor: Vendor) => MARKS[vendor].name;
/** The name under a vendor tile's mark; the tile still speaks the full name. */
export const vendorShortName = (vendor: Vendor) => MARKS[vendor].short ?? MARKS[vendor].name;

/** The vendor that made the model, never the route it runs through; a vendor without a bundled mark gets a neutral one. */
export function VendorMark({ vendor, size = 12, colors }: { vendor: Vendor; size?: number; colors: Colors }) {
  const mark = MARKS[vendor], png = PNGS[vendor];
  return <View testID={`vendor-mark-${vendor}`} accessibilityLabel={mark.name} role="img" style={{ width: size, height: size, alignItems: "center", justifyContent: "center" }}>
    {png ? <Image source={{ uri: png }} accessibilityIgnoresInvertColors style={{ width: size, height: size, tintColor: mark.color ?? colors.foreground }} />
      : <Icon name={vendor === "unknown" ? "CircleHelp" : "Box"} size={size} color={colors.foregroundMuted} />}
  </View>;
}
