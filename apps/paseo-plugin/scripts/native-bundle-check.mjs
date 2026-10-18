import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { createRequire } from "node:module";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { DaemonClient } from "@getpaseo/client/internal/daemon-client";

const port = process.env.PASEO_CLOSURE_PORT ?? process.argv[2];
if (!port) {
  throw new Error("Usage: node scripts/native-bundle-check.mjs <daemon-port>");
}

const require = createRequire(import.meta.url);
const reactNativePackage = require.resolve("react-native/package.json");
const hermesBin =
  process.platform === "darwin"
    ? "osx-bin"
    : process.platform === "win32"
      ? "win64-bin"
      : "linux64-bin";
const hermesc = join(
  dirname(reactNativePackage),
  "sdks",
  "hermesc",
  hermesBin,
  process.platform === "win32" ? "hermesc.exe" : "hermesc",
);
assert.ok(existsSync(hermesc), `Hermes compiler not found at ${hermesc}`);

const sha256 = (value) => createHash("sha256").update(value).digest("hex");
const appVersion = process.env.PASEO_CLOSURE_APP_VERSION ?? "0.11.4";
const client = new DaemonClient({
  url: `ws://127.0.0.1:${port}/ws`,
  clientId: `kontor-native-check-${Date.now()}`,
  clientType: "cli",
  appVersion,
});
const workDir = mkdtempSync(join(tmpdir(), "paseo-kontor-native-"));

try {
  await client.connect();
  const catalog = await client.getPluginCatalog();
  const plugin = catalog.find((item) => item.id === "kontor");
  assert.ok(plugin, "the host catalog must contain the kontor plugin");
  assert.equal(typeof plugin.clientBundle, "string");
  assert.ok(plugin.clientBundle.length > 0, "the host must return a compiled client bundle");

  const bundleDir = join(workDir, "client");
  mkdirSync(bundleDir, { recursive: true });
  const bundlePath = join(bundleDir, "kontor.js");
  const bytecodePath = join(workDir, "kontor.hbc");
  writeFileSync(bundlePath, plugin.clientBundle);

  const hermesVersion = execFileSync(hermesc, ["--version"], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  }).trim();
  execFileSync(hermesc, ["-emit-binary", "-out", bytecodePath, bundlePath], {
    encoding: "utf8",
    stdio: ["ignore", "pipe", "pipe"],
  });
  const bytecode = readFileSync(bytecodePath);

  console.log(
    JSON.stringify(
      {
        pluginId: plugin.id,
        appVersion,
        compiler: hermesc,
        compilerVersion: hermesVersion,
        bundleBytes: plugin.clientBundle.length,
        bundleSha256: sha256(plugin.clientBundle),
        hermesExit: 0,
        bytecodeBytes: bytecode.length,
        bytecodeSha256: sha256(bytecode),
      },
      null,
      2,
    ),
  );
} catch (error) {
  if (error && typeof error === "object" && "stderr" in error) {
    process.stderr.write(String(error.stderr));
  }
  throw error;
} finally {
  await client.close().catch(() => undefined);
  rmSync(workDir, { recursive: true, force: true });
}
