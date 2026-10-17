#!/usr/bin/env bash
# CI gate: replay the seeds and findings, then fuzz a fixed set of seeds.
# Fails on any failure that matches no filed issue, and on a finding that no longer fails.
set -uo pipefail
ITERS=${FUZZ_ITERS:-12}
SEEDS=${FUZZ_SEEDS:-"1 2 3 4"}
OUT=target/fuzz-ci
cargo build --release --locked -p artfuzz || exit 1
BIN=target/release/artfuzz
rm -rf "$OUT" && mkdir -p "$OUT"
status=0
"$BIN" repro tools/fuzz/seeds/*.art tools/fuzz/findings/*.art || status=1
pids=()
for s in $SEEDS; do
  "$BIN" --seed "$s" --iters "$ITERS" --out "$OUT/s$s" >"$OUT/s$s.log" 2>&1 &
  pids+=($!)
done
for p in "${pids[@]}"; do wait "$p" || status=1; done
for s in $SEEDS; do echo "== seed $s"; cat "$OUT/s$s.log"; done
if [ "$status" != 0 ]; then
  echo "Minimized repros for new failures are in $OUT. Rerun one with: $BIN repro <file>"
fi
exit "$status"
