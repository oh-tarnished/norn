#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Srikanth K
# SPDX-License-Identifier: AGPL-3.0-or-later

# Export and import rate, file size and client peak RSS per format, on a real
# daemon over a Unix socket. docs/performance.md records what this printed.
#
# usage: tools/interchange/bench.sh [SIGNALS] [SECONDS]
# Build first: cargo build --release -p norn-server -p norn-cli
set -euo pipefail

signals=${1:-100}
seconds=${2:-10000}
bin=target/release
work=$(mktemp -d -t ni)
pid=
trap 'if [ -n "$pid" ]; then kill "$pid" 2>/dev/null; fi; rm -rf "$work"' EXIT

start() { # DIRECTORY
    mkdir -p "$1"
    "$bin/norn" --listen "unix:$1/s/n.sock" --data-dir "$1/data" 2>"$1/log" &
    pid=$!
    for _ in $(seq 1 500); do
        if "$bin/nornctl" --endpoint "unix:$1/s/n.sock" status >/dev/null 2>&1; then return; fi
        sleep 0.01
    done
    echo "the daemon did not start: $(cat "$1/log")" >&2
    exit 1
}

stop() {
    kill "$pid"
    wait "$pid" || true
    pid=
}

# Prints seconds and peak RSS in MiB for one nornctl run; its stdout goes to $work/out.
timed() { # DIRECTORY, ARGUMENTS...
    local dir=$1
    shift
    if [ "$(uname)" = Darwin ]; then
        /usr/bin/time -l "$bin/nornctl" --endpoint "unix:$dir/s/n.sock" --timeout 60 "$@" \
            >"$work/out" 2>"$work/time"
        awk '/real/ { s = $1 } /maximum resident/ { r = $1 / 1048576 } END { printf "%s %.0f\n", s, r }' "$work/time"
    else
        /usr/bin/time -f '%e %M' "$bin/nornctl" --endpoint "unix:$dir/s/n.sock" --timeout 60 "$@" \
            >"$work/out" 2>"$work/time"
        awk '{ printf "%s %.0f\n", $1, $2 / 1024 }' "$work/time"
    fi
}

rate() { awk -v n="$1" -v s="$2" 'BEGIN { printf "%.0f", n / s }'; }

python3 tools/interchange/generate.py "$signals" "$seconds" >"$work/plain.csv"
total=$((signals * seconds))
echo "samples: $total  plain csv: $(wc -c <"$work/plain.csv") bytes"

start "$work/a"
read -r s r < <(timed "$work/a" import "$work/plain.csv" --ack persisted)
echo "fill: ${s}s, $(rate "$total" "$s") samples/s, ${r} MiB; $(cat "$work/out")"

echo "format  bytes  bytes/sample  export/s  export_MiB  import/s  import_MiB  repeat/s"
for format in csv parquet arrow mcap; do
    file="$work/out.$format"
    start_dir="$work/a"
    read -r es er < <(timed "$start_dir" export all --format "$format" --output "$file")
    size=$(wc -c <"$file")
    stop
    start "$work/b"
    read -r is ir < <(timed "$work/b" import "$file" --ack persisted)
    grep -q "read=$total sent=$total" "$work/out" || { cat "$work/out"; exit 1; }
    read -r rs _ < <(timed "$work/b" import "$file" --ack persisted)
    grep -q "read=$total sent=0 present=$total" "$work/out" || { cat "$work/out"; exit 1; }
    stop
    rm -rf "$work/b"
    start "$work/a"
    printf '%s  %s  %s  %s  %s  %s  %s  %s\n' "$format" "$size" \
        "$(awk -v a="$size" -v b="$total" 'BEGIN { printf "%.2f", a / b }')" \
        "$(rate "$total" "$es")" "$er" "$(rate "$total" "$is")" "$ir" "$(rate "$total" "$rs")"
done
stop
