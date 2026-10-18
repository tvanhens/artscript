#!/usr/bin/env bash
# Runs every fuzz target in parallel for SECS seconds (default 300) and prints each target's final
# coverage. Usage: fuzz/campaign.sh [SECS] [TARGET...]
set -uo pipefail
cd "$(dirname "$0")"
export ASAN_OPTIONS="${ASAN_OPTIONS:+$ASAN_OPTIONS:}detect_leaks=1"
export LSAN_OPTIONS="${LSAN_OPTIONS:+$LSAN_OPTIONS:}suppressions=$PWD/lsan.supp"
secs=${1:-300}
shift || true
targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(parse format manifest eval render fmt_eval gen)
mkdir -p logs
cargo +nightly fuzz build >/dev/null 2>&1 || cargo +nightly fuzz build
for t in "${targets[@]}"; do
  mkdir -p "corpus/$t"
  extra=(-dict=art.dict)
  [ "$t" = gen ] && extra=(-max_len=16384)
  cargo +nightly fuzz run "$t" -- "${extra[@]}" -max_total_time="$secs" -timeout=30 \
    -rss_limit_mb=4096 -fork="${FORKS:-2}" -ignore_crashes=1 -ignore_timeouts=1 -ignore_ooms=1 >"logs/$t.log" 2>&1 &
done
wait
for t in "${targets[@]}"; do
  last=$(grep -E '^#[0-9]+' "logs/$t.log" | tail -1)
  printf '%-9s %s\n' "$t" "${last:-no stats}"
done
./triage.sh "${targets[@]}"
