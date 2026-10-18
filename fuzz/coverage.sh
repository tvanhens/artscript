#!/usr/bin/env bash
# Replays each target's corpus under source coverage and prints the union line and region coverage
# of the artscript crates. `fuzz/coverage.sh show <file-substring>` prints that file's uncovered lines.
set -euo pipefail
cd "$(dirname "$0")"
export ASAN_OPTIONS="${ASAN_OPTIONS:+$ASAN_OPTIONS:}detect_leaks=1"
export LSAN_OPTIONS="${LSAN_OPTIONS:+$LSAN_OPTIONS:}suppressions=$PWD/lsan.supp"
targets=(parse format manifest eval render fmt_eval gen)
triple=$(rustc +nightly -vV | sed -n 's/^host: //p')
bin=$(dirname "$(rustc +nightly --print target-libdir)")/bin
objs=()
for t in "${targets[@]}"; do objs+=(-object "target/$triple/coverage/$triple/release/$t"); done
objs=("${objs[@]:1}")
mkdir -p logs
if [ "${1:-}" != show ]; then
  for t in "${targets[@]}"; do
    cargo +nightly fuzz coverage "$t" -- -rss_limit_mb=4096 -timeout=0 >"logs/coverage-$t.log" 2>&1 || {
      echo "coverage replay of $t failed; see fuzz/logs/coverage-$t.log" >&2
      exit 1
    }
  done
  "$bin/llvm-profdata" merge -sparse coverage/*/coverage.profdata -o coverage/union.profdata
fi
filter=(-ignore-filename-regex='(\.cargo|rustc|fuzz/)')
if [ "${1:-}" = show ]; then
  "$bin/llvm-cov" show "${objs[@]}" -instr-profile=coverage/union.profdata "${filter[@]}" \
    "$(cd .. && pwd)/$2" 2>/dev/null | grep -E '^ +[0-9]+\| +0\|' || true
else
  "$bin/llvm-cov" report "${objs[@]}" -instr-profile=coverage/union.profdata "${filter[@]}" \
    | sed "s|$(cd .. && pwd)/||"
fi
