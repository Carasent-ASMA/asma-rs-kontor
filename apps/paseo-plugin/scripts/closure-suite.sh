#!/usr/bin/env bash
# Isolated closure test for the Kontor Paseo plugin. Never uses ~/.paseo.
set -euo pipefail

PLUGIN_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
HOME_DIR="$(mktemp -d "${TMPDIR:-/tmp}/paseo-kontor-closure.XXXXXX")"
PORT="$(node -e 'const net=require("node:net");const s=net.createServer();s.listen(0,"127.0.0.1",()=>{console.log(s.address().port);s.close();});')"

cleanup() {
  paseo daemon stop --home "$HOME_DIR" >/dev/null 2>&1 || true
  rm -r "$HOME_DIR"
}
trap cleanup EXIT

paseo daemon config set daemon.listen "127.0.0.1:${PORT}" --home "$HOME_DIR" --string >/dev/null
paseo daemon config set pluginsEnabled true --home "$HOME_DIR" >/dev/null
KONTOR_PLUGIN_MCP=off paseo daemon start --home "$HOME_DIR" >/dev/null

echo "== isolated daemon =="
paseo daemon status --home "$HOME_DIR" --json | node -e 'let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{const j=JSON.parse(s);console.log(JSON.stringify({home:j.home,listen:j.listen,pid:j.pid,version:j.daemonVersion}))})'

echo "== install =="
paseo plugin install "$PLUGIN_DIR" --home "$HOME_DIR" --json >"$HOME_DIR/install.json" 2>"$HOME_DIR/install.stderr"
paseo plugin ls --home "$HOME_DIR" --json >"$HOME_DIR/installed.json"
node -e 'const j=require(process.argv[1]);const p=j.find(x=>x.id==="kontor");if(!p||p.status!=="running")process.exit(1);console.log(JSON.stringify({id:p.id,status:p.status,identity:p.installation.identity}))' "$HOME_DIR/installed.json"

echo "== native client bundle parse (Hermes) =="
PASEO_CLOSURE_PORT="$PORT" node "$PLUGIN_DIR/scripts/native-bundle-check.mjs" "$PORT" | tee "$HOME_DIR/native-bundle.json"

echo "== native navigator API (no kontord) =="
PASEO_CLOSURE_PORT="$PORT" node "$PLUGIN_DIR/scripts/navigator-probe.mjs" "$PORT" "$HOME_DIR" | tee "$HOME_DIR/navigator.json"

echo "== guarded behaviour =="
PASEO_CLOSURE_PORT="$PORT" node "$PLUGIN_DIR/scripts/closure.mjs" "$PORT" guarded | tee "$HOME_DIR/guarded.json"

echo "== cli workspace guard (labelled seat environment) =="
CLI_REPO="$HOME_DIR/cli-fixture"
mkdir -p "$CLI_REPO"
git -C "$CLI_REPO" init -q
git -C "$CLI_REPO" -c user.name=closure -c user.email=closure@example.com commit -q --allow-empty -m init
SEAT_JSON="$(PASEO_CLOSURE_PORT="$PORT" node "$PLUGIN_DIR/scripts/cli-seat.mjs" "$PORT" "$CLI_REPO")"
SEAT_AGENT="$(node -e 'console.log(JSON.parse(process.argv[1]).agentId)' "$SEAT_JSON")"
SEAT_WORKSPACE="$(node -e 'console.log(JSON.parse(process.argv[1]).workspaceId)' "$SEAT_JSON")"
CLI_ENV=(env "PASEO_AGENT_ID=$SEAT_AGENT" "PASEO_WORKSPACE_ID=$SEAT_WORKSPACE" "PASEO_AGENT_CWD=$CLI_REPO")

# Since ASMA-8530 the guard refuses both CLI paths for a labelled seat. A case
# passes only when the command exits non-zero with the guard's own refusal and
# no workspace appears; anything else, including a create, fails the suite.
expect_guard_refusal() {
  local name="$1" tag="$2" code=0
  shift 2
  paseo workspace ls --json --home "$HOME_DIR" >"$HOME_DIR/$tag-before.json"
  "${CLI_ENV[@]}" "$@" --json --home "$HOME_DIR" >"$HOME_DIR/$tag.json" 2>"$HOME_DIR/$tag.stderr" || code=$?
  paseo workspace ls --json --home "$HOME_DIR" >"$HOME_DIR/$tag-after.json"
  # A run that slipped through must not leave its agent behind.
  node -e 'try{const j=require(process.argv[1]);if(j.agentId)console.log(j.agentId)}catch{}' "$HOME_DIR/$tag.json" | while read -r agent; do paseo agent stop "$agent" --home "$HOME_DIR" >/dev/null 2>&1 || true; done
  node -e '
const fs = require("node:fs");
const [name, code, before, after, out, err] = process.argv.slice(1);
const seen = new Set(require(before).map((x) => x.workspaceId));
const added = require(after).filter((x) => !seen.has(x.workspaceId)).map((x) => x.workspaceId);
const text = fs.readFileSync(out, "utf8") + fs.readFileSync(err, "utf8");
const message = (text.match(/Kontor workspace guard refused:[^"\n]*/) || [])[0] ?? null;
const refused = Number(code) !== 0 && added.length === 0 && message !== null;
const outcome = refused ? "refused" : added.length > 0 ? "created" : "failed without the guard refusal";
console.log(JSON.stringify({ name, outcome, exit: Number(code), message, addedWorkspaces: added, expected: "refused by the Kontor workspace guard (ASMA-8530)" }));
if (!refused) { console.error(text.trim()); process.exit(1); }
' "$name" "$code" "$HOME_DIR/$tag-before.json" "$HOME_DIR/$tag-after.json" "$HOME_DIR/$tag.json" "$HOME_DIR/$tag.stderr"
}

expect_guard_refusal "cli workspace create (labelled seat env)" cli-create \
  paseo workspace create --isolation local --path "$CLI_REPO"
expect_guard_refusal "cli run --new-workspace (labelled seat env)" cli-run \
  paseo run --new-workspace local --cwd "$CLI_REPO" --provider opencode/deepseek/deepseek-flash --background "closure check"

echo "== uninstall =="
paseo plugin remove kontor --home "$HOME_DIR" --json >"$HOME_DIR/removed.json"
paseo plugin ls --home "$HOME_DIR" --json >"$HOME_DIR/after-remove.json"
node -e 'const j=require(process.argv[1]);if(j.length!==0)process.exit(1);console.log(JSON.stringify({plugins:j.length}))' "$HOME_DIR/after-remove.json"

echo "== restored behaviour =="
PASEO_CLOSURE_PORT="$PORT" node "$PLUGIN_DIR/scripts/closure.mjs" "$PORT" unguarded | tee "$HOME_DIR/unguarded.json"

echo "== closure passed =="
