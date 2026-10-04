#!/bin/zsh
# One recorded mutation run.
#
# Restoration is unconditional (EXIT trap): a killed mutant exits non-zero, and
# an earlier version let `set -e` abort at `wait` *before* the restore, leaving
# the next run to snapshot an already-mutated file. Evidence that corrupts the
# next measurement is worse than no evidence.
#
# Patches are mutation-only against a named snapshot (never `git diff`, which
# sweeps in uncommitted work), verified by git apply --check and patch
# --dry-run, and every run is wall-clock bounded so a coordination defect
# reports a timeout rather than hanging.
# Portable: the repo root is derived from this script's own location, so the
# reproduce line each log records runs from a fresh clone without editing.
K=$(cd "${0:a:h}/../../../../.." && pwd)
cd "$K" || exit 2
ID="$1"; PATCH="$2"; TARGET="$3"
HERE=docs/evidence/ASMA-8187/mutants/harness
REPRO="LIMIT=${LIMIT:-420} $HERE/mutate.sh $1 ${2/#*\//$HERE/} $3 ${@:4}"
shift 3
M=docs/evidence/ASMA-8187/mutants
W=${MUTATE_WORKDIR:-$(mktemp -d -t asma8187)}
LIMIT=${LIMIT:-420}
BASE=$(git rev-parse HEAD)

cp "$TARGET" "$W/$ID.snapshot" || exit 2
PRE=$(shasum -a 256 "$TARGET" | cut -d' ' -f1)
restore() { cp "$W/$ID.snapshot" "$K/$TARGET"; }
trap restore EXIT INT TERM

python3 "$PATCH" > "$W/$ID.apply.txt" 2>&1 || { echo "$ID: patch script failed"; exit 2; }
MUT=$(shasum -a 256 "$TARGET" | cut -d' ' -f1)
cp "$TARGET" "$W/$ID.mutated"
diff -u --label "a/$TARGET" --label "b/$TARGET" "$W/$ID.snapshot" "$W/$ID.mutated" > "$M/$ID.patch"
cp "$W/$ID.snapshot" "$TARGET"
APPLY=$(git apply --check "$M/$ID.patch" 2>&1 && echo accepted || echo REJECTED)
PATCHDR=$(patch -p1 --dry-run --forward < "$M/$ID.patch" 2>&1 | head -1)
cp "$W/$ID.mutated" "$TARGET"

{
  echo "=== mutant $ID (corrected baseline) ==="
  echo "baseline commit   : $BASE"
  echo "mutated file      : $TARGET"
  echo "snapshot sha256   : $PRE"
  echo "mutated  sha256   : $MUT"
  echo "patch             : $ID.patch (mutation-only, $(wc -l < "$M/$ID.patch" | tr -d ' ') lines)"
  echo "  git apply --check: $APPLY"
  echo "  patch --dry-run  : $PATCHDR"
  echo "test command      : cargo $@"
  echo "reproduce (exact) : $REPRO"
  echo "limiter           : this harness bounds the run at ${LIMIT}s wall clock and"
  echo "                    reports 124 = timeout, inconclusive, never a kill."
  echo "toolchain         : $(rustc --version)"
  echo "platform          : $(uname -srm)"
  echo "--- raw output ---"
} > "$M/$ID.log"

cargo "$@" >> "$M/$ID.log" 2>&1 &
PID=$!
for i in $(seq 1 $LIMIT); do kill -0 $PID 2>/dev/null || break; sleep 1; done
if kill -0 $PID 2>/dev/null; then
  pkill -KILL -P $PID 2>/dev/null; kill -KILL $PID 2>/dev/null; wait $PID 2>/dev/null
  STATUS=124
  echo "TIMEOUT after ${LIMIT}s -- inconclusive, not a kill" >> "$M/$ID.log"
else
  wait $PID
  STATUS=$?
fi

restore
RES=$(shasum -a 256 "$TARGET" | cut -d' ' -f1)
{
  echo "--- exit status: $STATUS (non-zero = killed; 124 = timeout, inconclusive) ---"
  echo "restored sha256   : $RES"
  if [ "$RES" = "$PRE" ]; then echo "restoration       : byte-identical to the named snapshot"
  else echo "restoration       : MISMATCH"; fi
  echo -n "MUTANT markers    : "; grep -c MUTANT "$TARGET" | tr -d ' '
} >> "$M/$ID.log"
echo "$ID: exit $STATUS"
exit 0
