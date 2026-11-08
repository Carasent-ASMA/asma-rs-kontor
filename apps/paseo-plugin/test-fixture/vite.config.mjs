// R38 fixture vite config: the kontor-prototype alias set (one SDK module instance, react-native-web,
// host-supplied @getpaseo/plugin/client) over this COMMITTED fixture root (test-fixture/). The worktree
// is supplied as R38_WORKTREE; nothing outside test-fixture/ in the worktree is modified by the build
// (run-fixture.py passes an explicit --outDir into the evidence dir). The alias set is copied from
// apps/kontor-prototype/vite.config.ts at the base commit and self-contained here so the fixture
// config never executes the prototype's own config file.
import { createRequire } from "node:module";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const worktree = path.resolve(process.env.R38_WORKTREE);
if (!worktree) throw new Error("R38_WORKTREE must name the worktree root");
const prototype = path.join(worktree, "apps/kontor-prototype");
const plugin = path.join(worktree, "apps/paseo-plugin");
const fromPlugin = createRequire(path.join(plugin, "package.json"));

// A plain object: this file is loaded from outside any node_modules tree, so it must not import
// the vite package to reach defineConfig (which is the identity function anyway).
export default {
  root: here,
  base: "./",
  resolve: {
    alias: [
      // The three host-supplied SDK modules (PLUGIN_CLIENT_ONLY_SDK_SPECIFIERS) as the prototype serves them.
      { find: /^@getpaseo\/plugin\/client$/, replacement: path.join(prototype, "src/host/sdk-client.ts") },
      { find: /^@getpaseo\/plugin\/client\/react-native$/, replacement: path.join(prototype, "src/host/sdk-react-native.tsx") },
      { find: /^@getpaseo\/plugin\/client\/ui$/, replacement: path.join(prototype, "src/host/sdk-ui.tsx") },
      // The real SDK pieces the fixture reuses unchanged (one module instance => one React context).
      { find: /^paseo-sdk\/client$/, replacement: fromPlugin.resolve("@getpaseo/plugin/client") },
      { find: /^paseo-sdk\/host$/, replacement: fromPlugin.resolve("@getpaseo/plugin/client/host") },
      { find: /^@getpaseo\/plugin$/, replacement: fromPlugin.resolve("@getpaseo/plugin") },
      { find: /^@getpaseo\/plugin\/client\/host$/, replacement: fromPlugin.resolve("@getpaseo/plugin/client/host") },
      { find: /^\.\/task-team-source$/, replacement: path.join(prototype, "src/host/task-team-source.ts") },
      { find: /^@plugin/, replacement: plugin },
      { find: /^@prototype/, replacement: prototype },
      { find: /^react-native$/, replacement: path.join(prototype, "src/host/react-native.ts") },
      { find: /^captured:sessions$/, replacement: path.join(prototype, "src/fixtures/captured-empty.json") },
      { find: /^captured:catalog$/, replacement: path.join(prototype, "src/fixtures/captured-empty.json") },
      { find: /^captured:features$/, replacement: path.join(prototype, "src/fixtures/captured-empty.json") },
      // One instance of every runtime dep: the fixture lives outside any node_modules tree, so the
      // prototype's own copies are pinned here (the same set its dedupe list names).
      { find: /^react$/, replacement: path.join(prototype, "node_modules/react") },
      { find: /^react\//, replacement: path.join(prototype, "node_modules/react") + "/" },
      { find: /^react-dom$/, replacement: path.join(prototype, "node_modules/react-dom") },
      { find: /^react-dom\//, replacement: path.join(prototype, "node_modules/react-dom") + "/" },
      { find: /^react-native-web$/, replacement: path.join(prototype, "node_modules/react-native-web") },
      { find: /^react-native-web\//, replacement: path.join(prototype, "node_modules/react-native-web") + "/" },
      { find: /^@tanstack\/react-query$/, replacement: path.join(prototype, "node_modules/@tanstack/react-query") },
      { find: /^zod$/, replacement: path.join(prototype, "node_modules/zod") },
    ],
    dedupe: ["react", "react-dom", "react-native-web", "@tanstack/react-query", "zod"],
  },
  define: { global: "globalThis", __DEV__: "false", "process.env.NODE_ENV": JSON.stringify("development") },
  build: { outDir: path.join(here, "dist"), sourcemap: false, chunkSizeWarningLimit: 6000 },
  server: { host: "127.0.0.1", port: 58241, strictPort: true, hmr: false, fs: { allow: [worktree, here] } },
  preview: { host: "127.0.0.1", port: 58242, strictPort: true },
};
