import type { PluginServerContext } from "@getpaseo/plugin/server";
import { contribute } from "./server/hooks";
import { organizationSettings } from "./shared/organization";
import { contributeReadSources } from "./server/read-sources";

export default function plugin(server: PluginServerContext): () => void {
  const settings = server.registerSettings(organizationSettings);
  const stopSources = contributeReadSources(server, settings);
  const stopHooks = contribute(server);
  return () => { stopSources(); stopHooks(); };
}
