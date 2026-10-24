/**
 * `react-native` for the web build: react-native-web, with Linking routed to
 * the host event log so no link leaves the prototype.
 */
import { Linking as WebLinking } from "react-native-web";
import { logHostEvent } from "./event-log";

export * from "react-native-web";

// Native Settings is imported by the real client; web profiles use localStorage.
export const Settings = undefined;

// Web has no native hardware Back events; the plugin installs its web key handlers itself.
export const BackHandler = { addEventListener: () => ({ remove() {} }), exitApp() {} };

export const Linking = {
  ...WebLinking,
  openURL: async (url: string) => { logHostEvent("linking.openURL", url); },
  canOpenURL: async () => true,
};
