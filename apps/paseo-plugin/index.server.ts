import type { PluginServerContext } from "@getpaseo/plugin/server";
import { contribute } from "./server/hooks";

export default function plugin(server: PluginServerContext): () => void {
  return contribute(server);
}
