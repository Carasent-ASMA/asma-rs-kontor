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

echo "== uninstall =="
paseo plugin remove kontor --home "$HOME_DIR" --json >"$HOME_DIR/removed.json"
paseo plugin ls --home "$HOME_DIR" --json >"$HOME_DIR/after-remove.json"
node -e 'const j=require(process.argv[1]);if(j.length!==0)process.exit(1);console.log(JSON.stringify({plugins:j.length}))' "$HOME_DIR/after-remove.json"

echo "== restored behaviour =="
PASEO_CLOSURE_PORT="$PORT" node "$PLUGIN_DIR/scripts/closure.mjs" "$PORT" unguarded | tee "$HOME_DIR/unguarded.json"

echo "== closure passed =="
