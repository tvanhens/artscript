#!/bin/sh
set -eu
cd "$(dirname "$0")"
if [ -n "${ARTSCRIPT:-}" ]; then
  bin="$ARTSCRIPT"
elif [ -x ../../target/debug/artscript ]; then
  bin=../../target/debug/artscript
else
  bin=artscript
fi
"$bin" check
"$bin" build --only 'art/scene/rain_span.art'
