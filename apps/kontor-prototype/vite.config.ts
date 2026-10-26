import { createRequire } from "node:module";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";

const here = path.dirname(fileURLToPath(import.meta.url));
const pluginRoot = path.resolve(here, "../paseo-plugin");
// Resolve the SDK through the plugin package so the prototype and the plugin
// share one module instance (and so one React context for useRpc).
const fromPlugin = createRequire(path.join(pluginRoot, "package.json"));
const host = (file: string) => path.join(here, "src/host", file);
// Operator-local captured data stays outside Git: KONTOR_PROTOTYPE_FIXTURE names its directory, which must hold every
// captured file, so real and synthetic data never mix. Without it, the committed empty stub loads and the synthetic
// fixtures stand alone.
const CAPTURED = ["captured-session-data.json", "captured-model-catalog.json", "captured-codex-features.json"] as const;
const captured = process.env.KONTOR_PROTOTYPE_FIXTURE ? path.resolve(process.env.KONTOR_PROTOTYPE_FIXTURE) : null;
const missing = captured ? CAPTURED.filter((file) => !fs.existsSync(path.join(captured, file))) : [];
if (missing.length) throw new Error(`KONTOR_PROTOTYPE_FIXTURE lacks ${missing.join(", ")}; capture every file or unset it for synthetic fixtures.`);
const capturedFile = (file: (typeof CAPTURED)[number]) => captured ? path.join(captured, file) : path.join(here, "src/fixtures/captured-empty.json");

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
      { find: /^captured:sessions$/, replacement: capturedFile("captured-session-data.json") },
      { find: /^captured:catalog$/, replacement: capturedFile("captured-model-catalog.json") },
      { find: /^captured:features$/, replacement: capturedFile("captured-codex-features.json") },
    ],
    dedupe: ["react", "react-dom", "react-native-web", "@tanstack/react-query", "zod"],
  },
  define: {
    // React Native Web animation cleanup references the native global alias.
    global: "globalThis",
    __DEV__: "false",
    "process.env.NODE_ENV": JSON.stringify(process.env.NODE_ENV ?? "development"),
  },
  server: { host: "127.0.0.1", port: 58141, strictPort: true, hmr: false, fs: { allow: [path.resolve(here, "../.."), ...(captured ? CAPTURED.map((file) => path.join(captured, file)) : [])] } },
  preview: { host: "127.0.0.1", port: 58142, strictPort: true },
  build: { outDir: "dist", sourcemap: true, chunkSizeWarningLimit: 4000 },
});
