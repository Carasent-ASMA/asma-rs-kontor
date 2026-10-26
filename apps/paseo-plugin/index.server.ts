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
import { contributeSessionCreate } from "./server/session-create";
import { contributeResponsibility } from "./server/responsibility";
import { contributeKontorStatus } from "./server/kontor-status";
import { readNavigatorCapabilities } from "./shared/session-navigator";
import { readInstalledNavigatorCapabilities } from "./server/session-navigator-capabilities";
import { contributeSessionSummaries } from "./server/session-summaries";

export default function plugin(server: PluginServerContext): () => void {
  contributeSessionLifecycle(server);
  // Registered whatever the floor; each request reads create.json: absent defaults on, explicit off or invalid/unreadable paths fail closed.
  contributeSessionCreate(server);
  contributeSessionSummaries(server);
  server.handle(readNavigatorCapabilities, () => readInstalledNavigatorCapabilities());
  const settings = server.registerSettings(organizationSettings);
  server.registerSettings(keyboardSettings);
  server.handle(readTeamsSource, (input) => {
    const catalog = sourceCatalog();
    const bindings = teamsSourceBindings(catalog.sources);
    return createTeamsSourceAdapter(catalog, bindings.formats).read(input);
  });
  const stopSources = contributeReadSources(server, settings, sourceCatalog);
  contributeResponsibility(server);
  // The operator's floor.json is read once per worker start; off registers none of the three hooks.
  const floor = readFloor();
  reportFloor(floor);
  // Registered whatever the floor; the floor is only reported, as whether new agents get kontor-observer.
  const stopStatus = contributeKontorStatus(server, floor.enabled);
  const stopHooks = floor.enabled ? contribute(server) : () => {};
  return () => { stopSources(); stopStatus(); stopHooks(); };
}
