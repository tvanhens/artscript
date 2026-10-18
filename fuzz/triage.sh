#!/usr/bin/env bash
# Replays every saved artifact and groups them by failure site, one line per distinct failure with
# its smallest reproducer. Usage: fuzz/triage.sh [TARGET...]
set -uo pipefail
cd "$(dirname "$0")"
triple=$(rustc +nightly -vV | sed -n 's/^host: //p')
targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(parse format manifest eval render fmt_eval gen)
for t in "${targets[@]}"; do
  for a in artifacts/"$t"/*; do
    [ -f "$a" ] || continue
    out=$(RUST_BACKTRACE=1 timeout 60 "target/$triple/release/$t" -timeout=30 -rss_limit_mb=4096 "$a" 2>&1)
    site=$(grep -m1 -oE 'panicked at [^ ]+|ERROR: libFuzzer: [a-z-]+' <<<"$out" | sed "s|$(cd .. && pwd)/||")
    caller=$(grep -oE 'at [^ ]*/crates/[^ ]+' <<<"$out" | sed -n 2p | sed 's|.*/crates/|crates/|')
    [ -n "$caller" ] && site="$site <- $caller"
    [ -z "$site" ] && site=$(grep -m1 -oE 'deadly signal|AddressSanitizer: [a-z-]+' <<<"$out")
    printf '%s\t%s\t%s\t%s\n' "$t" "${site:-passes now}" "$(wc -c <"$a" | tr -d ' ')" "$a"
  done
done | sort -t$'\t' -k1,2 -k3n | awk -F'\t' '!seen[$1 FS $2]++ { n[$1 FS $2]=0 } { n[$1 FS $2]++; if (!($1 FS $2 in first)) first[$1 FS $2]=$4 } END { for (k in n) { split(k, p, FS); printf "%-9s %4d  %-60s %s\n", p[1], n[k], p[2], first[k] } }' | sort
