#!/usr/bin/env bash
# Copy the current reliquary scene render into the timelapse.
# Usage: scripts/snap.sh <label>
set -euo pipefail

label="${1:?label}"
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if [ -x /workspace/target/debug/ramose ]; then
  export PATH="/workspace/target/debug:$PATH"
fi

ramose build --only 'art/scene/reliquary.ramo' --backend cpu

frames="$root/timelapse/frames"
mkdir -p "$frames"
n="$(find "$frames" -maxdepth 1 -name '*.png' | wc -l | tr -d ' ')"
id="$(printf '%02d' "$n")"
out="$frames/${id}-${label}.png"
cp "$root/assets/scene/reliquary.png" "$out"

manifest="$root/timelapse/manifest.tsv"
if [ ! -s "$manifest" ]; then
  printf 'frame\tlabel\tcommit\tfile\n' > "$manifest"
fi
commit="$(git -C "$root" rev-parse --short HEAD 2>/dev/null || printf 'uncommitted')"
printf '%s\t%s\t%s\t%s\n' "$id" "$label" "$commit" "timelapse/frames/${id}-${label}.png" >> "$manifest"
printf '%s\n' "$out"
