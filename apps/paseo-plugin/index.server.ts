import type { PluginServerContext } from "@getpaseo/plugin/server";
import { contribute } from "./server/hooks";
import { readFloor, reportFloor } from "./server/floor";
import { organizationSettings } from "./shared/organization";
import { keyboardSettings } from "./shared/keyboard";
import { teamsSourceBindings } from "./shared/teams-source-bindings";
import { createTeamsSourceAdapter } from "./server/teams-source-adapter";
import { readTeamsSource } from "./shared/teams-source-snapshot";
import { sourceCatalog, contributeReadSources } from "./server/read-sources";
import { contributeSessionLifecycle } from "./server/session-lifecycle";
import { contributeResponsibility } from "./server/responsibility";
import { contributeKontorStatus } from "./server/kontor-status";

export default function plugin(server: PluginServerContext): () => void {
  contributeSessionLifecycle(server);
  const settings = server.registerSettings(organizationSettings);
  server.registerSettings(keyboardSettings);
  server.handle(readTeamsSource, (input) => {
    const catalog = sourceCatalog();
    const bindings = teamsSourceBindings(catalog.sources);
    return createTeamsSourceAdapter(catalog, bindings.formats).read(input);
  });
  const stopSources = contributeReadSources(server, settings, sourceCatalog);
  contributeResponsibility(server);
  const stopStatus = contributeKontorStatus(server);
  // The operator's floor.json is read once per worker start; off registers none of the three hooks.
  const floor = readFloor();
  reportFloor(floor);
  const stopHooks = floor.enabled ? contribute(server) : () => {};
  return () => { stopSources(); stopStatus(); stopHooks(); };
}
