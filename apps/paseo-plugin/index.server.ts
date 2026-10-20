import type { PluginServerContext } from "@getpaseo/plugin/server";
import { contribute } from "./server/hooks";
import { readFloor, reportFloor } from "./server/floor";
import { organizationSettings } from "./shared/organization";
import { keyboardSettings } from "./shared/keyboard";
import { teamsSourceBindings } from "./shared/teams-source-bindings";
import { contributeTeamsSourceAdapter, createTeamsSourceAdapter } from "./server/teams-source-adapter";
import { sourceCatalog, contributeReadSources } from "./server/read-sources";

export default function plugin(server: PluginServerContext): () => void {
  const settings = server.registerSettings(organizationSettings);
  server.registerSettings(keyboardSettings);
  const catalog = sourceCatalog();
  const bindings = teamsSourceBindings(catalog.sources);
  contributeTeamsSourceAdapter(server, createTeamsSourceAdapter(catalog, bindings.formats));
  const stopSources = contributeReadSources(server, settings, catalog);
  // The operator's floor.json is read once per worker start; off registers none of the three hooks.
  const floor = readFloor();
  reportFloor(floor);
  const stopHooks = floor.enabled ? contribute(server) : () => {};
  return () => { stopSources(); stopHooks(); };
}
