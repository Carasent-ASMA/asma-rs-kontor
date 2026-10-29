import { constants } from "node:fs";
import { access, stat } from "node:fs/promises";
import { homedir, userInfo } from "node:os";
import { isAbsolute, join } from "node:path";

export const ASMA_COMMAND_ENV = "KONTOR_PLUGIN_ASMA_COMMAND";
export const ASMA_WORKSPACE_ENV = "KONTOR_PLUGIN_ASMA_WORKSPACE";

export type AsmaCommandOptions = {
  env?: NodeJS.ProcessEnv;
  home?: string;
  inspect?: (path: string) => Promise<boolean>;
};

async function executable(path: string): Promise<boolean> {
  try {
    await access(path, constants.X_OK);
    return (await stat(path)).isFile();
  } catch { return false; }
}

function candidates(env: NodeJS.ProcessEnv, home: string): string[] {
  const paths = [join(home, ".local/bin/asma")];
  const workspace = env[ASMA_WORKSPACE_ENV]?.trim();
  if (workspace && isAbsolute(workspace)) paths.push(join(workspace, ".venv/bin/asma"));
  paths.push("/opt/homebrew/bin/asma", "/usr/local/bin/asma", "/usr/bin/asma");
  return [...new Set(paths.filter(isAbsolute))];
}

export async function resolveAsmaCommand(options: AsmaCommandOptions = {}): Promise<string> {
  const env = options.env ?? process.env;
  const check = options.inspect ?? executable;
  const configured = env[ASMA_COMMAND_ENV]?.trim();
  if (configured) {
    if (!isAbsolute(configured))
      throw new Error(`${ASMA_COMMAND_ENV} must be an absolute path to the installed asma executable.`);
    if (!(await check(configured)))
      throw new Error(`${ASMA_COMMAND_ENV} does not name an executable asma file.`);
    return configured;
  }
  const configuredHome = options.home?.trim() || env.HOME?.trim();
  let home = configuredHome && isAbsolute(configuredHome) ? configuredHome : "";
  if (!home) {
    try { home = userInfo().homedir; } catch { home = ""; }
    if (!isAbsolute(home)) home = homedir();
  }
  for (const path of candidates(env, home)) {
    if (!isAbsolute(path)) continue;
    if (await check(path)) return path;
  }
  const install = isAbsolute(home) ? join(home, ".local/bin/asma") : "an absolute home directory";
  throw new Error(`The ASMA CLI could not be found at a trusted absolute path. Install it at ${install} or set ${ASMA_COMMAND_ENV} to its absolute executable path.`);
}
