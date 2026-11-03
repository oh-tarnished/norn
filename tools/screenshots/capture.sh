#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Srikanth K
# SPDX-License-Identifier: AGPL-3.0-or-later

# Captures docs/images from a real daemon: every image is the actual output of
# the commands it shows, never drawn or typed by hand.
#
# usage: capture.sh TARGET_DIR OUTDIR
#
#   TARGET_DIR  where `norn` and `nornctl` are, such as target/debug
#   OUTDIR      where the SVGs go, such as docs/images
#
# The daemon runs on the default endpoint and data directory, kept in a fresh
# temporary directory that is removed on exit. Takes a little over a minute: a
# stream's sample count moves only when a chunk is stored, up to
# max_chunk_age_ms (one minute by default) after the append.
set -euo pipefail

if [ $# -ne 2 ]; then
    sed -n '5,8p' "$0" >&2
    exit 2
fi
BIN=$(cd "$1" && pwd)
mkdir -p "$2"
OUT=$(cd "$2" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd)
WORK=$(mktemp -d /tmp/norn-shots.XXXXXX)
export PATH="$BIN:$PATH" TMPDIR="$WORK/run" XDG_DATA_HOME="$WORK/data"
unset XDG_RUNTIME_DIR
mkdir -p "$TMPDIR" "$XDG_DATA_HOME"

norn 2>"$WORK/daemon.log" &
DAEMON=$!
trap 'kill "$DAEMON" 2>/dev/null || true; wait "$DAEMON" 2>/dev/null || true; rm -rf "$WORK"' EXIT

for _ in $(seq 100); do
    nornctl status >/dev/null 2>&1 && break
    sleep 0.1
done

# run COMMAND...: one command of the session, echoed as it was typed.
run() {
    printf '$ %s\n' "$*"
    "$@" 2>&1
}

# shot NAME: draws the transcript on stdin as nornctl-NAME.svg.
shot() {
    python3 -I "$HERE/render_svg.py" "nornctl $1" >"$OUT/nornctl-$1.svg"
}

SIGNAL=entities/dev-01/signals/temperature
run nornctl status | shot status
run nornctl version | shot version
{
    run nornctl streams create plant
    run nornctl entities create dev-01
    run nornctl signals create dev-01 temperature plant double
    run nornctl signals
} | shot signals
{
    run nornctl append plant "$SIGNAL" 21.5
    run nornctl append plant "$SIGNAL" 21.75 --ack persisted --sequence 2
    run nornctl append plant "$SIGNAL" 22 --ack buffered --sequence 3
} | shot append
{
    run nornctl query "$SIGNAL"
    run nornctl query "$SIGNAL" --agg average
} | shot query
{
    run nornctl streams
    run sleep 61
    run nornctl streams
} | shot streams
