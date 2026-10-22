import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const here = path.dirname(fileURLToPath(import.meta.url));
const pluginRoot = path.resolve(here, "../paseo-plugin");
// Resolve the SDK through the plugin package so the prototype and the plugin
// share one module instance (and so one React context for useRpc).
const fromPlugin = createRequire(path.join(pluginRoot, "package.json"));
const host = (file: string) => path.join(here, "src/host", file);

export default defineConfig({
  root: here,
  base: "./",
  resolve: {
    alias: [
      // The three host-supplied SDK modules (Paseo compiler: PLUGIN_CLIENT_ONLY_SDK_SPECIFIERS).
      { find: /^@getpaseo\/plugin\/client$/, replacement: host("sdk-client.ts") },
      { find: /^@getpaseo\/plugin\/client\/react-native$/, replacement: host("sdk-react-native.tsx") },
      { find: /^@getpaseo\/plugin\/client\/ui$/, replacement: host("sdk-ui.tsx") },
      // The real SDK pieces the fixture host reuses unchanged.
      { find: /^paseo-sdk\/client$/, replacement: fromPlugin.resolve("@getpaseo/plugin/client") },
      { find: /^paseo-sdk\/host$/, replacement: fromPlugin.resolve("@getpaseo/plugin/client/host") },
      { find: /^@getpaseo\/plugin$/, replacement: fromPlugin.resolve("@getpaseo/plugin") },
      // The production task team reader, or simulated declarations in scenarios that ask for them.
      { find: /^\.\/task-team-source$/, replacement: host("task-team-source.ts") },
      // React Native renders through react-native-web; the shim only reroutes Linking.
      { find: /^react-native$/, replacement: host("react-native.ts") },
    ],
    dedupe: ["react", "react-dom", "react-native-web", "@tanstack/react-query", "zod"],
  },
  define: {
    // React Native Web animation cleanup references the native global alias.
    global: "globalThis",
    __DEV__: "false",
    "process.env.NODE_ENV": JSON.stringify(process.env.NODE_ENV ?? "development"),
  },
  server: { host: "127.0.0.1", port: 58141, strictPort: true, hmr: false, fs: { allow: [path.resolve(here, "../..")] } },
  preview: { host: "127.0.0.1", port: 58142, strictPort: true },
  build: { outDir: "dist", sourcemap: true, chunkSizeWarningLimit: 4000 },
});
