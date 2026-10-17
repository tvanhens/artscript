#!/usr/bin/env bash
# CLI-surface properties over generated cases: check agrees with build, a cached rebuild
# replays the same report, and two fresh builds write identical bytes.
# usage: cli_props.sh <artscript bin> <artfuzz bin> <example root> <count> <seed> <workdir>
set -u
BIN=$1; FUZZ=$2; ROOT=$3; N=$4; SEED=$5; W=$6
rm -rf "$W"; mkdir -p "$W"
"$FUZZ" --root "$ROOT" --seed "$SEED" --out "$W/cases" gen "$N"
fails=0
for c in "$W"/cases/*; do
  entry=$(cat "$c/entry")
  for side in a b; do
    rm -rf "$W/$side"; mkdir -p "$W/$side"
    cp -R "$ROOT/artscript.toml" "$ROOT/lib" "$W/$side/"
    mkdir -p "$W/$side/$(dirname "$entry")"
    cp "$c/case.art" "$W/$side/$entry"
  done
  (cd "$W/a" && timeout 300 "$BIN" check >"$W/check.out" 2>&1); ce=$?
  (cd "$W/a" && timeout 300 "$BIN" build --allow-warnings >"$W/a1.out" 2>&1); be=$?
  (cd "$W/a" && timeout 300 "$BIN" build --allow-warnings >"$W/a2.out" 2>&1); be2=$?
  (cd "$W/b" && timeout 300 "$BIN" build --allow-warnings >"$W/b1.out" 2>&1); bbe=$?
  cls() { case $1 in 0|3) echo ok;; 1) echo err;; 2) echo assert;; 124) echo timeout;; *) echo "crash$1";; esac; }
  report() { fails=$((fails+1)); echo "FAIL $(basename "$c") $entry: $1"; cp "$c/case.art" "$W/fail-$(basename "$c").art"; }
  [ "$(cls $ce)" != "$(cls $be)" ] && report "check exit $ce vs build exit $be"
  case $(cls $be) in timeout|crash*) report "build exit $be";; esac
  grep -v '^built\|^removed\|^cached\|in [0-9.]*s' "$W/a1.out" > "$W/a1.d"; grep -v '^built\|^removed\|^cached\|in [0-9.]*s' "$W/a2.out" > "$W/a2.d"
  { [ $be != $be2 ] || ! cmp -s "$W/a1.d" "$W/a2.d"; } && report "cached rebuild differs (exit $be vs $be2)"
  if [ -d "$W/a/assets" ]; then
    (cd "$W/a/assets" && find . -type f ! -name '*.json' -exec shasum {} + | sort) > "$W/ha"
    (cd "$W/b/assets" 2>/dev/null && find . -type f ! -name '*.json' -exec shasum {} + | sort) > "$W/hb"
    cmp -s "$W/ha" "$W/hb" || report "two fresh builds wrote different bytes"
  fi
done
echo "$fails failing of $N"
