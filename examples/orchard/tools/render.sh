#!/bin/sh
set -e
cd "$(dirname "$0")/.."
bin="${RAMOSE:-/workspace/target/debug/ramose}"
err=0
"$bin" check || err=$?
"$bin" build --only 'art/beast/ash_hart.ramo' || err=$?
png="assets/beast/ash_hart.png"
bytes=0
if [ -f "$png" ]; then
  bytes=$(wc -c < "$png" | tr -d ' ')
fi
printf 'errors %s\n' "$err"
printf 'work 1\n'
printf 'bytes %s\n' "$bytes"
printf 'png %s\n' "$png"
exit "$err"
