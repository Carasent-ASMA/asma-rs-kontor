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

echo "== guarded behaviour =="
PASEO_CLOSURE_PORT="$PORT" node "$PLUGIN_DIR/scripts/closure.mjs" "$PORT" guarded | tee "$HOME_DIR/guarded.json"

echo "== cli residual (labelled seat environment) =="
CLI_REPO="$HOME_DIR/cli-fixture"
mkdir -p "$CLI_REPO"
git -C "$CLI_REPO" init -q
git -C "$CLI_REPO" -c user.name=closure -c user.email=closure@example.com commit -q --allow-empty -m init
SEAT_JSON="$(PASEO_CLOSURE_PORT="$PORT" node "$PLUGIN_DIR/scripts/cli-seat.mjs" "$PORT" "$CLI_REPO")"
SEAT_AGENT="$(node -e 'console.log(JSON.parse(process.argv[1]).agentId)' "$SEAT_JSON")"
SEAT_WORKSPACE="$(node -e 'console.log(JSON.parse(process.argv[1]).workspaceId)' "$SEAT_JSON")"
CLI_ENV=(env "PASEO_AGENT_ID=$SEAT_AGENT" "PASEO_WORKSPACE_ID=$SEAT_WORKSPACE" "PASEO_AGENT_CWD=$CLI_REPO")

"${CLI_ENV[@]}" paseo workspace create --isolation local --path "$CLI_REPO" --json --home "$HOME_DIR" >"$HOME_DIR/cli-created.json"
node -e 'const j=require(process.argv[1]);if(!j.workspaceId)process.exit(1);console.log(JSON.stringify({name:"cli workspace create (labelled seat env)",outcome:"created",workspaceId:j.workspaceId,must_be:"refused after ASMA-8530"}))' "$HOME_DIR/cli-created.json"

paseo workspace ls --json --home "$HOME_DIR" >"$HOME_DIR/ws-before.json"
"${CLI_ENV[@]}" paseo run --new-workspace local --cwd "$CLI_REPO" --provider opencode/deepseek/deepseek-flash --background "closure check" --json --home "$HOME_DIR" >"$HOME_DIR/cli-run.json" 2>"$HOME_DIR/cli-run.stderr"
paseo workspace ls --json --home "$HOME_DIR" >"$HOME_DIR/ws-after.json"
node -e 'const b=require(process.argv[1]);const a=require(process.argv[2]);const seen=new Set(b.map(x=>x.workspaceId));const added=a.filter(x=>!seen.has(x.workspaceId));if(added.length!==1)process.exit(1);console.log(JSON.stringify({name:"cli run --new-workspace (labelled seat env)",outcome:"created",workspaceId:added[0].workspaceId,must_be:"refused after ASMA-8530"}))' "$HOME_DIR/ws-before.json" "$HOME_DIR/ws-after.json"
"${CLI_ENV[@]}" node -e 'const j=require(process.argv[1]);if(j.agentId)console.log(j.agentId)' "$HOME_DIR/cli-run.json" | while read -r run_agent; do paseo agent stop "$run_agent" --home "$HOME_DIR" >/dev/null 2>&1 || true; done

echo "== uninstall =="
paseo plugin remove kontor --home "$HOME_DIR" --json >"$HOME_DIR/removed.json"
paseo plugin ls --home "$HOME_DIR" --json >"$HOME_DIR/after-remove.json"
node -e 'const j=require(process.argv[1]);if(j.length!==0)process.exit(1);console.log(JSON.stringify({plugins:j.length}))' "$HOME_DIR/after-remove.json"

echo "== restored behaviour =="
PASEO_CLOSURE_PORT="$PORT" node "$PLUGIN_DIR/scripts/closure.mjs" "$PORT" unguarded | tee "$HOME_DIR/unguarded.json"

echo "== closure passed =="
