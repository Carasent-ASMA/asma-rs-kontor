#!/bin/sh
# Install the two KR-07 host agents, and only those two.
#
# This script never addresses com.asma.kontor.daemon. It renders the templates
# in operations/launchd with one explicit home directory, validates each plist,
# loads it into that user's gui domain and prints the loaded job back.
#
# Usage:
#   install-launchd-agents.sh --home "$HOME" [--uid "$(id -u)"]
#   install-launchd-agents.sh --home "$HOME" --remove
set -eu

here=$(cd "$(dirname "$0")" && pwd)
templates=$here/../launchd
home=
uid=$(id -u)
remove=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --home) home=${2:?--home needs a value}; shift 2 ;;
        --uid) uid=${2:?--uid needs a value}; shift 2 ;;
        --remove) remove=1; shift ;;
        *) echo "install-launchd-agents: unknown argument $1" >&2; exit 2 ;;
    esac
done

[ -n "$home" ] || { echo "install-launchd-agents: --home is required" >&2; exit 2; }
case "$home" in
    /*) ;;
    *) echo "install-launchd-agents: --home must be absolute" >&2; exit 2 ;;
esac
[ -d "$home" ] || { echo "install-launchd-agents: $home is not a directory" >&2; exit 2; }

agents=$home/Library/LaunchAgents
libexec=$home/.local/libexec/kontor
labels="com.asma.kontor.snapshot com.asma.kontor.logrotate"

if [ "$remove" -eq 1 ]; then
    for label in $labels; do
        if launchctl print "gui/$uid/$label" >/dev/null 2>&1; then
            launchctl bootout "gui/$uid/$label"
        fi
        rm -f "$agents/$label.plist"
    done
    echo "removed the KR-07 agents from gui/$uid"
    exit 0
fi

mkdir -p "$agents" "$libexec"
install -m 0755 "$here/kontor-log-rotate.sh" "$libexec/kontor-log-rotate.sh"

for label in $labels; do
    source=$templates/$label.plist.in
    target=$agents/$label.plist
    [ -f "$source" ] || { echo "install-launchd-agents: $source is missing" >&2; exit 1; }
    sed "s|@HOME@|$home|g" "$source" > "$target.rendering"
    if grep -q '@HOME@' "$target.rendering"; then
        rm -f "$target.rendering"
        echo "install-launchd-agents: $label still carries @HOME@" >&2
        exit 1
    fi
    plutil -lint "$target.rendering" >/dev/null
    mv "$target.rendering" "$target"
    chmod 0644 "$target"
done

for label in $labels; do
    if launchctl print "gui/$uid/$label" >/dev/null 2>&1; then
        launchctl bootout "gui/$uid/$label"
    fi
    launchctl bootstrap "gui/$uid" "$agents/$label.plist"
    echo "loaded gui/$uid/$label"
    launchctl print "gui/$uid/$label" | sed -n '1,12p'
done
