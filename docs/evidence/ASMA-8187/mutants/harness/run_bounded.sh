#!/bin/zsh
# Run one focused test under a hard wall-clock limit, so a coordination defect
# surfaces as a timeout with evidence rather than an indefinite hang.
LIMIT=${LIMIT:-300}
LOG="$1"; shift
cd /Users/igor/carasent/asma-modules/.worktrees/feat/ASMA-8187-repair-admin-preview-apply-stale-native-core-team-succession/_tools/asma-rs-kontor
: > "$LOG"
echo "command: cargo $@" >> "$LOG"
echo "hard limit: ${LIMIT}s" >> "$LOG"
cargo "$@" >> "$LOG" 2>&1 &
PID=$!
for i in $(seq 1 $LIMIT); do
  kill -0 $PID 2>/dev/null || break
  sleep 1
done
if kill -0 $PID 2>/dev/null; then
  echo "TIMEOUT after ${LIMIT}s -- killing $PID" >> "$LOG"
  pkill -KILL -P $PID 2>/dev/null
  kill -KILL $PID 2>/dev/null
  wait $PID 2>/dev/null
  echo "verdict: TIMEOUT (inconclusive)" >> "$LOG"
  exit 124
fi
wait $PID; STATUS=$?
echo "exit status: $STATUS" >> "$LOG"
exit $STATUS
