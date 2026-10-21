#!/bin/sh
# Rotate one launchd-written log without restarting its daemon.
#
# launchd keeps the StandardOutPath file open, so a rename alone would leave the
# daemon writing to the unlinked inode. This is the copy-truncate form: copy the
# bytes aside, then truncate the same inode in place. The daemon reopens nothing
# and keeps appending at the new end.
#
# Usage: kontor-log-rotate.sh <log> [keep]
#
# The agent runs this once a day and keeps seven rotated files. The current log
# is left in place; `keep` bounds only the numbered copies.
set -eu

log=${1:?the log file is required}
keep=${2:-7}

case "$keep" in
    ''|*[!0-9]*) echo "kontor-log-rotate: keep must be a positive integer" >&2; exit 2 ;;
esac

[ -f "$log" ] || exit 0
[ -s "$log" ] || exit 0

# Drop the oldest copy first, then shift the rest up, so a rotation never
# addresses a name it is about to overwrite.
rm -f "$log.$keep"
i=$((keep - 1))
while [ "$i" -ge 1 ]; do
    if [ -f "$log.$i" ]; then
        mv -f "$log.$i" "$log.$((i + 1))"
    fi
    i=$((i - 1))
done

# Copy first and truncate second. A crash between the two leaves a complete
# rotated copy and an unrotated live log, never an empty copy and a truncated
# original.
cp -p "$log" "$log.1"
: > "$log"

echo "kontor-log-rotate: rotated $log (kept $keep copies)"
